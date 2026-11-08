//! Windows removal of the login item registered under the HKCU
//! `Software\Microsoft\Windows\CurrentVersion\Run` registry key.
//!
//! This is the per-user startup list surfaced by **Settings → Apps → Startup**
//! and **Task Manager → Startup apps** on Windows 10/11.

use uncaged_core::channel::{Channel, ChannelState};
use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
use winreg::RegKey;

use super::Outcome;

/// The registry subkey Windows scans on sign-in to launch per-user startup apps.
const RUN_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Removes this app's values from the `Run` key, leaving every other value alone.
pub(super) fn remove_login_item() -> Outcome {
    let mut outcome = Outcome::NotRegistered;
    for value_name in legacy_value_names(ChannelState::channel()) {
        match remove_value_in(HKEY_CURRENT_USER, RUN_SUBKEY, value_name) {
            Ok(true) => {
                log::info!(r"Removed the {value_name} value from HKCU\{RUN_SUBKEY}");
                if outcome == Outcome::NotRegistered {
                    outcome = Outcome::Removed;
                }
            }
            Ok(false) => {}
            Err(err) => {
                log::warn!(
                    r"Failed to remove the {value_name} value from HKCU\{RUN_SUBKEY}: {err}"
                );
                outcome = Outcome::Failed;
            }
        }
    }
    outcome
}

/// The `Run` value names earlier builds of this channel registered.
///
/// Uncaged registered under `WarpOss`, its application name up to v0.3.4, and
/// kept that name across the rename to `Uncaged`, so it is the only one. The
/// other channels' names (`Warp`, `WarpPreview`, ...) belong to Warp's own
/// installs and are never touched.
fn legacy_value_names(channel: Channel) -> &'static [&'static str] {
    match channel {
        Channel::Oss => &["WarpOss"],
        Channel::Stable
        | Channel::Preview
        | Channel::Dev
        | Channel::Local
        | Channel::Integration => &[],
    }
}

/// Deletes `value_name` under `subkey` if present. Returns whether a value was
/// deleted; a missing subkey or value is not an error.
fn remove_value_in(hive: winreg::HKEY, subkey: &str, value_name: &str) -> std::io::Result<bool> {
    let hkey = RegKey::predef(hive);
    let run_key = match hkey.open_subkey_with_flags(subkey, KEY_SET_VALUE) {
        Ok(k) => k,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };
    match run_key.delete_value(value_name) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;
