//! Local backup & restore of the user's portable settings and workflows.
//!
//! A generic "export your settings to a file / import them back" capability —
//! the same thing many apps expose — wired into Settings. Only a WHITELIST of
//! non-secret, portable items is archived, so credentials (the engine's API
//! keys in `connections.json` / `engine.json`, MCP tokens) are never included.
//! A whitelist (not a blacklist) guarantees a new secret file can't be swept up
//! by accident. Uses the system `tar`; nothing leaves the machine.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};

/// Portable, non-secret items to include (only those that exist).
const WHITELIST: &[&str] = &[
    "settings.toml",
    "keybindings.yaml",
    "keybindings.yml",
    "themes",
    "workflows",
    "notebooks",
    "launch_configurations",
    "snippets",
];

/// The user config directory (e.g. `~/.uncaged`).
fn config_dir() -> PathBuf {
    warp_core::paths::data_dir()
}

fn existing_items(dir: &Path) -> Vec<String> {
    WHITELIST
        .iter()
        .filter(|item| dir.join(item).exists())
        .map(|s| (*s).to_string())
        .collect()
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Archive the portable settings into `dest_dir`, returning the archive path.
pub fn export_to_dir(dest_dir: &Path) -> Result<PathBuf> {
    let dir = config_dir();
    let items = existing_items(&dir);
    if items.is_empty() {
        bail!("Nothing to back up yet — no settings, themes, or workflows found.");
    }
    let out = dest_dir.join(format!("settings-backup-{}.tgz", timestamp()));

    let mut cmd = Command::new("tar");
    cmd.arg("-czf").arg(&out).arg("-C").arg(&dir);
    for item in &items {
        cmd.arg(item);
    }
    let status = cmd.status().context("failed to run tar")?;
    if !status.success() {
        bail!("tar exited with status {status}");
    }
    Ok(out)
}

/// Restore portable settings from `archive`, copying the current config dir to a
/// timestamped `.bak-<n>` sibling first. Files in the archive overwrite their
/// counterparts; anything not in the archive is left untouched.
pub fn import_from(archive: &Path) -> Result<()> {
    // Validate it is a gzip tar before touching anything.
    let check = Command::new("tar")
        .arg("-tzf")
        .arg(archive)
        .output()
        .context("failed to run tar")?;
    if !check.status.success() {
        bail!("That file is not a valid settings backup archive.");
    }

    let dir = config_dir();
    if dir.exists() {
        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("config");
        let bak = dir.with_file_name(format!("{name}.bak-{}", timestamp()));
        // Best-effort snapshot of the current config before overwriting.
        let _ = Command::new("cp").arg("-R").arg(&dir).arg(&bak).status();
    } else {
        std::fs::create_dir_all(&dir).ok();
    }

    let status = Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(&dir)
        .status()
        .context("failed to run tar")?;
    if !status.success() {
        bail!("tar extraction exited with status {status}");
    }
    Ok(())
}

/// Reveal a path in Finder (macOS).
pub fn reveal(path: &Path) {
    let _ = Command::new("open").arg("-R").arg(path).status();
}
