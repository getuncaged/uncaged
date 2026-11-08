use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::*;

/// A probe with fixed answers that records whether it was consulted.
#[derive(Default)]
struct FakeProbe {
    instance: Option<String>,
    database_in_use: bool,
    consulted: Cell<bool>,
}

impl Probe for FakeProbe {
    fn other_instance_running(&self) -> Option<String> {
        self.consulted.set(true);
        self.instance.clone()
    }

    fn database_in_use(&self, _database: &Path) -> bool {
        self.consulted.set(true);
        self.database_in_use
    }
}

/// Two identities with three roots each, laid out like macOS's: the first root
/// holds the database.
struct Layout {
    temp: TempDir,
    old: IdentityDirs,
    new: IdentityDirs,
}

fn layout() -> Layout {
    let temp = TempDir::new().unwrap();
    let base = temp.path();
    let dirs = |name: &str| IdentityDirs {
        state_dir: base.join("support").join(name),
        roots: vec![
            base.join("support").join(name),
            base.join("caches").join(name),
            base.join("saved").join(format!("{name}.savedState")),
        ],
    };
    Layout {
        old: dirs("dev.uncaged.WarpOss"),
        new: dirs("dev.uncaged.Uncaged"),
        temp,
    }
}

/// Seeds the old identity like a v0.3.4 install: a database with WAL and SHM,
/// secrets, caches and saved state.
fn seed_old(layout: &Layout) {
    let state = &layout.old.state_dir;
    fs::create_dir_all(state.join("secrets")).unwrap();
    fs::write(state.join("warp.sqlite"), "db").unwrap();
    fs::write(state.join("warp.sqlite-wal"), "wal").unwrap();
    fs::write(state.join("warp.sqlite-shm"), "shm").unwrap();
    fs::write(
        state.join("secrets/dev.uncaged.WarpOss-FileBasedMcpCredentials"),
        "secret",
    )
    .unwrap();
    for root in &layout.old.roots[1..] {
        fs::create_dir_all(root).unwrap();
        fs::write(root.join("data"), root.display().to_string()).unwrap();
    }
}

fn is_link_to(from: &Path, to: &Path) -> bool {
    fs::symlink_metadata(from).is_ok_and(|metadata| metadata.is_symlink())
        && fs::read_link(from).is_ok_and(|target| target == to)
}

fn tree(root: &Path) -> Vec<PathBuf> {
    let mut entries = Vec::new();
    let mut stack = vec![root.to_owned()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                stack.push(path.clone());
            }
            entries.push(path);
        }
    }
    entries.sort();
    entries
}

#[test]
fn fresh_install_creates_nothing() {
    let layout = layout();
    let probe = FakeProbe::default();

    let report = migrate(&layout.old, &layout.new, &probe);

    assert_eq!(report.outcome, Outcome::NothingToMigrate);
    assert!(!probe.consulted.get(), "nothing to move, so no probing");
    for root in layout.old.roots.iter().chain(&layout.new.roots) {
        assert!(fs::symlink_metadata(root).is_err(), "{root:?} was created");
    }
}

#[test]
fn moves_every_root_and_links_the_old_paths() {
    let layout = layout();
    seed_old(&layout);

    let report = migrate(&layout.old, &layout.new, &FakeProbe::default());

    assert_eq!(report.outcome, Outcome::Migrated, "{:?}", report.notes);
    let state = &layout.new.state_dir;
    assert_eq!(fs::read_to_string(state.join("warp.sqlite")).unwrap(), "db");
    assert_eq!(
        fs::read_to_string(state.join("warp.sqlite-wal")).unwrap(),
        "wal"
    );
    assert_eq!(
        fs::read_to_string(state.join("warp.sqlite-shm")).unwrap(),
        "shm"
    );
    assert!(state
        .join("secrets/dev.uncaged.WarpOss-FileBasedMcpCredentials")
        .exists());
    for (old, new) in layout.old.roots.iter().zip(&layout.new.roots) {
        assert!(is_link_to(old, new), "{old:?} should link to {new:?}");
        assert!(
            !new.join(MARKER_FILE_NAME).exists(),
            "marker left in {new:?}"
        );
    }
    // A downgraded v0.3.4 reads the same files through the old paths.
    assert_eq!(
        fs::read_to_string(layout.old.state_dir.join("warp.sqlite")).unwrap(),
        "db"
    );
}

#[test]
fn already_migrated_is_left_untouched() {
    let layout = layout();
    seed_old(&layout);
    migrate(&layout.old, &layout.new, &FakeProbe::default());
    let before = tree(layout.temp.path());
    let probe = FakeProbe::default();

    let report = migrate(&layout.old, &layout.new, &probe);

    assert_eq!(report.outcome, Outcome::AlreadyMigrated);
    assert!(!probe.consulted.get());
    assert_eq!(tree(layout.temp.path()), before);
}

#[test]
fn finishes_a_partial_previous_attempt() {
    let layout = layout();
    seed_old(&layout);
    // A previous launch moved and linked the first root only.
    fs::create_dir_all(layout.new.roots[0].parent().unwrap()).unwrap();
    fs::rename(&layout.old.roots[0], &layout.new.roots[0]).unwrap();
    std::os::unix::fs::symlink(&layout.new.roots[0], &layout.old.roots[0]).unwrap();

    let report = migrate(&layout.old, &layout.new, &FakeProbe::default());

    assert_eq!(report.outcome, Outcome::Migrated, "{:?}", report.notes);
    for (old, new) in layout.old.roots.iter().zip(&layout.new.roots) {
        assert!(is_link_to(old, new), "{old:?} should link to {new:?}");
    }
    assert_eq!(
        fs::read_to_string(layout.new.roots[1].join("data")).unwrap(),
        layout.old.roots[1].display().to_string()
    );
}

#[test]
fn finishes_a_move_interrupted_before_the_link() {
    let layout = layout();
    seed_old(&layout);
    // A previous launch wrote the marker and renamed, then stopped.
    fs::write(layout.old.roots[0].join(MARKER_FILE_NAME), "").unwrap();
    fs::create_dir_all(layout.new.roots[0].parent().unwrap()).unwrap();
    fs::rename(&layout.old.roots[0], &layout.new.roots[0]).unwrap();
    for (old, new) in layout.old.roots[1..].iter().zip(&layout.new.roots[1..]) {
        fs::create_dir_all(new.parent().unwrap()).unwrap();
        fs::rename(old, new).unwrap();
        std::os::unix::fs::symlink(new, old).unwrap();
    }

    let report = migrate(&layout.old, &layout.new, &FakeProbe::default());

    assert_eq!(
        report.outcome,
        Outcome::AlreadyMigrated,
        "{:?}",
        report.notes
    );
    assert!(is_link_to(&layout.old.roots[0], &layout.new.roots[0]));
    assert!(!layout.new.roots[0].join(MARKER_FILE_NAME).exists());
}

#[test]
fn never_merges_into_an_existing_new_location() {
    let layout = layout();
    seed_old(&layout);
    fs::create_dir_all(&layout.new.roots[0]).unwrap();
    fs::write(layout.new.roots[0].join("warp.sqlite"), "new db").unwrap();

    let report = migrate(&layout.old, &layout.new, &FakeProbe::default());

    // The first root is left alone on both sides; the others still move.
    assert_eq!(report.outcome, Outcome::Migrated, "{:?}", report.notes);
    assert_eq!(
        fs::read_to_string(layout.old.roots[0].join("warp.sqlite")).unwrap(),
        "db"
    );
    assert_eq!(
        fs::read_to_string(layout.new.roots[0].join("warp.sqlite")).unwrap(),
        "new db"
    );
    assert!(!fs::symlink_metadata(&layout.old.roots[0])
        .unwrap()
        .is_symlink());
    assert!(is_link_to(&layout.old.roots[1], &layout.new.roots[1]));
}

#[test]
fn a_dangling_compat_link_is_not_data() {
    let layout = layout();
    // The user deleted the new directory; the old path still links to it.
    fs::create_dir_all(layout.old.roots[0].parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&layout.new.roots[0], &layout.old.roots[0]).unwrap();

    let report = migrate(&layout.old, &layout.new, &FakeProbe::default());

    assert_eq!(report.outcome, Outcome::NothingToMigrate);
    assert!(fs::symlink_metadata(&layout.new.roots[0]).is_err());
}

#[test]
fn a_users_own_symlink_moves_with_its_target_intact() {
    let layout = layout();
    let elsewhere = layout.temp.path().join("dotfiles/uncaged");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::write(elsewhere.join("warp.sqlite"), "db").unwrap();
    fs::create_dir_all(layout.old.roots[0].parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &layout.old.roots[0]).unwrap();

    let report = migrate(&layout.old, &layout.new, &FakeProbe::default());

    assert_eq!(report.outcome, Outcome::Migrated, "{:?}", report.notes);
    assert!(is_link_to(&layout.new.roots[0], &elsewhere));
    assert!(is_link_to(&layout.old.roots[0], &layout.new.roots[0]));
    assert_eq!(
        fs::read_to_string(layout.new.state_dir.join("warp.sqlite")).unwrap(),
        "db"
    );
    assert!(!elsewhere.join(MARKER_FILE_NAME).exists());
}

#[test]
fn defers_while_the_database_is_open() {
    let layout = layout();
    seed_old(&layout);
    let before = tree(layout.temp.path());
    let probe = FakeProbe {
        database_in_use: true,
        ..Default::default()
    };

    let report = migrate(&layout.old, &layout.new, &probe);

    assert_eq!(report.outcome, Outcome::Deferred);
    assert_eq!(tree(layout.temp.path()), before, "nothing may move");
}

#[test]
fn defers_while_another_instance_runs() {
    let layout = layout();
    seed_old(&layout);
    let before = tree(layout.temp.path());
    let probe = FakeProbe {
        instance: Some("warp-oss (pid 1)".to_owned()),
        ..Default::default()
    };

    let report = migrate(&layout.old, &layout.new, &probe);

    assert_eq!(report.outcome, Outcome::Deferred);
    assert!(report.notes.iter().any(|note| note.contains("warp-oss")));
    assert_eq!(tree(layout.temp.path()), before, "nothing may move");
}

#[test]
fn a_failed_rename_puts_earlier_roots_back() {
    let layout = layout();
    seed_old(&layout);
    // The second root's parent is read-only, so its rename cannot happen.
    let caches = layout.new.roots[1].parent().unwrap().to_owned();
    let writable = fs::metadata(&caches).unwrap().permissions();
    let mut read_only = writable.clone();
    read_only.set_readonly(true);
    fs::set_permissions(&caches, read_only).unwrap();

    let report = migrate(&layout.old, &layout.new, &FakeProbe::default());
    fs::set_permissions(&caches, writable).unwrap();

    assert_eq!(report.outcome, Outcome::Deferred, "{:?}", report.notes);
    assert_eq!(
        fs::read_to_string(layout.old.state_dir.join("warp.sqlite")).unwrap(),
        "db"
    );
    assert!(!fs::symlink_metadata(&layout.old.roots[0])
        .unwrap()
        .is_symlink());
    assert!(fs::symlink_metadata(&layout.new.roots[0]).is_err());
    assert!(!layout.old.roots[0].join(MARKER_FILE_NAME).exists());
}

/// The real lock probe against a real sqlite connection held open by another
/// process. Skipped where the `sqlite3` CLI is not installed.
#[test]
fn detects_a_database_another_process_has_open() {
    use std::io::Write as _;
    use std::process::Stdio;

    let Some(sqlite3) = ["/usr/bin/sqlite3", "/usr/local/bin/sqlite3"]
        .into_iter()
        .map(Path::new)
        .find(|path| path.exists())
    else {
        eprintln!("sqlite3 not found; skipping");
        return;
    };
    let temp = TempDir::new().unwrap();
    let database = temp.path().join("warp.sqlite");
    let status = command::blocking::Command::new(sqlite3)
        .arg(&database)
        .arg("PRAGMA journal_mode=WAL; CREATE TABLE t(x);")
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    assert!(!database_in_use(&database), "closed database reported open");

    // A connection that stays open until its stdin closes.
    let mut child = command::blocking::Command::new(sqlite3)
        .arg(&database)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, "SELECT count(*) FROM t;").unwrap();
    stdin.flush().unwrap();
    let opened = (0..100).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(20));
        database_in_use(&database)
    });

    drop(stdin);
    child.wait().unwrap();
    assert!(opened, "open database not detected");
    assert!(!database_in_use(&database), "database still reported open");
}

/// Copies between two throwaway defaults domains, never the app's own.
#[cfg(target_os = "macos")]
#[test]
fn copies_user_defaults_once_keeping_newer_values() {
    use objc2::AnyThread as _;
    use objc2_foundation::{NSString, NSUserDefaults};

    // Fixed names, so a run leaves at most these two (emptied) domains behind.
    let from = "dev.uncaged.identity-migration-test-old".to_owned();
    let to = "dev.uncaged.identity-migration-test-new".to_owned();
    let defaults = |name: &str| {
        NSUserDefaults::initWithSuiteName(NSUserDefaults::alloc(), Some(&NSString::from_str(name)))
            .unwrap()
    };
    let set = |name: &str, key: &str, value: &str| {
        let suite = defaults(name);
        let value = NSString::from_str(value);
        let value: &objc2::runtime::AnyObject = &value;
        // SAFETY: an `NSString` is a valid property-list value.
        unsafe { suite.setObject_forKey(Some(value), &NSString::from_str(key)) };
    };
    let get = |name: &str, key: &str| {
        defaults(name)
            .stringForKey(&NSString::from_str(key))
            .map(|value| value.to_string())
    };
    set(&from, "Theme", "old theme");
    set(&from, "Font", "old font");
    set(&to, "Font", "new font");

    let copied = copy_user_defaults(&from, &to);
    set(&from, "Theme", "changed after the copy");
    let copied_again = copy_user_defaults(&from, &to);
    let results = (
        get(&to, "Theme"),
        get(&to, "Font"),
        get(&from, "Font"),
        get(&to, PREFERENCES_MARKER_KEY),
    );

    let standard = NSUserDefaults::standardUserDefaults();
    for name in [&from, &to] {
        standard.removePersistentDomainForName(&NSString::from_str(name));
        if let Some(home) = dirs::home_dir() {
            let _ = fs::remove_file(home.join(format!("Library/Preferences/{name}.plist")));
        }
    }
    assert!(copied);
    assert!(!copied_again, "the copy happens once");
    assert_eq!(
        results,
        (
            Some("old theme".to_owned()),
            Some("new font".to_owned()),
            Some("old font".to_owned()),
            Some(from.clone()),
        )
    );
}
