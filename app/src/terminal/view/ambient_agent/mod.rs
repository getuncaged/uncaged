mod auth_secret_ftux_dropdown;
mod auth_secret_ftux_view;
mod block;
mod first_time_setup;
mod footer;
mod harness_selector;
mod loading_screen;
mod model;
mod progress;
mod progress_ui_state;
mod tips;
mod view_impl;

pub use auth_secret_ftux_view::{
    AuthSecretFtuxAction, AuthSecretFtuxView, AuthSecretFtuxViewEvent,
};
pub use block::*;
pub use first_time_setup::{FirstTimeCloudAgentSetupView, FirstTimeCloudAgentSetupViewEvent};
pub use footer::{render_error_footer, render_loading_footer};
pub use harness_selector::{HarnessSelector, HarnessSelectorAction, HarnessSelectorEvent};
pub use loading_screen::{render_cloud_mode_error_screen, render_cloud_mode_loading_screen};
pub(crate) use model::should_disable_snapshot;
#[cfg(all(feature = "local_fs", not(target_family = "wasm")))]
pub(crate) use model::PendingHandoff;
pub use model::{AgentProgress, AmbientAgentViewModel, AmbientAgentViewModelEvent, Status};
#[cfg(all(feature = "local_fs", not(target_family = "wasm")))]
pub(crate) use model::{HandoffSubmissionState, SnapshotUploadStatus};
pub use progress::{render_progress, ProgressProps, ProgressStep, ProgressStepState};
pub use progress_ui_state::AmbientAgentProgressUIState;
pub use tips::{get_cloud_mode_tips, CloudModeTip};
use warp_core::features::FeatureFlag;
use warpui::geometry::vector::Vector2F;
use warpui::{AppContext, ModelHandle, ViewHandle, WindowId};

use crate::ai::blocklist::agent_view::{AgentViewController, AgentViewState};
use crate::pane_group::TerminalViewResources;
use crate::terminal::{MockTerminalManager, TerminalManager, TerminalModel, TerminalView};

/// Creates a cloud mode terminal view and manager for ambient agent sessions.
pub fn create_cloud_mode_view(
    resources: TerminalViewResources,
    view_bounds_size: Vector2F,
    window_id: WindowId,
    ctx: &mut AppContext,
) -> (
    ViewHandle<TerminalView>,
    ModelHandle<Box<dyn TerminalManager>>,
) {
    let terminal_init =
        MockTerminalManager::create_cloud_mode_model(resources, view_bounds_size, window_id, ctx);
    (terminal_init.view, terminal_init.manager)
}

/// Returns `true` when a cloud agent session is in any pre-first-exchange phase —
/// either still spawning (loading screen) or running setup commands before the first
/// agent turn. In this state, we hide the interactive input and render a loading footer.
pub fn is_cloud_agent_pre_first_exchange(
    ambient_agent_view_model: Option<&ModelHandle<AmbientAgentViewModel>>,
    agent_view_controller: &ModelHandle<AgentViewController>,
    terminal_model: &TerminalModel,
    app: &AppContext,
) -> bool {
    if !(FeatureFlag::CloudMode.is_enabled() && FeatureFlag::AgentView.is_enabled()) {
        return false;
    }

    let Some(ambient_agent_view_model) = ambient_agent_view_model else {
        return false;
    };

    let view_model = ambient_agent_view_model.as_ref(app);

    let is_in_pre_first_exchange_status = matches!(
        view_model.status(),
        Status::WaitingForSession { .. } | Status::AgentRunning
    );
    if !is_in_pre_first_exchange_status {
        return false;
    }

    let agent_view_state = agent_view_controller.as_ref(app).agent_view_state().clone();
    let AgentViewState::Active { origin, .. } = agent_view_state else {
        return false;
    };

    // Handoff panes enter agent view with `RestoreExistingConversation` because they restore the
    // forked conversation, not `CloudAgent`. The `is_local_to_cloud_handoff` flag is the
    // authoritative "this is a cloud agent pane" signal for that path, so accept either.
    if !origin.is_cloud_agent() && !view_model.is_local_to_cloud_handoff() {
        return false;
    }

    // For non-oz harness runs, there is no Oz `AppendedExchange` to key off of, so we also
    // exit the pre-first-exchange phase when the harness CLI (e.g. `claude`, `gemini`) has
    // been detected. See `mark_harness_command_started`.
    if view_model.harness_command_started() {
        return false;
    }

    // Loading phase (`WaitingForSession`): no setup commands have started yet, but we're
    // still pre-first-exchange. Skip the block-list flag check.
    if matches!(view_model.status(), Status::WaitingForSession { .. }) {
        return true;
    }

    terminal_model
        .block_list()
        .is_executing_oz_environment_startup_commands()
}
