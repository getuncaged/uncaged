//! Uncaged: who owns this install?
//!
//! If a package manager put the app there, the app must not replace itself. Doing
//! so leaves the manager's records pointing at a version that is no longer on
//! disk: `brew upgrade` would later reinstall over the top, `brew uninstall`
//! would remove a bundle it no longer matches, and the user's own tooling stops
//! describing their machine. The correct move is to tell them a version exists
//! and let the thing that installed it do the installing.
//!
//! Detection is by Homebrew's install receipt, not by the app being a symlink.
//! A cask's `app` stanza *copies* the bundle into /Applications — verified
//! against real installs — so there is no symlink to find, and a check for one
//! would silently never fire.

use std::path::{Path, PathBuf};

/// The cask token Uncaged is published under (`getuncaged/homebrew-tap`).
const CASK_TOKEN: &str = "uncaged";

/// Where Homebrew lives: Apple Silicon first, then Intel.
const BREW_PREFIXES: &[&str] = &["/opt/homebrew", "/usr/local"];

/// Set to any non-empty value to stop the app ever replacing itself, whatever
/// the detection concludes. An escape hatch for people who manage their own
/// installs and do not want us guessing.
const DISABLE_ENV: &str = "UNCAGED_DISABLE_SELF_UPDATE";

/// How this copy of Uncaged appears to have been installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallSource {
    /// Nothing claims ownership — a direct download. Safe to self-update.
    SelfManaged,
    /// Homebrew has a receipt for this cask. Point the user at `brew upgrade`.
    Homebrew,
    /// The user explicitly opted out of self-update.
    DisabledByUser,
}

impl InstallSource {
    /// Whether the app may replace its own bundle.
    pub fn may_self_update(self) -> bool {
        matches!(self, InstallSource::SelfManaged)
    }

    /// What to tell the user to run instead, if anything.
    pub fn upgrade_hint(self) -> Option<&'static str> {
        match self {
            InstallSource::Homebrew => Some("brew upgrade --cask uncaged"),
            InstallSource::SelfManaged | InstallSource::DisabledByUser => None,
        }
    }

    /// The command that upgrades this install, for us to run on the user's behalf.
    ///
    /// Uncaged: a managed install is still a one-click update -- the click asks the manager
    /// to do it instead of replacing the bundle behind its back, which keeps the manager's
    /// receipt truthful. `DisabledByUser` deliberately yields nothing: someone who set
    /// `UNCAGED_DISABLE_SELF_UPDATE` asked us not to touch their install, and running their
    /// package manager is still touching it.
    ///
    /// The brew binary is resolved from the prefix that actually exists rather than assumed
    /// to be on `PATH`, because a GUI app inherits launchd's environment, not a login
    /// shell's -- `/opt/homebrew/bin` is typically absent from it.
    pub fn upgrade_command(self) -> Option<Vec<String>> {
        match self {
            InstallSource::Homebrew => {
                let brew = BREW_PREFIXES
                    .iter()
                    .map(|prefix| Path::new(prefix).join("bin/brew"))
                    .find(|candidate| candidate.exists())?;
                Some(vec![
                    brew.to_string_lossy().into_owned(),
                    "upgrade".to_owned(),
                    "--cask".to_owned(),
                    CASK_TOKEN.to_owned(),
                ])
            }
            InstallSource::SelfManaged | InstallSource::DisabledByUser => None,
        }
    }
}

/// Detects how this copy was installed.
pub fn detect() -> InstallSource {
    detect_with(
        |name| std::env::var(name).ok(),
        BREW_PREFIXES.iter().map(Path::new),
    )
}

/// The testable core: `env` resolves an environment variable, `prefixes` are the
/// Homebrew roots to look under.
fn detect_with<'a>(
    env: impl Fn(&str) -> Option<String>,
    prefixes: impl Iterator<Item = &'a Path>,
) -> InstallSource {
    // An empty value is not an opt-out: `UNCAGED_DISABLE_SELF_UPDATE=` left in a
    // shell profile would otherwise disable updates forever with no way for the
    // user to tell that is what happened.
    if env(DISABLE_ENV).is_some_and(|v| !v.trim().is_empty()) {
        return InstallSource::DisabledByUser;
    }
    for prefix in prefixes {
        if has_cask_receipt(prefix) {
            return InstallSource::Homebrew;
        }
    }
    InstallSource::SelfManaged
}

/// Whether Homebrew holds an install receipt for our cask under `prefix`.
///
/// The receipt is the thing that makes brew's records authoritative; its
/// presence is what a self-update would invalidate. We deliberately do not also
/// require the recorded version to match the running one: a user who brew-
/// installed and has since replaced the bundle by hand is exactly the case where
/// guessing wrong breaks their tooling, and the cost of being cautious is one
/// line of text suggesting `brew upgrade` instead of an automatic install.
fn has_cask_receipt(prefix: &Path) -> bool {
    receipt_path(prefix).is_file()
}

fn receipt_path(prefix: &Path) -> PathBuf {
    prefix
        .join("Caskroom")
        .join(CASK_TOKEN)
        .join(".metadata")
        .join("INSTALL_RECEIPT.json")
}

#[cfg(test)]
#[path = "install_source_tests.rs"]
mod tests;
