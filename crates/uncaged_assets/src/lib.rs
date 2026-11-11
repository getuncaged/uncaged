use std::borrow::Cow;

use anyhow::{Result, anyhow};
use rust_embed::RustEmbed;
use uncaged_ui_core::AssetProvider;

/// Assets embedded in the binary.
///
/// The large onboarding/theme imagery under `app/assets/async/` is deliberately
/// not embedded: it is 36 MB, the bulk of the binary. It ships as plain files in
/// the app bundle's resources directory instead and is read through
/// [`Assets::get`] (see [`async_asset_bytes`]). The WASM build never had these
/// (it fetches them over the network), and standalone CLI builds are headless.
#[derive(Clone, Copy, RustEmbed)]
#[folder = "../../app/assets"]
#[include = "bundled/**"] // Should be kept in sync with BUNDLED_ASSETS_DIR.
pub struct Assets;

/// Prefix of the asset paths served from the resources directory rather than
/// from the binary. Should be kept in sync with ASYNC_ASSETS_DIR.
#[cfg(not(target_family = "wasm"))]
const ASYNC_ASSETS_PREFIX: &str = "async/";

impl AssetProvider for Assets {
    fn get(&self, path: &str) -> Result<Cow<'_, [u8]>> {
        #[cfg(not(target_family = "wasm"))]
        {
            if path.starts_with(ASYNC_ASSETS_PREFIX) {
                return async_asset_bytes(path).map(Cow::Owned);
            }
        }

        <Assets as RustEmbed>::get(path)
            .map(|f| f.data)
            .ok_or_else(|| anyhow!("no asset exists at path {}", path))
    }
}

/// Reads an `async/...` asset from disk.
///
/// Looks in the bundle's resources directory first (`Contents/Resources` on
/// macOS, `resources/` next to the executable on Linux and Windows), where the
/// bundle scripts install `async/`. Builds that aren't bundled (`cargo run`)
/// fall back to the source tree's `app/assets`, which is where `cargo run`
/// previously read these files from too.
#[cfg(not(target_family = "wasm"))]
fn async_asset_bytes(path: &str) -> Result<Vec<u8>> {
    use std::path::{Component, Path, PathBuf};

    // Asset paths are compile-time literals, but never let one escape the
    // resources directory.
    if Path::new(path)
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(anyhow!("invalid asset path {path}"));
    }

    let bundled = uncaged_core::paths::bundled_resources_dir().map(|dir| dir.join(path));
    let source_tree = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../app/assets")
        .join(path);

    let mut last_error = None;
    for candidate in bundled.into_iter().chain([source_tree]) {
        match std::fs::read(&candidate) {
            Ok(bytes) => return Ok(bytes),
            Err(err) => last_error = Some((candidate, err)),
        }
    }
    match last_error {
        Some((candidate, err)) => Err(anyhow!(
            "no asset exists at path {path} (tried {}): {err}",
            candidate.display()
        )),
        None => Err(anyhow!("no asset exists at path {path}")),
    }
}
