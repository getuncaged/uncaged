//! Types for Next Command suggestions. The response shape mirrors Warp's GenerateAIInputSuggestions
//! endpoint; Uncaged builds it locally from command history and the completer.

mod api;

pub use api::*;

/// A section of command history that can be used as context for intelligent autosuggestions.
/// The context includes a consecutive sequence of commands in a session (previous_commands) and the next_command,
/// previous_commands can be similar to the user's recently run commands, and next_command can be used
/// to predict the next command.
#[derive(Clone)]
pub struct HistoryContext {
    pub previous_commands: Vec<crate::persistence::model::Command>,
    pub next_command: crate::persistence::model::Command,
}

/// Context needed for a Next Command suggestion that remains constant regardless of what
/// the user has typed in the input. This is reused as the user edits the input and generates new suggestions.
#[derive(Clone)]
pub struct NextCommandContext {
    pub history_contexts: Vec<HistoryContext>,
}

#[cfg(test)]
#[path = "generate_ai_input_suggestions_tests.rs"]
mod tests;
