//! Secure storage for passwords and other application secrets.
//!
//! This defines an API for interacting with an underlying secure storage
//! system, implementations of the API for various platforms, testing
//! utilities, and extension traits to improve ergonomics of using the APIs.

#[cfg(not(target_family = "wasm"))]
#[cfg_attr(target_os = "macos", path = "mac.rs")]
#[cfg_attr(any(target_os = "linux", target_os = "freebsd"), path = "linux.rs")]
#[cfg_attr(target_os = "windows", path = "windows.rs")]
mod imp;
mod legacy;
mod noop;
mod unavailable;

// Treat this as a noop on web, as there is no backing storage which is "secure".
#[cfg(target_family = "wasm")]
use noop as imp;

#[cfg(target_os = "windows")]
mod windows_only {
    pub(super) use std::string::FromUtf8Error;
}

#[cfg(target_os = "windows")]
use windows_only::*;

/// A type alias for the concrete type stored within a uncaged_ui
/// app context, enabling usage such as:
///
/// ```
/// use uncaged_ui_core::{App, SingletonEntity};
/// use uncaged_ui_extras::secure_storage;
///
/// App::test((), |mut app| async move {
///     app.update(|ctx| {
///         #[cfg(not(windows))]
///         secure_storage::register("service_name", None, ctx);
///         #[cfg(windows)]
///         secure_storage::register_with_dir("service_name", None, std::path::PathBuf::from(r"C:\some\path"), ctx);
///
///         let _ = secure_storage::Model::handle(ctx).as_ref(ctx).read_value("some_key");
///     });
/// });
/// ```
/// Note that the above rustdoc example is `ignore`d in compilation
/// due to API differences across platforms.
pub type Model = Box<dyn SecureStorage>;

/// Registers a platform-native Secure Storage provider with the application.
///
/// The service name is used as a namespace for the application's secrets.  It
/// is recommended that this be a unique identifier for the application; one
/// common scheme is reverse-DNS notation (e.g.: "dev.warp.Warp").
///
/// `legacy_service_name` is the service name a previous identity of the app
/// stored its secrets under, if any: values missing from `service_name` are read
/// from it and copied forward (see `legacy.rs`).
#[cfg(not(target_os = "windows"))]
pub fn register(
    service_name: &str,
    legacy_service_name: Option<&str>,
    ctx: &mut uncaged_ui_core::AppContext,
) {
    ctx.add_singleton_model(|_| -> Model {
        let current: Model = Box::new(imp::SecureStorage::new(service_name));
        #[cfg(target_os = "macos")]
        if let Some(legacy_service_name) = legacy_service_name {
            // After the identity's directories moved, the previous service's
            // files sit in the current service's directory.
            let legacy = imp::SecureStorage::new_with_path(
                legacy_service_name,
                imp::default_storage_dir(service_name),
            );
            return with_legacy(current, Box::new(legacy));
        }
        #[cfg(not(target_os = "macos"))]
        if let Some(legacy_service_name) = legacy_service_name {
            return with_legacy(
                current,
                Box::new(imp::SecureStorage::new(legacy_service_name)),
            );
        }
        current
    });
}

/// Registers a no-op Secure Storage provider with the application.
pub fn register_noop(service_name: &str, ctx: &mut uncaged_ui_core::AppContext) {
    ctx.add_singleton_model(|_| -> Model { Box::new(noop::SecureStorage::new(service_name)) });
}

/// Registers an unavailable Secure Storage provider that deliberately does not persist values.
///
/// Reads report missing values, while writes and removals succeed without accessing storage.
pub fn register_unavailable(ctx: &mut uncaged_ui_core::AppContext) {
    ctx.add_singleton_model(|_| -> Model { Box::new(unavailable::SecureStorage) });
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub fn register_with_fallback(
    service_name: &str,
    legacy_service_name: Option<&str>,
    fallback_dir: std::path::PathBuf,
    ctx: &mut uncaged_ui_core::AppContext,
) {
    ctx.add_singleton_model(|_| -> Model {
        let current: Model = Box::new(imp::SecureStorage::new_with_fallback(
            service_name,
            fallback_dir.clone(),
        ));
        match legacy_service_name {
            Some(legacy_service_name) => with_legacy(
                current,
                Box::new(imp::SecureStorage::new_with_fallback(
                    legacy_service_name,
                    fallback_dir,
                )),
            ),
            None => current,
        }
    });
}

/// Registers a Windows-native Secure Storage provider
/// that uses the provided directory to store data in encrypted files.
#[cfg(target_os = "windows")]
pub fn register_with_dir(
    service_name: &str,
    legacy_service_name: Option<&str>,
    storage_dir: std::path::PathBuf,
    ctx: &mut uncaged_ui_core::AppContext,
) {
    ctx.add_singleton_model(|_| -> Model {
        let current: Model = Box::new(imp::SecureStorage::new_with_path(
            service_name,
            storage_dir.clone(),
        ));
        match legacy_service_name {
            Some(legacy_service_name) => with_legacy(
                current,
                Box::new(imp::SecureStorage::new_with_path(
                    legacy_service_name,
                    storage_dir,
                )),
            ),
            None => current,
        }
    });
}

fn with_legacy(current: Model, legacy: Model) -> Model {
    Box::new(legacy::WithLegacyService { current, legacy })
}

/// A trait representing a secure store for key-value pairs.
///
/// This is typically backed by an OS-provided secure storage system.
pub trait SecureStorage {
    /// Writes a value at the given key.
    fn write_value(&self, key: &str, value: &str) -> Result<(), Error>;
    /// Writes a value while requiring any file fallback to be owner-only.
    ///
    /// Platforms without a file fallback use their normal secure-storage write
    /// path. Callers should opt into this only when they require the stronger
    /// fallback behavior because it may create or change fallback permissions.
    fn write_value_with_owner_only_fallback(&self, key: &str, value: &str) -> Result<(), Error> {
        self.write_value(key, value)
    }

    /// Reads the value stored at the given key.
    fn read_value(&self, key: &str) -> Result<String, Error>;

    /// Removes the value stored at the given key, if any.
    fn remove_value(&self, key: &str) -> Result<(), Error>;
}

impl uncaged_ui_core::Entity for Model {
    type Event = ();
}

impl uncaged_ui_core::SingletonEntity for Model {}

/// Enumerates the various errors that can occur when interacting with secure
/// storage.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The item with the given key was not found in secure storage.
    ///
    /// This is not guaranteed to be returned in all cases where the item is
    /// not found; if we are not able to interpret the error returned by the
    /// underlying implementation, [`SecureStorageError::Unknown`] may be
    /// returned.
    #[error("item not found")]
    NotFound,

    /// Failed to decode the stored bytes into a UTF-8 string.
    #[error("failed to decode UTF-8 string from bytes")]
    DecodeError(#[from] std::str::Utf8Error),

    /// Encountered an error when reading to or from a file.
    #[cfg(windows)]
    #[error("File I/O error")]
    IOError(#[from] std::io::Error),

    /// An error was encountered while using the windows CryptProtect API.
    #[cfg(windows)]
    #[error("Windows CryptProtect API error")]
    WindowsAPIError(#[from] windows::core::Error),

    /// The provided secure storage directory path was not valid.
    #[cfg(windows)]
    #[error("Invalid secure storage location")]
    InvalidLocation,

    /// Catch-all for unclassifiable errors.
    #[error("unknown error")]
    Unknown(#[from] anyhow::Error),
}

#[cfg(windows)]
impl From<FromUtf8Error> for Error {
    fn from(value: FromUtf8Error) -> Self {
        Self::DecodeError(value.utf8_error())
    }
}

/// An extension trait to make secure storage easier to use.
///
/// ```
/// use uncaged_ui_core::{App, SingletonEntity};
/// use uncaged_ui_extras::secure_storage;
///
/// App::test((), |mut app| async move {
///     app.update(|ctx| {
///         #[cfg(not(windows))]
///         secure_storage::register("service_name", None, ctx);
///         #[cfg(windows)]
///         secure_storage::register_with_dir("service_name", None, std::path::PathBuf::from(r"C:\some\path"), ctx);
///
///         use secure_storage::AppContextExt;
///         let _ = ctx.secure_storage().read_value("some_key");
///     });
/// });
/// ```
/// Note that the above rustdoc example is `ignore`d in compilation
/// due to API differences across platforms.
pub trait AppContextExt {
    fn secure_storage(&self) -> &dyn SecureStorage;
}

impl AppContextExt for uncaged_ui_core::AppContext {
    fn secure_storage(&self) -> &dyn SecureStorage {
        use uncaged_ui_core::SingletonEntity;

        <Model as SingletonEntity>::as_ref(self).as_ref()
    }
}
