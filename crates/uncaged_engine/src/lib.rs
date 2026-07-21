//! # Uncaged Engine
//!
//! A local, bring-your-own-model agent harness for the Uncaged fork of the
//! open-source Warp client. It replaces Warp's proprietary server-side
//! inference (`/ai/multi-agent`) so the native Agent Mode UI is driven by the
//! user's own backend — a hosted API (Anthropic / OpenAI), any OpenAI-compatible
//! local server (Ollama, LM Studio, llama.cpp, vLLM), or a CLI agent over ACP.
//!
//! ## How it plugs in
//!
//! Warp's client funnels every Agent Mode turn through one function,
//! [`warp_multi_agent_client::generate_multi_agent_output`]. That function calls
//! [`active`]; if it returns a config, the request is handed to [`run_turn`]
//! instead of being POSTed to Warp's servers. The client's request/response
//! pipeline, native tool execution, and permission UI are all unchanged — only
//! the inference source moves onto the user's machine.
//!
//! ## What the engine does and doesn't own
//!
//! The engine is "inference + tool-call event protocol" only. Warp's client
//! already owns the full tool loop (it runs shell commands in the user's PTY,
//! reads/edits files, calls MCP servers, and feeds results back as the next
//! turn). So the engine just: parses the request into a neutral conversation,
//! authors a system prompt and tool schemas (which Warp's server normally keeps
//! private), calls the model, and streams the result back as the `ResponseEvent`
//! actions the UI renders.

mod catalog;
mod config;
mod connections;
mod engine;
mod model;
mod proto;
mod providers;
mod request_parse;
mod system_prompt;
mod tools;
mod wire;

#[cfg(test)]
mod connections_tests;
#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod tests;

pub use catalog::Group;
pub use catalog::PRESETS;
pub use catalog::Preset;
pub use catalog::Wire;
pub use catalog::preset_by_id;
pub use catalog::presets_in;
pub use config::ANTHROPIC_DEFAULT_BASE_URL;
pub use config::LMSTUDIO_DEFAULT_BASE_URL;
pub use config::OLLAMA_DEFAULT_BASE_URL;
pub use config::OPENAI_DEFAULT_BASE_URL;
pub use config::OPENROUTER_DEFAULT_BASE_URL;
pub use config::ProviderConfig;
pub use config::UncagedConfig;
pub use config::active;
pub use config::config_path;
pub use config::reload;
pub use config::save;
pub use connections::Connection;
pub use connections::Roster;
pub use connections::add as add_connection;
pub use connections::connect_or_focus;
pub use connections::load as load_roster;
pub use connections::load_or_seed as roster;
pub use connections::remove as remove_connection;
pub use connections::set_active as set_active_connection;
pub use connections::update as update_connection;
pub use engine::EngineStream;
pub use engine::run_turn;

/// Whether the local engine is currently active (enabled with a valid backend).
/// When `false`, callers should fall back to Warp's normal server path.
pub fn is_active() -> bool {
    config::active().is_some()
}

/// Convenience used at the seam: if the local engine is active, run the turn
/// locally and return its event stream; otherwise return `None` so the caller
/// uses Warp's server.
pub fn run_if_active(request: &warp_multi_agent_api::Request) -> Option<EngineStream> {
    let config = config::active()?;
    Some(engine::run_turn(&config, request))
}
