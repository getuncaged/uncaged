use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::SyncSender;
use std::sync::Arc;

use anyhow::Result;
use lazy_static::lazy_static;
use onboarding::{
    AgentOnboardingEvent, AgentOnboardingView, ConnectConnectionView, ConnectGalleryData,
    ConnectPresetView, ConnectSectionView, OnboardingIntention,
};
use parking_lot::Mutex;
use pathfinder_geometry::rect::RectF;
use pathfinder_geometry::vector::{vec2f, Vector2F};
use serde::{Deserialize, Serialize};
use settings::Setting as _;
use warp_core::context_flag::ContextFlag;
use warp_core::user_preferences::GetUserPreferences as _;
use warpui::elements::{
    ChildAnchor, OffsetPositioning, ParentAnchor, ParentElement, ParentOffsetBounds, Stack,
};
use warpui::keymap::{EditableBinding, FixedBinding};
use warpui::platform::{WindowBounds, WindowStyle};
use warpui::presenter::ChildView;
use warpui::windowing::WindowManager;
use warpui::{
    id, AddWindowOptions, AppContext, DisplayId, Element, Entity, EntityId, FocusContext,
    NextNewWindowsHasThisWindowsBoundsUponClose, SingletonEntity, TypedActionView, View,
    ViewContext, ViewHandle, WindowId,
};

use crate::ai::agent::api::ServerConversationToken;
use crate::ai::blocklist::SerializedBlockListItem;
use crate::ai::llms::{LLMPreferences, LLMPreferencesEvent};
use crate::ai::onboarding::build_onboarding_models;
use crate::app_state::{AppState, PaneUuid, WindowSnapshot};
use crate::appearance::Appearance;
use crate::autoupdate::{AutoupdateState, AutoupdateStateEvent, RequestType, UpdateReady};
use crate::changelog_model::ChangelogRequestType;
use crate::cloud_object::model::persistence::CloudModel;
use crate::cloud_object::{GenericStringObjectFormat, JsonObjectType, ObjectType};
use crate::drive::items::WarpDriveItemId;
use crate::drive::{CloudObjectTypeAndId, OpenWarpDriveObjectArgs, OpenWarpDriveObjectSettings};
use crate::features::FeatureFlag;
use crate::interval_timer::IntervalTimer;
use crate::launch_configs::launch_config;
use crate::launch_splash::{self, LaunchSplash};
use crate::linear::LinearIssueWork;
use crate::notebooks::manager::NotebookSource;
use crate::pane_group::{NewTerminalOptions, PanesLayout};
use crate::persistence::ModelEvent;
use crate::server::cloud_objects::update_manager::UpdateManager;
use crate::server::ids::SyncId;
use crate::server::server_api::{ServerApi, ServerApiProvider, ServerTime};
use crate::settings::{apply_onboarding_settings, AISettings, QuakeModeSettings, ThemeSettings};
use crate::settings_view::mcp_servers_page::MCPServersSettingsPage;
use crate::settings_view::{flags, SettingsSection};
use crate::terminal::available_shells::AvailableShell;
use crate::terminal::general_settings::GeneralSettings;
use crate::terminal::keys_settings::KeysSettings;
use crate::terminal::shell::ShellType;
use crate::terminal::view::cell_size_and_padding;
use crate::themes::onboarding_theme_picker_themes;
use crate::themes::theme::{AnsiColorIdentifier, ThemeKind, WarpThemeConfig};
use crate::uri::OpenMCPSettingsArgs;
use crate::util::bindings::{self, is_binding_pty_compliant};
use crate::util::traffic_lights::{traffic_light_data, TrafficLightData, TrafficLightMouseStates};
use crate::view_components::DismissibleToast;
use crate::window_settings::WindowSettings;
use crate::workspace::hoa_onboarding::mark_hoa_onboarding_completed;
use crate::workspace::view::OnboardingTutorial;
use crate::workspace::{PaneViewLocator, Workspace, WorkspaceAction, WorkspaceRegistry};
use crate::{
    report_if_error, ChannelState, GlobalResourceHandles, GlobalResourceHandlesProvider,
    UpdateQuakeModeEventArg,
};

const WINDOW_TITLE: &str = "Uncaged";

lazy_static! {
    static ref FALLBACK_WINDOW_SIZE: Vector2F = vec2f(800.0, 600.0);
    static ref QUAKE_STATE: Arc<Mutex<Option<QuakeModeState>>> = Arc::new(Mutex::new(None));
}

#[derive(Debug, Clone)]
enum WindowState {
    /// Quake mode window is open and visible on the screen.
    Open,
    /// Quake mode window is opening but has not become the key window yet.
    /// This happens when the app is not focused when the quake mode window
    /// is opened.
    PendingOpen,
    /// Quake mode window is open but hidden away from the screen.
    /// In this state, toggling quake mode will show the hidden window rather
    /// than creating a new one.
    Hidden,
}

#[derive(Debug, Clone)]
pub struct QuakeModeState {
    /// State of the opened quake mode window.
    window_state: WindowState,
    window_id: WindowId,
    /// ID of the active screen when we last positioned the quake mode window.
    /// Note that this is not necessarily the screen quake mode lives in if user
    /// set a specific pinned screen.
    active_display_id: DisplayId,
}

/// Configuration for the new quake mode window including the active screen id and the window bound.
struct QuakeModeFrameConfig {
    display_id: DisplayId,
    window_bounds: RectF,
}

/// Trigger of a potential quake window move.
#[derive(Debug)]
enum QuakeModeMoveTrigger {
    /// The screen configuration changed (plug / unplug monitor). We need
    /// to reposition quake mode as it might be in an invalid position.
    ScreenConfigurationChange,
    /// User set "active screen" as the screen to pin to. In this case,
    /// we will attempt to move the quake window if the active screen dimension
    /// changed. If it hasn't change, we will keep the window as is to avoid
    /// meaningless resizing.
    ActiveScreenSetting,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Hash,
    Eq,
    PartialEq,
    Deserialize,
    Serialize,
    Default,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(
    description = "Screen edge to pin the hotkey window to.",
    rename_all = "snake_case"
)]
pub enum QuakeModePinPosition {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}

pub struct OpenFromRestoredArg {
    pub app_state: Option<AppState>,
}

pub struct OpenLaunchConfigArg {
    pub launch_config: launch_config::LaunchConfig,

    /// Tries to open the launch config into the active window, if any.
    ///
    /// Currently, this is only supported by single-window launch configs
    /// and will open the window tabs into the existing window when true.
    pub open_in_active_window: bool,
}

pub struct OpenPath {
    pub path: PathBuf,
}

// Arguments for actions that run a command that should start a subshell.
pub struct SubshellCommandArg {
    pub command: String,
    pub shell_type: Option<ShellType>,
}

pub fn init(app: &mut AppContext) {
    app.register_binding_validator::<RootView>(is_binding_pty_compliant);

    app.add_global_action("root_view:open_from_restored", open_from_restored);
    app.add_global_action("root_view:open_new", open_new);
    app.add_global_action("root_view:open_new_with_shell", open_new_with_shell);
    app.add_global_action("root_view:open_new_from_path", |arg, ctx| {
        let _ = open_new_from_path(arg, ctx);
    });
    app.add_global_action(
        "root_view:open_new_tab_insert_subshell_command_and_bootstrap_if_supported",
        open_new_tab_insert_subshell_command_and_bootstrap_if_supported,
    );
    app.add_global_action("root_view:open_launch_config", open_launch_config);
    app.add_global_action("root_view:send_feedback", send_feedback);
    app.add_global_action(
        "root_view:toggle_quake_mode_window",
        toggle_quake_mode_window,
    );
    app.add_global_action(
        "root_view:show_or_hide_non_quake_mode_windows",
        show_or_hide_non_quake_mode_windows,
    );
    app.add_global_action("root_view:update_quake_mode_state", update_quake_mode_state);
    app.add_global_action(
        "root_view:move_quake_mode_window_from_screen_change",
        move_quake_mode_window_from_screen_change,
    );
    app.add_action(
        "root_view:add_session_at_path",
        RootView::add_session_at_path,
    );
    app.add_action(
        "root_view:handle_team_intent_link_action",
        RootView::handle_team_intent_link_action,
    );
    app.add_action(
        "root_view:handle_notification_click",
        RootView::handle_notification_click,
    );
    app.add_action(
        "root_view:handle_pane_navigation_event",
        RootView::focus_pane,
    );
    app.add_action(
        "root_view:activate_tab_by_pane_group_id",
        RootView::activate_tab_by_pane_group_id,
    );
    app.add_action("root_view:close_window", RootView::close_window);
    app.add_action("root_view:minimize_window", RootView::minimize_window);
    app.add_action(
        "root_view:toggle_maximize_window",
        RootView::toggle_maximize_window,
    );
    app.add_action("root_view:toggle_fullscreen", RootView::toggle_fullscreen);

    app.add_global_action(
        "root_view:open_conversation_viewer",
        open_conversation_viewer,
    );
    app.add_action(
        "root_view:open_cloud_conversation_in_existing_window",
        RootView::open_cloud_conversation_in_existing_window,
    );

    app.add_global_action(
        "root_view:open_drive_object_new_window",
        open_warp_drive_object,
    );
    app.add_action(
        "root_view:open_drive_object_existing_window",
        RootView::open_warp_drive_object_in_existing_window,
    );

    app.add_global_action(
        "root_view:open_settings_page_in_new_window",
        open_settings_page_in_new_window,
    );
    app.add_action(
        "root_view:open_settings_page_in_existing_window",
        RootView::open_settings_page_in_existing_window,
    );

    app.add_global_action(
        "root_view:open_mcp_settings_in_new_window",
        open_mcp_settings_in_new_window,
    );
    app.add_action(
        "root_view:open_mcp_settings_in_existing_window",
        RootView::open_mcp_settings_in_existing_window,
    );

    app.add_global_action(
        "root_view:open_codex_in_new_window",
        open_codex_in_new_window,
    );
    app.add_action(
        "root_view:open_codex_in_existing_window",
        RootView::open_codex_in_existing_window,
    );

    app.add_global_action(
        "root_view:open_linear_issue_work_in_new_window",
        open_linear_issue_work_in_new_window,
    );
    app.add_action(
        "root_view:open_linear_issue_work_in_existing_window",
        RootView::open_linear_issue_work_in_existing_window,
    );

    app.add_action("root_view:add_file_pane", RootView::add_file_pane);
    app.add_global_action(
        "root_view:open_new_with_file_notebook",
        open_new_with_file_notebook,
    );

    app.register_fixed_bindings([
        FixedBinding::empty(
            "Hide All Windows",
            RootViewAction::ShowOrHideNonQuakeModeWindows,
            id!("RootView") & id!(flags::ACTIVATION_HOTKEY_FLAG),
        ),
        FixedBinding::empty(
            "Show Dedicated Hotkey Window",
            RootViewAction::ToggleQuakeModeWindow,
            id!("RootView")
                & id!(flags::QUAKE_MODE_ENABLED_CONTEXT_FLAG)
                & !id!(flags::QUAKE_WINDOW_OPEN_FLAG),
        ),
        FixedBinding::empty(
            "Hide Dedicated Hotkey Window",
            RootViewAction::ToggleQuakeModeWindow,
            id!("RootView")
                & id!(flags::QUAKE_MODE_ENABLED_CONTEXT_FLAG)
                & id!(flags::QUAKE_WINDOW_OPEN_FLAG),
        ),
    ]);

    app.register_editable_bindings([
        // Register a binding to toggle fullscreen on Linux and Windows.
        EditableBinding::new(
            "root_view:toggle_fullscreen",
            "Toggle fullscreen",
            RootViewAction::ToggleFullscreen,
        )
        .with_group(bindings::BindingGroup::Navigation.as_str())
        .with_context_predicate(id!("RootView"))
        .with_linux_or_windows_key_binding("f11"),
        // Debug binding for onboarding state
        EditableBinding::new(
            "root_view:enter_onboarding_state",
            "[Debug] Enter Onboarding State",
            RootViewAction::DebugEnterOnboardingState,
        )
        .with_group(bindings::BindingGroup::Settings.as_str())
        .with_context_predicate(id!("RootView"))
        .with_key_binding("shift-f12")
        .with_enabled(|| {
            FeatureFlag::AgentOnboarding.is_enabled() && ChannelState::enable_debug_features()
        }),
    ])
}

fn maybe_register_global_window_shortcuts(
    global_resource_handles: GlobalResourceHandles,
    ctx: &mut AppContext,
) {
    // let keys_settings = KeysSettings::handle(ctx).as_ref(ctx);
    if let Some(key) = KeysSettings::as_ref(ctx)
        .quake_mode_settings
        .keybinding
        .clone()
        .filter(|_| *KeysSettings::as_ref(ctx).quake_mode_enabled)
    {
        ctx.register_global_shortcut(
            key.clone(),
            "root_view:toggle_quake_mode_window",
            global_resource_handles,
        );
    }

    if let Some(key) = KeysSettings::as_ref(ctx)
        .activation_hotkey_keybinding
        .clone()
        .filter(|_| *KeysSettings::as_ref(ctx).activation_hotkey_enabled)
    {
        ctx.register_global_shortcut(
            key.clone(),
            "root_view:show_or_hide_non_quake_mode_windows",
            (),
        )
    }
}

/// Find the root [`Workspace`] view for the active window.
fn active_workspace(ctx: &mut AppContext) -> Option<ViewHandle<Workspace>> {
    let window_id = ctx.windows().active_window()?;
    WorkspaceRegistry::as_ref(ctx).get(window_id, ctx)
}

fn open_launch_config(arg: &OpenLaunchConfigArg, ctx: &mut AppContext) {
    let active_window_workspace = active_workspace(ctx);
    if arg.launch_config.windows.is_empty() {
        open_new(&(), ctx);
    } else if arg.open_in_active_window
        && arg.launch_config.windows.len() == 1
        && active_window_workspace.is_some()
    {
        active_window_workspace
            .expect("already checked if there is a workspace for the active window")
            .update(ctx, |workspace, ctx| {
                workspace.open_launch_config_window(arg.launch_config.windows[0].clone(), ctx)
            });
    } else {
        let mut active_index = None;
        for (idx, window_template) in arg.launch_config.windows.iter().enumerate() {
            if arg
                .launch_config
                .active_window_index
                .map(|window_idx| window_idx == idx)
                .unwrap_or(false)
            {
                active_index = Some(idx);
            } else {
                open_new_with_workspace_source(
                    NewWorkspaceSource::FromTemplate {
                        window_template: window_template.clone(),
                    },
                    ctx,
                );
            }
        }

        if let Some(idx) = active_index {
            let window_template = arg
                .launch_config
                .windows
                .get(idx)
                .expect("Window should exist at idx");

            open_new_with_workspace_source(
                NewWorkspaceSource::FromTemplate {
                    window_template: window_template.clone(),
                },
                ctx,
            );
        }
    }
}

fn send_feedback(_: &(), ctx: &mut AppContext) {
    if let Some(workspace) = active_workspace(ctx) {
        workspace.update(ctx, |workspace, ctx| {
            workspace.handle_action(&WorkspaceAction::SendFeedback, ctx);
        });
    } else {
        ctx.open_url(&crate::util::links::feedback_form_url());
    }
}

/// Creates a new window with the transferred pane group.
///
/// If `is_tab_drag_preview` is true, the window is created without stealing
/// focus so it can follow the cursor during a tab drag.
///
/// Returns the new window ID.
pub fn create_transferred_window(
    transferred_tab: crate::workspace::view::TransferredTab,
    source_window_id: WindowId,
    window_size: Vector2F,
    window_position: Vector2F,
    is_tab_drag_preview: bool,
    ctx: &mut AppContext,
) -> WindowId {
    let global_resource_handles = GlobalResourceHandlesProvider::handle(ctx)
        .as_ref(ctx)
        .get()
        .clone();
    let window_settings = WindowSettings::handle(ctx).as_ref(ctx);

    let window_bounds = WindowBounds::ExactPosition(RectF::new(window_position, window_size));

    let window_style = if is_tab_drag_preview {
        WindowStyle::PositionedNoFocus
    } else {
        WindowStyle::Normal
    };

    let (new_window_id, _) = ctx.add_window(
        AddWindowOptions {
            window_style,
            window_bounds,
            title: Some(WINDOW_TITLE.to_owned()),
            background_blur_radius_pixels: Some(*window_settings.background_blur_radius),
            background_blur_texture: *window_settings.background_blur_texture,
            on_gpu_driver_selected: None,
            ..Default::default()
        },
        |ctx| {
            let mut view = RootView::new(
                global_resource_handles.clone(),
                NewWorkspaceSource::TransferredTab {
                    tab_color: transferred_tab.color,
                    custom_title: transferred_tab.custom_title.clone(),
                    left_panel_open: transferred_tab.left_panel_open,
                    vertical_tabs_panel_open: transferred_tab.vertical_tabs_panel_open,
                    right_panel_open: transferred_tab.right_panel_open,
                    is_right_panel_maximized: transferred_tab.is_right_panel_maximized,
                    is_tab_drag_preview,
                },
                ctx,
            );
            if !is_tab_drag_preview {
                view.focus(ctx);
            }
            view
        },
    );

    let pane_group_id = transferred_tab.pane_group.id();
    ctx.transfer_view_tree_to_window(pane_group_id, source_window_id, new_window_id);

    if let Some(new_workspace) = WorkspaceRegistry::as_ref(ctx).get(new_window_id, ctx) {
        new_workspace.update(ctx, |workspace, ctx| {
            workspace.adopt_transferred_pane_group(transferred_tab.pane_group.clone(), ctx);
        });
    } else {
        log::warn!("Failed to find workspace in newly created window {new_window_id:?}");
    }
    new_window_id
}

fn open_from_restored(arg: &OpenFromRestoredArg, ctx: &mut AppContext) {
    let global_resource_handles = GlobalResourceHandlesProvider::as_ref(ctx).get().clone();
    IntervalTimer::handle(ctx).update(ctx, |timer, _| {
        timer.mark_interval_end("HANDLING_OPEN_ACTION");
    });

    if let Some(app_state) = &arg.app_state {
        maybe_register_global_window_shortcuts(global_resource_handles.clone(), ctx);

        let (background_blur_radius_pixels, background_blur_texture) = {
            let window_settings = WindowSettings::as_ref(ctx);
            (
                Some(*window_settings.background_blur_radius),
                *window_settings.background_blur_texture,
            )
        };

        // Check whether user has enabled session restoration.
        if *GeneralSettings::as_ref(ctx).restore_session {
            let mut active_index = None;
            let mut normal_window_count = 0;
            for (idx, window) in app_state.windows.iter().enumerate() {
                // If this window is a quake window, hide it by default.
                if window.quake_mode {
                    // If this is Windows, skip restoring the quake window. Creating a hidden window
                    // is not supported on Windows. We can't have the quake window visible on
                    // startup or else it will get mistaken for a normal window.
                    if cfg!(windows) {
                        continue;
                    }
                    let frame_args = quake_mode_config(
                        &KeysSettings::as_ref(ctx)
                            .quake_mode_settings
                            .value()
                            .clone(),
                        ctx,
                    );

                    let (id, _) = ctx.add_window(
                        AddWindowOptions {
                            window_style: WindowStyle::Pin,
                            window_bounds: WindowBounds::ExactPosition(frame_args.window_bounds),
                            title: Some("Uncaged".to_owned()),
                            fullscreen_state: window.fullscreen_state,
                            background_blur_radius_pixels,
                            background_blur_texture,
                            // Don't use the quake window for positioning new windows.
                            anchor_new_windows_from_closed_position:
                                NextNewWindowsHasThisWindowsBoundsUponClose::No,
                            on_gpu_driver_selected: None,
                            window_instance: Some(ChannelState::app_id().to_string() + "-hotkey"),
                        },
                        |ctx| {
                            let mut view = RootView::new(
                                global_resource_handles.clone(),
                                NewWorkspaceSource::Restored {
                                    window_snapshot: window.clone(),
                                    block_lists: app_state.block_lists.clone(),
                                },
                                ctx,
                            );
                            view.focus(ctx);
                            view
                        },
                    );
                    ctx.windows().hide_window(id);

                    let mut quake_mode_state = QUAKE_STATE.lock();
                    *quake_mode_state = Some(QuakeModeState {
                        window_state: WindowState::Hidden,
                        window_id: id,
                        active_display_id: frame_args.display_id,
                    });
                } else {
                    normal_window_count += 1;
                    if app_state
                        .active_window_index
                        .map(|window_idx| window_idx == idx)
                        .unwrap_or(false)
                    {
                        active_index = Some(idx);
                    } else {
                        ctx.add_window(
                            AddWindowOptions {
                                window_bounds: WindowBounds::new(window.bounds),
                                title: Some("Uncaged".to_owned()),
                                fullscreen_state: window.fullscreen_state,
                                background_blur_radius_pixels,
                                background_blur_texture,
                                on_gpu_driver_selected: None,
                                ..Default::default()
                            },
                            |ctx| {
                                let mut view = RootView::new(
                                    global_resource_handles.clone(),
                                    NewWorkspaceSource::Restored {
                                        window_snapshot: window.clone(),
                                        block_lists: app_state.block_lists.clone(),
                                    },
                                    ctx,
                                );
                                view.focus(ctx);
                                view
                            },
                        );
                    }
                }
            }

            // If only the quake mode window was restored (which starts hidden), create a new normal
            // window so that something visible is created on startup.
            if normal_window_count == 0 {
                let window_settings = WindowSettings::as_ref(ctx);
                let options = default_window_options(window_settings, ctx);
                ctx.add_window(options, |ctx| {
                    let mut view = RootView::new(
                        global_resource_handles.clone(),
                        NewWorkspaceSource::Empty {
                            previous_active_window: None,
                            shell: None,
                        },
                        ctx,
                    );
                    view.focus(ctx);
                    view
                });
            }

            // Create the active window last to make sure it is focused on startup.
            if let Some(idx) = active_index {
                let window = app_state
                    .windows
                    .get(idx)
                    .expect("Window should exist at idx");
                ctx.add_window(
                    AddWindowOptions {
                        window_bounds: WindowBounds::new(window.bounds),
                        title: Some("Uncaged".to_owned()),
                        fullscreen_state: window.fullscreen_state,
                        background_blur_radius_pixels,
                        background_blur_texture,
                        on_gpu_driver_selected: None,
                        ..Default::default()
                    },
                    |ctx| {
                        let mut view = RootView::new(
                            global_resource_handles,
                            NewWorkspaceSource::Restored {
                                window_snapshot: window.clone(),
                                block_lists: app_state.block_lists.clone(),
                            },
                            ctx,
                        );
                        view.focus(ctx);
                        view
                    },
                );
            }
        }
    }
}

fn path_if_directory(path: &Path) -> Option<&Path> {
    path.is_dir().then_some(path)
}

/// Opens a new window with the workspace configured according to `source`. Returns the
/// newly-opened window ID and a handle to the root view in that window.
///
/// This is the canonical way to open a new Warp window - all other entrypoints should delegate to
/// it if possible.
pub(crate) fn open_new_with_workspace_source(
    source: NewWorkspaceSource,
    ctx: &mut AppContext,
) -> (WindowId, ViewHandle<RootView>) {
    let global_resource_handles = GlobalResourceHandlesProvider::as_ref(ctx).get().clone();
    let window_settings = WindowSettings::as_ref(ctx);
    let options = default_window_options(window_settings, ctx);
    ctx.add_window(options, |ctx| {
        let mut view = RootView::new(global_resource_handles, source, ctx);
        view.focus(ctx);
        view
    })
}

pub(crate) fn open_new_from_path(
    arg: &OpenPath,
    ctx: &mut AppContext,
) -> (WindowId, ViewHandle<RootView>) {
    open_new_with_workspace_source(
        NewWorkspaceSource::Session {
            options: Box::new(
                NewTerminalOptions::default()
                    .with_initial_directory_opt(path_if_directory(&arg.path).map(Into::into)),
            ),
        },
        ctx,
    )
}

/// Opens a new window to view a persisted view-only cloud conversation.
/// The conversation data is loaded via GraphQL API.
fn open_conversation_viewer(conversation_id: &ServerConversationToken, ctx: &mut AppContext) {
    // Trigger the workspace loading mechanism by dispatching the LoadConversationData event
    // This will open a new window with a loading state, fetch data via GraphQL, and display it
    open_new_with_workspace_source(
        NewWorkspaceSource::FromCloudConversationId {
            conversation_id: conversation_id.clone(),
        },
        ctx,
    );
}

fn open_settings_page_in_new_window(section: &SettingsSection, ctx: &mut AppContext) {
    let root_handle = open_new_window_get_handles(None, ctx).1;
    root_handle.update(ctx, |root_view, ctx| {
        if let AuthOnboardingState::Terminal(workspace_view_handle) =
            &root_view.auth_onboarding_state
        {
            let window_id = ctx.window_id();
            ctx.dispatch_typed_action_for_view(
                window_id,
                workspace_view_handle.id(),
                &WorkspaceAction::ShowSettingsPage(*section),
            );
        }
    });
}

/// MCP servers need to wait for initial load to complete, so we have this action in addition
/// to the general-purpose [`open_settings_page_in_new_window`].
fn open_mcp_settings_in_new_window(args: &OpenMCPSettingsArgs, ctx: &mut AppContext) {
    let autoinstall = args.autoinstall.clone();
    let root_handle = open_new_window_get_handles(None, ctx).1;
    root_handle.update(ctx, |root_view, ctx| {
        if let AuthOnboardingState::Terminal(workspace_view_handle) =
            &root_view.auth_onboarding_state
        {
            let initial_load_complete = UpdateManager::as_ref(ctx).initial_load_complete();
            workspace_view_handle.update(ctx, |_, ctx| {
                let _ = ctx.spawn(initial_load_complete, move |workspace, _, ctx| {
                    workspace.open_mcp_servers_page(
                        MCPServersSettingsPage::List,
                        autoinstall.as_deref(),
                        ctx,
                    )
                });
            });
        }
    });
}

/// Opens a new window and shows the Codex modal.
fn open_codex_in_new_window(_: &(), ctx: &mut AppContext) {
    let root_handle = open_new_window_get_handles(None, ctx).1;
    root_handle.update(ctx, |root_view, ctx| {
        if let AuthOnboardingState::Terminal(workspace_view_handle) =
            &root_view.auth_onboarding_state
        {
            let initial_load_complete = UpdateManager::as_ref(ctx).initial_load_complete();
            workspace_view_handle.update(ctx, |_, ctx| {
                let _ = ctx.spawn(initial_load_complete, move |workspace, _, ctx| {
                    workspace.open_codex_modal(ctx)
                });
            });
        }
    });
}

/// Opens a new window and enters agent view with the Linear issue work prompt.
fn open_linear_issue_work_in_new_window(args: &LinearIssueWork, ctx: &mut AppContext) {
    let (_, root_handle) = open_new_window_get_handles(None, ctx);
    let args = args.clone();
    root_handle.update(ctx, |root_view, ctx| {
        if let AuthOnboardingState::Terminal(workspace_view_handle) =
            &root_view.auth_onboarding_state
        {
            workspace_view_handle.update(ctx, |workspace, ctx| {
                workspace.open_linear_issue_work(&args, ctx);
            });
        }
    });
}

fn open_warp_drive_object(arg: &OpenWarpDriveObjectArgs, ctx: &mut AppContext) {
    match arg.object_type {
        ObjectType::Notebook => open_new_workspace_with_notebook_open(
            SyncId::ServerId(arg.server_id),
            arg.settings.clone(),
            ctx,
        ),
        ObjectType::Workflow => open_new_workspace_with_workflow_open(
            SyncId::ServerId(arg.server_id),
            arg.settings.clone(),
            ctx,
        ),
        _ => log::info!("Open object type {:?} not yet supported", arg.object_type),
    }
}

fn display_object_missing_error_in_window(window_id: WindowId, ctx: &mut AppContext) {
    crate::workspace::ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
        let toast = DismissibleToast::error(String::from("Resource not found or access denied"));
        toast_stack.add_ephemeral_toast(toast, window_id, ctx);
    });
}

fn open_new_workspace_with_notebook_open(
    notebook_id: SyncId,
    settings: OpenWarpDriveObjectSettings,
    ctx: &mut AppContext,
) {
    open_new_with_workspace_source(
        NewWorkspaceSource::NotebookById {
            id: notebook_id,
            settings,
        },
        ctx,
    );
}

fn open_new_workspace_with_workflow_open(
    workflow_id: SyncId,
    settings: OpenWarpDriveObjectSettings,
    ctx: &mut AppContext,
) {
    open_new_with_workspace_source(
        NewWorkspaceSource::WorkflowById {
            id: workflow_id,
            settings,
        },
        ctx,
    );
}

/// Opens a new window with a file-based notebook open.
fn open_new_with_file_notebook(arg: &PathBuf, ctx: &mut AppContext) {
    open_new_with_workspace_source(
        NewWorkspaceSource::NotebookFromFilePath {
            file_path: Some(arg.to_owned()),
        },
        ctx,
    );
}

/// Creates a new window and returns its [`WindowId`] and root view's [`ViewHandle`].
pub(crate) fn open_new_window_get_handles(
    shell: Option<AvailableShell>,
    ctx: &mut AppContext,
) -> (WindowId, ViewHandle<RootView>) {
    let active_window_id = ctx.windows().active_window();
    open_new_with_workspace_source(
        NewWorkspaceSource::Empty {
            previous_active_window: active_window_id,
            shell,
        },
        ctx,
    )
}

/// Opens a new window.
fn open_new(_: &(), ctx: &mut AppContext) {
    open_new_window_get_handles(None, ctx);
}

/// Opens a new window with a specific shell
fn open_new_with_shell(shell: &Option<AvailableShell>, ctx: &mut AppContext) {
    open_new_window_get_handles(shell.to_owned(), ctx);
}

/// Global action that performs a few steps:
/// 1. Open a new tab, or open a window if there is none.
/// 2. Set the terminal input buffer to a command that should open a subshell
/// 3. Set a flag that we should automatically bootstrap that subshell if its we can bootstrap its
/// [`ShellType`].
fn open_new_tab_insert_subshell_command_and_bootstrap_if_supported(
    arg: &SubshellCommandArg,
    ctx: &mut AppContext,
) {
    let root_view_handle: Option<ViewHandle<RootView>> = ctx
        .windows()
        .frontmost_window_id()
        .and_then(|window_id| ctx.root_view(window_id));

    let root_view_handle = match root_view_handle {
        Some(root_view_handle) => {
            root_view_handle.update(ctx, |root_view, ctx| {
                if let AuthOnboardingState::Terminal(workspace_view_handle) =
                    &root_view.auth_onboarding_state
                {
                    workspace_view_handle.update(ctx, |workspace, ctx| {
                        workspace.add_terminal_tab(false /* hide_homepage */, ctx);
                    });
                }
            });
            root_view_handle
        }
        None => open_new_window_get_handles(None, ctx).1,
    };

    root_view_handle.update(ctx, |root_view, ctx| {
        root_view.insert_subshell_command_and_bootstrap_if_supported(arg, ctx);
    });
}

/// Returns the common configuration for a new "regular" window (not Quake Mode).
fn default_window_options(window_settings: &WindowSettings, ctx: &AppContext) -> AddWindowOptions {
    let (inherited_bounds, window_style) = ctx.next_window_bounds_and_style();
    let next_bounds =
        bounds_for_opening_at_custom_window_size(inherited_bounds, window_settings, ctx);

    AddWindowOptions {
        window_style,
        window_bounds: next_bounds,
        title: Some("Uncaged".to_owned()),
        background_blur_radius_pixels: Some(*window_settings.background_blur_radius),
        background_blur_texture: *window_settings.background_blur_texture,
        on_gpu_driver_selected: None,
        ..Default::default()
    }
}

/// Returns the bounds to open the next window at taking into account whether
/// the user has configured their settings to open windows at a custom size
/// and whether that feature is flagged on.
fn bounds_for_opening_at_custom_window_size(
    bounds: WindowBounds,
    window_settings: &WindowSettings,
    app: &AppContext,
) -> WindowBounds {
    if *window_settings.open_windows_at_custom_size.value() {
        let font_cache = app.font_cache();
        let appearance = Appearance::as_ref(app);

        let cell_size_and_padding = cell_size_and_padding(
            font_cache,
            appearance.monospace_font_family(),
            appearance.monospace_font_size(),
            appearance.ui_builder().line_height_ratio(),
        );
        let window_size = vec2f(
            *window_settings.new_windows_num_columns.value() as f32
                * cell_size_and_padding.cell_width_px.as_f32()
                + 2. * cell_size_and_padding.padding_x_px.as_f32(),
            *window_settings.new_windows_num_rows.value() as f32
                * cell_size_and_padding.cell_height_px.as_f32()
                + 2. * cell_size_and_padding.padding_y_px.as_f32(),
        );

        match bounds {
            WindowBounds::ExactPosition(rect) => {
                WindowBounds::ExactPosition(RectF::new(rect.origin(), window_size))
            }
            WindowBounds::ExactSize(_) | WindowBounds::Default => {
                WindowBounds::ExactSize(window_size)
            }
        }
    } else {
        bounds
    }
}

pub fn quake_mode_window_is_open() -> bool {
    let quake_mode_state = QUAKE_STATE.lock();

    quake_mode_state
        .as_ref()
        .map(|state| {
            matches!(
                state.window_state,
                WindowState::Open | WindowState::PendingOpen
            )
        })
        .unwrap_or_default()
}

pub fn quake_mode_window_id() -> Option<WindowId> {
    let quake_mode_state = QUAKE_STATE.lock();

    quake_mode_state.as_ref().map(|state| state.window_id)
}

pub fn set_quake_mode(new_state: Option<QuakeModeState>) {
    let mut quake_mode_state = QUAKE_STATE.lock();
    *quake_mode_state = new_state;
}

fn move_quake_mode_window_from_screen_change(settings: &QuakeModeSettings, ctx: &mut AppContext) {
    fit_quake_mode_window_within_active_screen(
        settings,
        QuakeModeMoveTrigger::ScreenConfigurationChange,
        ctx,
    )
}

/// If there exists a quake window, mutate its size and position, i.e. its bounds, to match the
/// bounds specified by the [`QuakeModeSettings`].
pub fn update_quake_window_bounds(quake_settings: &QuakeModeSettings, ctx: &mut AppContext) {
    let config = quake_mode_config(quake_settings, ctx);
    let Some(ref state) = *QUAKE_STATE.lock() else {
        return;
    };
    ctx.windows()
        .set_window_bounds(state.window_id, config.window_bounds);
}

/// Move Quake Mode window to the active screen if it is already open or hidden.
fn fit_quake_mode_window_within_active_screen(
    settings: &QuakeModeSettings,
    trigger: QuakeModeMoveTrigger,
    ctx: &mut AppContext,
) {
    let mut quake_mode_state = QUAKE_STATE.lock();

    if let Some(state) = quake_mode_state.as_mut() {
        let active_id = ctx.windows().active_display_id();

        // When there is no screen config and active screen id change, we don't need to reposition
        // the quake mode window as its position should still be valid.
        if matches!(trigger, QuakeModeMoveTrigger::ActiveScreenSetting)
            && active_id == state.active_display_id
        {
            return;
        }

        let window_bound = settings.resolve_quake_mode_bounds(ctx);
        ctx.windows()
            .set_window_bounds(state.window_id, window_bound);
        state.active_display_id = active_id;
    }
}

fn update_quake_mode_state(arg: &UpdateQuakeModeEventArg, ctx: &mut AppContext) {
    if !KeysSettings::as_ref(ctx)
        .quake_mode_settings
        .hide_window_when_unfocused
    {
        return;
    }

    {
        let mut quake_mode_state = QUAKE_STATE.lock();

        if let Some(state) = quake_mode_state.as_mut() {
            state.window_state = match state.window_state {
                WindowState::PendingOpen => WindowState::Open,
                WindowState::Open => {
                    if arg.active_window_id.is_some_and(|id| id == state.window_id) {
                        WindowState::Open
                    } else {
                        ctx.windows().hide_window(state.window_id);
                        WindowState::Hidden
                    }
                }
                WindowState::Hidden => WindowState::Hidden,
            }
        }
    }
}

// Configuration of the next positioning of the quake mode window.
fn quake_mode_config(settings: &QuakeModeSettings, ctx: &mut AppContext) -> QuakeModeFrameConfig {
    QuakeModeFrameConfig {
        display_id: ctx.windows().active_display_id(),
        window_bounds: settings.resolve_quake_mode_bounds(ctx),
    }
}

fn get_quake_mode_state(ctx: &mut AppContext) -> Option<QuakeModeState> {
    let quake_mode_state = QUAKE_STATE.lock();

    match quake_mode_state.as_ref() {
        Some(state) if ctx.is_window_open(state.window_id) => Some(state.clone()),
        _ => None,
    }
}

fn toggle_quake_mode_window(global_resource_handles: &GlobalResourceHandles, ctx: &mut AppContext) {
    // Get the current state of quake mode.
    let state = get_quake_mode_state(ctx);
    match state {
        None => {
            let config = quake_mode_config(
                &KeysSettings::as_ref(ctx)
                    .quake_mode_settings
                    .value()
                    .clone(),
                ctx,
            );

            let window_settings = WindowSettings::as_ref(ctx);

            let active_window_id = ctx.windows().active_window();
            let (id, _) = ctx.add_window(
                AddWindowOptions {
                    window_style: WindowStyle::Pin,
                    window_bounds: WindowBounds::ExactPosition(config.window_bounds),
                    title: Some("Uncaged".to_owned()),
                    background_blur_radius_pixels: Some(*window_settings.background_blur_radius),
                    background_blur_texture: *window_settings.background_blur_texture,
                    // Ignore the quake window for positioning the next window
                    anchor_new_windows_from_closed_position:
                        warpui::NextNewWindowsHasThisWindowsBoundsUponClose::No,
                    on_gpu_driver_selected: None,
                    window_instance: Some(ChannelState::app_id().to_string() + "-hotkey"),
                    ..Default::default()
                },
                |ctx| {
                    let mut view = RootView::new(
                        global_resource_handles.clone(),
                        NewWorkspaceSource::Empty {
                            previous_active_window: active_window_id,
                            shell: None,
                        },
                        ctx,
                    );
                    view.focus(ctx);
                    view
                },
            );

            // Update quake mode state after the call to prevent deadlocking.
            let mut quake_mode_state = QUAKE_STATE.lock();
            *quake_mode_state = Some(QuakeModeState {
                window_state: WindowState::PendingOpen,
                window_id: id,
                active_display_id: config.display_id,
            });
        }
        Some(state) if matches!(state.window_state, WindowState::Hidden) => {
            // If quake mode does not have a set pin screen -- move it to the current active screen.
            if KeysSettings::as_ref(ctx)
                .quake_mode_settings
                .pin_screen
                .is_none()
            {
                fit_quake_mode_window_within_active_screen(
                    &KeysSettings::as_ref(ctx)
                        .quake_mode_settings
                        .value()
                        .clone(),
                    QuakeModeMoveTrigger::ActiveScreenSetting,
                    ctx,
                );
            }
            ctx.windows().show_window_and_focus_app(state.window_id);

            // Update quake mode state after the call to prevent deadlocking.
            let mut quake_mode_state = QUAKE_STATE.lock();

            if let Some(state) = quake_mode_state.as_mut() {
                state.window_state = WindowState::PendingOpen;
            }
        }
        Some(state) => {
            ctx.windows().hide_window(state.window_id);

            // Update quake mode state after the call to prevent deadlocking.
            let mut quake_mode_state = QUAKE_STATE.lock();

            if let Some(state) = quake_mode_state.as_mut() {
                state.window_state = WindowState::Hidden;
            }
        }
    };
}

/// This action will show or hide all of Warp's windows except the quake window
///
/// - If Warp is active and has any windows, hide those windows.
/// - If Warp is hidden, show all windows.
/// - If Warp is active but has 0 normal windows, create a new window with a new session.
fn show_or_hide_non_quake_mode_windows(_: &(), ctx: &mut AppContext) {
    let quake_window_id = get_quake_mode_state(ctx).map(|state| state.window_id);
    let non_quake_mode_window_ids = ctx
        .window_ids()
        .filter(|window_id| Some(window_id) != quake_window_id.as_ref());
    if non_quake_mode_window_ids.count() == 0 {
        // If there are no normal windows, this action should create one.
        open_new(&(), ctx);
    }
    let windowing_model = ctx.windows();
    // Now there is at least one window. If a Warp window is active, hide the app.
    // Otherwise, show activate the app to show it in front.
    let active_window_id = windowing_model.active_window();
    match active_window_id {
        Some(_) => windowing_model.hide_app(),
        None => {
            windowing_model.activate_app();
        }
    };
}

#[derive(Clone)]
pub enum NewWorkspaceSource {
    Empty {
        previous_active_window: Option<WindowId>,
        shell: Option<AvailableShell>,
    },
    FromTemplate {
        window_template: launch_config::WindowTemplate,
    },
    Restored {
        window_snapshot: WindowSnapshot,
        block_lists: Arc<HashMap<PaneUuid, Vec<SerializedBlockListItem>>>,
    },
    Session {
        options: Box<NewTerminalOptions>,
    },
    FromCloudConversationId {
        conversation_id: ServerConversationToken,
    },
    NotebookFromFilePath {
        file_path: Option<PathBuf>,
    },
    NotebookById {
        id: SyncId,
        settings: OpenWarpDriveObjectSettings,
    },
    WorkflowById {
        id: SyncId,
        settings: OpenWarpDriveObjectSettings,
    },
    AgentSession {
        options: Box<NewTerminalOptions>,
        initial_query: Option<String>,
    },
    /// A tab is being transferred from another window via the transferable views framework.
    /// The workspace will create a placeholder tab, which will be replaced by the transferred
    /// PaneGroup after window creation.
    TransferredTab {
        /// Tab color from the source tab
        tab_color: Option<AnsiColorIdentifier>,
        /// Custom title from the source tab
        custom_title: Option<String>,
        /// Whether the left panel was open in the source tab
        left_panel_open: bool,
        /// Captured from the source window so detached tabs inherit the panel state.
        vertical_tabs_panel_open: bool,
        /// Whether the right panel was open in the source tab
        right_panel_open: bool,
        /// Whether the right panel was maximized in the source tab
        is_right_panel_maximized: bool,
        /// Whether this transferred tab window is currently being used as a drag preview.
        is_tab_drag_preview: bool,
    },
}

impl NewWorkspaceSource {
    pub fn has_horizontal_split(&self) -> bool {
        match self {
            NewWorkspaceSource::Restored {
                window_snapshot, ..
            } => {
                if window_snapshot.tabs.is_empty() {
                    false
                } else {
                    let active_index = window_snapshot.active_tab_index;
                    let active_tab = window_snapshot
                        .tabs
                        .get(active_index)
                        .unwrap_or(&window_snapshot.tabs[0]);
                    active_tab.root.has_horizontal_split()
                }
            }
            _ => false,
        }
    }
}

/// Args needed to construct a `Workspace`.
#[derive(Clone)]
struct WorkspaceArgs {
    global_resource_handles: GlobalResourceHandles,
    server_time: Option<Arc<ServerTime>>,
    workspace_setting: NewWorkspaceSource,
}

// Some onboarding states can either contain a ref to an existing terminal view
// if it exists or, if it doesn't, the args needed to create a new empty one.
#[derive(Clone)]
enum AuthOnboardingTarget {
    Workspace(Box<WorkspaceArgs>),
    Terminal(ViewHandle<Workspace>),
}

/// User preferences key to track whether the user has completed the onboarding slides locally
/// (before login). This is needed because the server-side `is_onboarded` flag requires
/// authentication.
const HAS_COMPLETED_ONBOARDING_KEY: &str = "HasCompletedOnboarding";

/// Returns whether the user has completed the onboarding slides locally (before login).
pub(crate) fn has_completed_local_onboarding(ctx: &AppContext) -> bool {
    ctx.private_user_preferences()
        .read_value(HAS_COMPLETED_ONBOARDING_KEY)
        .unwrap_or_default()
        .and_then(|s| serde_json::from_str::<bool>(&s).ok())
        .unwrap_or(false)
}

/// Persists the local onboarding-completed flag so we don't show onboarding again.
fn mark_local_onboarding_completed(ctx: &AppContext) {
    let _ = ctx.private_user_preferences().write_value(
        HAS_COMPLETED_ONBOARDING_KEY,
        serde_json::to_string(&true).expect("bool serializes to JSON"),
    );
}

/// Whether onboarding has completed and we should render the `Workspace`.
enum AuthOnboardingState {
    Onboarding {
        onboarding_view: ViewHandle<AgentOnboardingView>,
        target: AuthOnboardingTarget,
    },
    Terminal(ViewHandle<Workspace>),
}

pub struct RootView {
    auth_onboarding_state: AuthOnboardingState,
    server_time: Option<Arc<ServerTime>>,
    pub server_api: Arc<ServerApi>,
    pub model_event_sender: Option<SyncSender<ModelEvent>>,
    mouse_states: TrafficLightMouseStates,
    /// The window ID is needed because the "maximize" button needs to change its icon based on
    /// whether or not the current window is maximized. Ideally the window ID could just be fetched
    /// in the [`Self::render`] method, but there is no [`ViewContext`] available there. So, we
    /// need to store it in a field instead.
    window_id: WindowId,
    /// Stores the tutorial from onboarding until the workspace exists to start it.
    pending_tutorial: Option<OnboardingTutorial>,
    /// Uncaged: the mark shown over the first window while the app starts.
    launch_splash: Option<LaunchSplash>,
}

impl RootView {
    /// Redraw the launch splash on the next frame, and drop it once it has faded out.
    fn schedule_launch_splash_frame(ctx: &mut ViewContext<Self>) {
        ctx.spawn(
            async {
                warpui::r#async::Timer::after(launch_splash::FRAME).await;
            },
            |me, _, ctx| {
                if me
                    .launch_splash
                    .as_ref()
                    .is_some_and(LaunchSplash::is_finished)
                {
                    me.launch_splash = None;
                }
                ctx.notify();
                if me.launch_splash.is_some() {
                    Self::schedule_launch_splash_frame(ctx);
                }
            },
        );
    }
}

impl RootView {
    pub fn new(
        global_resource_handles: GlobalResourceHandles,
        workspace_setting: NewWorkspaceSource,
        ctx: &mut ViewContext<Self>,
    ) -> Self {
        let server_api_provider = ServerApiProvider::as_ref(ctx);
        let server_api = server_api_provider.get();

        let model_event_sender = global_resource_handles.model_event_sender.clone();
        let workspace_args = WorkspaceArgs {
            global_resource_handles,
            server_time: None,
            workspace_setting,
        };

        // When OpenWarpNewSettingsModes is enabled, show onboarding to users who haven't
        // completed it yet (tracked via a local UserPreferences key).
        let has_completed_local_onboarding = FeatureFlag::OpenWarpNewSettingsModes.is_enabled()
            && has_completed_local_onboarding(ctx);
        let should_show_onboarding = FeatureFlag::OpenWarpNewSettingsModes.is_enabled()
            && FeatureFlag::AgentOnboarding.is_enabled()
            && !has_completed_local_onboarding;
        // Uncaged: show onboarding on first run, otherwise drop straight into a usable
        // terminal. AI is powered by the local engine, so no account is needed.
        let auth_onboarding_state = if should_show_onboarding {
            let workspace_args_box: Box<WorkspaceArgs> = workspace_args.into();
            let onboarding_view = Self::create_agent_onboarding_view(ctx);
            onboarding_view.update(ctx, |view, ctx| {
                view.start_onboarding(ctx);
            });
            AuthOnboardingState::Onboarding {
                onboarding_view,
                target: AuthOnboardingTarget::Workspace(workspace_args_box),
            }
        } else {
            AuthOnboardingState::Terminal(workspace_args.create_workspace(ctx))
        };

        let auth_onboarding_state_is_terminal =
            matches!(auth_onboarding_state, AuthOnboardingState::Terminal(_));
        let root_view = Self {
            auth_onboarding_state,
            server_time: None,
            server_api: server_api.clone(),
            model_event_sender,
            mouse_states: Default::default(),
            window_id: ctx.window_id(),
            pending_tutorial: None,
            // First-run onboarding has its own welcome, so the splash is for the terminal.
            launch_splash: auth_onboarding_state_is_terminal
                .then(|| LaunchSplash::for_first_window(ctx))
                .flatten(),
        };
        if root_view.launch_splash.is_some() {
            Self::schedule_launch_splash_frame(ctx);
        }

        match &root_view.auth_onboarding_state {
            AuthOnboardingState::Terminal(workspace) if FeatureFlag::Changelog.is_enabled() => {
                // Only show the changelog if we aren't about to launch onboarding
                workspace.update(ctx, |workspace, ctx| {
                    workspace.check_for_changelog(ChangelogRequestType::WindowLaunch, ctx);
                })
            }
            AuthOnboardingState::Terminal(_) | AuthOnboardingState::Onboarding { .. } => {}
        }

        let autoupdate_handle = AutoupdateState::handle(ctx);
        ctx.subscribe_to_model(&autoupdate_handle, |root_view, _handle, evt, ctx| {
            if let AutoupdateStateEvent::CheckComplete {
                result,
                request_type: RequestType::Poll,
            } = evt
            {
                root_view.polling_update_check_complete(result, ctx)
            }
        });

        // Ensure the onboarding view has focus after all views are created, so keyboard input
        // (Enter, arrow keys) routes to onboarding.
        if let AuthOnboardingState::Onboarding {
            onboarding_view, ..
        } = &root_view.auth_onboarding_state
        {
            ctx.focus(onboarding_view);
        }

        // For users who bypass onboarding (already onboarded, or onboarding flags not active),
        // start autoupdate polling immediately. For new users in onboarding, this is a no-op;
        // polling will be started once onboarding completes.
        root_view.start_autoupdate_polling(ctx);

        root_view
    }

    /// Starts the autoupdate polling loop, but only if we are already in the `Terminal` state
    /// (i.e. onboarding has completed or was not shown). Safe to call unconditionally — it is
    /// a no-op when still in a pre-terminal state.
    fn start_autoupdate_polling(&self, ctx: &mut ViewContext<Self>) {
        if matches!(self.auth_onboarding_state, AuthOnboardingState::Terminal(_)) {
            AutoupdateState::handle(ctx).update(ctx, |state, ctx| state.start_polling(ctx));
        }
    }

    /// Used for integration tests.
    pub fn workspace_view(&self) -> Option<&ViewHandle<Workspace>> {
        match &self.auth_onboarding_state {
            AuthOnboardingState::Terminal(workspace) => Some(workspace),
            _ => None,
        }
    }

    fn polling_update_check_complete(
        &mut self,
        result: &Result<UpdateReady>,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Ok(UpdateReady::Yes {
            ref new_version, ..
        }) = result
        {
            log::info!("Update ready for channel version {new_version:?}");
            if new_version.update_by.is_some() {
                log::info!("Update ready, there is an update-by time, checking for server time.");
                let server_api = self.server_api.clone();
                let _ = ctx.spawn(
                    async move { server_api.server_time().await },
                    Self::server_time_updated,
                );
            }
        }
    }

    fn server_time_updated(
        &mut self,
        server_time: Result<ServerTime>,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Ok(server_time) = server_time {
            let server_time = Arc::new(server_time);
            self.server_time = Some(server_time.clone());

            if let AuthOnboardingState::Terminal(workspace) = &self.auth_onboarding_state {
                workspace.update(ctx, |workspace, ctx| {
                    workspace.set_server_time(server_time);
                    ctx.notify();
                })
            }
        } else {
            log::error!("Error fetching server time {:?}", server_time.err());
        }
    }

    fn close_window(&mut self, _: &(), ctx: &mut ViewContext<Self>) -> bool {
        if ContextFlag::CloseWindow.is_enabled() {
            ctx.close_window();
        }
        true
    }

    fn toggle_maximize_window(&mut self, _: &(), ctx: &mut ViewContext<Self>) -> bool {
        ctx.toggle_maximized_window();
        true
    }

    fn toggle_fullscreen(&mut self, _: &(), ctx: &mut ViewContext<Self>) -> bool {
        let window_id = ctx.window_id();
        WindowManager::handle(ctx).update(ctx, |state, ctx| {
            state.toggle_fullscreen(window_id, ctx);
        });
        true
    }

    fn create_agent_onboarding_view(
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<AgentOnboardingView> {
        LLMPreferences::handle(ctx).update(ctx, |prefs, ctx| {
            prefs.refresh_available_models(ctx);
        });

        let themes = onboarding_theme_picker_themes();
        let onboarding_view = ctx.add_typed_action_view(move |ctx| {
            let (models, default_model_id) = build_onboarding_models(LLMPreferences::as_ref(ctx));

            AgentOnboardingView::new(
                themes.clone(),
                false, // Always use unskippable onboarding.
                models,
                default_model_id,
                FeatureFlag::AgentView.is_enabled(),
                ctx,
            )
        });

        let onboarding_view_clone = onboarding_view.clone();
        ctx.subscribe_to_model(
            &LLMPreferences::handle(ctx),
            move |_, llm_preferences, event, ctx| match event {
                LLMPreferencesEvent::UpdatedAvailableLLMs => {
                    let (models, default_model_id) =
                        build_onboarding_models(llm_preferences.as_ref(ctx));
                    onboarding_view_clone.update(ctx, |onboarding_view, ctx| {
                        onboarding_view.set_onboarding_models(models, default_model_id, ctx);
                    })
                }

                LLMPreferencesEvent::UpdatedActiveAgentModeLLM
                | LLMPreferencesEvent::UpdatedActiveCodingLLM => {}
            },
        );

        ctx.subscribe_to_view(&onboarding_view, |me, _view, event, ctx| {
            me.handle_agent_onboarding_event(event, ctx);
        });

        // Seed the onboarding "Connect your model" gallery with the current
        // catalog + roster so it renders real, connectable models.
        let gallery = Self::build_connect_gallery();
        onboarding_view.update(ctx, |onboarding_view, ctx| {
            onboarding_view.set_connect_gallery(gallery, ctx);
        });

        onboarding_view
    }

    /// Convert the app-crate uncaged bridge (`catalog_sections` / `connections` /
    /// `engine_active`) into the onboarding crate's plain-data gallery structs,
    /// stamping each preset / connection with its vendor icon. The onboarding
    /// crate can't call the bridge itself, so root_view builds this and pushes it
    /// in — and rebuilds it after every connect/activate to refresh live.
    fn build_connect_gallery() -> ConnectGalleryData {
        let sections = crate::uncaged::catalog_sections()
            .into_iter()
            .map(|section| ConnectSectionView {
                title: section.title,
                subtitle: section.subtitle,
                presets: section
                    .presets
                    .into_iter()
                    .map(|preset| ConnectPresetView {
                        icon: crate::uncaged::preset_icon(&preset.id),
                        id: preset.id,
                        label: preset.label,
                        blurb: preset.blurb,
                        local: preset.local,
                        needs_key: preset.needs_key,
                        wire: preset.wire,
                    })
                    .collect(),
            })
            .collect();

        let connections = crate::uncaged::connections()
            .into_iter()
            .map(|conn| ConnectConnectionView {
                icon: crate::uncaged::preset_icon(&conn.preset),
                id: conn.id,
                preset: conn.preset,
                label: conn.label,
                endpoint: conn.endpoint,
                model: conn.model,
                status: conn.status,
                local: conn.local,
                usable: conn.usable,
                is_active: conn.is_active,
            })
            .collect();

        ConnectGalleryData {
            sections,
            connections,
            engine_available: crate::uncaged::engine_active(),
        }
    }

    /// Rebuild the connect gallery from the (now-updated) roster and push it into
    /// the active onboarding view, so a connect/activate reflects immediately.
    fn refresh_onboarding_connect_gallery(&mut self, ctx: &mut ViewContext<Self>) {
        let AuthOnboardingState::Onboarding {
            onboarding_view, ..
        } = &self.auth_onboarding_state
        else {
            return;
        };
        let onboarding_view = onboarding_view.clone();
        let gallery = Self::build_connect_gallery();
        onboarding_view.update(ctx, |onboarding_view, ctx| {
            onboarding_view.set_connect_gallery(gallery, ctx);
        });
    }

    /// Debug method to enter the onboarding state.
    fn debug_enter_onboarding_state(&mut self, _: &(), ctx: &mut ViewContext<Self>) -> bool {
        if !ChannelState::enable_debug_features() {
            log::warn!("Attempted to enter onboarding state in release build");
            return false;
        }

        if !FeatureFlag::AgentOnboarding.is_enabled() {
            log::warn!("Attempted to enter onboarding state without AgentOnboarding enabled");
            return false;
        }

        self.auth_onboarding_state.try_open_onboarding_slides(ctx);

        ctx.emit(RootViewEvent::AuthOnboardingStateChanged);
        ctx.notify();
        true
    }

    fn onboarding_theme_kind(theme_name: &str) -> Option<ThemeKind> {
        WarpThemeConfig::new()
            .theme_items()
            .find_map(|(kind, theme)| {
                (theme.name().as_deref() == Some(theme_name)).then(|| kind.clone())
            })
    }

    fn handle_agent_onboarding_event(
        &mut self,
        event: &AgentOnboardingEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            AgentOnboardingEvent::ThemeSelected { theme_name } => {
                let Some(theme_kind) = Self::onboarding_theme_kind(theme_name) else {
                    log::warn!("Unknown onboarding theme selected: {theme_name}");
                    return;
                };

                // Update both what we render with immediately, and the user's theme setting.
                ThemeSettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings.use_system_theme.set_value(false, ctx));
                    report_if_error!(settings.theme_kind.set_value(theme_kind.clone(), ctx));
                });
            }
            AgentOnboardingEvent::SyncWithOsToggled { enabled } => {
                ThemeSettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings.use_system_theme.set_value(*enabled, ctx));
                });
            }
            AgentOnboardingEvent::OnboardingCompleted(selected_settings, wants_to_connect) => {
                let AuthOnboardingState::Onboarding { target, .. } = &self.auth_onboarding_state
                else {
                    return;
                };
                let target = target.clone();

                mark_local_onboarding_completed(ctx);
                if FeatureFlag::HOAOnboardingFlow.is_enabled() {
                    mark_hoa_onboarding_completed(ctx);
                }

                apply_onboarding_settings(selected_settings, ctx);

                let workspace = target.to_workspace(ctx);
                let tutorial = OnboardingTutorial::from(selected_settings.clone());
                self.pending_tutorial = Some(tutorial);
                self.auth_onboarding_state = AuthOnboardingState::Terminal(workspace.clone());
                ctx.emit(RootViewEvent::AuthOnboardingStateChanged);
                self.start_pending_tutorial(ctx);
                self.start_autoupdate_polling(ctx);

                // Uncaged: the Connect step now wires up a real model inline, so
                // only fall through to the full AI Models page when the user wanted
                // to connect but nothing usable ended up active — e.g. they picked
                // an API-key provider that still needs its key. If a model is already
                // live (a local runtime / CLI agent they connected), or they chose
                // "Set up later", drop straight into the terminal, ready to go.
                // We read the roster (not the engine's in-memory state, which isn't
                // initialised yet mid-onboarding) since that's what the gallery wrote.
                let has_active_model = crate::uncaged::connections()
                    .iter()
                    .any(|c| c.is_active && c.usable);
                if *wants_to_connect && !has_active_model {
                    workspace.update(ctx, |ws, ctx| {
                        ws.show_settings_with_section(Some(SettingsSection::WarpAgent), ctx);
                    });
                }

                ctx.notify();
            }
            AgentOnboardingEvent::OnboardingSkipped => {
                let AuthOnboardingState::Onboarding { target, .. } = &self.auth_onboarding_state
                else {
                    return;
                };

                mark_local_onboarding_completed(ctx);
                if FeatureFlag::HOAOnboardingFlow.is_enabled() {
                    mark_hoa_onboarding_completed(ctx);
                }

                let workspace = target.to_workspace(ctx);
                self.auth_onboarding_state = AuthOnboardingState::Terminal(workspace);
                ctx.emit(RootViewEvent::AuthOnboardingStateChanged);
                self.start_autoupdate_polling(ctx);
                ctx.notify();
            }
            AgentOnboardingEvent::ConnectPresetRequested(preset_id) => {
                // Mirror ai_page.rs::uncaged_connect_preset: create (or focus) a
                // connection for this preset; when it's immediately usable and
                // needs no key (local runtimes, CLI agents), activate it in one
                // click. Presets that need an API key create an incomplete
                // connection here and surface in the "Connected" roster with a
                // "Needs key" status — the user finishes them in Settings after
                // onboarding. (TODO: host the CustomEndpointModal as an overlay
                // to collect the key inline — plan step 6.)
                if let Ok(id) = crate::uncaged::connect(preset_id) {
                    if let Some(conn) = crate::uncaged::connections()
                        .into_iter()
                        .find(|c| c.id == id)
                    {
                        if conn.usable && !conn.needs_key {
                            let _ = crate::uncaged::activate(&conn.id);
                        }
                    }
                }
                self.refresh_onboarding_connect_gallery(ctx);
            }
            AgentOnboardingEvent::ActivateConnectionRequested(id) => {
                let _ = crate::uncaged::activate(id);
                self.refresh_onboarding_connect_gallery(ctx);
            }
            AgentOnboardingEvent::AppBecameActive => {
                // fetch the models when the user tabs/intents back into the app during
                // onboarding after potentially upgrading
                LLMPreferences::handle(ctx).update(ctx, |prefs, ctx| {
                    prefs.refresh_available_models(ctx);
                });
            }
        }
    }

    fn minimize_window(&mut self, _: &(), ctx: &mut ViewContext<Self>) -> bool {
        ctx.minimize_window();
        true
    }

    fn focus_pane(
        &mut self,
        pane_view_locator: &PaneViewLocator,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        // Focus the appropriate window.
        let window_id = ctx.window_id();

        let mut quake_mode_state = QUAKE_STATE.lock();
        // If the window we are focusing is the Quake Mode window, then update the QuakeModeState.
        if let Some(mode) = quake_mode_state.as_mut() {
            if mode.window_id == window_id {
                mode.window_state = WindowState::Open;
            }
        }

        ctx.windows().show_window_and_focus_app(window_id);

        // Focus the appropriate tab/pane.
        if let AuthOnboardingState::Terminal(workspace) = &self.auth_onboarding_state {
            workspace.update(ctx, |view, ctx| {
                view.focus_pane(*pane_view_locator, ctx);
            });
        }
        true
    }

    fn activate_tab_by_pane_group_id(
        &mut self,
        pane_group_id: &EntityId,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        ctx.windows().show_window_and_focus_app(ctx.window_id());
        if let AuthOnboardingState::Terminal(workspace) = &self.auth_onboarding_state {
            workspace.update(ctx, |view, ctx| {
                view.activate_tab_by_pane_group_id(*pane_group_id, ctx);
            });
        }
        true
    }

    fn handle_notification_click(
        &mut self,
        pane_view_locator: &PaneViewLocator,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        // Focus the pane that the notification originated from.
        self.focus_pane(pane_view_locator, ctx);
        true
    }

    #[allow(clippy::ptr_arg)]
    fn add_session_at_path(&mut self, path: &PathBuf, ctx: &mut ViewContext<Self>) -> bool {
        let window_id = ctx.window_id();
        if let AuthOnboardingState::Terminal(handle) = &self.auth_onboarding_state {
            handle.update(ctx, |view, ctx| {
                view.add_tab_with_pane_layout(
                    PanesLayout::SingleTerminal(Box::new(
                        NewTerminalOptions::default()
                            .with_initial_directory_opt(path_if_directory(path).map(Into::into)),
                    )),
                    Arc::new(HashMap::new()),
                    None,
                    ctx,
                );
                ctx.windows().show_window_and_focus_app(window_id);
                ctx.notify();
            })
        } else {
            log::warn!("Auth not complete before trying to add new session at path");
        }
        true
    }

    pub fn open_warp_drive_object_in_existing_window(
        &mut self,
        arg: &OpenWarpDriveObjectArgs,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        if let AuthOnboardingState::Terminal(handle) = &self.auth_onboarding_state {
            let cloud_model = CloudModel::as_ref(ctx);

            match arg.object_type {
                ObjectType::Notebook => {
                    handle.update(ctx, |workspace, ctx| {
                        let initialized_section_states =
                            workspace.has_warp_drive_initialized_sections(ctx);
                        let notebook_id = SyncId::ServerId(arg.server_id);
                        let settings = arg.settings.clone();
                        let _ = ctx.spawn(initialized_section_states, move |workspace, _, ctx| {
                            workspace.open_notebook(
                                &NotebookSource::Existing(notebook_id),
                                &settings,
                                ctx,
                                false,
                            );
                        });
                    });
                }
                ObjectType::Workflow => {
                    handle.update(ctx, |workspace, ctx| {
                        let initialized_section_states =
                            workspace.has_warp_drive_initialized_sections(ctx);
                        let workflow_id = SyncId::ServerId(arg.server_id);
                        let settings = arg.settings.clone();
                        let _ = ctx.spawn(initialized_section_states, move |workspace, _, ctx| {
                            workspace.open_workflow_from_intent(workflow_id, &settings, ctx);
                        });
                    });
                }
                ObjectType::GenericStringObject(GenericStringObjectFormat::Json(
                    JsonObjectType::EnvVarCollection,
                )) => {
                    if cloud_model.get_by_uid(&arg.server_id.uid()).is_none() {
                        display_object_missing_error_in_window(ctx.window_id(), ctx);
                        return false;
                    }

                    let item_id =
                        WarpDriveItemId::Object(CloudObjectTypeAndId::from_generic_string_object(
                            GenericStringObjectFormat::Json(JsonObjectType::EnvVarCollection),
                            SyncId::ServerId(arg.server_id),
                        ));

                    handle.update(ctx, |workspace, ctx| {
                        let initialized_section_states =
                            workspace.has_warp_drive_initialized_sections(ctx);
                        let _ = ctx.spawn(initialized_section_states, move |workspace, _, ctx| {
                            workspace.view_in_and_focus_warp_drive(item_id, ctx);
                        });
                    });
                }
                ObjectType::Folder => {
                    if cloud_model.get_by_uid(&arg.server_id.uid()).is_none() {
                        display_object_missing_error_in_window(ctx.window_id(), ctx);
                        return false;
                    }

                    let item_id = WarpDriveItemId::Object(CloudObjectTypeAndId::Folder(
                        SyncId::ServerId(arg.server_id),
                    ));
                    handle.update(ctx, |workspace, ctx| {
                        let initialized_section_states =
                            workspace.has_warp_drive_initialized_sections(ctx);
                        let _ = ctx.spawn(initialized_section_states, move |workspace, _, ctx| {
                            workspace.view_in_and_focus_warp_drive(item_id, ctx);
                        });
                    });
                }
                _ => {
                    log::info!(
                        "Object type {:?} not support yet for opening via link",
                        arg.object_type
                    )
                }
            }

            let window_id = ctx.window_id();
            ctx.windows().show_window_and_focus_app(window_id);
            ctx.notify();
        } else {
            log::warn!("Auth not complete before trying to open warp drive object");
        }
        true
    }

    /// Opens a cloud conversation in an existing window.
    /// If the user owns the conversation, restores or navigates to it directly.
    /// Otherwise, opens a read-only transcript viewer.
    pub fn open_cloud_conversation_in_existing_window(
        &mut self,
        conversation_id: &ServerConversationToken,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        if let AuthOnboardingState::Terminal(handle) = &self.auth_onboarding_state {
            handle.update(ctx, |workspace, ctx| {
                workspace.open_cloud_conversation_from_server_token(conversation_id.clone(), ctx);
            });
            let window_id = ctx.window_id();
            ctx.windows().show_window_and_focus_app(window_id);
            ctx.notify();
            true
        } else {
            log::warn!("Auth not complete before trying to open conversation viewer");
            false
        }
    }

    pub fn add_file_pane(&mut self, path: &PathBuf, ctx: &mut ViewContext<Self>) -> bool {
        if let AuthOnboardingState::Terminal(handle) = &self.auth_onboarding_state {
            handle.update(ctx, |workspace, ctx| {
                workspace.add_tab_for_file_notebook(Some(path.to_owned()), ctx);
            });
            let window_id = ctx.window_id();
            ctx.windows().show_window_and_focus_app(window_id);
            ctx.notify();
        } else {
            log::warn!("Auth not complete before trying to open file pane");
        }
        true
    }

    /// Insert a command that should create a subshell. If we support bootstrapping AKA
    /// "warpifying" its [`ShellType`], set a flag to automatically bootstrap it when the command's
    /// block receives the [`AfterBlockStarted`] event.
    pub fn insert_subshell_command_and_bootstrap_if_supported(
        &mut self,
        arg: &SubshellCommandArg,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        let window_id = ctx.window_id();
        if let AuthOnboardingState::Terminal(handle) = &self.auth_onboarding_state {
            handle.update(ctx, |workspace, ctx| {
                workspace.insert_subshell_command_and_bootstrap_if_supported(
                    &arg.command,
                    arg.shell_type,
                    ctx,
                );
                ctx.windows().show_window_and_focus_app(window_id);
            })
        } else {
            log::warn!("Auth not complete before trying to fill input");
        }
        true
    }

    /// Shows the user the settings view of their newly joined team
    /// within the app.
    pub fn handle_team_intent_link_action(&mut self, _: &(), ctx: &mut ViewContext<Self>) -> bool {
        // Force-open warp drive.
        let window_id = ctx.window_id();
        if let AuthOnboardingState::Terminal(handle) = &self.auth_onboarding_state {
            ctx.dispatch_typed_action_for_view(
                window_id,
                handle.id(),
                &WorkspaceAction::OpenWarpDrive,
            );
            ctx.windows().show_window_and_focus_app(window_id);
        } else {
            log::error!("Auth not complete before trying to open warp drive");
        }
        true
    }

    pub fn open_settings_page_in_existing_window(
        &mut self,
        section: &SettingsSection,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        let window_id = ctx.window_id();
        if let AuthOnboardingState::Terminal(handle) = &self.auth_onboarding_state {
            ctx.dispatch_typed_action_for_view(
                window_id,
                handle.id(),
                &WorkspaceAction::ShowSettingsPage(*section),
            );
            ctx.windows().show_window_and_focus_app(window_id);
        } else {
            log::error!("Auth not complete before trying to open settings page {section:?}");
        }
        true
    }

    /// Opens the MCP servers settings page in an existing window, optionally triggering auto-install.
    /// Waits for `initial_load_complete` before opening so gallery data is available for autoinstall.
    pub fn open_mcp_settings_in_existing_window(
        &mut self,
        args: &OpenMCPSettingsArgs,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        if let AuthOnboardingState::Terminal(handle) = &self.auth_onboarding_state {
            let autoinstall = args.autoinstall.clone();
            let initial_load_complete = UpdateManager::as_ref(ctx).initial_load_complete();
            handle.update(ctx, |_, ctx| {
                let _ = ctx.spawn(initial_load_complete, move |workspace, _, ctx| {
                    workspace.open_mcp_servers_page(
                        MCPServersSettingsPage::List,
                        autoinstall.as_deref(),
                        ctx,
                    )
                });
            });
            let window_id = ctx.window_id();
            ctx.windows().show_window_and_focus_app(window_id);
        } else {
            log::error!("Auth not complete before trying to open MCP settings page");
        }
        true
    }

    /// Opens the Codex modal in an existing window.
    pub fn open_codex_in_existing_window(&mut self, _: &(), ctx: &mut ViewContext<Self>) -> bool {
        let window_id = ctx.window_id();
        if let AuthOnboardingState::Terminal(handle) = &self.auth_onboarding_state {
            handle.update(ctx, |workspace, ctx| {
                workspace.open_codex_modal(ctx);
            });
            ctx.windows().show_window_and_focus_app(window_id);
        } else {
            log::error!("Auth not complete before trying to open Codex modal");
        }
        true
    }

    /// Opens a new tab with agent view for a Linear issue work deeplink.
    pub fn open_linear_issue_work_in_existing_window(
        &mut self,
        args: &LinearIssueWork,
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        let window_id = ctx.window_id();
        if let AuthOnboardingState::Terminal(handle) = &self.auth_onboarding_state {
            let args = args.clone();
            handle.update(ctx, |workspace, ctx| {
                workspace.open_linear_issue_work(&args, ctx);
            });
            ctx.windows().show_window_and_focus_app(window_id);
        } else {
            log::error!("Auth not complete before trying to open Linear issue work");
        }
        true
    }

    pub fn focus(&mut self, ctx: &mut ViewContext<Self>) -> bool {
        match &self.auth_onboarding_state {
            AuthOnboardingState::Onboarding {
                onboarding_view, ..
            } => {
                ctx.focus(onboarding_view);
            }
            AuthOnboardingState::Terminal(workspace) => {
                ctx.focus(workspace);
            }
        }
        ctx.notify();
        true
    }

    /// If onboarding stored a pending tutorial, start it now that the workspace exists.
    fn start_pending_tutorial(&mut self, ctx: &mut ViewContext<Self>) {
        let Some(tutorial) = self.pending_tutorial.take() else {
            return;
        };

        let AuthOnboardingState::Terminal(workspace) = &self.auth_onboarding_state else {
            return;
        };

        if FeatureFlag::OpenWarpNewSettingsModes.is_enabled()
            && FeatureFlag::TabConfigs.is_enabled()
        {
            let intention = tutorial.intention();
            if matches!(intention, OnboardingIntention::AgentDrivenDevelopment) {
                workspace.update(ctx, |view, ctx| {
                    view.open_vertical_tabs_panel_if_enabled(ctx);
                    view.start_agent_onboarding_tutorial(tutorial, ctx);
                });
            } else {
                workspace.update(ctx, |view, ctx| {
                    view.open_vertical_tabs_panel_if_enabled(ctx);
                });
            }
        } else if *AISettings::as_ref(ctx).is_any_ai_enabled {
            workspace.update(ctx, |view, ctx| {
                view.start_agent_onboarding_tutorial(tutorial, ctx);
            });
        }
    }

    fn traffic_light_data(&self, ctx: &AppContext) -> Option<TrafficLightData> {
        // The workspace view will handle rendering of the traffic lights (so
        // that they can be hidden when the tab bar is hidden).
        if matches!(self.auth_onboarding_state, AuthOnboardingState::Terminal(_)) {
            return None;
        }

        traffic_light_data(ctx, self.window_id)
    }
}

#[derive(Clone, Debug)]
pub enum RootViewEvent {
    AuthOnboardingStateChanged,
}

impl Entity for RootView {
    type Event = RootViewEvent;
}

impl View for RootView {
    fn ui_name() -> &'static str {
        "RootView"
    }

    fn on_focus(&mut self, focus_ctx: &FocusContext, ctx: &mut ViewContext<Self>) {
        if focus_ctx.is_self_focused() {
            self.focus(ctx);
        } else if matches!(
            self.auth_onboarding_state,
            AuthOnboardingState::Onboarding { .. }
        ) {
            // During onboarding, aggressively redirect focus.
            // This ensures keystrokes (Enter) are handled by the correct view rather
            // than something hidden like the input editor.
            self.focus(ctx);
        }
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let child = match &self.auth_onboarding_state {
            AuthOnboardingState::Onboarding {
                onboarding_view, ..
            } => ChildView::new(onboarding_view).finish(),
            AuthOnboardingState::Terminal(workspace) => ChildView::new(workspace).finish(),
        };

        let mut stack = Stack::new();
        stack.add_child(child);

        if let Some(splash) = &self.launch_splash {
            stack.add_child(splash.render(Appearance::as_ref(app)));
        }

        if let Some(traffic_light_data) = self.traffic_light_data(app) {
            let theme = Appearance::as_ref(app).theme();
            let fullscreen_state = app
                .windows()
                .platform_window(self.window_id)
                .map(|window| window.fullscreen_state())
                .unwrap_or_default();
            stack.add_positioned_child(
                traffic_light_data.render(fullscreen_state, &self.mouse_states, theme, app),
                OffsetPositioning::offset_from_parent(
                    vec2f(0., 0.),
                    ParentOffsetBounds::WindowByPosition,
                    ParentAnchor::TopRight,
                    ChildAnchor::TopRight,
                ),
            );
        }

        stack.finish()
    }

    fn keymap_context(&self, app: &AppContext) -> warpui::keymap::Context {
        let mut context = Self::default_keymap_context();
        if quake_mode_window_is_open() {
            context.set.insert(flags::QUAKE_WINDOW_OPEN_FLAG);
        }
        if *KeysSettings::as_ref(app).quake_mode_enabled {
            context.set.insert(flags::QUAKE_MODE_ENABLED_CONTEXT_FLAG);
        }
        if *KeysSettings::as_ref(app).activation_hotkey_enabled.value() {
            context.set.insert(flags::ACTIVATION_HOTKEY_FLAG);
        }
        context
    }
}

#[derive(Clone, Debug)]
pub enum RootViewAction {
    ToggleQuakeModeWindow,
    ShowOrHideNonQuakeModeWindows,
    ToggleFullscreen,
    DebugEnterOnboardingState,
}

impl TypedActionView for RootView {
    type Action = RootViewAction;
    fn handle_action(&mut self, action: &RootViewAction, ctx: &mut ViewContext<Self>) {
        match action {
            RootViewAction::ToggleQuakeModeWindow => {
                let global_resource_handles =
                    GlobalResourceHandlesProvider::as_ref(ctx).get().clone();
                toggle_quake_mode_window(&global_resource_handles, ctx)
            }
            RootViewAction::ShowOrHideNonQuakeModeWindows => {
                show_or_hide_non_quake_mode_windows(&(), ctx)
            }
            RootViewAction::ToggleFullscreen => {
                let window_id = ctx.window_id();
                WindowManager::handle(ctx).update(ctx, |state, ctx| {
                    state.toggle_fullscreen(window_id, ctx);
                });
            }
            RootViewAction::DebugEnterOnboardingState => {
                self.debug_enter_onboarding_state(&(), ctx);
            }
        }
    }
}

impl WorkspaceArgs {
    fn create_workspace(self, ctx: &mut ViewContext<RootView>) -> ViewHandle<Workspace> {
        ctx.add_typed_action_view(|ctx| {
            Workspace::new(
                self.global_resource_handles,
                self.server_time,
                self.workspace_setting,
                ctx,
            )
        })
    }
}

impl AuthOnboardingState {
    fn try_open_onboarding_slides(&mut self, ctx: &mut ViewContext<RootView>) {
        let target = match self {
            AuthOnboardingState::Terminal(workspace) => {
                AuthOnboardingTarget::Terminal(workspace.clone())
            }
            AuthOnboardingState::Onboarding { .. } => {
                // Onboarding slides can only be opened from the Terminal state
                return;
            }
        };

        let onboarding_view = RootView::create_agent_onboarding_view(ctx);
        onboarding_view.update(ctx, |view, ctx| {
            view.start_onboarding(ctx);
        });
        *self = AuthOnboardingState::Onboarding {
            onboarding_view,
            target,
        };
    }
}

impl AuthOnboardingTarget {
    fn to_workspace(&self, ctx: &mut ViewContext<RootView>) -> ViewHandle<Workspace> {
        match self {
            AuthOnboardingTarget::Terminal(workspace) => workspace.clone(),
            AuthOnboardingTarget::Workspace(args) => args.clone().create_workspace(ctx),
        }
    }
}
