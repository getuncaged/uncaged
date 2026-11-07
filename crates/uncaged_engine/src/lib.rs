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
//! the app's `generate_multi_agent_output` (`app/src/ai/agent/api/impl.rs`). That
//! function calls [`run_if_active`]; if a model is connected, the request is handed to
//! [`run_turn`], otherwise the turn fails with a "connect a model" message. There is
//! no path to Warp's servers. The client's request/response
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
mod code_search;
mod config;
mod connections;
mod engine;
mod model;
mod models;
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

pub use catalog::{Group, PRESETS, Preset, Wire, preset_by_id, presets_in};
pub use code_search::{
    SearchFn as CodeSearchFn, SearchRequest as CodeSearchRequest, start as start_code_search_server,
};
pub use config::{
    ANTHROPIC_DEFAULT_BASE_URL, Effort, LMSTUDIO_DEFAULT_BASE_URL, OLLAMA_DEFAULT_BASE_URL,
    OPENAI_DEFAULT_BASE_URL, OPENROUTER_DEFAULT_BASE_URL, ProviderConfig, UncagedConfig, active,
    config_path, reload, save,
};
pub use connections::{
    Connection, Roster, add as add_connection, connect_or_focus, load as load_roster,
    load_or_seed as roster, remove as remove_connection, set_active as set_active_connection,
    set_effort as set_connection_effort, update as update_connection,
};
pub use engine::{EngineStream, run_turn};
pub use models::discover as discover_models;

/// Whether the local engine is currently active (enabled with a valid backend).
/// When `false`, callers should fall back to Warp's normal server path.
pub fn is_active() -> bool {
    config::active().is_some()
}

/// Convenience used at the seam: if the local engine is active, run the turn
/// locally and return its event stream; otherwise return `None` so the caller
/// uses Warp's server.
pub fn run_if_active(request: &uncaged_agent_api::Request) -> Option<EngineStream> {
    let config = config::active()?;
    Some(engine::run_turn(&config, request))
}
