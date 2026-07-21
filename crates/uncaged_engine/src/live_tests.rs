//! Live end-to-end tests against a local model.
//!
//! These are `#[ignore]`d because they require a running OpenAI-compatible
//! server (e.g. LM Studio at 127.0.0.1:1234 with a tool-capable coding model).
//! Run explicitly:
//!
//! ```text
//! cargo test -p uncaged_engine --  --ignored --nocapture live
//! ```
//!
//! They exercise the *whole* engine — request parsing, system prompt + tool
//! schema authoring, the OpenAI-compatible provider (streaming + tool-call
//! accumulation), and the ResponseEvent wire mapping — against a real model.

use futures::StreamExt as _;

use crate::config::ProviderConfig;
use crate::config::UncagedConfig;
use crate::proto::api;

const BASE_URL: &str = "http://127.0.0.1:1234/v1";
const MODEL: &str = "qwen3-coder-30b-a3b-instruct-mlx";

fn lmstudio_config() -> UncagedConfig {
    UncagedConfig {
        enabled: true,
        provider: ProviderConfig::OpenAiCompatible {
            base_url: BASE_URL.to_string(),
            api_key: None,
            model: MODEL.to_string(),
            max_tokens: 512,
            label: Some("lmstudio".to_string()),
        },
    }
}

fn acp_config() -> UncagedConfig {
    UncagedConfig {
        enabled: true,
        provider: ProviderConfig::Acp {
            command: vec!["claude-code-acp".to_string()],
            model: Some("sonnet".to_string()),
        },
    }
}

/// Build a minimal request: an empty task (so the reply attaches to it) plus a
/// user turn whose completion needs a shell command.
fn request_with_user(query: &str, tools: Vec<api::ToolType>) -> api::Request {
    api::Request {
        task_context: Some(api::request::TaskContext {
            tasks: vec![api::Task {
                id: "task-live".into(),
                ..Default::default()
            }],
            ..Default::default()
        }),
        input: Some(api::request::Input {
            r#type: Some(api::request::input::Type::UserInputs(
                api::request::input::UserInputs {
                    inputs: vec![api::request::input::user_inputs::UserInput {
                        input: Some(
                            api::request::input::user_inputs::user_input::Input::UserQuery(
                                api::request::input::UserQuery {
                                    query: query.to_string(),
                                    ..Default::default()
                                },
                            ),
                        ),
                    }],
                    ..Default::default()
                },
            )),
            ..Default::default()
        }),
        settings: Some(api::request::Settings {
            supported_tools: tools.into_iter().map(|t| t as i32).collect(),
            ..Default::default()
        }),
        metadata: Some(api::request::Metadata {
            conversation_id: "conv-live".into(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

struct Collected {
    saw_init: bool,
    saw_finished_done: bool,
    shell_commands: Vec<String>,
    suggest_prompts: Vec<String>,
    text: String,
}

async fn run(config: &UncagedConfig, request: &api::Request) -> Collected {
    let mut stream = crate::engine::run_turn(config, request);
    let mut c = Collected {
        saw_init: false,
        saw_finished_done: false,
        shell_commands: Vec::new(),
        suggest_prompts: Vec::new(),
        text: String::new(),
    };
    while let Some(item) = stream.next().await {
        let event = item.expect("engine yielded an error");
        match event.r#type {
            Some(api::response_event::Type::Init(_)) => c.saw_init = true,
            Some(api::response_event::Type::Finished(f)) => {
                if matches!(
                    f.reason,
                    Some(api::response_event::stream_finished::Reason::Done(_))
                ) {
                    c.saw_finished_done = true;
                }
            }
            Some(api::response_event::Type::ClientActions(actions)) => {
                for action in actions.actions {
                    match action.action {
                        Some(api::client_action::Action::AddMessagesToTask(add)) => {
                            for m in add.messages {
                                collect_message(m, &mut c);
                            }
                        }
                        Some(api::client_action::Action::AppendToMessageContent(app)) => {
                            if let Some(m) = app.message {
                                collect_message(m, &mut c);
                            }
                        }
                        _ => {}
                    }
                }
            }
            None => {}
        }
    }
    c
}

fn collect_message(m: api::Message, c: &mut Collected) {
    match m.message {
        Some(api::message::Message::ToolCall(tc)) => match tc.tool {
            Some(api::message::tool_call::Tool::RunShellCommand(cmd)) => {
                c.shell_commands.push(cmd.command);
            }
            Some(api::message::tool_call::Tool::SuggestPrompt(sp)) => {
                if let Some(api::message::tool_call::suggest_prompt::DisplayMode::PromptChip(
                    chip,
                )) = sp.display_mode
                {
                    c.suggest_prompts.push(chip.prompt);
                }
            }
            _ => {}
        },
        Some(api::message::Message::AgentOutput(a)) => c.text.push_str(&a.text),
        _ => {}
    }
}

#[tokio::test]
#[ignore = "requires LM Studio (qwen3-coder) at 127.0.0.1:1234"]
async fn live_engine_emits_native_shell_tool_call() {
    let config = lmstudio_config();
    let request = request_with_user(
        "List the files in the current directory using a shell command.",
        vec![api::ToolType::RunShellCommand],
    );
    let c = run(&config, &request).await;
    eprintln!(
        "[live] init={} done={} shell_commands={:?} text={:?}",
        c.saw_init, c.saw_finished_done, c.shell_commands, c.text
    );
    assert!(c.saw_init, "engine must emit StreamInit");
    assert!(c.saw_finished_done, "engine must finish with Done");
    assert!(
        !c.shell_commands.is_empty(),
        "qwen3-coder should emit a run_shell_command tool call; got text instead: {:?}",
        c.text
    );
    assert!(
        c.shell_commands.iter().any(|cmd| cmd.contains("ls")),
        "expected an ls-like command, got {:?}",
        c.shell_commands
    );
}

#[tokio::test]
#[ignore = "requires LM Studio (qwen3-coder) at 127.0.0.1:1234"]
async fn live_engine_streams_plain_text() {
    let config = lmstudio_config();
    // No tools offered -> the model should just answer in prose (validates the
    // text streaming path: AddMessagesToTask + AppendToMessageContent deltas).
    let request = request_with_user("Say the single word: uncaged", vec![]);
    let c = run(&config, &request).await;
    eprintln!(
        "[live] init={} done={} text={:?}",
        c.saw_init, c.saw_finished_done, c.text
    );
    assert!(c.saw_init && c.saw_finished_done);
    assert!(!c.text.trim().is_empty(), "expected streamed prose");
}

/// Build a passive-suggestion request the way the client does after a shell
/// command completes.
fn passive_suggestion_request(command: &str, output: &str, exit_code: i32) -> api::Request {
    use api::request::input::generate_passive_suggestions as gps;
    api::Request {
        input: Some(api::request::Input {
            r#type: Some(api::request::input::Type::GeneratePassiveSuggestions(
                api::request::input::GeneratePassiveSuggestions {
                    trigger: Some(gps::Trigger::ShellCommandCompleted(
                        gps::ShellCommandCompleted {
                            executed_shell_command: Some(api::ExecutedShellCommand {
                                command: command.to_string(),
                                output: output.to_string(),
                                exit_code,
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
        metadata: Some(api::request::Metadata {
            conversation_id: "conv-live-passive".into(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[tokio::test]
#[ignore = "requires LM Studio (qwen3-coder) at 127.0.0.1:1234"]
async fn live_engine_emits_passive_suggestion_chip() {
    let config = lmstudio_config();
    // A failed `git stat` should elicit a `suggest_prompt` tool call proposing the
    // corrected command (e.g. `git status`). Exercises the whole passive path end
    // to end: request lowering + SuggestPrompt tool schema + the model choosing to
    // call it + the wire mapping the client turns into a chip.
    let request = passive_suggestion_request(
        "git stat",
        "git: 'stat' is not a git command. See 'git --help'.\n\nThe most similar command is\n\tstatus",
        1,
    );
    let c = run(&config, &request).await;
    eprintln!(
        "[live] init={} done={} suggest_prompts={:?} text={:?}",
        c.saw_init, c.saw_finished_done, c.suggest_prompts, c.text
    );
    assert!(c.saw_init && c.saw_finished_done);
    assert!(
        !c.suggest_prompts.is_empty(),
        "qwen3-coder should call suggest_prompt with a corrected command; got text instead: {:?}",
        c.text
    );
    assert!(
        c.suggest_prompts.iter().any(|p| p.contains("git")),
        "expected a git-related suggestion, got {:?}",
        c.suggest_prompts
    );
}

#[tokio::test]
#[ignore = "requires claude-code-acp on PATH + a logged-in Claude Code"]
async fn live_acp_responds_and_reuses_session() {
    let config = acp_config();
    // Turn 1 establishes context in the (persistent) CLI session.
    let c1 = run(
        &config,
        &request_with_user("Remember the number 42. Reply with only: OK", vec![]),
    )
    .await;
    eprintln!(
        "[acp] turn1 init={} done={} text={:?}",
        c1.saw_init, c1.saw_finished_done, c1.text
    );
    assert!(
        c1.saw_init && c1.saw_finished_done,
        "ACP turn should init + finish"
    );
    assert!(
        !c1.text.trim().is_empty(),
        "Claude Code should reply — empty means the CLI didn't spawn/answer"
    );
    // Turn 2 reuses the SAME conversation id (request_with_user hardcodes it), so
    // the persistent session must still remember the number from turn 1.
    let c2 = run(
        &config,
        &request_with_user(
            "What number did I ask you to remember? Reply with just the number.",
            vec![],
        ),
    )
    .await;
    eprintln!("[acp] turn2 text={:?}", c2.text);
    assert!(
        c2.text.contains("42"),
        "session reuse should retain context across turns; got {:?}",
        c2.text
    );
}
