//! Uncaged: GitHub Releases as the update source.
//!
//! Warp asks its own server which version is current and derives the download
//! URL from the answer, trusting the result because the bundle carries a
//! Developer-ID signature. Uncaged can do neither. Its releases live on GitHub,
//! and it is ad-hoc signed — `codesign -R <team>` has no team to pin, so it
//! cannot tell a real build from a substituted one.
//!
//! What replaces the signature is the release API's per-asset `digest`. GitHub
//! reports it over TLS alongside the download URL, so the hash arrives by a
//! different response than the bytes it describes, and the installer verifies
//! the download against it before doing anything with the file.
//!
//! Be precise about what that buys: the digest proves the bytes are the bytes
//! GitHub stores. It says nothing about who published them. With no signing
//! identity, publish access to the repository is the whole security perimeter —
//! documented in SECURITY.md rather than papered over here.
//!
//! Nothing in this module runs before the user opts in; see
//! `autoupdate::uncaged_updates_consented`.

use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context as _, Result};
use futures_lite::{AsyncReadExt as _, AsyncWriteExt as _, StreamExt as _};
use sha2::{Digest as _, Sha256};

use channel_versions::{ReleaseAsset, VersionInfo};
use serde::Deserialize;

use crate::brand;

/// The release JSON is small (v0.2.9 is ~9 KB with 15 assets). Anything far
/// larger is not a release we understand, and we would rather refuse than parse
/// an unbounded body.
const MAX_RELEASE_JSON_BYTES: u64 = 1024 * 1024;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// Downloads are big (the macOS DMG is ~150 MB) and slow links are real.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(900);

/// A hard ceiling on what we will write to disk for an update. Our largest
/// artifact is around 160 MB; anything past this is not one of ours.
const MAX_ASSET_BYTES: u64 = 512 * 1024 * 1024;

/// The subset of GitHub's release object we rely on.
#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
    /// `"sha256:<64 hex>"`. Absent on very old releases, in which case we refuse
    /// the asset rather than fall back to trusting the download.
    #[serde(default)]
    digest: Option<String>,
    /// GitHub reports `"uploaded"` once an asset is complete. A release can be
    /// visible while its assets are still arriving.
    #[serde(default)]
    state: String,
}

/// A plain `reqwest::Client`, deliberately not `http_client::Client` — that
/// wrapper attaches Warp client headers (including an install identifier),
/// which must never be sent anywhere from this build.
///
/// The redirect policy is part of the security boundary: a release response
/// names its own download URL, and `browser_download_url` redirects to an
/// object host, so every hop is checked against the allowed GitHub hosts. Without
/// this, anyone able to alter the response chooses where we download from.
fn client() -> Result<reqwest::Client> {
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 5 {
            return attempt.error("too many redirects");
        }
        match host_is_allowed(attempt.url()) {
            true => attempt.follow(),
            false => attempt.stop(),
        }
    });

    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(brand::HTTP_USER_AGENT)
        .redirect(policy)
        .build()
        .context("building the release HTTP client")
}

/// Whether a URL is one we will talk to: HTTPS, on a GitHub host.
pub fn host_is_allowed(url: &reqwest::Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    brand::RELEASE_DOWNLOAD_HOSTS
        .iter()
        .any(|allowed| host == *allowed)
}

/// Fetches the newest published release and returns it as a [`VersionInfo`]
/// carrying the assets for `target` (an asset-name fragment such as
/// `"macos-aarch64"`).
///
/// Returns `Ok(None)` when the release is a draft or prerelease, or has no asset
/// for this platform — all ordinary states that mean "nothing to offer", not
/// failures worth surfacing to the user.
pub async fn fetch_latest_release(target: &str) -> Result<Option<VersionInfo>> {
    let client = client()?;
    let response = client
        .get(brand::LATEST_RELEASE_API_URL)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .context("requesting the latest release")?;

    if !response.status().is_success() {
        bail!("release API returned {}", response.status());
    }

    if let Some(length) = response.content_length() {
        if length > MAX_RELEASE_JSON_BYTES {
            bail!("release JSON is {length} bytes, refusing to parse");
        }
    }

    let body = response.text().await.context("reading the release body")?;
    if body.len() as u64 > MAX_RELEASE_JSON_BYTES {
        bail!("release JSON is {} bytes, refusing to parse", body.len());
    }

    let release: GhRelease = serde_json::from_str(&body).context("parsing the release JSON")?;
    Ok(version_info_from_release(release, target))
}

/// Maps a release onto a [`VersionInfo`], keeping only assets for `target`.
///
/// Split out from the request so it can be tested against real payloads without
/// touching the network.
fn version_info_from_release(release: GhRelease, target: &str) -> Option<VersionInfo> {
    if release.draft || release.prerelease {
        log::info!(
            "Ignoring release {}: draft={} prerelease={}",
            release.tag_name,
            release.draft,
            release.prerelease
        );
        return None;
    }

    let assets: Vec<ReleaseAsset> = release
        .assets
        .into_iter()
        .filter(|a| a.name.contains(target))
        .filter_map(|a| {
            // An asset still uploading is not one we can hash.
            if !a.state.is_empty() && a.state != "uploaded" {
                log::warn!("Skipping asset {} in state {}", a.name, a.state);
                return None;
            }
            // No digest, no install. There is no safe fallback: without a hash
            // published separately from the bytes, we would be trusting the
            // download to vouch for itself.
            let sha256 = match a.digest.as_deref().and_then(parse_sha256_digest) {
                Some(hash) => hash,
                None => {
                    log::warn!("Skipping asset {}: no usable sha256 digest", a.name);
                    return None;
                }
            };
            Some(ReleaseAsset {
                name: a.name,
                url: a.browser_download_url,
                size: a.size,
                sha256,
            })
        })
        .collect();

    if assets.is_empty() {
        log::info!(
            "Release {} has no verifiable asset for {target}",
            release.tag_name
        );
        return None;
    }

    let mut info = VersionInfo::new(release.tag_name);
    info.assets = assets;
    Some(info)
}

/// Extracts the hex from `"sha256:<64 hex>"`, rejecting any other algorithm or
/// shape. Lowercased so comparisons are a plain string equality.
fn parse_sha256_digest(digest: &str) -> Option<String> {
    let hex = digest.strip_prefix("sha256:")?;
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(hex.to_ascii_lowercase())
}

/// Downloads `asset` to `dest`, then verifies the file on disk against the
/// SHA-256 the release API reported.
///
/// Two deliberate choices.
///
/// The hash is computed from the file we just wrote, not from the bytes as they
/// stream past. What matters is that the artifact we are about to mount and
/// install is the right one; hashing the stream would leave a window in which
/// the file changed between the check and the use, and would also miss a bad
/// write.
///
/// The URL is re-validated here even though it was validated when the release
/// was parsed, because this is the last point before we fetch it. Host checks
/// are cheap and the cost of getting one wrong is executing someone else's code.
///
/// On any mismatch the partial file is removed. A half-verified download left on
/// disk is exactly the kind of thing a later "we already have it" optimisation
/// would pick up.
pub async fn download_and_verify(asset: &ReleaseAsset, dest: &Path) -> Result<()> {
    let url = reqwest::Url::parse(&asset.url)
        .with_context(|| format!("release asset URL is not a URL: {}", asset.url))?;
    if !host_is_allowed(&url) {
        bail!("refusing to download from {url}: not an https GitHub host");
    }

    let client = client()?;
    let response = client
        .get(url.clone())
        .timeout(DOWNLOAD_TIMEOUT)
        .send()
        .await
        .with_context(|| format!("downloading {}", asset.name))?
        .error_for_status()
        .with_context(|| format!("downloading {}", asset.name))?;

    // GitHub redirects to an object host; confirm we did not get walked
    // somewhere else, since a redirect chain is only as good as its last hop.
    if !host_is_allowed(response.url()) {
        bail!(
            "download ended at {}, which is not a GitHub host",
            response.url()
        );
    }

    if let Some(length) = response.content_length() {
        if length > MAX_ASSET_BYTES {
            bail!("{} is {length} bytes, refusing to download", asset.name);
        }
    }

    let mut file = async_fs::File::create(dest)
        .await
        .with_context(|| format!("creating {}", dest.display()))?;
    let mut stream = response.bytes_stream();
    let mut written: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("reading the download")?;
        written += chunk.len() as u64;
        if written > MAX_ASSET_BYTES {
            drop(file);
            let _ = async_fs::remove_file(dest).await;
            bail!(
                "{} exceeded {MAX_ASSET_BYTES} bytes mid-download",
                asset.name
            );
        }
        file.write_all(&chunk)
            .await
            .with_context(|| format!("writing {}", dest.display()))?;
    }
    file.flush().await.context("flushing the download")?;
    drop(file);

    match verify_sha256(dest, &asset.sha256).await {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = async_fs::remove_file(dest).await;
            Err(err)
        }
    }
}

/// Hashes the file at `path` and compares it to `expected` (lowercase hex).
///
/// This is the check that stands in for a code signature. If it does not hold,
/// nothing downstream should touch the file.
pub async fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    let mut file = async_fs::File::open(path)
        .await
        .with_context(|| format!("opening {} to hash it", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buf).await.context("reading to hash")?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        bail!(
            "checksum mismatch for {}: release says {expected}, downloaded file is {actual}",
            path.display()
        );
    }
    log::info!("Verified {} against the release checksum", path.display());
    Ok(())
}

/// The asset-name fragment identifying the running platform, e.g.
/// `"macos-aarch64"`. `None` on a platform we do not publish builds for.
pub fn current_target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("macos-aarch64"),
        ("macos", "x86_64") => Some("macos-x86_64"),
        ("windows", "aarch64") => Some("windows-aarch64"),
        ("windows", "x86_64") => Some("windows-x86_64"),
        ("linux", "aarch64") => Some("linux-aarch64"),
        ("linux", "x86_64") => Some("linux-x86_64"),
        _ => None,
    }
}

#[cfg(test)]
#[path = "github_releases_tests.rs"]
mod tests;
