//! One-time move of the data the previous app identity left behind.
//!
//! Uncaged shipped up to v0.3.4 as `warp-oss`, with the bundle id and data
//! domain `dev.uncaged.WarpOss`. Every data, state, cache and log directory the
//! platform derives from that identity ([`paths::identity_dirs`]) moves when the
//! identity becomes `dev.uncaged.Uncaged`, so the first launch of a renamed build
//! carries an existing install's data over. `~/.uncaged` is not derived from the
//! identity and is left alone.
//!
//! The rules, each pinned by `identity_migration_tests.rs`:
//! - A directory moves only when its new location does not exist (or holds
//!   nothing but empty directories). Nothing is merged and nothing is
//!   overwritten.
//! - A move is a rename: atomic per directory and never a copy.
//! - Each moved directory leaves a symlink (a junction on Windows) at its old
//!   path, so downgrading to v0.3.4 still finds its data.
//! - If another Uncaged process is running (old or new binary) or the old
//!   database is open, nothing moves: this launch keeps using the old identity's
//!   data ([`ChannelState::set_data_app_id`]) and the next launch tries again.
//! - If a rename fails, the renames done by this launch are put back and the
//!   launch keeps using the old identity's data.
//! - Only the app's own launch moves data. Command-line and worker invocations
//!   (`uncaged <subcommand>`) keep using the old identity's data until it has.
//! - A marker file written before a rename and removed once the old path links
//!   to the new one lets a later launch finish a move interrupted in between.
//!
//! Preferences (NSUserDefaults on macOS, the registry on Windows) are copied
//! once rather than moved, so the old identity keeps its own copy.
//!
//! Nothing here can log when it runs: it runs before logging is initialized,
//! because on Linux and Windows the log file lives in a directory it moves. What
//! happened is kept in [`REPORT`] and logged by [`log_report`] once logging is up.

use std::path::Path;
use std::sync::OnceLock;
use std::{fs, io};

use uncaged_core::channel::{Channel, ChannelState};
use uncaged_core::paths::{self, IdentityDirs};
use uncaged_core::AppId;

/// The identity Uncaged used up to and including v0.3.4.
pub(crate) fn legacy_app_id() -> AppId {
    AppId::new("dev", "uncaged", "WarpOss")
}

/// Written into a directory just before it is renamed, removed once its old path
/// links to the new one.
const MARKER_FILE_NAME: &str = ".uncaged-identity-migration";

/// The sqlite database inside [`IdentityDirs::state_dir`].
const DATABASE_FILE_NAME: &str = "warp.sqlite";

/// Executable names a running Uncaged process can have, old and new.
#[cfg(not(target_family = "wasm"))]
const EXECUTABLE_NAMES: &[&str] = &["uncaged", "warp-oss"];

/// Records, in the new preferences domain, which domain it was copied from.
#[cfg(any(target_os = "macos", windows))]
const PREFERENCES_MARKER_KEY: &str = "UncagedIdentityMigratedFrom";

static REPORT: OnceLock<Report> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// No data of the previous identity exists.
    NothingToMigrate,
    /// Everything the previous identity had is already at its new location.
    AlreadyMigrated,
    /// This launch moved the previous identity's data.
    Migrated,
    /// Moving was not safe on this launch; it keeps using the previous
    /// identity's data and tries again on the next.
    Deferred,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Report {
    pub(crate) outcome: Outcome,
    /// What happened, for the log.
    pub(crate) notes: Vec<String>,
}

/// What [`migrate`] needs to know about the rest of the system.
pub(crate) trait Probe {
    /// Describes another running Uncaged process (or another reason this
    /// process must not move data), if there is one.
    fn other_instance_running(&self) -> Option<String>;
    /// Whether some process has `database` open.
    fn database_in_use(&self, database: &Path) -> bool;
}

/// Moves the previous identity's data to the current identity if this build
/// uses the renamed identity.
///
/// Must run before anything computes a data path or domain: logging, secure
/// storage, settings, sqlite. With `may_move` false (a command-line or worker
/// invocation) nothing moves: if data is still to move, this process uses the
/// previous identity's data, as a deferred launch does.
pub(crate) fn run_if_needed(may_move: bool) {
    let current = ChannelState::app_id();
    let legacy = legacy_app_id();
    if ChannelState::channel() != Channel::Oss || current.to_string() == legacy.to_string() {
        return;
    }

    let data_profile = ChannelState::data_profile();
    let (Some(old), Some(new)) = (
        paths::identity_dirs(legacy.clone(), data_profile.as_deref()),
        paths::identity_dirs(current.clone(), data_profile.as_deref()),
    ) else {
        return;
    };

    // A data profile is a debug-only sandbox, usually run next to the user's
    // real install, so any other Uncaged process says nothing about whether the
    // profile's own data is in use. Its database check still applies.
    let probe = SystemProbe {
        may_move,
        check_processes: data_profile.is_none(),
    };
    #[cfg_attr(not(any(target_os = "macos", windows)), expect(unused_mut))]
    let mut report = migrate(&old, &new, &probe);

    if report.outcome == Outcome::Deferred {
        ChannelState::set_data_app_id(legacy);
    } else {
        // Preferences are copied rather than moved, so the old identity keeps
        // its own; values the current identity already has win.
        #[cfg(target_os = "macos")]
        {
            let from = data_domain(&legacy, data_profile.as_deref());
            let to = data_domain(&current, data_profile.as_deref());
            if copy_user_defaults(&from, &to) {
                report
                    .notes
                    .push(format!("copied preferences from {from} to {to}"));
            }
        }
        #[cfg(windows)]
        copy_registry_preferences(
            legacy.application_name(),
            current.application_name(),
            &mut report.notes,
        );
    }
    let _ = REPORT.set(report);
}

/// The previous identity's data domain, while this launch uses a different
/// one: secrets stored under it are still read (and copied forward).
pub(crate) fn legacy_data_domain() -> Option<String> {
    if ChannelState::channel() != Channel::Oss {
        return None;
    }
    let legacy = legacy_app_id().to_string();
    let legacy = match ChannelState::data_profile() {
        Some(profile) => format!("{legacy}-{profile}"),
        None => legacy,
    };
    (legacy != ChannelState::data_domain()).then_some(legacy)
}

/// Logs what [`run_if_needed`] did. Call once logging is initialized.
pub(crate) fn log_report() {
    let Some(report) = REPORT.get() else {
        return;
    };
    match report.outcome {
        Outcome::NothingToMigrate | Outcome::AlreadyMigrated => {
            for note in &report.notes {
                log::info!("Identity migration: {note}");
            }
        }
        Outcome::Migrated => {
            log::info!("Identity migration: moved the previous identity's data");
            for note in &report.notes {
                log::info!("Identity migration: {note}");
            }
        }
        Outcome::Deferred => {
            log::warn!(
                "Identity migration deferred; using {} data for this launch",
                legacy_app_id()
            );
            for note in &report.notes {
                log::warn!("Identity migration: {note}");
            }
        }
    }
}

/// How one directory of the previous identity relates to its new location.
#[derive(Debug, PartialEq, Eq)]
enum Root {
    /// Nothing at the old path (or only a link that leads nowhere).
    Absent,
    /// The new location exists; the old path, if any, is not ours to touch.
    NewExists,
    /// A previous launch renamed it but stopped before linking the old path.
    Interrupted,
    /// Data at the old path, nothing at the new one.
    Pending,
}

fn classify(from: &Path, to: &Path) -> Root {
    // Directories without a single file in them are not data: something
    // (another tool, a test run, the system) can create them before the first
    // launch, and treating them as a migrated identity would strand the data.
    let new_exists = fs::symlink_metadata(to).is_ok() && !is_empty_tree(to);
    let old = fs::symlink_metadata(from);
    if new_exists {
        if old.is_err() && to.join(MARKER_FILE_NAME).exists() {
            return Root::Interrupted;
        }
        return Root::NewExists;
    }
    match old {
        Err(_) => Root::Absent,
        Ok(metadata) if metadata.file_type().is_symlink() => {
            // Our own link whose target was removed is not data. A link the user
            // made (say, into a dotfiles checkout) is: it moves like a directory.
            let ours = fs::read_link(from).is_ok_and(|target| target == to);
            if ours || fs::metadata(from).is_err() {
                Root::Absent
            } else {
                Root::Pending
            }
        }
        Ok(_) => Root::Pending,
    }
}

/// Moves each root of `old` to the matching root of `new`, per the rules in the
/// module docs. Pure apart from the filesystem and `probe`.
pub(crate) fn migrate(old: &IdentityDirs, new: &IdentityDirs, probe: &dyn Probe) -> Report {
    let mut notes = Vec::new();
    let mut pending = Vec::new();
    let mut found_migrated = false;

    for (from, to) in old.roots.iter().zip(&new.roots) {
        match classify(from, to) {
            Root::Absent => {}
            Root::NewExists => {
                found_migrated = true;
                if fs::symlink_metadata(from).is_ok_and(|metadata| !metadata.is_symlink()) {
                    notes.push(format!(
                        "{} and {} both exist; left both alone",
                        from.display(),
                        to.display()
                    ));
                }
            }
            Root::Interrupted => {
                found_migrated = true;
                match link_and_clear_marker(from, to) {
                    Ok(()) => notes.push(format!(
                        "finished an interrupted move by linking {} to {}",
                        from.display(),
                        to.display()
                    )),
                    Err(err) => notes.push(format!(
                        "could not link {} to {}: {err}",
                        from.display(),
                        to.display()
                    )),
                }
            }
            Root::Pending => pending.push((from.as_path(), to.as_path())),
        }
    }

    if pending.is_empty() {
        let outcome = match found_migrated {
            true => Outcome::AlreadyMigrated,
            false => Outcome::NothingToMigrate,
        };
        return Report { outcome, notes };
    }

    if let Some(instance) = probe.other_instance_running() {
        notes.push(format!("another Uncaged process is running: {instance}"));
        return Report {
            outcome: Outcome::Deferred,
            notes,
        };
    }
    let database = old.state_dir.join(DATABASE_FILE_NAME);
    if database.exists() && probe.database_in_use(&database) {
        notes.push(format!("{} is open in another process", database.display()));
        return Report {
            outcome: Outcome::Deferred,
            notes,
        };
    }

    let mut moved: Vec<(&Path, &Path)> = Vec::new();
    for &(from, to) in &pending {
        if let Err(err) = move_root(from, to) {
            notes.push(format!(
                "moving {} to {} failed: {err}",
                from.display(),
                to.display()
            ));
            for &(done_from, done_to) in moved.iter().rev() {
                notes.push(put_back(done_from, done_to));
            }
            return Report {
                outcome: Outcome::Deferred,
                notes,
            };
        }
        moved.push((from, to));
    }

    for (from, to) in moved {
        match link_and_clear_marker(from, to) {
            Ok(()) => notes.push(format!("moved {} to {}", from.display(), to.display())),
            Err(err) => notes.push(format!(
                "moved {} to {}, but could not link the old path (a downgrade will not find this data): {err}",
                from.display(),
                to.display()
            )),
        }
    }
    Report {
        outcome: Outcome::Migrated,
        notes,
    }
}

fn move_root(from: &Path, to: &Path) -> io::Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    // `classify` only lets an empty directory tree stand at `to`.
    if fs::symlink_metadata(to).is_ok() {
        remove_empty_tree(to)?;
    }
    // Best-effort: without the marker an interrupted move is still safe, a
    // later launch just cannot restore the downgrade link.
    let marker = from.join(MARKER_FILE_NAME);
    if from.is_dir() {
        let _ = fs::write(&marker, format!("{}\n", from.display()));
    }
    if let Err(err) = fs::rename(from, to) {
        let _ = fs::remove_file(&marker);
        return Err(err);
    }
    Ok(())
}

/// Whether `path` is a directory (not a link) containing nothing but
/// directories, at any depth.
fn is_empty_tree(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.is_dir() {
        return false;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };
    entries
        .into_iter()
        .all(|entry| entry.is_ok_and(|entry| is_empty_tree(&entry.path())))
}

/// Removes a tree [`is_empty_tree`] accepted. `remove_dir` refuses a directory
/// with anything in it, so a file that appeared meanwhile stops this rather
/// than being deleted.
fn remove_empty_tree(path: &Path) -> io::Result<()> {
    for entry in fs::read_dir(path)? {
        remove_empty_tree(&entry?.path())?;
    }
    fs::remove_dir(path)
}

/// Undoes [`move_root`] after a later root failed to move. If the rename back
/// fails too, links the old path instead, so this launch (which keeps the old
/// identity) still reaches the data.
fn put_back(from: &Path, to: &Path) -> String {
    match fs::rename(to, from) {
        Ok(()) => {
            let _ = fs::remove_file(from.join(MARKER_FILE_NAME));
            format!("put {} back", from.display())
        }
        Err(err) => match link_and_clear_marker(from, to) {
            Ok(()) => format!(
                "could not put {} back ({err}); linked it to {} instead",
                from.display(),
                to.display()
            ),
            Err(link_err) => format!(
                "could not put {} back ({err}) or link it ({link_err}); its data is at {}",
                from.display(),
                to.display()
            ),
        },
    }
}

fn link_and_clear_marker(from: &Path, to: &Path) -> io::Result<()> {
    link(from, to)?;
    match fs::remove_file(to.join(MARKER_FILE_NAME)) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

/// Makes `from` lead to `to`.
#[cfg(unix)]
fn link(from: &Path, to: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(to, from)
}

/// Makes `from` lead to `to`. A junction, unlike a directory symlink, needs
/// neither administrator rights nor Developer Mode.
#[cfg(windows)]
fn link(from: &Path, to: &Path) -> io::Result<()> {
    let output = command::blocking::Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(from)
        .arg(to)
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "mklink /J failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

#[cfg(not(any(unix, windows)))]
fn link(_from: &Path, _to: &Path) -> io::Result<()> {
    Err(io::Error::other("links are not supported on this platform"))
}

struct SystemProbe {
    may_move: bool,
    check_processes: bool,
}

impl Probe for SystemProbe {
    fn other_instance_running(&self) -> Option<String> {
        if !self.may_move {
            return Some("this is not the app's own launch".to_owned());
        }
        if !self.check_processes {
            return None;
        }
        running_instance()
    }

    fn database_in_use(&self, database: &Path) -> bool {
        database_in_use(database)
    }
}

#[cfg(not(target_family = "wasm"))]
fn running_instance() -> Option<String> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

    let own_pid = sysinfo::get_current_pid().ok();
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true, /* remove_dead_processes */
        ProcessRefreshKind::nothing(),
    );
    system.processes().iter().find_map(|(pid, process)| {
        if Some(*pid) == own_pid {
            return None;
        }
        let name = process.name().to_string_lossy();
        let name = name.strip_suffix(".exe").unwrap_or(&name);
        EXECUTABLE_NAMES
            .contains(&name)
            .then(|| format!("{name} (pid {pid})"))
    })
}

#[cfg(target_family = "wasm")]
fn running_instance() -> Option<String> {
    None
}

/// Whether another process has the sqlite database at `database` open.
///
/// SQLite's unix VFS holds a read lock on the shared-memory file's "DMS" byte
/// for as long as a connection to a WAL database is open, and locks in the
/// database's lock-byte range during a rollback-journal transaction. `F_GETLK`
/// reports locks held by other processes without taking any.
#[cfg(unix)]
pub(crate) fn database_in_use(database: &Path) -> bool {
    const WAL_DMS_BYTE: libc::off_t = 128;
    const PENDING_BYTE: libc::off_t = 0x4000_0000;
    const LOCK_BYTES: libc::off_t = 512;

    let mut shm = database.as_os_str().to_owned();
    shm.push("-shm");
    byte_range_locked(Path::new(&shm), WAL_DMS_BYTE, 1)
        || byte_range_locked(database, PENDING_BYTE, LOCK_BYTES)
}

#[cfg(unix)]
fn byte_range_locked(path: &Path, start: libc::off_t, len: libc::off_t) -> bool {
    use std::os::fd::AsRawFd as _;

    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return false,
        // Unreadable: assume the worst and leave the data where it is.
        Err(_) => return true,
    };
    // SAFETY: `flock` is a plain C struct for which all-zeroes is valid.
    let mut lock: libc::flock = unsafe { std::mem::zeroed() };
    lock.l_type = libc::F_WRLCK as libc::c_short;
    lock.l_whence = libc::SEEK_SET as libc::c_short;
    lock.l_start = start;
    lock.l_len = len;
    // SAFETY: `F_GETLK` only reads and writes the `flock` we pass, and the
    // descriptor is valid for the duration of the call.
    let result = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETLK, &mut lock) };
    result == -1 || lock.l_type != libc::F_UNLCK as libc::c_short
}

/// Whether another process has the sqlite database at `database` open. SQLite
/// opens it shared, so an exclusive open fails while anyone else holds it.
#[cfg(windows)]
pub(crate) fn database_in_use(database: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt as _;

    match fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(database)
    {
        Ok(_) => false,
        Err(err) => err.kind() != io::ErrorKind::NotFound,
    }
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn database_in_use(_database: &Path) -> bool {
    false
}

/// The preferences domain for `app_id`, matching [`ChannelState::data_domain`].
#[cfg(target_os = "macos")]
fn data_domain(app_id: &AppId, data_profile: Option<&str>) -> String {
    match data_profile {
        Some(profile) => format!("{app_id}-{profile}"),
        None => app_id.to_string(),
    }
}

/// Copies the previous identity's registry preferences, which are keyed by
/// application name alone, to the current identity once.
#[cfg(windows)]
fn copy_registry_preferences(from: &str, to: &str, notes: &mut Vec<String>) {
    use uncaged_ui_extras::user_preferences::registry_backed;

    match registry_backed::copy_app_values_once(from, to, PREFERENCES_MARKER_KEY) {
        Ok(true) => notes.push(format!("copied registry preferences from {from} to {to}")),
        Ok(false) => {}
        Err(err) => notes.push(format!(
            "could not copy registry preferences from {from} to {to}: {err}"
        )),
    }
}

/// Copies the persistent defaults domain `from` into `to`, unless `to` records
/// that it already happened. Returns whether anything was copied.
#[cfg(target_os = "macos")]
fn copy_user_defaults(from: &str, to: &str) -> bool {
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSMutableDictionary, NSString, NSUserDefaults};

    let defaults = NSUserDefaults::standardUserDefaults();
    let to_name = NSString::from_str(to);
    let marker_key = NSString::from_str(PREFERENCES_MARKER_KEY);
    let existing = defaults.persistentDomainForName(&to_name);
    if existing
        .as_ref()
        .is_some_and(|domain| domain.objectForKey(&marker_key).is_some())
    {
        return false;
    }
    let Some(old) = defaults.persistentDomainForName(&NSString::from_str(from)) else {
        return false;
    };

    let merged = NSMutableDictionary::<NSString, AnyObject>::new();
    for domain in std::iter::once(&old).chain(existing.as_ref()) {
        let (keys, values) = domain.to_vecs();
        for (key, value) in keys.iter().zip(&values) {
            merged.insert(&**key, &**value);
        }
    }
    let marker_value = NSString::from_str(from);
    let marker_value: &AnyObject = &marker_value;
    merged.insert(&*marker_key, marker_value);
    // SAFETY: every key is an `NSString` and every value came out of a
    // persistent domain or is an `NSString`, all valid property-list types.
    unsafe { defaults.setPersistentDomain_forName(&merged, &to_name) };
    true
}

// The tests build directory layouts with unix symlinks.
#[cfg(all(test, unix))]
#[path = "identity_migration_tests.rs"]
mod tests;
