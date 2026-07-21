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

    /// A readable, unique id for a new connection seeded from `base`.
    fn unique_id(&self, base: &str) -> String {
        let base = if base.trim().is_empty() {
            "model"
        } else {
            base
        };
        if self.get(base).is_none() {
            return base.to_string();
        }
        (2..)
            .map(|n| format!("{base}-{n}"))
            .find(|candidate| self.get(candidate).is_none())
            .unwrap_or_else(|| base.to_string())
    }

    /// The first connection created from a given preset, if any.
    fn find_by_preset(&self, preset_id: &str) -> Option<&Connection> {
        self.connections.iter().find(|c| c.preset == preset_id)
    }

    /// Collapse duplicate connections that share a preset — the active one wins,
    /// else the first seen — preserving order and `active_id`. `custom` is exempt
    /// (multiple distinct custom endpoints are legitimate). Returns whether any
    /// duplicate was removed. This repairs rosters dirtied before the
    /// one-connection-per-preset rule existed.
    fn dedupe_by_preset(&mut self) -> bool {
        use std::collections::HashMap;
        let active = self.active_id.clone();
        // Decide which id to keep for each non-custom preset (active wins).
        let mut keep: HashMap<String, String> = HashMap::new();
        for c in &self.connections {
            if c.preset == "custom" {
                continue;
            }
            let is_active = active.as_deref() == Some(c.id.as_str());
            if is_active || !keep.contains_key(&c.preset) {
                keep.insert(c.preset.clone(), c.id.clone());
            }
        }
        let before = self.connections.len();
        self.connections.retain(|c| {
            c.preset == "custom" || keep.get(&c.preset).map(String::as_str) == Some(c.id.as_str())
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

fn write(roster: &Roster) -> anyhow::Result<()> {
    let path = roster_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(roster)?)?;
    Ok(())
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
/// one. Returns the new connection's id.
pub fn add(preset_id: &str) -> anyhow::Result<String> {
    let preset = catalog::preset_by_id(preset_id);
    let mut roster = load_or_seed();
    let id = roster.unique_id(preset.id);
    let conn = Connection::from_preset(preset, id.clone());
    let first = roster.connections.is_empty();
    roster.connections.push(conn);
    if first {
        roster.active_id = Some(id.clone());
    }
    write(&roster)?;
    project(&roster)?;
    Ok(id)
}

/// Connect a preset, but never create a duplicate: if a connection for that
/// preset already exists, make it active and return its id instead of adding a
/// second row. `custom` is exempt (each custom endpoint is distinct), so it
/// always adds. This is what the "Connect" buttons call, so re-clicking an
/// already-connected preset simply switches to it.
pub fn connect_or_focus(preset_id: &str) -> anyhow::Result<String> {
    if preset_id != "custom" {
        let roster = load_or_seed();
        if let Some(existing) = roster.find_by_preset(preset_id).map(|c| c.id.clone()) {
            set_active(&existing)?;
            return Ok(existing);
        }
    }
    add(preset_id)
}

/// Overwrite the editable fields of a connection.
pub fn update(
    id: &str,
    label: String,
    base_url: String,
    model: String,
    api_key: Option<String>,
    cli_command: Vec<String>,
) -> anyhow::Result<()> {
    let mut roster = load_or_seed();
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
            conn.base_url = base_url;
            // A `Some("")` from a cleared field means "leave the stored key"; only
            // a non-empty value replaces it, and `None` is an explicit clear.
            match api_key {
                Some(k) if !k.trim().is_empty() => conn.api_key = Some(k),
                Some(_) => {}
                None => conn.api_key = None,
            }
        }
    }
    write(&roster)?;
    project(&roster)?;
    Ok(())
}

/// Remove a connection. If it was active, the first remaining usable connection
/// (or the first remaining, or none) becomes active.
pub fn remove(id: &str) -> anyhow::Result<()> {
    let mut roster = load_or_seed();
    roster.connections.retain(|c| c.id != id);
    if roster.active_id.as_deref() == Some(id) {
        roster.active_id = roster
            .connections
            .iter()
            .find(|c| c.usable())
            .or_else(|| roster.connections.first())
            .map(|c| c.id.clone());
    }
    write(&roster)?;
    project(&roster)?;
    Ok(())
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
