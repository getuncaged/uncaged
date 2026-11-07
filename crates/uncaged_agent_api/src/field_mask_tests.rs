use prost_types::{FieldMask, Timestamp};

use super::*;
use crate::{message, Citation};

const AGENT_OUTPUT_TEXT: &str = "agent_output.text";

fn mask(paths: &[&str]) -> FieldMask {
    FieldMask {
        paths: paths.iter().map(|path| path.to_string()).collect(),
    }
}

/// A destination message with every top-level field set, holding `body`.
fn destination(body: Option<message::Message>) -> Message {
    Message {
        id: "message-1".to_string(),
        task_id: "task-1".to_string(),
        request_id: "request-1".to_string(),
        timestamp: Some(Timestamp {
            seconds: 1_700_000_000,
            nanos: 42,
        }),
        server_message_data: "server-data".to_string(),
        citations: vec![Citation {
            document_id: "doc-1".to_string(),
            document_type: 1,
        }],
        message: body,
    }
}

/// A source message whose other fields differ from [`destination`]'s, so a
/// test notices if any of them leak into the result.
fn source(body: Option<message::Message>) -> Message {
    Message {
        id: "message-1".to_string(),
        task_id: "other-task".to_string(),
        request_id: "other-request".to_string(),
        timestamp: None,
        server_message_data: String::new(),
        citations: vec![],
        message: body,
    }
}

fn agent_output(text: &str) -> Option<message::Message> {
    Some(message::Message::AgentOutput(message::AgentOutput {
        text: text.to_string(),
    }))
}

fn user_query() -> Option<message::Message> {
    Some(message::Message::UserQuery(message::UserQuery {
        query: "list files".to_string(),
        ..Default::default()
    }))
}

fn reasoning() -> Option<message::Message> {
    Some(message::Message::AgentReasoning(message::AgentReasoning {
        reasoning: "thinking".to_string(),
        finished_duration: None,
    }))
}

fn tool_call() -> Option<message::Message> {
    Some(message::Message::ToolCall(message::ToolCall {
        tool_call_id: "call-1".to_string(),
        tool: None,
    }))
}

/// What a case expects the merged message's `message` oneof to hold.
#[derive(Debug, Clone, Copy)]
enum Expected {
    /// The destination, untouched.
    Unchanged,
    /// The destination's other fields, with an agent output holding this text.
    AgentOutput(&'static str),
}

struct Case {
    operation: Operation,
    destination: Option<message::Message>,
    source: Option<message::Message>,
    paths: &'static [&'static str],
    expected: Expected,
}

fn cases() -> Vec<Case> {
    use Expected::*;
    use Operation::*;
    let one = &[AGENT_OUTPUT_TEXT][..];
    let two = &[AGENT_OUTPUT_TEXT, AGENT_OUTPUT_TEXT][..];
    let none = &[][..];
    let case = |operation, destination, source, paths, expected| Case {
        operation,
        destination,
        source,
        paths,
        expected,
    };
    vec![
        // The engine's streaming path: append a delta to agent output text.
        case(
            Append,
            agent_output("Hello"),
            agent_output(", world"),
            one,
            AgentOutput("Hello, world"),
        ),
        case(
            Append,
            agent_output(""),
            agent_output("Hi"),
            one,
            AgentOutput("Hi"),
        ),
        case(
            Append,
            agent_output("Hello"),
            agent_output(""),
            one,
            AgentOutput("Hello"),
        ),
        case(
            Append,
            agent_output("ab"),
            agent_output("c"),
            two,
            AgentOutput("abcc"),
        ),
        case(
            Append,
            agent_output("Hello"),
            agent_output("!"),
            none,
            Unchanged,
        ),
        // A destination without agent output: the oneof is replaced.
        case(Append, None, agent_output("Hi"), one, AgentOutput("Hi")),
        case(
            Append,
            user_query(),
            agent_output("Hi"),
            one,
            AgentOutput("Hi"),
        ),
        case(
            Append,
            reasoning(),
            agent_output("Hi"),
            one,
            AgentOutput("Hi"),
        ),
        case(
            Append,
            tool_call(),
            agent_output("Hi"),
            one,
            AgentOutput("Hi"),
        ),
        // A source without agent output contributes the empty string.
        case(
            Append,
            agent_output("Hello"),
            None,
            one,
            AgentOutput("Hello"),
        ),
        case(
            Append,
            agent_output("Hello"),
            tool_call(),
            one,
            AgentOutput("Hello"),
        ),
        case(Append, tool_call(), user_query(), one, AgentOutput("")),
        // Update replaces the text.
        case(
            Update,
            agent_output("Hello"),
            agent_output("Bye"),
            one,
            AgentOutput("Bye"),
        ),
        case(
            Update,
            agent_output("Hello"),
            agent_output("Bye"),
            two,
            AgentOutput("Bye"),
        ),
        case(
            Update,
            agent_output("Hello"),
            agent_output(""),
            one,
            AgentOutput(""),
        ),
        case(Update, agent_output("Hello"), None, one, AgentOutput("")),
        case(Update, None, agent_output("Bye"), one, AgentOutput("Bye")),
        case(
            Update,
            reasoning(),
            agent_output("Bye"),
            one,
            AgentOutput("Bye"),
        ),
        case(Update, tool_call(), user_query(), one, AgentOutput("")),
        case(Update, tool_call(), agent_output("Bye"), none, Unchanged),
    ]
}

fn run(operation: Operation, destination: &Message, source: &Message, mask: &FieldMask) -> Message {
    match operation {
        Operation::Update => update(destination, source, mask),
        Operation::Append => append(destination, source, mask),
    }
    .expect("supported path")
}

#[test]
fn supported_paths_merge_as_expected() {
    for (index, case) in cases().into_iter().enumerate() {
        let destination = destination(case.destination);
        let source = source(case.source);
        let mask = mask(case.paths);
        let expected = match case.expected {
            Expected::Unchanged => destination.clone(),
            Expected::AgentOutput(text) => Message {
                message: agent_output(text),
                ..destination.clone()
            },
        };
        let result = run(case.operation, &destination, &source, &mask);
        assert_eq!(result, expected, "case {index}");
    }
}

#[test]
fn unsupported_paths_are_rejected() {
    let destination = destination(agent_output("Hello"));
    let source = source(agent_output("!"));
    for path in ["tool_call", "agent_output", "message.tool_call", "text", ""] {
        let mask = mask(&[AGENT_OUTPUT_TEXT, path]);
        let error = FieldMaskError::UnsupportedPath(path.to_string());
        assert_eq!(update(&destination, &source, &mask), Err(error.clone()));
        assert_eq!(append(&destination, &source, &mask), Err(error));
    }
}
