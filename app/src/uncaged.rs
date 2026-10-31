//! Bridge to the local Uncaged engine.
//!
//! The `warp` app crate also compiles to wasm (the web-compiled terminal),
//! where `uncaged_engine` (tokio + native HTTP + filesystem) is not available.
//! This shim lets the settings UI, gate, and onboarding talk to the local
//! bring-your-own-model engine through plain-data view types on every target,
//! without sprinkling `cfg` at each call site.
//!
//! The view types are the contract the "Connect a model" settings gallery
//! renders: a grouped [catalog](catalog_sections) of connectable platforms and
//! the user's saved [connections](connections), one of which is active and
//! drives Agent Mode.

/// One connectable platform in the "Connect a model" gallery.
#[derive(Clone, Debug, Default)]
pub struct PresetView {
    pub id: String,
    pub label: String,
    pub blurb: String,
    /// "anthropic" | "openai_compatible" | "cli".
    pub wire: String,
    pub needs_key: bool,
    pub local: bool,
    pub key_url: String,
    pub note: Option<String>,
}

/// A gallery section grouping presets that connect the same way.
#[derive(Clone, Debug, Default)]
pub struct CatalogSection {
    pub title: String,
    pub subtitle: String,
    /// The badge shown on every card in this section ("Your login" / …).
    pub pill: String,
    pub presets: Vec<PresetView>,
}

/// A saved connection as the settings UI sees it (no secrets — only whether a
/// key is stored).
#[derive(Clone, Debug, Default)]
pub struct ConnectionView {
    pub id: String,
    pub preset: String,
    pub label: String,
    /// "anthropic" | "openai_compatible" | "cli".
    pub wire: String,
    pub base_url: String,
    /// A short host/endpoint label for the row subtitle.
    pub endpoint: String,
    pub model: String,
    /// The selectable models for this connection's platform (from the preset),
    /// for the inline model menu. Always includes the active `model` so the
    /// menu can show it even when the preset list is empty (locally-discovered
    /// or custom endpoints).
    pub models: Vec<String>,
    pub cli_command: Vec<String>,
    /// How hard the model thinks, as the engine's value ("low" … "max"); `None` is the
    /// model's own default.
    pub effort: Option<String>,
    /// The effort levels this connection's wire accepts, as `(value, label)`. Empty for a CLI
    /// agent, which picks its own.
    pub effort_levels: Vec<(String, String)>,
    /// Whether a key is stored (never the key itself).
    pub has_key: bool,
    pub needs_key: bool,
    pub local: bool,
    /// "Ready" | "Needs key" | "Incomplete".
    pub status: String,
    pub usable: bool,
    pub is_active: bool,
    pub note: Option<String>,
    pub key_url: String,
}

// ── provider branding ───────────────────────────────────────────────────────
// The one place the app maps a provider to its logo. The wider brand surface
// (name, palette, the [ ❯_ ] mark) lives in `app/src/brand.rs`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Model lists the providers reported, by connection id. Filled by [`discover_models`]; read
/// when building each [`ConnectionView`].
#[cfg_attr(target_family = "wasm", allow(dead_code))]
static DISCOVERED_MODELS: std::sync::LazyLock<
    Mutex<std::collections::HashMap<String, Vec<String>>>,
> = std::sync::LazyLock::new(Default::default);

use warp_core::ui::icons::Icon;

/// The vendor logo for a provider preset id, if it has one. `None` for
/// "custom"/unknown providers, which fall back to the Uncaged mark.
fn provider_logo(preset: &str) -> Option<Icon> {
    Some(match preset {
        "claude-code" | "anthropic" => Icon::ClaudeLogo,
        "gemini-cli" | "google" => Icon::GeminiLogo,
        "codex" | "openai" => Icon::OpenAILogo,
        "xai" => Icon::XLogo,
        "ollama" => Icon::OllamaLogo,
        "lmstudio" => Icon::LmStudioLogo,
        "openrouter" => Icon::OpenRouterLogo,
        "groq" => Icon::GroqLogo,
        "deepseek" => Icon::DeepSeekLogo,
        "mistral" => Icon::MistralLogo,
        "together" => Icon::TogetherLogo,
        _ => return None,
    })
}

/// The gallery/card icon for a preset — the vendor logo, or a generic glyph for
/// "custom".
pub fn preset_icon(preset: &str) -> Icon {
    provider_logo(preset).unwrap_or(Icon::Sliders)
}

// `active_provider_icon` is read on every AI-reply avatar render, but the roster
// lives on disk. Cache the result, invalidating whenever a connection is
// added / edited / activated / removed (all of which flow through this module).
static ROSTER_GENERATION: AtomicU64 = AtomicU64::new(0);
static ACTIVE_ICON_CACHE: Mutex<(u64, Option<Icon>)> = Mutex::new((u64::MAX, None));

fn invalidate_active_provider_icon() {
    ROSTER_GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// The logo of the platform currently answering, for the reply avatar. `None`
/// when the local engine isn't driving, or the active provider has no logo — in
/// both cases the caller falls back to the Uncaged mark.
pub fn active_provider_icon() -> Option<Icon> {
    let generation = ROSTER_GENERATION.load(Ordering::Relaxed);
    if let Ok(cache) = ACTIVE_ICON_CACHE.lock() {
        if cache.0 == generation {
            return cache.1;
        }
    }
    let icon = if engine_active() {
        connections()
            .into_iter()
            .find(|c| c.is_active)
            .and_then(|c| provider_logo(&c.preset))
    } else {
        None
    };
    if let Ok(mut cache) = ACTIVE_ICON_CACHE.lock() {
        *cache = (generation, icon);
    }
    icon
}

// ── native implementation ───────────────────────────────────────────────────
#[cfg(not(target_family = "wasm"))]
mod imp {
    use super::{CatalogSection, ConnectionView, PresetView};

    /// Whether the local Uncaged engine is configured and active.
    pub fn engine_active() -> bool {
        uncaged_engine::is_active()
    }

    fn wire_str(w: uncaged_engine::Wire) -> &'static str {
        match w {
            uncaged_engine::Wire::Anthropic => "anthropic",
            uncaged_engine::Wire::OpenAiCompatible => "openai_compatible",
            uncaged_engine::Wire::Cli => "cli",
        }
    }

    fn preset_view(p: &uncaged_engine::Preset) -> PresetView {
        PresetView {
            id: p.id.to_string(),
            label: p.label.to_string(),
            blurb: p.blurb.to_string(),
            wire: wire_str(p.wire).to_string(),
            needs_key: p.needs_key,
            local: p.local,
            key_url: p.key_url.to_string(),
            note: p.note.map(str::to_string),
        }
    }

    /// The full catalog, grouped into the three gallery sections.
    pub fn catalog_sections() -> Vec<CatalogSection> {
        uncaged_engine::Group::ORDER
            .iter()
            .map(|g| CatalogSection {
                title: g.title().to_string(),
                subtitle: g.subtitle().to_string(),
                pill: g.pill().to_string(),
                presets: uncaged_engine::presets_in(*g).map(preset_view).collect(),
            })
            .collect()
    }

    fn host_label(url: &str) -> String {
        let stripped = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .unwrap_or(url);
        let host = stripped.split('/').next().unwrap_or(stripped);
        if host.is_empty() {
            "no endpoint".to_string()
        } else {
            host.to_string()
        }
    }

    fn connection_view(c: &uncaged_engine::Connection, active_id: Option<&str>) -> ConnectionView {
        let preset = uncaged_engine::preset_by_id(&c.preset);
        let endpoint = if c.wire == "cli" {
            if c.cli_command.is_empty() {
                "no command".to_string()
            } else {
                c.cli_command.join(" ")
            }
        } else {
            host_label(&c.base_url)
        };
        // The menu's model list: the preset's known models, guaranteeing the
        // connection's current model is present (it may be a custom edit, or the
        // preset list may be empty for locally-discovered endpoints).
        let mut models: Vec<String> = preset.models.iter().map(|m| m.to_string()).collect();
        // Then whatever the provider itself reported serving, the last time it was asked.
        if let Some(discovered) = super::DISCOVERED_MODELS.lock().unwrap().get(&c.id) {
            for model in discovered {
                if !models.contains(model) {
                    models.push(model.clone());
                }
            }
        }
        if !c.model.trim().is_empty() && !models.iter().any(|m| m == &c.model) {
            models.insert(0, c.model.clone());
        }
        let effort_value = |effort: uncaged_engine::Effort| {
            serde_json::to_value(effort)
                .ok()
                .and_then(|value| value.as_str().map(str::to_string))
                .unwrap_or_default()
        };
        ConnectionView {
            id: c.id.clone(),
            preset: c.preset.clone(),
            label: c.label.clone(),
            wire: c.wire.clone(),
            base_url: c.base_url.clone(),
            endpoint,
            model: c.model.clone(),
            models,
            cli_command: c.cli_command.clone(),
            effort: c.effort.map(effort_value),
            effort_levels: uncaged_engine::Effort::levels_for_wire(&c.wire)
                .iter()
                .map(|&effort| (effort_value(effort), effort.label().to_string()))
                .collect(),
            has_key: c
                .api_key
                .as_deref()
                .map(|k| !k.trim().is_empty())
                .unwrap_or(false),
            needs_key: c.needs_key,
            local: c.local,
            status: c.status().to_string(),
            usable: c.usable(),
            is_active: active_id == Some(c.id.as_str()),
            note: preset.note.map(str::to_string),
            key_url: preset.key_url.to_string(),
        }
    }

    /// The user's saved connections (seeding from an existing `engine.json` the
    /// first time so a hand-made setup shows up).
    pub fn connections() -> Vec<ConnectionView> {
        let roster = uncaged_engine::roster();
        let active = roster.active_id.clone();
        roster
            .connections
            .iter()
            .map(|c| connection_view(c, active.as_deref()))
            .collect()
    }

    /// Create a connection from a catalog preset — or, if one already exists for
    /// that preset, switch to it instead of adding a duplicate. Returns the id.
    pub fn connect(preset_id: &str) -> Result<String, String> {
        let result = uncaged_engine::connect_or_focus(preset_id).map_err(|e| e.to_string());
        super::invalidate_active_provider_icon();
        result
    }

    /// Make a saved connection the active one that powers Agent Mode.
    pub fn activate(id: &str) -> Result<(), String> {
        let result = uncaged_engine::set_active_connection(id).map_err(|e| e.to_string());
        super::invalidate_active_provider_icon();
        result
    }

    /// Remove a saved connection.
    pub fn remove(id: &str) -> Result<(), String> {
        let result = uncaged_engine::remove_connection(id).map_err(|e| e.to_string());
        super::invalidate_active_provider_icon();
        result
    }

    /// Overwrite a connection's editable fields. For `api_key`, a
    /// `Some(non-empty)` replaces the stored key and `Some("")` keeps it.
    pub fn update(
        id: &str,
        label: String,
        base_url: String,
        model: String,
        api_key: Option<String>,
        cli_command: Vec<String>,
    ) -> Result<(), String> {
        let result =
            uncaged_engine::update_connection(id, label, base_url, model, api_key, cli_command)
                .map_err(|e| e.to_string());
        super::invalidate_active_provider_icon();
        result
    }
    /// Set how hard the connection's model thinks; `None` restores the model's default.
    pub fn set_effort(id: &str, effort: Option<&str>) -> Result<(), String> {
        let effort = effort
            .map(|value| {
                serde_json::from_value::<uncaged_engine::Effort>(serde_json::json!(value))
                    .map_err(|e| e.to_string())
            })
            .transpose()?;
        uncaged_engine::set_connection_effort(id, effort).map_err(|e| e.to_string())
    }
    /// Ask the connection's provider which models it serves and remember the answer, so
    /// [`connections`] lists them. Blocking: run it off the UI thread.
    pub fn discover_models(id: &str) -> Result<(), String> {
        let models = uncaged_engine::discover_models(id).map_err(|e| e.to_string())?;
        super::DISCOVERED_MODELS
            .lock()
            .unwrap()
            .insert(id.to_string(), models);
        Ok(())
    }
}

// ── wasm stubs (no local engine on the web build) ───────────────────────────

#[cfg(target_family = "wasm")]
mod imp {
    use super::{CatalogSection, ConnectionView};

    pub fn engine_active() -> bool {
        false
    }
    pub fn catalog_sections() -> Vec<CatalogSection> {
        Vec::new()
    }
    pub fn connections() -> Vec<ConnectionView> {
        Vec::new()
    }
    pub fn connect(_preset_id: &str) -> Result<String, String> {
        Err("The local engine is not available on this build.".to_string())
    }
    pub fn activate(_id: &str) -> Result<(), String> {
        Err("The local engine is not available on this build.".to_string())
    }
    pub fn remove(_id: &str) -> Result<(), String> {
        Err("The local engine is not available on this build.".to_string())
    }
    pub fn update(
        _id: &str,
        _label: String,
        _base_url: String,
        _model: String,
        _api_key: Option<String>,
        _cli_command: Vec<String>,
    ) -> Result<(), String> {
        Err("The local engine is not available on this build.".to_string())
    }
    pub fn set_effort(_id: &str, _effort: Option<&str>) -> Result<(), String> {
        Err("The local engine is not available on this build.".to_string())
    }
    pub fn discover_models(_id: &str) -> Result<(), String> {
        Err("The local engine is not available on this build.".to_string())
    }
}

pub use imp::{
    activate, catalog_sections, connect, connections, discover_models, engine_active, remove,
    set_effort, update,
};
