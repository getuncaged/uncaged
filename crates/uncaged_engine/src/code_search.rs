//! Uncaged's codebase search, served to CLI agents as an MCP tool.
//!
//! Agents connected over ACP (Claude Code, Gemini CLI, Codex) run their own tool loop,
//! so they never see the native `search_codebase` tool. Instead the app starts this
//! small MCP server on the loopback interface and the ACP bridge hands its address to
//! every session it opens (`session/new` → `mcpServers`). The agent then calls
//! `search_codebase` like any other MCP tool, and the query runs against the same
//! on-device index the native tool uses.
//!
//! The server speaks the Streamable HTTP transport in its simplest form: every
//! JSON-RPC request is a POST answered with a JSON body, and there is no server-sent
//! event stream. Its address carries a random token, so other local programs can't
//! query the index by guessing the port.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::{Value, json};

/// The name agents show the server under; Claude Code names the tool
/// `mcp__uncaged__search_codebase`.
pub const SERVER_NAME: &str = "uncaged";
pub const TOOL_NAME: &str = "search_codebase";
/// The newest MCP revision this server implements. Clients asking for another
/// revision get this one back and decide whether they can work with it.
const PROTOCOL_VERSION: &str = "2025-06-18";
/// How long one search may take before the agent gets an error instead.
const SEARCH_TIMEOUT: Duration = Duration::from_secs(30);

const TOOL_DESCRIPTION: &str = "Search this codebase with Uncaged's on-device index. \
Returns the most relevant code chunks with their file paths and line numbers. Use it to \
find where a feature, concept or identifier lives before reading files, especially in a \
large or unfamiliar repository. Works best with keywords and identifiers \
(\"tab drag reorder\", \"CodebaseIndexManager sync\") rather than full questions.";

/// One search an agent asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRequest {
    pub query: String,
    /// The directory to search: the agent's own `path` argument, else the folder its
    /// session was opened in.
    pub directory: String,
}

/// Runs a search and returns the text to show the agent, or a message saying why it
/// couldn't.
pub type SearchFn =
    Arc<dyn Fn(SearchRequest) -> BoxFuture<'static, Result<String, String>> + Send + Sync>;

/// Where the running server listens, e.g. `http://127.0.0.1:52011/mcp/<token>`.
static SERVER_URL: OnceLock<String> = OnceLock::new();

/// Start the server on its own thread. Later calls do nothing, so the app can call
/// this whenever it sets up. Agents opened before it is listening simply don't get
/// the tool.
pub fn start(search: SearchFn) {
    if SERVER_URL.get().is_some() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("uncaged-code-search".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(err) => {
                    tracing::warn!("code search server: couldn't start a runtime: {err}");
                    return;
                }
            };
            runtime.block_on(async move {
                if let Err(err) = serve(search).await {
                    tracing::warn!("code search server stopped: {err}");
                }
            });
        });
    if let Err(err) = spawned {
        tracing::warn!("code search server: couldn't start a thread: {err}");
    }
}

async fn serve(search: SearchFn) -> anyhow::Result<()> {
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await?;
    let port = listener.local_addr()?.port();
    let token = uuid::Uuid::new_v4().simple().to_string();
    let _ = SERVER_URL.set(format!("http://127.0.0.1:{port}/mcp/{token}"));

    let state = Arc::new(ServerState { token, search });
    let app = Router::new()
        .route(
            "/mcp/{token}",
            post(handle_post).get(no_stream).delete(no_stream),
        )
        .with_state(state);
    axum::serve(listener, app).await?;
    Ok(())
}

/// The `mcpServers` entry for an ACP `session/new` whose agent works in `directory`,
/// or `None` while the server isn't running.
pub fn acp_server_entry(directory: &str) -> Option<Value> {
    let base = SERVER_URL.get()?;
    let url = reqwest::Url::parse_with_params(base, [("dir", directory)]).ok()?;
    Some(json!({
        "type": "http",
        "name": SERVER_NAME,
        "url": url.as_str(),
        "headers": [],
    }))
}

struct ServerState {
    token: String,
    search: SearchFn,
}

#[derive(Deserialize)]
struct SessionParams {
    dir: Option<String>,
}

/// There's no server-to-client stream; the transport lets a server say so with 405.
async fn no_stream() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}

async fn handle_post(
    State(state): State<Arc<ServerState>>,
    Path(token): Path<String>,
    Query(params): Query<SessionParams>,
    body: String,
) -> Response {
    if token != state.token {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Ok(message) = serde_json::from_str::<Value>(&body) else {
        return json_response(rpc_error(Value::Null, -32700, "Parse error"));
    };
    let directory = params.dir.unwrap_or_default();

    // A batch is answered with an array of the responses its requests produce.
    if let Value::Array(messages) = message {
        let mut responses = Vec::new();
        for message in messages {
            if let Some(response) = handle_message(&message, &directory, &state.search).await {
                responses.push(response);
            }
        }
        return if responses.is_empty() {
            StatusCode::ACCEPTED.into_response()
        } else {
            json_response(Value::Array(responses))
        };
    }

    match handle_message(&message, &directory, &state.search).await {
        Some(response) => json_response(response),
        // Notifications and responses get no body.
        None => StatusCode::ACCEPTED.into_response(),
    }
}

fn json_response(value: Value) -> Response {
    (
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        value.to_string(),
    )
        .into_response()
}

/// Answer one JSON-RPC message, or `None` for a notification.
pub(crate) async fn handle_message(
    message: &Value,
    directory: &str,
    search: &SearchFn,
) -> Option<Value> {
    let method = message.get("method").and_then(Value::as_str)?;
    // Without an id it's a notification (e.g. `notifications/initialized`).
    let id = message.get("id")?.clone();
    let params = message.get("params").cloned().unwrap_or(Value::Null);

    let response = match method {
        "initialize" => rpc_result(
            id,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
                "instructions": "Uncaged's on-device codebase index. Call search_codebase \
                    to find where something lives in the repository before reading files.",
            }),
        ),
        "ping" => rpc_result(id, json!({})),
        "tools/list" => rpc_result(id, json!({ "tools": [tool_definition()] })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            if name != TOOL_NAME {
                return Some(rpc_error(id, -32602, &format!("Unknown tool: {name}")));
            }
            let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
            rpc_result(id, call_search(&arguments, directory, search).await)
        }
        _ => rpc_error(id, -32601, &format!("Method not found: {method}")),
    };
    Some(response)
}

fn tool_definition() -> Value {
    json!({
        "name": TOOL_NAME,
        "title": "Search codebase",
        "description": TOOL_DESCRIPTION,
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Keywords and identifiers to look for."
                },
                "path": {
                    "type": "string",
                    "description": "Absolute path of another repository or folder to search. \
                        Leave it out to search the folder this session works in."
                }
            },
            "required": ["query"]
        },
        "annotations": { "readOnlyHint": true, "openWorldHint": false }
    })
}

/// Run the search and wrap the outcome as a tool result. Failures are tool errors
/// (`isError`), so the agent reads the reason and carries on with its own tools.
async fn call_search(arguments: &Value, session_directory: &str, search: &SearchFn) -> Value {
    let query = arguments
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if query.is_empty() {
        return tool_text("Give a query: keywords or identifiers to look for.", true);
    }
    let directory = arguments
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .unwrap_or(session_directory);
    if directory.is_empty() {
        return tool_text(
            "This session has no working folder to search. Pass `path` with the repository's absolute path.",
            true,
        );
    }

    let request = SearchRequest {
        query: query.to_string(),
        directory: directory.to_string(),
    };
    match tokio::time::timeout(SEARCH_TIMEOUT, search(request)).await {
        Ok(Ok(text)) => tool_text(&text, false),
        Ok(Err(message)) => tool_text(&message, true),
        Err(_) => tool_text(
            "The search took too long. Search with your own tools instead.",
            true,
        ),
    }
}

fn tool_text(text: &str, is_error: bool) -> Value {
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error,
    })
}

fn rpc_result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Whether an ACP `session/request_permission` is about this server's own tool, which
/// only reads the index and is safe to allow without asking.
pub(crate) fn is_own_tool_permission(params: &Value) -> bool {
    let Some(tool_call) = params.get("toolCall") else {
        return false;
    };
    // Look at how the agent names the tool, not at its input, which the agent wrote.
    let mut names = String::new();
    if let Some(title) = tool_call.get("title").and_then(Value::as_str) {
        names.push_str(title);
    }
    if let Some(meta) = tool_call.get("_meta") {
        names.push(' ');
        names.push_str(&meta.to_string());
    }
    names.contains(&format!("mcp__{SERVER_NAME}__{TOOL_NAME}"))
        || (names.contains(TOOL_NAME) && names.contains(SERVER_NAME))
}

#[cfg(test)]
#[path = "code_search_tests.rs"]
mod tests;
