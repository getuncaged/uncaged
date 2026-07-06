//! Build the `ResponseEvent`s Warp's client consumes.
//!
//! The client renders Agent Mode by applying a stream of events: one
//! `StreamInit`, then `ClientActions` that create a task / add messages /
//! append streaming text / emit tool calls, then one `StreamFinished`. These
//! helpers construct those events; the engine sequences them.

use crate::proto::api;

/// The agent-output field path used by `AppendToMessageContent` to stream text
/// deltas into an existing message.
const AGENT_OUTPUT_TEXT_PATH: &str = "agent_output.text";

pub fn init_event(conversation_id: &str, request_id: &str, run_id: &str) -> api::ResponseEvent {
    api::ResponseEvent {
        r#type: Some(api::response_event::Type::Init(
            api::response_event::StreamInit {
                conversation_id: conversation_id.to_string(),
                request_id: request_id.to_string(),
                run_id: run_id.to_string(),
            },
        )),
    }
}

/// Wrap one or more client actions into a single event.
pub fn client_actions(actions: Vec<api::ClientAction>) -> api::ResponseEvent {
    api::ResponseEvent {
        r#type: Some(api::response_event::Type::ClientActions(
            api::response_event::ClientActions { actions },
        )),
    }
}

pub fn finished_done() -> api::ResponseEvent {
    api::ResponseEvent {
        r#type: Some(api::response_event::Type::Finished(
            api::response_event::StreamFinished {
                reason: Some(api::response_event::stream_finished::Reason::Done(
                    api::response_event::stream_finished::Done {},
                )),
                ..Default::default()
            },
        )),
    }
}

pub fn finished_error(message: impl Into<String>) -> api::ResponseEvent {
    api::ResponseEvent {
        r#type: Some(api::response_event::Type::Finished(
            api::response_event::StreamFinished {
                reason: Some(api::response_event::stream_finished::Reason::InternalError(
                    api::response_event::stream_finished::InternalError {
                        message: message.into(),
                    },
                )),
                ..Default::default()
            },
        )),
    }
}

/// `CreateTask` for conversations that don't have a task yet.
pub fn create_task(task_id: &str) -> api::ClientAction {
    api::ClientAction {
        action: Some(api::client_action::Action::CreateTask(
            api::client_action::CreateTask {
                task: Some(api::Task {
                    id: task_id.to_string(),
                    ..Default::default()
                }),
            },
        )),
    }
}

/// `AddMessagesToTask` — seed a new message in the task.
pub fn add_message(task_id: &str, message: api::Message) -> api::ClientAction {
    api::ClientAction {
        action: Some(api::client_action::Action::AddMessagesToTask(
            api::client_action::AddMessagesToTask {
                task_id: task_id.to_string(),
                messages: vec![message],
            },
        )),
    }
}

/// `AppendToMessageContent` — stream a text delta into an existing agent message.
pub fn append_text(task_id: &str, message_id: &str, delta: &str) -> api::ClientAction {
    api::ClientAction {
        action: Some(api::client_action::Action::AppendToMessageContent(
            api::client_action::AppendToMessageContent {
                task_id: task_id.to_string(),
                message: Some(agent_text_message(message_id, delta)),
                mask: Some(prost_types::FieldMask {
                    paths: vec![AGENT_OUTPUT_TEXT_PATH.to_string()],
                }),
            },
        )),
    }
}

/// An agent-output (prose) message.
pub fn agent_text_message(message_id: &str, text: &str) -> api::Message {
    api::Message {
        id: message_id.to_string(),
        message: Some(api::message::Message::AgentOutput(
            api::message::AgentOutput {
                text: text.to_string(),
            },
        )),
        ..Default::default()
    }
}

/// A tool-call message the client will execute natively.
pub fn tool_call_message(
    message_id: &str,
    tool_call_id: &str,
    tool: api::message::tool_call::Tool,
) -> api::Message {
    api::Message {
        id: message_id.to_string(),
        message: Some(api::message::Message::ToolCall(api::message::ToolCall {
            tool_call_id: tool_call_id.to_string(),
            tool: Some(tool),
        })),
        ..Default::default()
    }
}
