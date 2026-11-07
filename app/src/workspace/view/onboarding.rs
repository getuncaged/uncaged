use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use onboarding::{ProjectOnboardingSettings, SelectedSettings};
use uncaged_core::execution_mode::AppExecutionMode;
use uncaged_ui::{SingletonEntity as _, ViewContext};

use crate::pane_group::{NewTerminalOptions, PanesLayout};
use crate::terminal::view::OnboardingIntention;
use crate::workspace::Workspace;
use crate::FeatureFlag;

/// Configuration for starting the agent onboarding tutorial.
#[derive(Debug, Clone)]
pub enum OnboardingTutorial {
    /// Start tutorial without a project context.
    NoProject { intention: OnboardingIntention },
    /// Start tutorial with a project path, but don't run init.
    Project {
        path: PathBuf,
        intention: OnboardingIntention,
    },
    /// Start tutorial with a project path and run init flow first.
    InitProject {
        path: PathBuf,
        intention: OnboardingIntention,
    },
}

impl OnboardingTutorial {
    /// Extracts the onboarding intention from any tutorial variant.
    pub(crate) fn intention(&self) -> OnboardingIntention {
        match self {
            OnboardingTutorial::NoProject { intention }
            | OnboardingTutorial::Project { intention, .. }
            | OnboardingTutorial::InitProject { intention, .. } => *intention,
        }
    }
}

impl From<SelectedSettings> for OnboardingTutorial {
    fn from(settings: SelectedSettings) -> Self {
        match settings {
            SelectedSettings::AgentDrivenDevelopment {
                project_settings, ..
            } => match project_settings {
                ProjectOnboardingSettings::Project {
                    selected_local_folder,
                    initialize_projects_automatically,
                } => {
                    let path = PathBuf::from(selected_local_folder);
                    // When AgentView is enabled, /init comes at the end of the tutorial.
                    if !FeatureFlag::AgentView.is_enabled() && initialize_projects_automatically {
                        OnboardingTutorial::InitProject {
                            path,
                            intention: OnboardingIntention::AgentDrivenDevelopment,
                        }
                    } else {
                        OnboardingTutorial::Project {
                            path,
                            intention: OnboardingIntention::AgentDrivenDevelopment,
                        }
                    }
                }
                ProjectOnboardingSettings::NoProject => OnboardingTutorial::NoProject {
                    intention: OnboardingIntention::AgentDrivenDevelopment,
                },
            },
            SelectedSettings::Terminal { .. } => OnboardingTutorial::NoProject {
                intention: OnboardingIntention::Terminal,
            },
        }
    }
}

impl Workspace {
    /// Opens the project picked during onboarding. The guided tutorial that used to follow never
    /// ran in Uncaged, so only the project is opened.
    ///
    /// Depending on the variant of `tutorial`, this will either:
    /// - `NoProject`: Do nothing
    /// - `Project`: Open a terminal in the project directory
    /// - `InitProject`: Open the repository, which creates a new terminal and runs init
    pub(crate) fn start_agent_onboarding_tutorial(
        &mut self,
        tutorial: OnboardingTutorial,
        ctx: &mut ViewContext<Self>,
    ) {
        // Onboarding requires a real user to interact with it; skip when running
        // in a headless mode like the SDK/CLI.
        if !AppExecutionMode::as_ref(ctx).can_show_onboarding() {
            return;
        }

        match tutorial {
            OnboardingTutorial::InitProject { ref path, .. } => {
                let Some(path_str) = path.to_str() else {
                    log::error!("Failed to convert path to string: {path:?}");
                    return;
                };
                self.handle_open_repository(path_str, ctx);
            }
            OnboardingTutorial::Project { path, .. } => {
                // Create a new terminal in the project directory
                self.add_tab_with_pane_layout(
                    PanesLayout::SingleTerminal(Box::new(NewTerminalOptions {
                        initial_directory: Some(path),
                        hide_homepage: true,
                        ..Default::default()
                    })),
                    Arc::new(HashMap::new()),
                    None,
                    ctx,
                );
            }
            OnboardingTutorial::NoProject { .. } => {}
        }
    }
}
