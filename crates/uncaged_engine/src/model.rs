//! Provider-neutral conversation model.
//!
//! Warp's protocol and each LLM provider speak different dialects. Rather than
//! translate N protocols × M providers, everything funnels through these
//! neutral types: the request parser lowers a `warp_multi_agent_api::Request`
//! into a `Conversation`, and each provider raises a `Conversation` into its
//! own wire format. The result events go the other way.

use serde_json::Value;

/// A single turn in the neutral conversation, already coalesced so that an
/// assistant turn carries both its text and any tool calls, and tool results
/// are grouped — the shape both Anthropic and OpenAI expect.
#[derive(Debug, Clone)]
pub enum NeutralMsg {
    /// Something the user (or Warp, on the user's behalf) said.
    User(String),
    /// What the model produced last turn: optional prose plus zero or more tool calls.
    Assistant {
        text: Option<String>,
        tool_uses: Vec<ToolUse>,
    },
    /// Results of tool calls the client executed locally, paired by id.
    ToolResults(Vec<ToolResult>),
}

/// A request from the model to invoke a tool. `id` must round-trip so the
/// matching result can be paired on the next turn.
#[derive(Debug, Clone)]
pub struct ToolUse {
    pub id: String,
    pub name: String,
    pub input: Value,
}

/// The outcome of a tool call, rendered to text for the model.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub id: String,
    pub content: String,
    pub is_error: bool,
}

/// A tool the model is allowed to call this turn, with a JSON Schema for args.
#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema object describing the tool's parameters.
    pub schema: Value,
}

/// The full neutral input to a provider: system prompt, history, and allowed tools.
#[derive(Debug, Clone)]
pub struct Conversation {
    pub system_prompt: String,
    pub messages: Vec<NeutralMsg>,
    pub tools: Vec<ToolSpec>,
}

impl Conversation {
    /// Appends a user/assistant/tool message, coalescing with the previous one
    /// where the provider APIs require it (consecutive user text merges;
    /// assistant text + tool calls share one turn; tool results group).
    pub fn push_user(&mut self, text: impl Into<String>) {
        let text = text.into();
        if let Some(NeutralMsg::User(existing)) = self.messages.last_mut() {
            existing.push('\n');
            existing.push_str(&text);
        } else {
            self.messages.push(NeutralMsg::User(text));
        }
    }

    pub fn push_assistant_text(&mut self, text: impl Into<String>) {
        match self.messages.last_mut() {
            Some(NeutralMsg::Assistant { text: existing, .. }) => {
                let incoming = text.into();
                match existing {
                    Some(t) => t.push_str(&incoming),
                    None => *existing = Some(incoming),
                }
            }
            _ => self.messages.push(NeutralMsg::Assistant {
                text: Some(text.into()),
                tool_uses: Vec::new(),
            }),
        }
    }

    pub fn push_tool_use(&mut self, tool_use: ToolUse) {
        match self.messages.last_mut() {
            Some(NeutralMsg::Assistant { tool_uses, .. }) => tool_uses.push(tool_use),
            _ => self.messages.push(NeutralMsg::Assistant {
                text: None,
                tool_uses: vec![tool_use],
            }),
        }
    }

    pub fn push_tool_result(&mut self, result: ToolResult) {
        match self.messages.last_mut() {
            Some(NeutralMsg::ToolResults(results)) => results.push(result),
            _ => self.messages.push(NeutralMsg::ToolResults(vec![result])),
        }
    }
}
