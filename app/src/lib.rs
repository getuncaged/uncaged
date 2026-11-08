#![allow(clippy::doc_lazy_continuation)]

mod ai;
mod alloc;
#[cfg(target_os = "macos")]
mod app_menus;
mod app_services;
mod app_state;
mod autoupdate;
mod banner;
/// Uncaged brand — the single source of truth for name, palette, and logo
/// paths. See `brand.rs`.
mod brand;
mod changelog_model;
mod chip_configurator;
mod cloud_object;
mod code;
mod code_review;
mod coding_entrypoints;
mod coding_panel_enablement_state;
mod command_palette;
mod completer;
#[allow(dead_code)]
mod context_chips;
#[cfg(enable_crash_recovery)]
mod crash_recovery;
mod debug_dump;
mod default_terminal;
mod drive;
#[cfg(windows)]
mod dynamic_libraries;
mod env_vars;
mod external_secrets;
mod gist_sync;
mod global_resource_handles;
mod gpu_state;
mod identity_migration;
mod input_classifier;
mod interval_timer;
/// Bridge to the local Uncaged engine (see `uncaged.rs`).
mod launch_splash;
mod linear;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod login_item;
mod menu;
mod modal;
mod network;
mod notebooks;
mod notification;
mod palette;
mod persistence;
mod platform;
#[cfg(feature = "plugin_host")]
mod plugin;
mod prefix;
#[cfg(target_os = "macos")]
mod preview_config_migration;
mod profiling;
mod projects;
mod prompt;
mod quit_warning;
mod resource_limits;
mod safe_triangle;
mod search_bar;
mod server;
mod session_management;
mod settings_backup;
mod shell_indicator;
mod suggestions;
mod system;
mod tab;
#[cfg(test)]
mod test_util;
mod throttle;
mod tips;
mod tracing;
mod ui_components;
mod ui_sources;
mod uncaged;
mod undo_close;
mod uri;
mod user_config;
pub mod util;
mod view_components;
mod vim_registers;
mod voltron;
mod warp_managed_paths_watcher;
#[cfg(target_family = "wasm")]
mod wasm_nux_dialog;
mod window_settings;
mod workspaces;

// PLEASE DO NOT ADD MORE PUBLIC MODULES!
//
// Any modules which we make public outside of the `warp` crate lose dead code
// checking support, as the compiler cannot make any assumptions about whether
// or not the function/type is used by another crate that pulls in this one as
// a dependency.
//
// If you feel the need to export a module so that a type or function within it
// can be used by an integration test, you should define a new assertion function
// in the uncaged::integration_testing::assertions module (or a sub-module).  These
// functions will allow us to keep types internal to this crate and expose a
// simpler API for integration tests to consume.
pub mod ai_assistant;
pub mod appearance;
pub mod channel;
pub mod editor;
pub mod features;
pub mod input_suggestions;
#[cfg(feature = "integration_tests")]
pub mod integration_testing;
pub mod keyboard;
pub mod launch_configs;
pub mod pane_group;
pub mod resource_center;
pub mod root_view;
pub mod search;
pub mod settings;
pub mod settings_view;
pub mod tab_configs;
pub mod terminal;
pub mod themes;
use ::ai::index::full_source_code_embedding::manager::{
    CodebaseIndexManager, CodebaseIndexManagerConfig,
};
use ::ai::index::full_source_code_embedding::{LocalStoreClient, SyncTask};
use ::ai::index::DEFAULT_SYNC_REQUESTS_PER_MIN;
use ::ai::project_context::model::ProjectContextModel;
pub use ai::agent::todos::AIAgentTodoList;
pub use ai::agent::{AIAgentActionResultType, FileEdit, TodoOperation};
use ai::agent_conversations_model::AgentConversationsModel;
use ai::agent_management::AgentNotificationsModel;
use ai::blocklist::{BlocklistAIHistoryModel, BlocklistAIPermissions};
use ai::execution_profiles::editor::ExecutionProfileEditorManager;
use ai::execution_profiles::profiles::AIExecutionProfilesModel;
use ai::metadata_project_rules::read_project_rule_contents;
use ai::persisted_workspace::PersistedWorkspace;
use code::editor_management::CodeManager;
use code::opened_files::OpenedFilesModel;
use code_review::git_repo_model::GitRepoModels;
use code_review::GlobalCodeReviewModel;
use quit_warning::UnsavedStateSummary;
#[cfg(feature = "local_fs")]
use repo_metadata::{
    repositories::DetectedRepositories, watcher::DirectoryWatcher, RepoMetadataModel,
};
use server::network_log_pane_manager::NetworkLogPaneManager;
#[cfg(feature = "local_fs")]
use settings::import::model::ImportedConfigModel;
use settings_view::pane_manager::SettingsPaneManager;
use terminal::general_settings::GeneralSettings;
use terminal::keys_settings::KeysSettings;
#[cfg(all(not(target_family = "wasm"), feature = "local_tty"))]
use terminal::local_shell::LocalShellState;
pub use util::bindings::cmd_or_ctrl_shift;
#[cfg(feature = "local_fs")]
use watcher::HomeDirectoryWatcher;

use crate::ai::active_agent_views_model::ActiveAgentViewsModel;
#[cfg(not(target_family = "wasm"))]
use crate::ai::mcp::{FileBasedMCPManager, FileMCPWatcher};
use crate::settings::CodeSettings;
pub mod workflows;
pub mod workspace;

use std::borrow::Cow;
use std::collections::HashSet;
use std::ops::Deref;
use std::sync::Arc;

use ::settings::{Setting, ToggleableSetting};
#[cfg(feature = "local_tty")]
use anyhow::Context;
use anyhow::{anyhow, Result};
use appearance::{Appearance, AppearanceManager};
use channel::ChannelState;
use interval_timer::{mark_startup_sub_interval, IntervalTimer};
use itertools::Itertools;
#[cfg(feature = "integration_tests")]
pub use persistence::testing as sqlite_testing;
#[cfg(feature = "plugin_host")]
pub use plugin::{run_plugin_host, PLUGIN_HOST_FLAG};
use server::network_logging::NetworkLogModel;
use server::shared_http_client::SharedHttpClient;
use settings::{ExtraMetaKeys, PrivacySettings};
use terminal::input;
use terminal::session_settings::SessionSettings;
pub use uncaged_core::errors::{report_error, report_if_error};
use uncaged_core::execution_mode::{AppExecutionMode, ExecutionMode};
// Re-export the debounce function to simplify imports.
pub use uncaged_core::r#async::debounce;
// Re-export the safe logging macros at the crate root level for backwards compatibility
pub use uncaged_core::{safe_debug, safe_error, safe_info, safe_warn};
#[cfg(feature = "local_fs")]
use uncaged_files::FileModel;
use uncaged_ui::integration::TestDriver;
use uncaged_ui::platform::app::{ApproveTerminateResult, TerminationRequestSource};
use uncaged_ui::platform::TerminationMode;
use uncaged_ui::windowing::state::ApplicationStage;
use uncaged_ui::{App, AppContext, Event, SingletonEntity, WindowId};
use url::Url;
use window_settings::WindowSettings;
use workflows::manager::WorkflowManager;
use workspace::sync_inputs::SyncedInputState;

use self::features::FeatureFlag;
use crate::ai::agent::conversation::AIConversationId;
use crate::ai::document::ai_document_model::AIDocumentModel;
use crate::ai::facts::manager::AIFactManager;
use crate::ai::harness_availability::HarnessAvailabilityModel;
use crate::ai::llms::LLMPreferences;
use crate::ai::mcp::TemplatableMCPServerManager;
use crate::ai::outline::RepoOutlines;
use crate::ai::restored_conversations::RestoredAgentConversations;
use crate::ai::skills::SkillManager;
use crate::ai::AIRequestUsageModel;
use crate::app_state::AppState;
use crate::autoupdate::{AutoupdateState, RelaunchModel};
use crate::changelog_model::ChangelogModel;
use crate::cloud_object::model::actions::ObjectActions;
use crate::cloud_object::model::persistence::CloudModel;
use crate::cloud_object::model::view::CloudViewModel;
use crate::code::global_buffer_model::GlobalBufferModel;
#[cfg(feature = "local_fs")]
use crate::code::language_server_shutdown_manager::LanguageServerShutdownManager;
use crate::context_chips::prompt::Prompt;
use crate::default_terminal::DefaultTerminal;
use crate::drive::export::ExportManager;
use crate::drive::CloudObjectTypeAndId;
use crate::env_vars::manager::EnvVarCollectionManager;
pub use crate::global_resource_handles::{GlobalResourceHandles, GlobalResourceHandlesProvider};
use crate::gpu_state::GPUState;
use crate::network::NetworkStatus;
use crate::notebooks::editor::keys::NotebookKeybindings;
use crate::notebooks::manager::NotebookManager;
use crate::notebooks::CloudNotebook;
use crate::notification::NotificationContext;
use crate::palette::PaletteMode;
use crate::persistence::model::AgentConversationData;
use crate::persistence::PersistenceWriter;
use crate::projects::ProjectManagementModel;
use crate::root_view::{
    quake_mode_window_id, quake_mode_window_is_open, OpenFromRestoredArg, OpenPath,
};
use crate::server::cloud_objects::update_manager::UpdateManager;
use crate::session_management::{RunningSessionSummary, SessionNavigationData};
use crate::settings::manager::SettingsManager;
use crate::settings::{AISettings, AccessibilitySettings, ScrollSettings, SelectionSettings};
use crate::settings_view::keybindings::KeybindingChangedNotifier;
use crate::settings_view::DisplayCount;
use crate::suggestions::ignored_suggestions_model::IgnoredSuggestionsModel;
use crate::system::SystemStats;
use crate::terminal::cli_agent_sessions::CLIAgentSessionsModel;
use crate::terminal::keys::TerminalKeybindings;
use crate::terminal::resizable_data::ResizableData;
use crate::terminal::{AudibleBell, CustomSecretRegexUpdater, History};
use crate::ui_sources::PaletteSource;
use crate::undo_close::UndoCloseStack;
use crate::user_config::WarpConfig;
use crate::util::bindings::is_binding_cross_platform;
use crate::vim_registers::VimRegisters;
use crate::warp_managed_paths_watcher::{ensure_warp_watch_roots_exist, WarpManagedPathsWatcher};
use crate::workflows::aliases::WorkflowAliases;
use crate::workflows::local_workflows::LocalWorkflows;
use crate::workspace::{
    ActiveSession, OneTimeModalModel, PaneViewLocator, ToastStack, Workspace, WorkspaceAction,
};
use crate::workspaces::user_profiles::UserProfiles;

/// Our embedded application assets.
pub static ASSETS: uncaged_assets::Assets = uncaged_assets::Assets;

/// Launch mode for how to start up Warp.
#[allow(clippy::large_enum_variant)]
pub enum LaunchMode {
    /// Run the regular GUI application.
    App { args: uncaged_cli::AppArgs },

    /// Run a test - this may be an integration test or an eval.
    Test {
        driver: Box<Option<TestDriver>>,
        is_integration_test: bool,
    },
}

impl LaunchMode {
    fn args(&self) -> Cow<'_, uncaged_cli::AppArgs> {
        match self {
            LaunchMode::App { args, .. } => Cow::Borrowed(args),
            LaunchMode::Test { .. } => Cow::Owned(uncaged_cli::AppArgs::default()),
        }
    }

    /// Returns `true` if this process is running an integration test.
    fn is_integration_test(&self) -> bool {
        match self {
            LaunchMode::Test {
                is_integration_test,
                ..
            } => *is_integration_test,
            LaunchMode::App { .. } => false,
        }
    }

    fn take_test_driver(&mut self) -> Option<TestDriver> {
        match self {
            LaunchMode::Test { driver, .. } => driver.take(),
            LaunchMode::App { .. } => None,
        }
    }

    /// Add an URL to open. Only supported for [`LaunchMode::App`]
    #[allow(dead_code)]
    fn add_url(&mut self, url: Url) {
        if let LaunchMode::App { ref mut args, .. } = self {
            args.urls.push(url);
        }
    }

    /// Whether or not to start a crash recovery process (on platforms that support it).
    #[cfg(enable_crash_recovery)]
    pub(crate) fn crash_recovery_enabled(&self) -> bool {
        match self {
            LaunchMode::App { .. } => true,
            LaunchMode::Test { .. } => false,
        }
    }

    fn as_str_for_tracing(&self) -> &'static str {
        match self {
            LaunchMode::App { .. } => "app",
            LaunchMode::Test { .. } => "test",
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_unit_test() -> Self {
        LaunchMode::Test {
            driver: Box::new(None),
            is_integration_test: false,
        }
    }
}

/// If the given event is a key down event containing alt modifiers, and those
/// alt modifiers should be treated as meta keys, then remove the alts and
/// prefix the keys with an escape. See WAR-472.
fn apply_extra_meta_keys(event: &mut Event, extra_metas: ExtraMetaKeys) {
    if let Event::KeyDown {
        keystroke, details, ..
    } = event
    {
        let left_as_meta = extra_metas.left_alt && details.left_alt;
        let right_as_meta = extra_metas.right_alt && details.right_alt;
        if left_as_meta || right_as_meta {
            let side = match (left_as_meta, right_as_meta) {
                (true, true) => "left+right alt",
                (true, false) => "left alt",
                (false, true) => "right alt",
                (false, false) => unreachable!(),
            };
            log::info!("Treating {side} as meta");
            keystroke.alt = false;
            keystroke.meta = true;
        }
    }
}

fn apply_scroll_multiplier(event: &mut Event, app: &AppContext) {
    if let Event::ScrollWheel { delta, precise, .. } = event {
        if !*precise {
            let scroll_multiplier = *ScrollSettings::as_ref(app).mouse_scroll_multiplier.value();
            *delta *= scroll_multiplier;
        }
    }
}

/// Runs the Uncaged executable as the app or as one of its worker subcommands.
#[::tracing::instrument(skip_all, fields(tags.cloud_agent = true))]
pub fn run() -> Result<()> {
    // Perform any necessary platform-specific initialization.
    platform::init();

    // Ensure feature flags are initialized before parsing command-line arguments.
    features::init_feature_flags();

    // Parse command-line arguments.
    let args = uncaged_cli::Args::from_env();

    // Carry the previous app identity's data over before anything (logging,
    // settings, secure storage, sqlite) computes a data path. Only the app
    // itself moves data; a command-line or worker invocation keeps using the
    // previous identity's data until the app has moved it.
    identity_migration::run_if_needed(args.command().is_none());

    if let Some(command) = args.command() {
        #[cfg(windows)]
        if command.prints_to_stdout() {
            // We attach a console to ensure that all standard output gets printed correctly.
            uncaged_util::windows::attach_to_parent_console();
        }
        match command {
            #[cfg(all(feature = "local_tty", unix))]
            uncaged_cli::Command::Worker(uncaged_cli::WorkerCommand::TerminalServer(args)) => {
                // If we were asked to run as a terminal server (as opposed to the main
                // GUI application), do so immediately.  Ideally, the terminal server would
                // be a separate binary, but it's much easier to distribute a single binary,
                // so starting the terminal server event loop immediately is the closest
                // approximation we can get to running a separate binary.
                crate::terminal::local_tty::server::run_terminal_server(args);
                return Ok(());
            }
            #[cfg(feature = "plugin_host")]
            uncaged_cli::Command::Worker(uncaged_cli::WorkerCommand::PluginHost { .. }) => {
                return crate::run_plugin_host();
            }
            #[cfg(not(target_family = "wasm"))]
            uncaged_cli::Command::Worker(uncaged_cli::WorkerCommand::RipgrepSearch {
                parent,
                ignore_case,
                multiline,
                pattern,
                paths,
            }) => {
                uncaged_ripgrep::search::run_search_subprocess(
                    std::slice::from_ref(pattern),
                    paths.clone(),
                    *ignore_case,
                    *multiline,
                    parent.pid,
                )
                .map_err(|err| anyhow!(err.to_string()))?;
                return Ok(());
            }
            #[cfg(not(any(
                feature = "local_tty",
                feature = "plugin_host",
                not(target_family = "wasm")
            )))]
            uncaged_cli::Command::Worker(worker) => {
                // Need this case to handle platforms where there are no enum variants in
                // uncaged_cli::WorkerCommand, as we still need to check Command::Worker.

                // On wasm, specifically, we should fail spectacularly if we get here.
                #[cfg(target_family = "wasm")]
                panic!("Worker process not supported on WASM: {worker:?}")
            }
            uncaged_cli::Command::Completions { shell } => {
                return uncaged_cli::completions::generate_to_stdout(*shell);
            }
            uncaged_cli::Command::DumpDebugInfo => {
                return debug_dump::run();
            }
        }
    }

    run_internal(LaunchMode::App {
        args: args.into_app_args(),
    })
}

/// Runs an integration test using the provided test driver.
pub fn run_integration_test(driver: TestDriver) -> Result<()> {
    let is_integration_test = std::env::var("WARP_INTEGRATION").is_ok();
    let launch = LaunchMode::Test {
        driver: Box::new(Some(driver)),
        is_integration_test,
    };
    run_internal(launch)
}

/// Runs the app.
fn run_internal(mut launch_mode: LaunchMode) -> Result<()> {
    let mut timer = IntervalTimer::new();

    // ── Early initialization (pre-AppBuilder) ──────────────────────
    // These steps run before the platform event loop is started.
    // They must not depend on AppContext.

    #[cfg(windows)]
    dynamic_libraries::configure_library_loading();

    profiling::init();

    // The `run` function already initializes feature flags, but ensure they're initialized here
    // for other entrypoints.
    features::init_feature_flags();

    tracing::init()?;

    // Start the `run_internal` span here - we can't do it before this point
    // because we need the tracing initialization to be complete first.
    let span = ::tracing::info_span!(
        "run_internal",
        tags.cloud_agent = true,
        launch_mode = launch_mode.as_str_for_tracing()
    );
    let _enter = span.enter();

    cfg_if::cfg_if! {
        if #[cfg(enable_crash_recovery)] {
            if crash_recovery::is_crash_recovery_process(launch_mode.args().as_ref()) {
                uncaged_logging::init_for_crash_recovery_process()?;
            } else {
                uncaged_logging::init(uncaged_logging::LogConfig::default())?;
            }
        } else {
            uncaged_logging::init(uncaged_logging::LogConfig::default())?;
        }
    }

    timer.mark_interval_end("LOG_FILE_SETUP_COMPLETE");
    identity_migration::log_report();

    #[cfg(windows)]
    platform::windows::check_redirection_guard();

    // Adjust resource limits early, before doing other work, to ensure that
    // any children we spawn (like the terminal server) inherit our adjusted
    // rlimits.
    resource_limits::adjust_resource_limits();

    // For wasm builds we have this special case to parse out the intent
    // from the url that is used to visite the app on web.
    #[cfg(target_family = "wasm")]
    {
        use uri::web_intent_parser;
        if let Some(intent) = web_intent_parser::parse_web_intent_from_current_url() {
            launch_mode.add_url(intent);
        }
        web_intent_parser::set_context_flags_from_current_url();
    }

    #[cfg(all(
        feature = "release_bundle",
        any(target_os = "linux", target_os = "freebsd")
    ))]
    if let LaunchMode::App { .. } = launch_mode {
        match app_services::linux::pass_startup_args_to_existing_instance(
            launch_mode.args().as_ref(),
        ) {
            // If we were able to contact an existing application instance, quit -
            // we only want to run a single instance of Warp at a time.
            Ok(_) => std::process::exit(0),
            // If Warp isn't already running, we're good to go.
            Err(app_services::linux::StartupArgsForwardingError::NoExistingInstance) => {}
            // If we just finished an auto-update, we should continue running.
            Err(app_services::linux::StartupArgsForwardingError::IgnoredAfterAutoUpdate) => {}
            // If we were unable to perform the forwarding for an unknown reason,
            // it's better to run a second instance than potentially end up in a
            // state where Warp refuses to run even a first instance.
            Err(err) => {
                let err = anyhow::Error::from(err).context("Failed to forward startup args");
                log::error!("{err:#}");
            }
        }
    }

    #[cfg(all(feature = "release_bundle", windows))]
    if let LaunchMode::App { .. } = launch_mode {
        match app_services::windows::pass_startup_args_to_existing_instance(
            launch_mode.args().as_ref(),
        ) {
            // If we were able to contact an existing application instance, quit -
            // we only want to run a single instance of Warp at a time.
            Ok(_) => std::process::exit(0),
            // If Warp isn't already running, we're good to go.
            Err(app_services::windows::StartupArgsForwardingError::NoExistingInstance) => {}
            // If we just finished an auto-update, we should continue running.
            Err(app_services::windows::StartupArgsForwardingError::IgnoredAfterAutoUpdate) => {}
            // If we were unable to perform the forwarding for an unknown reason,
            // it's better to run a second instance than potentially end up in a
            // state where Warp refuses to run even a first instance.
            Err(err) => {
                let err = anyhow::Error::from(err).context("Failed to forward startup args");
                log::error!("{err:#}");
            }
        }
    }

    // Sets up a Job Object that we associate with the Warp process to handle
    // shared fate with its child processes. This should be called before we
    // start spawning any child processes.
    #[cfg(windows)]
    command::windows::init();
    mark_startup_sub_interval("args_forwarding");

    let private_preferences = settings::init_private_user_preferences();
    mark_startup_sub_interval("private_prefs");
    let (public_preferences, startup_toml_parse_error) = settings::init_public_user_preferences();
    mark_startup_sub_interval("settings_toml");

    // When the SettingsFile feature flag is enabled, public settings live in
    // the TOML-backed store. When disabled, they live in the platform-native
    // store (same backend as private). Use the correct one for pre-app reads.
    #[cfg_attr(
        not(any(
            enable_crash_recovery,
            target_os = "linux",
            target_os = "freebsd",
            target_os = "macos"
        )),
        expect(unused)
    )]
    let prefs_for_public_settings: &dyn uncaged_ui_extras::user_preferences::UserPreferences =
        if FeatureFlag::SettingsFile.is_enabled() {
            public_preferences.as_ref()
        } else {
            private_preferences.deref()
        };

    #[cfg(enable_crash_recovery)]
    let crash_recovery =
        crash_recovery::CrashRecovery::new(&launch_mode, prefs_for_public_settings);

    // Set up the pty spawner before doing any meaningful work. We want to
    // ensure that the process is in the cleanest possible state (minimal opened
    // files, modified signal handlers, etc.) to avoid unexpected effects on
    // spawned ptys.
    #[cfg(feature = "local_tty")]
    let pty_spawner =
        terminal::local_tty::spawner::PtySpawner::new().context("Failed to create pty spawner")?;
    mark_startup_sub_interval("pty_spawner");

    let mut app_builder = uncaged_ui::platform::AppBuilder::new(
        app_callbacks(launch_mode.is_integration_test()),
        Box::new(ASSETS),
        launch_mode.take_test_driver(),
    );
    mark_startup_sub_interval("app_builder_new");

    #[cfg(target_os = "macos")]
    {
        use uncaged_ui::platform::mac::AppExt;
        use uncaged_ui::AssetProvider as _;

        let activate_on_launch = !launch_mode.is_integration_test()
            || std::env::var("WARPUI_USE_REAL_DISPLAY_IN_INTEGRATION_TESTS").is_ok();
        app_builder.set_activate_on_launch(activate_on_launch);

        let dev_icon = ASSETS.get("bundled/png/local.png")?;
        app_builder.set_dev_icon(dev_icon);

        let show_dock_icon = crate::settings::app_icon::ShowDockIconState::read_from_preferences(
            prefs_for_public_settings,
        )
        .unwrap_or_else(crate::settings::app_icon::ShowDockIconState::default_value);
        app_builder.set_show_dock_icon_on_launch(show_dock_icon);
        app_builder.set_menu_bar_builder(app_menus::menu_bar);
        app_builder.set_dock_menu_builder(|_| app_menus::dock_menu());
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    {
        use uncaged_ui::platform::linux::{self, AppBuilderExt};

        use crate::settings::ForceX11;

        app_builder.set_window_class(ChannelState::app_id().to_string());

        let force_x11 = ForceX11::read_from_preferences(prefs_for_public_settings)
            .unwrap_or(ForceX11::default_value());
        // Force use of wayland if the user has passed the `WARP_ENABLE_WAYLAND` env var.
        let allow_wayland = linux::is_wayland_env_var_set() || !force_x11;
        app_builder.force_x11(!allow_wayland);
    }

    #[cfg(target_os = "windows")]
    {
        use uncaged_ui::platform::windows::AppBuilderExt;
        app_builder.set_app_user_model_id(ChannelState::app_id().to_string());

        // Only use DXC for DirectX shader compilation if we're not running in a Parallels VM
        // Parallels VMs can have issues with DXC shader compilation
        let is_parallels_vm = crate::util::vm_detection::is_running_in_windows_parallels_vm();
        if !is_parallels_vm {
            log::info!("Using DXC for DirectX shader compilation");
            use uncaged_ui::platform::windows::DXCPath;

            app_builder.use_dxc_for_directx_shader_compilation(DXCPath {
                dxc_path: "dxcompiler.dll".to_string(),
                dxil_path: "dxil.dll".to_string(),
            });
        } else {
            log::info!("Skipping DXC for DirectX shader compilation; running in a Parallels VM");
        }
    }

    // Override any bindings that have a `Custom` trigger to a `Keystroke`-based trigger. In theory,
    // this should be a noop on Mac (since the keystrokes registered via the  Mac menus first
    // intercept the binding), but just to be safe we only enable this in cases where we don't
    // include mac menus.
    #[cfg(not(target_os = "macos"))]
    app_builder.convert_custom_triggers_to_keystroke_triggers(
        crate::util::bindings::custom_tag_to_keystroke,
    );

    #[cfg(target_os = "macos")]
    app_builder.register_default_keystroke_triggers_for_custom_actions(
        crate::util::bindings::custom_tag_to_keystroke,
    );

    mark_startup_sub_interval("app_builder_config");

    app_builder.run(move |ctx| {
        mark_startup_sub_interval("app_launch");

        #[cfg(not(target_family = "wasm"))]
        // Rotate the log files in the background.
        ctx.background_executor()
            .spawn(uncaged_logging::rotate_log_files())
            .detach();

        ctx.add_singleton_model(|ctx| AppExecutionMode::new(ExecutionMode::App, false, ctx));

        // Add the terminal server singleton to the application.
        #[cfg(feature = "local_tty")]
        ctx.add_singleton_model(move |_ctx| pty_spawner);

        // Register user preferences.  This must be done before initializing
        // feature flags, which check user preferences for overrides.
        ctx.add_singleton_model(move |_ctx| ::settings::PublicPreferences::new(public_preferences));
        ctx.add_singleton_model(move |_ctx| private_preferences);
        let startup_toml_parse_error = startup_toml_parse_error;

        // Uncaged: scan for installed third-party CLI agents (Claude Code, Codex,
        // Gemini, …) so the "Third party CLI agents" settings page can list which
        // ones you have. Local PATH scan only — no network, no subprocess.
        ctx.add_singleton_model(crate::terminal::cli_agent::CLIAgentInstallModel::new);

        #[cfg(enable_crash_recovery)]
        ctx.add_singleton_model(move |_ctx| crash_recovery);

        #[cfg(feature = "plugin_host")]
        ctx.add_singleton_model(move |ctx| {
            plugin::PluginHost::new(ctx).expect("Could not instantiate PluginHost")
        });
        mark_startup_sub_interval("early_singletons");
        let app_state = initialize_app(timer, startup_toml_parse_error, ctx);

        // Pinned from the ImprovedPaletteSearch client experiment, which assigned every user
        // (100% of buckets) to the full-text palette search.
        FeatureFlag::UseTantivySearch.set_enabled(true);

        launch(ctx, app_state, launch_mode);
    })
}

pub struct UpdateQuakeModeEventArg {
    active_window_id: Option<WindowId>,
}

#[::tracing::instrument(skip_all, fields(tags.cloud_agent = true))]
pub(crate) fn initialize_app(
    mut timer: IntervalTimer,
    startup_toml_parse_error: Option<uncaged_ui_extras::user_preferences::Error>,
    ctx: &mut uncaged_ui::AppContext,
) -> Option<AppState> {
    let data_domain = ChannelState::data_domain();

    // Register an implementation of the secure storage service.
    cfg_if::cfg_if! {
        if #[cfg(feature = "integration_tests")] {
            uncaged_ui_extras::secure_storage::register_noop(&data_domain, ctx);
        } else {
            // Secrets the previous app identity stored are read through and
            // copied forward (see `identity_migration`).
            let legacy_data_domain = identity_migration::legacy_data_domain();
            let legacy_data_domain = legacy_data_domain.as_deref();
            cfg_if::cfg_if! {
                if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
                    uncaged_ui_extras::secure_storage::register_with_fallback(&data_domain, legacy_data_domain, uncaged_core::paths::state_dir(), ctx)
                } else if #[cfg(target_os = "windows")] {
                    uncaged_ui_extras::secure_storage::register_with_dir(&data_domain, legacy_data_domain, uncaged_core::paths::state_dir(), ctx)
                } else {
                    // Uncaged: on macOS this is file-backed rather than the login keychain --
                    // see `secure_storage/mac.rs` for why, and for what to restore if Uncaged
                    // ever gets a signing identity.
                    uncaged_ui_extras::secure_storage::register(&data_domain, legacy_data_domain, ctx);
                }
            }
        }
    }
    mark_startup_sub_interval("secure_storage");

    // One-time migration: give Preview its own config directory by
    // symlinking contents from the shared ~/.warp location. Must run
    // before ensure_warp_watch_roots_exist() creates the new directory.
    #[cfg(target_os = "macos")]
    preview_config_migration::migrate_preview_config_dir_if_needed();

    ensure_warp_watch_roots_exist();
    ctx.add_singleton_model(WarpManagedPathsWatcher::new);

    ctx.add_singleton_model(WarpConfig::new);
    // Empty until the theme gallery is first shown; see `ThemeGalleryCatalogue`.
    ctx.add_singleton_model(|_| themes::theme_gallery::ThemeGalleryCatalogue::new());
    ctx.add_singleton_model(|_ctx| SettingsManager::default());
    mark_startup_sub_interval("config_watchers");

    let user_defaults_on_startup = settings::init(startup_toml_parse_error, ctx);
    timer.mark_interval_end("READ_USER_DEFAULTS_AND_INITIALIZE_SETTINGS");

    if FeatureFlag::UIZoom.is_enabled() {
        ctx.set_zoom_factor(WindowSettings::as_ref(ctx).zoom_level.as_zoom_factor());
    }

    // NetworkLogModel must be registered before SharedHttpClient so that
    // `NetworkLogModel::install_on_clients` can reach it when forwarding items
    // captured by the HTTP client hooks.
    ctx.add_singleton_model(|_ctx| NetworkLogModel::default());
    ctx.add_singleton_model(SharedHttpClient::new);

    ctx.add_singleton_model(|_ctx| GPUState::new());

    PrivacySettings::register_singleton(ctx);

    // If any part of sqlite initialization fails, we just don't do session restoration (i.e.
    // feature degradation).
    let persistence_scope = persistence::PersistenceScope::App;
    // Uncaged one-history (§3): default ON. The runtime feature-flag menu is
    // the kill-switch -- flipping it off reverts to the legacy restore
    // heuristic and destructive Cmd-K (turns cleared while it was on will
    // reappear then; a kill-switch that resurrects data errs on the safe
    // side). Must precede persistence::initialize so the restore read path
    // sees it.
    FeatureFlag::OneHistory.set_enabled(true);
    mark_startup_sub_interval("pre_sqlite_singletons");
    let (sqlite_data, writer_handles) = persistence::initialize(persistence_scope);
    timer.mark_interval_end("SQLITE_INITIALIZED");

    let persistence_writer = PersistenceWriter::new(writer_handles);

    let model_event_sender = persistence_writer.sender();

    let tips_handle = ctx.add_model(|_| user_defaults_on_startup.tips_data);
    let user_default_shell_unsupported_banner_model_handle =
        ctx.add_model(|_| user_defaults_on_startup.user_default_shell_unsupported_banner_state);
    let settings_file_error = user_defaults_on_startup.settings_file_error;
    ctx.add_singleton_model(move |_ctx| {
        GlobalResourceHandlesProvider::new(GlobalResourceHandles {
            model_event_sender,
            tips_completed: tips_handle,
            user_default_shell_unsupported_banner_model_handle,
            settings_file_error,
        })
    });

    let (
        cloud_objects,
        app_state,
        command_history,
        restored_user_profiles,
        object_actions,
        ai_queries,
        persisted_workspaces,
        workspace_language_servers,
        multi_agent_conversations,
        persisted_projects,
        persisted_project_rules,
        persisted_ignored_suggestions,
        persisted_mcp_server_installations,
        mcp_servers_to_restore,
    ) = sqlite_data
        .map(|sqlite_data| {
            (
                sqlite_data.cloud_objects,
                Some(sqlite_data.app_state),
                sqlite_data.command_history,
                sqlite_data.user_profiles,
                sqlite_data.object_actions,
                sqlite_data.ai_queries,
                sqlite_data.codebase_indices,
                sqlite_data.workspace_language_servers,
                sqlite_data.multi_agent_conversations,
                sqlite_data.projects,
                sqlite_data.project_rules,
                sqlite_data.ignored_suggestions,
                sqlite_data.mcp_server_installations,
                sqlite_data.mcp_servers_to_restore,
            )
        })
        .unwrap_or_else(|| {
            (
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
            )
        });

    ctx.add_singleton_model(|_| AIRequestUsageModel::new());

    ctx.add_singleton_model(|ctx| {
        #[cfg_attr(target_family = "wasm", allow(unused_mut))]
        let mut manager = ::ai::api_keys::ApiKeyManager::new(ctx);
        // The Grok subscription refresher (`ai::grok_subscription`) has no
        // visibility into the BYO API key policy, so wire it in here. Uncaged has
        // no BYO entitlement on Warp's server, so proactive refresh stays off.
        #[cfg(not(target_family = "wasm"))]
        if FeatureFlag::SuperGrok.is_enabled() {
            manager.set_grok_refresh_allowed(false, ctx);
        }
        manager
    });

    ctx.set_fallback_font_source_provider(|url| ::asset_cache::url_source(url));

    ctx.set_default_binding_validator(is_binding_cross_platform);

    if FeatureFlag::Autoupdate.is_enabled() {
        // Attempt to clean up any old executable, whether or not we were
        // explicitly launched as part of the auto-update process.  We may have
        // failed to remove the executable on a previous launch of the app and
        // should try again.
        if let Err(e) = autoupdate::remove_old_executable() {
            log::error!("Failed to remove old executable: {e:?}");
        }
    }

    mark_startup_sub_interval("singletons_core");
    // Initialize timestamp for session id and last active event
    App::record_last_active_timestamp();

    ctx.add_singleton_model(|_| SettingsPaneManager::new());
    ctx.add_singleton_model(|_| AIFactManager::new());
    ctx.add_singleton_model(|_| ExecutionProfileEditorManager::default());
    ctx.add_singleton_model(|_| NetworkLogPaneManager::default());

    #[cfg(target_os = "macos")]
    AppearanceManager::as_ref(ctx).set_app_icon(ctx);

    #[cfg(feature = "local_tty")]
    terminal::available_shells::register(ctx);

    // Add truly global actions that don't depend on the existence of any view here
    ctx.add_global_action("app:toggle_user_ps1", move |_args: &(), ctx| {
        SessionSettings::handle(ctx).update(ctx, |session_settings, ctx| {
            report_if_error!(session_settings.honor_ps1.toggle_and_save_value(ctx));
        });
    });
    ctx.add_global_action("app:toggle_copy_on_select", move |_args: &(), ctx| {
        SelectionSettings::handle(ctx).update(ctx, |selection_settings, ctx| {
            report_if_error!(selection_settings.copy_on_select.toggle_and_save_value(ctx));
        });
    });

    ctx.add_singleton_model(|_ctx| SyncedInputState::new());

    log::info!(
        "Starting warp with channel state {} and version {:?}",
        ChannelState::debug_str(),
        ChannelState::app_version()
    );

    // Teach our app that sometimes option means meta.
    ctx.set_event_munger(move |event, ctx| {
        let extra_meta_keys = *KeysSettings::as_ref(ctx).extra_meta_keys;
        apply_extra_meta_keys(event, extra_meta_keys);
        apply_scroll_multiplier(event, ctx);
    });

    ctx.set_a11y_verbosity(*AccessibilitySettings::as_ref(ctx).a11y_verbosity);

    #[cfg(enable_crash_recovery)]
    ctx.on_draw_frame_error(|ctx, window_id| {
        crash_recovery::CrashRecovery::handle(ctx).update(ctx, |crash_recovery, _ctx| {
            crash_recovery.on_draw_frame_error(window_id);
        });
    });

    // Uncaged: log where startup time went, so a slow launch can be traced from the log.
    ctx.on_first_frame_drawn(|ctx| {
        IntervalTimer::handle(ctx).update(ctx, |timer, _| {
            timer.mark_interval_end("FIRST_FRAME_DRAWN");
            log::info!("Startup timing: {}", timer.startup_summary());
        });
    });

    #[cfg(not(target_family = "wasm"))]
    {
        ctx.add_singleton_model(DirectoryWatcher::new);
        // Register the skill provider directories as force-included paths so
        // the gitignore-pruning watch descend filter still watches gitignored
        // skill directories (e.g. `.agents/skills`) for `Repository`
        // subscribers (LSP, MCP). Registered before any repository begins
        // watching so it gates descent on the very first registration.
        DirectoryWatcher::handle(ctx).update(ctx, |watcher, _| {
            watcher.register_force_included_paths(
                ::ai::skills::SKILL_PROVIDER_DEFINITIONS
                    .iter()
                    .map(|provider| provider.skills_path.clone()),
            );
        });
        ctx.add_singleton_model(|_| DetectedRepositories::default());
        if let Some(home_dir) = dirs::home_dir() {
            ctx.add_singleton_model(|ctx| HomeDirectoryWatcher::new(home_dir, ctx));
        } else {
            log::info!("Home directory not found; skipping HomeDirectoryWatcher registration");
        }
    }

    #[cfg(feature = "local_fs")]
    {
        // Uncaged: registered, not searched. `search_for_settings_to_import` ran here on
        // every launch, and its first act is `all_system_fonts` -- which makes CoreText map
        // every font file on the machine. Measured with malloc_history: ~1.17 GB of
        // font-file mmaps traced back to this call chain, the largest single source of
        // file-backed memory in the process, paid at startup for an import-settings
        // convenience almost nobody is about to use. The search now starts when the
        // import UI is opened (`SettingsImportView::new`).
        let _imported_config_model = ctx.add_singleton_model(ImportedConfigModel::new);

        ctx.add_singleton_model(|ctx| {
            let model = RepoMetadataModel::new(ctx);
            model.register_force_included_paths(
                ::ai::skills::SKILL_PROVIDER_DEFINITIONS
                    .iter()
                    .map(|provider| provider.skills_path.clone()),
                ctx,
            );
            model.set_project_skill_provider_paths(
                ::ai::skills::SKILL_PROVIDER_DEFINITIONS
                    .iter()
                    .map(|provider| provider.skills_path.clone()),
                ctx,
            );

            model
        });
    }

    mark_startup_sub_interval("watchers");
    ctx.add_singleton_model(|_| GitRepoModels::new());

    ctx.add_singleton_model(|ctx| {
        ProjectManagementModel::new(persisted_projects, persistence_writer.sender(), ctx)
    });

    ctx.add_singleton_model(move |_| History::new(command_history));

    ctx.add_singleton_model(CustomSecretRegexUpdater::new);

    mark_startup_sub_interval("history");
    // Register initial keybindings prior to creating menus
    ai::init(ctx);
    app_services::init(ctx);
    // // TODO: Temporarily disabling keybindings for WASM builds. Will be implemented in future WASM support.
    #[cfg(not(target_family = "wasm"))]
    code::editor::find::view::init(ctx);
    workspace::init(ctx);
    pane_group::init(ctx);
    terminal::init(ctx);
    input::init(ctx);
    editor::init(ctx);
    onboarding::init(ctx);
    menu::init(ctx);
    tips::tip_view::init(ctx);
    launch_configs::init(ctx);
    workflows::init(ctx);
    root_view::init(ctx);
    voltron::init(ctx);
    crate::view_components::find::init(ctx);
    prompt::editor_modal::init(ctx);
    ai::blocklist::agent_view::editor::init(ctx);
    undo_close::init(ctx);
    tab_configs::new_worktree_modal::init(ctx);
    tab_configs::params_modal::init(ctx);
    ai::blocklist::init(ctx);
    ai::blocklist::block::status_bar::init(ctx);
    drive::index::init(ctx);
    env_vars::env_var_collection_block::init(ctx);
    context_chips::display_menu::init(ctx);
    context_chips::node_version_popup::init(ctx);
    env_vars::view::env_var_collection::init(ctx);
    ai::agent::todos::popup::init(ctx);
    coding_entrypoints::project_buttons::init(ctx);
    if FeatureFlag::CodeReviewSaveChanges.is_enabled() {
        code_review::init(ctx);
    }

    mark_startup_sub_interval("view_inits");
    let display_count = ctx.windows().display_count();
    ctx.add_singleton_model(|_| DisplayCount(display_count));

    ctx.add_singleton_model(|_| RelaunchModel::new());
    ctx.add_singleton_model(|_| ChangelogModel::default());
    ctx.add_singleton_model(|_| NetworkStatus::new());
    ctx.add_singleton_model(|_| SystemStats::new());
    ctx.add_singleton_model(|_| KeybindingChangedNotifier::new());
    ctx.add_singleton_model(|_| search::command_palette::SelectedItems::new());
    ctx.add_singleton_model(search::files::model::FileSearchModel::new);
    ctx.add_singleton_model(|_| VimRegisters::new());
    ctx.add_singleton_model(UndoCloseStack::new);
    ctx.add_singleton_model(|_| ToastStack);
    ctx.add_singleton_model(|_| GlobalCodeReviewModel);
    ctx.add_singleton_model(|_| workspace::OneTimeModalModel::new());
    #[cfg(feature = "local_fs")]
    ctx.add_singleton_model(FileModel::new);
    ctx.add_singleton_model(GlobalBufferModel::new);
    #[cfg(windows)]
    ctx.add_singleton_model(util::traffic_lights::windows::RendererState::new);
    #[cfg(feature = "local_fs")]
    ctx.add_singleton_model(|_| LanguageServerShutdownManager::new());

    mark_startup_sub_interval("misc_models");
    let notebooks = cloud_objects
        .iter()
        .filter_map(|object| {
            let notebook: Option<&CloudNotebook> = object.into();
            notebook
        })
        .cloned()
        .collect::<Vec<_>>();

    ctx.add_singleton_model(|_ctx| CloudModel::new(persistence_writer.sender(), cloud_objects));

    mark_startup_sub_interval("cloud_model");
    // Seed the orchestration pin set from persisted conversation data
    // before the conversations vec is consumed by the singletons below.
    // Each conversation's `AgentConversationData.pinned` is the source of
    // truth; the singleton mirrors them in memory for fast cross-pane lookups.
    let initial_pinned_conversations: HashSet<AIConversationId> = multi_agent_conversations
        .iter()
        .filter_map(|conv| {
            let data =
                serde_json::from_str::<AgentConversationData>(&conv.conversation.conversation_data)
                    .ok()?;
            if !data.pinned {
                return None;
            }
            AIConversationId::try_from(conv.conversation.conversation_id.clone()).ok()
        })
        .collect();
    {
        let conversations = &multi_agent_conversations;
        ctx.add_singleton_model(move |_| BlocklistAIHistoryModel::new(ai_queries, conversations));
    }
    // Per-conversation queued prompts. Registered after the history model
    // since it subscribes to history events for cleanup.
    ctx.add_singleton_model(ai::blocklist::QueuedQueryModel::new);
    // Cross-pane UI state for the orchestration pill bar. Registered
    // after the history model since it subscribes to history events.
    ctx.add_singleton_model(move |ctx| {
        ai::blocklist::agent_view::orchestration_pill_bar_model::OrchestrationPillBarModel::new(
            initial_pinned_conversations,
            ctx,
        )
    });
    ctx.add_singleton_model(move |_| RestoredAgentConversations::new(multi_agent_conversations));
    ctx.add_singleton_model(|_| CLIAgentSessionsModel::new());
    // ActiveAgentViewsModel is used to track active agent conversations and notify listeners when they change.
    ctx.add_singleton_model(|_| ActiveAgentViewsModel::new());
    ctx.add_singleton_model(AgentNotificationsModel::new);
    ctx.add_singleton_model(BlocklistAIPermissions::new);
    ctx.add_singleton_model(ai::blocklist::orchestration_events::OrchestrationEventService::new);

    mark_startup_sub_interval("conversation_models");
    ctx.add_singleton_model(RepoOutlines::new);
    ctx.add_singleton_model(|ctx| {
        uncaged_core::sync_queue::SyncQueue::<SyncTask>::new_with_rate_limit(
            &ctx.background_executor(),
            Some(DEFAULT_SYNC_REQUESTS_PER_MIN),
        )
    });

    ctx.add_singleton_model(|_| UserProfiles::new(restored_user_profiles));

    ctx.add_singleton_model(|_| ObjectActions::new(object_actions));

    ctx.add_singleton_model(|_| AudibleBell::new());

    ctx.add_singleton_model(|_| UpdateManager::new(persistence_writer.sender()));

    mark_startup_sub_interval("profiles_update_manager");
    // LogManager must be registered before any subsystem (e.g. MCP, LSP) that creates file-based loggers.
    ctx.add_singleton_model(|_| simple_logger::manager::LogManager::new());

    let running_mcp_servers = app_state
        .as_ref()
        .map(|app_state| app_state.running_mcp_servers.as_slice())
        .unwrap_or(&[]);

    // FileMCPWatcher must be registered before FileBasedMCPManager, which subscribes to it.
    ctx.add_singleton_model(FileMCPWatcher::new);
    ctx.add_singleton_model(FileBasedMCPManager::new);

    // TemplatableMCPServerManager must be registered after UpdateManager and MCPServerManager so it can migrate legacy MCPs on start up
    // It should also be registered after FileBasedMCPManager so it can receive file-based server updates.
    ctx.add_singleton_model(|ctx| {
        TemplatableMCPServerManager::new(
            persisted_mcp_server_installations,
            mcp_servers_to_restore,
            running_mcp_servers,
            ctx,
        )
    });

    mark_startup_sub_interval("mcp_managers");
    // SkillManager is used to cache SKILL.md files for all active terminal views and their working directories
    ctx.add_singleton_model(SkillManager::new);

    ctx.add_singleton_model(CloudViewModel::new);

    ctx.add_singleton_model(AIDocumentModel::new);

    ctx.add_singleton_model(AgentConversationsModel::new);

    ctx.add_singleton_model(ExportManager::new);
    ctx.add_singleton_model(|ctx| NotebookManager::new(notebooks, ctx));
    ctx.add_singleton_model(|_| CodeManager::default());
    ctx.add_singleton_model(|_| OpenedFilesModel::new());
    ctx.add_singleton_model(NotebookKeybindings::new);
    ctx.add_singleton_model(TerminalKeybindings::new);
    ctx.add_singleton_model(|_| ActiveSession::default());

    mark_startup_sub_interval("drive_code_models");
    #[cfg(all(not(target_family = "wasm"), feature = "local_tty"))]
    {
        ctx.add_singleton_model(LocalShellState::new);
        ctx.add_singleton_model(system::SystemInfo::new);
    }

    // Add a singleton model that holds the current prompt configuration.
    ctx.add_singleton_model(Prompt::new);

    // Add a singleton model for resizable modals whose size should be persisted through restarts.
    ctx.add_singleton_model(|_| ResizableData::default());

    ctx.add_singleton_model(|_| EnvVarCollectionManager::default());
    ctx.add_singleton_model(|_| WorkflowManager::default());

    AutoupdateState::register(ctx);

    ctx.add_singleton_model(LocalWorkflows::new);

    ctx.add_singleton_model(LLMPreferences::new);
    ctx.add_singleton_model(HarnessAvailabilityModel::new);

    mark_startup_sub_interval("prompt_workflows_llm");
    let tip_model_handle = ctx.add_singleton_model(|ctx| {
        ai::agent_tips::AITipModel::<ai::AgentTip>::new_for_agent_tips(ctx)
    });
    {
        // Rebuild the tip pool when AI settings change so tips whose applicability
        // depends on AI settings appear/disappear without waiting for the next cooldown cycle.
        let tip_model_handle_for_ai = tip_model_handle.clone();
        ctx.subscribe_to_model(&AISettings::handle(ctx), move |_, _, ctx| {
            tip_model_handle_for_ai.update(ctx, |model, ctx| {
                model.revalidate_tips(ctx);
            });
        });
        // Revalidate when any keybinding changes so tips with `<keybinding>`
        // placeholders are hidden/shown when the referenced binding is cleared
        // or reassigned.
        ctx.subscribe_to_model(&KeybindingChangedNotifier::handle(ctx), move |_, _, ctx| {
            tip_model_handle.update(ctx, |model, ctx| {
                model.revalidate_tips(ctx);
            });
        });
    }

    mark_startup_sub_interval("tips");
    timer.mark_interval_end("SINGLETON_MODELS_REGISTERED");

    ctx.add_singleton_model(move |_| timer);

    ctx.add_singleton_model(AIExecutionProfilesModel::new);

    ctx.add_singleton_model(DefaultTerminal::new);

    ctx.add_singleton_model(|ctx| {
        let should_restore_indices = CodeSettings::is_codebase_context_enabled(ctx);
        let indices_to_restore = if should_restore_indices {
            persisted_workspaces.clone()
        } else {
            vec![]
        };

        let codebase_limits = AIRequestUsageModel::as_ref(ctx).codebase_context_limits();
        let codebase_index_config = CodebaseIndexManagerConfig::new(
            indices_to_restore,
            codebase_limits.max_indices_allowed,
            codebase_limits.max_files_per_repo,
            codebase_limits.embedding_generation_batch_size,
            // Uncaged: chunks are indexed and searched on this machine, not on Warp's servers.
            Arc::new(LocalStoreClient::new()),
            true,
        );
        CodebaseIndexManager::new_with_config(codebase_index_config, ctx)
    });

    // Uncaged: serve the index to CLI agents (Claude Code, Gemini CLI, Codex) as an MCP tool.
    #[cfg(not(target_family = "wasm"))]
    ctx.add_singleton_model(ai::uncaged_code_search::UncagedCodeSearch::new);

    ctx.add_singleton_model(|ctx| {
        ProjectContextModel::new_from_persisted(
            persisted_project_rules,
            read_project_rule_contents,
            ctx,
        )
    });

    // Index global rules (e.g. ~/.agents/AGENTS.md) on a background task so
    // they are available to subsequent agent queries.
    ProjectContextModel::handle(ctx).update(ctx, |me, ctx| me.index_global_rules(ctx));

    ctx.add_singleton_model(|ctx| {
        PersistedWorkspace::new(
            persisted_workspaces,
            workspace_language_servers,
            persistence_writer.sender(),
            ctx,
        )
    });
    ctx.add_singleton_model(move |_| persistence_writer);

    ctx.add_singleton_model(input_classifier::InputClassifierModel::new);

    ctx.add_singleton_model(move |_| IgnoredSuggestionsModel::new(persisted_ignored_suggestions));

    // Subscribe WorkflowAliases to the UpdateManager so that it can be notified when objects are
    // trashed.
    WorkflowAliases::handle(ctx).update(ctx, |aliases, ctx| {
        aliases.connect(ctx);
    });

    app_state
}

pub(crate) fn app_callbacks(is_integration_test: bool) -> uncaged_ui::platform::AppCallbacks {
    uncaged_ui::platform::AppCallbacks {
        on_internet_reachability_changed: Some(Box::new(move |reachable, ctx| {
            NetworkStatus::handle(ctx)
                .update(ctx, move |me, ctx| me.reachability_changed(reachable, ctx));
        })),
        on_screen_changed: Some(Box::new(move |ctx| {
            ctx.dispatch_global_action(
                "root_view:move_quake_mode_window_from_screen_change",
                &KeysSettings::as_ref(ctx)
                    .quake_mode_settings
                    .value()
                    .clone(),
            );

            let new_display_count = ctx.windows().display_count();
            DisplayCount::handle(ctx).update(ctx, |display_count, ctx| {
                display_count.0 = new_display_count;
                ctx.notify();
            });
        })),
        on_cpu_awakened: Some(Box::new(move |ctx| {
            SystemStats::handle(ctx).update(ctx, move |system, ctx| {
                log::info!("System has returned from sleep");
                system.dispatch_cpu_was_awakened(ctx);
            });
        })),
        on_cpu_will_sleep: Some(Box::new(move |ctx| {
            SystemStats::handle(ctx).update(ctx, move |system, ctx| {
                log::info!("System is going to sleep...");
                system.dispatch_cpu_will_sleep(ctx);
            });
        })),
        on_resigned_active: Some(Box::new(move |ctx| {
            let active_window_id = ctx.windows().active_window();
            let update_quake_mode_arg = UpdateQuakeModeEventArg { active_window_id };

            ctx.dispatch_global_action("root_view:update_quake_mode_state", &update_quake_mode_arg);
        })),
        on_will_terminate: Some(Box::new(move |ctx| {
            NotebookManager::handle(ctx).update(ctx, |manager, ctx| {
                // Notebooks are only saved periodically, so ensure that any pending changes have
                // been sent to the writer thread before terminating.
                manager.close_notebooks(ctx);
            });

            PersistenceWriter::handle(ctx).update(ctx, |writer, _ctx| {
                writer.terminate();
            });

            // Shutdown all LSP servers gracefully before app termination
            lsp::LspManagerModel::handle(ctx).update(ctx, |manager, ctx| {
                manager.terminate(ctx);
            });

            // We want to tear down the terminal server before relaunching for
            // autoupdate, to ensure we're not running any extra Warp processes
            // when we bring up the new process.  Additionally, this must occur
            // after terminating the persistence writer, so we don't keep track
            // of the fact that the shell sessions terminated.
            #[cfg(feature = "local_tty")]
            terminal::local_tty::spawner::PtySpawner::handle(ctx).update(ctx, |pty_spawner, _| {
                pty_spawner.prepare_for_app_termination();
            });

            #[cfg(all(feature = "local_tty", windows))]
            terminal::local_tty::shutdown_all_pty_event_loops(ctx);

            // Tear down app services before spawning the new process, to
            // ensure that the new process doesn't find the old process while
            // attempting to enforce our single-instance policy on Linux.
            app_services::teardown(ctx);
            autoupdate::spawn_child_if_necessary(ctx);

            // Tear down any application profilers that are running, writing
            // results to disk.
            profiling::teardown();

            #[cfg(enable_crash_recovery)]
            crash_recovery::CrashRecovery::handle(ctx).update(ctx, |crash_recovery, _ctx| {
                crash_recovery.teardown();
            });
        })),
        on_should_close_window: Some(Box::new(move |window_id, ctx| {
            let general_settings = GeneralSettings::as_ref(ctx);
            // On Linux or Windows, if we're about to close the final window, we should quit the app instead.
            // On Mac, we do this conditionally based on a user setting.
            let quit_on_last_window_closed =
                cfg!(any(target_os = "linux", target_os = "freebsd", windows))
                    || *general_settings.quit_on_last_window_closed;
            if ctx.window_ids().count() == 1 && quit_on_last_window_closed {
                log::info!("No windows left, terminating app");
                ctx.terminate_app(TerminationMode::Cancellable, None);
                return ApproveTerminateResult::Cancel;
            }

            let summary = UnsavedStateSummary::for_window(window_id, ctx);

            // Don't show dialog on integration test. Machine can't press buttons.
            if !is_integration_test && summary.should_display_warning(ctx) {
                let shown = summary
                    .dialog()
                    .on_confirm(move |ctx| {
                        ctx.windows()
                            .close_window(window_id, TerminationMode::ForceTerminate);
                    })
                    .on_cancel(move |ctx| {
                        on_close_window_cancelled(window_id, false, ctx);
                    })
                    .on_show_processes(move |ctx| {
                        on_close_window_cancelled(window_id, true, ctx);
                    })
                    .show(ctx);
                if shown {
                    ApproveTerminateResult::Cancel
                } else {
                    ApproveTerminateResult::Terminate
                }
            } else {
                ApproveTerminateResult::Terminate
            }
        })),
        on_should_terminate_app: Some(Box::new(move |source, ctx| {
            // Never interrupt a system-initiated termination (logout / restart /
            // scheduled OS update): both cancel paths below return
            // `ApproveTerminateResult::Cancel`, which macOS interprets as Warp
            // refusing to quit. That can abort a scheduled OS update while the
            // quit-warning modal has no visible window to attach to, leaving
            // Warp waiting on a prompt nobody can see (#12441). Skipping
            // `apply_pending_update` here doesn't lose the update: the next
            // update check re-detects it (autoupdate state isn't persisted
            // across restarts, so the artifact may be re-downloaded).
            if source == TerminationRequestSource::System {
                return ApproveTerminateResult::Terminate;
            }

            // If there's a pending autoupdate, apply that before showing the unsaved changes
            // dialog. We apply the update first so that the dialog can force-terminate.
            let applying_update = autoupdate::apply_pending_update(ctx, |ctx| {
                // Once the deferred update is applied, re-terminate the app. This termination is
                // cancellable so that we still show the unsaved changes dialog.
                log::info!("Deferred autoupdate applied, terminating app");
                ctx.terminate_app(TerminationMode::Cancellable, None);
            });
            if applying_update {
                return ApproveTerminateResult::Cancel;
            }

            let summary = UnsavedStateSummary::for_app(ctx);
            // Don't show dialog on integration test. Machine can't press buttons.
            if !is_integration_test && summary.should_display_warning(ctx) {
                let shown = summary
                    .dialog()
                    .on_confirm(|ctx| ctx.terminate_app(TerminationMode::ForceTerminate, None))
                    .on_show_processes(|ctx| on_close_app_cancelled(true, ctx))
                    .on_cancel(|ctx| on_close_app_cancelled(false, ctx))
                    .show(ctx);
                if shown {
                    return ApproveTerminateResult::Cancel;
                }
            }

            ApproveTerminateResult::Terminate
        })),
        on_disable_warning_modal: Some(Box::new(move |ctx| {
            GeneralSettings::handle(ctx).update(ctx, |general_settings, ctx| {
                report_if_error!(general_settings
                    .show_warning_before_quitting
                    .toggle_and_save_value(ctx));
            });
        })),
        on_notification_clicked: Some(Box::new(move |notification_response, ctx| {
            if let Some(notification_data) = notification_response.data() {
                let context: serde_json::Result<NotificationContext> =
                    serde_json::from_str(notification_data);
                if let Ok(NotificationContext::BlockOrigin {
                    window_id,
                    pane_group_id,
                    pane_id,
                }) = context
                {
                    // Ensure the window ID exists, if so dispatch an action to focus
                    // the correct pane.
                    if ctx.window_ids().contains(&window_id) {
                        if let Some(root_view_id) = ctx.root_view_id(window_id) {
                            ctx.dispatch_action(
                                window_id,
                                &[root_view_id],
                                "root_view:handle_notification_click",
                                &PaneViewLocator {
                                    pane_group_id,
                                    pane_id,
                                },
                                log::Level::Info,
                            );
                        }
                    }
                }
            }
        })),
        on_new_window_requested: Some(Box::new(move |ctx| {
            // This one is called when the app is requested to open a new window,
            // e.g. clicking on the Dock icon. It is NOT called from the New Window
            // menu item.
            App::record_last_active_timestamp();
            ctx.dispatch_global_action("root_view:open_new", &());
            ctx.dispatch_global_action("workspace:save_app", &());
        })),
        on_open_urls: Some(Box::new(move |urls, ctx| {
            for url in &urls {
                let parsed_url = Url::parse(url);
                match parsed_url {
                    Ok(url) => uri::handle_incoming_uri(&url, ctx),
                    Err(e) => log::warn!("Unable to parse received url: {e}"),
                }
            }
        })),
        on_os_appearance_changed: Some(Box::new(move |ctx| {
            AppearanceManager::handle(ctx).update(ctx, |appearance_manager, ctx| {
                appearance_manager.refresh_theme_state(ctx);
            });
        })),
        on_active_window_changed: Some(Box::new(move |ctx| {
            let windowing_model = ctx.windows();
            let active_window_id = windowing_model.active_window();
            let key_window_is_modal_panel = windowing_model.key_window_is_modal_panel();

            if !key_window_is_modal_panel {
                let update_quake_mode_arg = UpdateQuakeModeEventArg { active_window_id };
                ctx.dispatch_global_action(
                    "root_view:update_quake_mode_state",
                    &update_quake_mode_arg,
                );
            }

            if let Some(active_window_id) = active_window_id {
                OneTimeModalModel::handle(ctx).update(ctx, |model, ctx| {
                    model.update_target_window_id(active_window_id, ctx);
                });
            }

            ctx.dispatch_global_action("workspace:save_app", &());
        })),
        on_window_will_close: Some(Box::new(move |closed_window_data, ctx| {
            if ctx.windows().stage() == ApplicationStage::Terminating {
                return;
            }

            if let Some(window_data) = closed_window_data {
                UndoCloseStack::handle(ctx).update(ctx, |stack, ctx| {
                    stack.handle_window_closed(window_data, ctx);
                });
            }
            ctx.dispatch_global_action("workspace:save_app", &());
        })),
        on_window_moved: Some(Box::new(move |ctx| {
            ctx.dispatch_global_action("workspace:save_app", &());
        })),
        on_window_resized: Some(Box::new(move |ctx| {
            ctx.dispatch_global_action("workspace:save_app", &());
        })),
        ..Default::default()
    }
}

fn on_close_app_cancelled(open_navigation_palette: bool, ctx: &mut AppContext) {
    autoupdate::cancel_relaunch(ctx);

    let sessions = SessionNavigationData::all_sessions(ctx).collect_vec();
    let sessions_summary = RunningSessionSummary::new(&sessions);

    // If open_navigation_palette is false, return early. Otherwise, we honor the open_navigation_palette
    // param which is true if the user clicked the modal button for that. However, if the running
    // processes in this window have finished since the modal popped, there is nothing to do now and we
    // can return early
    if !open_navigation_palette || sessions_summary.long_running_cmds.is_empty() {
        return;
    }

    let windowing_model = ctx.windows();
    let active_window_id = windowing_model.active_window();
    // show the nav palette in the active window. if there is no active window,
    // arbitrarily pick one of the windows having a running process
    let window_id_to_focus = active_window_id.unwrap_or_else(|| {
        *sessions_summary
            .windows_running()
            .iter()
            .next()
            .expect("already checked len > 0")
    });

    windowing_model.show_window_and_focus_app(window_id_to_focus);

    // open the nav palette in the selected window
    if let Some(workspaces) = ctx.views_of_type::<Workspace>(window_id_to_focus) {
        if let Some(handle) = workspaces.first() {
            ctx.dispatch_typed_action_for_view(
                window_id_to_focus,
                handle.id(),
                &WorkspaceAction::OpenPalette {
                    mode: PaletteMode::Navigation,
                    source: PaletteSource::QuitModal,
                    query: Some("running".to_owned()),
                },
            );
        }
    }
}

fn on_close_window_cancelled(
    window_id: WindowId,
    open_navigation_palette: bool,
    ctx: &mut AppContext,
) {
    let sessions = SessionNavigationData::all_sessions(ctx).collect_vec();
    let sessions_summary = RunningSessionSummary::new(&sessions);
    let num_processes_in_window = sessions_summary.processes_in_window(&window_id).len();

    // If open_navigation_palette is false, return early. Otherwise, we honor the
    // open_navigation_palette param which is true if the user clicked the modal
    // button for that. However, if the running processes in this window have finished
    // since the modal popped, there is nothing to do now and we can return early
    if !open_navigation_palette || num_processes_in_window == 0 {
        return;
    }

    ctx.windows().show_window_and_focus_app(window_id);

    // if we haven't returned early, it means open_navigation_palette is true as the
    // user pressed the modal button for opening the navigation palette to show their
    // running processes
    if let Some(workspaces) = ctx.views_of_type::<Workspace>(window_id) {
        if let Some(handle) = workspaces.first() {
            ctx.dispatch_typed_action_for_view(
                window_id,
                handle.id(),
                &WorkspaceAction::OpenPalette {
                    mode: PaletteMode::Navigation,
                    source: PaletteSource::QuitModal,
                    query: Some("running".to_owned()),
                },
            );
        }
    }
}

#[::tracing::instrument(skip_all, fields(tags.cloud_agent = true))]
fn launch(ctx: &mut uncaged_ui::AppContext, app_state: Option<AppState>, launch_mode: LaunchMode) {
    IntervalTimer::handle(ctx).update(ctx, |timer, _ctx| {
        timer.mark_interval_end("APP_LAUNCHED");
    });

    keyboard::load_custom_keybindings(ctx);

    IntervalTimer::handle(ctx).update(ctx, |timer, _ctx| {
        timer.mark_interval_end("KEYBINDINGS_LOADED");
    });

    // Attempt to restore windows from the persisted application state.
    let arg = OpenFromRestoredArg { app_state };
    ctx.dispatch_global_action("root_view:open_from_restored", &arg);

    // Process any URLs that were provided on the command line (which may be
    // file:// URLs or ones using our custom URL scheme).
    for url in launch_mode.args().urls.iter() {
        uri::handle_incoming_uri(url, ctx);
    }

    // If, after session restoration and command-line argument handling, we
    // haven't opened any windows, open a new window.
    if ctx.window_ids().count() == 0 {
        ctx.dispatch_global_action("root_view:open_new", &());
    }

    IntervalTimer::handle(ctx).update(ctx, |timer, _| {
        timer.mark_interval_end("WINDOWS_CREATED");
    });

    // TODO(ben): We should skip this for LaunchMode::Test.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        use crate::login_item::maybe_register_app_as_login_item;
        use crate::terminal::general_settings::GeneralSettingsChangedEvent;
        // Note that we put this here because it depends on settings already having been initialized.
        ctx.subscribe_to_model(&GeneralSettings::handle(ctx), |_, event, ctx| {
            if matches!(event, GeneralSettingsChangedEvent::LoginItem { .. }) {
                maybe_register_app_as_login_item(ctx);
            }
        });
        maybe_register_app_as_login_item(ctx);
    }
}

/// Initializes the logger before running tests.
///
/// The `ctor` attribute here means that this runs BEFORE main(), whenever the
/// binary is executed. For this reason, we need to ensure that this function
/// only exists within unit test code. Production bundles and integration tests
/// also initialize the logging system, and initializing it twice causes a panic.
///
/// Additionally, we must not write anything to stdout in this function, as it
/// can interfere with test harnesses collecting the set of tests to run. (This
/// is why we're not simply calling the init() function above.)
#[ctor::ctor]
#[cfg(test)]
fn init_logging_for_unit_tests_glue() {
    // Initialize terminal-friendly logging for tests from the shared logger crate.
    uncaged_logging::init_logging_for_unit_tests();
}
