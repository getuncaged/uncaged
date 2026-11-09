use chrono::{Duration, Utc};
use serde_json::{json, Value};

use super::{AmbientAgentTask, TaskStatusErrorCode, TaskStatusMessage};

fn task_json_with_run_time(run_time_key: &str, run_time: Value) -> Value {
    let now = Utc::now().to_rfc3339();
    let mut task = json!({
        "task_id": "11111111-1111-1111-1111-111111111111",
        "title": "Task",
        "state": "SUCCEEDED",
        "prompt": "test",
        "created_at": now,
        "started_at": now,
        "updated_at": now,
        "status_message": null,
        "session_id": null,
        "session_link": null,
        "creator": null,
        "conversation_id": null,
        "request_usage": null,
        "is_sandbox_running": false
    });
    task[run_time_key] = run_time;
    task
}

#[test]
fn task_status_error_code_deserializes_public_api_casing() {
    let message: TaskStatusMessage = serde_json::from_str(
        "{\"message\":\"setup failed\",\"error_code\":\"environment_setup_failed\"}",
    )
    .unwrap();

    assert_eq!(
        message.error_code,
        Some(TaskStatusErrorCode::EnvironmentSetupFailed)
    );
}

#[test]
fn task_status_error_code_deserializes_graphql_casing() {
    let message: TaskStatusMessage = serde_json::from_str(
        "{\"message\":\"setup failed\",\"errorCode\":\"ENVIRONMENT_SETUP_FAILED\"}",
    )
    .unwrap();

    assert_eq!(
        message.error_code,
        Some(TaskStatusErrorCode::EnvironmentSetupFailed)
    );
}

#[test]
fn task_status_error_code_deserializes_unknown_codes() {
    let message: TaskStatusMessage =
        serde_json::from_str("{\"message\":\"failed\",\"error_code\":\"new_error\"}").unwrap();

    assert_eq!(message.error_code, Some(TaskStatusErrorCode::Unknown));
}

#[test]
fn ambient_agent_task_deserializes_run_time_iso8601() {
    let task: AmbientAgentTask =
        serde_json::from_value(task_json_with_run_time("run_time", json!("PT2M30S"))).unwrap();

    assert_eq!(task.run_time(), Some(Duration::seconds(150)));
}
