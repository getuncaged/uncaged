pub(super) mod chips;
pub mod editor;
pub mod toolbar_item;

#[cfg(not(target_family = "wasm"))]
use std::env;
#[cfg(not(target_family = "wasm"))]
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(not(target_family = "wasm"))]
use std::time::Duration;

use ai::document::{AIDocumentId, AIDocumentVersion};
use chrono::{DateTime, Local};
use parking_lot::FairMutex;
use pathfinder_color::ColorU;
use pathfinder_geometry::vector::{vec2f, Vector2F};
use settings::{Setting, ToggleableSetting};
#[cfg(not(target_family = "wasm"))]
use tokio::fs;
use toolbar_item::AgentToolbarItemKind;
use warp_core::report_if_error;
use warp_core::ui::color::blend::Blend;
use warp_core::ui::color::contrast::MinimumAllowedContrast;
use warp_core::ui::color::ContrastingColor;
use warp_core::ui::theme::color::internal_colors;
use warp_core::ui::theme::{AnsiColorIdentifier, Fill};
use warpui::elements::{
    Border, ChildAnchor, ChildView, Clipped, ConstrainedBox, Container, CornerRadius,
    CrossAxisAlignment, DispatchEventResult, Element, Empty, EventHandler, Expanded, Flex,
    MainAxisAlignment, MainAxisSize, OffsetPositioning, ParentAnchor, ParentElement,
    ParentOffsetBounds, PositionedElementAnchor, PositionedElementOffsetBounds, Radius, Shrinkable,
    Stack, Text, Wrap, WrapFill, WrapFillEntireRun, DEFAULT_UI_LINE_HEIGHT_RATIO,
};
use warpui::r#async::{SpawnedFutureHandle, Timer};
use warpui::ui_components::segmented_control::SegmentedControl;
use warpui::{
    AppContext, Entity, EntityId, ModelHandle, SingletonEntity, TypedActionView, View, ViewContext,
    ViewHandle,
};

use crate::ai::blocklist::agent_view::is_in_cloud_context;
use crate::ai::blocklist::history_model::{BlocklistAIHistoryEvent, BlocklistAIHistoryModel};
use crate::ai::blocklist::prompt::prompt_alert::{PromptAlertEvent, PromptAlertView};
use crate::ai::blocklist::usage::icon_for_context_window_usage;
use crate::ai::blocklist::{BlocklistAIInputModel, InputType};
use crate::ai::execution_profiles::profiles::AIExecutionProfilesModel;
use crate::ai::AIRequestUsageModel;
use crate::appearance::Appearance;
use crate::completer::SessionContext;
use crate::context_chips::display_chip::{DisplayChip, DisplayChipConfig, PromptChipShellCommand};
use crate::context_chips::prompt_type::PromptType;
use crate::context_chips::{self, ContextChipKind};
use crate::features::FeatureFlag;
use crate::network::NetworkStatus;
use crate::settings::{
    AISettings, AISettingsChangedEvent, PrivacySettings, PrivacySettingsChangedEvent,
};
use crate::settings_view::SettingsSection;
#[cfg(not(target_family = "wasm"))]
use crate::terminal::cli_agent_sessions::plugin_manager::{
    compare_versions, plugin_manager_for, plugin_manager_for_with_shell, CliAgentPluginManager,
    PluginInstallError, PluginModalKind,
};
use crate::terminal::cli_agent_sessions::{
    CLIAgentInputState, CLIAgentSessionsModel, CLIAgentSessionsModelEvent,
};
use crate::terminal::input::models::InlineModelSelectorTab;
use crate::terminal::input::MenuPositioningProvider;
#[cfg(not(target_family = "wasm"))]
use crate::terminal::local_shell::LocalShellState;
use crate::terminal::model_events::ModelEvent;
use crate::terminal::profile_model_selector::{ProfileModelSelector, ProfileModelSelectorEvent};
use crate::terminal::session_settings::{
    SessionSettings, SessionSettingsChangedEvent, ToolbarChipSelection,
};
use crate::terminal::universal_developer_input::InputToggleMode;
use crate::terminal::view::init::OPEN_CLI_AGENT_RICH_INPUT_KEYBINDING;
use crate::terminal::view::TerminalAction;
#[cfg(not(target_family = "wasm"))]
use crate::terminal::ShellLaunchData;
use crate::terminal::{CLIAgent, TerminalModel};
use crate::ui_components::icons::Icon;
use crate::view_components::action_button::{
    ActionButton, ActionButtonTheme, AdjoinedSide, ButtonSize, KeystrokeSource, NakedTheme,
    TooltipAlignment,
};
use crate::view_components::DismissibleToast;
#[cfg(not(target_family = "wasm"))]
use crate::view_components::ToastLink;
use crate::workspace::view::TOGGLE_PROJECT_EXPLORER_BINDING_NAME;
use crate::workspace::ToastStack;
#[cfg(not(target_family = "wasm"))]
use crate::workspace::WorkspaceAction;

const ENABLE_NLD_TOOLTIP: &str = "Enable terminal command autodetection";
const DISABLE_NLD_TOOLTIP: &str = "Disable terminal command autodetection";

const FAST_FORWARD_ON_TOOLTIP: &str = "Turn off auto-approve all agent actions";
const FAST_FORWARD_OFF_TOOLTIP: &str = "Auto-approve all agent actions for this task";
const FAST_FORWARD_LOCKED_TOOLTIP: &str =
    "Fast forward is always enabled for cloud agent conversations";

/// How long to wait after session creation before showing the install chip.
/// Gives the plugin time to connect and send its `SessionStart` event.
#[cfg(not(target_family = "wasm"))]
const PLUGIN_CHIP_DEBOUNCE: Duration = Duration::from_secs(3);

#[cfg_attr(target_family = "wasm", allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PluginChipKind {
    Install,
    Update,
}

/// Builds a composite key for per-agent, per-host plugin chip dismissal.
/// Returns `"<agent_prefix>"` for local sessions or `"<agent_prefix>@<host>"` for remote.
fn plugin_chip_key(agent_prefix: &str, remote_host: &Option<String>) -> String {
    match remote_host {
        Some(host) => format!("{agent_prefix}@{host}"),
        None => agent_prefix.to_owned(),
    }
}

/// Footer control bar at the bottom of the agent input.
///
/// Renders in two modes:
/// - **Agent View mode** (default): model selector, NLD toggle, chips, etc.
/// - **CLI agent mode**: agent icon, image, mic, file explorer, view changes, rich input.
///
/// The mode is determined by reading `CLIAgentSessionsModel` at render time.
/// A single `ViewHandle<AgentInputFooter>` is shared between `Input` and
/// `UseAgentToolbar`, rendering the appropriate mode in each context.
pub struct AgentInputFooter {
    terminal_view_id: EntityId,
    nld_button: ViewHandle<ActionButton>,
    file_button: ViewHandle<ActionButton>,
    context_window_button: ViewHandle<ActionButton>,
    model_selector: ViewHandle<ProfileModelSelector>,
    ftu_callout_close_button: ViewHandle<ActionButton>,
    prompt_alert: ViewHandle<PromptAlertView>,
    left_display_chips: Vec<ViewHandle<DisplayChip>>,
    right_display_chips: Vec<ViewHandle<DisplayChip>>,
    // Separate set of display chips for the CLI agent footer.
    // Needed because the CLI footer chip selection can include chips not present in the agent view selection.
    cli_display_chips: Vec<ViewHandle<DisplayChip>>,
    display_chip_config: DisplayChipConfig,

    terminal_model: Arc<FairMutex<TerminalModel>>,
    render_ftu_callout: bool,

    // CLI agent-specific buttons (rendered when a CLI agent session is active).
    file_explorer_button: ViewHandle<ActionButton>,
    rich_input_button: ViewHandle<ActionButton>,
    settings_button: ViewHandle<ActionButton>,
    install_plugin_button: ViewHandle<ActionButton>,
    plugin_instructions_button: ViewHandle<ActionButton>,
    update_plugin_button: ViewHandle<ActionButton>,
    update_instructions_button: ViewHandle<ActionButton>,
    dismiss_plugin_chip_button: ViewHandle<ActionButton>,
    plugin_operation_in_progress: bool,
    /// When `true`, the install chip is allowed to render.
    /// Starts `false` and is set to `true` after a debounce timer fires,
    /// giving the plugin time to connect before we prompt installation.
    /// Reset to `false` when a listener connects.
    plugin_chip_ready: bool,

    // Fast-forward (auto-approve) toggle button shown in the agent view footer.
    fast_forward_button: ViewHandle<ActionButton>,

    /// Pending one-shot timer that refreshes the context-window button at the
    /// prompt-cache expiry instant so the notification dot appears while idle.
    prompt_cache_expiry_timer_handle: Option<SpawnedFutureHandle>,

    /// Whether the active conversation's prompt cache has expired. Drives the
    /// yellow notification dot on the context-window chip when the
    /// `PromptCacheExpiryWarning` flag is enabled.
    prompt_cache_expired: bool,

    ai_input_model: ModelHandle<BlocklistAIInputModel>,

    /// Uncaged: the Terminal / Agent switch, pinned first in the row. The input owns it (it
    /// lives on `InputModeToggle`, which keeps its enabled state current);
    /// the footer only renders it, so the switch sits in the same place in both modes.
    mode_toggle: Option<ViewHandle<SegmentedControl<InputToggleMode>>>,
}

impl AgentInputFooter {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        menu_positioning_provider: Arc<dyn MenuPositioningProvider>,
        terminal_view_id: EntityId,
        ai_input_model: ModelHandle<BlocklistAIInputModel>,
        terminal_model: Arc<FairMutex<TerminalModel>>,
        prompt: ModelHandle<PromptType>,
        display_chip_config: DisplayChipConfig,
        ctx: &mut ViewContext<Self>,
    ) -> Self {
        let button_size = ButtonSize::AgentInputButton;

        let nld_button = ctx.add_typed_action_view(|ctx| {
            let is_nld_enabled = AISettings::as_ref(ctx).is_ai_autodetection_enabled(ctx);
            let mut button = ActionButton::new(
                "",
                NLDButtonTheme {
                    is_enabled: is_nld_enabled,
                },
            )
            .with_icon(Icon::NLD)
            .with_size(button_size)
            .with_tooltip_alignment(TooltipAlignment::Left)
            .on_click(|ctx| {
                ctx.dispatch_typed_action(AgentInputFooterAction::ToggleAutodetectionSetting);
            });
            button.set_active(is_nld_enabled, ctx);
            button.set_tooltip(
                Some(if is_nld_enabled {
                    DISABLE_NLD_TOOLTIP
                } else {
                    ENABLE_NLD_TOOLTIP
                }),
                ctx,
            );
            button
        });
        ctx.subscribe_to_model(&AISettings::handle(ctx), |me, settings, event, ctx| {
            let AISettingsChangedEvent::AIAutoDetectionEnabled { .. } = event else {
                return;
            };
            let is_nld_enabled = settings.as_ref(ctx).is_ai_autodetection_enabled(ctx);
            me.nld_button.update(ctx, |button, ctx| {
                button.set_theme(
                    NLDButtonTheme {
                        is_enabled: is_nld_enabled,
                    },
                    ctx,
                );
                button.set_active(is_nld_enabled, ctx);
                button.set_tooltip(
                    Some(if is_nld_enabled {
                        DISABLE_NLD_TOOLTIP
                    } else {
                        ENABLE_NLD_TOOLTIP
                    }),
                    ctx,
                );
            });
        });

        let file_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("", AgentInputButtonTheme)
                .with_icon(Icon::Plus)
                .with_tooltip("Attach file")
                .with_size(button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(AgentInputFooterAction::SelectFile);
                })
        });

        // Fast-forward (auto-approve) toggle button.
        // Uses FastForwardButtonTheme so the button keeps its one-off semantics.
        // The theme still delegates its fill to the shared chip background.
        let fast_forward_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("", FastForwardButtonTheme)
                .with_icon(Icon::FastForward)
                .with_tooltip(FAST_FORWARD_OFF_TOOLTIP)
                .with_size(button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .with_disabled_theme(FastForwardLockedTheme)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(TerminalAction::ToggleAutoexecuteMode);
                })
        });

        // CLI agent-specific buttons (only rendered when a CLI agent session is active).
        let cli_button_size = ButtonSize::AgentInputButton;
        let file_explorer_button = ctx.add_typed_action_view(|ctx| {
            ActionButton::new("File explorer", AgentInputButtonTheme)
                .with_icon(Icon::FileCopy)
                .with_tooltip("Open file explorer")
                .with_size(cli_button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .with_keybinding(
                    KeystrokeSource::Binding(TOGGLE_PROJECT_EXPLORER_BINDING_NAME),
                    ctx,
                )
                .with_compact_keybinding(true)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(AgentInputFooterAction::ToggleFileExplorer);
                })
        });
        let rich_input_button = ctx.add_typed_action_view(|ctx| {
            ActionButton::new("Rich Input", AgentInputButtonTheme)
                .with_icon(Icon::TextInput)
                .with_tooltip("Open Rich Input")
                .with_size(cli_button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .with_keybinding(
                    KeystrokeSource::Binding(OPEN_CLI_AGENT_RICH_INPUT_KEYBINDING),
                    ctx,
                )
                .with_compact_keybinding(true)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(AgentInputFooterAction::ToggleRichInput);
                })
        });
        let settings_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("", AgentInputButtonTheme)
                .with_icon(Icon::Settings)
                .with_tooltip("Open coding agent settings")
                .with_size(cli_button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(AgentInputFooterAction::OpenCodingAgentSettings);
                })
        });

        let install_plugin_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("Enable notifications", InstallPluginButtonTheme)
                .with_icon(Icon::Download)
                .with_tooltip(
                    "Install the Uncaged plugin to enable rich agent notifications within Uncaged",
                )
                .with_size(cli_button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .with_adjoined_side(AdjoinedSide::Right)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(AgentInputFooterAction::InstallPlugin);
                })
        });

        let plugin_instructions_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("Notifications setup instructions", InstallPluginButtonTheme)
                .with_icon(Icon::Info)
                .with_tooltip("View instructions to install the Uncaged plugin")
                .with_size(cli_button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .with_adjoined_side(AdjoinedSide::Right)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(
                        AgentInputFooterAction::OpenPluginInstallInstructionsPane,
                    );
                })
        });

        let update_plugin_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("Update Uncaged plugin", InstallPluginButtonTheme)
                .with_icon(Icon::Download)
                .with_tooltip("A new version of the Uncaged plugin is available")
                .with_size(cli_button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .with_adjoined_side(AdjoinedSide::Right)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(AgentInputFooterAction::UpdatePlugin);
                })
        });

        let update_instructions_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("Plugin update instructions", InstallPluginButtonTheme)
                .with_icon(Icon::Info)
                .with_tooltip("View instructions to update the Uncaged plugin")
                .with_size(cli_button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
                .with_adjoined_side(AdjoinedSide::Right)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(
                        AgentInputFooterAction::OpenPluginUpdateInstructionsPane,
                    );
                })
        });

        let dismiss_plugin_chip_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("", InstallPluginButtonTheme)
                .with_icon(Icon::X)
                .with_size(cli_button_size)
                .with_tooltip("Dismiss")
                .with_tooltip_alignment(TooltipAlignment::Left)
                .with_adjoined_side(AdjoinedSide::Left)
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(AgentInputFooterAction::DismissPluginChip);
                })
        });

        // Toggle rich input button label when CLI input session opens/closes.
        ctx.subscribe_to_model(
            &CLIAgentSessionsModel::handle(ctx),
            move |me, _, event, ctx| {
                if event.terminal_view_id() != terminal_view_id {
                    return;
                }

                // Reset the debounce when a session ends so the next
                // session gets a fresh debounce window.
                if let CLIAgentSessionsModelEvent::Ended { .. } = event {
                    me.plugin_chip_ready = false;
                }

                // When a structured plugin connects, the plugin is verified
                // installed — hide the chip. Codex's OSC 9 fallback is not a
                // structured plugin, so its chip stays until the plugin connects.
                if CLIAgentSessionsModel::as_ref(ctx)
                    .session(me.terminal_view_id)
                    .is_some_and(|s| s.supports_rich_status())
                {
                    me.plugin_chip_ready = false;
                }

                // When a session starts, update the install chip label and
                // start a debounce timer for non-auto-install agents.
                #[cfg(not(target_family = "wasm"))]
                if let CLIAgentSessionsModelEvent::Started { .. } = event {
                    if let Some(agent) = me.cli_agent(ctx) {
                        let label = format!("Enable {} notifications", agent.display_name());
                        me.install_plugin_button.update(ctx, |button, ctx| {
                            button.set_label(label, ctx);
                        });
                        if let Some(manager) = plugin_manager_for(agent) {
                            if !manager.can_auto_install() {
                                ctx.spawn(
                                    Timer::after(PLUGIN_CHIP_DEBOUNCE),
                                    |me, _, ctx: &mut ViewContext<Self>| {
                                        let suppress = CLIAgentSessionsModel::as_ref(ctx)
                                            .session(me.terminal_view_id)
                                            .is_some_and(|s| s.supports_rich_status());
                                        if !suppress {
                                            me.plugin_chip_ready = true;
                                            ctx.notify();
                                        }
                                    },
                                );
                            }
                        }
                    }
                }

                let CLIAgentSessionsModelEvent::InputSessionChanged {
                    new_input_state, ..
                } = event
                else {
                    ctx.notify();
                    return;
                };
                let is_open = matches!(new_input_state, CLIAgentInputState::Open { .. });
                me.rich_input_button.update(ctx, |button, ctx| {
                    if is_open {
                        button.set_label("Hide Rich Input", ctx);
                        button.set_tooltip(Some("Hide Rich Input"), ctx);
                        button.set_keybinding(
                            Some(KeystrokeSource::Binding(
                                OPEN_CLI_AGENT_RICH_INPUT_KEYBINDING,
                            )),
                            ctx,
                        );
                    } else {
                        button.set_label("Rich Input", ctx);
                        button.set_tooltip(Some("Open Rich Input"), ctx);
                        button.set_keybinding(
                            Some(KeystrokeSource::Binding(
                                OPEN_CLI_AGENT_RICH_INPUT_KEYBINDING,
                            )),
                            ctx,
                        );
                    }
                });
                ctx.notify();
            },
        );

        let context_window_button = ctx.add_typed_action_view(|_ctx| {
            ActionButton::new("", AgentInputButtonTheme)
                .with_icon(Icon::ConversationContext0)
                .with_tooltip("Context window usage")
                .with_size(button_size)
                .with_tooltip_alignment(TooltipAlignment::Left)
        });

        let profile_model_selector_full = ctx.add_typed_action_view(|ctx| {
            let mut selector = ProfileModelSelector::new(
                menu_positioning_provider.clone(),
                terminal_view_id,
                ai_input_model.clone(),
                terminal_model.clone(),
                None,
                ctx,
            );
            selector.set_render_compact(false, ctx);
            selector
        });

        ctx.subscribe_to_view(&profile_model_selector_full, |me, _, event, ctx| {
            me.handle_profile_model_selector_event(event, ctx);
        });

        let prompt_alert = ctx.add_typed_action_view(PromptAlertView::new);
        ctx.subscribe_to_view(&prompt_alert, |_, _, event, ctx| {
            ctx.emit(AgentInputFooterEvent::PromptAlert(event.clone()));
        });

        ctx.subscribe_to_model(&NetworkStatus::handle(ctx), |_, _, _, ctx| {
            ctx.notify();
        });
        ctx.subscribe_to_model(&AIRequestUsageModel::handle(ctx), |_, _, _, ctx| {
            ctx.notify()
        });
        ctx.subscribe_to_model(&AISettings::handle(ctx), |_, _, event, ctx| {
            if matches!(
                event,
                AISettingsChangedEvent::AIAutoDetectionEnabled { .. }
                    | AISettingsChangedEvent::ShouldForceDisableCloudHandoff { .. }
            ) {
                ctx.notify()
            }
        });
        ctx.subscribe_to_model(&PrivacySettings::handle(ctx), |_, _, event, ctx| {
            if matches!(
                event,
                PrivacySettingsChangedEvent::UpdateIsCloudConversationStorageEnabled { .. }
            ) {
                ctx.notify()
            }
        });
        ctx.subscribe_to_model(&display_chip_config.model_events, |me, _, event, ctx| {
            if let ModelEvent::AgentTaggedInChanged { .. } = event {
                me.update_ftu_callout_render_state(ctx);
            }
        });
        ctx.subscribe_to_model(
            &display_chip_config.agent_view_controller,
            |me, _, _, ctx| {
                me.sync_fast_forward_button(ctx);
            },
        );

        let prompt_for_session_settings = prompt.clone();
        ctx.subscribe_to_model(
            &SessionSettings::handle(ctx),
            move |me, _, event, ctx| match event {
                SessionSettingsChangedEvent::ShowModelSelectorsInPrompt { .. } => {
                    ctx.notify();
                }
                SessionSettingsChangedEvent::AgentToolbarChipSelectionSetting { .. }
                | SessionSettingsChangedEvent::CLIAgentToolbarChipSelectionSetting { .. }
                | SessionSettingsChangedEvent::GithubPrChipDefaultValidation { .. } => {
                    me.update_display_chips(&prompt_for_session_settings, ctx);
                    ctx.notify();
                }
                _ => {}
            },
        );
        // Subscribe to AIExecutionProfilesModel to potentially show/hide the profile selector button when profiles are added/removed
        ctx.subscribe_to_model(&AIExecutionProfilesModel::handle(ctx), |_, _, _, ctx| {
            ctx.notify();
        });

        ctx.subscribe_to_model(
            &BlocklistAIHistoryModel::handle(ctx),
            |me, _, event, ctx| {
                if event
                    .terminal_view_id()
                    .is_some_and(|id| id != me.terminal_view_id)
                {
                    return;
                }
                me.update_ftu_callout_render_state(ctx);

                match event {
                    BlocklistAIHistoryEvent::StartedNewConversation { .. }
                    | BlocklistAIHistoryEvent::SetActiveConversation { .. }
                    | BlocklistAIHistoryEvent::ClearedActiveConversation { .. }
                    | BlocklistAIHistoryEvent::ClearedConversationsInTerminalView { .. }
                    | BlocklistAIHistoryEvent::RemoveConversation { .. }
                    | BlocklistAIHistoryEvent::UpdatedAutoexecuteOverride { .. } => {
                        me.sync_fast_forward_button(ctx);
                        me.update_context_window_button(ctx);
                        me.model_selector.update(ctx, |_, ctx| ctx.notify());
                        ctx.notify();
                    }
                    BlocklistAIHistoryEvent::UpdatedTodoList { .. }
                    | BlocklistAIHistoryEvent::UpdatedConversationStatus { .. }
                    | BlocklistAIHistoryEvent::AppendedExchange { .. }
                    | BlocklistAIHistoryEvent::UpdatedStreamingExchange { .. } => {
                        me.update_context_window_button(ctx);
                        me.model_selector.update(ctx, |_, ctx| ctx.notify());
                        ctx.notify();
                    }
                    _ => (),
                }
            },
        );

        ctx.observe(&prompt, |me, model, ctx| {
            me.update_display_chips(&model, ctx);
        });

        // The row's contents follow the input mode, so redraw whenever it changes.
        ctx.subscribe_to_model(&ai_input_model, |_, _, _, ctx| ctx.notify());

        let mut me = Self {
            terminal_view_id,
            nld_button,
            file_button,
            file_explorer_button,
            rich_input_button,
            settings_button,
            install_plugin_button,
            plugin_instructions_button,
            update_plugin_button,
            update_instructions_button,
            dismiss_plugin_chip_button,
            plugin_operation_in_progress: false,
            plugin_chip_ready: false,
            context_window_button,
            model_selector: profile_model_selector_full,
            prompt_alert,
            terminal_model,
            render_ftu_callout: false,
            left_display_chips: vec![],
            right_display_chips: vec![],
            cli_display_chips: vec![],
            display_chip_config,
            fast_forward_button,
            ftu_callout_close_button: ctx.add_typed_action_view(|_ctx| {
                ActionButton::new("", NakedTheme)
                    .with_icon(Icon::X)
                    .with_size(ButtonSize::XSmall)
                    .on_click(|ctx| {
                        ctx.dispatch_typed_action(AgentInputFooterAction::DismissFtuModelCallout);
                    })
            }),
            prompt_cache_expiry_timer_handle: None,
            prompt_cache_expired: false,
            ai_input_model,
            mode_toggle: None,
        };
        me.sync_fast_forward_button(ctx);
        me.update_context_window_button(ctx);
        me.update_display_chips(&prompt, ctx);
        me.update_ftu_callout_render_state(ctx);
        me
    }

    pub fn set_mode_toggle(
        &mut self,
        mode_toggle: ViewHandle<SegmentedControl<InputToggleMode>>,
        ctx: &mut ViewContext<Self>,
    ) {
        self.mode_toggle = Some(mode_toggle);
        ctx.notify();
    }

    pub fn set_current_repo_path(
        &mut self,
        repo_path: Option<std::path::PathBuf>,
        ctx: &mut ViewContext<Self>,
    ) {
        self.display_chip_config.current_repo_path = repo_path;
        // Chips will be rebuilt on the next GitRepoStatusEvent::MetadataChanged.
        // Notify to ensure any existing chips reflect the change.
        ctx.notify();
    }

    fn all_display_chips(&self) -> impl Iterator<Item = &ViewHandle<DisplayChip>> {
        self.left_display_chips
            .iter()
            .chain(self.right_display_chips.iter())
            .chain(self.cli_display_chips.iter())
    }

    pub fn update_session_context(
        &mut self,
        session_context: Option<SessionContext>,
        ctx: &mut ViewContext<Self>,
    ) {
        self.display_chip_config.session_context = session_context.clone();
        for chip_view in self.all_display_chips() {
            chip_view.update(ctx, |chip, chip_ctx| {
                chip.update_session_context(session_context.clone(), chip_ctx);
            });
        }
    }

    fn has_active_cli_agent_input_session(&self, app: &AppContext) -> bool {
        CLIAgentSessionsModel::as_ref(app).is_input_open(self.terminal_view_id)
    }

    fn cli_agent(&self, app: &AppContext) -> Option<CLIAgent> {
        CLIAgentSessionsModel::as_ref(app)
            .session(self.terminal_view_id)
            .map(|session| session.agent)
    }

    fn is_cli_agent_session_active(&self, app: &AppContext) -> bool {
        CLIAgentSessionsModel::as_ref(app)
            .session(self.terminal_view_id)
            .is_some()
    }

    fn select_cli_file(&mut self, ctx: &mut ViewContext<Self>) {
        let window_id = ctx.window_id();
        let view_id = ctx.view_id();
        let file_picker_config = warpui::platform::FilePickerConfiguration::new();

        ctx.open_file_picker(
            move |result, ctx| match result {
                Ok(paths) => {
                    if let Some(path) = paths.first() {
                        ctx.dispatch_typed_action_for_view(
                            window_id,
                            view_id,
                            &AgentInputFooterAction::InsertFilePath(path.clone()),
                        );
                    }
                }
                Err(err) => {
                    let window_id = ctx.window_id();
                    ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
                        toast_stack.add_ephemeral_toast(
                            DismissibleToast::error(format!("{err}")),
                            window_id,
                            ctx,
                        );
                    });
                }
            },
            file_picker_config,
        );
    }

    /// Which plugin chip to show, if any.
    fn plugin_chip_kind(&self, app: &AppContext) -> Option<PluginChipKind> {
        #[cfg(target_family = "wasm")]
        {
            let _ = (app, self.plugin_operation_in_progress);
            None
        }
        #[cfg(not(target_family = "wasm"))]
        {
            if self.plugin_operation_in_progress || !FeatureFlag::HOANotifications.is_enabled() {
                return None;
            }

            let ai_settings = AISettings::as_ref(app);
            if !*ai_settings.show_agent_notifications {
                return None;
            }

            let session = CLIAgentSessionsModel::as_ref(app).session(self.terminal_view_id)?;

            let manager = plugin_manager_for(session.agent)?;
            let min_version = manager.minimum_plugin_version();
            let chip_key = plugin_chip_key(session.agent.command_prefix(), &session.remote_host);
            // If a structured plugin is connected and this agent supports
            // version-based updates, check the reported version.
            if session.supports_rich_status() && manager.supports_update() {
                let needs_update = match &session.plugin_version {
                    // No version reported = pre-versioning plugin, definitely outdated.
                    None => true,
                    Some(v) => compare_versions(v, min_version).is_lt(),
                };
                if !needs_update {
                    return None;
                }
                // Check update chip dismissal.
                let dismissed_version = ai_settings.plugin_update_chip_dismissed_version(&chip_key);
                if !dismissed_version.is_empty()
                    && compare_versions(dismissed_version, min_version).is_ge()
                {
                    return None;
                }
                return Some(PluginChipKind::Update);
            }

            // For agents without auto-install, wait for the debounce timer
            // before showing the install chip.
            if !manager.can_auto_install() && !self.plugin_chip_ready {
                return None;
            }

            let install_chip_dismissed = ai_settings.is_plugin_install_chip_dismissed(&chip_key);

            // For remote sessions, we can't check the filesystem.
            if session.is_remote() {
                return (!install_chip_dismissed).then_some(PluginChipKind::Install);
            }

            if manager.is_installed() {
                // Installed but no listener yet. Check the on-disk version as a fallback
                // — the plugin may be too old to send structured events.
                if manager.needs_update() {
                    let dismissed_version =
                        ai_settings.plugin_update_chip_dismissed_version(&chip_key);
                    if !dismissed_version.is_empty()
                        && compare_versions(dismissed_version, min_version).is_ge()
                    {
                        return None;
                    }
                    return Some(PluginChipKind::Update);
                }
                // Up to date on disk — wait for the listener to connect.
                return None;
            }

            // Not installed locally.
            (!install_chip_dismissed).then_some(PluginChipKind::Install)
        }
    }

    /// Whether the chip should open the manual instructions modal instead of auto-operating.
    fn should_use_manual_mode(&self, app: &AppContext) -> bool {
        let sessions_model = CLIAgentSessionsModel::as_ref(app);
        let session = match sessions_model.session(self.terminal_view_id) {
            Some(s) => s,
            None => return false,
        };

        // Custom toolbar commands always use manual mode because the user's
        // binary may differ from the agent's standard CLI tool.
        if session.custom_command_prefix.is_some() {
            return true;
        }

        #[cfg(not(target_family = "wasm"))]
        if let Some(manager) = plugin_manager_for(session.agent) {
            if !manager.can_auto_install() {
                return true;
            }
        }
        if session.is_remote() {
            return true;
        }
        // Docker sandbox sessions run inside a container; our auto-install
        // path operates on the host's shell config and would install the
        // plugin in the wrong place. Fall back to the manual-instructions
        // modal so the user can paste the install command into the sandbox
        // PTY and install inside the container.
        //
        // `ShellLaunchData` is only available on native builds; wasm builds
        // can't produce a `DockerSandbox` variant either way.
        #[cfg(not(target_family = "wasm"))]
        {
            let shell_data = {
                let model = self.terminal_model.lock();
                model.active_shell_launch_data().cloned()
            };
            if matches!(shell_data, Some(ShellLaunchData::DockerSandbox { .. })) {
                return true;
            }
        }
        sessions_model.has_plugin_auto_failed(session.agent, &session.remote_host)
    }

    /// Records that the auto plugin operation could not start, shows an error toast,
    /// and re-renders so the chip switches to manual-instructions mode.
    #[cfg(not(target_family = "wasm"))]
    fn record_plugin_auto_failure_and_notify(&mut self, ctx: &mut ViewContext<Self>) {
        if let Some(agent) = self.cli_agent(ctx) {
            let remote_host = CLIAgentSessionsModel::as_ref(ctx)
                .session(self.terminal_view_id)
                .and_then(|s| s.remote_host.clone());
            CLIAgentSessionsModel::handle(ctx).update(ctx, |model, _| {
                model.record_plugin_auto_failure(agent, remote_host);
            });
        }
        let window_id = ctx.window_id();
        ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
            toast_stack.add_ephemeral_toast(
                DismissibleToast::error(
                    "Could not automatically install plugin. \
                     Please click the chip again for manual installation steps."
                        .to_owned(),
                ),
                window_id,
                ctx,
            );
        });
        ctx.notify();
    }

    /// Shared handler for both install and update plugin operations.
    /// `progress_toast` is shown while the operation runs; `success_toast` on success.
    #[cfg(not(target_family = "wasm"))]
    fn handle_plugin_operation<F, Fut>(
        &mut self,
        progress_toast: &str,
        error_label: &str,
        success_toast: &str,
        operation: F,
        ctx: &mut ViewContext<Self>,
    ) -> bool
    where
        F: FnOnce(Box<dyn CliAgentPluginManager>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), PluginInstallError>> + Send + 'static,
    {
        let Some(agent) = self.cli_agent(ctx) else {
            return false;
        };
        let shell_data = {
            let model = self.terminal_model.lock();
            model.active_shell_launch_data().cloned()
        };
        let (shell_path, shell_type) = match shell_data {
            Some(ShellLaunchData::Executable {
                executable_path,
                shell_type,
            })
            | Some(ShellLaunchData::MSYS2 {
                executable_path,
                shell_type,
            }) => (Some(executable_path), Some(shell_type)),
            // Shell not yet resolved (e.g. still bootstrapping).
            None => (None, None),
            // WSL is not supported for auto-install.
            Some(ShellLaunchData::WSL { .. }) => return false,
            // Auto-install isn't supported for Docker sandbox sessions — the
            // install would run against the *host's* shell config, not the
            // container's. `should_use_manual_mode` already routes sandbox
            // sessions to the manual-instructions modal, so this arm is a
            // defensive fallthrough; users can still install the plugin by
            // pasting the command into the sandbox PTY themselves.
            //
            // TODO(advait): Add native auto-install support for sandboxes,
            // e.g. by routing the install through the session's in-band
            // executor so it runs inside the container and targets the
            // container's shell / package layout. A common use case will be
            // running a 3p harness (e.g. Claude Code) inside a sandbox and
            // needing the Warp plugin to integrate with it.
            Some(ShellLaunchData::DockerSandbox { .. }) => return false,
        };

        // Await the interactive PATH so nvm-installed tools like `claude`
        // are on PATH, matching how LSP operations capture the PATH.
        let path_future = LocalShellState::handle(ctx).update(ctx, |shell_state, ctx| {
            shell_state.get_interactive_path_env_var(ctx)
        });

        self.plugin_operation_in_progress = true;
        ctx.notify();

        let window_id = ctx.window_id();
        let toast_id = "cli-agent-plugin-operation".to_owned();

        ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
            toast_stack.add_persistent_toast(
                DismissibleToast::default(progress_toast.to_owned())
                    .with_object_id(toast_id.clone()),
                window_id,
                ctx,
            );
        });

        let toast_id_for_callback = toast_id.clone();
        let error_label = error_label.to_owned();
        let success_toast = success_toast.to_owned();
        ctx.spawn(
            async move {
                let path_env_var = path_future.await;
                let Some(manager) =
                    plugin_manager_for_with_shell(agent, shell_path, shell_type, path_env_var)
                else {
                    return Err((
                        PluginInstallError {
                            message: "No plugin manager available".to_owned(),
                            log: String::new(),
                        },
                        None,
                    ));
                };

                match operation(manager).await {
                    Ok(()) => Ok(()),
                    Err(err) => {
                        let log_path = write_install_log(agent, &err).await;
                        Err((err, log_path))
                    }
                }
            },
            move |me, result, ctx| {
                me.plugin_operation_in_progress = false;

                if result.is_ok() {
                    ctx.emit(AgentInputFooterEvent::PluginInstalled(agent));
                }

                ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
                    let toast = match result {
                        Ok(()) => DismissibleToast::success(success_toast.clone()),
                        Err((err, log_path)) => {
                            let remote_host = CLIAgentSessionsModel::as_ref(ctx)
                                .session(me.terminal_view_id)
                                .and_then(|s| s.remote_host.clone());
                            CLIAgentSessionsModel::handle(ctx).update(ctx, |model, _| {
                                model.record_plugin_auto_failure(agent, remote_host);
                            });
                            log::error!(
                                "Failed plugin operation for {agent:?}: {err}\n{log}",
                                log = err.log,
                            );
                            let mut toast =
                                DismissibleToast::error(format!("{error_label}: {err}"));
                            if let Some(log_path) = log_path {
                                toast = toast.with_link(
                                    ToastLink::new("See logs for details".to_owned())
                                        .with_onclick_action(WorkspaceAction::OpenFilePath {
                                            path: log_path,
                                        }),
                                );
                            }
                            toast
                        }
                    };
                    toast_stack.add_ephemeral_toast(
                        toast.with_object_id(toast_id_for_callback),
                        window_id,
                        ctx,
                    );
                });
                ctx.notify();
            },
        );
        true
    }

    #[cfg(not(target_family = "wasm"))]
    fn handle_install_plugin(&mut self, ctx: &mut ViewContext<Self>) -> bool {
        let success_msg = self
            .cli_agent(ctx)
            .and_then(plugin_manager_for)
            .map(|m| m.install_success_message())
            .unwrap_or("Uncaged plugin installed. Please restart the session to activate.");
        self.handle_plugin_operation(
            "Installing Uncaged plugin...",
            "Failed to install Uncaged plugin",
            success_msg,
            |manager| async move { manager.install().await },
            ctx,
        )
    }

    #[cfg(not(target_family = "wasm"))]
    fn handle_update_plugin(&mut self, ctx: &mut ViewContext<Self>) -> bool {
        let success_msg = self
            .cli_agent(ctx)
            .and_then(plugin_manager_for)
            .map(|m| m.update_success_message())
            .unwrap_or("Uncaged plugin updated. Please restart the session to activate.");
        self.handle_plugin_operation(
            "Updating Uncaged plugin...",
            "Failed to update Uncaged plugin",
            success_msg,
            |manager| async move { manager.update().await },
            ctx,
        )
    }

    fn cli_display_chip(
        &self,
        chip_kind: ContextChipKind,
        app: &AppContext,
    ) -> Option<Box<dyn Element>> {
        self.cli_display_chips
            .iter()
            .find(|chip| chip.as_ref(app).chip_kind() == &chip_kind)
            .filter(|chip| chip.as_ref(app).should_render(app))
            .map(|chip| ChildView::new(chip).finish())
    }

    fn render_cli_toolbar_item(
        &self,
        item: &AgentToolbarItemKind,
        app: &AppContext,
    ) -> Option<Box<dyn Element>> {
        if !item.available_in().is_available_for_cli() {
            return None;
        }

        match item {
            AgentToolbarItemKind::ContextChip(chip_kind) => {
                self.cli_display_chip(chip_kind.clone(), app)
            }
            AgentToolbarItemKind::FileExplorer => {
                Some(ChildView::new(&self.file_explorer_button).finish())
            }
            AgentToolbarItemKind::RichInput => FeatureFlag::CLIAgentRichInput
                .is_enabled()
                .then(|| ChildView::new(&self.rich_input_button).finish()),
            AgentToolbarItemKind::FileAttach => Some(ChildView::new(&self.file_button).finish()),
            AgentToolbarItemKind::Settings => Some(ChildView::new(&self.settings_button).finish()),
            // Handled by the available_in() guard above; included for exhaustiveness.
            AgentToolbarItemKind::ModelSelector
            | AgentToolbarItemKind::NLDToggle
            | AgentToolbarItemKind::ContextWindowUsage
            | AgentToolbarItemKind::FastForwardToggle => None,
        }
    }

    fn render_cli_mode_footer(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let cli_icon_size = ButtonSize::AgentInputButton.icon_size(appearance, app);

        // Extract everything we need from the terminal model up front and drop
        // the lock before calling into helpers like `should_use_manual_mode`
        // and `render_cli_toolbar_item`, which may re-lock the same model and
        // would deadlock since the lock is non-reentrant.
        let background_color = {
            let terminal_model = self.terminal_model.lock();
            let background_color = if terminal_model.is_alt_screen_active() {
                terminal_model
                    .alt_screen()
                    .inferred_bg_color()
                    .unwrap_or_else(|| appearance.theme().surface_1().into_solid())
            } else {
                appearance.theme().surface_1().into_solid()
            };
            background_color
        };

        let session_settings = SessionSettings::as_ref(app);
        let left_items = session_settings
            .cli_agent_footer_chip_selection
            .left_items();
        let right_items = session_settings
            .cli_agent_footer_chip_selection
            .right_items();

        let mut left_buttons = Wrap::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_alignment(MainAxisAlignment::Start)
            .with_run_spacing(4.)
            .with_spacing(4.);

        // CLI agent brand icon is always rendered (not configurable).
        if let Some(agent) = self.cli_agent(app) {
            if let Some(icon) = agent.icon() {
                let icon_color = agent
                    .brand_color()
                    .map(|c| c.on_background(background_color, MinimumAllowedContrast::NonText))
                    .unwrap_or_else(|| appearance.theme().foreground().into_solid());
                left_buttons.add_child(
                    Container::new(
                        ConstrainedBox::new(icon.to_warpui_icon(Fill::Solid(icon_color)).finish())
                            .with_width(cli_icon_size)
                            .with_height(cli_icon_size)
                            .finish(),
                    )
                    .with_padding_right(8.)
                    .finish(),
                );
            }
        }

        if let Some(chip_kind) = self.plugin_chip_kind(app) {
            let manual = self.should_use_manual_mode(app);
            let chip = match (chip_kind, manual) {
                (PluginChipKind::Install, false) => {
                    ChildView::new(&self.install_plugin_button).finish()
                }
                (PluginChipKind::Install, true) => {
                    ChildView::new(&self.plugin_instructions_button).finish()
                }
                (PluginChipKind::Update, false) => {
                    ChildView::new(&self.update_plugin_button).finish()
                }
                (PluginChipKind::Update, true) => {
                    ChildView::new(&self.update_instructions_button).finish()
                }
            };
            let chip_with_dismiss = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(chip)
                .with_child(ChildView::new(&self.dismiss_plugin_chip_button).finish())
                .finish();
            left_buttons.add_child(chip_with_dismiss);
        }

        for item in &left_items {
            if let Some(element) = self.render_cli_toolbar_item(item, app) {
                left_buttons.add_child(element);
            }
        }

        let mut right_buttons = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_size(MainAxisSize::Min)
            .with_spacing(4.);

        for item in &right_items {
            if let Some(element) = self.render_cli_toolbar_item(item, app) {
                right_buttons.add_child(element);
            }
        }

        let content = Wrap::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(WrapFillEntireRun::new(left_buttons.finish()).finish())
            .with_child(WrapFill::new(0., right_buttons.finish()).finish())
            .with_run_spacing(context_chips::spacing::UDI_ROW_RUN_SPACING)
            .finish();
        let content = EventHandler::new(content)
            .on_right_mouse_down(|ctx, _, position| {
                ctx.dispatch_typed_action(AgentInputFooterAction::ShowContextMenu { position });
                DispatchEventResult::StopPropagation
            })
            .finish();

        Container::new(content).with_vertical_padding(4.).finish()
    }

    pub fn has_open_chip_menu(&self, app: &AppContext) -> bool {
        let has_open_display_chip = self
            .all_display_chips()
            .any(|chip| chip.as_ref(app).display_chip_kind().has_open_menu());

        has_open_display_chip
    }

    pub fn is_model_selector_open(&self, app: &AppContext) -> bool {
        self.model_selector.as_ref(app).is_open()
    }

    fn update_ftu_callout_render_state(&mut self, ctx: &mut ViewContext<Self>) {
        let ftu_dismissed = *AISettings::as_ref(ctx).ftu_model_callout_dismissed;
        if !self.render_ftu_callout && ftu_dismissed {
            return;
        }

        let showing_ftu_model_picker = FeatureFlag::InlineMenuHeaders.is_enabled()
            && self
                .terminal_model
                .lock()
                .block_list()
                .active_block()
                .is_agent_in_control_or_tagged_in();
        if showing_ftu_model_picker && !ftu_dismissed {
            if !self.render_ftu_callout {
                self.render_ftu_callout = true;
                ctx.notify();
            }
            AISettings::handle(ctx).update(ctx, |settings, ctx| {
                // This setting actually indicates whether we've shown the ftu callout at all,
                // but it originally tracked whether the user manually dismissed the callout and
                // we don't want to resurface the callout to folks who have already dismissed.
                let _ = settings.ftu_model_callout_dismissed.set_value(true, ctx);
            });
        } else if !showing_ftu_model_picker && self.render_ftu_callout {
            self.render_ftu_callout = false;
            ctx.notify();
        }
    }

    fn handle_profile_model_selector_event(
        &mut self,
        event: &ProfileModelSelectorEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            ProfileModelSelectorEvent::MenuVisibilityChanged { open } => {
                if *open {
                    ctx.emit(AgentInputFooterEvent::ModelSelectorOpened);
                } else {
                    ctx.emit(AgentInputFooterEvent::ModelSelectorClosed);
                }
            }
            ProfileModelSelectorEvent::OpenSettings(section) => {
                ctx.emit(AgentInputFooterEvent::OpenSettings(*section));
            }
            ProfileModelSelectorEvent::ToggleInlineModelSelector => {
                if self.render_ftu_callout {
                    self.render_ftu_callout = false;
                    ctx.notify();
                }

                let initial_tab = if self
                    .terminal_model
                    .lock()
                    .block_list()
                    .active_block()
                    .is_agent_in_control_or_tagged_in()
                {
                    InlineModelSelectorTab::FullTerminalUse
                } else {
                    InlineModelSelectorTab::BaseAgent
                };

                ctx.emit(AgentInputFooterEvent::ToggleInlineModelSelector { initial_tab });
            }
        }
    }

    fn sync_fast_forward_button(&self, ctx: &mut ViewContext<Self>) {
        // In cloud agent conversations fast forward is force-enabled.
        let terminal_model = self.terminal_model.lock();
        let is_force_enabled = is_in_cloud_context(
            terminal_model.block_list().agent_view_state(),
            &terminal_model,
        );
        drop(terminal_model);

        // Read directly from the conversation, same data source as the warping
        // indicator footer's auto-approve chip.
        let is_active = BlocklistAIHistoryModel::as_ref(ctx)
            .active_conversation(self.terminal_view_id)
            .map(|c| c.autoexecute_any_action())
            .unwrap_or(false)
            || is_force_enabled;

        let icon = if is_active {
            Icon::FastForwardFilled
        } else {
            Icon::FastForward
        };
        let tooltip = if is_force_enabled {
            FAST_FORWARD_LOCKED_TOOLTIP
        } else if is_active {
            FAST_FORWARD_ON_TOOLTIP
        } else {
            FAST_FORWARD_OFF_TOOLTIP
        };

        self.fast_forward_button.update(ctx, |button, ctx| {
            button.set_icon(Some(icon), ctx);
            button.set_tooltip(Some(tooltip), ctx);
            button.set_active(is_active, ctx);
            button.set_disabled(is_force_enabled, ctx);
        });
    }

    fn update_context_window_button(&mut self, ctx: &mut ViewContext<Self>) {
        if let Some(conversation) =
            BlocklistAIHistoryModel::as_ref(ctx).active_conversation(self.terminal_view_id)
        {
            let usage = conversation.context_window_usage();
            let icon = icon_for_context_window_usage(usage);
            let remaining_pct = ((1.0 - usage) * 100.0).round() as i32;

            let expiry = conversation.latest_exchange().and_then(|exchange| {
                let output = exchange.output_status.output()?;
                output.get().model_info.as_ref()?.prompt_cache_expires_at
            });
            let is_cache_expired = FeatureFlag::PromptCacheExpiryWarning.is_enabled()
                && expiry.is_some_and(|expiry| expiry <= Local::now());
            let context_remaining_tooltip = format!("{remaining_pct}% context remaining");
            let tooltip = if is_cache_expired {
                format!("{context_remaining_tooltip} · prompt cache expired")
            } else {
                context_remaining_tooltip
            };

            self.prompt_cache_expired = is_cache_expired;
            self.context_window_button.update(ctx, |button, ctx| {
                button.set_icon(Some(icon), ctx);
                button.set_tooltip(Some(tooltip), ctx);
            });

            self.reschedule_prompt_cache_expiry_timer(expiry, ctx);
        }
    }

    /// Schedules a refresh of the context-window button at the prompt-cache
    /// expiry instant so the notification dot appears while the conversation is idle.
    fn reschedule_prompt_cache_expiry_timer(
        &mut self,
        expiry: Option<DateTime<Local>>,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(handle) = self.prompt_cache_expiry_timer_handle.take() {
            handle.abort();
        }
        if !FeatureFlag::PromptCacheExpiryWarning.is_enabled() {
            return;
        }
        // Only future expiries need a timer; past ones already render as expired.
        let Some(delay) = expiry.and_then(|expiry| (expiry - Local::now()).to_std().ok()) else {
            return;
        };
        let handle = ctx.spawn(
            async move {
                Timer::after(delay).await;
            },
            |me, _, ctx| {
                me.prompt_cache_expiry_timer_handle = None;
                me.update_context_window_button(ctx);
                ctx.notify();
            },
        );
        self.prompt_cache_expiry_timer_handle = Some(handle);
    }

    fn render_toolbar_item(
        &self,
        item: &AgentToolbarItemKind,
        app: &AppContext,
    ) -> Option<Box<dyn Element>> {
        if !item.available_in().is_available_for_agent_view() {
            return None;
        }

        match item {
            AgentToolbarItemKind::ContextChip(chip_kind) => {
                let chips = match SessionSettings::as_ref(app)
                    .agent_footer_chip_selection
                    .left_chips()
                    .contains(chip_kind)
                {
                    true => &self.left_display_chips,
                    false => &self.right_display_chips,
                };
                chips
                    .iter()
                    .find(|chip| chip.as_ref(app).chip_kind() == chip_kind)
                    .filter(|chip| chip.as_ref(app).should_render(app))
                    .map(|chip| ChildView::new(chip).finish())
            }
            AgentToolbarItemKind::ModelSelector => {
                let show = FeatureFlag::ProfilesDesignRevamp.is_enabled()
                    || *SessionSettings::as_ref(app).show_model_selectors_in_prompt;
                show.then(|| ChildView::new(&self.model_selector).finish())
            }
            AgentToolbarItemKind::NLDToggle => Some(ChildView::new(&self.nld_button).finish()),
            AgentToolbarItemKind::FileAttach => Some(ChildView::new(&self.file_button).finish()),
            AgentToolbarItemKind::ContextWindowUsage => {
                let has_conversation = FeatureFlag::ContextWindowUsageV2.is_enabled()
                    && BlocklistAIHistoryModel::as_ref(app)
                        .active_conversation(self.terminal_view_id)
                        .is_some();
                has_conversation.then(|| {
                    let chip = ChildView::new(&self.context_window_button).finish();
                    if !self.prompt_cache_expired {
                        return chip;
                    }

                    let appearance = Appearance::as_ref(app);
                    let dot = Container::new(
                        ConstrainedBox::new(Empty::new().finish())
                            .with_width(6.)
                            .with_height(6.)
                            .finish(),
                    )
                    .with_corner_radius(CornerRadius::with_all(Radius::Percentage(50.)))
                    .with_background(Fill::Solid(
                        AnsiColorIdentifier::Yellow
                            .to_ansi_color(&appearance.theme().terminal_colors().normal)
                            .into(),
                    ))
                    .finish();

                    let mut stack = Stack::new();
                    stack.add_child(chip);
                    stack.add_positioned_overlay_child(
                        dot,
                        OffsetPositioning::offset_from_parent(
                            vec2f(3., -3.),
                            ParentOffsetBounds::WindowByPosition,
                            ParentAnchor::TopRight,
                            ChildAnchor::TopRight,
                        ),
                    );
                    stack.finish()
                })
            }
            AgentToolbarItemKind::FastForwardToggle => FeatureFlag::FastForwardAutoexecuteButton
                .is_enabled()
                .then(|| ChildView::new(&self.fast_forward_button).finish()),
            // Handled by the available_in() guard above; included for exhaustiveness.
            AgentToolbarItemKind::FileExplorer
            | AgentToolbarItemKind::RichInput
            | AgentToolbarItemKind::Settings => None,
        }
    }

    #[cfg(test)]
    pub fn displayed_chip_kinds(
        &self,
        app: &AppContext,
    ) -> (
        Vec<crate::context_chips::ContextChipKind>,
        Vec<crate::context_chips::ContextChipKind>,
    ) {
        let collect_chip_kinds = |chips: &[ViewHandle<DisplayChip>]| {
            chips
                .iter()
                .map(|chip| chip.as_ref(app).chip_kind().clone())
                .collect()
        };

        (
            collect_chip_kinds(&self.left_display_chips),
            collect_chip_kinds(&self.right_display_chips),
        )
    }

    #[cfg(test)]
    pub fn cli_display_chip_kinds(
        &self,
        app: &AppContext,
    ) -> Vec<crate::context_chips::ContextChipKind> {
        self.cli_display_chips
            .iter()
            .map(|chip| chip.as_ref(app).chip_kind().clone())
            .collect()
    }
}

impl View for AgentInputFooter {
    fn ui_name() -> &'static str {
        "AgentViewFooter"
    }

    fn render(&self, app: &warpui::AppContext) -> Box<dyn warpui::Element> {
        // When a CLI agent session is active, render the CLI agent toolbar instead.
        if self.is_cli_agent_session_active(app) {
            return self.render_cli_mode_footer(app);
        }

        let session_settings = SessionSettings::as_ref(app);
        let left_items = session_settings.agent_footer_chip_selection.left_items();
        let right_items = session_settings.agent_footer_chip_selection.right_items();

        let mut left_buttons = Wrap::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_alignment(MainAxisAlignment::Start)
            .with_run_spacing(4.)
            .with_spacing(4.);

        // Uncaged: one row for both modes. The switch is always first; Terminal mode shows
        // the switch and nothing else, Agent mode adds the agent tooling after it. Context
        // chips are left out while the switch is here, because the prompt row above the
        // editor already shows them in both modes.
        if let Some(mode_toggle) = self.mode_toggle.as_ref() {
            left_buttons = left_buttons.with_child(ChildView::new(mode_toggle).finish());
        }
        let is_terminal_mode = self.mode_toggle.is_some()
            && matches!(
                self.ai_input_model.as_ref(app).input_type(),
                InputType::Shell
            );
        let hide_context_chips = self.mode_toggle.is_some();
        let is_hidden = |item: &AgentToolbarItemKind| {
            is_terminal_mode || (hide_context_chips && item.is_context_chip())
        };
        let left_items: Vec<_> = left_items
            .into_iter()
            .filter(|item| !is_hidden(item))
            .collect();
        let right_items: Vec<_> = right_items
            .into_iter()
            .filter(|item| !is_hidden(item))
            .collect();

        for item in &left_items {
            if let Some(element) = self.render_toolbar_item(item, app) {
                left_buttons.add_child(element);
            }
        }

        let mut right_buttons = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_size(MainAxisSize::Min)
            .with_spacing(4.);

        let has_prompt_alert = !is_terminal_mode && !self.prompt_alert.as_ref(app).is_no_alert();
        if has_prompt_alert {
            right_buttons.add_child(
                Shrinkable::new(
                    1.,
                    Clipped::new(ChildView::new(&self.prompt_alert).finish()).finish(),
                )
                .finish(),
            );
        } else {
            for item in &right_items {
                if let Some(element) = self.render_toolbar_item(item, app) {
                    right_buttons.add_child(element);
                }
            }
        }

        let content = Wrap::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(WrapFill::new(0., left_buttons.finish()).finish())
            .with_child(WrapFill::new(0., right_buttons.finish()).finish())
            .with_run_spacing(context_chips::spacing::UDI_ROW_RUN_SPACING)
            .finish();
        let content = EventHandler::new(content)
            .on_right_mouse_down(|ctx, _, position| {
                ctx.dispatch_typed_action(AgentInputFooterAction::ShowContextMenu { position });
                DispatchEventResult::StopPropagation
            })
            .finish();

        let mut container = Container::new(content).with_padding_bottom(8.0);
        if !has_prompt_alert {
            container = container.with_padding_right(16.);
        }

        // If the model chip has switched to show the ftu model options
        // (and this is the first time this has happened)
        // we show a little callout explaining the change.
        let showing_ftu_model_picker = FeatureFlag::InlineMenuHeaders.is_enabled()
            && self
                .terminal_model
                .lock()
                .block_list()
                .active_block()
                .is_agent_in_control_or_tagged_in();

        if showing_ftu_model_picker && self.render_ftu_callout {
            let mut stack = Stack::new();
            stack.add_child(container.finish());
            stack.add_positioned_overlay_child(
                render_ftu_callout(&self.ftu_callout_close_button, app),
                OffsetPositioning::offset_from_save_position_element(
                    "profile_model_selector_model_button",
                    vec2f(8., -8.),
                    PositionedElementOffsetBounds::WindowByPosition,
                    PositionedElementAnchor::TopRight,
                    ChildAnchor::BottomRight,
                ),
            );
            stack.finish()
        } else {
            container.finish()
        }
    }
}

/// Render a message bubble calling out that the model has switched now that we're in FTU mode.
/// This callout is dismissable and does not re-appear once you've dismissed it once.
fn render_ftu_callout(
    close_button: &ViewHandle<ActionButton>,
    app: &AppContext,
) -> Box<dyn Element> {
    let appearance = Appearance::as_ref(app);
    let theme = appearance.theme();
    let background = theme.background().blend(&theme.accent().with_opacity(50));
    let text_color = internal_colors::text_main(theme, background.into_solid());

    let callout_box = ConstrainedBox::new(
        Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(8.)
                .with_child(
                    Expanded::new(
                        1.,
                        Text::new(
                            "Now using Full Terminal Agent's default model.",
                            appearance.ui_font_family(),
                            appearance.monospace_font_size() - 2.,
                        )
                        .with_color(text_color)
                        .with_line_height_ratio(DEFAULT_UI_LINE_HEIGHT_RATIO)
                        .with_selectable(false)
                        .finish(),
                    )
                    .finish(),
                )
                .with_child(
                    Container::new(ChildView::new(close_button).finish())
                        .with_margin_top(-3.)
                        .finish(),
                )
                .finish(),
        )
        .with_vertical_padding(12.)
        .with_horizontal_padding(16.)
        .with_background(background)
        .with_border(Border::all(1.).with_border_fill(theme.accent()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
        .finish(),
    )
    .with_width(348.)
    .finish();

    // The way that we render the little triangle in the bottom of the message bubble
    // is by rendering two triangle icons (a filled triangle and an outlined triangle) and then
    // stacking them on top of each other below the message bubble. I don't think there's a simpler
    // way to do this with our UI framework.
    let triangle_stack = Stack::new()
        .with_child(
            ConstrainedBox::new(
                Icon::CalloutTriangleBorderDown
                    .to_warpui_icon(Fill::Solid(theme.accent().into_solid()))
                    .finish(),
            )
            .with_width(24.)
            .with_height(24.)
            .finish(),
        )
        .with_child(
            ConstrainedBox::new(
                Icon::CalloutTriangleFillDown
                    .to_warpui_icon(background)
                    .finish(),
            )
            .with_width(24.)
            .with_height(24.)
            .finish(),
        );

    Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_child(callout_box)
        .with_child(
            Container::new(triangle_stack.finish())
                .with_margin_left(300.)
                .with_margin_top(-3.)
                .finish(),
        )
        .finish()
}

#[derive(Debug, Clone)]
pub enum AgentInputFooterAction {
    SelectFile,
    InsertFilePath(String),
    ToggleCodeReview,
    ToggleFileExplorer,
    ToggleRichInput,
    ToggleAutodetectionSetting,
    DismissFtuModelCallout,
    InstallPlugin,
    UpdatePlugin,
    OpenPluginInstallInstructionsPane,
    OpenPluginUpdateInstructionsPane,
    DismissPluginChip,
    OpenCodingAgentSettings,
    ShowContextMenu { position: Vector2F },
}

impl TypedActionView for AgentInputFooter {
    type Action = AgentInputFooterAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut warpui::ViewContext<Self>) {
        match action {
            AgentInputFooterAction::SelectFile => {
                // Fork based on CLI agent session: in CLI mode, open a file
                // picker and insert/write the path; in normal mode, use the
                // standard AI file attachment flow.
                if self.is_cli_agent_session_active(ctx) {
                    self.select_cli_file(ctx);
                } else {
                    ctx.emit(AgentInputFooterEvent::SelectFile);
                }
            }
            AgentInputFooterAction::InsertFilePath(path) => {
                let path_with_space = format!("{path} ");
                if self.has_active_cli_agent_input_session(ctx) {
                    ctx.emit(AgentInputFooterEvent::InsertIntoCLIRichInput(
                        path_with_space,
                    ));
                } else {
                    ctx.emit(AgentInputFooterEvent::WriteToPty(path_with_space));
                }
            }
            AgentInputFooterAction::ToggleCodeReview => {
                if let Some(agent) = self.cli_agent(ctx) {
                    ctx.emit(AgentInputFooterEvent::ToggleCodeReviewPane(agent));
                }
            }
            AgentInputFooterAction::ToggleFileExplorer => {
                if self.cli_agent(ctx).is_some() {
                    ctx.emit(AgentInputFooterEvent::ToggleFileExplorer);
                }
            }
            AgentInputFooterAction::ToggleRichInput => {
                if self.has_active_cli_agent_input_session(ctx) {
                    ctx.emit(AgentInputFooterEvent::HideRichInput);
                } else {
                    ctx.emit(AgentInputFooterEvent::OpenRichInput);
                }
            }
            AgentInputFooterAction::ToggleAutodetectionSetting => {
                let ai_settings = AISettings::handle(ctx);
                ai_settings.update(ctx, |settings, ctx| {
                    report_if_error!(settings
                        .ai_autodetection_enabled_internal
                        .toggle_and_save_value(ctx));
                });
            }
            AgentInputFooterAction::DismissFtuModelCallout => {
                if self.render_ftu_callout {
                    self.render_ftu_callout = false;
                    ctx.notify();
                }
            }
            AgentInputFooterAction::InstallPlugin => {
                #[cfg(not(target_family = "wasm"))]
                {
                    if !self.handle_install_plugin(ctx) {
                        self.record_plugin_auto_failure_and_notify(ctx);
                    }
                }
            }
            AgentInputFooterAction::UpdatePlugin => {
                #[cfg(not(target_family = "wasm"))]
                {
                    if !self.handle_update_plugin(ctx) {
                        self.record_plugin_auto_failure_and_notify(ctx);
                    }
                }
            }
            AgentInputFooterAction::OpenPluginInstallInstructionsPane => {
                #[cfg(not(target_family = "wasm"))]
                if let Some(agent) = self.cli_agent(ctx) {
                    ctx.emit(AgentInputFooterEvent::OpenPluginInstructionsPane(
                        agent,
                        PluginModalKind::Install,
                    ));
                }
            }
            AgentInputFooterAction::OpenPluginUpdateInstructionsPane => {
                #[cfg(not(target_family = "wasm"))]
                if let Some(agent) = self.cli_agent(ctx) {
                    ctx.emit(AgentInputFooterEvent::OpenPluginInstructionsPane(
                        agent,
                        PluginModalKind::Update,
                    ));
                }
            }
            AgentInputFooterAction::DismissPluginChip => {
                let chip_kind = self.plugin_chip_kind(ctx);
                let is_update = matches!(chip_kind, Some(PluginChipKind::Update));
                let session = CLIAgentSessionsModel::as_ref(ctx)
                    .session(self.terminal_view_id)
                    .cloned();
                if let Some(session) = session {
                    let chip_key =
                        plugin_chip_key(session.agent.command_prefix(), &session.remote_host);
                    if is_update {
                        #[cfg(not(target_family = "wasm"))]
                        if let Some(manager) = plugin_manager_for(session.agent) {
                            let version = manager.minimum_plugin_version().to_owned();
                            AISettings::handle(ctx).update(ctx, |settings, ctx| {
                                settings.dismiss_plugin_update_chip(&chip_key, version, ctx);
                            });
                        }
                    } else {
                        AISettings::handle(ctx).update(ctx, |settings, ctx| {
                            settings.dismiss_plugin_install_chip(&chip_key, ctx);
                        });
                    }
                }
                ctx.notify();
            }
            AgentInputFooterAction::OpenCodingAgentSettings => {
                #[cfg(not(target_family = "wasm"))]
                ctx.dispatch_typed_action_deferred(WorkspaceAction::ScrollToSettingsWidget {
                    page: SettingsSection::ThirdPartyCLIAgents,
                    widget_id: crate::settings_view::cli_agent_settings_widget_id(),
                });
            }
            AgentInputFooterAction::ShowContextMenu { position } => {
                ctx.emit(AgentInputFooterEvent::ShowContextMenu {
                    position: *position,
                });
            }
        }
    }
}

pub enum AgentInputFooterEvent {
    SelectFile,
    WriteToPty(String),
    /// Insert text into the CLI agent rich input.
    InsertIntoCLIRichInput(String),
    ToggleCodeReviewPane(CLIAgent),
    ToggleFileExplorer,
    OpenRichInput,
    HideRichInput,
    ToggledChipMenu {
        open: bool,
    },
    TryExecuteChipCommand(PromptChipShellCommand),
    PromptAlert(PromptAlertEvent),
    ModelSelectorOpened,
    ModelSelectorClosed,
    ToggleInlineModelSelector {
        initial_tab: InlineModelSelectorTab,
    },
    OpenSettings(SettingsSection),
    OpenCodeReview,
    OpenAIDocument {
        document_id: AIDocumentId,
        document_version: AIDocumentVersion,
    },
    ShowContextMenu {
        position: Vector2F,
    },
    PluginInstalled(CLIAgent),
    #[cfg(not(target_family = "wasm"))]
    OpenPluginInstructionsPane(CLIAgent, PluginModalKind),
}

impl Entity for AgentInputFooter {
    type Event = AgentInputFooterEvent;
}

pub(crate) struct AgentInputButtonTheme;

impl ActionButtonTheme for AgentInputButtonTheme {
    fn background(&self, hovered: bool, appearance: &Appearance) -> Option<Fill> {
        // Solid surface fills keep the button readable even when its parent
        // isn't `theme.background()` (for example, over an alt-screen CLI agent).
        let theme = appearance.theme();
        Some(if hovered {
            theme.surface_2()
        } else {
            theme.surface_1()
        })
    }

    fn text_color(
        &self,
        _hovered: bool,
        background: Option<Fill>,
        appearance: &Appearance,
    ) -> ColorU {
        // If a caller overrides `background()` with a translucent fill, blend
        // it over `surface_1` so text contrast is computed against the actual
        // rendered color rather than the raw overlay.
        let base_bg = appearance.theme().surface_1();
        let effective_bg = background
            .map(|overlay| base_bg.blend(&overlay))
            .unwrap_or(base_bg);

        appearance.theme().sub_text_color(effective_bg).into_solid()
    }

    fn border(&self, appearance: &Appearance) -> Option<ColorU> {
        Some(internal_colors::neutral_3(appearance.theme()))
    }

    fn should_opt_out_of_contrast_adjustment(&self) -> bool {
        true
    }
}

/// Green-accented theme for the "Install Warp plugin" chip.
struct InstallPluginButtonTheme;

impl ActionButtonTheme for InstallPluginButtonTheme {
    fn background(&self, hovered: bool, appearance: &Appearance) -> Option<Fill> {
        let green = appearance.theme().ansi_fg_green();
        let base = appearance.theme().surface_1();
        Some(if hovered {
            base.blend(&Fill::Solid(green).with_opacity(30))
        } else {
            base.blend(&Fill::Solid(green).with_opacity(15))
        })
    }

    fn text_color(
        &self,
        _hovered: bool,
        _background: Option<Fill>,
        appearance: &Appearance,
    ) -> ColorU {
        appearance.theme().ansi_fg_green()
    }

    fn border(&self, appearance: &Appearance) -> Option<ColorU> {
        let green = appearance.theme().ansi_fg_green();
        Some(ColorU::new(green.r, green.g, green.b, 80))
    }

    fn should_opt_out_of_contrast_adjustment(&self) -> bool {
        true
    }
}

/// Writes the detailed plugin installation log to a temp file.
/// Returns the log file path on success, or `None` if writing failed.
#[cfg(not(target_family = "wasm"))]
async fn write_install_log(agent: CLIAgent, err: &PluginInstallError) -> Option<PathBuf> {
    let log_path = env::temp_dir().join("warp-plugin-install.log");
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    let contents = format!(
        "Warp plugin installation — {agent:?}\n\
         {now}\n\
         \n\
         {log}",
        log = err.log,
    );
    fs::write(&log_path, contents).await.ok()?;
    Some(log_path)
}

/// Keeps the auto-approve chip's muted text semantics while using the shared opaque chip fill.
struct FastForwardButtonTheme;

impl ActionButtonTheme for FastForwardButtonTheme {
    fn background(&self, hovered: bool, appearance: &Appearance) -> Option<Fill> {
        AgentInputButtonTheme.background(hovered, appearance)
    }

    fn text_color(
        &self,
        _hovered: bool,
        _background: Option<Fill>,
        appearance: &Appearance,
    ) -> ColorU {
        appearance
            .theme()
            .sub_text_color(appearance.theme().surface_1())
            .into_solid()
    }

    fn border(&self, appearance: &Appearance) -> Option<ColorU> {
        AgentInputButtonTheme.border(appearance)
    }

    fn should_opt_out_of_contrast_adjustment(&self) -> bool {
        true
    }
}

/// Disabled-state theme used by the fast-forward chip when fast-forward is
/// locked on (cloud agent conversations). Delegates entirely to
/// `FastForwardButtonTheme`, but forces `hovered=true` on the background so
/// the chip still reads as "on" while the underlying button is disabled
/// (which gives us the arrow cursor and no-op click handler for free).
struct FastForwardLockedTheme;

impl ActionButtonTheme for FastForwardLockedTheme {
    fn background(&self, _hovered: bool, appearance: &Appearance) -> Option<Fill> {
        // Force the active (hovered) background so the disabled chip still
        // visually looks like fast-forward is on.
        FastForwardButtonTheme.background(true, appearance)
    }

    fn text_color(
        &self,
        hovered: bool,
        background: Option<Fill>,
        appearance: &Appearance,
    ) -> ColorU {
        FastForwardButtonTheme.text_color(hovered, background, appearance)
    }

    fn border(&self, appearance: &Appearance) -> Option<ColorU> {
        FastForwardButtonTheme.border(appearance)
    }

    fn should_opt_out_of_contrast_adjustment(&self) -> bool {
        FastForwardButtonTheme.should_opt_out_of_contrast_adjustment()
    }
}

/// Same as `AgentInputButtonTheme`, except with one-off special active styling for the NLD button.
/// Uncaged: `ActionButton` draws its active state with the hover styles, so a theme that turns
/// blue on hover made an off toggle look on the moment the pointer was over it -- right after
/// clicking it off. The theme carries the real state instead: blue only while autodetection is
/// on, and hover merely lifts the icon from disabled grey to the usual sub-text grey.
#[derive(Clone)]
struct NLDButtonTheme {
    is_enabled: bool,
}

impl ActionButtonTheme for NLDButtonTheme {
    fn background(&self, hovered: bool, appearance: &Appearance) -> Option<Fill> {
        AgentInputButtonTheme.background(hovered, appearance)
    }

    fn text_color(
        &self,
        hovered: bool,
        _background: Option<Fill>,
        appearance: &Appearance,
    ) -> ColorU {
        let theme = appearance.theme();
        if self.is_enabled {
            theme.ansi_fg_blue()
        } else if hovered {
            theme.sub_text_color(theme.surface_1()).into_solid()
        } else {
            theme.disabled_text_color(theme.surface_1()).into_solid()
        }
    }

    fn border(&self, appearance: &Appearance) -> Option<ColorU> {
        AgentInputButtonTheme.border(appearance)
    }

    fn should_opt_out_of_contrast_adjustment(&self) -> bool {
        true
    }
}
