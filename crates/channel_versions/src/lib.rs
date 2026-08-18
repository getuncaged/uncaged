pub mod overrides;

use std::collections::HashMap;
use std::fmt::Write;

use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset, NaiveDateTime};
use lazy_static::lazy_static;
use memo_map::MemoMap;
use overrides::*;
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct ChannelVersions {
    pub dev: ChannelVersion,
    pub preview: ChannelVersion,
    pub stable: ChannelVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changelogs: Option<ChannelChangelogs>,
}

impl std::fmt::Display for ChannelVersions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "dev: {:?}; preview: {:?}; stable: {:?}",
            self.dev, self.preview, self.stable
        )
    }
}

lazy_static! {
    static ref VERSION_RE: Regex = Regex::new(r"v(\d+)\.(.+)\.(.+)_(\d+)").unwrap();

    // Cached mapping of version strings to semantic versions.
    static ref PARSED_VERSIONS_CACHE: MemoMap<String, ParsedVersion> = Default::default();
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Clone)]
pub struct ParsedVersion {
    major: usize,
    date: NaiveDateTime,
    patch: usize,
}

impl TryFrom<&str> for ParsedVersion {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self> {
        PARSED_VERSIONS_CACHE
            .get_or_try_insert(value, || {
                VERSION_RE
                    .captures(value)
                    .and_then(|captures| {
                        let date_str = captures.get(2)?.as_str();
                        let date =
                            NaiveDateTime::parse_from_str(date_str, "%Y.%m.%d.%H.%M").ok()?;
                        Some(ParsedVersion {
                            major: captures.get(1)?.as_str().parse().ok()?,
                            date,
                            patch: captures.get(4)?.as_str().parse().ok()?,
                        })
                    })
                    .context("Can't parse string into Version")
            })
            .cloned()
    }
}

impl Ord for ParsedVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.major, self.date, self.patch).cmp(&(other.major, other.date, other.patch))
    }
}

impl PartialOrd for ParsedVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ChannelVersion {
    #[serde(flatten)]
    version_info: VersionInfo,
    /// Any overrides which should be applied for this channel.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    overrides: Vec<VersionOverride>,
}

impl ChannelVersion {
    pub fn new(version_info: VersionInfo) -> Self {
        Self {
            version_info,
            overrides: vec![],
        }
    }

    /// Returns the version information, with any applicable overrides applied
    /// based on the current execution environment.
    pub fn version_info(&self) -> VersionInfo {
        let context = overrides::Context::from_env();
        self.version_info
            .with_overrides_applied(&self.overrides, &context)
    }

    /// Returns the version information, with any applicable overrides applied
    /// based on the provided context.
    pub fn version_info_for_execution_context(&self, context: &overrides::Context) -> VersionInfo {
        self.version_info
            .with_overrides_applied(&self.overrides, context)
    }
}

/// A plain `vMAJOR.MINOR.PATCH` release tag, as Uncaged tags them.
///
/// [`ParsedVersion`] cannot help here: its regex is `v(\d+)\.(.+)\.(.+)_(\d+)`,
/// which requires Warp's `_NN` build-number suffix and a `%Y.%m.%d.%H.%M`
/// datetime in the middle, so `v0.2.9` simply does not match. The caller that
/// guards against downgrades swallows the parse error (`if let Ok(true)`), which
/// means that on Uncaged the guard silently never fires — and a release
/// re-published, a moved tag, or a bad `make_latest` would walk every client
/// backwards. This type is the comparison that actually works for our tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct UncagedVersion {
    major: u64,
    minor: u64,
    patch: u64,
}

impl UncagedVersion {
    /// Parses `v1.2.3` or `1.2.3`. Anything else — a suffix, a missing
    /// component, a non-numeric part — is rejected rather than guessed at.
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.strip_prefix('v').unwrap_or(value);
        let mut parts = value.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        // Reject trailing components so "1.2.3.4" is not silently read as 1.2.3.
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
        })
    }

    /// Whether `candidate` is strictly newer than `current`.
    ///
    /// Returns `None` when either side is unparseable, so the caller can decide
    /// what to do about an unknown version rather than being handed a `false`
    /// that means "not newer" and "no idea" at the same time.
    pub fn is_newer(candidate: &str, current: &str) -> Option<bool> {
        Some(Self::parse(candidate)? > Self::parse(current)?)
    }
}

/// One downloadable file from a release, paired with the hash to check it against.
///
/// Uncaged only. Warp's channels derive a download URL from the version string
/// (`autoupdate::release_assets_directory_url`) and trust the result because the
/// bundle is Developer-ID signed. Uncaged has neither: its releases live on
/// GitHub, whose URLs are not derivable from a version alone, and it is ad-hoc
/// signed, so `codesign -R <team>` can prove nothing. Instead the version source
/// carries each file's URL and the SHA-256 GitHub computed over it, and the
/// installer verifies the hash where Warp verifies the signature.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    /// File name, e.g. `Uncaged-macos-aarch64.dmg`. This is how a platform picks
    /// among the formats a release offers — a `.deb` and an `.AppImage` are the
    /// same release installed two different ways.
    pub name: String,
    /// Direct HTTPS download URL.
    pub url: String,
    /// Size in bytes as the release recorded it, so a download can be bounded
    /// before it is read into memory.
    pub size: u64,
    /// Lowercase hex SHA-256 of the file.
    ///
    /// Read from the release *API response*, never from anything served next to
    /// the download itself — a hash published alongside an artifact by whoever
    /// served that artifact proves nothing.
    pub sha256: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct VersionInfo {
    pub version: String,
    /// The version to download for new users from the download page. This is not used on the client
    /// other than in the `apply_overrides` binary used from the `channel-versions` repo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_for_new_users: Option<String>,
    /// The time by which the client needs to be updated, after which
    /// the user sees a warning banner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_by: Option<DateTime<FixedOffset>>,
    /// If specified, this field indicates the oldest version of the client that is still
    /// supported. Any version before this version is not supported and the user should update.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soft_cutoff: Option<String>,
    /// If specified, this field indicates the latest client version that has a prominent update.
    /// Versions before `prominent_update` should display the prominent update UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_prominent_update: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_rollback: Option<bool>,
    /// The version to use for CLI downloads, falling back to `version` if not set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_version: Option<String>,
    /// Downloadable files for this release, already narrowed to the platform the
    /// running build targets.
    ///
    /// Empty on Warp's channels, which derive their URLs and trust a signature.
    /// Populated on Uncaged, where the URL and hash have to travel with the
    /// version. See [`ReleaseAsset`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<ReleaseAsset>,
}

impl VersionInfo {
    pub fn new(version: String) -> Self {
        Self {
            version,
            update_by: None,
            soft_cutoff: None,
            last_prominent_update: None,
            version_for_new_users: None,
            is_rollback: None,
            cli_version: None,
            assets: Vec::new(),
        }
    }

    /// The release asset whose file name matches `predicate`, if this version
    /// carries one. Used by the platform installers to pick their artifact.
    pub fn asset_matching(&self, predicate: impl Fn(&str) -> bool) -> Option<&ReleaseAsset> {
        self.assets.iter().find(|a| predicate(&a.name))
    }

    /// Returns the CLI version, falling back to the app version if not set.
    pub fn cli_version(&self) -> &str {
        self.cli_version.as_deref().unwrap_or(&self.version)
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ChannelChangelogs {
    // Maps of changelogs by version
    pub dev: HashMap<String, Changelog>,
    pub preview: HashMap<String, Changelog>,
    pub stable: HashMap<String, Changelog>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Changelog {
    pub date: DateTime<FixedOffset>,
    pub sections: Vec<Section>,
    #[serde(default = "default_markdown_sections")]
    pub markdown_sections: Vec<MarkdownSection>,
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub oz_updates: Vec<String>,
}

// Default value for when the changelog JSON doesn't have the markdown_sections field
fn default_markdown_sections() -> Vec<MarkdownSection> {
    vec![
        MarkdownSection {
            title: "New features".to_string(),
            markdown: "".to_string(),
        },
        MarkdownSection {
            title: "Improvements".to_string(),
            markdown: "".to_string(),
        },
        MarkdownSection {
            title: "Coming soon".to_string(),
            markdown: "".to_string(),
        },
    ]
}

impl std::fmt::Display for Changelog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            self.sections
                .iter()
                .fold(String::new(), |mut output, item| {
                    let _ = write!(output, "{item}\n\n");
                    output
                })
        )
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub title: String,
    pub items: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct MarkdownSection {
    pub title: String,
    pub markdown: String,
}

impl std::fmt::Display for Section {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:\n{}",
            self.title,
            self.items.iter().fold(String::new(), |mut output, item| {
                let _ = writeln!(output, "- {item}");
                output
            })
        )
    }
}

#[cfg(test)]
#[path = "channel_versions_tests.rs"]
mod tests;

#[cfg(test)]
mod uncaged_version_tests {
    use super::UncagedVersion;

    #[test]
    fn parses_our_tag_format() {
        assert!(UncagedVersion::parse("v0.2.9").is_some());
        assert!(UncagedVersion::parse("0.2.9").is_some());
        assert_eq!(
            UncagedVersion::parse("v1.2.3"),
            UncagedVersion::parse("1.2.3")
        );
    }

    #[test]
    fn rejects_what_it_cannot_order() {
        // Warp's own format: parseable by ParsedVersion, not by this.
        assert!(UncagedVersion::parse("v0.2023.05.15.08.04.stable_01").is_none());
        assert!(UncagedVersion::parse("v0.2").is_none());
        assert!(UncagedVersion::parse("v0.2.9.1").is_none());
        assert!(UncagedVersion::parse("v0.2.9-rc1").is_none());
        assert!(UncagedVersion::parse("").is_none());
    }

    #[test]
    fn orders_numerically_not_lexically() {
        // The bug a string compare would introduce: "0.2.10" < "0.2.9" as text.
        assert_eq!(UncagedVersion::is_newer("v0.2.10", "v0.2.9"), Some(true));
        assert_eq!(UncagedVersion::is_newer("v0.10.0", "v0.9.9"), Some(true));
        assert_eq!(UncagedVersion::is_newer("v1.0.0", "v0.99.99"), Some(true));
    }

    #[test]
    fn same_version_is_not_newer() {
        assert_eq!(UncagedVersion::is_newer("v0.2.9", "v0.2.9"), Some(false));
    }

    #[test]
    fn older_is_not_newer() {
        assert_eq!(UncagedVersion::is_newer("v0.2.8", "v0.2.9"), Some(false));
    }

    #[test]
    fn unparseable_is_none_not_false() {
        assert_eq!(UncagedVersion::is_newer("nonsense", "v0.2.9"), None);
        assert_eq!(UncagedVersion::is_newer("v0.3.0", "nonsense"), None);
    }
}
