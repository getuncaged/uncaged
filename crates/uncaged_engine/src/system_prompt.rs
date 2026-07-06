//! System prompt authoring.
//!
//! Warp's server builds the agent's system prompt and never sends it to the
//! client, so a local engine must author its own. This is a faithful,
//! self-contained agent prompt that (a) describes the agentic terminal role,
//! (b) explains the available tools and how to use them well, and (c) folds in
//! the live environment context (pwd, OS, shell, git, project rules) the way
//! Warp's server does. Treat the base prompt as a living artifact — tune it
//! against observed behavior.

use crate::model::ToolSpec;
use crate::proto::api;

const BASE_PROMPT: &str = r#"You are the agent inside Uncaged, an agentic terminal (an open fork of Warp). You help the user accomplish software engineering and command-line tasks directly in their terminal.

Operating principles:
- You act by calling tools. Prefer doing the work with tools over telling the user how to do it.
- Before editing code, read the relevant files so your edits are precise. Use search and grep to locate things.
- Make minimal, correct changes that match the surrounding code style.
- Run shell commands to inspect state, build, and test. Explain briefly what you're about to do before running anything with side effects.
- When you apply file edits, the `search` text must match the file exactly. Keep diffs focused.
- After a tool runs, read its result before deciding the next step. Stop when the task is done; don't pad with unnecessary tool calls.
- Be concise in prose. Use Markdown. Don't restate the user's request back to them.

You are running on the user's own machine, powered by the user's own model/subscription. Respect that tool calls have real effects on their system."#;

/// Build the full system prompt for this turn, folding in environment context.
pub fn build(request: &api::Request, tools: &[ToolSpec]) -> String {
    let mut prompt = String::from(BASE_PROMPT);

    if let Some(ctx) = latest_context(request) {
        let env = render_environment(ctx);
        if !env.is_empty() {
            prompt.push_str("\n\n## Environment\n");
            prompt.push_str(&env);
        }
        let rules = render_project_rules(ctx);
        if !rules.is_empty() {
            prompt.push_str("\n\n## Project rules (follow these)\n");
            prompt.push_str(&rules);
        }
    }

    if !tools.is_empty() {
        prompt.push_str("\n\n## Tools available this turn\n");
        for tool in tools {
            prompt.push_str(&format!("- `{}`: {}\n", tool.name, tool.description));
        }
    }

    prompt
}

/// The most relevant `InputContext` is the one attached to the new input.
fn latest_context(request: &api::Request) -> Option<&api::InputContext> {
    request.input.as_ref().and_then(|i| i.context.as_ref())
}

fn render_environment(ctx: &api::InputContext) -> String {
    let mut lines = Vec::new();
    if let Some(dir) = &ctx.directory
        && !dir.pwd.is_empty()
    {
        lines.push(format!("- Working directory: {}", dir.pwd));
    }
    if let Some(os) = &ctx.operating_system {
        let mut os_line = os.platform.clone();
        if !os.distribution.is_empty() {
            os_line.push_str(&format!(" ({})", os.distribution));
        }
        if !os_line.is_empty() {
            lines.push(format!("- OS: {os_line}"));
        }
    }
    if let Some(shell) = &ctx.shell
        && !shell.name.is_empty()
    {
        lines.push(format!("- Shell: {}", shell.name));
    }
    if let Some(git) = &ctx.git {
        let mut git_parts = Vec::new();
        if !git.branch.is_empty() {
            git_parts.push(format!("branch {}", git.branch));
        } else if !git.head.is_empty() {
            git_parts.push(format!("head {}", git.head));
        }
        if let Some(repo) = &git.repository
            && !repo.name.is_empty()
        {
            git_parts.push(format!("repo {}", repo.name));
        }
        if !git_parts.is_empty() {
            lines.push(format!("- Git: {}", git_parts.join(", ")));
        }
    }
    lines.join("\n")
}

fn render_project_rules(ctx: &api::InputContext) -> String {
    let mut blocks = Vec::new();
    for rules in &ctx.project_rules {
        for file in &rules.active_rule_files {
            if !file.content.is_empty() {
                blocks.push(file.content.clone());
            }
        }
    }
    blocks.join("\n\n")
}
