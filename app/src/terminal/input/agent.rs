use pathfinder_color::ColorU;
use warpui::elements::{Container, Element, Flex, MainAxisSize, ParentElement};
use warpui::presenter::ChildView;
use warpui::{AppContext, SingletonEntity as _};

use super::Input;
use crate::ai::blocklist::agent_view::shortcuts::{
    render_agent_shortcuts_view, AgentShortcutsViewContext,
};
use crate::ai::blocklist::agent_view::AgentViewState;
use crate::ai::harness_availability::HarnessAvailabilityModel;
use crate::appearance::Appearance;
use crate::context_chips::spacing;
use crate::features::FeatureFlag;
use crate::BlocklistAIHistoryModel;

impl Input {
    /// Renders the input when there is an active `AgentView`.
    ///
    /// Only used when `FeatureFlag::AgentView` is enabled. Uncaged: this is the same surface
    /// as the terminal input -- see `render_composer` -- so entering or leaving the agent view
    /// does not change the input's shape.
    pub(super) fn render_agent_input(&self, app: &AppContext) -> Box<dyn Element> {
        self.render_composer(/*is_agent_view=*/ true, app)
    }

    /// The composer box's border: magenta while composing a cloud handoff, blue when the input
    /// will run as a shell command, neutral otherwise.
    pub(super) fn composer_border_color(&self, app: &AppContext) -> ColorU {
        let appearance = Appearance::as_ref(app);
        if self.handoff_compose_state.as_ref(app).is_active() {
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
        }
    }

    /// The cloud-mode harness selector, shown above the editor while configuring a cloud agent.
    pub(super) fn render_harness_row(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        let show_harness_row = FeatureFlag::CloudMode.is_enabled()
            && HarnessAvailabilityModel::as_ref(app).should_show_harness_selector()
            && self
                .ambient_agent_view_model()
                .is_some_and(|ambient_agent_model| {
                    ambient_agent_model
                        .as_ref(app)
                        .is_configuring_ambient_agent()
                });
        if !show_harness_row {
            return None;
        }
        let harness_selector = self.harness_selector()?;
        // Temporarily render the harness selector in the cloud mode UDI until we fully
        // implement the new designs.
        let harness_row = Flex::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_child(ChildView::new(harness_selector).finish())
            .finish();
        Some(
            Container::new(harness_row)
                .with_padding_top(spacing::UDI_CHIP_MARGIN)
                .with_padding_bottom(4.)
                .finish(),
        )
    }

    pub(super) fn render_agent_shortcuts(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        if !self
            .agent_shortcut_view_model
            .as_ref(app)
            .is_shortcut_view_open()
        {
            return None;
        }
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

        Some(render_agent_shortcuts_view(
            AgentShortcutsViewContext {
                is_cloud_agent,
                has_submitted_first_prompt,
            },
            app,
        ))
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
