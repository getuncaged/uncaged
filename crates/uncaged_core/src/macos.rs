use anyhow::Result;
use objc2_foundation::NSBundle;

/// Apple Developer Team ID used for code signing and validation.
///
/// Uncaged is ad-hoc signed and has no Apple Developer Team, so this is empty.
/// It deliberately does **not** carry upstream Warp's team identifier: that is
/// another company's signing identity, and embedding it in a binary we
/// distribute under our own name is both wrong and pointless.
///
/// Nothing on this channel reads it for a real decision:
/// * `paths::shared_container_path` returns `None` for `Channel::Oss` before it
///   builds an app-group id, so callers fall back to `~/.uncaged`.
/// * `autoupdate` uses it to pin a signing team when verifying a staged update,
///   and autoupdate is `None` on this channel (updates are manual, from GitHub
///   Releases).
///
/// If you fork Uncaged and ship notarized builds, put your own Team ID here.
pub const APPLE_TEAM_ID: &str = "";

/// Get the path to the macOS `.app` bundle.
pub fn get_bundle_path() -> Result<String> {
    let bundle = NSBundle::mainBundle();
    let path = bundle.bundlePath();
    Ok(path.to_string())
}
