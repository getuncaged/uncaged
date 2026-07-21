//! Behavioral tests for the Uncaged engine. These run against the real
//! generated `warp_multi_agent_api` types, so they validate both the protocol
//! mapping and the conversation logic.

use serde_json::json;

use crate::config::ProviderConfig;
use crate::config::UncagedConfig;
use crate::model::NeutralMsg;
use crate::proto::api;
use crate::request_parse;
use crate::tools::ToolRegistry;
use crate::tools::decode_history_result;
use crate::wire;

// ---- config round-trips (must match what script/uncaged-setup writes) ----

#[test]
fn config_openai_compatible_round_trips() {
    let raw = r#"{
        "enabled": true,
        "provider": {
            "kind": "openai_compatible",
            "base_url": "http://localhost:11434/v1",
            "model": "llama3.1:8b",
            "max_tokens": 8192,
            "label": "ollama"
        }
    }"#;
    let cfg: UncagedConfig = serde_json::from_str(raw).unwrap();
    assert!(cfg.enabled);
    match cfg.provider {
        ProviderConfig::OpenAiCompatible {
            model, base_url, ..
        } => {
            assert_eq!(model, "llama3.1:8b");
            assert_eq!(base_url, "http://localhost:11434/v1");
        }
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn config_anthropic_round_trips() {
    let raw = r#"{"enabled":true,"provider":{"kind":"anthropic","api_key":"sk-ant-x","model":"claude-sonnet-4-5"}}"#;
    let cfg: UncagedConfig = serde_json::from_str(raw).unwrap();
    match cfg.provider {
        ProviderConfig::Anthropic {
            model,
            base_url,
            max_tokens,
            ..
        } => {
            assert_eq!(model, "claude-sonnet-4-5");
            // base_url + max_tokens come from serde defaults.
            assert_eq!(base_url, crate::config::ANTHROPIC_DEFAULT_BASE_URL);
            assert_eq!(max_tokens, 8192);
        }
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn config_acp_round_trips() {
    let raw = r#"{"enabled":true,"provider":{"kind":"acp","command":["claude-code-acp"]}}"#;
    let cfg: UncagedConfig = serde_json::from_str(raw).unwrap();
    match cfg.provider {
        ProviderConfig::Acp { command, .. } => assert_eq!(command, vec!["claude-code-acp"]),
        other => panic!("wrong variant: {other:?}"),
    }
}

#[test]
fn config_serializes_with_expected_tag() {
    let cfg = UncagedConfig {
        enabled: true,
        provider: ProviderConfig::OpenAiCompatible {
            base_url: "http://localhost:1234/v1".into(),
            api_key: None,
            model: "x".into(),
            max_tokens: 8192,
            label: Some("lmstudio".into()),
        },
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(
        json.contains("\"kind\":\"openai_compatible\""),
        "tag mismatch: {json}"
    );
}

// ---- tool registry: schema, encode, decode ----

#[test]
fn registry_exposes_supported_builtins_only() {
    let request = api::Request {
        settings: Some(api::request::Settings {
            supported_tools: vec![api::ToolType::RunShellCommand as i32],
            ..Default::default()
        }),
        ..Default::default()
    };
    let registry = ToolRegistry::build(&request);
    let names: Vec<&str> = registry.specs.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"run_shell_command"));
    assert!(
        !names.contains(&"grep"),
        "grep should not be offered when unsupported"
    );
}

#[test]
fn encode_run_shell_command() {
    let request = api::Request::default();
    let registry = ToolRegistry::build(&request);
    let tool = registry
        .encode("run_shell_command", &json!({ "command": "ls -la" }))
        .expect("should encode");
    match tool {
        api::message::tool_call::Tool::RunShellCommand(c) => assert_eq!(c.command, "ls -la"),
        other => panic!("wrong tool: {other:?}"),
    }
}

// ---- attached context (regression: pinned blocks must reach the model) ----

#[test]
fn attached_block_context_is_surfaced_to_the_model() {
    // A user pins a terminal block ("Attach as agent context") and asks a question.
    // The block rides on `input.context.executed_shell_commands`; the engine must
    // fold it into the user turn (it used to be silently dropped).
    let request = api::Request {
        input: Some(api::request::Input {
            context: Some(api::InputContext {
                executed_shell_commands: vec![api::ExecutedShellCommand {
                    command: "git stus".to_string(),
                    output: "git: 'stus' is not a git command.".to_string(),
                    ..Default::default()
                }],
                selected_text: vec![api::input_context::SelectedText {
                    text: "let x = broken;".to_string(),
                }],
                ..Default::default()
            }),
            // The context path runs independently of the query type; leaving this
            // `None` isolates the regression (attached context being dropped).
            r#type: None,
        }),
        ..Default::default()
    };

    let parsed = request_parse::parse(&request);
    let user_text = parsed
        .messages
        .iter()
        .filter_map(|m| match m {
            NeutralMsg::User(t) => Some(t.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        user_text.contains("git stus"),
        "attached command missing:\n{user_text}"
    );
    assert!(
        user_text.contains("not a git command"),
        "attached output missing:\n{user_text}"
    );
    assert!(
        user_text.contains("let x = broken;"),
        "selected text missing:\n{user_text}"
    );
}

// ---- passive suggestions (chip after a shell command completes) ----

#[test]
fn passive_suggestion_request_prompts_for_a_tool_call() {
    // The client fires `GeneratePassiveSuggestions` after `git stat` fails. The
    // engine must (a) surface the command in a user turn that instructs the model
    // to call `suggest_prompt`, and (b) actually advertise that tool.
    use api::request::input::GeneratePassiveSuggestions;
    use api::request::input::generate_passive_suggestions as gps;

    let request = api::Request {
        input: Some(api::request::Input {
            r#type: Some(api::request::input::Type::GeneratePassiveSuggestions(
                GeneratePassiveSuggestions {
                    trigger: Some(gps::Trigger::ShellCommandCompleted(
                        gps::ShellCommandCompleted {
                            executed_shell_command: Some(api::ExecutedShellCommand {
                                command: "git stat".into(),
                                output: "git: 'stat' is not a git command.".into(),
                                exit_code: 1,
                                ..Default::default()
                            }),
                            ..Default::default()
                        },
                    )),
                    ..Default::default()
                },
            )),
            ..Default::default()
        }),
        settings: Some(api::request::Settings {
            supported_tools: vec![api::ToolType::SuggestPrompt as i32],
            ..Default::default()
        }),
        ..Default::default()
    };

    let parsed = request_parse::parse(&request);
    let user_text = parsed
        .messages
        .iter()
        .filter_map(|m| match m {
            NeutralMsg::User(t) => Some(t.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        user_text.contains("git stat"),
        "command missing:\n{user_text}"
    );
    assert!(
        user_text.contains("suggest_prompt"),
        "directive to call the tool missing:\n{user_text}"
    );

    // The tool must be offered, or the model has nothing to call.
    let registry = ToolRegistry::build(&request);
    let names: Vec<&str> = registry.specs.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"suggest_prompt"),
        "suggest_prompt not advertised: {names:?}"
    );
}

#[test]
fn encode_apply_file_diffs() {
    let registry = ToolRegistry::build(&api::Request::default());
    let tool = registry
        .encode(
            "apply_file_diffs",
            &json!({
                "summary": "tweak",
                "diffs": [{ "file_path": "a.rs", "search": "foo", "replace": "bar" }]
            }),
        )
        .expect("should encode");
    match tool {
        api::message::tool_call::Tool::ApplyFileDiffs(a) => {
            assert_eq!(a.summary, "tweak");
            assert_eq!(a.diffs.len(), 1);
            assert_eq!(a.diffs[0].file_path, "a.rs");
            assert_eq!(a.diffs[0].replace, "bar");
        }
        other => panic!("wrong tool: {other:?}"),
    }
}

#[test]
fn encode_unknown_tool_returns_none() {
    let registry = ToolRegistry::build(&api::Request::default());
    assert!(
        registry
            .encode("definitely_not_a_tool", &json!({}))
            .is_none()
    );
}

#[test]
fn decode_shell_result_includes_output_and_exit() {
    let result = api::message::ToolCallResult {
        tool_call_id: "t1".into(),
        result: Some(api::message::tool_call_result::Result::RunShellCommand(
            api::RunShellCommandResult {
                command: "echo hi".into(),
                result: Some(api::run_shell_command_result::Result::CommandFinished(
                    api::ShellCommandFinished {
                        output: "hi".into(),
                        exit_code: 0,
                        ..Default::default()
                    },
                )),
                ..Default::default()
            },
        )),
        ..Default::default()
    };
    let decoded = decode_history_result(&result);
    assert_eq!(decoded.id, "t1");
    assert!(decoded.content.contains("hi"));
    assert!(decoded.content.contains("exit code 0"));
    assert!(!decoded.is_error);
}

#[test]
fn decode_shell_result_nonzero_exit_is_error() {
    let result = api::message::ToolCallResult {
        tool_call_id: "t2".into(),
        result: Some(api::message::tool_call_result::Result::RunShellCommand(
            api::RunShellCommandResult {
                command: "false".into(),
                result: Some(api::run_shell_command_result::Result::CommandFinished(
                    api::ShellCommandFinished {
                        output: String::new(),
                        exit_code: 1,
                        ..Default::default()
                    },
                )),
                ..Default::default()
            },
        )),
        ..Default::default()
    };
    assert!(decode_history_result(&result).is_error);
}

// ---- request parsing: coalescing + dedup ----

#[test]
fn parse_builds_coalesced_deduped_conversation() {
    // History: a user query, an agent reply, a tool call.
    let history_task = api::Task {
        id: "task-1".into(),
        messages: vec![
            message_with(api::message::Message::UserQuery(api::message::UserQuery {
                query: "list files".into(),
                ..Default::default()
            })),
            message_with(api::message::Message::AgentOutput(
                api::message::AgentOutput {
                    text: "Sure.".into(),
                },
            )),
            message_with(api::message::Message::ToolCall(api::message::ToolCall {
                tool_call_id: "tc-1".into(),
                tool: Some(api::message::tool_call::Tool::RunShellCommand(
                    api::message::tool_call::RunShellCommand {
                        command: "ls".into(),
                        ..Default::default()
                    },
                )),
                ..Default::default()
            })),
        ],
        ..Default::default()
    };

    // New input: the tool result the client just produced.
    let tool_result_input = api::request::input::user_inputs::UserInput {
        input: Some(
            api::request::input::user_inputs::user_input::Input::ToolCallResult(
                api::request::input::ToolCallResult {
                    tool_call_id: "tc-1".into(),
                    result: Some(
                        api::request::input::tool_call_result::Result::RunShellCommand(
                            api::RunShellCommandResult {
                                command: "ls".into(),
                                result: Some(
                                    api::run_shell_command_result::Result::CommandFinished(
                                        api::ShellCommandFinished {
                                            output: "a.txt".into(),
                                            exit_code: 0,
                                            ..Default::default()
                                        },
                                    ),
                                ),
                                ..Default::default()
                            },
                        ),
                    ),
                },
            ),
        ),
    };

    let request = api::Request {
        task_context: Some(api::request::TaskContext {
            tasks: vec![history_task],
            ..Default::default()
        }),
        input: Some(api::request::Input {
            r#type: Some(api::request::input::Type::UserInputs(
                api::request::input::UserInputs {
                    inputs: vec![tool_result_input],
                    ..Default::default()
                },
            )),
            ..Default::default()
        }),
        metadata: Some(api::request::Metadata {
            conversation_id: "conv-1".into(),
            ..Default::default()
        }),
        ..Default::default()
    };

    let parsed = request_parse::parse(&request);
    assert_eq!(parsed.conversation_id, "conv-1");
    assert_eq!(parsed.target_task_id.as_deref(), Some("task-1"));

    // Expect: User("list files"), Assistant{text+tool_use}, ToolResults[1].
    assert_eq!(parsed.messages.len(), 3);
    assert!(matches!(parsed.messages[0], NeutralMsg::User(ref t) if t == "list files"));
    match &parsed.messages[1] {
        NeutralMsg::Assistant { text, tool_uses } => {
            assert_eq!(text.as_deref(), Some("Sure."));
            assert_eq!(tool_uses.len(), 1);
            assert_eq!(tool_uses[0].id, "tc-1");
            assert_eq!(tool_uses[0].name, "run_shell_command");
        }
        other => panic!("expected assistant turn, got {other:?}"),
    }
    match &parsed.messages[2] {
        NeutralMsg::ToolResults(results) => {
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].id, "tc-1");
            assert!(results[0].content.contains("a.txt"));
        }
        other => panic!("expected tool results, got {other:?}"),
    }
}

// ---- wire builders ----

#[test]
fn wire_init_and_finished_have_expected_variants() {
    let init = wire::init_event("c", "r", "run");
    assert!(matches!(
        init.r#type,
        Some(api::response_event::Type::Init(_))
    ));
    let finished = wire::finished_done();
    match finished.r#type {
        Some(api::response_event::Type::Finished(f)) => assert!(matches!(
            f.reason,
            Some(api::response_event::stream_finished::Reason::Done(_))
        )),
        _ => panic!("expected finished event"),
    }
}

#[test]
fn wire_append_text_targets_agent_output() {
    let action = wire::append_text("task-1", "msg-1", "hello");
    match action.action {
        Some(api::client_action::Action::AppendToMessageContent(append)) => {
            assert_eq!(append.task_id, "task-1");
            let mask = append.mask.expect("mask present");
            assert_eq!(mask.paths, vec!["agent_output.text".to_string()]);
        }
        other => panic!("wrong action: {other:?}"),
    }
}

fn message_with(message: api::message::Message) -> api::Message {
    api::Message {
        message: Some(message),
        ..Default::default()
    }
}
