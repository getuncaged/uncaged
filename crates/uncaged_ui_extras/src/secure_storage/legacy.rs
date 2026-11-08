//! Secure storage that still finds values stored under a previous service name.
//!
//! An app that changes its identity changes the service name its secrets are
//! namespaced by. Rather than copying every secret up front (on Linux that means
//! enumerating Secret Service items before the app has started), a value the
//! current service does not have is read from the previous one and copied
//! forward. The previous service keeps its copy, so a downgrade still finds it.

use super::{Error, Model, SecureStorage};

pub(super) struct WithLegacyService {
    pub(super) current: Model,
    pub(super) legacy: Model,
}

impl SecureStorage for WithLegacyService {
    fn write_value(&self, key: &str, value: &str) -> Result<(), Error> {
        self.current.write_value(key, value)
    }

    fn write_value_with_owner_only_fallback(&self, key: &str, value: &str) -> Result<(), Error> {
        self.current
            .write_value_with_owner_only_fallback(key, value)
    }

    fn read_value(&self, key: &str) -> Result<String, Error> {
        match self.current.read_value(key) {
            Err(Error::NotFound) => {
                // Whatever went wrong reading the previous service, the value
                // is not available, which is what `NotFound` says.
                let value = self.legacy.read_value(key).map_err(|_| Error::NotFound)?;
                // Owner-only, since we cannot tell how the value was first written.
                if let Err(err) = self
                    .current
                    .write_value_with_owner_only_fallback(key, &value)
                {
                    log::warn!(
                        "Could not copy secret {key} forward from the previous identity: {err}"
                    );
                }
                Ok(value)
            }
            other => other,
        }
    }

    /// Removes the value from both services: otherwise the next read would
    /// bring a value the user deleted back from the previous one.
    fn remove_value(&self, key: &str) -> Result<(), Error> {
        let current = self.current.remove_value(key);
        let legacy = self.legacy.remove_value(key);
        match (current, legacy) {
            (Err(Error::NotFound), Ok(())) => Ok(()),
            (current, _) => current,
        }
    }
}

#[cfg(test)]
#[path = "legacy_tests.rs"]
mod tests;
