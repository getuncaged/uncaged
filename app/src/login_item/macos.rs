//! macOS removal of the login item registered via `SMAppService`.
//!
//! `SMAppService` exists on macOS 13 (Ventura) and later only, which is also
//! the only place earlier builds could register a login item; without it there
//! is nothing to remove.

use objc2::runtime::{AnyClass, AnyObject};
use objc2::{class, msg_send};

use super::Outcome;

/// `SMAppServiceStatusEnabled`: registered and allowed to run at login.
const STATUS_ENABLED: isize = 1;
/// `SMAppServiceStatusRequiresApproval`: registered, but switched off by the
/// user in System Settings > General > Login Items.
const STATUS_REQUIRES_APPROVAL: isize = 2;

/// Whether an `SMAppServiceStatus` value means the service is registered.
/// `NotRegistered` (0) and `NotFound` (3) leave nothing to unregister.
fn is_registered(status: isize) -> bool {
    matches!(status, STATUS_ENABLED | STATUS_REQUIRES_APPROVAL)
}

/// Unregisters this app's `SMAppService.mainAppService` if it is registered.
/// `mainAppService` is the running bundle's own login item, so this never
/// touches another app's.
pub(super) fn remove_login_item() -> Outcome {
    // `[NSBundle mainBundle]` is nil when we're not running from a bundle, in
    // which case there is no login item for this app.
    let bundle: *mut AnyObject = unsafe { msg_send![class!(NSBundle), mainBundle] };
    if bundle.is_null() {
        log::debug!("Not running in a bundle, so there is no login item to remove");
        return Outcome::NotRegistered;
    }

    let Some(sm_app_service_class) = AnyClass::get(c"SMAppService") else {
        log::debug!("SMAppService is unavailable (macOS 12 or earlier); no login item to remove");
        return Outcome::NotRegistered;
    };
    let app_service: *mut AnyObject = unsafe { msg_send![sm_app_service_class, mainAppService] };
    if app_service.is_null() {
        return Outcome::NotRegistered;
    }

    let status: isize = unsafe { msg_send![app_service, status] };
    if !is_registered(status) {
        log::debug!("Login item is not registered (SMAppServiceStatus {status})");
        return Outcome::NotRegistered;
    }

    let mut error: *mut AnyObject = std::ptr::null_mut();
    let unregistered: bool =
        unsafe { msg_send![app_service, unregisterAndReturnError: &mut error] };
    if unregistered {
        return Outcome::Removed;
    }
    if error.is_null() {
        log::warn!("Failed to unregister the login item (SMAppServiceStatus {status})");
    } else {
        let code: isize = unsafe { msg_send![error, code] };
        log::warn!(
            "Failed to unregister the login item (SMAppServiceStatus {status}, error code {code})"
        );
    }
    Outcome::Failed
}

#[cfg(test)]
#[path = "macos_tests.rs"]
mod tests;
