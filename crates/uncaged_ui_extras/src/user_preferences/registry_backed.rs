use std::io;

use windows_registry::{Key, Value, CURRENT_USER};
use windows_result::HRESULT;

/// Store user preferences in the Windows Registry.
/// Modeled after https://github.com/neovide/neovide/blob/main/src/windows_utils.rs .
use super::UserPreferences;

pub struct RegistryBackedPreferences {
    app_key_path: String,
}

static WARP_REGISTRY_BASE_PATH: &str = "Software\\Warp.dev\\";
pub const KEY_NOT_FOUND_ERR: HRESULT = HRESULT::from_win32(0x80070002);

impl RegistryBackedPreferences {
    /// Construct a separate registry path for each channel (stable, dev, local, etc.)
    pub fn new(app_name: &str) -> Self {
        Self {
            app_key_path: WARP_REGISTRY_BASE_PATH.to_owned() + app_name,
        }
    }

    /// Gets Warp's registry key, creating it if it does not already exist.
    fn get_warp_registry(&self) -> Result<Key, super::Error> {
        CURRENT_USER.create(self.app_key_path.clone()).map_err(|e| {
            log::error!("unable to access Warp app key in Windows Registry: {e:#}");
            super::Error::IoError(io::Error::from(e))
        })
    }
}

/// Copies the values of `from_app`'s preferences key into `to_app`'s key, once.
///
/// Values `to_app` already has are kept. Afterwards `to_app`'s key records
/// `from_app` under `marker_value_name`, and later calls return `Ok(false)`
/// without copying again; so do calls when `from_app` has no key. Returns
/// whether anything was copied.
pub fn copy_app_values_once(
    from_app: &str,
    to_app: &str,
    marker_value_name: &str,
) -> windows_result::Result<bool> {
    let to_key = CURRENT_USER.create(WARP_REGISTRY_BASE_PATH.to_owned() + to_app)?;
    if to_key.get_value(marker_value_name).is_ok() {
        return Ok(false);
    }
    let from_key = match CURRENT_USER.open(WARP_REGISTRY_BASE_PATH.to_owned() + from_app) {
        Ok(key) => key,
        Err(err) if err.code() == KEY_NOT_FOUND_ERR => return Ok(false),
        Err(err) => return Err(err),
    };
    for (name, value) in from_key.values()? {
        if to_key.get_value(&name).is_err() {
            match value {
                Value::U32(number) => to_key.set_u32(name.as_str(), number)?,
                Value::U64(number) => to_key.set_u64(name.as_str(), number)?,
                Value::String(text) => to_key.set_string(name.as_str(), text.as_str())?,
                Value::Bytes(bytes) => to_key.set_bytes(name.as_str(), &bytes)?,
                Value::MultiString(texts) => {
                    let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
                    to_key.set_multi_string(name.as_str(), &texts)?
                }
                // A registry type the crate can't represent: there is no public way to
                // write it back, so it is left behind in the old identity's key.
                Value::Unknown(_) => {}
            }
        }
    }
    to_key.set_string(marker_value_name, from_app)?;
    Ok(true)
}

impl UserPreferences for RegistryBackedPreferences {
    fn read_value(&self, name: &str) -> Result<Option<String>, super::Error> {
        Ok(self.get_warp_registry()?.get_string(name).ok())
    }

    fn write_value(&self, key: &str, value: String) -> Result<(), super::Error> {
        self.get_warp_registry()?
            .set_string(key, value.as_str())
            .map_err(|e| super::Error::from(io::Error::from(e)))
    }

    fn remove_value(&self, key: &str) -> Result<(), super::Error> {
        match self.get_warp_registry()?.remove_value(key) {
            Ok(_) => Ok(()),
            // If the key doesn't exist, then treat removal of that nonexistent key as a success.
            Err(e) if e.code() == KEY_NOT_FOUND_ERR => Ok(()),
            Err(e) => Err(super::Error::from(io::Error::from(e))),
        }
    }
}
