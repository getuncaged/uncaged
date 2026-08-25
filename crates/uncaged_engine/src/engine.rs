//! Turn orchestration: parse the request, run the configured provider, and
//! sequence the `ResponseEvent`s Warp's Agent Mode expects.
//!
//! The engine is per-turn and stateless: Warp's client owns the cross-turn tool
//! loop, so each call here is "take a request, stream one assistant turn (text
//! plus tool calls), finish". When the client executes the tool calls it issues
//! a fresh request, which is just another call into here.

use futures::StreamExt as _;
use futures::channel::mpsc;
use futures::stream::BoxStream;

use crate::config::UncagedConfig;
use crate::model::Conversation;
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
    let registry = ToolRegistry::build(request);
    let system = system_prompt::build(request, &registry.specs);
    let parsed = request_parse::parse(request);

    // Uncaged one-history: the user inputs of THIS turn, to echo into the task
    // so persisted tasks carry the full exchange (see wire::user_query_message).
    let echo_queries: Vec<String> =
        request
            .input
            .as_ref()
            .and_then(|input| input.r#type.as_ref())
            .map(|input_type| match input_type {
                api::request::input::Type::UserInputs(user_inputs) => user_inputs
                    .inputs
                    .iter()
                    .filter_map(|item| match &item.input {
                        Some(api::request::input::user_inputs::user_input::Input::UserQuery(
                            uq,
                        )) if !uq.query.is_empty() => Some(uq.query.clone()),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            })
            .unwrap_or_default();

    let conversation = Conversation {
        system_prompt: system,
        messages: parsed.messages,
        tools: registry.specs.clone(),
    };

    let provider = providers::build(config, &parsed.conversation_id);
    let (tx, rx) = mpsc::unbounded();

    let target_task_id = parsed.target_task_id.clone();
    let conversation_id = parsed.conversation_id.clone();

    tokio::spawn(async move {
        drive(
            provider,
            registry,
            conversation,
            conversation_id,
            target_task_id,
            echo_queries,
            tx,
        )
        .await;
    });

    rx.boxed()
}

#[allow(clippy::too_many_arguments)]
async fn drive(
    provider: Box<dyn providers::Provider>,
    registry: ToolRegistry,
    conversation: Conversation,
    conversation_id: String,
    target_task_id: Option<String>,
    echo_queries: Vec<String>,
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

    // 2b. Echo this turn's user inputs into the task so the persisted proto
    // carries the full exchange (inputs never enter the task otherwise -- the
    // app's live exchange is in-memory only).
    for query in &echo_queries {
        let message_id = request_parse::new_id();
        let _ = tx.unbounded_send(Ok(wire::client_actions(vec![wire::add_message(
            &task_id,
            wire::user_query_message(&message_id, query, &request_id),
        )])));
    }

    // 3. Stream the assistant turn.
    let assistant_message_id = request_parse::new_id();
    let mut started_text = false;

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
                        wire::agent_text_message(&assistant_message_id, &delta, &request_id),
                    )
                };
                let _ = tx.unbounded_send(Ok(wire::client_actions(vec![action])));
            }
            ProviderEvent::ToolCall { id, name, input } => match registry.encode(&name, &input) {
                Some(tool) => {
                    let message_id = request_parse::new_id();
                    let message = wire::tool_call_message(&message_id, &id, tool, &request_id);
                    let _ = tx.unbounded_send(Ok(wire::client_actions(vec![wire::add_message(
                        &task_id, message,
                    )])));
                }
                None => {
                    tracing::warn!("uncaged: model called unknown tool `{name}`, ignoring");
                }
            },
            ProviderEvent::Done => break,
            ProviderEvent::Error(message) => {
                let _ = tx.unbounded_send(Ok(wire::finished_error(message)));
                return;
            }
        }
    }

    // 4. Finish.
    let _ = tx.unbounded_send(Ok(wire::finished_done()));
}
