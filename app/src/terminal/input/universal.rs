use settings::Setting;
use warpui::elements::{
    Border, ChildView, Container, CornerRadius, DropTarget, Element, Flex, Hoverable,
    ParentElement, Radius, SavePosition, Stack,
};
use warpui::{AppContext, SingletonEntity};

use super::common::{
    add_command_xray_overlay, add_input_suggestions_overlays, add_vim_status_to_stack,
    add_voltron_overlay, add_workflow_info_overlay, maybe_add_buy_credits_banner,
    wrap_input_with_terminal_padding_and_focus_handler,
};
use super::Input;
use crate::ai::blocklist::agent_view::agent_view_bg_fill;
use crate::ai::blocklist::InputType;
use crate::appearance::Appearance;
use crate::context_chips::spacing;
use crate::features::FeatureFlag;
use crate::settings::{AppEditorSettings, InputModeSettings};
use crate::terminal::block_list_viewport::InputMode;
use crate::terminal::input::{InputAction, InputDropTargetData};
use crate::terminal::settings::TerminalSettings;
use crate::terminal::view::TerminalAction;
use crate::themes::theme::color::internal_colors;

impl Input {
    /// Renders the universal input. This is used when `FeatureFlag::AgentView` is disabled and the
    /// user has 'universal' input type selected in settings.
    pub(super) fn render_universal_developer_input(&self, app: &AppContext) -> Box<dyn Element> {
        self.render_composer(/*is_agent_view=*/ false, app)
    }

    /// Uncaged: the one input surface for Terminal mode, Agent mode and an active agent view.
    ///
    /// The agent view used to have its own renderer: no prompt row, the banner always on
    /// top, a full-width top border instead of the inset box, and a different footer. Every
    /// time a prompt entered or left the agent view the whole input changed shape. Now the
    /// frame, the prompt row and the footer are the same in every mode; what changes is the
    /// colour of the box and which items the footer shows. The agent view only adds the
    /// panels that belong to a running conversation (harness row, shortcuts, queued prompts).
    pub(super) fn render_composer(
        &self,
        is_agent_view: bool,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let menu_positioning = self.menu_positioning(app);

        let model = self.model.lock();

        // We should likely rework this stack to not need to use `with_constrain_absolute_children`,
        // by reworking the positioning of the children to not depend on this.
        let mut stack = Stack::new().with_constrain_absolute_children();

        let mut prompt_row = Stack::new();

        let prompt_elements = self
            .prompt_render_helper
            .render_universal_developer_input_prompt(&model, appearance, app);
        prompt_row.add_child(prompt_elements);

        let vim_state = self.editor.as_ref(app).vim_state(app);
        let app_editor_settings = AppEditorSettings::as_ref(app);
        let show_vim_status = vim_state.is_some() && *app_editor_settings.vim_status_bar.value();
        let input_mode = *InputModeSettings::as_ref(app).input_mode.value();

        // For Universal Developer Input, ignore compact mode setting
        let is_compact_mode = false;
        let mut column = Flex::column();

        if matches!(input_mode, InputMode::PinnedToBottom | InputMode::Waterfall) {
            if let Some(banner) =
                self.render_input_banner(appearance, app, input_mode, is_compact_mode)
            {
                column.add_child(
                    Container::new(banner)
                        .with_margin_top(spacing::UDI_CHIP_MARGIN)
                        .finish(),
                );
            }
        }

        column.add_child(prompt_row.finish());

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

        if is_agent_view {
            if let Some(harness_row) = self.render_harness_row(app) {
                column.add_child(harness_row);
            }
        }

        let terminal_spacing = TerminalSettings::as_ref(app)
            .terminal_input_spacing(appearance.line_height_ratio(), app);
        column.add_child(
            Container::new(self.render_input_box(show_vim_status, appearance, app))
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

        if matches!(input_mode, InputMode::PinnedToTop) {
            if let Some(banner) =
                self.render_input_banner(appearance, app, input_mode, is_compact_mode)
            {
                column.add_child(
                    Container::new(banner)
                        .with_margin_bottom(spacing::UDI_CHIP_MARGIN)
                        .finish(),
                );
            }
        }

        if let Some(vim_state) = vim_state.as_ref() {
            if show_vim_status {
                add_vim_status_to_stack(
                    &mut stack, vim_state, appearance, true, // use adjusted padding for UDI
                );
            }
        }

        stack.add_child(wrap_input_with_terminal_padding_and_focus_handler(
            self.is_active_session(app),
            column.finish(),
            true, // use adjusted padding for UDI
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

        maybe_add_buy_credits_banner(
            &mut stack,
            &self.buy_credits_banner,
            self.is_pane_focused(app),
            self.terminal_view_id,
            self.is_input_at_top(&model, app),
            app,
        );

        // If the file tree is enabled, don't include the top margin for UDI so that the UDI is flush with the
        // file tree.
        let margin_top = if FeatureFlag::FileTree.is_enabled() && self.is_input_at_top(&model, app)
        {
            0.
        } else {
            6.
        };

        // Wrap the stack in a container with background and border styling based on focus
        let mut container = Container::new(
            SavePosition::new(stack.finish(), &self.status_free_input_save_position_id()).finish(),
        )
        .with_margin_bottom(6.)
        .with_margin_left(6.)
        .with_margin_right(6.)
        .with_margin_top(margin_top)
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)));

        // Uncaged: the composer box belongs to Agent Mode, not to the terminal.
        //
        // A bordered, inset, rounded panel reads as a chat composer -- which is right when
        // you are writing a prompt, and wrong when you are typing a shell command. A
        // terminal prompt should sit flush against the output above it, the way every other
        // terminal does. The box therefore follows the mode, like everything else on this
        // surface.
        //
        // In an agent view the box stays even when the user switches to Terminal mode for a
        // command -- it is the conversation's composer -- and only its border colour says
        // which mode the input is in.
        let is_ai_mode = matches!(self.ai_input_model.as_ref(app).input_type(), InputType::AI);
        if is_ai_mode || is_agent_view {
            let border = Border::all(1.).with_border_color(self.composer_border_color(app));
            container = container.with_border(border);
            if is_agent_view && self.agent_view_controller.as_ref(app).is_inline() {
                container = container.with_background(agent_view_bg_fill(app));
            } else if self.is_pane_focused(app) {
                container = container.with_background(internal_colors::fg_overlay_1(theme));
            }
        }

        let drop_target = DropTarget::new(
            container.finish(),
            InputDropTargetData::new(self.weak_view_handle.clone()),
        )
        .finish();

        let input = Hoverable::new(self.hoverable_handle.clone(), |_| drop_target)
            .on_hover(|is_hovered, ctx, _app, _position| {
                ctx.dispatch_typed_action(InputAction::SetUDIHovered(is_hovered));
            })
            .on_middle_click(|ctx, _app, _position| {
                ctx.dispatch_typed_action(TerminalAction::MiddleClickOnInput)
            })
            .finish();

        let mut column = Flex::column();

        // Uncaged: the inline menus (slash commands, prompts, rewind, plan, history, ...)
        // render on this surface too. The selector chain lived only in `render_agent_input`,
        // so on the terminal surface "/" set the suggestion mode and then nothing appeared.
        // The menu sits between the input and the block list, whichever side that is.
        let inline_menu = if self
            .inline_terminal_menu_positioner
            .as_ref(app)
            .should_hide_inline_menu_for_pane_size(app)
        {
            None
        } else {
            self.render_active_inline_menu(app)
        };

        // The agent view keeps its conversation at the bottom, whatever the input mode.
        if input_mode.is_pinned_to_top() && !is_agent_view {
            column.add_child(input);
            if let Some(menu) = inline_menu {
                column.add_child(menu);
            }
            column.add_child(ChildView::new(&self.agent_status_view).finish());
        } else {
            if is_agent_view {
                if let Some(shortcuts) = self.render_agent_shortcuts(app) {
                    column.add_child(shortcuts);
                }
            }
            column.add_child(ChildView::new(&self.agent_status_view).finish());
            if is_agent_view {
                if let Some(panel) = self.queued_prompts_panel.as_ref() {
                    if panel.as_ref(app).should_render(app) {
                        column.add_child(ChildView::new(panel).finish());
                    }
                }
            }
            if let Some(menu) = inline_menu {
                column.add_child(menu);
            }
            column.add_child(input);
        }

        SavePosition::new(column.finish(), &self.save_position_id()).finish()
    }
}
