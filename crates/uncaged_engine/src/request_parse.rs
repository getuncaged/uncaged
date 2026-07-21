//! Lower a `warp_multi_agent_api::Request` into the neutral conversation.
//!
//! History lives in `task_context.tasks[].messages`; the latest user turn and
//! freshly-executed tool results arrive in `input`. The two can overlap (the
//! just-typed query may already be in the task, tool results may not be yet),
//! so we dedupe by content/id to present each turn to the model exactly once.

use std::collections::HashSet;

use crate::model::Conversation;
use crate::model::NeutralMsg;
use crate::proto::api;
use crate::tools::decode_history_result;
use crate::tools::decode_input_result;
use crate::tools::decode_tool_call;

pub struct ParsedHistory {
    pub messages: Vec<NeutralMsg>,
    /// The task to attach the assistant's reply to, if one already exists.
    pub target_task_id: Option<String>,
    /// Round-tripped to the client in `StreamInit`.
    pub conversation_id: String,
}

pub fn parse(request: &api::Request) -> ParsedHistory {
    // Reuse Conversation's coalescing logic as a message builder.
    let mut builder = Conversation {
        system_prompt: String::new(),
        messages: Vec::new(),
        tools: Vec::new(),
    };

    let mut seen_result_ids: HashSet<String> = HashSet::new();
    let mut target_task_id: Option<String> = None;

    // --- history ---
    if let Some(task_context) = &request.task_context {
        for task in &task_context.tasks {
            if !task.id.is_empty() {
                target_task_id = Some(task.id.clone());
            }
            for message in &task.messages {
                lower_history_message(message, &mut builder, &mut seen_result_ids);
            }
        }
    }

    // --- the new input turn ---
    if let Some(input) = &request.input {
        lower_input(input, &mut builder, &mut seen_result_ids);
    }

    let conversation_id = request
        .metadata
        .as_ref()
        .map(|m| m.conversation_id.clone())
        .filter(|id| !id.is_empty())
        .unwrap_or_else(new_id);

    ParsedHistory {
        messages: builder.messages,
        target_task_id,
        conversation_id,
    }
}

fn lower_history_message(
    message: &api::Message,
    builder: &mut Conversation,
    seen_result_ids: &mut HashSet<String>,
) {
    use api::message::Message as M;
    match &message.message {
        Some(M::UserQuery(uq)) => builder.push_user(uq.query.clone()),
        Some(M::AgentOutput(ao)) => {
            if !ao.text.is_empty() {
                builder.push_assistant_text(ao.text.clone());
            }
        }
        Some(M::ToolCall(tc)) => builder.push_tool_use(decode_tool_call(tc)),
        Some(M::ToolCallResult(tr)) => {
            let result = decode_history_result(tr);
            seen_result_ids.insert(result.id.clone());
            builder.push_tool_result(result);
        }
        // Reasoning, summaries, todos, server events, etc. are not replayed to
        // the model — they aren't part of the user-visible turn contract.
        _ => {}
    }
}

// The deprecated `UserQuery` / `ToolCallResult` direct input forms are still
// handled for robustness against older request shapes; allow their use here.
#[allow(deprecated)]
fn lower_input(
    input: &api::request::Input,
    builder: &mut Conversation,
    seen_result_ids: &mut HashSet<String>,
) {
    // Attached context — terminal blocks the user pinned via "Attach as agent
    // context", plus any selected text or files — rides on `input.context`
    // alongside the environment bits (cwd/git/os/rules, which the system prompt
    // already folds in). Surface the *attachments* here so the model can actually
    // see them. `push_user` coalesces, so these merge with the query below into a
    // single user turn (keeps the backends' strict user/assistant alternation intact).
    if let Some(ctx) = &input.context {
        lower_attached_context(ctx, builder);
    }

    use api::request::input::Type;
    match &input.r#type {
        Some(Type::UserInputs(user_inputs)) => {
            for item in &user_inputs.inputs {
                lower_user_input(item, builder, seen_result_ids);
            }
        }
        // Deprecated direct forms, still handled defensively.
        Some(Type::UserQuery(uq)) => push_user_deduped(builder, &uq.query),
        Some(Type::ToolCallResult(tr)) => push_input_result(builder, tr, seen_result_ids),
        // Hardcoded-prompt inputs: surface their user-visible text if present.
        Some(Type::CreateNewProject(p)) => push_user_deduped(builder, &p.query),
        Some(Type::CloneRepository(c)) => push_user_deduped(builder, &c.url),
        Some(Type::SummarizeConversation(s)) => {
            push_user_deduped(builder, "Summarize the conversation so far.");
            if !s.prompt.is_empty() {
                push_user_deduped(builder, &s.prompt);
            }
        }
        // Passive follow-up suggestions: the client fires this after a shell
        // command (or agent response) completes and expects a `suggest_prompt`
        // tool call it can render as a chip. Turn the trigger into a directive
        // user message so the model calls that tool.
        Some(Type::GeneratePassiveSuggestions(passive)) => {
            lower_passive_suggestion(passive, builder)
        }
        _ => {}
    }
}

/// Turn a passive-suggestion trigger into a directive user message. We push a
/// plain user turn (rather than anything tool-specific) because the tool itself
/// is already advertised via `ToolRegistry`; the model just needs the context
/// plus an instruction to respond *only* by calling `suggest_prompt`.
fn lower_passive_suggestion(
    passive: &api::request::input::GeneratePassiveSuggestions,
    builder: &mut Conversation,
) {
    use api::request::input::generate_passive_suggestions::Trigger;
    match &passive.trigger {
        Some(Trigger::ShellCommandCompleted(scc)) => {
            let Some(cmd) = &scc.executed_shell_command else {
                return;
            };
            let directive = format!(
                "The user just ran a shell command in their terminal:\n\n\
                 $ {command}\n\
                 (exit code: {exit_code})\n\
                 {output}\n\n\
                 Suggest ONE helpful follow-up command by calling the `suggest_prompt` tool. \
                 If the command failed, suggest the corrected command. Keep the `label` short \
                 (2-4 words). Do NOT reply with prose — only call the tool. If nothing useful \
                 applies, do not call the tool.",
                command = cmd.command,
                exit_code = cmd.exit_code,
                output = cmd.output,
            );
            push_user_deduped(builder, &directive);
        }
        Some(Trigger::AgentResponseCompleted(_)) => {
            push_user_deduped(
                builder,
                "The agent just finished responding. Suggest ONE helpful next step by calling \
                 the `suggest_prompt` tool. Keep the `label` short. Do NOT reply with prose — \
                 only call the tool. If nothing useful applies, do not call the tool.",
            );
        }
        // The `FilesChanged` / `CommandRun` triggers are deprecated placeholders
        // (empty payloads); nothing actionable to prompt on.
        _ => {}
    }
}

/// Fold user-attached context (pinned terminal blocks, selected text, files)
/// from `input.context` into the current user turn. Environment context
/// (directory/git/OS/project rules) is intentionally skipped — that belongs in
/// the system prompt (see `system_prompt.rs`), not the message stream.
fn lower_attached_context(ctx: &api::InputContext, builder: &mut Conversation) {
    for cmd in &ctx.executed_shell_commands {
        if cmd.command.is_empty() && cmd.output.is_empty() {
            continue;
        }
        let mut block = format!("[Attached terminal block]\n$ {}", cmd.command);
        if !cmd.output.is_empty() {
            block.push('\n');
            block.push_str(&cmd.output);
        }
        push_user_deduped(builder, &block);
    }
    for selected in &ctx.selected_text {
        if !selected.text.is_empty() {
            push_user_deduped(builder, &format!("[Attached selection]\n{}", selected.text));
        }
    }
    for file in &ctx.files {
        if let Some(content) = &file.content
            && !content.content.is_empty()
        {
            push_user_deduped(
                builder,
                &format!(
                    "[Attached file: {}]\n{}",
                    content.file_path, content.content
                ),
            );
        }
    }
}

fn lower_user_input(
    item: &api::request::input::user_inputs::UserInput,
    builder: &mut Conversation,
    seen_result_ids: &mut HashSet<String>,
) {
    use api::request::input::user_inputs::user_input::Input as I;
    match &item.input {
        Some(I::UserQuery(uq)) => push_user_deduped(builder, &uq.query),
        Some(I::ToolCallResult(tr)) => push_input_result(builder, tr, seen_result_ids),
        _ => {}
    }
}

fn push_input_result(
    builder: &mut Conversation,
    tr: &api::request::input::ToolCallResult,
    seen_result_ids: &mut HashSet<String>,
) {
    let result = decode_input_result(tr);
    if seen_result_ids.contains(&result.id) {
        return;
    }
    seen_result_ids.insert(result.id.clone());
    builder.push_tool_result(result);
}

/// Push a user message unless it exactly duplicates the last user message
/// already present (the just-typed query can appear in both task and input).
fn push_user_deduped(builder: &mut Conversation, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(NeutralMsg::User(last)) = builder.messages.last()
        && last == text
    {
        return;
    }
    builder.push_user(text.to_string());
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
