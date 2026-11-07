use std::future::Future;

use settings::Setting as _;
use uncaged_ui::{Entity, ModelContext, SingletonEntity, WindowId};
use uncaged_util::sync::Condition;

use crate::channel::{Channel, ChannelState};
use crate::settings::AISettings;

/// A generic model for managing one-time modals that should be shown to users only once.
///
/// Initially implemented for the ADE launch modal, but designed to be extensible to support
/// other types of one-time modals in the future. The model holds the canonical state of whether
/// a modal is currently being shown and automatically triggers the modal when appropriate
/// conditions are met (e.g., user becomes onboarded).
pub struct OneTimeModalModel {
    /// Whether the Oz launch modal is currently being shown.
    is_oz_launch_modal_open: bool,
    /// Whether the OpenWarp launch modal is currently being shown.
    is_openwarp_launch_modal_open: bool,
    is_orchestration_launch_modal_open: bool,
    /// Whether the auto-handoff sleep discoverability modal is currently being shown.
    is_auto_handoff_sleep_modal_open: bool,
    /// Set while the auto-handoff sleep modal is closed and reset while it is
    /// open, so async work (e.g. auto-resume-after-error) can wait for the
    /// modal to close. Mirrors the `Condition` pattern used by
    /// `NetworkStatus::pending_reconnect`.
    auto_handoff_sleep_modal_closed: Condition,
    /// Whether the free-AI-removal notice modal is currently being shown.
    is_free_ai_removal_modal_open: bool,
    /// Whether the HOA onboarding flow is currently being shown.
    is_hoa_onboarding_open: bool,
    /// The window ID where the currently open one-time modal should be displayed.
    /// This is captured when a modal is first opened and ensures the modal stays on that window.
    target_window_id: Option<WindowId>,
}

impl Default for OneTimeModalModel {
    fn default() -> Self {
        Self::new()
    }
}

impl OneTimeModalModel {
    pub fn new() -> Self {
        // The auto-handoff sleep modal starts closed, so its close condition
        // starts satisfied.
        let auto_handoff_sleep_modal_closed = Condition::new();
        auto_handoff_sleep_modal_closed.set();

        Self {
            is_oz_launch_modal_open: false,
            is_openwarp_launch_modal_open: false,
            is_orchestration_launch_modal_open: false,
            is_auto_handoff_sleep_modal_open: false,
            auto_handoff_sleep_modal_closed,
            is_free_ai_removal_modal_open: false,
            is_hoa_onboarding_open: false,
            target_window_id: None,
        }
    }

    /// Returns whether the Oz launch modal is currently open.
    pub fn is_oz_launch_modal_open(&self) -> bool {
        self.is_oz_launch_modal_open && self.target_window_id.is_some()
    }

    /// Returns the window ID where the currently open one-time modal should be displayed.
    pub fn target_window_id(&self) -> Option<WindowId> {
        self.target_window_id
    }

    pub fn mark_oz_launch_modal_dismissed(&mut self, ctx: &mut ModelContext<Self>) {
        self.set_oz_launch_modal_open(false, ctx);
    }

    /// Returns whether the OpenWarp launch modal is currently open.
    pub fn is_openwarp_launch_modal_open(&self) -> bool {
        self.is_openwarp_launch_modal_open && self.target_window_id.is_some()
    }

    pub fn mark_openwarp_launch_modal_dismissed(&mut self, ctx: &mut ModelContext<Self>) {
        self.set_openwarp_launch_modal_open(false, ctx);
    }

    pub fn is_orchestration_launch_modal_open(&self) -> bool {
        self.is_orchestration_launch_modal_open && self.target_window_id.is_some()
    }

    pub fn mark_orchestration_launch_modal_dismissed(&mut self, ctx: &mut ModelContext<Self>) {
        self.set_orchestration_launch_modal_open(false, ctx);
    }

    /// Returns whether the auto-handoff sleep discoverability modal is currently open.
    pub fn is_auto_handoff_sleep_modal_open(&self) -> bool {
        self.is_auto_handoff_sleep_modal_open && self.target_window_id.is_some()
    }

    pub fn mark_auto_handoff_sleep_modal_dismissed(&mut self, ctx: &mut ModelContext<Self>) {
        self.set_auto_handoff_sleep_modal_open(false, ctx);
    }

    /// Triggers the auto-handoff sleep discoverability modal. Unlike the launch
    /// modals, this is not called on startup: the auto-handoff controller calls
    /// it on wake when a sleep interrupted an in-progress local agent run that
    /// would have been handed off had `auto_handoff_on_sleep_enabled` been on.
    /// Shows at most once per user (tracked by a synced private setting).
    /// Returns true when the modal was opened.
    pub fn check_and_trigger_auto_handoff_sleep_modal(
        &mut self,
        ctx: &mut ModelContext<Self>,
    ) -> bool {
        let ai_settings = AISettings::as_ref(ctx);
        if *ai_settings.did_show_auto_handoff_sleep_modal {
            return false;
        }

        AISettings::handle(ctx).update(ctx, |settings, ctx| {
            if let Err(e) = settings
                .did_show_auto_handoff_sleep_modal
                .set_value(true, ctx)
            {
                log::warn!("Failed to mark auto-handoff sleep modal as shown: {e}");
            }
        });

        let should_show = !matches!(ChannelState::channel(), Channel::Integration);
        self.set_auto_handoff_sleep_modal_open(should_show, ctx);
        should_show
    }

    /// Sets whether the auto-handoff sleep modal is open. `pub(crate)` so the
    /// debug palette action can force the modal open.
    pub(crate) fn set_auto_handoff_sleep_modal_open(
        &mut self,
        is_open: bool,
        ctx: &mut ModelContext<Self>,
    ) -> bool {
        if self.is_auto_handoff_sleep_modal_open != is_open {
            self.is_auto_handoff_sleep_modal_open = is_open;
            if is_open {
                self.auto_handoff_sleep_modal_closed.reset();
            } else {
                self.auto_handoff_sleep_modal_closed.set();
            }
            ctx.emit(OneTimeModalEvent::VisibilityChanged { is_open });
            return true;
        }
        false
    }

    /// Returns a future that resolves immediately if the auto-handoff sleep
    /// modal is closed, or when it next closes if currently open. The future
    /// reads live modal state at poll time, so it can be created ahead of the
    /// modal opening.
    pub fn wait_until_auto_handoff_sleep_modal_closed(&self) -> impl Future<Output = ()> {
        self.auto_handoff_sleep_modal_closed.wait()
    }

    /// Returns whether the HOA onboarding flow is currently open.
    pub fn is_hoa_onboarding_open(&self) -> bool {
        self.is_hoa_onboarding_open && self.target_window_id.is_some()
    }

    pub fn mark_hoa_onboarding_dismissed(&mut self, ctx: &mut ModelContext<Self>) {
        self.set_hoa_onboarding_open(false, ctx);
    }

    /// Returns true if any one-time modal is currently open.
    pub fn is_any_modal_open(&self) -> bool {
        (self.is_oz_launch_modal_open
            || self.is_openwarp_launch_modal_open
            || self.is_orchestration_launch_modal_open
            || self.is_auto_handoff_sleep_modal_open
            || self.is_free_ai_removal_modal_open
            || self.is_hoa_onboarding_open)
            && self.target_window_id.is_some()
    }

    #[cfg(debug_assertions)]
    pub fn force_open_oz_launch_modal(&mut self, ctx: &mut ModelContext<Self>) {
        self.set_oz_launch_modal_open(true, ctx);
    }

    #[cfg(debug_assertions)]
    pub fn force_open_openwarp_launch_modal(&mut self, ctx: &mut ModelContext<Self>) {
        self.set_openwarp_launch_modal_open(true, ctx);
    }

    #[cfg(debug_assertions)]
    pub fn force_open_orchestration_launch_modal(&mut self, ctx: &mut ModelContext<Self>) {
        self.set_orchestration_launch_modal_open(true, ctx);
    }

    pub fn update_target_window_id(&mut self, window_id: WindowId, ctx: &mut ModelContext<Self>) {
        let was_any_modal_visible = self.is_any_modal_open();
        self.target_window_id = Some(window_id);
        if was_any_modal_visible != self.is_any_modal_open() {
            ctx.emit(OneTimeModalEvent::VisibilityChanged {
                is_open: self.is_any_modal_open(),
            });
        }
    }

    fn set_oz_launch_modal_open(&mut self, is_open: bool, ctx: &mut ModelContext<Self>) -> bool {
        if self.is_oz_launch_modal_open != is_open {
            self.is_oz_launch_modal_open = is_open;
            ctx.emit(OneTimeModalEvent::VisibilityChanged { is_open });
            return true;
        }
        false
    }

    fn set_openwarp_launch_modal_open(
        &mut self,
        is_open: bool,
        ctx: &mut ModelContext<Self>,
    ) -> bool {
        if self.is_openwarp_launch_modal_open != is_open {
            self.is_openwarp_launch_modal_open = is_open;
            ctx.emit(OneTimeModalEvent::VisibilityChanged { is_open });
            return true;
        }
        false
    }

    fn set_orchestration_launch_modal_open(
        &mut self,
        is_open: bool,
        ctx: &mut ModelContext<Self>,
    ) -> bool {
        if self.is_orchestration_launch_modal_open != is_open {
            self.is_orchestration_launch_modal_open = is_open;
            ctx.emit(OneTimeModalEvent::VisibilityChanged { is_open });
            return true;
        }
        false
    }

    /// Returns whether the free-AI-removal notice modal is currently open.
    pub fn is_free_ai_removal_modal_open(&self) -> bool {
        self.is_free_ai_removal_modal_open && self.target_window_id.is_some()
    }

    pub fn mark_free_ai_removal_modal_dismissed(&mut self, ctx: &mut ModelContext<Self>) {
        self.set_free_ai_removal_modal_open(false, ctx);
    }

    #[cfg(debug_assertions)]
    pub fn force_open_free_ai_removal_modal(&mut self, ctx: &mut ModelContext<Self>) {
        self.set_free_ai_removal_modal_open(true, ctx);
    }

    fn set_free_ai_removal_modal_open(
        &mut self,
        is_open: bool,
        ctx: &mut ModelContext<Self>,
    ) -> bool {
        if self.is_free_ai_removal_modal_open != is_open {
            self.is_free_ai_removal_modal_open = is_open;
            ctx.emit(OneTimeModalEvent::VisibilityChanged { is_open });
            return true;
        }
        false
    }

    fn set_hoa_onboarding_open(&mut self, is_open: bool, ctx: &mut ModelContext<Self>) -> bool {
        if self.is_hoa_onboarding_open != is_open {
            self.is_hoa_onboarding_open = is_open;
            ctx.emit(OneTimeModalEvent::VisibilityChanged { is_open });
            return true;
        }
        false
    }
}

/// The outcome of evaluating the free-AI-removal notice conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OneTimeModalEvent {
    VisibilityChanged { is_open: bool },
}

impl Entity for OneTimeModalModel {
    type Event = OneTimeModalEvent;
}

impl SingletonEntity for OneTimeModalModel {}

#[cfg(test)]
#[path = "one_time_modal_model_tests.rs"]
mod tests;
