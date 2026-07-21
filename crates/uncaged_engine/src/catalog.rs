//! The catalog of model platforms Uncaged knows how to connect to.
//!
//! The settings gallery and the `uncaged-setup` script both render this list so
//! "connect a model" is one click (or one keystroke) plus — usually — one API
//! key, instead of hand-typing base URLs and model ids. Every field is only a
//! seed: once a connection is created from a preset the user can edit all of it.
//!
//! Three wire protocols cover the field, matching [`crate::config::ProviderConfig`]:
//!   - `Anthropic`      — the Anthropic Messages HTTP API (bring your own key).
//!   - `OpenAiCompatible` — any OpenAI `/chat/completions` endpoint. This is the
//!     workhorse: OpenAI, OpenRouter, Google (compat), Groq, local Ollama and
//!     LM Studio all speak it, so one variant + base URL covers "any API" and
//!     "any local model".
//!   - `Cli`            — a locally-installed agent CLI (Claude Code, Codex,
//!     Gemini CLI) driven over the Agent Client Protocol using ITS OWN login /
//!     subscription. No API key; Uncaged spawns the process.

/// Which of the three gallery sections a preset belongs to. Derived from the
/// wire + `local` flag so the UI never has to hard-code membership.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// "Use a local agent" — a CLI you've already signed in to. No key.
    LocalAgent,
    /// "Run a model locally" — Ollama / LM Studio on your own machine. No key.
    RunLocal,
    /// "Connect with an API key" — a hosted cloud provider.
    ApiKey,
}

impl Group {
    /// Section heading shown in the gallery.
    pub fn title(self) -> &'static str {
        match self {
            Group::LocalAgent => "Use a local agent",
            Group::RunLocal => "Run a model locally",
            Group::ApiKey => "Connect with an API key",
        }
    }

    /// One-line section blurb shown under the heading.
    pub fn subtitle(self) -> &'static str {
        match self {
            Group::LocalAgent => {
                "Drive a CLI you've already signed in to — no API key, your own subscription."
            }
            Group::RunLocal => "Open models on your own machine — fully private, no key.",
            Group::ApiKey => "Bring your own key from a cloud provider.",
        }
    }

    /// The short pill shown on each card in this section.
    pub fn pill(self) -> &'static str {
        match self {
            Group::LocalAgent => "Your login",
            Group::RunLocal => "On-device",
            Group::ApiKey => "API key",
        }
    }

    /// Stable order the gallery renders sections in.
    pub const ORDER: [Group; 3] = [Group::LocalAgent, Group::RunLocal, Group::ApiKey];
}

/// How a preset reaches its model. Mirrors [`crate::config::ProviderConfig`]'s
/// three shapes without pulling in the (heavier) config types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wire {
    Anthropic,
    OpenAiCompatible,
    Cli,
}

/// A single connectable platform. Static data — the roster copies the parts it
/// needs into an editable [`crate::connections::Connection`].
#[derive(Debug, Clone, Copy)]
pub struct Preset {
    /// Stable catalog key; also the connection's `preset` field.
    pub id: &'static str,
    /// Display name, e.g. "Anthropic (Claude)".
    pub label: &'static str,
    /// Short tagline for the gallery card.
    pub blurb: &'static str,
    pub wire: Wire,
    /// Default API base URL (OpenAI-compat: the `/v1` root; Anthropic: host).
    /// Empty for CLI agents (they use the local binary, not HTTP).
    pub base_url: &'static str,
    /// A sensible default model id. Editable after connecting.
    pub model: &'static str,
    /// The known selectable models for this platform, for the inline model
    /// menu. Empty for platforms whose models are discovered at runtime
    /// (local servers, custom endpoints) — the menu then just shows the
    /// connection's current `model`.
    pub models: &'static [&'static str],
    /// Whether an API key is required. Local servers and CLIs need none.
    pub needs_key: bool,
    /// True for anything that runs on the user's own machine (localhost server
    /// or a local CLI) — surfaced as a privacy-friendly badge.
    pub local: bool,
    /// Where to get a key / install the CLI (opened in the browser).
    pub key_url: &'static str,
    /// The argv used to spawn a CLI agent over ACP. Empty for HTTP providers.
    pub cli_command: &'static [&'static str],
    /// Optional extra guidance shown under the connect form.
    pub note: Option<&'static str>,
}

impl Preset {
    /// Which gallery section this preset belongs to.
    pub fn group(&self) -> Group {
        match self.wire {
            Wire::Cli => Group::LocalAgent,
            _ if self.local => Group::RunLocal,
            _ => Group::ApiKey,
        }
    }
}

/// The known platforms, roughly ordered within each group by how commonly a
/// developer already has it set up. `custom` is the escape hatch for any other
/// OpenAI-compatible server.
pub const PRESETS: &[Preset] = &[
    // ── Use a local agent (your own CLI login, no key) ──────────────────────
    Preset {
        id: "claude-code",
        label: "Claude Code",
        blurb: "Drive your installed `claude` CLI with its own login — no API key.",
        wire: Wire::Cli,
        base_url: "",
        model: "sonnet",
        models: &["default", "sonnet", "haiku", "opus[1m]"],
        needs_key: false,
        local: true,
        key_url: "https://docs.anthropic.com/en/docs/claude-code",
        cli_command: &["claude-code-acp"],
        note: Some(
            "Needs the Claude Code ACP adapter on your PATH (`npm i -g @zed-industries/claude-code-acp`) and the `claude` CLI signed in. Model names map to `claude --model`.",
        ),
    },
    Preset {
        id: "gemini-cli",
        label: "Gemini CLI",
        blurb: "Drive your installed `gemini` CLI with its Google login — no API key.",
        wire: Wire::Cli,
        base_url: "",
        model: "gemini-2.5-pro",
        models: &["gemini-2.5-pro", "gemini-2.5-flash"],
        needs_key: false,
        local: true,
        key_url: "https://github.com/google-gemini/gemini-cli",
        cli_command: &["gemini", "--experimental-acp"],
        note: Some("Needs the `gemini` CLI on your PATH and signed in."),
    },
    Preset {
        id: "codex",
        label: "Codex CLI",
        blurb: "Drive your installed `codex` CLI with its ChatGPT/OpenAI login — no API key.",
        wire: Wire::Cli,
        base_url: "",
        model: "",
        models: &[],
        needs_key: false,
        local: true,
        key_url: "https://github.com/openai/codex",
        cli_command: &["codex", "acp"],
        note: Some(
            "Needs a recent `codex` CLI on your PATH and signed in. If your version lacks ACP, point the command at a compatible adapter.",
        ),
    },
    // ── Run a model locally (localhost server, no key) ──────────────────────
    Preset {
        id: "ollama",
        label: "Ollama (local)",
        blurb: "Run open models on your own machine — fully private, no key.",
        wire: Wire::OpenAiCompatible,
        base_url: "http://localhost:11434/v1",
        model: "qwen2.5-coder",
        models: &[],
        needs_key: false,
        local: true,
        key_url: "https://ollama.com/download",
        cli_command: &[],
        note: Some("Start Ollama, then `ollama pull qwen2.5-coder`. Nothing leaves your machine."),
    },
    Preset {
        id: "lmstudio",
        label: "LM Studio (local)",
        blurb: "Local models via LM Studio's server — private, no key.",
        wire: Wire::OpenAiCompatible,
        base_url: "http://localhost:1234/v1",
        model: "local-model",
        models: &[],
        needs_key: false,
        local: true,
        key_url: "https://lmstudio.ai",
        cli_command: &[],
        note: Some(
            "Enable LM Studio's local server (Developer ▸ Start Server), then load a model.",
        ),
    },
    // ── Connect with an API key (hosted cloud) ──────────────────────────────
    Preset {
        id: "anthropic",
        label: "Anthropic (Claude)",
        blurb: "Claude Opus, Sonnet & Haiku — strongest for code & agentic work.",
        wire: Wire::Anthropic,
        base_url: "https://api.anthropic.com",
        model: "claude-sonnet-4-5",
        models: &["claude-opus-4-8", "claude-sonnet-4-5", "claude-haiku-4-5"],
        needs_key: true,
        local: false,
        key_url: "https://console.anthropic.com/settings/keys",
        cli_command: &[],
        note: None,
    },
    Preset {
        id: "openai",
        label: "OpenAI",
        blurb: "GPT-4o / o-series via the OpenAI API.",
        wire: Wire::OpenAiCompatible,
        base_url: "https://api.openai.com/v1",
        model: "gpt-4o",
        models: &["gpt-4o", "gpt-4o-mini", "o3-mini"],
        needs_key: true,
        local: false,
        key_url: "https://platform.openai.com/api-keys",
        cli_command: &[],
        note: None,
    },
    Preset {
        id: "openrouter",
        label: "OpenRouter",
        blurb: "One key, hundreds of models (Claude, GPT, Llama, Gemini, …).",
        wire: Wire::OpenAiCompatible,
        base_url: "https://openrouter.ai/api/v1",
        model: "anthropic/claude-sonnet-4-5",
        models: &[
            "anthropic/claude-sonnet-4-5",
            "anthropic/claude-opus-4-1",
            "openai/gpt-4o",
            "google/gemini-2.5-pro",
        ],
        needs_key: true,
        local: false,
        key_url: "https://openrouter.ai/keys",
        cli_command: &[],
        note: None,
    },
    Preset {
        id: "google",
        label: "Google Gemini",
        blurb: "Gemini 2.5 Pro & Flash via the OpenAI-compatible endpoint.",
        wire: Wire::OpenAiCompatible,
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        model: "gemini-2.5-pro",
        models: &["gemini-2.5-pro", "gemini-2.5-flash"],
        needs_key: true,
        local: false,
        key_url: "https://aistudio.google.com/apikey",
        cli_command: &[],
        note: None,
    },
    Preset {
        id: "groq",
        label: "Groq",
        blurb: "Open models at very low latency.",
        wire: Wire::OpenAiCompatible,
        base_url: "https://api.groq.com/openai/v1",
        model: "llama-3.3-70b-versatile",
        models: &["llama-3.3-70b-versatile", "llama-3.1-8b-instant"],
        needs_key: true,
        local: false,
        key_url: "https://console.groq.com/keys",
        cli_command: &[],
        note: None,
    },
    Preset {
        id: "deepseek",
        label: "DeepSeek",
        blurb: "DeepSeek-V3 chat & R1 reasoning.",
        wire: Wire::OpenAiCompatible,
        base_url: "https://api.deepseek.com",
        model: "deepseek-chat",
        models: &["deepseek-chat", "deepseek-reasoner"],
        needs_key: true,
        local: false,
        key_url: "https://platform.deepseek.com/api_keys",
        cli_command: &[],
        note: None,
    },
    Preset {
        id: "mistral",
        label: "Mistral",
        blurb: "Mistral & Codestral models.",
        wire: Wire::OpenAiCompatible,
        base_url: "https://api.mistral.ai/v1",
        model: "mistral-large-latest",
        models: &["mistral-large-latest", "codestral-latest"],
        needs_key: true,
        local: false,
        key_url: "https://console.mistral.ai/api-keys",
        cli_command: &[],
        note: None,
    },
    Preset {
        id: "xai",
        label: "xAI (Grok)",
        blurb: "Grok models from xAI.",
        wire: Wire::OpenAiCompatible,
        base_url: "https://api.x.ai/v1",
        model: "grok-3",
        models: &["grok-3", "grok-3-mini"],
        needs_key: true,
        local: false,
        key_url: "https://console.x.ai",
        cli_command: &[],
        note: None,
    },
    Preset {
        id: "together",
        label: "Together AI",
        blurb: "A broad catalog of open models.",
        wire: Wire::OpenAiCompatible,
        base_url: "https://api.together.xyz/v1",
        model: "meta-llama/Llama-3.3-70B-Instruct-Turbo",
        models: &[
            "meta-llama/Llama-3.3-70B-Instruct-Turbo",
            "Qwen/Qwen2.5-Coder-32B-Instruct",
        ],
        needs_key: true,
        local: false,
        key_url: "https://api.together.xyz/settings/api-keys",
        cli_command: &[],
        note: None,
    },
    Preset {
        id: "custom",
        label: "Custom (OpenAI-compatible)",
        blurb: "Any server that speaks /chat/completions.",
        wire: Wire::OpenAiCompatible,
        base_url: "",
        model: "",
        models: &[],
        needs_key: false,
        local: false,
        key_url: "",
        cli_command: &[],
        note: Some("Point the base URL at any OpenAI-compatible `/v1` endpoint."),
    },
];

/// Look a preset up by id, falling back to the `custom` escape hatch.
pub fn preset_by_id(id: &str) -> &'static Preset {
    PRESETS
        .iter()
        .find(|p| p.id == id)
        .unwrap_or_else(|| PRESETS.iter().find(|p| p.id == "custom").unwrap())
}

/// All presets belonging to a gallery section, in catalog order.
pub fn presets_in(group: Group) -> impl Iterator<Item = &'static Preset> {
    PRESETS.iter().filter(move |p| p.group() == group)
}
