//! Pins every published name the v0.3.4 updater (and the scripts it runs)
//! depends on, so a later rename cannot silently strand existing installs.
//!
//! v0.3.4 is the last release named `warp-oss` / `dev.uncaged.WarpOss`. How it
//! updates, per platform, is described under "The identity rename" in
//! RELEASING.md; each test below names the step it protects.

use super::github_releases;

const RELEASE_WORKFLOW: &str = include_str!("../../../.github/workflows/uncaged-release.yml");
const MACOS_BUNDLE: &str = include_str!("../../../script/macos/bundle");
const CLI_UPDATER: &str = include_str!("../../../script/uncaged-update");
const LINUX_BUNDLE: &str = include_str!("../../../script/linux/bundle");
const LINUX_BUNDLE_INSTALL: &str = include_str!("../../../script/linux/bundle_install");
const LINUX_BUNDLE_DEB: &str = include_str!("../../../script/linux/bundle_deb");
const DEB_CONTROL: &str = include_str!("../../../resources/linux/debian/app/control.template");
const WINDOWS_INSTALLER: &str = include_str!("../../../script/windows/windows-installer.iss");
const APP_MANIFEST: &str = include_str!("../../Cargo.toml");

/// The asset-name fragments v0.3.4's `github_releases::current_target()` looks
/// for. Every published platform must keep carrying its fragment.
const V0_3_4_TARGETS: &[&str] = &[
    "macos-aarch64",
    "macos-x86_64",
    "windows-aarch64",
    "windows-x86_64",
    "linux-aarch64",
    "linux-x86_64",
];

/// The release assets the workflow publishes, with `${{ matrix.arch }}` expanded.
fn published_assets() -> Vec<String> {
    let mut assets = Vec::new();
    for line in RELEASE_WORKFLOW.lines() {
        let line = line.trim();
        let line = line.strip_prefix("files: ").unwrap_or(line);
        let Some(path) = line.strip_prefix("dist/") else {
            continue;
        };
        if path.contains("${{ matrix.arch }}") {
            for arch in ["aarch64", "x86_64"] {
                assets.push(path.replace("${{ matrix.arch }}", arch));
            }
        } else {
            assets.push(path.to_owned());
        }
    }
    assets
}

#[test]
fn every_v0_3_4_target_still_has_a_published_asset() {
    let assets = published_assets();
    for target in V0_3_4_TARGETS {
        assert!(
            assets.iter().any(|asset| asset.contains(target)),
            "no published asset carries {target}: {assets:?}"
        );
    }
    // The current updater looks for the same fragments.
    if let Some(target) = github_releases::current_target() {
        assert!(V0_3_4_TARGETS.contains(&target));
    }
}

/// v0.3.4's CLI updater runs getuncaged.dev/install.sh, which downloads
/// `Uncaged-macos-<arch>.zip` and expects `Uncaged.app` at its top level; the
/// Homebrew cask installs `Uncaged.app` from the DMG.
#[test]
fn macos_assets_and_the_app_inside_them_keep_their_names() {
    let assets = published_assets();
    for arch in ["aarch64", "x86_64"] {
        for ext in ["zip", "dmg"] {
            let name = format!("Uncaged-macos-{arch}.{ext}");
            assert!(assets.contains(&name), "{name} is not published");
        }
    }
    assert!(RELEASE_WORKFLOW.contains(
        r#"ditto -c -k --keepParent "$OUT/Uncaged.app" "dist/Uncaged-macos-${{ matrix.arch }}.zip""#
    ));
    assert!(RELEASE_WORKFLOW
        .contains(r#"cp "$OUT/Uncaged.dmg" "dist/Uncaged-macos-${{ matrix.arch }}.dmg""#));
    assert!(MACOS_BUNDLE.contains(r#"WARP_APP_NAME="Uncaged""#));
}

/// The bundle keeps the pre-rename executable and CLI names as symlinks to the
/// renamed executable, and ships the CLI updater the next update will run.
#[test]
fn macos_bundle_keeps_compat_entries_and_the_cli_updater() {
    assert!(MACOS_BUNDLE.contains(r#"WARP_BIN="uncaged""#));
    assert!(MACOS_BUNDLE.contains(r#"BUNDLE_ID="dev.uncaged.Uncaged""#));
    assert!(MACOS_BUNDLE.contains(
        r#"ln -sfn "$WARP_BIN" "$BUNDLE_DIR/$WARP_APP_NAME.app/Contents/MacOS/warp-oss""#
    ));
    assert!(MACOS_BUNDLE.contains(r#"ln -sfn "uncaged" "$BUNDLED_RESOURCES_DIR/bin/warp-oss""#));
    assert!(MACOS_BUNDLE.contains(r#""$BUNDLED_RESOURCES_DIR/bin/uncaged-update""#));
    // The bundle's executable comes from the cargo binary of the same name.
    assert!(APP_MANIFEST.contains("[[bin]]\nname = \"uncaged\"\npath = \"src/bin/oss.rs\""));
    assert!(APP_MANIFEST.contains("[package.metadata.bundle.bin.uncaged]"));
}

/// The script v0.3.4 spawns waits on the bundle directory (so on every
/// executable name), then reinstalls through the cask or install.sh.
#[test]
fn cli_updater_waits_on_every_executable_and_reinstalls_the_same_bundle() {
    assert!(CLI_UPDATER.contains(r#"APP_PATH="/Applications/Uncaged.app""#));
    assert!(CLI_UPDATER.contains(r#"pgrep -f "$APP_PATH/Contents/MacOS/""#));
    assert!(CLI_UPDATER.contains(r#"CASK_TOKEN="uncaged""#));
    assert!(CLI_UPDATER.contains(r#"INSTALLER_URL="https://getuncaged.dev/install.sh""#));
}

#[cfg(target_os = "macos")]
#[test]
fn macos_updater_accepts_bundles_from_both_sides_of_the_rename() {
    use uncaged_core::channel::Channel;

    assert_eq!(super::mac::app_name(Channel::Oss), "Uncaged.app");
    assert_eq!(
        super::mac::executable_names(Channel::Oss),
        &["uncaged", "warp-oss"]
    );
}

/// Linux packages keep the package name v0.3.4 installed, so they upgrade it
/// in place, and keep the old binary name as a symlink.
#[test]
fn linux_packages_keep_their_name_and_the_old_binary_name() {
    assert!(LINUX_BUNDLE.contains(r#"BINARY_NAME="uncaged""#));
    assert!(LINUX_BUNDLE.contains(r#"PACKAGE_CHANNEL_SUFFIX="-oss""#));
    assert!(
        LINUX_BUNDLE_DEB.contains(r#"CHANNEL_SUFFIX="${PACKAGE_CHANNEL_SUFFIX:-$CHANNEL_SUFFIX}""#)
    );
    assert!(LINUX_BUNDLE_DEB.contains(r#"PACKAGE_NAME="warp-terminal$CHANNEL_SUFFIX""#));
    assert!(DEB_CONTROL.starts_with("Package: warp-terminal@@CHANNEL_SUFFIX@@"));
    assert!(LINUX_BUNDLE_INSTALL
        .contains(r#"ln -sfn "$BINARY_NAME" "${TARGET_DIR}${OPT_DIR}/warp-oss""#));
    // The tarball's binary was already named `uncaged` in v0.3.4.
    assert!(RELEASE_WORKFLOW
        .contains("cp target/release-lto/uncaged stage/Uncaged-linux-x86_64/uncaged"));
}

/// The Inno Setup AppId decides whether a new installer upgrades the existing
/// install; it must never change. The single-instance mutex it waits on is
/// keyed by the channel, not the app id.
#[test]
fn windows_installer_upgrades_the_existing_install() {
    assert!(WINDOWS_INSTALLER.contains("AppId=warp-terminal-{#ReleaseChannel}"));
    assert!(WINDOWS_INSTALLER
        .contains(r#"#define AppMutexName "Local\Warp" + ChannelPascalCase + "_SingleInstance""#));
    assert!(WINDOWS_INSTALLER.contains(r#"Type: files; Name: "{app}\warp-oss.exe""#));
    assert!(WINDOWS_INSTALLER.contains(r"SaveStringToFile(BinDir + '\warp-oss.cmd'"));
}
