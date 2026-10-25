use warp_cli::agent::Harness;
use warp_core::settings::Setting;
use warp_core::ui::theme::color::internal_colors;
use warpui::elements::{
    Align, AnchorPair, Border, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment,
    DispatchEventResult, DropTarget, Element, Empty, EventHandler, Expanded, Flex, Hoverable,
    MainAxisSize, OffsetPositioning, OffsetType, ParentElement, PositionedElementOffsetBounds,
    PositioningAxis, Radius, SavePosition, Stack, XAxisAnchor, YAxisAnchor,
};
use warpui::presenter::ChildView;
use warpui::{AppContext, SingletonEntity as _};

use super::common::{
    add_command_xray_overlay, add_input_suggestions_overlays, add_voltron_overlay,
    add_workflow_info_overlay, maybe_add_buy_credits_banner,
    wrap_input_with_terminal_padding_and_focus_handler,
};
use super::{Input, InputAction, InputDropTargetData};
use crate::ai::blocklist::agent_view::shortcuts::{
    render_agent_shortcuts_view, AgentShortcutsViewContext,
};
use crate::ai::blocklist::agent_view::{agent_view_bg_fill, AgentViewState};
use crate::ai::blocklist::InputType;
use crate::ai::harness_availability::HarnessAvailabilityModel;
use crate::appearance::Appearance;
use crate::context_chips::spacing::{self};
use crate::editor::position_id_for_cursor;
use crate::features::FeatureFlag;
use crate::settings::InputModeSettings;
use crate::terminal::settings::TerminalSettings;
use crate::terminal::view::TerminalAction;
use crate::BlocklistAIHistoryModel;

impl Input {
    pub fn is_cloud_mode_input_v2_composing(&self, app: &AppContext) -> bool {
        FeatureFlag::CloudModeInputV2.is_enabled()
            && FeatureFlag::CloudMode.is_enabled()
            && self.ambient_agent_view_model().is_some_and(|model| {
                let view_model = model.as_ref(app);
                view_model.is_configuring_ambient_agent()
                    // The handoff pane intentionally stays on the existing input UI even
                    // when V2 is on — V2 is for fresh cloud-mode runs only, and handoff has
                    // its own pre-spawn flow (submit interception).
                    && !view_model.is_local_to_cloud_handoff()
            })
    }

    /// Renders the input when there is an active `AgentView`.
    ///
    /// Only used when `FeatureFlag::AgentView` is enabled.
    pub(super) fn render_agent_input(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let menu_positioning = self.menu_positioning(app);

        let model = self.model.lock();

        // We should likely rework this stack to not need to use `with_constrain_absolute_children`,
        // by reworking the positioning of the children to not depend on this.
        let mut stack = Stack::new().with_constrain_absolute_children();

        let input_mode = *InputModeSettings::as_ref(app).input_mode.value();

        let mut column = Flex::column();

        if let Some(banner) =
            self.render_input_banner(appearance, app, input_mode, /*is_compact_mode=*/ false)
        {
            column.add_child(
                Container::new(banner)
                    .with_margin_top(spacing::UDI_CHIP_MARGIN)
                    .finish(),
            );
        }

        let ai_input_model = self.ai_input_model.as_ref(app);

        if FeatureFlag::ImageAsContext.is_enabled()
            && matches!(ai_input_model.input_type(), InputType::AI)
        {
            if let Some(images) = self.render_attachment_chips(appearance) {
                column.add_child(
                    Container::new(images)
                        .with_margin_top(spacing::UDI_CHIP_MARGIN)
                        .finish(),
                );
            }
        }

        let show_harness_row = FeatureFlag::CloudMode.is_enabled()
            && HarnessAvailabilityModel::as_ref(app).should_show_harness_selector()
            && self
                .ambient_agent_view_model()
                .is_some_and(|ambient_agent_model| {
                    ambient_agent_model
                        .as_ref(app)
                        .is_configuring_ambient_agent()
                });
        if show_harness_row {
            if let Some(harness_selector) = self.harness_selector() {
                // Temporarily render the harness selector in the cloud mode UDI until we fully
                // implement the new designs.
                let harness_row = Flex::row()
                    .with_main_axis_size(MainAxisSize::Min)
                    .with_child(ChildView::new(harness_selector).finish())
                    .finish();
                column.add_child(
                    Container::new(harness_row)
                        .with_padding_top(spacing::UDI_CHIP_MARGIN)
                        .with_padding_bottom(4.)
                        .finish(),
                );
            }
        }

        let terminal_spacing = TerminalSettings::as_ref(app)
            .terminal_input_spacing(appearance.line_height_ratio(), app);
        column.add_child(
            Container::new(self.render_input_box(/*show_vim_status=*/ false, appearance, app))
                .with_margin_top(
                    terminal_spacing.prompt_to_editor_padding
                        * spacing::UDI_PROMPT_BOTTOM_PADDING_FACTOR,
                )
                .finish(),
        );
        column.add_child(
            SavePosition::new(
                ChildView::new(&self.agent_input_footer).finish(),
                &self.prompt_save_position_id(),
            )
            .finish(),
        );

        stack.add_child(wrap_input_with_terminal_padding_and_focus_handler(
            self.is_active_session(app),
            column.finish(),
            false,
        ));

        if let Some(selected_workflow_state) = self.workflows_state.selected_workflow_state.as_ref()
        {
            if selected_workflow_state.should_show_more_info_view {
                add_workflow_info_overlay(
                    &mut stack,
                    selected_workflow_state,
                    self.size_info(app).pane_height_px().as_f32(),
                    menu_positioning,
                );
            }
        }

        if self.is_voltron_open && self.is_pane_focused(app) {
            add_voltron_overlay(&mut stack, &self.voltron_view, menu_positioning);
        }

        if self.is_pane_focused(app) {
            add_input_suggestions_overlays(self, &mut stack, appearance, menu_positioning, app);
        }

        if let Some(token_description) = &self.command_x_ray_description {
            add_command_xray_overlay(
                self,
                &mut stack,
                token_description,
                appearance,
                menu_positioning,
                app,
            );
        }

        let drop_target = DropTarget::new(
            SavePosition::new(stack.finish(), &self.status_free_input_save_position_id()).finish(),
            InputDropTargetData::new(self.weak_view_handle.clone()),
        )
        .finish();

        let border_color = if self.handoff_compose_state.as_ref(app).is_active() {
            appearance.theme().ansi_fg_magenta()
        } else if !self.ai_input_model.as_ref(app).is_ai_input_enabled()
            && !self.suggestions_mode_model.as_ref(app).is_slash_commands()
            && !self.slash_command_model.as_ref(app).state().is_detected_command()
            // If NLD, don't color the border if the input is empty, because the current
            // classification is necessarily stale (intentionally inherited from the last
            // classification prior to clearing the input)
            && (!self.editor.as_ref(app).is_empty(app)
                || self.ai_input_model.as_ref(app).is_input_type_locked())
        {
            appearance.theme().ansi_fg_blue()
        } else {
            styles::default_border_color(appearance.theme())
        };

        let mut input = Container::new(
            Hoverable::new(self.hoverable_handle.clone(), |_| drop_target)
                .on_hover(|is_hovered, ctx, _app, _position| {
                    ctx.dispatch_typed_action(InputAction::SetUDIHovered(is_hovered));
                })
                .on_middle_click(|ctx, _app, _position| {
                    ctx.dispatch_typed_action(TerminalAction::MiddleClickOnInput)
                })
                .finish(),
        )
        .with_border(Border::top(1.).with_border_color(border_color))
        .with_padding_bottom(4.);

        if self.agent_view_controller.as_ref(app).is_inline() {
            input = input.with_background(agent_view_bg_fill(app));
        }

        let input = input.finish();

        let mut column = Flex::column();

        // Uncaged: shared with every other input surface -- see `render_active_inline_menu`.
        if let Some(menu) = self.render_active_inline_menu(app) {
            column.add_child(menu);
        }

        if self
            .agent_shortcut_view_model
            .as_ref(app)
            .is_shortcut_view_open()
        {
            let agent_view_controller = self.agent_view_controller.as_ref(app);
            let (is_cloud_agent, has_submitted_first_prompt) =
                match agent_view_controller.agent_view_state() {
                    AgentViewState::Active {
                        conversation_id,
                        origin,
                        ..
                    } => {
                        let is_cloud_agent = origin.is_cloud_agent();
                        let has_submitted_first_prompt = if is_cloud_agent {
                            BlocklistAIHistoryModel::as_ref(app)
                                .conversation(conversation_id)
                                .is_some_and(|c| c.initial_user_query().is_some())
                        } else {
                            true
                        };
                        (is_cloud_agent, has_submitted_first_prompt)
                    }
                    // When inactive, show all shortcuts (treat as not-cloud and not in the zero-state).
                    AgentViewState::Inactive => (false, true),
                };

            column.add_child(render_agent_shortcuts_view(
                AgentShortcutsViewContext {
                    is_cloud_agent,
                    has_submitted_first_prompt,
                },
                app,
            ));
        }
        column.add_child(ChildView::new(&self.agent_status_view).finish());
        if let Some(panel) = self.queued_prompts_panel.as_ref() {
            if panel.as_ref(app).should_render(app) {
                column.add_child(ChildView::new(panel).finish());
            }
        }
        column.add_child(input);

        let mut outer_stack = Stack::new().with_constrain_absolute_children();
        outer_stack.add_child(column.finish());
        maybe_add_buy_credits_banner(
            &mut outer_stack,
            &self.buy_credits_banner,
            self.is_pane_focused(app),
            self.terminal_view_id,
            self.is_input_at_top(&model, app),
            app,
        );

        SavePosition::new(outer_stack.finish(), &self.save_position_id()).finish()
    }

    pub(super) fn should_show_auth_secret_ftux(&self, app: &AppContext) -> bool {
        let Some(view_model) = self.ambient_agent_view_model() else {
            return false;
        };
        let vm = view_model.as_ref(app);
        let harness = vm.selected_harness();
        if harness == Harness::Oz {
            return false;
        }
        // Skip FTUX for harnesses that have no auth secret types defined.
        if crate::ai::auth_secret_types::auth_secret_types_for_harness(harness).is_empty() {
            return false;
        }
        if let Some(ftux_view) = self.auth_secret_ftux_view() {
            if ftux_view.as_ref(app).has_creation_state() {
                return true;
            }
        }
        if crate::ai::cloud_agent_settings::CloudAgentSettings::as_ref(app)
            .is_harness_auth_ftux_completed(harness)
        {
            return false;
        }
        vm.selected_harness_auth_secret_name().is_none()
    }

    fn render_auth_secret_ftux_content(&self) -> Box<dyn Element> {
        match self.auth_secret_ftux_view() {
            Some(view) => ChildView::new(view).finish(),
            None => Empty::new().finish(),
        }
    }
}

pub mod styles {
    use pathfinder_color::ColorU;
    use warp_core::ui::theme::WarpTheme;

    use crate::ui_components::blended_colors;

    pub fn default_border_color(theme: &WarpTheme) -> ColorU {
        blended_colors::neutral_2(theme)
    }
}
