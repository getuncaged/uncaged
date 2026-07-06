//! Uncaged engine configuration.
//!
//! This is the single source of truth that decides (a) whether the local
//! engine is active at all, and (b) which backend powers it. The settings UI
//! writes this file; the engine reads it. Everything is intentionally simple
//! JSON so it can be edited by hand or by a few clicks in Settings.
//!
//! Resolution order:
//!   1. `$UNCAGED_CONFIG` if set, else `~/.uncaged/engine.json`.
//!   2. Environment overrides (handy for quick experiments / CI):
//!        - `UNCAGED_ENABLED=1|0`
//!        - `UNCAGED_PROVIDER=anthropic|openai|ollama|lmstudio|openrouter|openai_compatible|acp`
//!        - `UNCAGED_API_KEY=...`
//!        - `UNCAGED_MODEL=...`
//!        - `UNCAGED_BASE_URL=...`
//!        - `UNCAGED_ACP_COMMAND=...` (space-separated argv)

use std::path::PathBuf;
use std::sync::RwLock;

use serde::Deserialize;
use serde::Serialize;

/// The default Anthropic API base. Uncaged only talks to the public
/// Messages API; no Warp endpoint is involved.
pub const ANTHROPIC_DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
/// A sensible default for hosted OpenAI.
pub const OPENAI_DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
/// Ollama's OpenAI-compatible endpoint.
pub const OLLAMA_DEFAULT_BASE_URL: &str = "http://localhost:11434/v1";
/// LM Studio's OpenAI-compatible endpoint.
pub const LMSTUDIO_DEFAULT_BASE_URL: &str = "http://localhost:1234/v1";
/// OpenRouter's OpenAI-compatible endpoint.
pub const OPENROUTER_DEFAULT_BASE_URL: &str = "https://openrouter.ai/api/v1";

const DEFAULT_MAX_TOKENS: u32 = 8192;

/// Top-level engine configuration persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UncagedConfig {
    /// When false, the engine is dormant and Warp falls back to its normal
    /// server path (so a user can flip back to Warp's hosted agent at will).
    #[serde(default)]
    pub enabled: bool,

    /// The selected backend.
    pub provider: ProviderConfig,
}

/// The backend that powers Agent Mode.
///
/// `OpenAiCompatible` is deliberately the workhorse: OpenAI, OpenRouter,
/// Ollama, LM Studio, llama.cpp and vLLM all speak the same Chat Completions
/// dialect, so a single variant + base URL covers "any API" and "any local
/// model". `Anthropic` gets its own variant because the Messages API differs.
/// `Acp` delegates the whole agent loop to a CLI you already pay for.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ProviderConfig {
    #[serde(rename = "anthropic")]
    Anthropic {
        api_key: String,
        /// e.g. "claude-sonnet-4-5", "claude-opus-4-1".
        model: String,
        #[serde(default = "anthropic_base_url")]
        base_url: String,
        #[serde(default = "default_max_tokens")]
        max_tokens: u32,
    },
    /// Any OpenAI Chat Completions-compatible endpoint.
    #[serde(rename = "openai_compatible")]
    OpenAiCompatible {
        /// Full base URL including the `/v1` suffix, e.g.
        /// `https://api.openai.com/v1` or `http://localhost:11434/v1`.
        base_url: String,
        /// Optional — local servers (Ollama/LM Studio/llama.cpp) usually need no key.
        #[serde(default)]
        api_key: Option<String>,
        /// e.g. "gpt-4o", "llama3.1:8b", "qwen2.5-coder:32b".
        model: String,
        #[serde(default = "default_max_tokens")]
        max_tokens: u32,
        /// A human label purely for logging/telemetry ("openai", "ollama", ...).
        #[serde(default)]
        label: Option<String>,
    },
    /// Delegate to a CLI agent over the Agent Client Protocol (stdio JSON-RPC).
    /// e.g. command = ["claude-code-acp"] or ["gemini", "--experimental-acp"].
    #[serde(rename = "acp")]
    Acp {
        command: Vec<String>,
        #[serde(default)]
        model: Option<String>,
    },
}

fn anthropic_base_url() -> String {
    ANTHROPIC_DEFAULT_BASE_URL.to_string()
}
fn default_max_tokens() -> u32 {
    DEFAULT_MAX_TOKENS
}

impl UncagedConfig {
    /// A short human description of the active backend, for logs and the
    /// settings UI status line.
    pub fn describe(&self) -> String {
        match &self.provider {
            ProviderConfig::Anthropic { model, .. } => format!("Anthropic · {model}"),
            ProviderConfig::OpenAiCompatible {
                model,
                label,
                base_url,
                ..
            } => {
                let who = label.clone().unwrap_or_else(|| base_url.clone());
                format!("{who} · {model}")
            }
            ProviderConfig::Acp { command, .. } => {
                format!(
                    "CLI agent · {}",
                    command.first().cloned().unwrap_or_default()
                )
            }
        }
    }
}

/// The canonical on-disk config path (`$UNCAGED_CONFIG` or
/// `~/.uncaged/engine.json`).
pub fn config_path() -> PathBuf {
    if let Ok(explicit) = std::env::var("UNCAGED_CONFIG") {
        return PathBuf::from(explicit);
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".uncaged").join("engine.json")
}

static ACTIVE: RwLock<Option<UncagedConfig>> = RwLock::new(None);
static LOADED: RwLock<bool> = RwLock::new(false);

/// Returns the active engine config if (and only if) the engine is enabled and
/// has a valid backend. `None` means "let Warp's normal server path run".
pub fn active() -> Option<UncagedConfig> {
    {
        let loaded = *LOADED.read().unwrap();
        if !loaded {
            reload();
        }
    }
    ACTIVE.read().unwrap().clone()
}

/// Re-reads config from disk + environment. The settings UI calls this after
/// writing a new config so changes take effect without a restart.
pub fn reload() {
    let cfg = load_from_disk_and_env();
    *ACTIVE.write().unwrap() = cfg.filter(|c| c.enabled);
    *LOADED.write().unwrap() = true;
}

/// Persists a config to disk and refreshes the in-memory cache.
pub fn save(config: &UncagedConfig) -> anyhow::Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(config)?;
    std::fs::write(&path, json)?;
    reload();
    Ok(())
}

/// Read the persisted on-disk config (`engine.json`) as-is — ignoring
/// environment overrides and the `enabled` flag. The connections roster uses
/// this to seed itself from a setup made by hand or by the setup script.
pub fn read_persisted() -> Option<UncagedConfig> {
    read_file_config()
}

/// Put the engine to sleep by removing the on-disk config, then refresh the
/// in-memory cache. Used when the roster has no usable active connection so an
/// unconfigured provider never half-activates.
pub fn disable() -> anyhow::Result<()> {
    let path = config_path();
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    reload();
    Ok(())
}

fn load_from_disk_and_env() -> Option<UncagedConfig> {
    let mut config = read_file_config();

    // Environment overrides win over the file, so a quick `UNCAGED_*` export
    // can flip behavior without editing JSON.
    if let Some(env_cfg) = read_env_config() {
        config = Some(env_cfg);
    }

    // `UNCAGED_ENABLED` can force-enable/disable whatever was loaded.
    if let Ok(flag) = std::env::var("UNCAGED_ENABLED")
        && let Some(c) = config.as_mut()
    {
        c.enabled = matches!(flag.as_str(), "1" | "true" | "yes" | "on");
    }
    config
}

fn read_file_config() -> Option<UncagedConfig> {
    let path = config_path();
    let contents = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str::<UncagedConfig>(&contents) {
        Ok(cfg) => Some(cfg),
        Err(err) => {
            tracing::warn!("uncaged: failed to parse {}: {err}", path.display());
            None
        }
    }
}

fn read_env_config() -> Option<UncagedConfig> {
    let provider = std::env::var("UNCAGED_PROVIDER").ok()?;
    let api_key = std::env::var("UNCAGED_API_KEY").ok();
    let model = std::env::var("UNCAGED_MODEL").ok();
    let base_url = std::env::var("UNCAGED_BASE_URL").ok();

    let provider = match provider.as_str() {
        "anthropic" => ProviderConfig::Anthropic {
            api_key: api_key.unwrap_or_default(),
            model: model.unwrap_or_else(|| "claude-sonnet-4-5".to_string()),
            base_url: base_url.unwrap_or_else(anthropic_base_url),
            max_tokens: DEFAULT_MAX_TOKENS,
        },
        "openai" | "openrouter" | "ollama" | "lmstudio" | "openai_compatible" => {
            let default_base = match provider.as_str() {
                "openai" => OPENAI_DEFAULT_BASE_URL,
                "openrouter" => OPENROUTER_DEFAULT_BASE_URL,
                "ollama" => OLLAMA_DEFAULT_BASE_URL,
                "lmstudio" => LMSTUDIO_DEFAULT_BASE_URL,
                _ => OPENAI_DEFAULT_BASE_URL,
            };
            ProviderConfig::OpenAiCompatible {
                base_url: base_url.unwrap_or_else(|| default_base.to_string()),
                api_key,
                model: model.unwrap_or_else(|| "gpt-4o".to_string()),
                max_tokens: DEFAULT_MAX_TOKENS,
                label: Some(provider.clone()),
            }
        }
        "acp" => {
            let command = std::env::var("UNCAGED_ACP_COMMAND")
                .ok()?
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>();
            ProviderConfig::Acp { command, model }
        }
        _ => return None,
    };

    Some(UncagedConfig {
        enabled: true,
        provider,
    })
}
