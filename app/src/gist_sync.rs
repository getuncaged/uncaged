//! User-initiated, opt-in sync of the portable config + Drive bundle to a
//! **private** GitHub gist via the system `gh` CLI.
//!
//! This reuses [`crate::settings_backup`] to produce/consume the exact same
//! `.tgz` bundle used by local Back up / Restore — which already includes Drive
//! content (workflows, notebooks, themes, launch configs, snippets) per its
//! WHITELIST. Only non-secret items are ever archived.
//!
//! No token is ever stored: `gh` owns the GitHub credential. We persist only the
//! non-secret gist id (in `gist_sync.json` under the data dir, deliberately NOT
//! in the backup whitelist so it can never leak into an exported bundle). Sync
//! is always explicit — nothing is pushed or pulled automatically.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::settings_backup;

/// Stable name for the archive inside the gist. Using a fixed name means
/// `gh gist edit` replaces the file in place on every push (rather than
/// accumulating timestamped copies), and `pull` always finds the same file.
///
/// GitHub gists only accept **text** files — uploading a raw `.tgz` fails with
/// "binary file not supported". So we base64-encode the archive into this
/// `.base64` text file for the gist and decode it back to a `.tgz` on pull.
const GIST_ARCHIVE_NAME: &str = "uncaged-config-backup.tgz.base64";

/// Legacy suffix for the pre-base64 raw archive, so a `pull` can still read a
/// gist that predates the base64 change (should be rare — those uploads failed).
const LEGACY_ARCHIVE_SUFFIX: &str = ".tgz";

/// The small, non-secret record of which gist we sync to. Stored at
/// `data_dir()/gist_sync.json`. NOT part of the backup whitelist.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GistSyncState {
    gist_id: String,
    updated_at: u64,
    /// When true, "Sync to gist" pushes without the confirmation prompt. Opt-in;
    /// defaults to false so an outbound upload always asks first.
    #[serde(default)]
    auto_sync: bool,
}

/// Whether automated (no-confirmation) gist sync is turned on.
pub fn auto_sync_enabled() -> bool {
    read_state().map(|s| s.auto_sync).unwrap_or(false)
}

/// Turns automated gist sync on/off, preserving any existing gist id.
pub fn set_auto_sync(enabled: bool) {
    let mut state = read_state().unwrap_or(GistSyncState {
        gist_id: String::new(),
        updated_at: now_secs(),
        auto_sync: false,
    });
    state.auto_sync = enabled;
    if let Ok(json) = serde_json::to_string_pretty(&state) {
        let _ = std::fs::write(state_path(), json);
    }
}

fn state_path() -> PathBuf {
    warp_core::paths::data_dir().join("gist_sync.json")
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn read_state() -> Option<GistSyncState> {
    let contents = std::fs::read_to_string(state_path()).ok()?;
    serde_json::from_str(&contents).ok()
}

fn write_state(gist_id: &str) -> Result<()> {
    let auto_sync = read_state().map(|s| s.auto_sync).unwrap_or(false);
    let state = GistSyncState {
        gist_id: gist_id.to_string(),
        updated_at: now_secs(),
        auto_sync,
    };
    let json = serde_json::to_string_pretty(&state).context("failed to serialize gist_sync state")?;
    std::fs::write(state_path(), json).context("failed to write gist_sync.json")?;
    Ok(())
}

/// Shell out to `gh`, capturing stdout. Mirrors the piped-stdio pattern in
/// `util/git.rs::run_gh_command`. On failure, returns an actionable error
/// (not-logged-in, gh missing, generic).
async fn gh(args: &[&str], path_env: Option<&str>) -> Result<String> {
    use command::r#async::Command;
    use command::Stdio;

    log::debug!("[GIST SYNC] gist_sync.rs gh {}", args.join(" "));

    let mut cmd = Command::new("gh");
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("HOMEBREW_NO_AUTO_UPDATE", "1")
        .kill_on_drop(true);
    if let Some(path_env) = path_env {
        cmd.env("PATH", path_env);
    }

    let output = match cmd.output().await {
        Ok(output) => output,
        Err(e) => {
            // Most commonly: the `gh` binary isn't on PATH.
            if e.kind() == std::io::ErrorKind::NotFound {
                bail!("Install the GitHub CLI (brew install gh) to sync to a gist.");
            }
            return Err(anyhow!("Failed to execute gh: {e}"));
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        return Ok(stdout);
    }

    let combined = format!("{stderr}{stdout}").to_lowercase();
    if combined.contains("not logged in")
        || combined.contains("gh auth")
        || combined.contains("authentication")
        || combined.contains("to authenticate")
    {
        bail!("Not logged in to GitHub. Run `gh auth login` in a terminal, then try again.");
    }
    if combined.contains("could not resolve host")
        || combined.contains("network")
        || combined.contains("timeout")
    {
        bail!("Couldn't reach GitHub. Check your network connection and try again.");
    }

    let detail = stderr.trim();
    if detail.is_empty() {
        bail!("gh command failed.");
    }
    bail!("gh command failed: {detail}");
}

/// Runs `gh auth status` and surfaces a clean, actionable error if the user is
/// not authenticated (or `gh` is missing).
pub async fn preflight(path_env: Option<&str>) -> Result<()> {
    gh(&["auth", "status"], path_env).await.map(|_| ())
}

/// Parses a gist id out of a `gh gist create` URL (the trailing path segment),
/// e.g. `https://gist.github.com/user/<id>` -> `<id>`.
fn gist_id_from_url(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/');
    let last = trimmed.rsplit('/').next()?;
    if last.is_empty() {
        None
    } else {
        Some(last.to_string())
    }
}

/// Reconstruct the gist URL from a stored id.
fn gist_url_from_id(gist_id: &str) -> String {
    format!("https://gist.github.com/{gist_id}")
}

/// Push the current config + Drive bundle to a private gist.
///
/// If a gist id is already stored, updates that gist in place; otherwise creates
/// a new **secret** gist and persists its id. Returns the gist URL on success.
pub async fn push(path_env: Option<&str>) -> Result<String> {
    preflight(path_env).await?;

    let exported = settings_backup::export_to_dir(&std::env::temp_dir())
        .context("failed to build the config backup bundle")?;
    // GitHub gists reject binary files, so base64-encode the `.tgz` into a text
    // file with a stable name (so `gh gist edit` overwrites in place instead of
    // accumulating copies, and `pull` always finds the same file).
    let tgz_bytes =
        std::fs::read(&exported).context("failed to read the config backup bundle")?;
    let _ = std::fs::remove_file(&exported);
    let encoded = base64::engine::general_purpose::STANDARD.encode(&tgz_bytes);
    let archive = std::env::temp_dir().join(GIST_ARCHIVE_NAME);
    std::fs::write(&archive, encoded)
        .context("failed to stage the encoded config backup bundle")?;
    let archive_str = archive
        .to_str()
        .ok_or_else(|| anyhow!("backup archive path is not valid UTF-8"))?;

    let result = if let Some(state) = read_state().filter(|s| !s.gist_id.is_empty()) {
        // Update the existing gist in place.
        gh(&["gist", "edit", &state.gist_id, archive_str], path_env).await?;
        // Refresh the persisted timestamp; id is unchanged.
        let _ = write_state(&state.gist_id);
        Ok(gist_url_from_id(&state.gist_id))
    } else {
        // Create a new gist. `gh gist create` makes a SECRET gist by default
        // (there is no `--secret` flag — `--public` would opt into a public one),
        // so we simply omit it to keep the backup private/unlisted.
        let stdout = gh(
            &[
                "gist",
                "create",
                "--desc",
                "Uncaged config + drive backup",
                archive_str,
            ],
            path_env,
        )
        .await?;

        let url = stdout
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with("https://"))
            .map(str::to_string)
            .ok_or_else(|| anyhow!("gh did not print a gist URL"))?;

        let gist_id = gist_id_from_url(&url)
            .ok_or_else(|| anyhow!("couldn't parse the gist id from the URL"))?;
        write_state(&gist_id)?;
        Ok(url)
    };

    // Best-effort cleanup of the temp bundle regardless of outcome.
    let _ = std::fs::remove_file(&archive);

    result
}

/// Pull the config + Drive bundle from the stored gist and import it (validates,
/// snapshots the current config, then extracts). Errors if nothing was pushed
/// yet.
pub async fn pull(path_env: Option<&str>) -> Result<()> {
    preflight(path_env).await?;

    let state = read_state().ok_or_else(|| anyhow!("No gist yet — push first."))?;

    // Discover the archive file name inside the gist. Prefer the stable name we
    // now write; fall back to any `.tgz` for gists created before that change.
    let files_out = gh(&["gist", "view", &state.gist_id, "--files"], path_env).await?;
    let file_names: Vec<&str> = files_out
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let file_name = file_names
        .iter()
        .find(|name| **name == GIST_ARCHIVE_NAME)
        .or_else(|| file_names.iter().find(|name| name.ends_with(".base64")))
        .or_else(|| {
            file_names
                .iter()
                .find(|name| name.ends_with(LEGACY_ARCHIVE_SUFFIX))
        })
        .ok_or_else(|| anyhow!("The gist doesn't contain a config backup archive."))?
        .to_string();

    // Fetch the archive contents. `gh gist view <id> -f <name>` prints the
    // file's bytes to stdout.
    let contents = gist_view_file_bytes(&state.gist_id, &file_name, path_env).await?;

    // Base64 text files (`.base64`) hold the encoded `.tgz`; decode them back to
    // bytes. A legacy raw `.tgz` file is used as-is.
    let tgz_bytes = if file_name.ends_with(".base64") {
        let text = String::from_utf8(contents)
            .context("downloaded gist file was not valid UTF-8")?;
        let cleaned: String = text.split_whitespace().collect();
        base64::engine::general_purpose::STANDARD
            .decode(cleaned.as_bytes())
            .context("failed to base64-decode the downloaded backup")?
    } else {
        contents
    };

    let dest: PathBuf = std::env::temp_dir().join("uncaged-config-backup.tgz");
    std::fs::write(&dest, &tgz_bytes).context("failed to write downloaded gist archive")?;

    let result = settings_backup::import_from(&dest)
        .context("failed to import the downloaded config bundle");

    let _ = std::fs::remove_file(&dest);
    result
}

/// Fetch a single gist file's raw bytes via `gh gist view <id> -f <name>`.
async fn gist_view_file_bytes(
    gist_id: &str,
    file_name: &str,
    path_env: Option<&str>,
) -> Result<Vec<u8>> {
    use command::r#async::Command;
    use command::Stdio;

    log::debug!("[GIST SYNC] gist_sync.rs gh gist view {gist_id} -f {file_name}");

    let mut cmd = Command::new("gh");
    cmd.args(["gist", "view", gist_id, "-f", file_name])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("HOMEBREW_NO_AUTO_UPDATE", "1")
        .kill_on_drop(true);
    if let Some(path_env) = path_env {
        cmd.env("PATH", path_env);
    }

    let output = match cmd.output().await {
        Ok(output) => output,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                bail!("Install the GitHub CLI (brew install gh) to sync to a gist.");
            }
            return Err(anyhow!("Failed to execute gh: {e}"));
        }
    };

    if output.status.success() {
        Ok(output.stdout)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("gh gist view failed: {}", stderr.trim());
    }
}
