//! Ambient agent task types and utilities.

use anyhow::anyhow;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use serde::{Deserialize, Serialize};
use uncaged_core::report_error;

pub use super::agent_config::AgentConfigSnapshot;
use super::AmbientAgentTaskId;
use crate::ai::artifacts::{deserialize_artifacts, Artifact};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentSource {
    Linear,
    AgentWebhook,
    Slack,
    Cli,
    ScheduledAgent,
    Interactive,
    WebApp,
    GitHubAction,
    CloudMode,
}

impl AgentSource {
    pub fn as_str(&self) -> &str {
        match self {
            AgentSource::Linear => "LINEAR",
            AgentSource::AgentWebhook => "API",
            AgentSource::Slack => "SLACK",
            AgentSource::Cli => "CLI",
            AgentSource::ScheduledAgent => "SCHEDULED_AGENT",
            // The public API's run source for local interactive tasks is named
            // `LOCAL`.
            AgentSource::Interactive => "LOCAL",
            AgentSource::WebApp => "WEB_APP",
            AgentSource::GitHubAction => "GITHUB_ACTION",
            AgentSource::CloudMode => "CLOUD_MODE",
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            AgentSource::Linear => "Linear",
            AgentSource::AgentWebhook => "API",
            AgentSource::Slack => "Slack",
            AgentSource::Cli => "CLI",
            AgentSource::ScheduledAgent => "Scheduled",
            AgentSource::Interactive | AgentSource::CloudMode => "Warp App",
            AgentSource::WebApp => "Oz Web",
            AgentSource::GitHubAction => "GitHub Action",
        }
    }

    /// Returns true if this source represents a user-initiated conversation
    /// (as opposed to automated/programmatic sources like CLI or scheduled runs).
    pub fn is_user_initiated(&self) -> bool {
        match self {
            AgentSource::Linear
            | AgentSource::Slack
            | AgentSource::Interactive
            | AgentSource::WebApp
            | AgentSource::CloudMode => true,
            AgentSource::Cli
            | AgentSource::ScheduledAgent
            | AgentSource::AgentWebhook
            | AgentSource::GitHubAction => false,
        }
    }
}

fn deserialize_ambient_agent_source<'de, D>(
    deserializer: D,
) -> Result<Option<AgentSource>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: Option<String> = serde::Deserialize::deserialize(deserializer)?;
    Ok(match s {
        Some(s) => match s.as_str() {
            "LINEAR" => Some(AgentSource::Linear),
            "AGENT_WEBHOOK" | "API" => Some(AgentSource::AgentWebhook),
            "SLACK" => Some(AgentSource::Slack),
            "LOCAL" => Some(AgentSource::Interactive),
            "CLI" => Some(AgentSource::Cli),
            "SCHEDULED_AGENT" => Some(AgentSource::ScheduledAgent),
            "WEB_APP" => Some(AgentSource::WebApp),
            "GITHUB_ACTION" => Some(AgentSource::GitHubAction),
            "CLOUD_MODE" => Some(AgentSource::CloudMode),
            _ => {
                report_error!(anyhow!("Unknown AmbientAgentSource: {}", s));
                None
            }
        },
        None => None,
    })
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct AmbientAgentTask {
    pub task_id: AmbientAgentTaskId,
    #[serde(default)]
    pub parent_run_id: Option<String>,
    pub title: String,
    pub state: AmbientAgentTaskState,
    pub prompt: String,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub run_time: Option<Iso8601Duration>,
    pub status_message: Option<TaskStatusMessage>,
    #[serde(default, deserialize_with = "deserialize_ambient_agent_source")]
    pub source: Option<AgentSource>,
    pub session_id: Option<String>,
    pub session_link: Option<String>,
    pub creator: Option<TaskPrincipalInfo>,
    #[serde(default)]
    pub executor: Option<TaskPrincipalInfo>,
    pub conversation_id: Option<String>,
    pub request_usage: Option<RequestUsage>,
    pub is_sandbox_running: bool,

    /// Snapshot of the agent config used to create the task.
    #[serde(default, alias = "agent_config")]
    pub agent_config_snapshot: Option<AgentConfigSnapshot>,
    #[serde(default, deserialize_with = "deserialize_artifacts")]
    pub artifacts: Vec<Artifact>,

    /// The last event sequence number recorded for this run by the server.
    /// Used by orchestration event delivery to resume from the correct
    /// cursor on restart. Populated by `GET /agent/runs/{run_id}` when the
    /// server supports it; `None` on older servers.
    #[serde(default)]
    pub last_event_sequence: Option<i64>,

    /// The server-recorded `run_id`s of direct children of this run. Used
    /// by orchestration event-delivery restore to discover children whose
    /// records may not exist locally (e.g. remote-worker children in the
    /// driver case). Empty on older servers.
    #[serde(default)]
    pub children: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RunExecution<'a> {
    pub session_id: Option<&'a str>,
    pub session_link: Option<&'a str>,
    pub request_usage: Option<&'a RequestUsage>,
    pub is_sandbox_running: bool,
}

impl RunExecution<'_> {
    pub fn has_joinable_session(&self) -> bool {
        self.session_id.is_some() || self.session_link.is_some()
    }

    pub fn is_active(&self) -> bool {
        self.is_sandbox_running && self.has_joinable_session()
    }
}

impl AmbientAgentTask {
    pub fn run_id(&self) -> AmbientAgentTaskId {
        self.task_id
    }

    pub fn conversation_id(&self) -> Option<&str> {
        self.conversation_id.as_deref()
    }

    pub fn active_run_execution(&self) -> RunExecution<'_> {
        RunExecution {
            session_id: self.session_id.as_deref(),
            session_link: self.session_link.as_deref().filter(|link| !link.is_empty()),
            request_usage: self.request_usage.as_ref(),
            is_sandbox_running: self.is_sandbox_running,
        }
    }

    pub fn has_active_execution(&self) -> bool {
        self.state == AmbientAgentTaskState::InProgress && self.active_run_execution().is_active()
    }

    /// Total credits used (inference + compute + platform).
    pub fn credits_used(&self) -> Option<f32> {
        self.active_run_execution().request_usage.map(|u| {
            (u.inference_cost.unwrap_or(0.0)
                + u.compute_cost.unwrap_or(0.0)
                + u.platform_cost.unwrap_or(0.0)) as f32
        })
    }

    /// Server-reported run duration.
    pub fn run_time(&self) -> Option<ChronoDuration> {
        self.run_time.and_then(|run_time| run_time.to_chrono())
    }

    /// Creator's display name, if available.
    pub fn creator_display_name(&self) -> Option<String> {
        self.creator.as_ref().and_then(|c| c.display_name.clone())
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "UPPERCASE")]
pub enum AmbientAgentTaskState {
    Queued,
    Pending,
    Claimed,
    #[serde(alias = "IN_PROGRESS")]
    InProgress,
    Succeeded,
    Failed,
    Error,
    Blocked,
    Cancelled,
    #[serde(other)]
    Unknown,
}

impl AmbientAgentTaskState {
    pub fn is_failure_like(&self) -> bool {
        match self {
            AmbientAgentTaskState::Failed
            | AmbientAgentTaskState::Error
            | AmbientAgentTaskState::Blocked
            | AmbientAgentTaskState::Unknown => true,
            AmbientAgentTaskState::Queued
            | AmbientAgentTaskState::Pending
            | AmbientAgentTaskState::Claimed
            | AmbientAgentTaskState::InProgress
            | AmbientAgentTaskState::Succeeded
            | AmbientAgentTaskState::Cancelled => false,
        }
    }
}

impl std::fmt::Display for AmbientAgentTaskState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AmbientAgentTaskState::Queued => write!(f, "Queued"),
            AmbientAgentTaskState::Pending => write!(f, "Pending"),
            AmbientAgentTaskState::Claimed => write!(f, "Claimed"),
            AmbientAgentTaskState::InProgress => write!(f, "In progress"),
            AmbientAgentTaskState::Succeeded => write!(f, "Done"),
            AmbientAgentTaskState::Failed => write!(f, "Failed"),
            AmbientAgentTaskState::Error => write!(f, "Error"),
            AmbientAgentTaskState::Blocked => write!(f, "Blocked"),
            AmbientAgentTaskState::Cancelled => write!(f, "Cancelled"),
            AmbientAgentTaskState::Unknown => write!(f, "Failed"),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct TaskPrincipalInfo {
    #[serde(rename = "type")]
    pub creator_type: String,
    pub uid: String,
    pub display_name: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct TaskStatusMessage {
    pub message: String,
    #[serde(default, alias = "errorCode")]
    pub error_code: Option<TaskStatusErrorCode>,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatusErrorCode {
    #[serde(alias = "ENVIRONMENT_SETUP_FAILED")]
    EnvironmentSetupFailed,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct RequestUsage {
    pub inference_cost: Option<f64>,
    pub compute_cost: Option<f64>,
    pub platform_cost: Option<f64>,
}

#[cfg(test)]
#[path = "task_tests.rs"]
mod tests;
