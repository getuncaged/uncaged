use std::time::Duration;

use anyhow::{anyhow, Context as _};
use async_trait::async_trait;
use base64::Engine;
use chrono::{DateTime, Utc};
use cynic::{MutationBuilder, QueryBuilder};
#[cfg(test)]
use mockall::automock;
use prost::Message;
use warp_core::channel::ChannelState;
use warp_core::features::FeatureFlag;
use warp_core::report_error;
use warp_graphql::ai::{AgentTaskState, PlatformErrorCode};
use warp_graphql::client::Operation;
use warp_graphql::mutations::create_agent_task::{
    CreateAgentTask, CreateAgentTaskInput, CreateAgentTaskResult, CreateAgentTaskVariables,
};
use warp_graphql::mutations::delete_ai_conversation::{
    DeleteAIConversation, DeleteAIConversationVariables, DeleteConversationInput,
    DeleteConversationResult,
};
use warp_graphql::mutations::generate_metadata_for_command::{
    GenerateMetadataForCommand, GenerateMetadataForCommandInput, GenerateMetadataForCommandResult,
    GenerateMetadataForCommandStatus, GenerateMetadataForCommandVariables,
};
use warp_graphql::mutations::request_bonus::{
    ProvideNegativeFeedbackResponseForAiConversation,
    ProvideNegativeFeedbackResponseForAiConversationInput,
    ProvideNegativeFeedbackResponseForAiConversationVariables, RequestsRefundedResult,
};
use warp_graphql::mutations::update_agent_task::{
    AgentTaskStatusMessageInput, UpdateAgentTask, UpdateAgentTaskInput, UpdateAgentTaskResult,
    UpdateAgentTaskVariables,
};
use warp_graphql::queries::free_available_models::{
    FreeAvailableModels, FreeAvailableModelsInput, FreeAvailableModelsResult,
    FreeAvailableModelsVariables,
};
use warp_graphql::queries::get_available_harnesses::{
    GetAvailableHarnesses, GetAvailableHarnessesVariables,
};
use warp_graphql::queries::get_feature_model_choices::{
    GetFeatureModelChoices, GetFeatureModelChoicesVariables,
};
#[cfg(not(feature = "agent_mode_evals"))]
use warp_graphql::queries::get_request_limit_info::{
    GetRequestLimitInfo, GetRequestLimitInfoVariables,
};
use warp_multi_agent_api::ConversationData;

use super::ServerApi;
use crate::ai::agent::api::ServerConversationToken;
use crate::ai::agent::conversation::{AIAgentHarness, ServerAIConversationMetadata};
use crate::ai::ambient_agents::AmbientAgentTaskId;
// Re-export ambient agent types for backwards compatibility
pub use crate::ai::ambient_agents::{
    AgentConfigSnapshot, AgentSource, AmbientAgentTask, AmbientAgentTaskState,
};
use crate::ai::artifacts::Artifact;
use crate::ai::generate_code_review_content::api::{
    GenerateCodeReviewContentRequest, GenerateCodeReviewContentResponse,
};
use crate::ai::harness_availability::HarnessAvailability;
use crate::ai::llms::{
    AvailableLLMs, DisableReason, LLMContextWindow, LLMInfo, LLMModelHost, LLMProvider, LLMSpec,
    LLMUsageMetadata, ModelsByFeature, RoutingHostConfig,
};
#[cfg(feature = "agent_mode_evals")]
use crate::ai::request_usage_model::RequestLimitInfo;
#[cfg(not(feature = "agent_mode_evals"))]
use crate::ai::BonusGrant;
use crate::ai::RequestUsageInfo;
use crate::drive::workflows::ai_assist::{GeneratedCommandMetadata, GeneratedCommandMetadataError};
use crate::persistence::model::ConversationUsageMetadata;
use crate::server::graphql::{get_request_context, get_user_facing_error_message};
use crate::terminal::model::block::SerializedBlock;
#[cfg(not(feature = "agent_mode_evals"))]
use crate::{
    ai::request_usage_model::BonusGrantScope, server::ids::ServerId,
    workspaces::workspace::WorkspaceUid,
};

const AI_ASSISTANT_REQUEST_TIMEOUT_SECONDS: u64 = 30;

/// The workspace uid the server returns for users without a real workspace.
#[cfg(not(feature = "agent_mode_evals"))]
const PLACEHOLDER_WORKSPACE_UID: &str = "NOT_A_REAL_WORKSPACE_UID";

/// A status update for a task, optionally including a platform error code.
pub struct TaskStatusUpdate {
    pub message: String,
    pub error_code: Option<PlatformErrorCode>,
}
impl TaskStatusUpdate {
    /// Create a status update with just a message (no error code).
    pub fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            error_code: None,
        }
    }

    /// Create a status update with a message and error code.
    pub fn with_error_code(message: impl Into<String>, error_code: PlatformErrorCode) -> Self {
        Self {
            message: message.into(),
            error_code: Some(error_code),
        }
    }
}

/// Request body for `POST /agent/conversations/{conversation_id}/fork`.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct ForkConversationRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
}

/// Response body for `POST /agent/conversations/{conversation_id}/fork`. The returned id is sent
/// on the subsequent `POST /agent/runs` request under `conversation_id` (resume semantics).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ForkConversationResponse {
    pub forked_conversation_id: String,
}

/// Request body for `POST /agent/conversations/{conversation_id}/rename`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RenameConversationRequest {
    pub title: String,
}

/// Response body for `POST /agent/conversations/{conversation_id}/rename`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RenameConversationResponse {
    pub title: String,
}

// --- Orchestrations V2 messaging types ---

#[derive(Debug, Clone, serde::Serialize)]
pub struct SendAgentMessageRequest {
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
    pub sender_run_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SendAgentMessageResponse {
    pub message_ids: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentRunEvent {
    pub event_type: String,
    pub run_id: String,
    pub ref_id: Option<String>,
    pub execution_id: Option<String>,
    pub occurred_at: String,
    pub sequence: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReadAgentMessageResponse {
    pub message_id: String,
    pub sender_run_id: String,
    pub subject: String,
    pub body: String,
    pub sent_at: String,
    pub delivered_at: Option<String>,
    pub read_at: Option<String>,
}

/// Response from the artifact endpoint.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(tag = "artifact_type")]
pub enum ArtifactDownloadResponse {
    #[serde(rename = "SCREENSHOT")]
    Screenshot {
        #[serde(flatten)]
        common: ArtifactDownloadCommonFields,
        data: ScreenshotArtifactResponseData,
    },
    #[serde(rename = "FILE")]
    File {
        #[serde(flatten)]
        common: ArtifactDownloadCommonFields,
        data: FileArtifactResponseData,
    },
}

impl ArtifactDownloadResponse {
    fn common(&self) -> &ArtifactDownloadCommonFields {
        match self {
            ArtifactDownloadResponse::Screenshot { common, .. }
            | ArtifactDownloadResponse::File { common, .. } => common,
        }
    }

    pub fn artifact_uid(&self) -> &str {
        &self.common().artifact_uid
    }

    pub fn artifact_type(&self) -> &'static str {
        match self {
            ArtifactDownloadResponse::Screenshot { .. } => "SCREENSHOT",
            ArtifactDownloadResponse::File { .. } => "FILE",
        }
    }

    pub fn created_at(&self) -> DateTime<Utc> {
        self.common().created_at
    }

    pub fn download_url(&self) -> &str {
        match self {
            ArtifactDownloadResponse::Screenshot { data, .. } => &data.download_url,
            ArtifactDownloadResponse::File { data, .. } => &data.download_url,
        }
    }

    pub fn expires_at(&self) -> DateTime<Utc> {
        match self {
            ArtifactDownloadResponse::Screenshot { data, .. } => data.expires_at,
            ArtifactDownloadResponse::File { data, .. } => data.expires_at,
        }
    }

    pub fn content_type(&self) -> &str {
        match self {
            ArtifactDownloadResponse::Screenshot { data, .. } => &data.content_type,
            ArtifactDownloadResponse::File { data, .. } => &data.content_type,
        }
    }

    pub fn filepath(&self) -> Option<&str> {
        match self {
            ArtifactDownloadResponse::Screenshot { .. } => None,
            ArtifactDownloadResponse::File { data, .. } => Some(&data.filepath),
        }
    }

    pub fn filename(&self) -> Option<&str> {
        match self {
            ArtifactDownloadResponse::Screenshot { .. } => None,
            ArtifactDownloadResponse::File { data, .. } => Some(&data.filename),
        }
    }

    pub fn description(&self) -> Option<&str> {
        match self {
            ArtifactDownloadResponse::Screenshot { data, .. } => data.description.as_deref(),
            ArtifactDownloadResponse::File { data, .. } => data.description.as_deref(),
        }
    }

    pub fn size_bytes(&self) -> Option<i64> {
        match self {
            ArtifactDownloadResponse::Screenshot { .. } => None,
            ArtifactDownloadResponse::File { data, .. } => data.size_bytes,
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ArtifactDownloadCommonFields {
    pub artifact_uid: String,
    pub created_at: DateTime<Utc>,
}

/// Screenshot-specific data from the artifact endpoint.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ScreenshotArtifactResponseData {
    pub download_url: String,
    pub expires_at: DateTime<Utc>,
    pub content_type: String,
    pub description: Option<String>,
}

/// File-specific data from the artifact endpoint.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct FileArtifactResponseData {
    pub download_url: String,
    pub expires_at: DateTime<Utc>,
    pub content_type: String,
    pub filepath: String,
    pub filename: String,
    pub description: Option<String>,
    pub size_bytes: Option<i64>,
}

/// Filter parameters for listing ambient agent tasks.
#[derive(Clone, Debug, Default)]
pub struct TaskListFilter {
    pub creator_uid: Option<String>,
    pub updated_after: Option<DateTime<Utc>>,
    pub created_after: Option<DateTime<Utc>>,
    pub created_before: Option<DateTime<Utc>>,
    pub states: Option<Vec<AmbientAgentTaskState>>,
    pub source: Option<AgentSource>,
    pub execution_location: Option<ExecutionLocation>,
    pub environment_id: Option<String>,
    pub skill_spec: Option<String>,
    pub schedule_id: Option<String>,
    pub ancestor_run_id: Option<String>,
    pub config_name: Option<String>,
    pub model_id: Option<String>,
    pub artifact_type: Option<ArtifactType>,
    pub search_query: Option<String>,
    pub sort_by: Option<RunSortBy>,
    pub sort_order: Option<RunSortOrder>,
    pub cursor: Option<String>,
}

/// Execution location filter values accepted by the public API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionLocation {
    Local,
    Remote,
}

impl ExecutionLocation {
    pub fn as_query_param(&self) -> &'static str {
        match self {
            ExecutionLocation::Local => "LOCAL",
            ExecutionLocation::Remote => "REMOTE",
        }
    }
}

/// Artifact type filter values accepted by the public API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtifactType {
    Plan,
    PullRequest,
    Screenshot,
    File,
}

impl ArtifactType {
    pub fn as_query_param(&self) -> &'static str {
        match self {
            ArtifactType::Plan => "PLAN",
            ArtifactType::PullRequest => "PULL_REQUEST",
            ArtifactType::Screenshot => "SCREENSHOT",
            ArtifactType::File => "FILE",
        }
    }
}

/// Sort-by values accepted by the public API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunSortBy {
    UpdatedAt,
    CreatedAt,
    Title,
    Agent,
}

impl RunSortBy {
    pub fn as_query_param(&self) -> &'static str {
        match self {
            RunSortBy::UpdatedAt => "updated_at",
            RunSortBy::CreatedAt => "created_at",
            RunSortBy::Title => "title",
            RunSortBy::Agent => "agent",
        }
    }
}

/// Sort-order values accepted by the public API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunSortOrder {
    Asc,
    Desc,
}

impl RunSortOrder {
    pub fn as_query_param(&self) -> &'static str {
        match self {
            RunSortOrder::Asc => "asc",
            RunSortOrder::Desc => "desc",
        }
    }
}

/// Build the path + query string for `GET /api/v1/agent/runs` from a filter.
pub(crate) fn build_list_agent_runs_url(limit: i32, filter: &TaskListFilter) -> String {
    let mut url = format!("agent/runs?limit={limit}");

    let mut push = |key: &str, value: &str| {
        url.push('&');
        url.push_str(key);
        url.push('=');
        url.push_str(urlencoding::encode(value).as_ref());
    };

    if let Some(creator_uid) = filter.creator_uid.as_deref() {
        push("creator", creator_uid);
    }
    if let Some(updated_after) = filter.updated_after {
        push("updated_after", &updated_after.to_rfc3339());
    }
    if let Some(created_after) = filter.created_after {
        push("created_after", &created_after.to_rfc3339());
    }
    if let Some(created_before) = filter.created_before {
        push("created_before", &created_before.to_rfc3339());
    }
    if let Some(states) = filter.states.as_ref() {
        for state in states {
            if let Some(value) = state.as_query_param() {
                push("state", value);
            }
        }
    }
    if let Some(source) = filter.source.as_ref() {
        push("source", source.as_str());
    }
    if let Some(execution_location) = filter.execution_location {
        push("execution_location", execution_location.as_query_param());
    }
    if let Some(environment_id) = filter.environment_id.as_deref() {
        push("environment_id", environment_id);
    }
    if let Some(skill_spec) = filter.skill_spec.as_deref() {
        push("skill_spec", skill_spec);
    }
    if let Some(schedule_id) = filter.schedule_id.as_deref() {
        push("schedule_id", schedule_id);
    }
    if let Some(ancestor_run_id) = filter.ancestor_run_id.as_deref() {
        push("ancestor_run_id", ancestor_run_id);
    }
    if let Some(config_name) = filter.config_name.as_deref() {
        push("name", config_name);
    }
    if let Some(model_id) = filter.model_id.as_deref() {
        push("model_id", model_id);
    }
    if let Some(artifact_type) = filter.artifact_type {
        push("artifact_type", artifact_type.as_query_param());
    }
    if let Some(search_query) = filter.search_query.as_deref() {
        push("q", search_query);
    }
    if let Some(sort_by) = filter.sort_by {
        push("sort_by", sort_by.as_query_param());
    }
    if let Some(sort_order) = filter.sort_order {
        push("sort_order", sort_order.as_query_param());
    }
    if let Some(cursor) = filter.cursor.as_deref() {
        push("cursor", cursor);
    }

    url
}

pub(crate) fn build_fork_conversation_url(conversation_id: &str) -> String {
    format!(
        "agent/conversations/{}/fork",
        urlencoding::encode(conversation_id)
    )
}

pub(crate) fn build_rename_conversation_url(conversation_id: &str) -> String {
    format!(
        "agent/conversations/{}/rename",
        urlencoding::encode(conversation_id)
    )
}

struct ListRunsResponse {
    runs: Vec<AmbientAgentTask>,
}

impl<'de> serde::Deserialize<'de> for ListRunsResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct RawResponse {
            runs: Vec<serde_json::Value>,
        }

        let raw = RawResponse::deserialize(deserializer)?;
        let mut runs = Vec::with_capacity(raw.runs.len());

        for task_value in raw.runs.into_iter() {
            match serde_json::from_value::<AmbientAgentTask>(task_value) {
                Ok(task) => runs.push(task),
                Err(e) => {
                    // Log the error and skip this task instead of failing the entire request
                    report_error!(anyhow!("Failed to deserialize ambient agent task: {}", e));
                }
            }
        }

        Ok(ListRunsResponse { runs })
    }
}

#[cfg_attr(test, automock)]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
pub trait AIClient: 'static + Send + Sync {
    async fn generate_metadata_for_command(
        &self,
        command: String,
    ) -> Result<GeneratedCommandMetadata, GeneratedCommandMetadataError>;

    async fn get_request_limit_info(&self) -> Result<RequestUsageInfo, anyhow::Error>;

    async fn get_feature_model_choices(&self) -> Result<ModelsByFeature, anyhow::Error>;

    async fn get_available_harnesses(&self) -> Result<Vec<HarnessAvailability>, anyhow::Error>;

    /// Fetches the free-tier available models without requiring authentication.
    /// Used during pre-login onboarding so logged-out users see an accurate model list
    /// instead of the hard-coded `ModelsByFeature::default()` fallback.
    async fn get_free_available_models(
        &self,
        referrer: Option<String>,
    ) -> Result<ModelsByFeature, anyhow::Error>;

    async fn provide_negative_feedback_response_for_ai_conversation(
        &self,
        conversation_id: String,
        request_ids: Vec<String>,
    ) -> anyhow::Result<i32, anyhow::Error>;

    async fn create_agent_task(
        &self,
        prompt: String,
        environment_uid: Option<String>,
        parent_run_id: Option<String>,
        config: Option<AgentConfigSnapshot>,
    ) -> anyhow::Result<AmbientAgentTaskId, anyhow::Error>;

    async fn update_agent_task(
        &self,
        task_id: AmbientAgentTaskId,
        task_state: Option<AgentTaskState>,
        session_id: Option<session_sharing_protocol::common::SessionId>,
        conversation_id: Option<String>,
        status_message: Option<TaskStatusUpdate>,
    ) -> anyhow::Result<(), anyhow::Error>;

    /// Materialize a server-side fork of a conversation.
    async fn fork_conversation(
        &self,
        conversation_id: String,
        title: Option<String>,
    ) -> anyhow::Result<ForkConversationResponse, anyhow::Error>;

    /// Rename a server-side conversation and return the normalized title.
    async fn rename_conversation(
        &self,
        conversation_id: String,
        title: String,
    ) -> anyhow::Result<RenameConversationResponse, anyhow::Error>;

    async fn list_ambient_agent_tasks(
        &self,
        limit: i32,
        filter: TaskListFilter,
    ) -> anyhow::Result<Vec<AmbientAgentTask>, anyhow::Error>;

    async fn get_ambient_agent_task(
        &self,
        task_id: &AmbientAgentTaskId,
    ) -> anyhow::Result<AmbientAgentTask, anyhow::Error>;

    async fn get_ai_conversation(
        &self,
        server_conversation_token: ServerConversationToken,
    ) -> anyhow::Result<(ConversationData, ServerAIConversationMetadata), anyhow::Error>;

    async fn list_ai_conversation_metadata(
        &self,
        conversation_ids: Option<Vec<String>>,
    ) -> anyhow::Result<Vec<ServerAIConversationMetadata>>;

    async fn get_block_snapshot(
        &self,
        server_conversation_token: ServerConversationToken,
    ) -> anyhow::Result<SerializedBlock, anyhow::Error>;

    async fn delete_ai_conversation(
        &self,
        server_conversation_token: String,
    ) -> anyhow::Result<(), anyhow::Error>;

    async fn cancel_ambient_agent_task(
        &self,
        task_id: &AmbientAgentTaskId,
    ) -> anyhow::Result<(), anyhow::Error>;

    async fn get_artifact_download(
        &self,
        artifact_uid: &str,
    ) -> anyhow::Result<ArtifactDownloadResponse, anyhow::Error>;

    // --- Orchestrations V2 messaging ---

    async fn send_agent_message(
        &self,
        request: SendAgentMessageRequest,
    ) -> anyhow::Result<SendAgentMessageResponse, anyhow::Error>;

    /// Persists the latest observed event sequence number for a run on the
    /// server. Used to keep the server-side cursor in sync with the client so
    /// that driver/cloud restores can resume without replaying events the
    /// parent has already acted on.
    async fn update_event_sequence_on_server(
        &self,
        run_id: &str,
        sequence: i64,
    ) -> anyhow::Result<(), anyhow::Error>;

    async fn mark_message_delivered(&self, message_id: &str) -> anyhow::Result<(), anyhow::Error>;

    async fn read_agent_message(
        &self,
        message_id: &str,
    ) -> anyhow::Result<ReadAgentMessageResponse, anyhow::Error>;

    /// Generates AI copy for code-review flows: commit messages at dialog-open
    /// time and PR titles / bodies at confirm time. `output_type` in the
    /// request picks which of the three the server returns.
    async fn generate_code_review_content(
        &self,
        request: GenerateCodeReviewContentRequest,
    ) -> Result<GenerateCodeReviewContentResponse, anyhow::Error>;
}

impl ServerApi {
    pub(crate) async fn post_public_api_response_for_task<B>(
        &self,
        task_id: &AmbientAgentTaskId,
        path: &str,
        body: &B,
    ) -> anyhow::Result<http_client::Response>
    where
        B: serde::Serialize,
    {
        let auth_token = self
            .get_or_refresh_access_token()
            .await
            .context("Failed to get access token for API request")?;

        let url = format!("{}/api/v1/{}", crate::ChannelState::server_root_url(), path);

        let mut request = self.base_client.http_client().post(&url).json(body);
        if let Some(token) = auth_token.as_bearer_token() {
            request = request.bearer_auth(token);
        }

        for (name, value) in self.ambient_agent_headers_for_task(task_id).await? {
            request = request.header(name, value);
        }

        let response = request
            .send()
            .await
            .with_context(|| format!("Failed to send API request to {url}"))?;

        if response.status().is_success() {
            Ok(response)
        } else {
            Err(Self::error_from_response(response).await)
        }
    }

    pub(crate) async fn send_agent_message_for_task(
        &self,
        task_id: &AmbientAgentTaskId,
        request: SendAgentMessageRequest,
    ) -> anyhow::Result<SendAgentMessageResponse, anyhow::Error> {
        let response = self
            .post_public_api_response_for_task(task_id, "agent/messages", &request)
            .await?;
        let response = response.json::<SendAgentMessageResponse>().await?;
        Ok(response)
    }

    pub(crate) async fn mark_message_delivered_for_task(
        &self,
        task_id: &AmbientAgentTaskId,
        message_id: &str,
    ) -> anyhow::Result<(), anyhow::Error> {
        self.post_public_api_response_for_task(
            task_id,
            &format!("agent/messages/{message_id}/delivered"),
            &(),
        )
        .await?;
        Ok(())
    }

    pub(crate) async fn read_agent_message_for_task(
        &self,
        task_id: &AmbientAgentTaskId,
        message_id: &str,
    ) -> anyhow::Result<ReadAgentMessageResponse, anyhow::Error> {
        let response = self
            .post_public_api_response_for_task(
                task_id,
                &format!("agent/messages/{message_id}/read"),
                &(),
            )
            .await?;
        let response = response.json::<ReadAgentMessageResponse>().await?;
        Ok(response)
    }
}

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl AIClient for ServerApi {
    async fn generate_metadata_for_command(
        &self,
        command: String,
    ) -> Result<GeneratedCommandMetadata, GeneratedCommandMetadataError> {
        let default_err = GeneratedCommandMetadataError::Other;
        let variables = GenerateMetadataForCommandVariables {
            input: GenerateMetadataForCommandInput { command },
            request_context: get_request_context(),
        };

        let operation = GenerateMetadataForCommand::build(variables);
        let response = self
            .send_graphql_request(
                operation,
                Some(Duration::from_secs(AI_ASSISTANT_REQUEST_TIMEOUT_SECONDS)),
            )
            .await
            .map_err(|_| default_err)?;

        match response.generate_metadata_for_command {
            GenerateMetadataForCommandResult::GenerateMetadataForCommandOutput(output) => {
                match output.status {
                    GenerateMetadataForCommandStatus::GenerateMetadataForCommandSuccess(
                        success,
                    ) => Ok(success.into()),
                    GenerateMetadataForCommandStatus::GenerateMetadataForCommandFailure(
                        failure,
                    ) => Err(failure.type_.into()),
                    GenerateMetadataForCommandStatus::Unknown => {
                        Err(GeneratedCommandMetadataError::Other)
                    }
                }
            }
            _ => Err(GeneratedCommandMetadataError::Other),
        }
    }

    #[cfg(feature = "agent_mode_evals")]
    async fn get_request_limit_info(&self) -> Result<RequestUsageInfo, anyhow::Error> {
        Ok(RequestUsageInfo {
            request_limit_info: RequestLimitInfo::new_for_evals(),
            bonus_grants: vec![],
        })
    }

    #[cfg(not(feature = "agent_mode_evals"))]
    async fn get_request_limit_info(&self) -> Result<RequestUsageInfo, anyhow::Error> {
        let variables = GetRequestLimitInfoVariables {
            request_context: get_request_context(),
        };
        let operation = GetRequestLimitInfo::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.user {
            warp_graphql::queries::get_request_limit_info::UserResult::UserOutput(user_output) => {
                let request_limit_info = user_output.user.request_limit_info.into();

                let workspace_bonus_grants = user_output
                    .user
                    .workspaces
                    .into_iter()
                    .filter(|workspace| workspace.uid != PLACEHOLDER_WORKSPACE_UID.into())
                    .flat_map(|workspace| {
                        let workspace_uid =
                            WorkspaceUid::from(ServerId::from_string_lossy(workspace.uid.inner()));
                        workspace
                            .bonus_grants_info
                            .grants
                            .into_iter()
                            .map(move |grant| {
                                BonusGrant::from_gql_bonus_grant(
                                    grant,
                                    BonusGrantScope::Workspace(workspace_uid),
                                )
                            })
                    });

                let bonus_grants: Vec<BonusGrant> = user_output
                    .user
                    .bonus_grants
                    .into_iter()
                    .map(|grant| BonusGrant::from_gql_bonus_grant(grant, BonusGrantScope::User))
                    .chain(workspace_bonus_grants)
                    .collect();

                Ok(RequestUsageInfo {
                    request_limit_info,
                    bonus_grants,
                })
            }
            warp_graphql::queries::get_request_limit_info::UserResult::UserFacingError(e) => {
                Err(anyhow!(get_user_facing_error_message(e)))
            }
            warp_graphql::queries::get_request_limit_info::UserResult::Unknown => {
                Err(anyhow!("failed to get request limit info"))
            }
        }
    }

    async fn get_feature_model_choices(&self) -> Result<ModelsByFeature, anyhow::Error> {
        let variables = GetFeatureModelChoicesVariables {
            request_context: get_request_context(),
        };
        let operation = GetFeatureModelChoices::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.user {
            warp_graphql::queries::get_feature_model_choices::UserResult::UserOutput(
                warp_graphql::queries::get_feature_model_choices::UserOutput {
                    user: warp_graphql::queries::get_feature_model_choices::User { mut workspaces },
                },
            ) if !workspaces.is_empty() => {
                // This is safe (`remove()` can panic) because we ensure workspaces is non-empty
                // above.
                workspaces.remove(0).feature_model_choice.try_into()
            }
            _ => Err(anyhow!("Failed to get available feature model choices")),
        }
    }

    async fn get_available_harnesses(&self) -> Result<Vec<HarnessAvailability>, anyhow::Error> {
        let variables = GetAvailableHarnessesVariables {
            request_context: get_request_context(),
        };
        let operation = GetAvailableHarnesses::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.user {
            warp_graphql::queries::get_available_harnesses::UserResult::UserOutput(output) => {
                Ok(output
                    .user
                    .available_harnesses
                    .harnesses
                    .into_iter()
                    .map(|h| HarnessAvailability {
                        harness: convert_harness(h.harness).into(),
                        display_name: h.display_name,
                        enabled: h.enabled,
                        available_models: h
                            .available_models
                            .into_iter()
                            .map(|m| crate::ai::harness_availability::HarnessModelInfo {
                                id: m.id.into_inner(),
                                display_name: m.display_name,
                                reasoning_level: m.reasoning_level,
                            })
                            .collect(),
                    })
                    .collect())
            }
            warp_graphql::queries::get_available_harnesses::UserResult::Unknown => {
                Err(anyhow!("Failed to get available harnesses"))
            }
        }
    }

    async fn get_free_available_models(
        &self,
        referrer: Option<String>,
    ) -> Result<ModelsByFeature, anyhow::Error> {
        // This resolver is public; it does not require an auth token. We must NOT go through
        // `send_graphql_request`, which awaits `get_or_refresh_access_token()`
        let variables = FreeAvailableModelsVariables {
            input: FreeAvailableModelsInput { referrer },
            request_context: get_request_context(),
        };
        let operation = FreeAvailableModels::build(variables);

        // Best-effort: if the user has a valid token (e.g. anonymous Firebase), include it;
        // otherwise send unauthenticated. Either is acceptable for this resolver.
        let auth_token = self
            .get_or_refresh_access_token()
            .await
            .ok()
            .and_then(|token| token.bearer_token());

        let response = operation
            .send_request(
                self.base_client.owned_http_client(),
                self.base_client
                    .graphql_request_options_with_token(auth_token),
            )
            .await?
            .data
            .ok_or_else(|| anyhow!("Missing data in freeAvailableModels response"))?;

        match response.free_available_models {
            FreeAvailableModelsResult::FreeAvailableModelsOutput(output) => {
                output.feature_model_choice.try_into()
            }
            FreeAvailableModelsResult::Unknown => {
                Err(anyhow!("Unexpected freeAvailableModels response variant"))
            }
        }
    }

    async fn provide_negative_feedback_response_for_ai_conversation(
        &self,
        conversation_id: String,
        request_ids: Vec<String>,
    ) -> anyhow::Result<i32, anyhow::Error> {
        let variables = ProvideNegativeFeedbackResponseForAiConversationVariables {
            input: ProvideNegativeFeedbackResponseForAiConversationInput {
                conversation_id: conversation_id.into(),
                request_ids: request_ids.into_iter().map(Into::into).collect(),
            },
            request_context: get_request_context(),
        };

        let operation = ProvideNegativeFeedbackResponseForAiConversation::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.provide_negative_feedback_response_for_ai_conversation {
            RequestsRefundedResult::RequestsRefundedOutput(output) => Ok(output.requests_refunded),
            RequestsRefundedResult::UserFacingError(e) => {
                Err(anyhow!(get_user_facing_error_message(e)))
            }
            RequestsRefundedResult::Unknown => Err(anyhow!(
                "failed to provide negative feedback response for ai conversation"
            )),
        }
    }

    #[tracing::instrument(skip_all, err, fields(
        tags.cloud_agent = true,
        config.worker_host = tracing::field::Empty,
        config.harness = tracing::field::Empty
    ))]
    async fn create_agent_task(
        &self,
        prompt: String,
        environment_uid: Option<String>,
        parent_run_id: Option<String>,
        config: Option<AgentConfigSnapshot>,
    ) -> anyhow::Result<AmbientAgentTaskId, anyhow::Error> {
        if let Some(config) = &config {
            if let Some(worker_host) = &config.worker_host {
                tracing::Span::current().record("config.worker_host", worker_host);
            }
            if let Some(harness) = &config.harness {
                let harness: Option<serde_json::Value> =
                    serde_json::to_value(harness.harness_type).ok();
                if let Some(serde_json::Value::String(harness)) = harness {
                    tracing::Span::current().record("config.harness", harness);
                }
            }
        }

        // Serialize the config to JSON if provided
        let agent_config_snapshot = config
            .map(|c| serde_json::to_string(&c))
            .transpose()
            .map_err(|e| anyhow!("Failed to serialize agent config: {e}"))?;

        let variables = CreateAgentTaskVariables {
            input: CreateAgentTaskInput {
                prompt,
                environment_uid: environment_uid.map(|uid| uid.into()),
                parent_run_id: parent_run_id.map(|run_id| run_id.into()),
                agent_config_snapshot,
            },
            request_context: get_request_context(),
        };

        let operation = CreateAgentTask::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.create_agent_task {
            CreateAgentTaskResult::CreateAgentTaskOutput(output) => output
                .task_id
                .into_inner()
                .parse()
                .map_err(|e| anyhow!("Failed to parse task ID from server: {e}")),
            CreateAgentTaskResult::UserFacingError(e) => {
                Err(anyhow!(get_user_facing_error_message(e)))
            }
            CreateAgentTaskResult::Unknown => Err(anyhow!("failed to create agent task")),
        }
    }

    #[tracing::instrument(skip_all, err, fields(
        tags.cloud_agent = true,
        ?task_state,
        ?session_id,
        ?conversation_id,
        error_code = ?status_message.as_ref().map(|m| m.error_code)
    ))]
    async fn update_agent_task(
        &self,
        task_id: AmbientAgentTaskId,
        task_state: Option<AgentTaskState>,
        session_id: Option<session_sharing_protocol::common::SessionId>,
        conversation_id: Option<String>,
        status_message: Option<TaskStatusUpdate>,
    ) -> anyhow::Result<(), anyhow::Error> {
        let variables = UpdateAgentTaskVariables {
            input: UpdateAgentTaskInput {
                task_id: task_id.into(),
                task_state,
                session_id: session_id.map(|id| id.to_string().into()),
                conversation_id: conversation_id.map(|id| id.into()),
                status_message: status_message.map(|update| AgentTaskStatusMessageInput {
                    message: update.message,
                    error_code: update.error_code,
                }),
            },
            request_context: get_request_context(),
        };

        let operation = UpdateAgentTask::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.update_agent_task {
            UpdateAgentTaskResult::UpdateAgentTaskOutput(_) => Ok(()),
            UpdateAgentTaskResult::UserFacingError(e) => {
                Err(anyhow!(get_user_facing_error_message(e)))
            }
            UpdateAgentTaskResult::Unknown => Err(anyhow!("failed to update agent task")),
        }
    }

    async fn fork_conversation(
        &self,
        conversation_id: String,
        title: Option<String>,
    ) -> anyhow::Result<ForkConversationResponse, anyhow::Error> {
        let request = ForkConversationRequest { title };
        let response: ForkConversationResponse = self
            .post_public_api(&build_fork_conversation_url(&conversation_id), &request)
            .await?;
        Ok(response)
    }

    async fn rename_conversation(
        &self,
        conversation_id: String,
        title: String,
    ) -> anyhow::Result<RenameConversationResponse, anyhow::Error> {
        let request = RenameConversationRequest { title };
        let response: RenameConversationResponse = self
            .post_public_api(&build_rename_conversation_url(&conversation_id), &request)
            .await?;
        Ok(response)
    }

    async fn list_ambient_agent_tasks(
        &self,
        limit: i32,
        filter: TaskListFilter,
    ) -> anyhow::Result<Vec<AmbientAgentTask>, anyhow::Error> {
        let url = build_list_agent_runs_url(limit, &filter);
        let response: ListRunsResponse = self.get_public_api(&url).await?;
        Ok(response.runs)
    }

    #[tracing::instrument(skip_all, err, fields(tags.cloud_agent = true))]
    async fn get_ambient_agent_task(
        &self,
        task_id: &AmbientAgentTaskId,
    ) -> anyhow::Result<AmbientAgentTask, anyhow::Error> {
        let response: AmbientAgentTask = self
            .get_public_api(&format!("agent/runs/{task_id}"))
            .await?;
        Ok(response)
    }

    #[tracing::instrument(skip_all, err, fields(tags.cloud_agent = true))]
    async fn get_ai_conversation(
        &self,
        server_conversation_token: ServerConversationToken,
    ) -> anyhow::Result<(ConversationData, ServerAIConversationMetadata), anyhow::Error> {
        use warp_graphql::queries::list_ai_conversations::{
            ListAIConversations, ListAIConversationsInput, ListAIConversationsResult,
            ListAIConversationsVariables,
        };

        let conversation_id = server_conversation_token.as_str().to_string();
        let operation = ListAIConversations::build(ListAIConversationsVariables {
            input: ListAIConversationsInput {
                conversation_ids: Some(vec![cynic::Id::new(conversation_id)]),
            },
            request_context: get_request_context(),
        });
        let response = self.send_graphql_request(operation, None).await?;

        let gql_conversation = match response.list_ai_conversations {
            ListAIConversationsResult::ListAIConversationsOutput(output) => output
                .conversations
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("Conversation not found"))?,
            ListAIConversationsResult::UserFacingError(e) => {
                return Err(anyhow!(get_user_facing_error_message(e)));
            }
            ListAIConversationsResult::Unknown => {
                return Err(anyhow!("Failed to get AI conversation"));
            }
        };

        let conversation_data_bytes = base64::engine::general_purpose::STANDARD
            .decode(&gql_conversation.final_task_list)
            .map_err(|e| anyhow!("Failed to decode base64 conversation data: {e}"))?;

        let conversation_data = ConversationData::decode(conversation_data_bytes.as_slice())
            .map_err(|e| anyhow!("Failed to decode proto ConversationData: {e}"))?;

        // Build AIConversationMetadata from GraphQL response
        let metadata = gql_conversation.try_into()?;

        Ok((conversation_data, metadata))
    }

    async fn list_ai_conversation_metadata(
        &self,
        conversation_ids: Option<Vec<String>>,
    ) -> anyhow::Result<Vec<ServerAIConversationMetadata>> {
        if !FeatureFlag::CloudConversations.is_enabled() {
            return Ok(vec![]);
        }
        use warp_graphql::queries::list_ai_conversations::{
            ListAIConversationMetadata, ListAIConversationMetadataResult,
            ListAIConversationMetadataVariables, ListAIConversationsInput,
        };

        let input = ListAIConversationsInput {
            conversation_ids: conversation_ids
                .map(|ids| ids.into_iter().map(cynic::Id::new).collect()),
        };

        let variables = ListAIConversationMetadataVariables {
            input,
            request_context: get_request_context(),
        };

        let operation = ListAIConversationMetadata::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.list_ai_conversations {
            ListAIConversationMetadataResult::ListAIConversationsOutput(output) => {
                let metadata_vec: Result<Vec<_>, _> = output
                    .conversations
                    .into_iter()
                    .map(|conv| conv.try_into())
                    .collect();
                metadata_vec
            }
            ListAIConversationMetadataResult::UserFacingError(e) => {
                Err(anyhow!(get_user_facing_error_message(e)))
            }
            ListAIConversationMetadataResult::Unknown => {
                Err(anyhow!("Failed to list AI conversations metadata"))
            }
        }
    }

    async fn get_block_snapshot(
        &self,
        server_conversation_token: ServerConversationToken,
    ) -> anyhow::Result<SerializedBlock, anyhow::Error> {
        let conversation_id = server_conversation_token.as_str();
        // Make sure to use `SerializedBlock::from_json` to correctly handle the serialized
        // command and output grid contents.
        let response = self
            .get_public_api_response(&format!(
                "agent/conversations/{conversation_id}/block-snapshot"
            ))
            .await?;
        let json_bytes = response
            .bytes()
            .await
            .map_err(|e| anyhow!("Failed to read block snapshot for {conversation_id}: {e}"))?;
        SerializedBlock::from_json(&json_bytes)
    }

    async fn delete_ai_conversation(
        &self,
        server_conversation_token: String,
    ) -> anyhow::Result<(), anyhow::Error> {
        let variables = DeleteAIConversationVariables {
            input: DeleteConversationInput {
                conversation_id: server_conversation_token.into(),
            },
            request_context: get_request_context(),
        };

        let operation = DeleteAIConversation::build(variables);
        let response = self.send_graphql_request(operation, None).await?;

        match response.delete_conversation {
            DeleteConversationResult::DeleteConversationOutput(_) => Ok(()),
            DeleteConversationResult::UserFacingError(e) => {
                Err(anyhow!(get_user_facing_error_message(e)))
            }
            DeleteConversationResult::Unknown => Err(anyhow!("Failed to delete AI conversation")),
        }
    }

    async fn cancel_ambient_agent_task(
        &self,
        task_id: &AmbientAgentTaskId,
    ) -> anyhow::Result<(), anyhow::Error> {
        let _: String = self
            .post_public_api(&format!("agent/tasks/{task_id}/cancel"), &())
            .await?;
        Ok(())
    }

    async fn get_artifact_download(
        &self,
        artifact_uid: &str,
    ) -> anyhow::Result<ArtifactDownloadResponse, anyhow::Error> {
        let response: ArtifactDownloadResponse = self
            .get_public_api(&format!("agent/artifacts/{artifact_uid}"))
            .await?;
        Ok(response)
    }

    // --- Orchestrations V2 messaging ---

    async fn send_agent_message(
        &self,
        request: SendAgentMessageRequest,
    ) -> anyhow::Result<SendAgentMessageResponse, anyhow::Error> {
        let response: SendAgentMessageResponse =
            self.post_public_api("agent/messages", &request).await?;
        Ok(response)
    }

    async fn update_event_sequence_on_server(
        &self,
        run_id: &str,
        sequence: i64,
    ) -> anyhow::Result<(), anyhow::Error> {
        #[derive(serde::Serialize)]
        struct UpdateBody {
            sequence: i64,
        }

        self.patch_public_api_unit(
            &format!("agent/runs/{run_id}/event-sequence"),
            &UpdateBody { sequence },
        )
        .await
    }

    async fn mark_message_delivered(&self, message_id: &str) -> anyhow::Result<(), anyhow::Error> {
        self.post_public_api_unit(&format!("agent/messages/{message_id}/delivered"), &())
            .await
    }

    async fn read_agent_message(
        &self,
        message_id: &str,
    ) -> anyhow::Result<ReadAgentMessageResponse, anyhow::Error> {
        let response: ReadAgentMessageResponse = self
            .post_public_api(&format!("agent/messages/{message_id}/read"), &())
            .await?;
        Ok(response)
    }

    async fn generate_code_review_content(
        &self,
        request: GenerateCodeReviewContentRequest,
    ) -> Result<GenerateCodeReviewContentResponse, anyhow::Error> {
        let auth_token = self.get_or_refresh_access_token().await?;
        let request_builder = self.base_client.http_client().post(format!(
            "{}/ai/generate_code_review_content",
            ChannelState::server_root_url()
        ));
        let response = if let Some(token) = auth_token.as_bearer_token() {
            request_builder.bearer_auth(token)
        } else {
            request_builder
        }
        .json(&request)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
        Ok(response)
    }
}

impl TryFrom<warp_graphql::queries::get_feature_model_choices::FeatureModelChoice>
    for ModelsByFeature
{
    type Error = anyhow::Error;

    fn try_from(
        value: warp_graphql::queries::get_feature_model_choices::FeatureModelChoice,
    ) -> Result<Self, Self::Error> {
        Ok(Self {
            agent_mode: value.agent_mode.try_into()?,
            coding: value.coding.try_into()?,
            cli_agent: Some(value.cli_agent.try_into()?),
            computer_use: Some(value.computer_use_agent.try_into()?),
        })
    }
}

impl TryFrom<warp_graphql::workspace::FeatureModelChoice> for ModelsByFeature {
    type Error = anyhow::Error;

    fn try_from(value: warp_graphql::workspace::FeatureModelChoice) -> Result<Self, Self::Error> {
        Ok(Self {
            agent_mode: value.agent_mode.try_into()?,
            coding: value.coding.try_into()?,
            cli_agent: Some(value.cli_agent.try_into()?),
            computer_use: Some(value.computer_use_agent.try_into()?),
        })
    }
}

impl TryFrom<warp_graphql::queries::get_feature_model_choices::AvailableLlms> for AvailableLLMs {
    type Error = anyhow::Error;

    fn try_from(
        value: warp_graphql::queries::get_feature_model_choices::AvailableLlms,
    ) -> Result<Self, Self::Error> {
        Self::new(
            value.default_id.into(),
            value.choices.into_iter().map(LLMInfo::from),
            value.preferred_codex_model_id.map(Into::into),
        )
    }
}

impl TryFrom<warp_graphql::workspace::AvailableLlms> for AvailableLLMs {
    type Error = anyhow::Error;

    fn try_from(value: warp_graphql::workspace::AvailableLlms) -> Result<Self, Self::Error> {
        Self::new(
            value.default_id.into(),
            value.choices.into_iter().map(LLMInfo::from),
            value.preferred_codex_model_id.map(Into::into),
        )
    }
}

impl From<warp_graphql::queries::get_feature_model_choices::LlmInfo> for LLMInfo {
    fn from(value: warp_graphql::queries::get_feature_model_choices::LlmInfo) -> Self {
        let host_configs = {
            let mut map = std::collections::HashMap::new();
            for config in value.host_configs {
                let config: RoutingHostConfig = config.into();
                let host = config.model_routing_host.clone();
                if map.insert(host.clone(), config).is_some() {
                    log::warn!(
                        "Duplicate LlmModelHost entry for {:?}, using latest value",
                        host
                    );
                }
            }
            map
        };
        Self {
            id: value.id.into(),
            display_name: value.display_name,
            base_model_name: value.base_model_name,
            reasoning_level: value.reasoning_level,
            usage_metadata: value.usage_metadata.into(),
            description: value.description,
            disable_reason: value.disable_reason.map(DisableReason::from),
            vision_supported: value.vision_supported,
            spec: value.spec.map(Into::into),
            provider: value.provider.into(),
            host_configs,
            discount_percentage: value.pricing.discount_percentage.map(|v| v as f32),
            context_window: LLMContextWindow {
                is_configurable: value.context_window.is_configurable,
                min: value.context_window.min.into(),
                max: value.context_window.max.into(),
                default_max: value.context_window.default.into(),
            },
        }
    }
}

impl From<warp_graphql::workspace::LlmInfo> for LLMInfo {
    fn from(value: warp_graphql::workspace::LlmInfo) -> Self {
        let host_configs = {
            let mut map = std::collections::HashMap::new();
            for config in value.host_configs {
                let config: RoutingHostConfig = config.into();
                let host = config.model_routing_host.clone();
                if map.insert(host.clone(), config).is_some() {
                    log::warn!(
                        "Duplicate LlmModelHost entry for {:?}, using latest value",
                        host
                    );
                }
            }
            map
        };
        Self {
            id: value.id.into(),
            display_name: value.display_name,
            base_model_name: value.base_model_name,
            reasoning_level: value.reasoning_level,
            usage_metadata: value.usage_metadata.into(),
            description: value.description,
            disable_reason: value.disable_reason.map(DisableReason::from),
            vision_supported: value.vision_supported,
            spec: value.spec.map(Into::into),
            provider: value.provider.into(),
            host_configs,
            discount_percentage: value.pricing.discount_percentage.map(|v| v as f32),
            context_window: LLMContextWindow {
                is_configurable: value.context_window.is_configurable,
                min: value.context_window.min.into(),
                max: value.context_window.max.into(),
                default_max: value.context_window.default.into(),
            },
        }
    }
}

impl From<warp_graphql::queries::get_feature_model_choices::RoutingHostConfig>
    for RoutingHostConfig
{
    fn from(value: warp_graphql::queries::get_feature_model_choices::RoutingHostConfig) -> Self {
        Self {
            enabled: value.enabled,
            model_routing_host: value.model_routing_host.into(),
        }
    }
}

impl From<warp_graphql::workspace::RoutingHostConfig> for RoutingHostConfig {
    fn from(value: warp_graphql::workspace::RoutingHostConfig) -> Self {
        Self {
            enabled: value.enabled,
            model_routing_host: value.model_routing_host.into(),
        }
    }
}

impl From<warp_graphql::queries::get_feature_model_choices::LlmModelHost> for LLMModelHost {
    fn from(value: warp_graphql::queries::get_feature_model_choices::LlmModelHost) -> Self {
        match value {
            warp_graphql::queries::get_feature_model_choices::LlmModelHost::DirectApi => {
                LLMModelHost::DirectApi
            }
            warp_graphql::queries::get_feature_model_choices::LlmModelHost::AwsBedrock => {
                LLMModelHost::AwsBedrock
            }
            warp_graphql::queries::get_feature_model_choices::LlmModelHost::CustomEndpoint => {
                LLMModelHost::CustomEndpoint
            }
            warp_graphql::queries::get_feature_model_choices::LlmModelHost::GeminiEnterprise => {
                LLMModelHost::GeminiEnterprise
            }
            warp_graphql::queries::get_feature_model_choices::LlmModelHost::Other(value) => {
                log::warn!(
                    "Unknown LlmModelHost '{value}'. Make sure to update client GraphQL types!"
                );
                LLMModelHost::Unknown
            }
        }
    }
}

impl From<warp_graphql::queries::get_feature_model_choices::LlmProvider> for LLMProvider {
    fn from(value: warp_graphql::queries::get_feature_model_choices::LlmProvider) -> Self {
        match value {
            warp_graphql::queries::get_feature_model_choices::LlmProvider::Openai => {
                LLMProvider::OpenAI
            }
            warp_graphql::queries::get_feature_model_choices::LlmProvider::Anthropic => {
                LLMProvider::Anthropic
            }
            warp_graphql::queries::get_feature_model_choices::LlmProvider::Google => {
                LLMProvider::Google
            }
            warp_graphql::queries::get_feature_model_choices::LlmProvider::Xai => LLMProvider::Xai,
            warp_graphql::queries::get_feature_model_choices::LlmProvider::Unknown => {
                LLMProvider::Unknown
            }
            warp_graphql::queries::get_feature_model_choices::LlmProvider::Other(value) => {
                report_error!(
                    anyhow!(
                        "Invalid LlmProvider '{value}'. Make sure to update client GraphQL types!"
                    ),
                    warp_core::errors::ReportErrorLogMode::OncePerRun
                );
                LLMProvider::Unknown
            }
        }
    }
}

impl From<warp_graphql::workspace::LlmProvider> for LLMProvider {
    fn from(value: warp_graphql::workspace::LlmProvider) -> Self {
        match value {
            warp_graphql::workspace::LlmProvider::Openai => LLMProvider::OpenAI,
            warp_graphql::workspace::LlmProvider::Anthropic => LLMProvider::Anthropic,
            warp_graphql::workspace::LlmProvider::Google => LLMProvider::Google,
            warp_graphql::workspace::LlmProvider::Xai => LLMProvider::Xai,
            warp_graphql::workspace::LlmProvider::Unknown => LLMProvider::Unknown,
            warp_graphql::workspace::LlmProvider::Other(value) => {
                report_error!(
                    anyhow!(
                        "Invalid LlmProvider '{value}'. Make sure to update client GraphQL types!"
                    ),
                    warp_core::errors::ReportErrorLogMode::OncePerRun
                );
                LLMProvider::Unknown
            }
        }
    }
}

impl From<warp_graphql::queries::get_feature_model_choices::LlmSpec> for LLMSpec {
    fn from(value: warp_graphql::queries::get_feature_model_choices::LlmSpec) -> Self {
        Self {
            cost: value.cost as f32,
            quality: value.quality as f32,
            speed: value.speed as f32,
        }
    }
}

impl From<warp_graphql::workspace::LlmSpec> for LLMSpec {
    fn from(value: warp_graphql::workspace::LlmSpec) -> Self {
        Self {
            cost: value.cost as f32,
            quality: value.quality as f32,
            speed: value.speed as f32,
        }
    }
}

impl From<warp_graphql::queries::get_feature_model_choices::LlmUsageMetadata> for LLMUsageMetadata {
    fn from(value: warp_graphql::queries::get_feature_model_choices::LlmUsageMetadata) -> Self {
        Self {
            request_multiplier: value.request_multiplier.max(1) as usize,
            credit_multiplier: value.credit_multiplier.map(|v| v as f32),
        }
    }
}

impl From<warp_graphql::workspace::LlmUsageMetadata> for LLMUsageMetadata {
    fn from(value: warp_graphql::workspace::LlmUsageMetadata) -> Self {
        Self {
            request_multiplier: value.request_multiplier.max(1) as usize,
            credit_multiplier: value.credit_multiplier.map(|v| v as f32),
        }
    }
}

impl From<warp_graphql::queries::get_feature_model_choices::DisableReason> for DisableReason {
    fn from(value: warp_graphql::queries::get_feature_model_choices::DisableReason) -> Self {
        match value {
            warp_graphql::queries::get_feature_model_choices::DisableReason::AdminDisabled => {
                DisableReason::AdminDisabled
            }
            warp_graphql::queries::get_feature_model_choices::DisableReason::OutOfRequests => {
                DisableReason::OutOfRequests
            }
            warp_graphql::queries::get_feature_model_choices::DisableReason::ProviderOutage => {
                DisableReason::ProviderOutage
            }
            warp_graphql::queries::get_feature_model_choices::DisableReason::RequiresUpgrade => {
                DisableReason::RequiresUpgrade
            }
            warp_graphql::queries::get_feature_model_choices::DisableReason::Other(_) => {
                DisableReason::Unavailable
            }
        }
    }
}

impl From<warp_graphql::workspace::DisableReason> for DisableReason {
    fn from(value: warp_graphql::workspace::DisableReason) -> Self {
        match value {
            warp_graphql::workspace::DisableReason::AdminDisabled => DisableReason::AdminDisabled,
            warp_graphql::workspace::DisableReason::OutOfRequests => DisableReason::OutOfRequests,
            warp_graphql::workspace::DisableReason::ProviderOutage => DisableReason::ProviderOutage,
            warp_graphql::workspace::DisableReason::RequiresUpgrade => {
                DisableReason::RequiresUpgrade
            }
            warp_graphql::workspace::DisableReason::Other(_) => DisableReason::Unavailable,
        }
    }
}

// Conversions for AIConversationMetadata from GraphQL types

fn convert_harness(harness: warp_graphql::ai::AgentHarness) -> AIAgentHarness {
    match harness {
        warp_graphql::ai::AgentHarness::Oz => AIAgentHarness::Oz,
        warp_graphql::ai::AgentHarness::ClaudeCode => AIAgentHarness::ClaudeCode,
        warp_graphql::ai::AgentHarness::Gemini => AIAgentHarness::Gemini,
        warp_graphql::ai::AgentHarness::Codex => AIAgentHarness::Codex,
        warp_graphql::ai::AgentHarness::Other(value) => {
            report_error!(
                anyhow!(
                    "Invalid AgentHarness '{value}'. Make sure to update client GraphQL types!"
                ),
                warp_core::errors::ReportErrorLogMode::OncePerRun
            );
            AIAgentHarness::Unknown
        }
    }
}

// Helper function
fn convert_usage_metadata(
    summarized: bool,
    context_window_usage: f64,
    credits_spent: f64,
    platform_credits_spent: f64,
    context_window_segments: &[warp_graphql::ai::ContextWindowSegment],
) -> ConversationUsageMetadata {
    ConversationUsageMetadata {
        was_summarized: summarized,
        context_window_usage: context_window_usage as f32,
        credits_spent: credits_spent as f32,
        platform_credits_spent: platform_credits_spent as f32,
        credits_spent_for_last_block: None,
        token_usage: vec![],
        tool_usage_metadata: Default::default(),
        context_window_segments: context_window_segments.iter().map(Into::into).collect(),
    }
}

impl TryFrom<warp_graphql::ai::AIConversation> for ServerAIConversationMetadata {
    type Error = anyhow::Error;

    fn try_from(value: warp_graphql::ai::AIConversation) -> Result<Self, Self::Error> {
        let usage = convert_usage_metadata(
            value.usage.usage_metadata.summarized,
            value.usage.usage_metadata.context_window_usage,
            value.usage.usage_metadata.credits_spent,
            value.usage.usage_metadata.platform_credits_spent,
            &value.usage.usage_metadata.context_window_segments,
        );
        let metadata = value.metadata.try_into()?;
        let permissions = value.permissions.try_into()?;
        let ambient_agent_task_id = value
            .ambient_agent_task_id
            .map(|id| id.into_inner().parse())
            .transpose()?;
        let server_conversation_token =
            ServerConversationToken::new(value.conversation_id.into_inner());

        // If we fail to parse any artifacts, don't fail the entire conversion -- just don't include them in the list
        let artifacts = value
            .artifacts
            .unwrap_or_default()
            .into_iter()
            .filter_map(|a| Artifact::try_from(a).ok())
            .collect();

        Ok(Self {
            title: value.title,
            working_directory: value.working_directory,
            harness: convert_harness(value.harness),
            usage,
            metadata,
            creator: value.creator.map(Into::into),
            permissions,
            ambient_agent_task_id,
            server_conversation_token,
            artifacts,
        })
    }
}

impl TryFrom<warp_graphql::queries::list_ai_conversations::AIConversationMetadata>
    for ServerAIConversationMetadata
{
    type Error = anyhow::Error;

    fn try_from(
        value: warp_graphql::queries::list_ai_conversations::AIConversationMetadata,
    ) -> Result<Self, Self::Error> {
        let usage = convert_usage_metadata(
            value.usage.usage_metadata.summarized,
            value.usage.usage_metadata.context_window_usage,
            value.usage.usage_metadata.credits_spent,
            value.usage.usage_metadata.platform_credits_spent,
            &value.usage.usage_metadata.context_window_segments,
        );
        let metadata = value.metadata.try_into()?;
        let permissions = value.permissions.try_into()?;
        let ambient_agent_task_id = value
            .ambient_agent_task_id
            .map(|id| id.into_inner().parse())
            .transpose()?;
        let server_conversation_token =
            ServerConversationToken::new(value.conversation_id.into_inner());

        let artifacts = value
            .artifacts
            .unwrap_or_default()
            .into_iter()
            .filter_map(|a| Artifact::try_from(a).ok())
            .collect();

        Ok(Self {
            title: value.title,
            working_directory: value.working_directory,
            harness: convert_harness(value.harness),
            usage,
            metadata,
            creator: value.creator.map(Into::into),
            permissions,
            ambient_agent_task_id,
            server_conversation_token,
            artifacts,
        })
    }
}

#[cfg(not(feature = "agent_mode_evals"))]
impl BonusGrant {
    pub fn from_gql_bonus_grant(
        bonus_grant: warp_graphql::billing::BonusGrant,
        scope: BonusGrantScope,
    ) -> Self {
        Self {
            created_at: bonus_grant.created_at.utc(),
            cost_cents: bonus_grant.cost_cents,
            expiration: bonus_grant.expiration.map(|exp| exp.utc()),
            grant_type: bonus_grant.grant_type,
            reason: bonus_grant.reason,
            user_facing_message: bonus_grant.user_facing_message,
            request_credits_granted: bonus_grant.request_credits_granted,
            request_credits_remaining: bonus_grant.request_credits_remaining,
            scope,
        }
    }
}

impl From<warp_graphql::workspace::LlmModelHost> for crate::ai::llms::LLMModelHost {
    fn from(gql_host: warp_graphql::workspace::LlmModelHost) -> Self {
        use warp_graphql::workspace::LlmModelHost as GqlLlmModelHost;
        match gql_host {
            GqlLlmModelHost::DirectApi => Self::DirectApi,
            GqlLlmModelHost::AwsBedrock => Self::AwsBedrock,
            GqlLlmModelHost::CustomEndpoint => Self::CustomEndpoint,
            GqlLlmModelHost::GeminiEnterprise => Self::GeminiEnterprise,
            GqlLlmModelHost::Other(value) => {
                log::warn!(
                    "Unknown LlmModelHost '{value}'. Make sure to update client GraphQL types!"
                );
                Self::Unknown
            }
        }
    }
}

#[cfg(test)]
#[path = "ai_tests.rs"]
mod tests;
