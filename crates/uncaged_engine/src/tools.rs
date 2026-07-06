//! Tool registry: the bridge between a provider's function-calling and Warp's
//! native, client-executed tools.
//!
//! Warp ships the *capabilities* (a `ToolType` enum) and executes tool calls
//! locally, but it does NOT send the model-facing JSON schemas — those live on
//! Warp's server. So this module authors a JSON Schema for each built-in tool,
//! encodes a model's `tool_use` into the corresponding Warp `ToolCall` proto
//! (which the client then runs in the user's PTY / filesystem), and decodes the
//! `ToolCallResult` the client sends back into text the model can read.
//!
//! MCP tools are the easy case: their real schemas *do* arrive in the request's
//! `mcp_context`, so we forward them verbatim and route calls to `CallMCPTool`.

use std::collections::HashMap;

use serde_json::Value;
use serde_json::json;

use crate::model::ToolResult;
use crate::model::ToolSpec;
use crate::model::ToolUse;
use crate::proto::api;
use crate::proto::json_to_struct;
use crate::proto::struct_to_json;

/// Built-in tools Uncaged knows how to schema + encode. Other `ToolType`s
/// the client advertises are simply not exposed to the model (the model can't
/// call what it can't see), which is safe.
const BUILTIN_RUN_SHELL: &str = "run_shell_command";
const BUILTIN_READ_FILES: &str = "read_files";
const BUILTIN_SEARCH_CODEBASE: &str = "search_codebase";
const BUILTIN_APPLY_DIFFS: &str = "apply_file_diffs";
const BUILTIN_GREP: &str = "grep";
const BUILTIN_FILE_GLOB: &str = "file_glob";
// Not a PTY/filesystem tool: calling this emits a `SuggestPrompt` the client
// renders as a one-click chip (passive follow-up suggestions). See
// `request_parse::lower_passive_suggestion`.
const BUILTIN_SUGGEST_PROMPT: &str = "suggest_prompt";

struct McpToolRef {
    server_id: String,
}

/// Everything the engine needs to present tools to a model and translate calls
/// in both directions.
pub struct ToolRegistry {
    pub specs: Vec<ToolSpec>,
    mcp: HashMap<String, McpToolRef>,
}

impl ToolRegistry {
    /// Build the registry from the request's advertised tool capabilities and
    /// MCP context.
    pub fn build(request: &api::Request) -> Self {
        let mut specs = Vec::new();
        let mut mcp = HashMap::new();

        let supported: Vec<i32> = request
            .settings
            .as_ref()
            .map(|s| s.supported_tools.clone())
            .unwrap_or_default();

        // Only expose a built-in if the client said it supports it. `ToolType`
        // values are proto enum i32s; compare against the known variants.
        let supports = |t: api::ToolType| supported.is_empty() || supported.contains(&(t as i32));

        if supports(api::ToolType::RunShellCommand) {
            specs.push(spec_run_shell());
        }
        if supports(api::ToolType::ReadFiles) {
            specs.push(spec_read_files());
        }
        if supports(api::ToolType::SearchCodebase) {
            specs.push(spec_search_codebase());
        }
        if supports(api::ToolType::ApplyFileDiffs) {
            specs.push(spec_apply_diffs());
        }
        if supports(api::ToolType::Grep) {
            specs.push(spec_grep());
        }
        if supports(api::ToolType::FileGlobV2) {
            specs.push(spec_file_glob());
        }
        if supports(api::ToolType::SuggestPrompt) {
            specs.push(spec_suggest_prompt());
        }

        // MCP tools: forward name/description/schema verbatim, remember which
        // server each came from so we can populate `CallMCPTool.server_id`.
        if let Some(ctx) = &request.mcp_context {
            for server in &ctx.servers {
                for tool in &server.tools {
                    let schema = tool
                        .input_schema
                        .as_ref()
                        .map(struct_to_json)
                        .unwrap_or_else(|| json!({ "type": "object" }));
                    specs.push(ToolSpec {
                        name: tool.name.clone(),
                        description: tool.description.clone(),
                        schema,
                    });
                    mcp.insert(
                        tool.name.clone(),
                        McpToolRef {
                            server_id: server.id.clone(),
                        },
                    );
                }
            }
        }

        Self { specs, mcp }
    }

    /// Encode a model `tool_use` into the Warp `ToolCall` oneof payload, so the
    /// client can execute it natively. Returns `None` for an unknown tool.
    pub fn encode(&self, name: &str, input: &Value) -> Option<api::message::tool_call::Tool> {
        use api::message::tool_call as tc;
        match name {
            BUILTIN_RUN_SHELL => Some(tc::Tool::RunShellCommand(tc::RunShellCommand {
                command: str_field(input, "command"),
                ..Default::default()
            })),
            BUILTIN_READ_FILES => {
                let files = str_array(input, "paths")
                    .into_iter()
                    .map(|name| tc::read_files::File {
                        name,
                        ..Default::default()
                    })
                    .collect();
                Some(tc::Tool::ReadFiles(tc::ReadFiles { files }))
            }
            BUILTIN_SEARCH_CODEBASE => Some(tc::Tool::SearchCodebase(tc::SearchCodebase {
                query: str_field(input, "query"),
                path_filters: str_array(input, "path_filters"),
                codebase_path: str_field(input, "codebase_path"),
            })),
            BUILTIN_GREP => Some(tc::Tool::Grep(tc::Grep {
                queries: str_array(input, "queries"),
                path: str_field(input, "path"),
            })),
            BUILTIN_FILE_GLOB => Some(tc::Tool::FileGlobV2(tc::FileGlobV2 {
                patterns: str_array(input, "patterns"),
                search_dir: str_field(input, "search_dir"),
                max_matches: int_field(input, "max_matches"),
                max_depth: int_field(input, "max_depth"),
                min_depth: int_field(input, "min_depth"),
            })),
            BUILTIN_APPLY_DIFFS => Some(tc::Tool::ApplyFileDiffs(encode_apply_diffs(input))),
            // `label` is optional in the schema; an empty string maps to the
            // proto's "no label" (the client falls back to showing `prompt`).
            BUILTIN_SUGGEST_PROMPT => Some(tc::Tool::SuggestPrompt(tc::SuggestPrompt {
                is_trigger_irrelevant: false,
                display_mode: Some(tc::suggest_prompt::DisplayMode::PromptChip(
                    tc::suggest_prompt::PromptChip {
                        prompt: str_field(input, "prompt"),
                        label: str_field(input, "label"),
                    },
                )),
            })),
            other => {
                let mcp = self.mcp.get(other)?;
                Some(tc::Tool::CallMcpTool(tc::CallMcpTool {
                    name: other.to_string(),
                    args: Some(json_to_struct(input)),
                    server_id: mcp.server_id.clone(),
                }))
            }
        }
    }
}

fn encode_apply_diffs(input: &Value) -> api::message::tool_call::ApplyFileDiffs {
    use api::message::tool_call::apply_file_diffs as afd;
    let diffs = input
        .get("diffs")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|d| afd::FileDiff {
                    file_path: str_field(d, "file_path"),
                    search: str_field(d, "search"),
                    replace: str_field(d, "replace"),
                })
                .collect()
        })
        .unwrap_or_default();
    let new_files = input
        .get("new_files")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|f| afd::NewFile {
                    file_path: str_field(f, "file_path"),
                    content: str_field(f, "content"),
                })
                .collect()
        })
        .unwrap_or_default();
    let deleted_files = str_array(input, "deleted_files")
        .into_iter()
        .map(|file_path| afd::DeleteFile { file_path })
        .collect();
    api::message::tool_call::ApplyFileDiffs {
        summary: str_field(input, "summary"),
        diffs,
        new_files,
        deleted_files,
        ..Default::default()
    }
}

/// Recover a neutral `ToolUse` from a Warp `ToolCall` stored in history, so the
/// provider sees a consistent prior-turn tool call.
pub fn decode_tool_call(call: &api::message::ToolCall) -> ToolUse {
    use api::message::tool_call::Tool;
    let (name, input) = match &call.tool {
        Some(Tool::RunShellCommand(c)) => (BUILTIN_RUN_SHELL, json!({ "command": c.command })),
        Some(Tool::ReadFiles(r)) => {
            let paths: Vec<String> = r.files.iter().map(|f| f.name.clone()).collect();
            (BUILTIN_READ_FILES, json!({ "paths": paths }))
        }
        Some(Tool::SearchCodebase(s)) => (
            BUILTIN_SEARCH_CODEBASE,
            json!({ "query": s.query, "path_filters": s.path_filters, "codebase_path": s.codebase_path }),
        ),
        Some(Tool::Grep(g)) => (
            BUILTIN_GREP,
            json!({ "queries": g.queries, "path": g.path }),
        ),
        Some(Tool::FileGlobV2(g)) => (
            BUILTIN_FILE_GLOB,
            json!({ "patterns": g.patterns, "search_dir": g.search_dir }),
        ),
        Some(Tool::ApplyFileDiffs(a)) => (BUILTIN_APPLY_DIFFS, json!({ "summary": a.summary })),
        Some(Tool::CallMcpTool(m)) => (
            m.name.as_str(),
            m.args.as_ref().map(struct_to_json).unwrap_or(json!({})),
        ),
        _ => ("unknown_tool", json!({})),
    };
    ToolUse {
        id: call.tool_call_id.clone(),
        name: name.to_string(),
        input,
    }
}

/// Decode a history `ToolCallResult` into neutral text for the model.
pub fn decode_history_result(result: &api::message::ToolCallResult) -> ToolResult {
    use api::message::tool_call_result::Result as R;
    let (content, is_error) = match &result.result {
        Some(R::RunShellCommand(r)) => format_shell_result(r),
        Some(R::ReadFiles(r)) => format_read_files(r),
        Some(R::SearchCodebase(r)) => format_search(r),
        Some(R::ApplyFileDiffs(r)) => format_apply_diffs(r),
        Some(R::Grep(r)) => format_grep(r),
        Some(R::FileGlobV2(r)) => format_file_glob(r),
        Some(R::CallMcpTool(r)) => format_mcp(r),
        Some(other) => (format!("{other:?}"), false),
        None => ("(no result)".to_string(), false),
    };
    ToolResult {
        id: result.tool_call_id.clone(),
        content,
        is_error,
    }
}

/// Decode a freshly-received input `ToolCallResult` (the results the client
/// just produced) into neutral text. Structurally identical to the history
/// variant but a distinct generated type.
pub fn decode_input_result(result: &api::request::input::ToolCallResult) -> ToolResult {
    use api::request::input::tool_call_result::Result as R;
    let (content, is_error) = match &result.result {
        Some(R::RunShellCommand(r)) => format_shell_result(r),
        Some(R::ReadFiles(r)) => format_read_files(r),
        Some(R::SearchCodebase(r)) => format_search(r),
        Some(R::ApplyFileDiffs(r)) => format_apply_diffs(r),
        Some(R::Grep(r)) => format_grep(r),
        Some(R::FileGlobV2(r)) => format_file_glob(r),
        Some(R::CallMcpTool(r)) => format_mcp(r),
        Some(other) => (format!("{other:?}"), false),
        None => ("(no result)".to_string(), false),
    };
    ToolResult {
        id: result.tool_call_id.clone(),
        content,
        is_error,
    }
}

// ---- result formatters (Warp result proto -> human/model-readable text) ----

fn format_shell_result(r: &api::RunShellCommandResult) -> (String, bool) {
    use api::run_shell_command_result::Result as R;
    match &r.result {
        Some(R::CommandFinished(f)) => {
            let header = format!("$ {}\n(exit code {})", r.command, f.exit_code);
            (format!("{header}\n{}", f.output), f.exit_code != 0)
        }
        Some(R::PermissionDenied(_)) => (
            format!("$ {}\n(permission denied by user)", r.command),
            true,
        ),
        Some(R::LongRunningCommandSnapshot(_)) => (
            format!("$ {}\n(command is still running)", r.command),
            false,
        ),
        None => (format!("$ {}\n(no output)", r.command), false),
    }
}

fn format_read_files(r: &api::ReadFilesResult) -> (String, bool) {
    use api::read_files_result::Result as R;
    match &r.result {
        Some(R::TextFilesSuccess(s)) => {
            let body = s
                .files
                .iter()
                .map(|f| format!("===== {} =====\n{}", f.file_path, f.content))
                .collect::<Vec<_>>()
                .join("\n\n");
            (body, false)
        }
        Some(R::AnyFilesSuccess(s)) => (format!("(read {} file(s))", s.files.len()), false),
        Some(R::Error(e)) => (format!("Error reading files: {}", e.message), true),
        None => ("(no result)".to_string(), false),
    }
}

fn format_search(r: &api::SearchCodebaseResult) -> (String, bool) {
    use api::search_codebase_result::Result as R;
    match &r.result {
        Some(R::Success(s)) => {
            let body = s
                .files
                .iter()
                .map(|f| format!("===== {} =====\n{}", f.file_path, f.content))
                .collect::<Vec<_>>()
                .join("\n\n");
            (body, false)
        }
        Some(R::Error(e)) => (format!("Search error: {}", e.message), true),
        None => ("(no matches)".to_string(), false),
    }
}

fn format_apply_diffs(r: &api::ApplyFileDiffsResult) -> (String, bool) {
    use api::apply_file_diffs_result::Result as R;
    match &r.result {
        Some(R::Success(s)) => {
            let updated = s.updated_files_v2.len();
            let deleted = s.deleted_files.len();
            (
                format!("Applied diffs: {updated} file(s) updated, {deleted} deleted."),
                false,
            )
        }
        Some(R::Error(e)) => (format!("Failed to apply diffs: {}", e.message), true),
        None => ("(no result)".to_string(), false),
    }
}

fn format_grep(r: &api::GrepResult) -> (String, bool) {
    use api::grep_result::Result as R;
    match &r.result {
        Some(R::Success(s)) => {
            let lines = s
                .matched_files
                .iter()
                .map(|file| {
                    let nums: Vec<String> = file
                        .matched_lines
                        .iter()
                        .map(|l| l.line_number.to_string())
                        .collect();
                    format!("{} (lines: {})", file.file_path, nums.join(", "))
                })
                .collect::<Vec<_>>()
                .join("\n");
            (
                if lines.is_empty() {
                    "(no matches)".to_string()
                } else {
                    lines
                },
                false,
            )
        }
        Some(R::Error(e)) => (format!("Grep error: {}", e.message), true),
        None => ("(no matches)".to_string(), false),
    }
}

fn format_file_glob(r: &api::FileGlobV2Result) -> (String, bool) {
    use api::file_glob_v2_result::Result as R;
    match &r.result {
        Some(R::Success(s)) => {
            let paths: Vec<String> = s
                .matched_files
                .iter()
                .map(|m| m.file_path.clone())
                .collect();
            (
                if paths.is_empty() {
                    "(no files matched)".to_string()
                } else {
                    paths.join("\n")
                },
                false,
            )
        }
        Some(R::Error(e)) => (format!("Glob error: {}", e.message), true),
        None => ("(no files matched)".to_string(), false),
    }
}

fn format_mcp(r: &api::CallMcpToolResult) -> (String, bool) {
    use api::call_mcp_tool_result::Result as R;
    match &r.result {
        Some(R::Success(s)) => {
            use api::call_mcp_tool_result::success::result::Result as Inner;
            let parts: Vec<String> = s
                .results
                .iter()
                .map(|item| match &item.result {
                    Some(Inner::Text(t)) => t.text.clone(),
                    Some(Inner::Image(_)) => "[image]".to_string(),
                    Some(Inner::Resource(_)) => "[resource]".to_string(),
                    None => String::new(),
                })
                .collect();
            (parts.join("\n"), false)
        }
        Some(R::Error(e)) => (format!("MCP tool error: {}", e.message), true),
        None => ("(no result)".to_string(), false),
    }
}

// ---- JSON Schemas for built-in tools ----

fn spec_run_shell() -> ToolSpec {
    ToolSpec {
        name: BUILTIN_RUN_SHELL.into(),
        description: "Run a shell command in the user's terminal. The command runs on the user's \
            machine in their current working directory; the user may be asked to approve it."
            .into(),
        schema: json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The shell command to run." }
            },
            "required": ["command"]
        }),
    }
}

fn spec_suggest_prompt() -> ToolSpec {
    ToolSpec {
        name: BUILTIN_SUGGEST_PROMPT.into(),
        description: "Suggest a single helpful follow-up command or action for the user, \
            shown as a one-click chip."
            .into(),
        schema: json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "The exact follow-up shell command or agent prompt to suggest"
                },
                "label": {
                    "type": "string",
                    "description": "A short human-readable label for the suggestion chip, e.g. 'Check git status'"
                }
            },
            "required": ["prompt"]
        }),
    }
}

fn spec_read_files() -> ToolSpec {
    ToolSpec {
        name: BUILTIN_READ_FILES.into(),
        description: "Read the full contents of one or more files by path.".into(),
        schema: json!({
            "type": "object",
            "properties": {
                "paths": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "File paths to read (absolute or relative to the working directory)."
                }
            },
            "required": ["paths"]
        }),
    }
}

fn spec_search_codebase() -> ToolSpec {
    ToolSpec {
        name: BUILTIN_SEARCH_CODEBASE.into(),
        description:
            "Semantically search the codebase for relevant files given a natural-language query."
                .into(),
        schema: json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "What to look for." },
                "path_filters": { "type": "array", "items": { "type": "string" } },
                "codebase_path": { "type": "string", "description": "Absolute path to the codebase to search. Optional." }
            },
            "required": ["query"]
        }),
    }
}

fn spec_grep() -> ToolSpec {
    ToolSpec {
        name: BUILTIN_GREP.into(),
        description: "Search file contents for literal strings or regex patterns.".into(),
        schema: json!({
            "type": "object",
            "properties": {
                "queries": { "type": "array", "items": { "type": "string" }, "description": "Patterns to search for." },
                "path": { "type": "string", "description": "File or directory to search in. Optional." }
            },
            "required": ["queries"]
        }),
    }
}

fn spec_file_glob() -> ToolSpec {
    ToolSpec {
        name: BUILTIN_FILE_GLOB.into(),
        description: "Find files whose names match glob patterns (supports ?, *, []).".into(),
        schema: json!({
            "type": "object",
            "properties": {
                "patterns": { "type": "array", "items": { "type": "string" } },
                "search_dir": { "type": "string", "description": "Directory to search in. Optional." },
                "max_matches": { "type": "integer" },
                "max_depth": { "type": "integer" },
                "min_depth": { "type": "integer" }
            },
            "required": ["patterns"]
        }),
    }
}

fn spec_apply_diffs() -> ToolSpec {
    ToolSpec {
        name: BUILTIN_APPLY_DIFFS.into(),
        description: "Edit files by search/replace, create new files, or delete files. Each diff \
            replaces an exact `search` string with `replace` in the given file."
            .into(),
        schema: json!({
            "type": "object",
            "properties": {
                "summary": { "type": "string", "description": "A short summary of the change." },
                "diffs": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string" },
                            "search": { "type": "string", "description": "Exact text to replace." },
                            "replace": { "type": "string", "description": "Replacement text." }
                        },
                        "required": ["file_path", "search", "replace"]
                    }
                },
                "new_files": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string" },
                            "content": { "type": "string" }
                        },
                        "required": ["file_path", "content"]
                    }
                },
                "deleted_files": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["summary"]
        }),
    }
}

// ---- small JSON accessors ----

fn str_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn str_array(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn int_field(value: &Value, key: &str) -> i32 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0) as i32
}
