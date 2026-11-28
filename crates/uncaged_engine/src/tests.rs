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

// ---- tool calls and their results (#43) ----

/// A history `ToolCall` message running `command`.
fn shell_call(id: &str, command: &str) -> api::Message {
    message_with(api::message::Message::ToolCall(api::message::ToolCall {
        tool_call_id: id.into(),
        tool: Some(api::message::tool_call::Tool::RunShellCommand(
            api::message::tool_call::RunShellCommand {
                command: command.into(),
                ..Default::default()
            },
        )),
    }))
}

fn user_query(text: &str) -> api::Message {
    message_with(api::message::Message::UserQuery(api::message::UserQuery {
        query: text.into(),
        ..Default::default()
    }))
}

fn agent_text(text: &str) -> api::Message {
    message_with(api::message::Message::AgentOutput(
        api::message::AgentOutput { text: text.into() },
    ))
}

/// A request continuing `history` with a new typed `query`.
fn follow_up_request(history: Vec<api::Message>, query: &str) -> api::Request {
    api::Request {
        task_context: Some(api::request::TaskContext {
            tasks: vec![api::Task {
                id: "task-1".into(),
                messages: history,
                ..Default::default()
            }],
        }),
        input: Some(api::request::Input {
            r#type: Some(api::request::input::Type::UserInputs(
                api::request::input::UserInputs {
                    inputs: vec![api::request::input::user_inputs::UserInput {
                        input: Some(
                            api::request::input::user_inputs::user_input::Input::UserQuery(
                                api::request::input::UserQuery {
                                    query: query.into(),
                                    ..Default::default()
                                },
                            ),
                        ),
                    }],
                },
            )),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// The rule OpenAI-compatible and Anthropic backends enforce: every tool call
/// is immediately followed by a result for it, and every result answers a tool
/// call in the assistant turn right before it.
fn assert_tool_calls_answered(messages: &[NeutralMsg]) {
    for (i, message) in messages.iter().enumerate() {
        match message {
            NeutralMsg::Assistant { tool_uses, .. } if !tool_uses.is_empty() => {
                let Some(NeutralMsg::ToolResults(results)) = messages.get(i + 1) else {
                    panic!("tool calls at {i} aren't followed by results: {messages:#?}");
                };
                for call in tool_uses {
                    assert!(
                        results.iter().any(|r| r.id == call.id),
                        "tool call {} has no result: {messages:#?}",
                        call.id
                    );
                }
            }
            NeutralMsg::ToolResults(results) => {
                let Some(NeutralMsg::Assistant { tool_uses, .. }) =
                    i.checked_sub(1).and_then(|p| messages.get(p))
                else {
                    panic!("tool results at {i} don't follow tool calls: {messages:#?}");
                };
                for result in results {
                    assert!(
                        tool_uses.iter().any(|c| c.id == result.id),
                        "result {} answers no tool call: {messages:#?}",
                        result.id
                    );
                }
            }
            _ => {}
        }
    }
}

#[test]
fn a_tool_call_whose_result_was_never_recorded_still_gets_one() {
    // The conversation from #43: an earlier round ran a shell command, but its
    // result never made it into the task, then the model replied and the user
    // asked something else.
    let request = follow_up_request(
        vec![
            user_query("list files"),
            shell_call("run_shell_command:1", "ls"),
            agent_text("There are two files."),
            user_query("now count the lines"),
        ],
        "now count the lines",
    );

    let parsed = request_parse::parse(&request);
    assert_tool_calls_answered(&parsed.messages);
}

/// The result of running `ls`, as the client sends it in `input`.
fn shell_result(id: &str, output: &str) -> api::request::input::ToolCallResult {
    api::request::input::ToolCallResult {
        tool_call_id: id.into(),
        result: Some(
            api::request::input::tool_call_result::Result::RunShellCommand(
                api::RunShellCommandResult {
                    command: "ls".into(),
                    result: Some(api::run_shell_command_result::Result::CommandFinished(
                        api::ShellCommandFinished {
                            output: output.into(),
                            exit_code: 0,
                            ..Default::default()
                        },
                    )),
                    ..Default::default()
                },
            ),
        ),
    }
}

/// A request sending `results` for tool calls in `history`.
fn tool_results_request(
    history: Vec<api::Message>,
    results: Vec<api::request::input::ToolCallResult>,
) -> api::Request {
    let inputs = results
        .into_iter()
        .map(|result| api::request::input::user_inputs::UserInput {
            input: Some(
                api::request::input::user_inputs::user_input::Input::ToolCallResult(result),
            ),
        })
        .collect();
    let mut request = follow_up_request(history, "");
    request.input = Some(api::request::Input {
        r#type: Some(api::request::input::Type::UserInputs(
            api::request::input::UserInputs { inputs },
        )),
        ..Default::default()
    });
    request
}

#[test]
fn a_missing_result_gets_a_placeholder_and_stray_results_are_dropped() {
    use crate::model::MISSING_TOOL_RESULT;
    use crate::model::ToolResult;
    use crate::model::ToolUse;
    use crate::model::answer_tool_calls;

    let call = |id: &str| ToolUse {
        id: id.into(),
        name: "run_shell_command".into(),
        input: json!({}),
    };
    let result = |id: &str| ToolResult {
        id: id.into(),
        content: format!("output of {id}"),
        is_error: false,
    };
    let messages = answer_tool_calls(vec![
        NeutralMsg::User("go".into()),
        // Two calls; only the second has a result, plus one that answers
        // nothing.
        NeutralMsg::Assistant {
            text: None,
            tool_uses: vec![call("a"), call("b")],
        },
        NeutralMsg::ToolResults(vec![result("stray"), result("b")]),
        // A result with no tool call before it at all.
        NeutralMsg::ToolResults(vec![result("orphan")]),
        NeutralMsg::User("next".into()),
    ]);
    assert_tool_calls_answered(&messages);
    assert_eq!(messages.len(), 4);
    let NeutralMsg::ToolResults(results) = &messages[2] else {
        panic!("expected tool results: {messages:#?}");
    };
    // In call order: a placeholder for `a`, then `b`'s real result.
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].id, "a");
    assert_eq!(results[0].content, MISSING_TOOL_RESULT);
    assert!(results[0].is_error);
    assert_eq!(results[1].id, "b");
    assert_eq!(results[1].content, "output of b");
}

#[test]
fn a_new_tool_result_is_reported_for_recording_once() {
    let history = vec![user_query("list files"), shell_call("tc-1", "ls")];

    // Fresh from the client: not in the history yet, so it's reported.
    let parsed = request_parse::parse(&tool_results_request(
        history.clone(),
        vec![shell_result("tc-1", "a.txt")],
    ));
    assert_eq!(parsed.new_tool_results.len(), 1);
    assert_eq!(parsed.new_tool_results[0].tool_call_id, "tc-1");

    // Already in the history (recorded by an earlier turn): not reported again.
    let mut recorded = history;
    recorded.push(wire::tool_call_result_message(
        "m-1",
        &shell_result("tc-1", "a.txt"),
    ));
    let parsed = request_parse::parse(&tool_results_request(
        recorded,
        vec![shell_result("tc-1", "a.txt")],
    ));
    assert!(parsed.new_tool_results.is_empty());
    assert_tool_calls_answered(&parsed.messages);
}

#[test]
fn a_recorded_tool_result_reads_back_the_same() {
    let input = shell_result("tc-1", "a.txt");
    let message = wire::tool_call_result_message("m-1", &input);
    let Some(api::message::Message::ToolCallResult(recorded)) = &message.message else {
        panic!("expected a tool call result message: {message:?}");
    };
    let read_back = decode_history_result(recorded);
    let original = crate::tools::decode_input_result(&input);
    assert_eq!(message.id, "m-1");
    assert_eq!(read_back.id, original.id);
    assert_eq!(read_back.content, original.content);
    assert_eq!(read_back.is_error, original.is_error);
}

/// A provider that replies "Done." and keeps what it was sent.
struct FakeProvider {
    sent: std::sync::Arc<std::sync::Mutex<Option<crate::model::Conversation>>>,
}

impl crate::providers::Provider for FakeProvider {
    fn stream(
        &self,
        conversation: crate::model::Conversation,
    ) -> futures::stream::BoxStream<'static, crate::providers::ProviderEvent> {
        use futures::StreamExt as _;
        *self.sent.lock().unwrap() = Some(conversation);
        futures::stream::iter(vec![
            crate::providers::ProviderEvent::TextDelta("Done.".into()),
            crate::providers::ProviderEvent::Done,
        ])
        .boxed()
    }
}

/// Run one turn of `request` against [`FakeProvider`]: the client actions the
/// engine sent, in order, and the conversation the provider got.
async fn run_fake_turn(
    request: &api::Request,
) -> (Vec<api::client_action::Action>, crate::model::Conversation) {
    use futures::StreamExt as _;
    let sent = std::sync::Arc::new(std::sync::Mutex::new(None));
    let provider = FakeProvider { sent: sent.clone() };
    let events: Vec<_> = crate::engine::run_turn_with(request, |_| Box::new(provider))
        .collect()
        .await;
    let actions = events
        .into_iter()
        .filter_map(|event| match event.unwrap().r#type {
            Some(api::response_event::Type::ClientActions(actions)) => Some(actions.actions),
            _ => None,
        })
        .flatten()
        .filter_map(|action| action.action)
        .collect();
    let conversation = sent
        .lock()
        .unwrap()
        .take()
        .expect("the provider was called");
    (actions, conversation)
}

#[tokio::test]
async fn a_turn_records_its_tool_results_before_the_reply() {
    let history = vec![user_query("list files"), shell_call("tc-1", "ls")];
    let request = tool_results_request(history.clone(), vec![shell_result("tc-1", "a.txt")]);

    let (actions, conversation) = run_fake_turn(&request).await;
    assert_tool_calls_answered(&conversation.messages);

    // The result is added to the task first, then the reply.
    let added: Vec<_> = actions
        .iter()
        .filter_map(|action| match action {
            api::client_action::Action::AddMessagesToTask(add) => Some(add),
            _ => None,
        })
        .collect();
    assert_eq!(added.len(), 2, "{actions:#?}");
    assert_eq!(added[0].task_id, "task-1");
    let recorded = &added[0].messages[0];
    let Some(api::message::Message::ToolCallResult(result)) = &recorded.message else {
        panic!("expected the tool result first: {actions:#?}");
    };
    assert_eq!(result.tool_call_id, "tc-1");
    assert!(matches!(
        added[1].messages[0].message,
        Some(api::message::Message::AgentOutput(_))
    ));

    // The client applies those actions to the task; the next request then
    // replays the call with its real result, not a placeholder.
    let mut next = history;
    next.push(recorded.clone());
    next.push(added[1].messages[0].clone());
    next.push(user_query("now count them"));
    let (_, conversation) = run_fake_turn(&follow_up_request(next, "now count them")).await;
    assert_tool_calls_answered(&conversation.messages);
    let results: Vec<_> = conversation
        .messages
        .iter()
        .filter_map(|m| match m {
            NeutralMsg::ToolResults(results) => Some(results),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(results.len(), 1);
    assert!(results[0].content.contains("a.txt"), "{results:#?}");
}
