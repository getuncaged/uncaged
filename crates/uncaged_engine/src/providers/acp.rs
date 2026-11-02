//! CLI-agent bridge over the Agent Client Protocol (ACP).
//!
//! Spawns a local CLI agent the user already pays for (e.g. a Claude/Gemini ACP
//! binary) and bridges its streamed text into Agent Mode.
//!
//! Sessions are PERSISTENT per conversation: the first turn in a tab spawns the
//! CLI and opens an ACP session; later turns reuse that same process + session
//! so the agent keeps its own context (and doesn't re-pay a cold start every
//! turn). A conversation maps 1:1 to a Warp tab's agent thread, so this is
//! effectively "one CLI session per tab", which is what you want from a stateful
//! CLI agent.
//!
//! IMPORTANT, documented honestly: an ACP agent runs *its own* tool loop in its
//! *own* process, so this path does NOT drive Warp's native client-side tools /
//! permission UI — it surfaces the agent's text output. For the full native
//! Agent Mode experience powered by your own model, use the Anthropic or
//! OpenAI-compatible providers. ACP method/shape details vary by agent and
//! protocol version; treat this as a starting point to adapt per agent.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex as StdMutex};

use futures::StreamExt as _;
use futures::channel::mpsc;
use futures::stream::BoxStream;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex as AsyncMutex;

use super::{Provider, ProviderEvent};
use crate::config::Effort;
use crate::model::{Conversation, NeutralMsg};

/// A live CLI process plus the ACP session opened against it. Held across turns.
struct AcpSession {
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    lines: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    session_id: String,
    /// The model and effort the session is running with, so a change in the menu can be
    /// applied to the live session instead of waiting for the next spawn.
    model: Option<String>,
    effort: Option<Effort>,
    /// Monotonic JSON-RPC request id — must keep climbing for the life of the
    /// session so responses can be matched unambiguously.
    next_id: u64,
}

/// Persistent ACP sessions keyed by conversation id. The outer lock is held only
/// briefly to fetch/insert a per-conversation slot; the turn itself holds just
/// that conversation's async lock, so different tabs run concurrently while turns
/// within one tab stay serialized (which they already are on the client side).
type SessionSlot = Arc<AsyncMutex<Option<AcpSession>>>;
static SESSIONS: LazyLock<StdMutex<HashMap<String, SessionSlot>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));

fn slot_for(conversation_id: &str) -> SessionSlot {
    let mut map = SESSIONS.lock().unwrap();
    map.entry(conversation_id.to_string())
        .or_insert_with(|| Arc::new(AsyncMutex::new(None)))
        .clone()
}

/// A Finder-launched macOS app inherits only a minimal `PATH`
/// (`/usr/bin:/bin:/usr/sbin:/sbin`), so agent CLIs installed via nvm / Homebrew
/// / `~/.local/bin` are invisible to it. Ask the user's login shell for the real
/// PATH once (markers guard against noisy rc output) and reuse it for spawns.
static USER_PATH: LazyLock<Option<String>> = LazyLock::new(|| {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    // A Unix login-shell PATH probe; Windows has no login shell to ask, so the console-window
    // flash the lint guards against doesn't arise here.
    #[allow(clippy::disallowed_types)]
    let out = std::process::Command::new(shell)
        .args(["-lic", "printf '__UNCAGED_PATH__%s__END__' \"$PATH\""])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let path = s
        .split("__UNCAGED_PATH__")
        .nth(1)?
        .split("__END__")
        .next()?
        .trim();
    (!path.is_empty()).then(|| path.to_string())
});

/// Resolve a bare program name (e.g. `claude-code-acp`) to an absolute path using
/// the user's real PATH, so spawning works regardless of the app's own PATH. An
/// already-qualified path or an unresolved name is returned unchanged.
fn resolve_program(program: &str) -> String {
    if program.contains('/') {
        return program.to_string();
    }
    if let Some(path) = USER_PATH.as_deref() {
        for dir in path.split(':').filter(|d| !d.is_empty()) {
            let candidate = std::path::Path::new(dir).join(program);
            if candidate.is_file() {
                return candidate.to_string_lossy().into_owned();
            }
        }
    }
    program.to_string()
}

#[derive(Clone)]
pub struct AcpProvider {
    pub command: Vec<String>,
    /// Passed to the agent at `session/new` (best-effort — agents that don't
    /// understand a model hint ignore it). `None` lets the CLI use its default.
    pub model: Option<String>,
    /// Forwarded to Claude Code's Agent SDK options at `session/new`; other agents ignore it.
    pub effort: Option<Effort>,
    /// The tab's agent-thread id; the key under which we keep the CLI session.
    pub conversation_id: String,
}

impl Provider for AcpProvider {
    fn stream(&self, conversation: Conversation) -> BoxStream<'static, ProviderEvent> {
        let (tx, rx) = mpsc::unbounded();
        let provider = self.clone();
        tokio::spawn(async move {
            if let Err(err) = provider.run(conversation, &tx).await {
                let _ = tx.unbounded_send(ProviderEvent::Error(err.to_string()));
            }
        });
        rx.boxed()
    }
}

impl AcpProvider {
    async fn run(
        &self,
        conversation: Conversation,
        tx: &mpsc::UnboundedSender<ProviderEvent>,
    ) -> anyhow::Result<()> {
        let prompt_text = latest_user_text(&conversation);
        let slot = slot_for(&self.conversation_id);
        let mut guard = slot.lock().await;

        // (Re)establish the session if we don't have a live one. A previously
        // stored session whose child has exited is treated as absent so we
        // transparently respawn (e.g. the CLI crashed or was killed).
        if guard.as_mut().is_none_or(session_is_dead) {
            *guard = Some(self.open_session(None, tx).await?);
        }

        // Effort is fixed when a session is created, so a changed effort reopens the session,
        // resuming the same conversation so the agent keeps its context.
        if let Some(session) = guard.as_ref()
            && session.effort != self.effort
        {
            let resume = session.session_id.clone();
            *guard = None;
            *guard = Some(self.open_session(Some(resume), tx).await?);
        }

        // A changed model is switched on the live session.
        let session = guard.as_mut().expect("session established above");
        if session.model != self.model {
            if let Some(model) = self.model.as_ref().filter(|m| !m.is_empty()) {
                set_model(session, model, tx).await;
            }
            session.model = self.model.clone();
        }

        // Reuse the live session for this turn. If the pipe breaks mid-turn,
        // drop the session so the *next* turn respawns cleanly rather than
        // wedging the tab on a dead process.
        if let Err(err) = prompt_turn(session, &prompt_text, tx).await {
            *guard = None;
            return Err(err);
        }

        let _ = tx.unbounded_send(ProviderEvent::Done);
        Ok(())
    }

    /// Spawn the CLI and complete the ACP handshake (`initialize` +
    /// `session/new`), returning a session ready to take prompts. `resume` is the id of an
    /// earlier session to continue, for agents that support it (Claude Code).
    async fn open_session(
        &self,
        resume: Option<String>,
        tx: &mpsc::UnboundedSender<ProviderEvent>,
    ) -> anyhow::Result<AcpSession> {
        let Some((program, args)) = self.command.split_first() else {
            anyhow::bail!("ACP provider has no command configured");
        };

        // Resolve against the user's real PATH and hand the child that PATH too,
        // so both the CLI and anything IT spawns (node, etc.) are findable.
        let resolved = resolve_program(program);
        let mut cmd = Command::new(&resolved);
        cmd.args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            // Claude Code refuses to launch "inside another Claude Code session";
            // clear the markers so the CLI still starts even if Uncaged itself was
            // launched from a Claude Code terminal. A Finder-launched app won't have
            // these set anyway, so this is just belt-and-braces.
            .env_remove("CLAUDECODE")
            .env_remove("CLAUDE_CODE_SSE_PORT");
        if let Some(path) = USER_PATH.as_deref() {
            cmd.env("PATH", path);
        }
        let mut child = cmd.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!(
                    "couldn't find `{program}` on your PATH. Install your agent CLI \
                     (for Claude Code: `npm install -g @zed-industries/claude-code-acp`) \
                     and make sure it's on your PATH, then reconnect."
                )
            } else {
                anyhow::anyhow!("failed to start `{program}`: {e}")
            }
        })?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("no stdout"))?;
        let mut lines = BufReader::new(stdout).lines();

        send(
            &mut stdin,
            1,
            "initialize",
            json!({ "protocolVersion": 1, "clientCapabilities": {} }),
        )
        .await?;
        let _ = read_result(&mut lines, 1, tx).await?; // capabilities ignored by the text bridge

        // Claude Code's ACP adapter spreads `_meta.claudeCode.options` into its Agent SDK
        // options, which is where effort and resume live. Other agents ignore `_meta`.
        let mut claude_code_options = serde_json::Map::new();
        if let Some(effort) = self.effort {
            claude_code_options.insert("effort".into(), json!(effort.claude_code_value()));
        }
        if let Some(resume) = resume {
            claude_code_options.insert("resume".into(), json!(resume));
        }
        let mut new_session = json!({ "cwd": ".", "mcpServers": [] });
        if !claude_code_options.is_empty() {
            new_session["_meta"] = json!({ "claudeCode": { "options": claude_code_options } });
        }
        send(&mut stdin, 2, "session/new", new_session).await?;
        let session = read_result(&mut lines, 2, tx).await?;
        let session_id = session
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        // Select the configured model. Agents that advertise `availableModels` in
        // the `session/new` result accept `session/set_model` (Claude Code's ids are
        // e.g. `default` / `sonnet` / `haiku` / `opus[1m]`). Best-effort: ignore a
        // failure so an unknown id, or an agent without model switching, still runs.
        let mut session = AcpSession {
            child,
            stdin,
            lines,
            session_id,
            model: self.model.clone(),
            effort: self.effort,
            next_id: 3,
        };
        if let Some(model) = self.model.as_ref().filter(|m| !m.is_empty()) {
            set_model(&mut session, model, tx).await;
        }
        Ok(session)
    }
}

/// Select `model` on a live session. Agents that advertise `availableModels` in the
/// `session/new` result accept `session/set_model` (Claude Code's ids are e.g. `default` /
/// `sonnet` / `haiku` / `opus[1m]`). Best-effort: an unknown id, or an agent without model
/// switching, keeps running on its current model.
async fn set_model(
    session: &mut AcpSession,
    model: &str,
    tx: &mpsc::UnboundedSender<ProviderEvent>,
) {
    let id = session.next_id;
    session.next_id += 1;
    let params = json!({ "sessionId": session.session_id, "modelId": model });
    if send(&mut session.stdin, id, "session/set_model", params)
        .await
        .is_ok()
    {
        let _ = read_result(&mut session.lines, id, tx).await;
    }
}

/// `true` if the child process has already exited (so the session is unusable).
fn session_is_dead(session: &mut AcpSession) -> bool {
    matches!(session.child.try_wait(), Ok(Some(_)) | Err(_))
}

/// Send one user turn to an existing session and stream its reply. The session's
/// request-id keeps climbing so responses stay unambiguous across turns.
async fn prompt_turn(
    session: &mut AcpSession,
    prompt_text: &str,
    tx: &mpsc::UnboundedSender<ProviderEvent>,
) -> anyhow::Result<()> {
    let id = session.next_id;
    session.next_id += 1;
    let session_id = session.session_id.clone();
    send(
        &mut session.stdin,
        id,
        "session/prompt",
        json!({
            "sessionId": session_id,
            "prompt": [{ "type": "text", "text": prompt_text }],
        }),
    )
    .await?;
    read_result(&mut session.lines, id, tx).await?;
    Ok(())
}

/// Write one JSON-RPC request line to the agent's stdin.
async fn send(
    stdin: &mut tokio::process::ChildStdin,
    id: u64,
    method: &str,
    params: Value,
) -> anyhow::Result<()> {
    let line = serde_json::to_string(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    }))?;
    stdin.write_all(line.as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    stdin.flush().await?;
    Ok(())
}

/// Read lines until the JSON-RPC response with `id` arrives, forwarding any
/// `session/update` text chunks to the event stream along the way.
async fn read_result(
    lines: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    id: u64,
    tx: &mpsc::UnboundedSender<ProviderEvent>,
) -> anyhow::Result<Value> {
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        // Notification: streamed agent output.
        if message.get("method").and_then(Value::as_str) == Some("session/update") {
            if let Some(text) = extract_chunk_text(&message) {
                let _ = tx.unbounded_send(ProviderEvent::TextDelta(text));
            }
            continue;
        }

        // Response to one of our requests.
        if message.get("id").and_then(Value::as_u64) == Some(id) {
            if let Some(error) = message.get("error") {
                let msg = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("ACP error");
                anyhow::bail!("{msg}");
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }
    anyhow::bail!("ACP agent closed the stream before responding");
}

fn extract_chunk_text(message: &Value) -> Option<String> {
    let update = message.get("params")?.get("update")?;
    let kind = update.get("sessionUpdate").and_then(Value::as_str)?;
    if kind != "agent_message_chunk" {
        return None;
    }
    let text = update.get("content")?.get("text")?.as_str()?;
    Some(text.to_string())
}

fn latest_user_text(conversation: &Conversation) -> String {
    conversation
        .messages
        .iter()
        .rev()
        .find_map(|m| match m {
            NeutralMsg::User(text) => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_default()
}
