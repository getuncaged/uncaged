// Onboarding library crate

mod agent_onboarding_view;
mod model;
pub mod slides;

/// The user's intention selected during onboarding slides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnboardingIntention {
    Terminal,
    AgentDrivenDevelopment,
}

/// User-facing descriptions of the AI features enabled when the agent intention is selected.
/// Shared by the intention slide's agent card checklist and the login slide's
/// skip-login confirmation dialog so the two always stay in sync.
pub const AI_FEATURES: &[&str] = &[
    "Bring your own model — API keys, local runtimes, or CLIs",
    "Automatically diagnose and fix terminal errors",
    "Agentic control of long-running commands and TUIs",
    "Review code diffs and send comments directly to agents",
    "Works with Claude Code, Codex, Gemini, and more",
];

/// User-facing names of the built-in terminal features highlighted alongside the
/// terminal intention. Shared by the login slide's skip-login confirmation dialog
/// so the list stays in sync with any future surfaces that need it.
pub const WARP_DRIVE_FEATURES: &[&str] = &["Blocks & workflows", "Command history"];

pub mod components;
mod visuals;

/// The default mode for new sessions, chosen during onboarding.
/// Mapped to `DefaultSessionMode` at the application boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SessionDefault {
    #[default]
    Agent,
    Terminal,
}

pub use agent_onboarding_view::{AgentOnboardingAction, AgentOnboardingEvent, AgentOnboardingView};
pub use model::{SelectedSettings, UICustomizationSettings};
pub use slides::{
    ConnectConnectionView, ConnectGalleryData, ConnectPresetView, ConnectSectionView,
    ProjectOnboardingSettings,
};

pub fn init(app: &mut warpui_core::AppContext) {
    agent_onboarding_view::init(app);
}
