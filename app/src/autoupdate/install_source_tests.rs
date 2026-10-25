use std::fs;

use super::*;

fn no_env(_: &str) -> Option<String> {
    None
}

/// The cask's own install location. Every test below that expects Homebrew detection has
/// to claim to be running from here -- a receipt alone is not enough, and should not be.
fn cask_bundle() -> Option<PathBuf> {
    Some(PathBuf::from(CASK_APP_PATH))
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
    let source = detect_with(no_env, std::iter::once(dir.path()), cask_bundle());
    assert_eq!(source, InstallSource::SelfManaged);
    assert!(source.may_self_update());
    assert_eq!(source.upgrade_hint(), None);
}

#[test]
fn a_homebrew_install_must_not_self_update() {
    let dir = tempfile::tempdir().unwrap();
    let prefix = brew_prefix_with_receipt(dir.path());
    let source = detect_with(no_env, std::iter::once(prefix.as_path()), cask_bundle());
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

    let source = detect_with(
        no_env,
        [empty.as_path(), intel.as_path()].into_iter(),
        cask_bundle(),
    );
    assert_eq!(source, InstallSource::Homebrew);
}

#[test]
fn the_escape_hatch_wins_over_detection() {
    let dir = tempfile::tempdir().unwrap();
    // Even on an install we would otherwise happily update.
    let source = detect_with(
        |name| (name == DISABLE_ENV).then(|| "1".to_string()),
        std::iter::once(dir.path()),
        cask_bundle(),
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
        cask_bundle(),
    );
    assert_eq!(source, InstallSource::SelfManaged);
}

/// A Caskroom directory without a receipt is not proof of anything — brew leaves
/// the parent around after `uninstall`.
#[test]
fn a_caskroom_without_a_receipt_is_not_a_homebrew_install() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("Caskroom").join(CASK_TOKEN)).unwrap();
    let source = detect_with(no_env, std::iter::once(dir.path()), cask_bundle());
    assert_eq!(source, InstallSource::SelfManaged);
}

/// A receipt proves Homebrew installed *a* copy of Uncaged. It says nothing about which
/// copy is asking.
///
/// Without this distinction, a build run out of a worktree -- or any `.dmg` copy sitting
/// elsewhere on a machine that also has the cask -- concludes Homebrew owns it and runs
/// `brew upgrade`. That upgrades a different installation, leaves the running one exactly
/// as it was, and then reports success, so the user restarts into the same version they
/// started with.
#[test]
fn a_receipt_elsewhere_does_not_make_this_copy_homebrews() {
    let dir = tempfile::tempdir().unwrap();
    let prefix = brew_prefix_with_receipt(dir.path());

    let source = detect_with(
        no_env,
        std::iter::once(prefix.as_path()),
        Some(PathBuf::from(
            "/Users/someone/src/uncaged/target/release/Uncaged.app",
        )),
    );

    assert_eq!(
        source,
        InstallSource::SelfManaged,
        "a copy running from somewhere else must update itself, not shell out to brew"
    );
    assert_eq!(source.upgrade_command(), None);
}

/// When the running bundle cannot be determined, prefer updating the copy that is actually
/// running over handing the job to a package manager that may own a different one.
#[test]
fn an_unknown_bundle_path_falls_back_to_self_managed() {
    let dir = tempfile::tempdir().unwrap();
    let prefix = brew_prefix_with_receipt(dir.path());
    let source = detect_with(no_env, std::iter::once(prefix.as_path()), None);
    assert_eq!(source, InstallSource::SelfManaged);
}

/// The upgrade command names the cask explicitly and points at a brew that exists, rather
/// than trusting `brew` to be on a GUI app's PATH -- it is not.
#[test]
fn the_homebrew_upgrade_command_is_a_real_brew_binary() {
    let source = InstallSource::Homebrew;
    match source.upgrade_command() {
        Some(command) => {
            assert!(
                command[0].ends_with("/bin/brew"),
                "expected an absolute brew path, got {:?}",
                command[0]
            );
            assert!(std::path::Path::new(&command[0]).exists());
            assert_eq!(&command[1..], ["upgrade", "--cask", CASK_TOKEN]);
        }
        // No Homebrew on this machine: nothing to assert, and nothing to run.
        None => assert!(!std::path::Path::new("/opt/homebrew/bin/brew").exists()),
    }
}
