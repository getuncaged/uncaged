//! Field-mask updates for streamed messages.
//!
//! `UpdateTaskMessage` and `AppendToMessageContent` client actions carry a
//! partial [`Message`] and a [`FieldMask`] naming the fields to take from it.
//! Only the paths the agent engine produces are supported; any other path is
//! rejected with [`FieldMaskError::UnsupportedPath`].
//!
//! Supported paths:
//! - `agent_output.text`: the text of an agent output message. `uncaged_engine`
//!   streams model text as appends to it.

use std::fmt;

use prost_types::FieldMask;

use crate::{message, Message};

/// Errors that can occur when applying a field mask operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldMaskError {
    /// The mask names a path this module does not support.
    UnsupportedPath(String),
}

impl fmt::Display for FieldMaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldMaskError::UnsupportedPath(path) => {
                write!(f, "Unsupported field mask path: {path}")
            }
        }
    }
}

impl std::error::Error for FieldMaskError {}

/// A supported field mask path on [`Message`].
#[derive(Debug, Clone, Copy)]
enum MessagePath {
    /// `agent_output.text`
    AgentOutputText,
}

impl MessagePath {
    fn parse(path: &str) -> Result<Self, FieldMaskError> {
        match path {
            "agent_output.text" => Ok(Self::AgentOutputText),
            other => Err(FieldMaskError::UnsupportedPath(other.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Operation {
    Update,
    Append,
}

/// Returns a copy of `destination` with each masked field replaced by its
/// value in `source`.
pub fn update(
    destination: &Message,
    source: &Message,
    mask: &FieldMask,
) -> Result<Message, FieldMaskError> {
    apply(destination, source, mask, Operation::Update)
}

/// Returns a copy of `destination` with each masked string field extended by
/// its value in `source`.
pub fn append(
    destination: &Message,
    source: &Message,
    mask: &FieldMask,
) -> Result<Message, FieldMaskError> {
    apply(destination, source, mask, Operation::Append)
}

fn apply(
    destination: &Message,
    source: &Message,
    mask: &FieldMask,
    operation: Operation,
) -> Result<Message, FieldMaskError> {
    let paths = mask
        .paths
        .iter()
        .map(|path| MessagePath::parse(path))
        .collect::<Result<Vec<_>, _>>()?;

    let mut result = destination.clone();
    for path in paths {
        match path {
            MessagePath::AgentOutputText => {
                // A source that is not an agent output contributes the empty
                // string, as proto field defaults do.
                let source_text =
                    if let Some(message::Message::AgentOutput(output)) = &source.message {
                        output.text.as_str()
                    } else {
                        ""
                    };
                let output = agent_output_mut(&mut result);
                match operation {
                    Operation::Update => output.text = source_text.to_owned(),
                    Operation::Append => output.text.push_str(source_text),
                }
            }
        }
    }
    Ok(result)
}

/// Returns the agent output of `message`, first replacing whatever else the
/// `message` oneof held with an empty agent output (setting a oneof member
/// clears the others).
fn agent_output_mut(message: &mut Message) -> &mut message::AgentOutput {
    if !matches!(message.message, Some(message::Message::AgentOutput(_))) {
        message.message = Some(message::Message::AgentOutput(
            message::AgentOutput::default(),
        ));
    }
    let Some(message::Message::AgentOutput(output)) = &mut message.message else {
        unreachable!("the agent output was just set");
    };
    output
}

#[cfg(test)]
#[path = "field_mask_tests.rs"]
mod tests;
