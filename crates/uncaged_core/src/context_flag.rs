//! ContextFlag flags are for behaviors that need to be conditionally enabled or disabled based
//! on where the app is being run and are a permanent part of the app.

use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};

use enum_iterator::{cardinality, Sequence};

use crate::channel::ChannelState;

/// All ContextFlag flag are enabled by default. Environments can conditionally disable flags.
///
/// Aside from manually setting specific flags in dogfood contexts, the complete list of contexts
/// this is used in is found in the ContextFlag impl.
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug, Sequence)]
pub enum ContextFlag {
    CreateNewSession,
    CloseWindow,
    ForceSidePanelOpen,
    HideOpenOnDesktopButton,
    PromptForVersionUpdates,
    NetworkLogConsole,
    RunWorkflow,
    LaunchConfigurations,
    WarpEssentials,
    AllowSettingsModalToClose,
    ShowSlowShellStartupBanner,
    DynamicBrowserUrl,
    ShowMCPServers,
}

/// The enablement states for context flags.  As mentioned in the documentation
/// for [`ContextFlag`], these are enabled by default.
static FLAG_STATES: [AtomicBool; cardinality::<ContextFlag>()] =
    [const { AtomicBool::new(true) }; { cardinality::<ContextFlag>() }];

impl ContextFlag {
    pub fn is_enabled(&self) -> bool {
        FLAG_STATES[*self as usize].load(Ordering::Relaxed)
    }

    /// Sets a ContextFlag flag. FOR DEBUG USE ONLY.
    pub fn set(&self, value: bool) {
        if !ChannelState::enable_debug_features() {
            log::error!(
                "Tried to set value of `ContextFlag` flag `{self:?}` in non-dogfood context."
            );
        }

        FLAG_STATES[*self as usize].store(value, Ordering::Relaxed);
    }
}

impl FromStr for ContextFlag {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "CreateNewSession" => Ok(Self::CreateNewSession),
            "CloseWindow" => Ok(Self::CloseWindow),
            "ForceSidePanelOpen" => Ok(Self::ForceSidePanelOpen),
            "HideOpenOnDesktopButton" => Ok(Self::HideOpenOnDesktopButton),
            "PromptForVersionUpdates" => Ok(Self::PromptForVersionUpdates),
            "NetworkLogConsole" => Ok(Self::NetworkLogConsole),
            "RunWorkflow" => Ok(Self::RunWorkflow),
            "LaunchConfigurations" => Ok(Self::LaunchConfigurations),
            "WarpEssentials" => Ok(Self::WarpEssentials),
            _ => Err(()),
        }
    }
}
