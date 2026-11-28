//! Turn orchestration: parse the request, run the configured provider, and
//! sequence the `ResponseEvent`s Warp's Agent Mode expects.
//!
//! The engine is per-turn and stateless: Warp's client owns the cross-turn tool
//! loop, so each call here is "take a request, stream one assistant turn (text
//! plus tool calls), finish". When the client executes the tool calls it issues
//! a fresh request, which is just another call into here.

use std::collections::HashSet;

use futures::StreamExt as _;
use futures::channel::mpsc;
use futures::stream::BoxStream;

use crate::config::UncagedConfig;
use crate::model::Conversation;
use crate::model::NeutralMsg;
use crate::proto::api;
use crate::providers;
use crate::providers::ProviderEvent;
use crate::request_parse;
use crate::system_prompt;
use crate::tools::ToolRegistry;
use crate::wire;

/// Output type matching the seam's expectation, minus the crate-specific error
/// (the seam maps `anyhow::Error` into its own `Error`).
pub type EngineStream = BoxStream<'static, Result<api::ResponseEvent, anyhow::Error>>;

/// Run a single Agent Mode turn against the configured local backend.
pub fn run_turn(config: &UncagedConfig, request: &api::Request) -> EngineStream {
    run_turn_with(request, |conversation_id| {
        providers::build(config, conversation_id)
    })
}

/// [`run_turn`] against the provider `build_provider` returns for the
/// conversation's id (so tests can supply their own).
pub(crate) fn run_turn_with(
    request: &api::Request,
    build_provider: impl FnOnce(&str) -> Box<dyn providers::Provider>,
) -> EngineStream {
    let registry = ToolRegistry::build(request);
    let system = system_prompt::build(request, &registry.specs);
    let parsed = request_parse::parse(request);

    let conversation = Conversation {
        system_prompt: system,
        messages: parsed.messages,
        tools: registry.specs.clone(),
    };

    let provider = build_provider(&parsed.conversation_id);
    let (tx, rx) = mpsc::unbounded();

    let target_task_id = parsed.target_task_id.clone();
    let conversation_id = parsed.conversation_id.clone();
    let new_tool_results = parsed.new_tool_results;

    tokio::spawn(async move {
        drive(
            provider,
            registry,
            conversation,
            conversation_id,
            target_task_id,
            new_tool_results,
            tx,
        )
        .await;
    });

    rx.boxed()
}

async fn drive(
    provider: Box<dyn providers::Provider>,
    registry: ToolRegistry,
    conversation: Conversation,
    conversation_id: String,
    target_task_id: Option<String>,
    new_tool_results: Vec<api::request::input::ToolCallResult>,
    tx: mpsc::UnboundedSender<Result<api::ResponseEvent, anyhow::Error>>,
) {
    let request_id = request_parse::new_id();
    let run_id = request_parse::new_id();

    // 1. Stream init.
    let _ = tx.unbounded_send(Ok(wire::init_event(&conversation_id, &request_id, &run_id)));

    // 2. Resolve the task to attach messages to, creating one if needed.
    let task_id = match target_task_id {
        Some(id) => id,
        None => {
            let id = request_parse::new_id();
            let _ = tx.unbounded_send(Ok(wire::client_actions(vec![wire::create_task(&id)])));
            id
        }
    };

    // 3. Record the tool results this request brought in the task, right after
    //    their tool calls, so later requests replay each call with its result.
    if !new_tool_results.is_empty() {
        let actions = new_tool_results
            .iter()
            .map(|result| {
                let message_id = request_parse::new_id();
                wire::add_message(
                    &task_id,
                    wire::tool_call_result_message(&message_id, result),
                )
            })
            .collect();
        let _ = tx.unbounded_send(Ok(wire::client_actions(actions)));
    }

    // 4. Stream the assistant turn.
    let assistant_message_id = request_parse::new_id();
    let mut started_text = false;
    // Tool-call ids must be unique across the conversation: results are paired
    // with their calls by id, and some backends number calls per response, so
    // they reuse ids ("run_shell_command:1") from turn to turn.
    let mut tool_call_ids = used_tool_call_ids(&conversation);

    let mut events = provider.stream(conversation);
    while let Some(event) = events.next().await {
        match event {
            ProviderEvent::TextDelta(delta) => {
                if delta.is_empty() {
                    continue;
                }
                let action = if started_text {
                    wire::append_text(&task_id, &assistant_message_id, &delta)
                } else {
                    started_text = true;
                    wire::add_message(
                        &task_id,
                        wire::agent_text_message(&assistant_message_id, &delta),
                    )
                };
                let _ = tx.unbounded_send(Ok(wire::client_actions(vec![action])));
            }
            ProviderEvent::ToolCall { id, name, input } => match registry.encode(&name, &input) {
                Some(tool) => {
                    let id = if id.is_empty() || tool_call_ids.contains(&id) {
                        request_parse::new_id()
                    } else {
                        id
                    };
                    tool_call_ids.insert(id.clone());
                    let message_id = request_parse::new_id();
                    let message = wire::tool_call_message(&message_id, &id, tool);
                    let _ = tx.unbounded_send(Ok(wire::client_actions(vec![wire::add_message(
                        &task_id, message,
                    )])));
                }
                None => {
                    tracing::warn!(
                        "uncaged: model called unknown tool `{name}` or gave it invalid arguments, ignoring"
                    );
                }
            },
            ProviderEvent::Done => break,
            ProviderEvent::Error(message) => {
                let _ = tx.unbounded_send(Ok(wire::finished_error(message)));
                return;
            }
        }
    }

    // 5. Finish.
    let _ = tx.unbounded_send(Ok(wire::finished_done()));
}

/// The ids of the tool calls already in `conversation`.
fn used_tool_call_ids(conversation: &Conversation) -> HashSet<String> {
    conversation
        .messages
        .iter()
        .flat_map(|message| match message {
            NeutralMsg::Assistant { tool_uses, .. } => tool_uses.as_slice(),
            NeutralMsg::User(_) | NeutralMsg::ToolResults(_) => &[],
        })
        .map(|call| call.id.clone())
        .collect()
}
