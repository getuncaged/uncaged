//! OpenAI-compatible Chat Completions provider (streaming, with tool calls).
//!
//! One implementation covers a lot of ground: hosted OpenAI, OpenRouter, and —
//! because they all speak the same Chat Completions dialect — local servers
//! like Ollama (`/v1`), LM Studio, llama.cpp's server, and vLLM. The only
//! difference is the base URL and whether an API key is needed.

use std::collections::BTreeMap;

use futures::StreamExt as _;
use futures::channel::mpsc;
use futures::stream::BoxStream;
use serde_json::Value;
use serde_json::json;

use super::Provider;
use super::ProviderEvent;
use super::sse::SseDecoder;
use crate::model::Conversation;
use crate::model::NeutralMsg;

#[derive(Clone)]
pub struct OpenAiProvider {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub max_tokens: u32,
}

impl Provider for OpenAiProvider {
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

impl OpenAiProvider {
    async fn run(
        &self,
        conversation: Conversation,
        tx: &mpsc::UnboundedSender<ProviderEvent>,
    ) -> anyhow::Result<()> {
        let mut body = json!({
            "model": self.model,
            "messages": build_messages(&conversation),
            "stream": true,
            "max_tokens": self.max_tokens,
        });
        let tools = build_tools(&conversation);
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools);
            body["tool_choice"] = json!("auto");
        }

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let mut builder = reqwest::Client::new()
            .post(url)
            .header("content-type", "application/json")
            .json(&body);
        if let Some(key) = &self.api_key
            && !key.is_empty()
        {
            builder = builder.bearer_auth(key);
        }

        let response = builder.send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI-compatible API error {status}: {text}");
        }

        let mut decoder = SseDecoder::new();
        let mut tool_accum: BTreeMap<i64, ToolAccum> = BTreeMap::new();
        let mut flushed = false;
        let mut bytes = response.bytes_stream();

        while let Some(chunk) = bytes.next().await {
            let chunk = chunk?;
            for payload in decoder.push(&chunk) {
                if payload == "[DONE]" {
                    if !flushed {
                        flush_tool_calls(&mut tool_accum, tx);
                    }
                    let _ = tx.unbounded_send(ProviderEvent::Done);
                    return Ok(());
                }
                let Ok(event) = serde_json::from_str::<Value>(&payload) else {
                    continue;
                };
                let Some(choice) = event.get("choices").and_then(|c| c.get(0)) else {
                    continue;
                };
                if let Some(delta) = choice.get("delta") {
                    if let Some(content) = delta.get("content").and_then(Value::as_str)
                        && !content.is_empty()
                    {
                        let _ = tx.unbounded_send(ProviderEvent::TextDelta(content.to_string()));
                    }
                    if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                        for call in calls {
                            accumulate_tool_call(call, &mut tool_accum);
                        }
                    }
                }
                if choice
                    .get("finish_reason")
                    .map(|r| !r.is_null())
                    .unwrap_or(false)
                    && !flushed
                {
                    flush_tool_calls(&mut tool_accum, tx);
                    flushed = true;
                }
            }
        }

        if !flushed {
            flush_tool_calls(&mut tool_accum, tx);
        }
        let _ = tx.unbounded_send(ProviderEvent::Done);
        Ok(())
    }
}

#[derive(Default)]
struct ToolAccum {
    id: String,
    name: String,
    args: String,
}

fn accumulate_tool_call(call: &Value, accum: &mut BTreeMap<i64, ToolAccum>) {
    let index = call.get("index").and_then(Value::as_i64).unwrap_or(0);
    let entry = accum.entry(index).or_default();
    if let Some(id) = call.get("id").and_then(Value::as_str)
        && !id.is_empty()
    {
        entry.id = id.to_string();
    }
    if let Some(function) = call.get("function") {
        if let Some(name) = function.get("name").and_then(Value::as_str)
            && !name.is_empty()
        {
            entry.name = name.to_string();
        }
        if let Some(args) = function.get("arguments").and_then(Value::as_str) {
            entry.args.push_str(args);
        }
    }
}

fn flush_tool_calls(
    accum: &mut BTreeMap<i64, ToolAccum>,
    tx: &mpsc::UnboundedSender<ProviderEvent>,
) {
    for (_, tool) in std::mem::take(accum) {
        if tool.name.is_empty() {
            continue;
        }
        let input = if tool.args.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&tool.args).unwrap_or_else(|_| json!({}))
        };
        let id = if tool.id.is_empty() {
            crate::request_parse::new_id()
        } else {
            tool.id
        };
        let _ = tx.unbounded_send(ProviderEvent::ToolCall {
            id,
            name: tool.name,
            input,
        });
    }
}

fn build_tools(conversation: &Conversation) -> Vec<Value> {
    conversation
        .tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.schema,
                }
            })
        })
        .collect()
}

fn build_messages(conversation: &Conversation) -> Vec<Value> {
    let mut out = Vec::new();
    out.push(json!({ "role": "system", "content": conversation.system_prompt }));

    for message in &conversation.messages {
        match message {
            NeutralMsg::User(text) => out.push(json!({ "role": "user", "content": text })),
            NeutralMsg::Assistant { text, tool_uses } => {
                let mut msg = json!({ "role": "assistant" });
                msg["content"] = json!(text.clone().unwrap_or_default());
                if !tool_uses.is_empty() {
                    msg["tool_calls"] = Value::Array(
                        tool_uses
                            .iter()
                            .map(|call| {
                                json!({
                                    "id": call.id,
                                    "type": "function",
                                    "function": {
                                        "name": call.name,
                                        "arguments": call.input.to_string(),
                                    }
                                })
                            })
                            .collect(),
                    );
                }
                out.push(msg);
            }
            NeutralMsg::ToolResults(results) => {
                for r in results {
                    out.push(json!({
                        "role": "tool",
                        "tool_call_id": r.id,
                        "content": r.content,
                    }));
                }
            }
        }
    }
    out
}
