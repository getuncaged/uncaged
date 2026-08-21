//! Implementation of the [`SecureStorage`] service for macOS.
//!
//! Uncaged: file-backed, not the login keychain.
//!
//! Keychain ACLs are bound to a bundle's *designated requirement*. Uncaged is ad-hoc
//! signed -- it has no Apple Developer ID -- so that requirement is derived from the
//! cdhash and changes with every build. macOS therefore treats each new build, and each
//! update a user installs, as a different application and re-prompts for every stored
//! item, one modal at a time.
//!
//! That is not merely annoying. `SecKeychainFindGenericPassword` is synchronous, and the
//! read happens on the main thread during `initialize_app`
//! (`TemplatableMCPServerManager::new` -> `load_credentials_from_secure_storage`), so the
//! app does not finish launching until every prompt has been answered. Observed on a fresh
//! build as a startup that sat for forty minutes at 0% CPU behind a dialog.
//!
//! What is given up is less than it appears: per-app gating keyed on a requirement that,
//! under an ad-hoc signature, identifies nothing stable. Uncaged already keeps provider
//! credentials in plain files under its config directory (`connections.json`,
//! `engine.json`), so the keychain was never the only copy of this class of secret.
//!
//! If Uncaged ever gets a signing identity, this file should go back to the keychain: the
//! requirement becomes stable, the prompts stop, and encryption at rest is worth having.
//! The git history has the original implementation.

use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;

use anyhow::anyhow;

use super::Error;

/// Owner-only, matching what the keychain's file would be.
const DIR_MODE: u32 = 0o700;
const FILE_MODE: u32 = 0o600;

pub struct SecureStorage {
    /// The name of the service under which to store the values.
    service_name: String,
    /// Directory holding one file per key.
    storage_dir: PathBuf,
}

impl SecureStorage {
    pub fn new(service_name: &str) -> Self {
        let storage_dir = default_storage_dir(service_name);
        Self::new_with_path(service_name, storage_dir)
    }

    pub fn new_with_path(service_name: &str, storage_dir: PathBuf) -> Self {
        Self {
            service_name: service_name.to_owned(),
            storage_dir,
        }
    }

    /// One file per key. The service name is part of the filename rather than a
    /// subdirectory so that two services can share a storage dir without colliding.
    fn storage_file(&self, key: &str) -> PathBuf {
        self.storage_dir
            .join(format!("{}-{}", self.service_name, sanitize(key)))
    }

    fn ensure_dir(&self) -> Result<(), Error> {
        fs::create_dir_all(&self.storage_dir)
            .map_err(|err| Error::Unknown(anyhow!("creating secure storage dir: {err}")))?;
        // create_dir_all applies the umask, so set the mode explicitly.
        let _ = fs::set_permissions(&self.storage_dir, fs::Permissions::from_mode(DIR_MODE));
        Ok(())
    }
}

/// `~/Library/Application Support/<service>/secrets`, alongside the rest of the app's
/// state rather than in a location the user would not think to look.
fn default_storage_dir(service_name: &str) -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join("Library")
        .join("Application Support")
        .join(service_name)
        .join("secrets")
}

/// Keys reach the filesystem, so anything that could climb out of the directory or name a
/// hidden file has to go. Keys are internal identifiers, so this only ever has to be safe,
/// not reversible.
fn sanitize(key: &str) -> String {
    key.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

impl super::SecureStorage for SecureStorage {
    fn write_value(&self, key: &str, value: &str) -> Result<(), Error> {
        self.ensure_dir()?;
        let path = self.storage_file(key);

        // Create with the right mode from the start: writing first and chmod-ing after
        // leaves a window where the file is world-readable.
        use std::io::Write as _;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(FILE_MODE)
            .open(&path)
            .map_err(|err| Error::Unknown(anyhow!("opening {}: {err}", path.display())))?;
        file.write_all(value.as_bytes())
            .map_err(|err| Error::Unknown(anyhow!("writing {}: {err}", path.display())))?;

        // An existing file keeps its old mode through OpenOptions::mode, so restate it.
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(FILE_MODE));
        Ok(())
    }

    fn read_value(&self, key: &str) -> Result<String, Error> {
        let path = self.storage_file(key);
        let bytes = fs::read(&path).map_err(|err| match err.kind() {
            ErrorKind::NotFound => Error::NotFound,
            _ => Error::Unknown(anyhow!("reading {}: {err}", path.display())),
        })?;
        String::from_utf8(bytes).map_err(|err| Error::DecodeError(err.utf8_error()))
    }

    fn remove_value(&self, key: &str) -> Result<(), Error> {
        let path = self.storage_file(key);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == ErrorKind::NotFound => Err(Error::NotFound),
            Err(err) => Err(Error::Unknown(anyhow!(
                "removing {}: {err}",
                path.display()
            ))),
        }
    }
}
