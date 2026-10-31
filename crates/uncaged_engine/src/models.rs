//! Live model lists: ask a connection's provider which models it serves.
//!
//! The catalog's preset model lists are seeds, and local servers (LM Studio, Ollama) have none
//! at all -- what they serve is whatever the user has downloaded. Both the Anthropic Messages
//! API and every OpenAI-compatible server answer `GET …/models` with `{"data": [{"id": …}]}`,
//! so one request per connection gives the model picker the real list.

use std::time::Duration;

use serde_json::Value;

use crate::config;
use crate::connections;
use crate::connections::Connection;

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);

/// The model ids the connection `id` serves, in the provider's order. A CLI agent picks its own
/// model, so it lists none.
///
/// Blocking: call it off the UI thread, and outside a tokio runtime.
pub fn discover(id: &str) -> anyhow::Result<Vec<String>> {
    let roster = connections::load_or_seed();
    let Some(conn) = roster.get(id) else {
        anyhow::bail!("no connection {id}");
    };
    discover_for(conn)
}

fn discover_for(conn: &Connection) -> anyhow::Result<Vec<String>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(DISCOVERY_TIMEOUT)
        .build()?;
    let key = conn.api_key.as_deref().filter(|key| !key.trim().is_empty());
    let request = match conn.wire.as_str() {
        "cli" => return Ok(Vec::new()),
        "anthropic" => {
            let base = match conn.base_url.trim() {
                "" => config::ANTHROPIC_DEFAULT_BASE_URL,
                base => base,
            };
            let request = client
                .get(format!(
                    "{}/v1/models?limit=1000",
                    base.trim_end_matches('/')
                ))
                .header("anthropic-version", "2023-06-01");
            match key {
                Some(key) => request.header("x-api-key", key),
                None => request,
            }
        }
        _ => {
            let request = client.get(format!(
                "{}/models",
                conn.base_url.trim().trim_end_matches('/')
            ));
            match key {
                Some(key) => request.bearer_auth(key),
                None => request,
            }
        }
    };
    let body = request.send()?.error_for_status()?.text()?;
    Ok(parse_model_ids(&serde_json::from_str(&body)?))
}

/// The `id` of every entry in a `{"data": [...]}` model list.
pub(crate) fn parse_model_ids(body: &Value) -> Vec<String> {
    body["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| model["id"].as_str())
        .map(str::to_string)
        .collect()
}
