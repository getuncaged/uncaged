//! One-time removal of the login item that earlier builds registered.
//!
//! Uncaged never launches itself at login. Builds up to this change registered
//! the app to start at sign-in by default (via `SMAppService` on macOS and the
//! HKCU `Run` key on Windows). This module removes that registration once per
//! install and records that it ran in
//! [`crate::terminal::general_settings::GeneralSettings`], so later launches
//! skip it. It only ever touches this app's own entry.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use ::settings::Setting;
#[cfg(target_os = "macos")]
use macos::remove_login_item;
use uncaged_core::channel::ChannelState;
use uncaged_ui::{AppContext, SingletonEntity};
#[cfg(target_os = "windows")]
use windows::remove_login_item;

use crate::report_if_error;
use crate::terminal::general_settings::GeneralSettings;

/// What the platform backend found and did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    /// There was no login item for this app, so nothing was changed.
    NotRegistered,
    /// The login item was removed.
    Removed,
    /// Removing the login item failed; the next launch tries again.
    Failed,
}

impl Outcome {
    /// Whether the removal is finished and shouldn't be attempted again.
    fn is_done(self) -> bool {
        match self {
            Outcome::NotRegistered | Outcome::Removed => true,
            Outcome::Failed => false,
        }
    }
}

/// Removes the login item earlier builds registered, once per install.
///
/// Skipped when the `WARP_INTEGRATION` env var is set, so integration tests
/// never touch the user's real login items / registry, and for non-release
/// builds (e.g. `cargo run`), which never registered one.
pub fn remove_legacy_login_item(ctx: &mut AppContext) {
    if std::env::var("WARP_INTEGRATION").is_ok() {
        log::debug!("Not removing the legacy login item in integration tests");
        return;
    }
    if !ChannelState::is_release_bundle() {
        log::debug!("Not a release bundle, skipping legacy login-item removal");
        return;
    }
    GeneralSettings::handle(ctx).update(ctx, |settings, ctx| {
        if *settings.legacy_login_item_removed {
            return;
        }
        // This can be slow, so we run it in a background thread.
        ctx.spawn(
            async move { remove_login_item() },
            |settings, outcome, ctx| {
                match outcome {
                    Outcome::NotRegistered => log::info!("No legacy login item to remove"),
                    Outcome::Removed => log::info!("Removed the legacy login item"),
                    Outcome::Failed => {
                        log::warn!("Failed to remove the legacy login item; will retry next launch")
                    }
                }
                if outcome.is_done() {
                    report_if_error!(settings.legacy_login_item_removed.set_value(true, ctx));
                }
            },
        );
    });
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
