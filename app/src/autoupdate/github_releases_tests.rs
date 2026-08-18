//! Tests for the GitHub release source.
//!
//! The fixtures below are the real shape of `/repos/getuncaged/uncaged/releases/latest`,
//! trimmed to the fields we read, with the genuine v0.2.9 digests.

use super::*;

/// A release payload shaped like the live one.
fn release_json(extra_assets: &str) -> String {
    format!(
        r#"{{
          "tag_name": "v0.3.0",
          "draft": false,
          "prerelease": false,
          "assets": [
            {{
              "name": "Uncaged-macos-aarch64.dmg",
              "browser_download_url": "https://github.com/getuncaged/uncaged/releases/download/v0.3.0/Uncaged-macos-aarch64.dmg",
              "size": 154667310,
              "state": "uploaded",
              "digest": "sha256:14836fc756ed68dcda024c0e08d641fbfc832bc044d0874663d07e49c648e803"
            }},
            {{
              "name": "Uncaged-macos-x86_64.dmg",
              "browser_download_url": "https://github.com/getuncaged/uncaged/releases/download/v0.3.0/Uncaged-macos-x86_64.dmg",
              "size": 161914861,
              "state": "uploaded",
              "digest": "sha256:852bbe56ff45ca21702a4a02d022c77cc3e1fd6fd15431996243ef5e0f9776cd"
            }}
            {extra_assets}
          ]
        }}"#
    )
}

fn parse(json: &str, target: &str) -> Option<VersionInfo> {
    let release: GhRelease = serde_json::from_str(json).expect("fixture should parse");
    version_info_from_release(release, target)
}

#[test]
fn picks_the_asset_for_this_platform() {
    let info = parse(&release_json(""), "macos-aarch64").expect("should offer an update");
    assert_eq!(info.version, "v0.3.0");
    assert_eq!(info.assets.len(), 1, "only this platform's asset");
    assert_eq!(info.assets[0].name, "Uncaged-macos-aarch64.dmg");
    assert_eq!(
        info.assets[0].sha256,
        "14836fc756ed68dcda024c0e08d641fbfc832bc044d0874663d07e49c648e803"
    );
}

#[test]
fn no_asset_for_this_platform_is_not_an_update() {
    assert!(parse(&release_json(""), "linux-aarch64").is_none());
}

#[test]
fn drafts_and_prereleases_are_ignored() {
    // Replace each flag in place. Substituting one key's text for the other's
    // would leave a duplicate key, and serde keeps the last — the test would
    // then pass or fail for the wrong reason.
    for (from, to) in [
        (r#""draft": false"#, r#""draft": true"#),
        (r#""prerelease": false"#, r#""prerelease": true"#),
    ] {
        let json = release_json("").replace(from, to);
        assert!(json.contains(to), "fixture should contain {to}");
        assert!(
            parse(&json, "macos-aarch64").is_none(),
            "should ignore a release with {to}"
        );
    }
}

/// No digest means no way to tell a real build from a substituted one, and the
/// app is ad-hoc signed so there is no signature to fall back on. Refuse.
#[test]
fn an_asset_without_a_digest_is_refused() {
    let json = release_json("").replace(
        r#""digest": "sha256:14836fc756ed68dcda024c0e08d641fbfc832bc044d0874663d07e49c648e803""#,
        r#""digest": null"#,
    );
    assert!(parse(&json, "macos-aarch64").is_none());
}

#[test]
fn an_asset_still_uploading_is_skipped() {
    let json = release_json("").replace(r#""state": "uploaded""#, r#""state": "starter""#);
    assert!(parse(&json, "macos-aarch64").is_none());
}

#[test]
fn only_sha256_digests_are_accepted() {
    assert_eq!(
        parse_sha256_digest(
            "sha256:14836fc756ed68dcda024c0e08d641fbfc832bc044d0874663d07e49c648e803"
        ),
        Some("14836fc756ed68dcda024c0e08d641fbfc832bc044d0874663d07e49c648e803".to_string())
    );
    // Uppercase hex normalises, so verification stays a string compare.
    assert_eq!(
        parse_sha256_digest(&format!("sha256:{}", "A".repeat(64))),
        Some("a".repeat(64))
    );
    // Anything else is refused rather than guessed at.
    assert_eq!(parse_sha256_digest("md5:abc"), None);
    assert_eq!(parse_sha256_digest("sha512:abc"), None);
    assert_eq!(parse_sha256_digest("sha256:short"), None);
    assert_eq!(
        parse_sha256_digest(&format!("sha256:{}", "z".repeat(64))),
        None
    );
    assert_eq!(parse_sha256_digest(""), None);
}

/// The download URL comes out of the API response, so it is untrusted input.
#[test]
fn only_https_github_hosts_are_downloadable() {
    let allowed = [
        "https://github.com/getuncaged/uncaged/releases/download/v0.3.0/x.dmg",
        "https://objects.githubusercontent.com/some/object",
        "https://release-assets.githubusercontent.com/some/object",
    ];
    for url in allowed {
        assert!(
            host_is_allowed(&reqwest::Url::parse(url).unwrap()),
            "{url} should be allowed"
        );
    }

    let refused = [
        // plaintext, even on the right host
        "http://github.com/getuncaged/uncaged/releases/download/v0.3.0/x.dmg",
        // a lookalike that merely ends with the string
        "https://evil-github.com/x.dmg",
        "https://githubusercontent.com.attacker.test/x.dmg",
        "https://attacker.test/x.dmg",
        // subdomain of an allowed host is still not an allowed host
        "https://evil.objects.githubusercontent.com.attacker.test/x",
    ];
    for url in refused {
        assert!(
            !host_is_allowed(&reqwest::Url::parse(url).unwrap()),
            "{url} should be refused"
        );
    }
}

#[test]
fn every_published_platform_has_a_target() {
    // Guards the asset-naming contract in DOWNLOADS.md: if a build is published
    // for a platform, the updater must know what its assets are called.
    for (os, arch) in [
        ("macos", "aarch64"),
        ("macos", "x86_64"),
        ("linux", "aarch64"),
        ("linux", "x86_64"),
        ("windows", "aarch64"),
        ("windows", "x86_64"),
    ] {
        let expected = format!("{os}-{arch}");
        assert!(
            [
                "macos-aarch64",
                "macos-x86_64",
                "linux-aarch64",
                "linux-x86_64",
                "windows-aarch64",
                "windows-x86_64"
            ]
            .contains(&expected.as_str()),
            "{expected} should be a known target"
        );
    }
}
