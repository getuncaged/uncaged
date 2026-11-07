use std::path::PathBuf;

use uuid::Uuid;
use warp_core::execution_mode::ExecutionMode;
use warp_util::path::EscapeChar;
use warpui::{App, EntityId, ModelHandle};

use super::{BlocklistAIHistoryModel, BlocklistAIPermissions};
use crate::ai::active_agent_views_model::ActiveAgentViewsModel;
use crate::ai::agent::conversation::AIConversationId;
use crate::ai::blocklist::permissions::{
    CommandExecutionPermission, CommandExecutionPermissionAllowedReason,
    CommandExecutionPermissionDeniedReason, FileReadPermission, FileReadPermissionAllowedReason,
    FileWritePermission, FileWritePermissionAllowedReason, FileWritePermissionDeniedReason,
};
use crate::ai::execution_profiles::profiles::AIExecutionProfilesModel;
use crate::ai::execution_profiles::ActionPermission;
use crate::ai::mcp::templatable_manager::TemplatableMCPServerManager;
use crate::cloud_object::model::persistence::CloudModel;
use crate::network::NetworkStatus;
use crate::server::cloud_objects::update_manager::UpdateManager;
use crate::server::sync_queue::SyncQueue;
use crate::settings::{AgentModeCommandExecutionPredicate, PrivacySettings};
use crate::terminal::cli_agent_sessions::CLIAgentSessionsModel;
use crate::test_util::settings::initialize_settings_for_tests_with_mode;
use crate::{AgentNotificationsModel, GlobalResourceHandles, GlobalResourceHandlesProvider};

struct PermissionsTestState {
    convo_id: AIConversationId,
    permissions: ModelHandle<BlocklistAIPermissions>,
    history: ModelHandle<BlocklistAIHistoryModel>,
    terminal_view_id: EntityId,
    profile_model: ModelHandle<AIExecutionProfilesModel>,
}

fn initialize_permissions_test(app: &mut App) -> PermissionsTestState {
    initialize_permissions_test_with_mode(app, ExecutionMode::App, false)
}

fn initialize_permissions_test_with_mode(
    app: &mut App,
    mode: ExecutionMode,
    is_sandboxed: bool,
) -> PermissionsTestState {
    initialize_settings_for_tests_with_mode(app, mode, is_sandboxed);
    let global_resource_handles = GlobalResourceHandles::mock(app);
    app.add_singleton_model(|_| GlobalResourceHandlesProvider::new(global_resource_handles));
    let history = app.add_singleton_model(|_| BlocklistAIHistoryModel::new(vec![], &[]));
    app.add_singleton_model(|_| CLIAgentSessionsModel::new());
    app.add_singleton_model(|_| ActiveAgentViewsModel::new());
    app.add_singleton_model(AgentNotificationsModel::new);
    let permissions = app.add_singleton_model(BlocklistAIPermissions::new);
    let terminal_view_id = EntityId::new();
    app.add_singleton_model(SyncQueue::mock);
    app.add_singleton_model(|_| NetworkStatus::new());
    app.add_singleton_model(|_| UpdateManager::new(None));
    app.add_singleton_model(CloudModel::mock);
    app.add_singleton_model(|_| TemplatableMCPServerManager::default());
    let profile_model = app.add_singleton_model(AIExecutionProfilesModel::new);
    app.add_singleton_model(PrivacySettings::mock);

    let conversation_id = history.update(app, |history_model, ctx| {
        history_model.start_new_conversation(terminal_view_id, false, false, false, ctx)
    });

    PermissionsTestState {
        convo_id: conversation_id,
        permissions,
        history,
        terminal_view_id,
        profile_model,
    }
}

#[test]
fn test_can_read_files_empty_paths() {
    App::test((), |mut app| async move {
        let PermissionsTestState {
            convo_id,
            permissions,
            terminal_view_id,
            ..
        } = initialize_permissions_test(&mut app);

        permissions.read(&app, |model, ctx| {
            let result = model.can_read_files_with_conversation(
                &convo_id,
                vec![],
                Some(terminal_view_id),
                ctx,
            );
            assert!(result.is_allowed());
            assert!(matches!(
                result,
                FileReadPermission::Allowed(FileReadPermissionAllowedReason::ExplicitlyAllowlisted)
            ));
        });
    })
}

#[test]
fn test_can_write_files() {
    App::test((), |mut app| async move {
        let PermissionsTestState {
            terminal_view_id,
            convo_id,
            permissions,
            profile_model,
            ..
        } = initialize_permissions_test(&mut app);

        // Test AgentDecides setting
        profile_model.update(&mut app, |model, ctx| {
            model.set_apply_code_diffs(
                *model.active_profile(Some(terminal_view_id), ctx).id(),
                &ActionPermission::AgentDecides,
                ctx,
            );
        });

        permissions.read(&app, |model, ctx| {
            let result = model.can_write_files(&convo_id, &[], Some(terminal_view_id), ctx);
            assert!(!result.is_allowed());
            assert!(
                matches!(
                    result,
                    FileWritePermission::Denied(FileWritePermissionDeniedReason::AgentDecided)
                ),
                "not allowed because AgentDecides right now just means ask"
            );
        });

        // Test AlwaysAllow setting
        profile_model.update(&mut app, |model, ctx| {
            model.set_apply_code_diffs(
                *model.active_profile(Some(terminal_view_id), ctx).id(),
                &ActionPermission::AlwaysAllow,
                ctx,
            );
        });

        permissions.read(&app, |model, ctx| {
            let result = model.can_write_files(&convo_id, &[], Some(terminal_view_id), ctx);
            assert!(result.is_allowed());
            assert!(matches!(
                result,
                FileWritePermission::Allowed(
                    FileWritePermissionAllowedReason::AutowriteSettingEnabled
                )
            ));
        });

        // Test AlwaysAsk setting
        profile_model.update(&mut app, |model, ctx| {
            model.set_apply_code_diffs(
                *model.active_profile(Some(terminal_view_id), ctx).id(),
                &ActionPermission::AlwaysAsk,
                ctx,
            );
        });

        permissions.read(&app, |model, ctx| {
            let result = model.can_write_files(&convo_id, &[], Some(terminal_view_id), ctx);
            assert!(!result.is_allowed());
            assert!(matches!(
                result,
                FileWritePermission::Denied(FileWritePermissionDeniedReason::AlwaysAskEnabled)
            ));
        });
    })
}

#[test]
fn test_can_write_files_mcp_config_always_denied() {
    App::test((), |mut app| async move {
        let PermissionsTestState {
            terminal_view_id,
            convo_id,
            permissions,
            profile_model,
            ..
        } = initialize_permissions_test(&mut app);

        // Even with AlwaysAllow, writing to an MCP config must be denied.
        profile_model.update(&mut app, |model, ctx| {
            model.set_apply_code_diffs(
                *model.active_profile(Some(terminal_view_id), ctx).id(),
                &ActionPermission::AlwaysAllow,
                ctx,
            );
        });

        let mcp_config_paths = vec![
            PathBuf::from("/project/.mcp.json"),
            PathBuf::from("/project/.warp/.mcp.json"),
            PathBuf::from("/project/.codex/config.toml"),
        ];

        for path in mcp_config_paths {
            permissions.read(&app, |model, ctx| {
                let result = model.can_write_files(
                    &convo_id,
                    std::slice::from_ref(&path),
                    Some(terminal_view_id),
                    ctx,
                );
                assert!(
                    !result.is_allowed(),
                    "expected MCP config path {path:?} to be denied"
                );
                assert!(
                    matches!(
                        result,
                        FileWritePermission::Denied(FileWritePermissionDeniedReason::ProtectedPath)
                    ),
                    "expected ProtectedPath denial for {path:?}, got {result:?}"
                );
            });
        }
    })
}

#[test]
fn test_can_autoexecute_command_denylist_matches_env_prefixed_commands() {
    App::test((), |mut app| async move {
        let PermissionsTestState {
            convo_id,
            permissions,
            profile_model,
            terminal_view_id,
            ..
        } = initialize_permissions_test(&mut app);

        profile_model.update(&mut app, |model, ctx| {
            let profile_id = *model.active_profile(Some(terminal_view_id), ctx).id();
            model.set_execute_commands(profile_id, &ActionPermission::AlwaysAllow, ctx);
            model.add_to_command_denylist(
                profile_id,
                &AgentModeCommandExecutionPredicate::new_regex("rm .*").unwrap(),
                ctx,
            );
        });

        for command in [
            "X=1 rm file.txt",
            "echo ok && X=1 rm file.txt",
            "echo $(X=1 rm file.txt)",
        ] {
            permissions.read(&app, |model, ctx| {
                let result = model.can_autoexecute_command(
                    &convo_id,
                    command,
                    EscapeChar::Backslash,
                    false,
                    None,
                    Some(terminal_view_id),
                    ctx,
                );
                assert!(
                    matches!(
                        result,
                        CommandExecutionPermission::Denied(
                            CommandExecutionPermissionDeniedReason::ExplicitlyDenylisted
                        )
                    ),
                    "{command:?} should be denied by the rm denylist, got {result:?}"
                );
            });
        }

        permissions.read(&app, |model, ctx| {
            let result = model.can_autoexecute_command(
                &convo_id,
                "X=1 git status",
                EscapeChar::Backslash,
                false,
                None,
                Some(terminal_view_id),
                ctx,
            );
            assert!(matches!(
                result,
                CommandExecutionPermission::Allowed(
                    CommandExecutionPermissionAllowedReason::AlwaysAllowed
                )
            ));
        });
    })
}

#[test]
fn test_can_autoexecute_command_denylist_beats_run_to_completion() {
    App::test((), |mut app| async move {
        let PermissionsTestState {
            convo_id,
            permissions,
            history,
            profile_model,
            terminal_view_id,
            ..
        } = initialize_permissions_test(&mut app);

        // Add a denylist rule that matches the test command.
        profile_model.update(&mut app, |model, ctx| {
            model.add_to_command_denylist(
                *model.active_profile(Some(terminal_view_id), ctx).id(),
                &AgentModeCommandExecutionPredicate::new_regex("rm .*").unwrap(),
                ctx,
            );
        });

        // Toggle run-to-completion override for this conversation.
        history.update(&mut app, |history, ctx| {
            history.toggle_autoexecute_override(&convo_id, terminal_view_id, ctx);
        });

        // Despite run-to-completion, denylist must take precedence and deny execution.
        permissions.read(&app, |model, ctx| {
            let result = model.can_autoexecute_command(
                &convo_id,
                "rm important.txt",
                EscapeChar::Backslash,
                false,
                None,
                Some(terminal_view_id),
                ctx,
            );
            assert!(!result.is_allowed());
            assert!(matches!(
                result,
                CommandExecutionPermission::Denied(
                    CommandExecutionPermissionDeniedReason::ExplicitlyDenylisted
                )
            ));
        });
    })
}

#[test]
fn test_can_autoexecute_command_run_to_completion_allows_non_denylisted() {
    App::test((), |mut app| async move {
        let PermissionsTestState {
            convo_id,
            permissions,
            history,
            terminal_view_id,
            ..
        } = initialize_permissions_test(&mut app);

        // Enable run-to-completion override for the conversation.
        history.update(&mut app, |history, ctx| {
            history.toggle_autoexecute_override(&convo_id, terminal_view_id, ctx);
        });

        // Since the command is not denylisted, the override should allow execution with RunToCompletion.
        permissions.read(&app, |model, ctx| {
            let result = model.can_autoexecute_command(
                &convo_id,
                "echo hello",
                EscapeChar::Backslash,
                true,        // read-only command
                Some(false), // not risky
                Some(terminal_view_id),
                ctx,
            );
            assert!(result.is_allowed());
            assert!(matches!(
                result,
                CommandExecutionPermission::Allowed(
                    CommandExecutionPermissionAllowedReason::RunToCompletion
                )
            ));
        });
    })
}

#[test]
fn test_can_use_mcp_server_always_allow_no_denylist() {
    App::test((), |mut app| async move {
        let PermissionsTestState {
            convo_id,
            permissions,
            profile_model,
            terminal_view_id,
            ..
        } = initialize_permissions_test(&mut app);

        let server_uuid = Uuid::new_v4();

        profile_model.update(&mut app, |model, ctx| {
            model.set_mcp_permissions(
                *model.active_profile(Some(terminal_view_id), ctx).id(),
                &ActionPermission::AlwaysAllow,
                ctx,
            );
        });

        permissions.read(&app, |model, ctx| {
            // Any server should be allowed when AlwaysAllow and not denylisted.
            assert!(model.can_use_mcp_server(
                &convo_id,
                Some(server_uuid),
                Some(terminal_view_id),
                ctx
            ));
            // None UUID should also be allowed (no denylist match possible).
            assert!(model.can_use_mcp_server(&convo_id, None, Some(terminal_view_id), ctx));
        });
    })
}

#[test]
fn test_can_use_mcp_server_always_allow_with_denylist() {
    App::test((), |mut app| async move {
        let PermissionsTestState {
            convo_id,
            permissions,
            profile_model,
            terminal_view_id,
            ..
        } = initialize_permissions_test(&mut app);

        let server_uuid = Uuid::new_v4();
        let other_uuid = Uuid::new_v4();

        profile_model.update(&mut app, |model, ctx| {
            model.set_mcp_permissions(
                *model.active_profile(Some(terminal_view_id), ctx).id(),
                &ActionPermission::AlwaysAllow,
                ctx,
            );
            model.add_to_mcp_denylist(
                *model.active_profile(Some(terminal_view_id), ctx).id(),
                &server_uuid,
                ctx,
            );
        });

        permissions.read(&app, |model, ctx| {
            // Denylisted server should be denied.
            assert!(!model.can_use_mcp_server(
                &convo_id,
                Some(server_uuid),
                Some(terminal_view_id),
                ctx
            ));
            // Non-denylisted server should be allowed.
            assert!(model.can_use_mcp_server(
                &convo_id,
                Some(other_uuid),
                Some(terminal_view_id),
                ctx
            ));
        });
    })
}

#[test]
fn test_can_use_mcp_server_always_ask_with_allowlist() {
    App::test((), |mut app| async move {
        let PermissionsTestState {
            convo_id,
            permissions,
            profile_model,
            terminal_view_id,
            ..
        } = initialize_permissions_test(&mut app);

        let server_uuid = Uuid::new_v4();
        let other_uuid = Uuid::new_v4();

        profile_model.update(&mut app, |model, ctx| {
            model.set_mcp_permissions(
                *model.active_profile(Some(terminal_view_id), ctx).id(),
                &ActionPermission::AlwaysAsk,
                ctx,
            );
            model.add_to_mcp_allowlist(
                *model.active_profile(Some(terminal_view_id), ctx).id(),
                &server_uuid,
                ctx,
            );
        });

        permissions.read(&app, |model, ctx| {
            // Allowlisted server should be allowed.
            assert!(model.can_use_mcp_server(
                &convo_id,
                Some(server_uuid),
                Some(terminal_view_id),
                ctx
            ));
            // Non-allowlisted server should be denied.
            assert!(!model.can_use_mcp_server(
                &convo_id,
                Some(other_uuid),
                Some(terminal_view_id),
                ctx
            ));
            // None UUID should be denied.
            assert!(!model.can_use_mcp_server(&convo_id, None, Some(terminal_view_id), ctx));
        });
    })
}

#[test]
fn test_can_use_mcp_server_always_ask_denylist_overrides_allowlist() {
    App::test((), |mut app| async move {
        let PermissionsTestState {
            convo_id,
            permissions,
            profile_model,
            terminal_view_id,
            ..
        } = initialize_permissions_test(&mut app);

        let server_uuid = Uuid::new_v4();

        profile_model.update(&mut app, |model, ctx| {
            model.set_mcp_permissions(
                *model.active_profile(Some(terminal_view_id), ctx).id(),
                &ActionPermission::AlwaysAsk,
                ctx,
            );
            model.add_to_mcp_allowlist(
                *model.active_profile(Some(terminal_view_id), ctx).id(),
                &server_uuid,
                ctx,
            );
            model.add_to_mcp_denylist(
                *model.active_profile(Some(terminal_view_id), ctx).id(),
                &server_uuid,
                ctx,
            );
        });

        permissions.read(&app, |model, ctx| {
            // Both allowlisted and denylisted: denylist wins.
            assert!(!model.can_use_mcp_server(
                &convo_id,
                Some(server_uuid),
                Some(terminal_view_id),
                ctx
            ));
        });
    })
}

#[test]
fn test_can_use_mcp_server_agent_decides() {
    App::test((), |mut app| async move {
        let PermissionsTestState {
            convo_id,
            permissions,
            profile_model,
            terminal_view_id,
            ..
        } = initialize_permissions_test(&mut app);

        let server_uuid = Uuid::new_v4();
        let other_uuid = Uuid::new_v4();

        profile_model.update(&mut app, |model, ctx| {
            model.set_mcp_permissions(
                *model.active_profile(Some(terminal_view_id), ctx).id(),
                &ActionPermission::AgentDecides,
                ctx,
            );
            model.add_to_mcp_allowlist(
                *model.active_profile(Some(terminal_view_id), ctx).id(),
                &server_uuid,
                ctx,
            );
        });

        permissions.read(&app, |model, ctx| {
            // Allowlisted and not denylisted should be allowed.
            assert!(model.can_use_mcp_server(
                &convo_id,
                Some(server_uuid),
                Some(terminal_view_id),
                ctx
            ));
            // Not allowlisted should be denied.
            assert!(!model.can_use_mcp_server(
                &convo_id,
                Some(other_uuid),
                Some(terminal_view_id),
                ctx
            ));
        });
    })
}

#[test]
fn test_can_use_mcp_server_agent_decides_denylist_overrides_allowlist() {
    App::test((), |mut app| async move {
        let PermissionsTestState {
            convo_id,
            permissions,
            profile_model,
            terminal_view_id,
            ..
        } = initialize_permissions_test(&mut app);

        let server_uuid = Uuid::new_v4();

        profile_model.update(&mut app, |model, ctx| {
            model.set_mcp_permissions(
                *model.active_profile(Some(terminal_view_id), ctx).id(),
                &ActionPermission::AgentDecides,
                ctx,
            );
            model.add_to_mcp_allowlist(
                *model.active_profile(Some(terminal_view_id), ctx).id(),
                &server_uuid,
                ctx,
            );
            model.add_to_mcp_denylist(
                *model.active_profile(Some(terminal_view_id), ctx).id(),
                &server_uuid,
                ctx,
            );
        });

        permissions.read(&app, |model, ctx| {
            // Both allowlisted and denylisted: denylist wins.
            assert!(!model.can_use_mcp_server(
                &convo_id,
                Some(server_uuid),
                Some(terminal_view_id),
                ctx
            ));
        });
    })
}

#[test]
fn test_denylist_matches_multiline_commands() {
    App::test((), |mut app| async move {
        let PermissionsTestState {
            convo_id,
            permissions,
            profile_model,
            terminal_view_id,
            ..
        } = initialize_permissions_test(&mut app);

        // Add denylist rule for rm
        profile_model.update(&mut app, |model, ctx| {
            model.add_to_command_denylist(
                *model.active_profile(Some(terminal_view_id), ctx).id(),
                &AgentModeCommandExecutionPredicate::new_regex("rm .*").unwrap(),
                ctx,
            );
        });

        // Single-line rm command should be denied
        permissions.read(&app, |model, ctx| {
            let result = model.can_autoexecute_command(
                &convo_id,
                "rm file.txt",
                EscapeChar::Backslash,
                false,
                None,
                Some(terminal_view_id),
                ctx,
            );
            assert!(!result.is_allowed());
            assert!(matches!(
                result,
                CommandExecutionPermission::Denied(
                    CommandExecutionPermissionDeniedReason::ExplicitlyDenylisted
                )
            ));
        });

        // Multiline rm command with backslash continuations should also be denied (POSIX)
        permissions.read(&app, |model, ctx| {
            let result = model.can_autoexecute_command(
                &convo_id,
                "rm file1.txt \\\nfile2.txt \\\nfile3.txt",
                EscapeChar::Backslash,
                false,
                None,
                Some(terminal_view_id),
                ctx,
            );
            assert!(
                !result.is_allowed(),
                "multiline rm command should be denied by denylist"
            );
            assert!(matches!(
                result,
                CommandExecutionPermission::Denied(
                    CommandExecutionPermissionDeniedReason::ExplicitlyDenylisted
                )
            ));
        });

        // Env-prefixed multiline rm command should also be denied after normalization.
        permissions.read(&app, |model, ctx| {
            let result = model.can_autoexecute_command(
                &convo_id,
                "X=1 rm file1.txt \\\nfile2.txt \\\nfile3.txt",
                EscapeChar::Backslash,
                false,
                None,
                Some(terminal_view_id),
                ctx,
            );
            assert!(
                !result.is_allowed(),
                "env-prefixed multiline rm command should be denied by denylist"
            );
            assert!(matches!(
                result,
                CommandExecutionPermission::Denied(
                    CommandExecutionPermissionDeniedReason::ExplicitlyDenylisted
                )
            ));
        });

        // Multiline rm command with backtick continuations should also be denied (PowerShell)
        permissions.read(&app, |model, ctx| {
            let result = model.can_autoexecute_command(
                &convo_id,
                "rm file1.txt `\nfile2.txt `\nfile3.txt",
                EscapeChar::Backtick,
                false,
                None,
                Some(terminal_view_id),
                ctx,
            );
            assert!(
                !result.is_allowed(),
                "multiline rm command with backtick continuations should be denied by denylist"
            );
            assert!(matches!(
                result,
                CommandExecutionPermission::Denied(
                    CommandExecutionPermissionDeniedReason::ExplicitlyDenylisted
                )
            ));
        });
    })
}
