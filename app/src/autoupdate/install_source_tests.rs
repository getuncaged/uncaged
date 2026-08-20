use std::fs;

use super::*;

fn no_env(_: &str) -> Option<String> {
    None
}

/// Builds a fake Homebrew prefix containing a cask receipt, laid out exactly as
/// a real one is: `<prefix>/Caskroom/uncaged/.metadata/INSTALL_RECEIPT.json`.
fn brew_prefix_with_receipt(root: &Path) -> PathBuf {
    let meta = root.join("Caskroom").join(CASK_TOKEN).join(".metadata");
    fs::create_dir_all(&meta).unwrap();
    fs::write(meta.join("INSTALL_RECEIPT.json"), "{}").unwrap();
    root.to_path_buf()
}

#[test]
fn a_plain_download_may_self_update() {
    let dir = tempfile::tempdir().unwrap();
    let source = detect_with(no_env, std::iter::once(dir.path()));
    assert_eq!(source, InstallSource::SelfManaged);
    assert!(source.may_self_update());
    assert_eq!(source.upgrade_hint(), None);
}

#[test]
fn a_homebrew_install_must_not_self_update() {
    let dir = tempfile::tempdir().unwrap();
    let prefix = brew_prefix_with_receipt(dir.path());
    let source = detect_with(no_env, std::iter::once(prefix.as_path()));
    assert_eq!(source, InstallSource::Homebrew);
    assert!(
        !source.may_self_update(),
        "replacing a brew-managed bundle desyncs brew's records"
    );
    assert_eq!(source.upgrade_hint(), Some("brew upgrade --cask uncaged"));
}

/// Apple Silicon and Intel put Homebrew in different places; finding it in
/// either is enough.
#[test]
fn either_homebrew_prefix_counts() {
    let dir = tempfile::tempdir().unwrap();
    let intel = brew_prefix_with_receipt(&dir.path().join("usr-local"));
    let empty = dir.path().join("opt-homebrew");
    fs::create_dir_all(&empty).unwrap();

    let source = detect_with(no_env, [empty.as_path(), intel.as_path()].into_iter());
    assert_eq!(source, InstallSource::Homebrew);
}

#[test]
fn the_escape_hatch_wins_over_detection() {
    let dir = tempfile::tempdir().unwrap();
    // Even on an install we would otherwise happily update.
    let source = detect_with(
        |name| (name == DISABLE_ENV).then(|| "1".to_string()),
        std::iter::once(dir.path()),
    );
    assert_eq!(source, InstallSource::DisabledByUser);
    assert!(!source.may_self_update());
}

/// An empty value is not an opt-out — otherwise `UNCAGED_DISABLE_SELF_UPDATE=`
/// left in a shell profile would silently disable updates forever.
#[test]
fn an_empty_escape_hatch_is_not_an_opt_out() {
    let dir = tempfile::tempdir().unwrap();
    let source = detect_with(
        |name| (name == DISABLE_ENV).then(String::new),
        std::iter::once(dir.path()),
    );
    assert_eq!(source, InstallSource::SelfManaged);
}

/// A Caskroom directory without a receipt is not proof of anything — brew leaves
/// the parent around after `uninstall`.
#[test]
fn a_caskroom_without_a_receipt_is_not_a_homebrew_install() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("Caskroom").join(CASK_TOKEN)).unwrap();
    let source = detect_with(no_env, std::iter::once(dir.path()));
    assert_eq!(source, InstallSource::SelfManaged);
}
