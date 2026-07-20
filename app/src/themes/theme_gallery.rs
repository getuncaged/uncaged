//! The catalogue of themes available to install from the community gallery.
//!
//! The gallery lives in the public `getuncaged/uncaged-themes` repository. Rather than crawling
//! the GitHub API — several requests per open, and rate limited for anyone unauthenticated — the
//! app fetches a single generated `index.json` from it.
//!
//! Each entry carries the theme's whole definition inline, so a card can be rendered and an
//! image-less theme installed without any further requests. Only a background image needs
//! fetching separately, and only at the moment someone installs the theme.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context as _, Result};
use serde::Deserialize;

use crate::themes::theme::{ThemeGroup, COMMUNITY_SUBFOLDER};
use warp_core::ui::theme::WarpTheme;

/// The branch the gallery reads from.
///
/// Overridable with `UNCAGED_THEME_GALLERY_REF` so a fork, or a pull request that has not landed
/// yet, can be pointed at without rebuilding — which is also how the gallery gets verified before
/// its catalogue is published to `main`.
pub fn gallery_ref() -> String {
    std::env::var("UNCAGED_THEME_GALLERY_REF").unwrap_or_else(|_| "main".to_owned())
}

/// Where the catalogue is fetched from.
///
/// Served by raw.githubusercontent rather than the API so it needs no token and no rate-limit
/// handling, and is edge-cached.
pub fn index_url() -> String {
    format!("{}/index.json", raw_base_url())
}

/// Base for anything else in the repo, such as a theme's background image.
pub fn raw_base_url() -> String {
    format!(
        "https://raw.githubusercontent.com/getuncaged/uncaged-themes/{}",
        gallery_ref()
    )
}

/// The shape this client understands. A future breaking change to the catalogue bumps this, and an
/// older client refuses it politely instead of misreading the contents.
pub const SUPPORTED_VERSION: u32 = 1;

/// A catalogue is fetched over the network, so it is bounded before parsing rather than trusted.
/// Five themes come to about 5KB, leaving room for a few thousand.
pub const MAX_INDEX_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize)]
pub struct GalleryIndex {
    pub version: u32,
    pub themes: Vec<GalleryTheme>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GalleryTheme {
    /// Stable identifier, `"<group>/<slug>"`, e.g. `"community/tokyo-rain"`.
    pub id: String,
    /// File stem, used as the installed file name.
    pub slug: String,
    /// Display name, as it appears in the picker.
    pub name: String,
    /// `"system"` or `"community"`, matching the folder it lives in.
    pub group: String,
    /// Path to the theme's YAML within the repo.
    pub path: String,
    /// Path to the background image within the repo, when the theme has one.
    #[serde(default)]
    pub image: Option<String>,
    /// The theme itself, inline — enough to render a preview card and to install, without a
    /// second request.
    pub definition: WarpTheme,
}

impl GalleryTheme {
    /// Which section of the picker this theme belongs to once installed.
    ///
    /// Everything installed from the gallery is [`ThemeGroup::Community`] regardless of the folder
    /// it came from: a theme the team wrote is still, from this machine's point of view, something
    /// that was fetched rather than something that shipped. Themes that ship are already present
    /// and are filtered out of the gallery entirely.
    pub fn installed_group(&self) -> ThemeGroup {
        ThemeGroup::Community
    }

    /// URL of this theme's background image, if it has one.
    pub fn image_url(&self) -> Option<String> {
        self.image
            .as_ref()
            .map(|path| format!("{}/{path}", raw_base_url()))
    }

    /// Does the search box's text match this theme?
    pub fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let query = query.to_lowercase();
        self.name.to_lowercase().contains(&query) || self.slug.to_lowercase().contains(&query)
    }
}

/// Parses a fetched catalogue, rejecting a version this client does not understand.
pub fn parse_index(bytes: &[u8]) -> anyhow::Result<GalleryIndex> {
    if bytes.len() > MAX_INDEX_BYTES {
        anyhow::bail!("theme catalogue is unreasonably large ({} bytes)", bytes.len());
    }

    let index: GalleryIndex = serde_json::from_slice(bytes)?;

    if index.version != SUPPORTED_VERSION {
        anyhow::bail!(
            "theme catalogue is version {}, but this version of Uncaged understands {}",
            index.version,
            SUPPORTED_VERSION,
        );
    }

    Ok(index)
}

// ── Fetching and installing ──────────────────────────────────────────────────

/// How long any single gallery request is allowed to take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// A background image is bounded before it is written to disk. The largest bundled theme image is
/// about 1MB, and the editor caps its own imports well below this.
const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;

/// The client used for every gallery request.
///
/// Deliberately a plain `reqwest::Client` rather than `http_client::Client`. That wrapper sets
/// `include_warp_http_headers` unconditionally on native, attaching `X-Warp-Client-Id`,
/// `X-Warp-Client-Version` and OS details to every request — fine for our own services, but this
/// talks to GitHub, and an account-free terminal has no business handing a third party a stable
/// client identifier just to download a colour scheme. It also sets no user agent, which GitHub's
/// API rejects.
fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(concat!("Uncaged/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("couldn't start a network client")
}

/// Downloads the catalogue of installable themes.
pub async fn fetch_index() -> Result<GalleryIndex> {
    let response = client()?
        .get(index_url())
        .send()
        .await
        .context("Couldn't reach the theme gallery — check your connection.")?;

    // A missing catalogue and an unreachable network are different problems and deserve different
    // messages: one means the gallery hasn't been published, the other means this machine is
    // offline. Reporting both as "unavailable" sends people looking in the wrong place.
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        bail!("The theme gallery hasn't been published yet.");
    }

    let response = response
        .error_for_status()
        .context("The theme gallery is temporarily unavailable.")?;

    let bytes = response
        .bytes()
        .await
        .context("Couldn't read the theme gallery.")?;

    parse_index(&bytes)
}

/// Installs `theme` into `<themes_dir>/community/`, returning the path of the written YAML.
///
/// Both the theme and its image land in the same folder, and the stored image path is made
/// absolute on the way in. Relative paths in a theme file are resolved against the themes dir
/// itself rather than the file's own folder, so a relative path written here would be looked up
/// one directory too high and the background would silently not load.
pub async fn install(theme: &GalleryTheme, themes_dir: &Path) -> Result<PathBuf> {
    let dir = themes_dir.join(COMMUNITY_SUBFOLDER);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("couldn't create {}", dir.display()))?;

    let mut definition = serde_yaml::to_value(&theme.definition)
        .context("couldn't prepare the theme for saving")?;

    if let Some(url) = theme.image_url() {
        let extension = Path::new(&url)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("jpg")
            .to_owned();
        let image_path = dir.join(format!("{}.{extension}", theme.slug));

        let bytes = fetch_image(&url).await?;
        std::fs::write(&image_path, &bytes)
            .with_context(|| format!("couldn't save {}", image_path.display()))?;

        set_image_path(&mut definition, &image_path);
    }

    let yaml = serde_yaml::to_string(&definition).context("couldn't serialize the theme")?;
    let theme_path = dir.join(format!("{}.yaml", theme.slug));
    std::fs::write(&theme_path, yaml)
        .with_context(|| format!("couldn't save {}", theme_path.display()))?;

    Ok(theme_path)
}

/// Downloads a background image, refusing anything implausibly large before reading it.
async fn fetch_image(url: &str) -> Result<Vec<u8>> {
    let response = client()?
        .get(url)
        .send()
        .await
        .context("couldn't download the theme's background image")?
        .error_for_status()
        .context("the theme's background image is missing from the gallery")?;

    // Trust the declared length only to reject early; the real bound is on what is read.
    if let Some(length) = response.content_length() {
        if length > MAX_IMAGE_BYTES {
            bail!("that theme's background image is unreasonably large ({length} bytes)");
        }
    }

    let bytes = response
        .bytes()
        .await
        .context("couldn't read the theme's background image")?;

    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        bail!(
            "that theme's background image is unreasonably large ({} bytes)",
            bytes.len()
        );
    }

    Ok(bytes.to_vec())
}

/// Points a serialized theme's `background_image.path` at `path`.
fn set_image_path(definition: &mut serde_yaml::Value, path: &Path) {
    let Some(image) = definition.get_mut("background_image") else {
        return;
    };
    if let Some(map) = image.as_mapping_mut() {
        map.insert(
            serde_yaml::Value::from("path"),
            serde_yaml::Value::from(path.to_string_lossy().into_owned()),
        );
    }
}

#[cfg(test)]
#[path = "theme_gallery_tests.rs"]
mod tests;
