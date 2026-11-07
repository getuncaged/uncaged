use pathfinder_color::ColorU;
use warpui::elements::Element;
use warpui::{AppContext, SingletonEntity as _};

use super::Input;
use crate::ai::blocklist::agent_view::shortcuts::{
    render_agent_shortcuts_view, AgentShortcutsViewContext,
};
use crate::ai::blocklist::agent_view::AgentViewState;
use crate::appearance::Appearance;
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

    /// The composer box's border.
    ///
    /// Uncaged: the border no longer follows the input mode. A mode-coloured ring around the
    /// whole input made switching modes look like switching surfaces; the mode toggle already
    /// says which mode the input is in.
    pub(super) fn composer_border_color(&self, app: &AppContext) -> ColorU {
        styles::default_border_color(Appearance::as_ref(app).theme())
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
