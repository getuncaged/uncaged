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
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context as _, Result};
use serde::Deserialize;
use warp_core::ui::theme::WarpTheme;
use warpui::{Entity, ModelContext, SingletonEntity};

use crate::brand;
use crate::themes::theme::{ThemeGroup, COMMUNITY_SUBFOLDER};
use crate::themes::theme_background_image;

/// The branch the gallery reads from.
///
/// Overridable via [`brand::THEME_GALLERY_REF_ENV`] so a fork, or a pull request that has not
/// landed yet, can be pointed at without rebuilding — which is also how the gallery gets verified
/// before its catalogue is published to `main`.
pub fn gallery_ref() -> String {
    std::env::var(brand::THEME_GALLERY_REF_ENV).unwrap_or_else(|_| "main".to_owned())
}

/// Where the catalogue is fetched from.
///
/// Served by raw.githubusercontent rather than the API so it needs no token and no rate-limit
/// handling, and is edge-cached.
pub fn index_url() -> String {
    format!("{}/index.json", raw_base_url())
}

/// Base for anything else in the repo, such as a theme's background image. The repo and host come
/// from [`brand`]; only the branch varies at runtime.
pub fn raw_base_url() -> String {
    format!("{}/{}", brand::THEMES_RAW_BASE, gallery_ref())
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

/// Whether a catalogue slug is safe to use as a filename.
///
/// It must be a single, boring path component: no separators, no `..`, no absolute prefix, nothing
/// that could redirect a `join` out of the themes dir. Anything else is refused rather than
/// sanitised, so a malformed catalogue entry fails loudly instead of installing somewhere strange.
pub fn is_safe_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 128
        && slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Parses a fetched catalogue, rejecting a version this client does not understand.
pub fn parse_index(bytes: &[u8]) -> anyhow::Result<GalleryIndex> {
    if bytes.len() > MAX_INDEX_BYTES {
        anyhow::bail!(
            "theme catalogue is unreasonably large ({} bytes)",
            bytes.len()
        );
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
/// A plain `reqwest::Client` rather than `http_client::Client`, so gallery requests carry their
/// own timeout. It sets the generic product user agent, which GitHub's API requires, and nothing
/// that identifies the install.
fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(brand::HTTP_USER_AGENT)
        .build()
        .context("couldn't start a network client")
}

/// Downloads the catalogue of installable themes and keeps a copy on disk, so the next visit —
/// and a visit without a network — can show it straight away.
pub async fn fetch_index() -> Result<GalleryIndex> {
    let bytes = fetch_index_bytes().await?;
    let index = parse_index(&bytes)?;
    // Best-effort: a catalogue that can't be cached still works for this run.
    if let Err(error) = write_cached_index(&bytes) {
        log::warn!("couldn't cache the theme catalogue: {error:#}");
    }
    Ok(index)
}

/// Where the last good catalogue is kept between runs.
pub fn cached_index_path() -> PathBuf {
    warp_core::paths::cache_dir()
        .join("theme-gallery")
        .join("index.json")
}

/// Reads the catalogue saved by the last successful fetch, if there is one and it still parses.
pub fn read_cached_index() -> Result<GalleryIndex> {
    let path = cached_index_path();
    let bytes =
        std::fs::read(&path).with_context(|| format!("couldn't read {}", path.display()))?;
    parse_index(&bytes)
}

fn write_cached_index(bytes: &[u8]) -> Result<()> {
    let path = cached_index_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("couldn't create {}", dir.display()))?;
    }
    // Written beside and renamed over, so a crash mid-write never leaves a half catalogue that
    // the next run would refuse.
    let partial = path.with_extension("json.partial");
    std::fs::write(&partial, bytes)
        .with_context(|| format!("couldn't write {}", partial.display()))?;
    std::fs::rename(&partial, &path).with_context(|| format!("couldn't replace {}", path.display()))
}

async fn fetch_index_bytes() -> Result<Vec<u8>> {
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

    // Refuse a catalogue that announces itself as oversized before reading it into memory. The
    // post-read cap in parse_index is the real guarantee, but this avoids buffering a body that a
    // hostile or misconfigured host claims is enormous.
    if let Some(length) = response.content_length() {
        if length > MAX_INDEX_BYTES as u64 {
            bail!("the theme gallery is unreasonably large ({length} bytes)");
        }
    }

    let bytes = response
        .bytes()
        .await
        .context("Couldn't read the theme gallery.")?;

    Ok(bytes.to_vec())
}

/// Installs `theme` into `<themes_dir>/community/`, returning the path of the written YAML.
///
/// Both the theme and its image land in the same folder, and the stored image path is made
/// absolute on the way in. Relative paths in a theme file are resolved against the themes dir
/// itself rather than the file's own folder, so a relative path written here would be looked up
/// one directory too high and the background would silently not load.
pub async fn install(theme: &GalleryTheme, themes_dir: &Path) -> Result<PathBuf> {
    // The slug comes straight from the downloaded catalogue and becomes a filename. The catalogue
    // is community-editable, so this is a trust boundary: a slug containing `..`, a path separator,
    // or an absolute prefix would make the joins below escape the themes dir and write elsewhere on
    // disk. Refuse anything that isn't a plain, single path component before touching the
    // filesystem.
    if !is_safe_slug(&theme.slug) {
        bail!("this theme has an unsafe name and won't be installed");
    }

    let dir = themes_dir.join(COMMUNITY_SUBFOLDER);
    std::fs::create_dir_all(&dir).with_context(|| format!("couldn't create {}", dir.display()))?;

    let mut definition =
        serde_yaml::to_value(&theme.definition).context("couldn't prepare the theme for saving")?;

    if let Some(url) = theme.image_url() {
        // Normalise the download exactly like an editor import: cap it, flatten it, re-encode it,
        // and write a preview thumbnail alongside. A community image is only bounded to a file size
        // on the wire, so without this a theme could ship a 4K wallpaper that every card decodes.
        let bytes = fetch_image(&url).await?;
        let image_path = theme_background_image::import_bytes(&bytes, &dir, &theme.slug)
            .context("couldn't process the theme's background image")?;

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

// ── The shared catalogue ─────────────────────────────────────────────────────

/// The community catalogue, shared by every window.
///
/// Nothing is read or fetched until the gallery is first shown ([`Self::ensure_loaded`]); building
/// Settings at startup costs no network. On that first showing the copy saved by the last
/// successful fetch is shown at once, and the network is asked for a fresh one at most once per
/// run — every window then gets the same answer through [`ThemeGalleryCatalogueEvent::Updated`].
#[derive(Default)]
pub struct ThemeGalleryCatalogue {
    themes: Option<Arc<Vec<GalleryTheme>>>,
    /// Whether `themes` came from the network during this run, rather than from the disk cache.
    fresh: bool,
    fetching: bool,
    /// Why the last fetch failed, until the next one starts.
    error: Option<String>,
    started: bool,
}

pub enum ThemeGalleryCatalogueEvent {
    Updated,
}

impl ThemeGalleryCatalogue {
    pub fn new() -> Self {
        Self::default()
    }

    /// The catalogue to show: fresh if this run has fetched one, otherwise the saved copy.
    pub fn themes(&self) -> Option<&Arc<Vec<GalleryTheme>>> {
        self.themes.as_ref()
    }

    /// True while there is nothing to show yet and a fetch is under way.
    pub fn is_loading(&self) -> bool {
        self.themes.is_none() && (self.fetching || !self.started)
    }

    /// Why the last fetch failed, if it did.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// True when what is shown is the saved copy because this run's fetch failed.
    pub fn is_stale(&self) -> bool {
        self.themes.is_some() && !self.fresh && self.error.is_some()
    }

    /// Shows the saved catalogue and starts this run's one refresh, the first time it is called.
    pub fn ensure_loaded(&mut self, ctx: &mut ModelContext<Self>) {
        if self.started {
            return;
        }
        self.started = true;

        ctx.spawn(
            async move { read_cached_index().map(|index| index.themes) },
            |me, cached, ctx| {
                // A fetch that finished first has the newer list; the saved copy never replaces it.
                if me.fresh {
                    return;
                }
                if let Ok(themes) = cached {
                    me.themes = Some(Arc::new(themes));
                    ctx.emit(ThemeGalleryCatalogueEvent::Updated);
                }
            },
        );
        self.refresh(ctx);
    }

    /// Asks the network for the catalogue now — "Try again" after a failure.
    pub fn refresh(&mut self, ctx: &mut ModelContext<Self>) {
        if self.fetching {
            return;
        }
        self.started = true;
        self.fetching = true;
        self.error = None;
        ctx.emit(ThemeGalleryCatalogueEvent::Updated);

        ctx.spawn(
            async move { fetch_index().await.map_err(|e| e.to_string()) },
            |me, result, ctx| {
                me.fetching = false;
                match result {
                    Ok(index) => {
                        me.themes = Some(Arc::new(index.themes));
                        me.fresh = true;
                    }
                    Err(message) => me.error = Some(message),
                }
                ctx.emit(ThemeGalleryCatalogueEvent::Updated);
            },
        );
    }
}

impl Entity for ThemeGalleryCatalogue {
    type Event = ThemeGalleryCatalogueEvent;
}

impl SingletonEntity for ThemeGalleryCatalogue {}

#[cfg(test)]
#[path = "theme_gallery_tests.rs"]
mod tests;
