//! Anthropic Messages API provider (streaming, with tool use).

use std::collections::HashMap;

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
pub struct AnthropicProvider {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
    pub max_tokens: u32,
}

impl Provider for AnthropicProvider {
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

impl AnthropicProvider {
    async fn run(
        &self,
        conversation: Conversation,
        tx: &mpsc::UnboundedSender<ProviderEvent>,
    ) -> anyhow::Result<()> {
        let body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "system": conversation.system_prompt,
            "messages": build_messages(&conversation.messages),
            "tools": build_tools(&conversation),
            "stream": true,
        });

        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let response = reqwest::Client::new()
            .post(url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic API error {status}: {text}");
        }

        let mut decoder = SseDecoder::new();
        // Per-content-block accumulator for streamed tool_use input JSON.
        let mut tools: HashMap<i64, ToolAccum> = HashMap::new();
        let mut bytes = response.bytes_stream();

        while let Some(chunk) = bytes.next().await {
            let chunk = chunk?;
            for payload in decoder.push(&chunk) {
                let Ok(event) = serde_json::from_str::<Value>(&payload) else {
                    continue;
                };
                match event.get("type").and_then(Value::as_str) {
                    Some("content_block_start") => {
                        let index = event.get("index").and_then(Value::as_i64).unwrap_or(0);
                        let block = event.get("content_block");
                        if block.and_then(|b| b.get("type")).and_then(Value::as_str)
                            == Some("tool_use")
                        {
                            tools.insert(
                                index,
                                ToolAccum {
                                    id: block
                                        .and_then(|b| b.get("id"))
                                        .and_then(Value::as_str)
                                        .unwrap_or("")
                                        .to_string(),
                                    name: block
                                        .and_then(|b| b.get("name"))
                                        .and_then(Value::as_str)
                                        .unwrap_or("")
                                        .to_string(),
                                    json: String::new(),
                                },
                            );
                        }
                    }
                    Some("content_block_delta") => {
                        let index = event.get("index").and_then(Value::as_i64).unwrap_or(0);
                        let delta = event.get("delta");
                        match delta.and_then(|d| d.get("type")).and_then(Value::as_str) {
                            Some("text_delta") => {
                                if let Some(text) =
                                    delta.and_then(|d| d.get("text")).and_then(Value::as_str)
                                {
                                    let _ = tx
                                        .unbounded_send(ProviderEvent::TextDelta(text.to_string()));
                                }
                            }
                            Some("input_json_delta") => {
                                if let (Some(accum), Some(partial)) = (
                                    tools.get_mut(&index),
                                    delta
                                        .and_then(|d| d.get("partial_json"))
                                        .and_then(Value::as_str),
                                ) {
                                    accum.json.push_str(partial);
                                }
                            }
                            _ => {}
                        }
                    }
                    Some("content_block_stop") => {
                        let index = event.get("index").and_then(Value::as_i64).unwrap_or(0);
                        if let Some(accum) = tools.remove(&index) {
                            let input = if accum.json.trim().is_empty() {
                                json!({})
                            } else {
                                serde_json::from_str(&accum.json).unwrap_or_else(|_| json!({}))
                            };
                            let _ = tx.unbounded_send(ProviderEvent::ToolCall {
                                id: accum.id,
                                name: accum.name,
                                input,
                            });
                        }
                    }
                    Some("message_stop") => {
                        let _ = tx.unbounded_send(ProviderEvent::Done);
                        return Ok(());
                    }
                    Some("error") => {
                        let message = event
                            .get("error")
                            .and_then(|e| e.get("message"))
                            .and_then(Value::as_str)
                            .unwrap_or("unknown Anthropic stream error");
                        anyhow::bail!("{message}");
                    }
                    _ => {}
                }
            }
        }
        let _ = tx.unbounded_send(ProviderEvent::Done);
        Ok(())
    }
}

struct ToolAccum {
    id: String,
    name: String,
    json: String,
}

fn build_tools(conversation: &Conversation) -> Vec<Value> {
    conversation
        .tools
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "input_schema": tool.schema,
            })
        })
        .collect()
}

/// Lower neutral messages into Anthropic's role/content-block format, merging
/// consecutive same-role turns (Anthropic requires strict alternation).
fn build_messages(messages: &[NeutralMsg]) -> Vec<Value> {
    let mut out: Vec<(String, Vec<Value>)> = Vec::new();

    let mut push = |role: &str, mut blocks: Vec<Value>| {
        if let Some((last_role, last_blocks)) = out.last_mut()
            && last_role == role
        {
            last_blocks.append(&mut blocks);
            return;
        }
        out.push((role.to_string(), blocks));
    };

    for message in messages {
        match message {
            NeutralMsg::User(text) => {
                push("user", vec![json!({ "type": "text", "text": text })]);
            }
            NeutralMsg::Assistant { text, tool_uses } => {
                let mut blocks = Vec::new();
                if let Some(text) = text
                    && !text.is_empty()
                {
                    blocks.push(json!({ "type": "text", "text": text }));
                }
                for call in tool_uses {
                    blocks.push(json!({
                        "type": "tool_use",
                        "id": call.id,
                        "name": call.name,
                        "input": call.input,
                    }));
                }
                if !blocks.is_empty() {
                    push("assistant", blocks);
                }
            }
            NeutralMsg::ToolResults(results) => {
                let blocks = results
                    .iter()
                    .map(|r| {
                        json!({
                            "type": "tool_result",
                            "tool_use_id": r.id,
                            "content": r.content,
                            "is_error": r.is_error,
                        })
                    })
                    .collect();
                push("user", blocks);
            }
        }
    }

    out.into_iter()
        .map(|(role, content)| json!({ "role": role, "content": content }))
        .collect()
}
