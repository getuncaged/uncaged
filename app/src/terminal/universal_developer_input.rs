use std::borrow::Cow;
use std::boxed::Box;
use std::cell::RefCell;
use std::rc::Rc;

use pathfinder_color::ColorU;
#[cfg(not(target_family = "wasm"))]
use settings::Setting as _;
use warp_core::ui::appearance::Appearance;
use warp_core::ui::color::contrast::{
    foreground_color_with_minimum_contrast, MinimumAllowedContrast,
};
use warp_core::ui::color::{coloru_with_opacity, Opacity, Rgb};
use warp_core::ui::theme::color::internal_colors;
use warpui::elements::{ChildView, CornerRadius, Fill, Radius};
use warpui::ui_components::components::UiComponentStyles;
use warpui::ui_components::segmented_control::{
    RenderableOptionConfig, SegmentedControl, SegmentedControlEvent, TooltipConfig,
};
use warpui::{
    AppContext, Element, Entity, ModelHandle, SingletonEntity as _, View, ViewContext, ViewHandle,
};

use crate::ai::blocklist::block::cli_controller::CLISubagentController;
use crate::ai::blocklist::{BlocklistAIInputModel, InputConfig, InputType};
#[cfg(not(target_family = "wasm"))]
use crate::search::ai_context_menu::view::AIContextMenu;
#[cfg(not(target_family = "wasm"))]
use crate::settings::InputSettings;
use crate::settings::{AISettings, AISettingsChangedEvent};
use crate::terminal::keys::TerminalKeybindings;
use crate::terminal::model::block::BlockMetadata;
#[cfg(not(target_family = "wasm"))]
use crate::terminal::model::session::SessionType;
use crate::terminal::model::session::Sessions;
use crate::terminal::shared_session::permissions_manager::SessionPermissionsManager;
use crate::ui_components::icons::Icon;

pub enum AtContextMenuDisabledReason {
    #[cfg(target_family = "wasm")]
    Wasm,
    #[cfg(not(target_family = "wasm"))]
    NoObjectsAvailable,
    #[cfg(not(target_family = "wasm"))]
    SshWithoutRemoteServer,
    #[cfg(not(target_family = "wasm"))]
    Subshell,
    #[cfg(not(target_family = "wasm"))]
    DisabledInTerminalMode,
}

impl AtContextMenuDisabledReason {
    #[cfg(target_family = "wasm")]
    pub fn get_disable_reason(
        _active_block_metadata: Option<&BlockMetadata>,
        _sessions: &Sessions,
        _input_config: &InputConfig,
        _ctx: &AppContext,
    ) -> Option<AtContextMenuDisabledReason> {
        Some(AtContextMenuDisabledReason::Wasm)
    }

    #[cfg(not(target_family = "wasm"))]
    pub fn get_disable_reason(
        active_block_metadata: Option<&BlockMetadata>,
        sessions: &Sessions,
        input_config: &InputConfig,
        ctx: &AppContext,
    ) -> Option<AtContextMenuDisabledReason> {
        // Derive session information from block metadata and sessions
        let (is_ssh_without_remote_server, is_subshell) = active_block_metadata
            .and_then(|metadata| metadata.session_id())
            .and_then(|session_id| sessions.get(session_id))
            .map(|session| {
                let session_type = session.session_type();
                let has_connected_remote_server = matches!(
                    session_type,
                    SessionType::WarpifiedRemote { host_id: Some(_) }
                );
                // The @ menu requires repo metadata which is only available for:
                // - Local sessions
                // - WarpifiedRemote sessions with a connected remote server (host_id is Some)
                //
                // Block when:
                // - SSH wrapper session without a remote server upgrade
                // - WarpifiedRemote still connecting (host_id is None)
                //
                // Note: is_ssh_wrapper_session() is set at bootstrap time and stays true
                // even after the session transitions to WarpifiedRemote with a host_id.
                // So we must check has_connected_remote_server first to avoid
                // incorrectly blocking upgraded sessions.
                let is_ssh_without_remote_server = !has_connected_remote_server
                    && (session.is_ssh_wrapper_session()
                        || matches!(session_type, SessionType::WarpifiedRemote { host_id: None }));
                let is_subshell = session.subshell_info().is_some();
                (is_ssh_without_remote_server, is_subshell)
            })
            .unwrap_or((false, false));

        // Only check the setting if we're in shell mode
        if input_config.input_type == InputType::Shell
            && !*InputSettings::as_ref(ctx)
                .at_context_menu_in_terminal_mode
                .value()
        {
            return Some(AtContextMenuDisabledReason::DisabledInTerminalMode);
        }

        if is_ssh_without_remote_server {
            return Some(AtContextMenuDisabledReason::SshWithoutRemoteServer);
        }
        if is_subshell {
            return Some(AtContextMenuDisabledReason::Subshell);
        }

        // Allow @ context menu outside of git repositories, when we have categories available.
        // Repo-based restrictions will be enforced at the category level
        // (e.g., Code will only be available inside a git repository).

        // This condition kicks in if we're locked in shell mode and not in a git repository, so we have
        // no categories available.
        if AIContextMenu::get_categories_for_mode(
            input_config.input_type.is_ai() || !input_config.is_locked,
            false,
            false, /* is_in_ambient_agent */
            false, /* is_cli_agent_input */
            ctx,
        )
        .is_empty()
        {
            return Some(AtContextMenuDisabledReason::NoObjectsAvailable);
        }

        None
    }
}

const BLURRED_OPACITY: Opacity = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputToggleMode {
    Terminal,
    AgentMode,
    AutoDetection,
}

impl From<&BlocklistAIInputModel> for InputToggleMode {
    fn from(input_model: &BlocklistAIInputModel) -> Self {
        if input_model.is_input_type_locked() {
            match input_model.input_type() {
                InputType::Shell => InputToggleMode::Terminal,
                InputType::AI => InputToggleMode::AgentMode,
            }
        } else {
            InputToggleMode::AutoDetection
        }
    }
}

/// Denormalized state that is required to render the toggle's styles that isn't easily directly
/// accessible because its owned by other views in other parts of the hierarchy and was never
/// extracted to a Model.
///
/// Kind of an 80/20 situation here.
struct CachedUIState {
    is_input_empty: bool,
    is_hovered: bool,
    is_in_active_terminal: bool,
}

impl CachedUIState {
    fn is_blurred(&self) -> bool {
        !self.is_hovered && !self.is_in_active_terminal
    }
}

/// The Terminal / Agent Mode switch.
///
/// Uncaged: this used to be `UniversalDeveloperInputButtonBar`, a full row of agent tooling
/// (voice, @-context, attach, slash commands, model selector, prompt alert) next to the switch.
/// The input now renders one footer in every mode -- `AgentInputFooter`, which already owns all
/// of that tooling -- and pins this switch to its left edge, so the rest of the bar was never on
/// screen. What is left is the state the switch needs: its options, selection, disabled tooltip
/// and blurred styling.
pub struct InputModeToggle {
    segmented_control: ViewHandle<SegmentedControl<InputToggleMode>>,
    cached_ui_state: Rc<RefCell<CachedUIState>>,
    terminal_model: std::sync::Arc<parking_lot::FairMutex<crate::terminal::TerminalModel>>,
}

pub enum InputModeToggleEvent {
    InputTypeSelected(InputType),
    EnableAutoDetection,
}

impl InputModeToggle {
    pub fn new(
        input_model: ModelHandle<BlocklistAIInputModel>,
        cli_subagent_controller: ModelHandle<CLISubagentController>,
        terminal_model: std::sync::Arc<parking_lot::FairMutex<crate::terminal::TerminalModel>>,
        ctx: &mut ViewContext<Self>,
    ) -> Self {
        // Create segmented control options based on auto-detection setting
        let ai_settings = AISettings::as_ref(ctx);
        let is_autodetection_enabled = ai_settings.is_ai_autodetection_enabled(ctx);

        let mut options = vec![InputToggleMode::Terminal, InputToggleMode::AgentMode];

        let mut default_option = input_model.as_ref(ctx).into();
        if is_autodetection_enabled {
            options.push(InputToggleMode::AutoDetection);
        } else if default_option == InputToggleMode::AutoDetection {
            // Don't set the default to auto-detection if it's not enabled.
            default_option = InputToggleMode::Terminal;
        }

        let cached_ui_state = Rc::new(RefCell::new(CachedUIState {
            is_input_empty: true,
            is_hovered: false,
            is_in_active_terminal: false,
        }));

        let ui_state_clone = cached_ui_state.clone();
        let input_model_clone = input_model.clone();
        let segmented_control_view = ctx.add_typed_action_view(|ctx| {
            SegmentedControl::new(
                options,
                move |option, is_selected, app| {
                    let ui_state = ui_state_clone.borrow();
                    build_renderable_option_config(
                        option,
                        is_selected,
                        &input_model_clone,
                        &ui_state,
                        app,
                    )
                },
                default_option,
                segmented_control_styles(ctx),
            )
        });
        // Subscribe to segmented control events
        ctx.subscribe_to_view(&segmented_control_view, |_, _, event, ctx| match event {
            SegmentedControlEvent::OptionSelected(input_mode) => match input_mode {
                InputToggleMode::Terminal => {
                    ctx.emit(InputModeToggleEvent::InputTypeSelected(InputType::Shell));
                }
                InputToggleMode::AgentMode => {
                    ctx.emit(InputModeToggleEvent::InputTypeSelected(InputType::AI));
                }
                InputToggleMode::AutoDetection => {
                    ctx.emit(InputModeToggleEvent::EnableAutoDetection);
                }
            },
        });

        ctx.subscribe_to_model(&Appearance::handle(ctx), |me, _, _, ctx| {
            me.segmented_control.update(ctx, |segmented_control, ctx| {
                segmented_control.set_styles(segmented_control_styles(ctx), ctx);
            });
            ctx.notify();
        });

        ctx.subscribe_to_model(&AISettings::handle(ctx), |me, ai_settings, event, ctx| {
            // Update segmented control options when auto-detection setting changes
            if let AISettingsChangedEvent::AIAutoDetectionEnabled { .. } = event {
                let is_autodection_enabled =
                    ai_settings.as_ref(ctx).is_ai_autodetection_enabled(ctx);
                me.segmented_control.update(ctx, |segmented_control, ctx| {
                    if is_autodection_enabled {
                        segmented_control.update_options(
                            vec![
                                InputToggleMode::Terminal,
                                InputToggleMode::AgentMode,
                                InputToggleMode::AutoDetection,
                            ],
                            ctx,
                        );
                    } else {
                        segmented_control.update_options(
                            vec![InputToggleMode::Terminal, InputToggleMode::AgentMode],
                            ctx,
                        );
                    }
                });
            }
            ctx.notify();
        });

        ctx.subscribe_to_model(&input_model, move |me, input_model, event, ctx| {
            if !event.did_update_input_config() {
                return;
            }
            let input_mode = InputToggleMode::from(input_model.as_ref(ctx));
            me.segmented_control.update(ctx, |control, ctx| {
                control.set_selected_option(input_mode, ctx);
            });
            me.notify_and_notify_children(ctx);
        });

        // Keep the control disabled state in sync with role changes
        ctx.subscribe_to_model(&SessionPermissionsManager::handle(ctx), |me, _, _, ctx| {
            me.update_segmented_control_disabled_state(ctx);
        });
        // Keep the control disabled state in sync with agent control state
        ctx.subscribe_to_model(&cli_subagent_controller, move |me, _, _, ctx| {
            me.update_segmented_control_disabled_state(ctx);
        });

        let mut me = Self {
            segmented_control: segmented_control_view,
            cached_ui_state,
            terminal_model,
        };

        // Initialize disabled state based on current role
        me.update_segmented_control_disabled_state(ctx);

        me
    }

    /// The Terminal / Agent Mode segments, for the footer to render.
    pub fn segmented_control(&self) -> &ViewHandle<SegmentedControl<InputToggleMode>> {
        &self.segmented_control
    }

    /// Update the input empty state and refresh the autodetection label
    pub fn update_input_empty_state(&mut self, is_empty: bool, ctx: &mut ViewContext<Self>) {
        if self.cached_ui_state.borrow().is_input_empty == is_empty {
            return;
        }
        self.cached_ui_state.borrow_mut().is_input_empty = is_empty;
        self.notify_and_notify_children(ctx);
    }

    fn notify_and_notify_children(&self, ctx: &mut ViewContext<Self>) {
        ctx.notify();
        self.segmented_control.update(ctx, |_, ctx| ctx.notify());
    }

    pub fn update_segmented_control_disabled_state(&mut self, ctx: &mut ViewContext<Self>) {
        let (is_reader, is_agent_in_control) = {
            let terminal_model = self.terminal_model.lock();
            (
                terminal_model.shared_session_status().is_reader(),
                terminal_model
                    .block_list()
                    .active_block()
                    .is_active_and_long_running(),
            )
        };

        let tooltip = if is_reader {
            Some("Request edit access to change input mode".to_string())
        } else if is_agent_in_control {
            Some("Input mode locked while agent is monitoring a command".to_string())
        } else {
            None
        };

        self.segmented_control
            .update(ctx, |segmented_control, ctx| {
                segmented_control.set_disabled_tooltip(tooltip.clone().map(Into::into), ctx);
            });
    }

    pub fn set_is_in_active_terminal(
        &mut self,
        is_in_active_terminal: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        if self.cached_ui_state.borrow().is_in_active_terminal == is_in_active_terminal {
            return;
        }

        self.cached_ui_state.borrow_mut().is_in_active_terminal = is_in_active_terminal;
        self.notify_and_notify_children(ctx);
    }

    /// Set the parent input hovered state.
    /// Used to fade the toggle when the input is not in focus.
    pub fn set_udi_hovered(&mut self, is_hovered: bool, ctx: &mut ViewContext<Self>) {
        if self.cached_ui_state.borrow().is_hovered == is_hovered {
            return;
        }
        self.cached_ui_state.borrow_mut().is_hovered = is_hovered;
        self.notify_and_notify_children(ctx);
    }
}

impl Entity for InputModeToggle {
    type Event = InputModeToggleEvent;
}

impl View for InputModeToggle {
    fn ui_name() -> &'static str {
        "InputModeToggle"
    }

    fn render(&self, _: &AppContext) -> Box<dyn warpui::Element> {
        ChildView::new(&self.segmented_control).finish()
    }
}

fn segmented_control_styles(app: &AppContext) -> UiComponentStyles {
    let appearance = Appearance::as_ref(app);
    let theme = appearance.theme();
    let button_size = 5.0 + appearance.monospace_font_size();
    // Use a smaller base font size that scales properly
    let base_font_size = 10.0; // Start with smaller font
    let scaled_ui_font_size = base_font_size * appearance.monospace_ui_scalar();

    let background = Some(internal_colors::fg_overlay_1(theme).into());

    UiComponentStyles {
        width: Some(button_size), // Match InputPrompt button height for square buttons
        height: Some(button_size), // Match InputPrompt button height
        border_radius: Some(CornerRadius::with_all(Radius::Pixels(4.0))),
        border_width: Some(1.0),
        border_color: Some(Fill::Solid(theme.outline().into_solid())),
        font_family_id: Some(appearance.ui_font_family()),
        font_size: Some(scaled_ui_font_size),
        background,
        ..Default::default()
    }
}

fn build_renderable_option_config(
    option: InputToggleMode,
    is_selected: bool,
    input_model: &ModelHandle<BlocklistAIInputModel>,
    ui_state: &CachedUIState,
    app: &AppContext,
) -> Option<RenderableOptionConfig> {
    let appearance = Appearance::as_ref(app);
    let theme = appearance.theme();

    let input_model = input_model.as_ref(app);
    let current_input_type = input_model.input_type();

    let terminal_keybindings = TerminalKeybindings::as_ref(app);

    let compute_colors = |input_type, accent_color| {
        let fg_color = if is_selected {
            theme.background().into_solid()
        } else if input_type == current_input_type && !ui_state.is_input_empty {
            accent_color
        } else {
            theme.sub_text_color(theme.surface_1()).into_solid()
        };

        let bg_color = if is_selected {
            accent_color
        } else {
            ColorU::transparent_black()
        };

        (fg_color, bg_color)
    };

    let mut config = match option {
        InputToggleMode::Terminal => {
            let accent_color = theme.terminal_colors().normal.blue.into();
            let (fg_color, bg_color) = compute_colors(InputType::Shell, accent_color);

            RenderableOptionConfig {
                icon_path: Icon::Terminal.into(),
                icon_color: fg_color,
                label: None,
                tooltip: Some(tooltip_config(
                    "Terminal",
                    Some(terminal_mode_tooltip_subtext(terminal_keybindings)),
                    app,
                )),
                background: bg_color.into(),
            }
        }
        InputToggleMode::AgentMode => {
            let accent_color = theme.terminal_colors().normal.yellow.into();
            let (fg_color, bg_color) = compute_colors(InputType::AI, accent_color);

            RenderableOptionConfig {
                icon_path: Icon::AgentMode.into(),
                icon_color: fg_color,
                label: None,
                tooltip: Some(tooltip_config(
                    "Agent Mode",
                    Some(agent_mode_tooltip_subtext(terminal_keybindings)),
                    app,
                )),
                background: bg_color.into(),
            }
        }
        InputToggleMode::AutoDetection => {
            // Should not actually render anything, when using the new two-option
            // UDI control.
            return None;
        }
    };

    if ui_state.is_blurred() {
        config.background = Fill::Solid(coloru_with_opacity(
            config.background.start_color(),
            BLURRED_OPACITY,
        ));
        config.icon_color = coloru_with_opacity(config.icon_color, BLURRED_OPACITY);

        if let Some(tooltip_config) = config.tooltip.as_mut() {
            tooltip_config.background_color =
                coloru_with_opacity(tooltip_config.background_color, BLURRED_OPACITY);
            tooltip_config.border_color =
                coloru_with_opacity(tooltip_config.background_color, BLURRED_OPACITY);
            // Shift text color to ensure sufficient contrast against the blurred background
            tooltip_config.text_color = foreground_color_with_minimum_contrast(
                tooltip_config.text_color,
                Rgb::from(tooltip_config.background_color),
                MinimumAllowedContrast::Text,
            );
        }
    }

    Some(config)
}

const AGENT_MODE_TOOLTIP_PREFIX: &str = "* + space";
// `TERMINAL_INPUT_PREFIX` is "!" with no trailing space, unlike `AI_INPUT_PREFIX` ("* "),
// so the tooltip must not promise a space that the matcher does not want.
const TERMINAL_MODE_TOOLTIP_PREFIX: &str = "!";

fn agent_mode_tooltip_subtext(terminal_keybindings: &TerminalKeybindings) -> String {
    let keybinding = terminal_keybindings.set_input_mode_agent_keybinding();
    let Some(keybinding) = keybinding else {
        return AGENT_MODE_TOOLTIP_PREFIX.into();
    };

    format!("{keybinding} or {AGENT_MODE_TOOLTIP_PREFIX}")
}

fn terminal_mode_tooltip_subtext(terminal_keybindings: &TerminalKeybindings) -> String {
    let keybinding = terminal_keybindings.set_input_mode_terminal_keybinding();
    let Some(keybinding) = keybinding else {
        return TERMINAL_MODE_TOOLTIP_PREFIX.into();
    };

    format!("{keybinding} or {TERMINAL_MODE_TOOLTIP_PREFIX}")
}

fn tooltip_config(
    text: impl Into<Cow<'static, str>>,
    subtext: Option<impl Into<Cow<'static, str>>>,
    app: &AppContext,
) -> TooltipConfig {
    let appearance = Appearance::as_ref(app);
    let theme = appearance.theme();

    TooltipConfig {
        text: text.into(),
        sub_text: subtext.map(Into::into),
        background_color: theme.tooltip_background(),
        // Match UiBuilder::default_tool_tip_styles text color (same as agent picker tooltip)
        text_color: theme.background().into_solid(),
        border_color: theme.outline().into_solid(),
    }
}
