//! The saved-connections roster.
//!
//! Uncaged's engine runs exactly one backend at a time (see
//! [`crate::config::UncagedConfig`]). But users want to keep several set up —
//! a local model for offline work, an API key for heavy lifting, a CLI login —
//! and flip between them. This module is that roster.
//!
//! It lives in its own file (`~/.uncaged/connections.json`) and is the source
//! of truth for the settings gallery. The **active** connection is *projected*
//! into `engine.json` via [`crate::config::save`], so the engine itself never
//! has to know the roster exists — it just reads the single active provider as
//! before. Nothing here talks to the network; keys stay on the user's machine.

use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use crate::catalog;
use crate::catalog::Preset;
use crate::catalog::Wire;
use crate::config;
use crate::config::ProviderConfig;
use crate::config::UncagedConfig;

const DEFAULT_MAX_TOKENS: u32 = 8192;

/// One saved, fully-editable connection. Seeded from a [`Preset`] but the user
/// can change every field afterwards.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    /// Stable per-connection id (also used as the roster key).
    pub id: String,
    /// The catalog preset this came from (for the card icon / re-seed).
    pub preset: String,
    /// User-facing name, e.g. "My Claude".
    pub label: String,
    /// Wire protocol: "anthropic" | "openai_compatible" | "cli".
    pub wire: String,
    /// API base URL (empty for CLI agents).
    #[serde(default)]
    pub base_url: String,
    /// API key, if this provider needs one. Stored locally only.
    #[serde(default)]
    pub api_key: Option<String>,
    /// The model id / name.
    #[serde(default)]
    pub model: String,
    /// argv for a CLI agent (empty for HTTP providers).
    #[serde(default)]
    pub cli_command: Vec<String>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// Whether this provider requires an API key (drives the "Needs key" badge).
    #[serde(default)]
    pub needs_key: bool,
    /// Whether this runs on the user's own machine (drives the "Local" badge).
    #[serde(default)]
    pub local: bool,
}

fn default_max_tokens() -> u32 {
    DEFAULT_MAX_TOKENS
}

fn wire_str(w: Wire) -> &'static str {
    match w {
        Wire::Anthropic => "anthropic",
        Wire::OpenAiCompatible => "openai_compatible",
        Wire::Cli => "cli",
    }
}

impl Connection {
    /// Build a fresh connection from a catalog preset.
    pub fn from_preset(preset: &Preset, id: String) -> Self {
        Connection {
            id,
            preset: preset.id.to_string(),
            label: preset.label.to_string(),
            wire: wire_str(preset.wire).to_string(),
            base_url: preset.base_url.to_string(),
            api_key: None,
            model: preset.model.to_string(),
            cli_command: preset.cli_command.iter().map(|s| s.to_string()).collect(),
            max_tokens: DEFAULT_MAX_TOKENS,
            needs_key: preset.needs_key,
            local: preset.local,
        }
    }

    /// Is this connection fully configured enough to run?
    pub fn usable(&self) -> bool {
        match self.wire.as_str() {
            "cli" => !self.cli_command.is_empty(),
            wire => {
                if self.model.trim().is_empty() {
                    return false;
                }
                // Anthropic falls back to its public API endpoint when the base
                // URL is blank; an OpenAI-compatible server has no such default.
                if wire != "anthropic" && self.base_url.trim().is_empty() {
                    return false;
                }
                if self.needs_key && self.api_key.as_deref().unwrap_or("").trim().is_empty() {
                    return false;
                }
                true
            }
        }
    }

    /// A short status word for the connection row.
    pub fn status(&self) -> &'static str {
        if self.usable() {
            "Ready"
        } else if self.needs_key && self.api_key.as_deref().unwrap_or("").trim().is_empty() {
            "Needs key"
        } else {
            "Incomplete"
        }
    }

    /// Project this connection onto the engine's single-provider config shape.
    pub fn to_provider(&self) -> ProviderConfig {
        match self.wire.as_str() {
            "anthropic" => ProviderConfig::Anthropic {
                api_key: self.api_key.clone().unwrap_or_default(),
                model: self.model.clone(),
                base_url: if self.base_url.trim().is_empty() {
                    config::ANTHROPIC_DEFAULT_BASE_URL.to_string()
                } else {
                    self.base_url.clone()
                },
                max_tokens: self.max_tokens,
            },
            "cli" => ProviderConfig::Acp {
                command: self.cli_command.clone(),
                model: Some(self.model.clone()).filter(|m| !m.trim().is_empty()),
            },
            // openai_compatible and anything else.
            _ => ProviderConfig::OpenAiCompatible {
                base_url: self.base_url.clone(),
                api_key: self.api_key.clone().filter(|k| !k.trim().is_empty()),
                model: self.model.clone(),
                max_tokens: self.max_tokens,
                label: Some(self.label.clone()),
            },
        }
    }
}

/// The persisted roster: an ordered list plus the id of the active connection.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Roster {
    #[serde(default)]
    pub active_id: Option<String>,
    #[serde(default)]
    pub connections: Vec<Connection>,
}

impl Roster {
    /// The active connection, if any.
    pub fn active(&self) -> Option<&Connection> {
        let id = self.active_id.as_deref()?;
        self.connections.iter().find(|c| c.id == id)
    }

    fn get(&self, id: &str) -> Option<&Connection> {
        self.connections.iter().find(|c| c.id == id)
    }

    /// Drop `id`. If it was active, the first remaining usable connection (or
    /// the first remaining, or none) becomes active.
    fn remove_and_repoint(&mut self, id: &str) {
        self.connections.retain(|c| c.id != id);
        if self.active_id.as_deref() == Some(id) {
            self.active_id = self
                .connections
                .iter()
                .find(|c| c.usable())
                .or_else(|| self.connections.first())
                .map(|c| c.id.clone());
        }
    }

    /// A readable, unique id for a new connection seeded from `base`:
    /// "custom", "custom-2", "custom-3", ...
    fn unique_id(&self, base: &str) -> String {
        let base = if base.trim().is_empty() {
            "model"
        } else {
            base
        };
        first_free(base, "-", |candidate| self.get(candidate).is_some())
    }

    /// The first connection created from a given preset, if any.
    fn find_by_preset(&self, preset_id: &str) -> Option<&Connection> {
        self.connections.iter().find(|c| c.preset == preset_id)
    }

    /// A unique default label for a new connection seeded from `base`, so
    /// repeated presets (e.g. several custom endpoints) stay distinguishable:
    /// "Custom", "Custom 2", "Custom 3", ...
    fn unique_label(&self, base: &str) -> String {
        first_free(base, " ", |candidate| {
            self.connections.iter().any(|c| c.label == candidate)
        })
    }

    /// Collapse duplicate connections that share a preset — the active one wins,
    /// else the first seen — preserving order and `active_id`. Presets that
    /// [allow multiple](catalog::allows_multiple) connections (`custom`) are
    /// exempt: multiple distinct custom endpoints are legitimate. Returns
    /// whether any duplicate was removed. This repairs rosters dirtied before the
    /// one-connection-per-preset rule existed.
    fn dedupe_by_preset(&mut self) -> bool {
        use std::collections::HashMap;
        let active = self.active_id.clone();
        // Decide which id to keep for each non-custom preset (active wins).
        let mut keep: HashMap<String, String> = HashMap::new();
        for c in &self.connections {
            if catalog::allows_multiple(&c.preset) {
                continue;
            }
            let is_active = active.as_deref() == Some(c.id.as_str());
            if is_active || !keep.contains_key(&c.preset) {
                keep.insert(c.preset.clone(), c.id.clone());
            }
        }
        let before = self.connections.len();
        self.connections.retain(|c| {
            catalog::allows_multiple(&c.preset)
                || keep.get(&c.preset).map(String::as_str) == Some(c.id.as_str())
        });
        // Defensive: if the active id was somehow dropped, repoint to a survivor.
        if let Some(a) = &active {
            if !self.connections.iter().any(|c| &c.id == a) {
                self.active_id = self
                    .connections
                    .iter()
                    .find(|c| c.usable())
                    .or_else(|| self.connections.first())
                    .map(|c| c.id.clone());
            }
        }
        self.connections.len() != before
    }
}

/// `base` if it isn't `taken`, else the first free `{base}{sep}{n}` for n >= 2.
fn first_free(base: &str, sep: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}{sep}{n}"))
        .find(|candidate| !taken(candidate))
        .expect("an unbounded range always has a free candidate")
}

/// Whether `conn` is still exactly what [`add`] seeds from `preset` — e.g. the
/// row onboarding's Connect leaves to be finished in Settings. Any edit (a URL,
/// model, key, command or different wire) makes it a real connection that must
/// never be handed out as a blank draft. The label and id are ignored: `add`
/// already makes those unique.
fn is_unedited_draft(conn: &Connection, preset: &Preset) -> bool {
    let seed = Connection::from_preset(preset, String::new());
    conn.preset == seed.preset
        && conn.wire == seed.wire
        && conn.base_url == seed.base_url
        && conn.model == seed.model
        && conn.api_key.is_none()
        && conn.cli_command == seed.cli_command
}

/// Whether two base URLs point at the same server: equal parsed origins
/// (scheme, host and port, with default ports normalized), using the same URL
/// parser as the HTTP client. A URL that doesn't parse, or has no host-based
/// origin, never matches — so a stored key is dropped rather than risked.
fn same_server(a: &str, b: &str) -> bool {
    let origin = |url: &str| reqwest::Url::parse(url.trim()).ok().map(|u| u.origin());
    match (origin(a), origin(b)) {
        (Some(a), Some(b)) => a.is_tuple() && a == b,
        _ => false,
    }
}

/// The on-disk roster path (`$UNCAGED_CONNECTIONS` or
/// `~/.uncaged/connections.json`).
pub fn roster_path() -> PathBuf {
    if let Ok(explicit) = std::env::var("UNCAGED_CONNECTIONS") {
        return PathBuf::from(explicit);
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".uncaged")
        .join("connections.json")
}

/// Read the roster from disk (missing / unparseable → empty).
pub fn load() -> Roster {
    let path = roster_path();
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return Roster::default();
    };
    match serde_json::from_str::<Roster>(&contents) {
        Ok(r) => r,
        Err(err) => {
            tracing::warn!("uncaged: failed to parse {}: {err}", path.display());
            Roster::default()
        }
    }
}

/// Write the roster atomically: the new contents go to a temporary file next
/// to it, which is then renamed over it. A write that fails part-way leaves the
/// previous roster intact instead of a truncated one.
///
/// A symlinked roster is written through to its target (the link is kept), and
/// on Unix the new file keeps the current roster's permissions: it holds API
/// keys, so a roster restricted to its owner stays that way.
fn write(roster: &Roster) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(roster)?;
    let path = write_target(roster_path());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = roster_temp_path(&path);
    // Only a staging file this call opened is cleaned up on failure.
    let file = std::fs::File::create(&tmp)?;
    if let Err(err) = stage(file, &path, &json).and_then(|()| std::fs::rename(&tmp, &path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err.into());
    }
    Ok(())
}

/// Fill (and close) the staging file for [`write`]. On Unix it first gets the
/// current roster's permissions, so keys are never readable more widely than
/// they were.
fn stage(mut file: std::fs::File, current: &Path, json: &str) -> std::io::Result<()> {
    if cfg!(unix)
        && let Ok(meta) = std::fs::metadata(current)
    {
        file.set_permissions(meta.permissions())?;
    }
    file.write_all(json.as_bytes())?;
    file.sync_all()
}

/// The file [`write`] replaces: the roster path, or the file it links to when
/// it's a symlink (so the rename updates the target instead of replacing the
/// link). A broken link falls back to the path itself.
fn write_target(path: PathBuf) -> PathBuf {
    let is_link = std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink());
    if is_link {
        std::fs::canonicalize(&path).unwrap_or(path)
    } else {
        path
    }
}

/// The temporary file [`write`] stages the roster in: same directory (so the
/// rename stays on one filesystem), named per process so two app instances
/// saving at once don't share it. Roster writes in one process all run on the
/// UI thread, so they never share it either.
pub(crate) fn roster_temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "connections.json".to_string());
    path.with_file_name(format!(".{name}.{}.tmp", std::process::id()))
}

/// Read the roster, first-run-seeding it from an existing `engine.json` so a
/// setup made by hand (or by the `uncaged-setup` script) shows up in the UI.
/// Never rewrites `engine.json`.
pub fn load_or_seed() -> Roster {
    let mut roster = load();
    if !roster.connections.is_empty() {
        // Self-heal any pre-existing duplicates (one connection per preset).
        if roster.dedupe_by_preset() {
            let _ = write(&roster);
            let _ = project(&roster);
        }
        return roster;
    }
    if let Some(existing) = config::read_persisted() {
        let conn = connection_from_config(&existing, &roster);
        roster.active_id = Some(conn.id.clone());
        roster.connections.push(conn);
        let _ = write(&roster);
    }
    roster
}

/// Add a new connection from a catalog preset. Becomes active if it's the first
/// one. Returns the new connection's id once the roster is written.
pub fn add(preset_id: &str) -> anyhow::Result<String> {
    add_row(preset_id, true)
}

/// Append a fresh row seeded from `preset_id` and return its id. With
/// `activate_first`, a row added to an empty roster becomes active (and is
/// projected); without it the active connection is never touched and
/// `engine.json` isn't rewritten — for rows a save is about to fill in, which
/// the caller activates only once they can run.
fn add_row(preset_id: &str, activate_first: bool) -> anyhow::Result<String> {
    let preset = catalog::preset_by_id(preset_id);
    let mut roster = load_or_seed();
    let id = roster.unique_id(preset.id);
    let mut conn = Connection::from_preset(preset, id.clone());
    conn.label = roster.unique_label(&conn.label);
    let activate = activate_first && roster.connections.is_empty();
    roster.connections.push(conn);
    if activate {
        roster.active_id = Some(id.clone());
    }
    write(&roster)?;
    // `engine.json` only mirrors the active connection, so it only needs
    // rewriting when this row became active. (Otherwise a projection failure
    // would report an error for a row that was in fact added.)
    if activate {
        project(&roster)?;
    }
    Ok(id)
}

/// Connect a preset, but never create a duplicate: if a connection for that
/// preset already exists, make it active and return its id instead of adding a
/// second row. Onboarding's Connect buttons call this, as does Settings'
/// one-click Connect for presets that are ready as seeded (local runtimes, CLI
/// agents); Settings' editor-based Connect adds nothing until Save (see
/// [`claim_for_preset`]).
///
/// Presets that [allow multiple](catalog::allows_multiple) connections
/// (`custom`) add a new connection each time — except that an untouched draft
/// (see [`is_unedited_draft`]) is returned, without activating it, so repeated
/// Connect clicks in onboarding don't pile up blank rows.
pub fn connect_or_focus(preset_id: &str) -> anyhow::Result<String> {
    let roster = load_or_seed();
    if catalog::allows_multiple(preset_id) {
        let preset = catalog::preset_by_id(preset_id);
        if let Some(draft) = roster
            .connections
            .iter()
            .find(|c| is_unedited_draft(c, preset))
        {
            return Ok(draft.id.clone());
        }
    } else if let Some(existing) = roster.find_by_preset(preset_id).map(|c| c.id.clone()) {
        set_active(&existing)?;
        return Ok(existing);
    }
    add(preset_id)
}

/// Whether `id` is an untouched draft (see [`is_unedited_draft`]) of a preset
/// that [allows multiple](catalog::allows_multiple) connections — i.e. a row
/// another editor (or a new connection's save) could also pick up.
pub fn is_draft(id: &str) -> bool {
    load_or_seed().get(id).is_some_and(|c| {
        catalog::allows_multiple(&c.preset)
            && is_unedited_draft(c, catalog::preset_by_id(&c.preset))
    })
}

/// For an editor that was opened on a draft of `preset_id`: return `id` if it
/// is still an untouched draft, otherwise add a fresh connection from the same
/// preset and return that. Two editors on the same draft (e.g. Edit on a blank
/// row onboarding left, in two windows) therefore save to separate rows instead
/// of the second overwriting the first. A fresh row isn't made active; the
/// caller activates it once the save has made it usable.
pub fn claim_draft(id: &str, preset_id: &str) -> anyhow::Result<String> {
    let roster = load_or_seed();
    let still_draft = roster
        .get(id)
        .is_some_and(|c| is_unedited_draft(c, catalog::preset_by_id(preset_id)));
    if still_draft {
        Ok(id.to_string())
    } else {
        add_row(preset_id, false)
    }
}

/// The row a save from a new-connection editor (opened by Connect, with nothing
/// added to the roster yet) should write to, and whether it was just added:
///
/// - a preset that [allows multiple](catalog::allows_multiple) connections
///   reuses an untouched draft (e.g. one onboarding left), else adds a row;
/// - any other preset reuses its existing connection (another window may have
///   connected it since), else adds one. The save then overwrites that row
///   with what its form holds: last save wins, as with two Edit editors.
///
/// Unlike [`connect_or_focus`] this never changes the active connection and
/// never rewrites `engine.json`, even on an empty roster: the caller activates
/// the row once the save has made it usable.
pub fn claim_for_preset(preset_id: &str) -> anyhow::Result<(String, bool)> {
    let roster = load_or_seed();
    let existing = if catalog::allows_multiple(preset_id) {
        let preset = catalog::preset_by_id(preset_id);
        roster
            .connections
            .iter()
            .find(|c| is_unedited_draft(c, preset))
    } else {
        roster.find_by_preset(preset_id)
    };
    match existing {
        Some(conn) => Ok((conn.id.clone(), false)),
        None => Ok((add_row(preset_id, false)?, true)),
    }
}

/// What [`update`] did besides writing the given fields.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateOutcome {
    /// The stored API key was cleared because the URL moved to a different
    /// server and no new key was entered.
    pub key_cleared: bool,
    /// The edit was saved to the roster, but mirroring the active connection
    /// into `engine.json` then failed with this error.
    pub engine_error: Option<String>,
}

/// Overwrite the editable fields of a connection.
///
/// Returns `Err` when the edit wasn't saved: the connection doesn't exist, the
/// roster couldn't be written (the previous roster is left intact), or the
/// edit was refused. (Reading the roster can still repair it on disk; see
/// [`load_or_seed`].) Once the roster is written the edit stands, and a failure
/// to update `engine.json` is reported in [`UpdateOutcome::engine_error`].
///
/// Refuses (writing nothing) to move a connection whose provider needs a key
/// to a different server without a new key: the stored key isn't carried
/// over, so the result couldn't run — and if it is the active connection,
/// writing it would stop the running model.
pub fn update(
    id: &str,
    label: String,
    base_url: String,
    model: String,
    api_key: Option<String>,
    cli_command: Vec<String>,
) -> anyhow::Result<UpdateOutcome> {
    let mut roster = load_or_seed();
    let mut outcome = UpdateOutcome::default();
    if let Some(conn) = roster.connections.iter_mut().find(|c| c.id == id) {
        conn.label = label;
        conn.model = model;
        // A CLI agent is customized through its command line (its editable lever),
        // not a URL/key: store the edited argv and keep base_url empty. An HTTP
        // provider is the mirror image — the URL is its base_url and it has no
        // command. Routing by the connection's own wire keeps each path from
        // clobbering the other's field.
        if conn.wire == "cli" {
            conn.cli_command = cli_command;
        } else {
            // A stored key belongs to the server it was entered for. If the URL
            // now points at a different server, drop it rather than send it
            // there; a key typed alongside the new URL is still kept below.
            let effective = |url: &str| {
                if url.trim().is_empty() && conn.wire == "anthropic" {
                    config::ANTHROPIC_DEFAULT_BASE_URL.to_string()
                } else {
                    url.to_string()
                }
            };
            let base_url = base_url.trim().to_string();
            let keep_stored_key = matches!(&api_key, Some(k) if k.trim().is_empty());
            // An unchanged URL is always the same server, even one that doesn't
            // parse; only an edited URL is compared by origin.
            let key_cleared = keep_stored_key
                && conn.api_key.is_some()
                && base_url != conn.base_url.trim()
                && !same_server(&effective(&conn.base_url), &effective(&base_url));
            if key_cleared && conn.needs_key {
                // Nothing has been written yet; bail before the roster is saved.
                anyhow::bail!(
                    "Enter the API key for the new server. A saved key is only sent to the \
                     server it was entered for."
                );
            }
            if key_cleared {
                conn.api_key = None;
                outcome.key_cleared = true;
            }
            conn.base_url = base_url;
            // A `Some("")` from a cleared field means "leave the stored key"; only
            // a non-empty value replaces it, and `None` is an explicit clear.
            match api_key {
                Some(k) if !k.trim().is_empty() => conn.api_key = Some(k),
                Some(_) => {}
                None => conn.api_key = None,
            }
        }
    } else {
        // A row removed since its editor opened (e.g. in another window) can't
        // be saved onto: say so, writing nothing, rather than report success
        // for an edit that went nowhere.
        anyhow::bail!(
            "This connection no longer exists. It may have been removed in another window."
        );
    }
    write(&roster)?;
    // The edit is saved from here on, so a failure to mirror it into
    // engine.json is reported in the outcome, not as an error: callers can tell
    // "nothing was written" (`Err`) from "saved, but the engine wasn't updated".
    if let Err(err) = project(&roster) {
        outcome.engine_error = Some(err.to_string());
    }
    Ok(outcome)
}

/// Remove a connection. If it was active, the first remaining usable connection
/// (or the first remaining, or none) becomes active.
pub fn remove(id: &str) -> anyhow::Result<()> {
    let mut roster = load_or_seed();
    roster.remove_and_repoint(id);
    write(&roster)?;
    project(&roster)?;
    Ok(())
}

/// Remove `id` only if it is still an untouched draft of its preset (see
/// [`is_unedited_draft`]): used to take back a row that a save added (see
/// [`claim_for_preset`]) when the save then failed before writing to it, so a
/// cancelled editor leaves nothing behind. A row that has been configured
/// (e.g. the save's write landed) is kept. Returns whether a row was removed;
/// nothing is written when none was. `engine.json` is rewritten only if the
/// removed row was the active one (a row added for a save never is).
pub fn discard_draft(id: &str) -> anyhow::Result<bool> {
    let mut roster = load_or_seed();
    let untouched = roster
        .get(id)
        .is_some_and(|c| is_unedited_draft(c, catalog::preset_by_id(&c.preset)));
    if !untouched {
        return Ok(false);
    }
    let was_active = roster.active_id.as_deref() == Some(id);
    roster.remove_and_repoint(id);
    write(&roster)?;
    // Rows added for a save are never made active, so `engine.json` normally
    // doesn't mention this one and needs no update. Only rewrite it when the
    // active connection actually changed.
    if was_active {
        project(&roster)?;
    }
    Ok(true)
}

/// Make a connection the active one and project it into `engine.json`.
pub fn set_active(id: &str) -> anyhow::Result<()> {
    let mut roster = load_or_seed();
    if roster.get(id).is_some() {
        roster.active_id = Some(id.to_string());
    }
    write(&roster)?;
    project(&roster)?;
    Ok(())
}

/// Sync `engine.json` to the roster's active connection: enable + write it if
/// it's usable, otherwise put the engine to sleep (so an unconfigured provider
/// doesn't half-activate).
fn project(roster: &Roster) -> anyhow::Result<()> {
    match roster.active() {
        Some(conn) if conn.usable() => config::save(&UncagedConfig {
            enabled: true,
            provider: conn.to_provider(),
        }),
        _ => config::disable(),
    }
}

/// Best-effort reconstruction of a roster [`Connection`] from an existing
/// engine config, matching a catalog preset by base URL when possible.
fn connection_from_config(cfg: &UncagedConfig, roster: &Roster) -> Connection {
    match &cfg.provider {
        ProviderConfig::Anthropic {
            api_key,
            model,
            base_url,
            max_tokens,
        } => Connection {
            id: roster.unique_id("anthropic"),
            preset: "anthropic".to_string(),
            label: "Anthropic (Claude)".to_string(),
            wire: "anthropic".to_string(),
            base_url: base_url.clone(),
            api_key: Some(api_key.clone()).filter(|k| !k.trim().is_empty()),
            model: model.clone(),
            cli_command: Vec::new(),
            max_tokens: *max_tokens,
            needs_key: true,
            local: false,
        },
        ProviderConfig::OpenAiCompatible {
            base_url,
            api_key,
            model,
            max_tokens,
            label,
        } => {
            let preset = match_openai_preset(base_url);
            Connection {
                id: roster.unique_id(preset.id),
                preset: preset.id.to_string(),
                label: label.clone().unwrap_or_else(|| preset.label.to_string()),
                wire: "openai_compatible".to_string(),
                base_url: base_url.clone(),
                api_key: api_key.clone().filter(|k| !k.trim().is_empty()),
                model: model.clone(),
                cli_command: Vec::new(),
                max_tokens: *max_tokens,
                needs_key: preset.needs_key,
                local: preset.local,
            }
        }
        ProviderConfig::Acp { command, model } => Connection {
            id: roster.unique_id("cli-agent"),
            preset: match_cli_preset(command).to_string(),
            label: command
                .first()
                .cloned()
                .unwrap_or_else(|| "CLI agent".to_string()),
            wire: "cli".to_string(),
            base_url: String::new(),
            api_key: None,
            model: model.clone().unwrap_or_default(),
            cli_command: command.clone(),
            max_tokens: DEFAULT_MAX_TOKENS,
            needs_key: false,
            local: true,
        },
    }
}

/// Guess which OpenAI-compatible preset a base URL corresponds to (for icon /
/// local flag), falling back to `custom`. `127.0.0.1` and `localhost` are
/// treated as the same host so a hand-written local config still matches.
fn match_openai_preset(base_url: &str) -> &'static Preset {
    let normalize = |u: &str| u.replace("127.0.0.1", "localhost");
    let target = normalize(base_url);
    catalog::PRESETS
        .iter()
        .filter(|p| matches!(p.wire, Wire::OpenAiCompatible) && !p.base_url.is_empty())
        .find(|p| {
            let preset_url = normalize(p.base_url);
            target.starts_with(preset_url.as_str()) || preset_url.starts_with(target.as_str())
        })
        .unwrap_or_else(|| catalog::preset_by_id("custom"))
}

/// Guess which CLI preset a command corresponds to, falling back to `custom`.
fn match_cli_preset(command: &[String]) -> &'static str {
    let first = command.first().map(String::as_str).unwrap_or("");
    catalog::PRESETS
        .iter()
        .filter(|p| matches!(p.wire, Wire::Cli))
        .find(|p| p.cli_command.first().copied().unwrap_or("") == first)
        .map(|p| p.id)
        .unwrap_or("custom")
}
