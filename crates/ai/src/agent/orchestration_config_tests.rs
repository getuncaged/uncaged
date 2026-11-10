use super::*;
use crate::agent::action::{RunAgentsAgentRunConfig, RunAgentsRequest};

fn make_config(model: &str, harness: &str) -> OrchestrationConfig {
    OrchestrationConfig {
        model_id: model.to_string(),
        harness_type: harness.to_string(),
    }
}

fn make_request(model: &str, harness: &str) -> RunAgentsRequest {
    RunAgentsRequest {
        summary: "test".to_string(),
        base_prompt: "prompt".to_string(),
        model_id: model.to_string(),
        harness_type: harness.to_string(),
        agent_run_configs: vec![RunAgentsAgentRunConfig {
            name: "a".to_string(),
            prompt: String::new(),
        }],
        plan_id: String::new(),
    }
}

#[test]
fn exact_match_local() {
    let config = make_config("auto", "oz");
    let request = make_request("auto", "oz");
    assert!(matches_active_config(&request, &config));
}

#[test]
fn empty_model_inherits_and_matches() {
    let config = make_config("auto", "oz");
    let request = make_request("", "oz");
    assert!(matches_active_config(&request, &config));
}

#[test]
fn empty_harness_inherits_and_matches() {
    let config = make_config("auto", "oz");
    let request = make_request("auto", "");
    assert!(matches_active_config(&request, &config));
}

#[test]
fn different_model_mismatches() {
    let config = make_config("auto", "oz");
    let request = make_request("claude-4-6-opus-high", "oz");
    assert!(!matches_active_config(&request, &config));
}

#[test]
fn different_harness_mismatches() {
    let config = make_config("auto", "oz");
    let request = make_request("auto", "claude");
    assert!(!matches_active_config(&request, &config));
}

#[test]
fn status_default_is_none() {
    assert_eq!(
        OrchestrationConfigStatus::default(),
        OrchestrationConfigStatus::None
    );
}

#[test]
fn status_predicates() {
    assert!(OrchestrationConfigStatus::Approved.is_approved());
    assert!(!OrchestrationConfigStatus::Approved.is_disapproved());
    assert!(OrchestrationConfigStatus::Disapproved.is_disapproved());
    assert!(!OrchestrationConfigStatus::None.is_approved());
}

#[test]
fn proto_round_trip_config_local() {
    let config = make_config("auto", "oz");
    let proto = config.to_proto();
    let round_tripped = OrchestrationConfig::from_proto(&proto);
    assert_eq!(config, round_tripped);
}

/// Configs saved while the cloud execution mode existed still load; they
/// become local configs, and save back as local.
#[test]
fn cloud_config_from_proto_loads_as_local() {
    let proto = api::OrchestrationConfig {
        model_id: "auto".to_string(),
        harness: harness_type_to_proto("claude"),
        execution_mode: Some(api::orchestration_config::ExecutionMode::Remote(
            api::orchestration_config::Remote {
                environment_id: "env-1".to_string(),
                worker_host: "warp".to_string(),
            },
        )),
    };
    let config = OrchestrationConfig::from_proto(&proto);
    assert_eq!(config, make_config("auto", "claude"));
    assert_eq!(
        config.to_proto().execution_mode,
        Some(api::orchestration_config::ExecutionMode::Local(
            api::orchestration_config::Local {}
        ))
    );
}

#[test]
fn proto_round_trip_status() {
    for status in [
        OrchestrationConfigStatus::None,
        OrchestrationConfigStatus::Approved,
        OrchestrationConfigStatus::Disapproved,
    ] {
        let proto = status.to_proto();
        let round_tripped = OrchestrationConfigStatus::from_proto(proto.as_ref());
        assert_eq!(status, round_tripped);
    }
}
