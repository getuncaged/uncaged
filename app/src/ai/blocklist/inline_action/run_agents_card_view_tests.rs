use ai::agent::action::{RunAgentsAgentRunConfig, RunAgentsRequest};
use ai::agent::action_result::{RunAgentsAgentOutcome, RunAgentsAgentOutcomeKind, RunAgentsResult};

use super::RunAgentsEditState;
use crate::ai::blocklist::inline_action::orchestration_controls::OrchestrationEditState;

fn make_request(harness: &str) -> RunAgentsRequest {
    RunAgentsRequest {
        summary: "summary".to_string(),
        base_prompt: "base".to_string(),
        model_id: "auto".to_string(),
        harness_type: harness.to_string(),
        agent_run_configs: vec![RunAgentsAgentRunConfig {
            name: "child".to_string(),
            prompt: "do work".to_string(),
        }],
        plan_id: String::new(),
    }
}

fn make_edit_state_with_orch_fields(harness: &str) -> RunAgentsEditState {
    let request = make_request(harness);
    RunAgentsEditState {
        orch: OrchestrationEditState::from_run_agents_fields(
            &request.model_id,
            &request.harness_type,
        ),
        agent_run_configs: request.agent_run_configs,
        base_prompt: request.base_prompt,
        summary: request.summary,
        plan_id: request.plan_id,
    }
}

#[test]
fn local_with_any_harness_does_not_disable_accept() {
    for harness in ["oz", "claude", "gemini", "opencode"] {
        let state = RunAgentsEditState::from_request(&make_request(harness));
        assert!(
            state.orch.accept_disabled_reason().is_none(),
            "Local + {harness} should allow Accept"
        );
    }
}

#[test]
fn local_with_disabled_codex_disables_accept() {
    let state = make_edit_state_with_orch_fields("codex");
    assert_eq!(
        state.orch.accept_disabled_reason(),
        Some("Local Codex child agents are temporarily disabled.")
    );
}

#[test]
fn from_request_sanitizes_disabled_local_harness_to_oz() {
    let state = RunAgentsEditState::from_request(&make_request("codex"));

    assert_eq!(state.orch.harness_type, "oz");
    assert_eq!(state.orch.model_id, "");
    assert!(state.orch.accept_disabled_reason().is_none());
}

#[test]
fn to_request_round_trips_request_fields() {
    let mut req = make_request("claude");
    req.plan_id = "plan-1".to_string();
    let state = RunAgentsEditState::from_request(&req);
    assert_eq!(state.to_request(), req);
}

mod format_terminal_state_tests {
    use super::super::{format_terminal_state, StatusKind};
    use super::*;

    fn launched(name: &str, agent_id: &str) -> RunAgentsAgentOutcome {
        RunAgentsAgentOutcome {
            name: name.to_string(),
            kind: RunAgentsAgentOutcomeKind::Launched {
                agent_id: agent_id.to_string(),
            },
        }
    }

    fn failed(name: &str, error: &str) -> RunAgentsAgentOutcome {
        RunAgentsAgentOutcome {
            name: name.to_string(),
            kind: RunAgentsAgentOutcomeKind::Failed {
                error: error.to_string(),
            },
        }
    }

    fn launched_result(agents: Vec<RunAgentsAgentOutcome>) -> RunAgentsResult {
        RunAgentsResult::Launched {
            model_id: "auto".to_string(),
            harness_type: "oz".to_string(),
            agents,
        }
    }

    #[test]
    fn launched_singular_uses_singular_label() {
        let result = launched_result(vec![launched("child", "a-1")]);
        let (label, kind) = format_terminal_state(&result);
        assert_eq!(label, "Spawned 1 agent");
        assert!(matches!(kind, StatusKind::Success));
    }

    #[test]
    fn launched_plural_uses_plural_label() {
        let result = launched_result(vec![
            launched("a", "a-1"),
            launched("b", "a-2"),
            launched("c", "a-3"),
        ]);
        let (label, kind) = format_terminal_state(&result);
        assert_eq!(label, "Spawned 3 agents");
        assert!(matches!(kind, StatusKind::Success));
    }

    #[test]
    fn launched_partial_uses_x_of_y_label_and_mixed_status() {
        let result = launched_result(vec![
            launched("a", "a-1"),
            failed("b", "boom"),
            launched("c", "a-3"),
        ]);
        let (label, kind) = format_terminal_state(&result);
        assert_eq!(label, "Spawned 2 of 3 agents");
        assert!(matches!(kind, StatusKind::Mixed));
    }

    #[test]
    fn all_failed_uses_failure_status_not_mixed() {
        let result = launched_result(vec![
            failed("a", "boom"),
            failed("b", "boom"),
            failed("c", "boom"),
        ]);
        let (label, kind) = format_terminal_state(&result);
        assert_eq!(label, "Failed to spawn 3 agents");
        assert!(matches!(kind, StatusKind::Failure));
    }

    #[test]
    fn single_failed_uses_singular_failure_label() {
        let result = launched_result(vec![failed("a", "boom")]);
        let (label, kind) = format_terminal_state(&result);
        assert_eq!(label, "Failed to spawn agent");
        assert!(matches!(kind, StatusKind::Failure));
    }

    #[test]
    fn failure_with_error_includes_error_text() {
        let (label, kind) = format_terminal_state(&RunAgentsResult::Failure {
            error: "server rejected request".to_string(),
        });
        assert_eq!(
            label,
            "Failed to start orchestration: server rejected request"
        );
        assert!(matches!(kind, StatusKind::Failure));
    }

    #[test]
    fn failure_with_empty_error_uses_short_label() {
        let (label, kind) = format_terminal_state(&RunAgentsResult::Failure {
            error: String::new(),
        });
        assert_eq!(label, "Failed to start orchestration");
        assert!(matches!(kind, StatusKind::Failure));
    }

    #[test]
    fn denied_with_reason_appends_reason() {
        let (label, kind) = format_terminal_state(&RunAgentsResult::Denied {
            reason: "disapproved".to_string(),
        });
        assert!(label.contains("disapproved"));
        assert!(matches!(kind, StatusKind::Cancelled));
    }

    #[test]
    fn denied_without_reason_uses_short_label() {
        let (label, kind) = format_terminal_state(&RunAgentsResult::Denied {
            reason: String::new(),
        });
        assert!(!label.contains("()"));
        assert!(matches!(kind, StatusKind::Cancelled));
    }

    #[test]
    fn cancelled_uses_cancelled_status() {
        let (label, kind) = format_terminal_state(&RunAgentsResult::Cancelled);
        assert_eq!(label, "Spawn agents cancelled");
        assert!(matches!(kind, StatusKind::Cancelled));
    }
}

mod override_from_approved_config_tests {
    use ai::agent::orchestration_config::OrchestrationConfig;

    use super::super::RunAgentsEditState;
    use super::*;

    fn local_config(model: &str, harness: &str) -> OrchestrationConfig {
        OrchestrationConfig {
            model_id: model.to_string(),
            harness_type: harness.to_string(),
        }
    }

    #[test]
    fn overrides_model_and_harness_unconditionally() {
        let mut state = RunAgentsEditState::from_request(&make_request("oz"));
        assert_eq!(state.orch.model_id, "auto");
        assert_eq!(state.orch.harness_type, "oz");

        state
            .orch
            .override_from_approved_config(&local_config("claude-4-opus", "claude"));
        assert_eq!(state.orch.model_id, "claude-4-opus");
        assert_eq!(state.orch.harness_type, "claude");
    }

    #[test]
    fn overrides_even_when_request_has_values() {
        let mut state = RunAgentsEditState::from_request(&make_request("claude"));
        state
            .orch
            .override_from_approved_config(&local_config("gpt-5", "codex"));
        assert_eq!(state.orch.model_id, "gpt-5");
        assert_eq!(state.orch.harness_type, "codex");
    }

    #[test]
    fn approved_local_disabled_harness_reports_disabled_reason_after_override() {
        let mut state = RunAgentsEditState::from_request(&make_request("oz"));
        state
            .orch
            .override_from_approved_config(&local_config("auto", "codex"));
        assert_eq!(
            state.orch.accept_disabled_reason(),
            Some("Local Codex child agents are temporarily disabled.")
        );
    }
}
