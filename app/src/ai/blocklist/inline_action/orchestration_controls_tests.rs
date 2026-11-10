use ai::agent::orchestration_config::OrchestrationConfig;

use super::{should_show_harness_picker, OrchestrationEditState};

fn local_config(harness_type: &str, model_id: &str) -> OrchestrationConfig {
    OrchestrationConfig {
        model_id: model_id.to_string(),
        harness_type: harness_type.to_string(),
    }
}

#[test]
fn from_orchestration_config_preserves_local_claude() {
    let state =
        OrchestrationEditState::from_orchestration_config(&local_config("claude", "sonnet"));
    assert_eq!(state.harness_type, "claude");
    assert_eq!(state.model_id, "sonnet");
}

#[test]
fn from_orchestration_config_sanitizes_disabled_codex() {
    let state = OrchestrationEditState::from_orchestration_config(&local_config("codex", "gpt-5"));

    assert_eq!(state.harness_type, "oz");
    assert_eq!(state.model_id, "");
}

#[test]
fn harness_picker_stays_visible_for_local_mode() {
    let state = OrchestrationEditState::from_run_agents_fields("auto", "oz");
    assert!(should_show_harness_picker(&state));
}

#[test]
fn accept_disabled_reason_allows_local_claude_product() {
    let state = OrchestrationEditState::from_run_agents_fields("auto", "claude");
    assert_eq!(state.accept_disabled_reason(), None);
}

#[test]
fn resolve_from_config_preserves_local_claude() {
    let mut state = OrchestrationEditState::from_run_agents_fields("", "");

    state.resolve_from_config(&local_config("claude", "sonnet"));
    assert_eq!(state.harness_type, "claude");
    assert_eq!(state.model_id, "sonnet");
}

#[test]
fn resolve_from_config_sanitizes_disabled_local_codex() {
    let mut state = OrchestrationEditState::from_run_agents_fields("", "");

    state.resolve_from_config(&local_config("codex", "gpt-5"));

    assert_eq!(state.harness_type, "oz");
    assert_eq!(state.model_id, "");
}
