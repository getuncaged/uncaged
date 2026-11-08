use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};

use super::*;

/// A scratch subkey under HKCU that tests create/destroy to avoid touching
/// the real `Software\Microsoft\Windows\CurrentVersion\Run` hive.
struct ScratchSubkey {
    path: String,
}

impl ScratchSubkey {
    fn new(name: &str) -> Self {
        let suffix = format!(
            "{}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            name,
        );
        let path = format!(r"Software\Warp\LoginItemTests\{suffix}");
        RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(&path)
            .expect("create scratch subkey");
        Self { path }
    }

    fn write(&self, value_name: &str, data: &str) {
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(&self.path)
            .expect("open scratch subkey");
        key.set_value(value_name, &data.to_owned())
            .expect("write scratch value");
    }

    fn read(&self, value_name: &str) -> Option<String> {
        let key = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(&self.path, KEY_READ)
            .ok()?;
        key.get_value::<String, _>(value_name).ok()
    }
}

impl Drop for ScratchSubkey {
    fn drop(&mut self) {
        let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&self.path);
    }
}

#[test]
fn oss_removes_only_its_own_value() {
    assert_eq!(legacy_value_names(Channel::Oss), &["WarpOss"]);
    for channel in [
        Channel::Stable,
        Channel::Preview,
        Channel::Dev,
        Channel::Local,
        Channel::Integration,
    ] {
        assert!(legacy_value_names(channel).is_empty(), "{channel:?}");
    }
}

#[test]
fn remove_is_idempotent() {
    let scratch = ScratchSubkey::new("remove_is_idempotent");
    // Never registered: nothing to remove.
    assert!(!remove_value_in(HKEY_CURRENT_USER, &scratch.path, "WarpOss").unwrap());
    scratch.write("WarpOss", r#""C:\Uncaged\uncaged.exe""#);
    assert!(remove_value_in(HKEY_CURRENT_USER, &scratch.path, "WarpOss").unwrap());
    assert!(!remove_value_in(HKEY_CURRENT_USER, &scratch.path, "WarpOss").unwrap());
    assert!(scratch.read("WarpOss").is_none());
}

#[test]
fn remove_leaves_other_values_alone() {
    let scratch = ScratchSubkey::new("remove_leaves_other_values_alone");
    scratch.write("WarpOss", r#""C:\Uncaged\uncaged.exe""#);
    scratch.write("Warp", r#""C:\Program Files\Warp\warp.exe""#);

    assert!(remove_value_in(HKEY_CURRENT_USER, &scratch.path, "WarpOss").unwrap());

    assert!(scratch.read("WarpOss").is_none());
    assert_eq!(
        scratch.read("Warp").as_deref(),
        Some(r#""C:\Program Files\Warp\warp.exe""#)
    );
}

#[test]
fn remove_missing_subkey_is_ok() {
    assert!(!remove_value_in(
        HKEY_CURRENT_USER,
        r"Software\Warp\LoginItemTests\does-not-exist",
        "WarpOss",
    )
    .unwrap());
}
