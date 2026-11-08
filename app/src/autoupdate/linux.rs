//! Uncaged does not self-update on Linux.
//!
//! Upstream's Linux updater drove the system package manager with Warp's
//! repositories and signing key, and its AppImage path buffered the whole
//! download with no size cap or hash before moving it over the running
//! executable. Neither ships here: Linux users install from a .deb/.rpm/AppImage
//! or their distro's package manager, so a self-update would fight whatever owns
//! the file anyway. We detect the new version and tell them; they update the way
//! they installed.

use anyhow::{bail, Result};

use super::DownloadReady;

/// The package name the Linux bundles install under. It predates the binary's
/// rename to `uncaged` and is kept so a new package upgrades the old one (see
/// script/linux/bundle).
const PACKAGE_NAME: &str = "warp-terminal-oss";

pub(super) fn download_update_and_cleanup() -> Result<DownloadReady> {
    Ok(DownloadReady::NeedsAuthorization)
}

pub(super) fn apply_update() -> Result<()> {
    bail!("Uncaged does not self-update on Linux; update via your package manager");
}

pub(super) fn relaunch() -> Result<()> {
    bail!("Uncaged does not self-update on Linux, so there is no update to relaunch into");
}

/// Describes how Uncaged was installed on this machine ("AppImage", the package manager's name,
/// or "unknown"), for the debug dump.
pub(crate) fn detect_install_method() -> String {
    if std::env::var_os("APPIMAGE").is_some() {
        return "AppImage".to_owned();
    }
    let detect_script = r#"
            command -p pacman -Qi $PACKAGE_NAME >/dev/null 2>/dev/null
            if [ $? -eq 0 ]; then
              echo "pacman"
              exit
            fi

            command -p zypper search --match-exact --installed-only $PACKAGE_NAME >/dev/null 2>/dev/null
            if [ $? -eq 0 ]; then
              echo "zypper"
              exit
            fi

            command -p dnf list --installed $PACKAGE_NAME >/dev/null 2>/dev/null
            if [ $? -eq 0 ]; then
              echo "dnf"
              exit
            fi

            command -p yum list installed $PACKAGE_NAME >/dev/null 2>/dev/null
            if [ $? -eq 0 ]; then
              echo "yum"
              exit
            fi

            if [ "$(command -p dpkg-query --show --showformat='${db:Status-Status}' $PACKAGE_NAME 2>/dev/null)" = "installed" ]; then
              echo "apt"
              exit
            fi

            exit 1
        "#;

    let output = command::blocking::Command::new("sh")
        .args(["-c", detect_script])
        .env("PACKAGE_NAME", PACKAGE_NAME)
        .output();
    match output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }
        Ok(_) | Err(_) => "unknown".to_owned(),
    }
}
