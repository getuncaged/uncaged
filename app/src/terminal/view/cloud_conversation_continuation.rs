use warp_cli::agent::Harness;
use warp_core::features::FeatureFlag;
use warpui::{AppContext, EntityId, SingletonEntity, ViewContext};

use crate::ai::agent::api::ServerConversationToken;
use crate::ai::agent::conversation::{
    AIAgentHarness, AIConversationId, ServerAIConversationMetadata,
};
use crate::ai::agent_conversations_model::AgentConversationsModel;
use crate::ai::ambient_agents::{
    conversation_output_status_from_conversation, AmbientAgentTask, AmbientAgentTaskId,
    AmbientConversationStatus,
};
use crate::ai::blocklist::BlocklistAIHistoryModel;
use crate::auth::AuthStateProvider;
use crate::cloud_object::{Owner, ServerGuestSubject};
use crate::drive::sharing::SharingAccessLevel;
use crate::editor::InteractionState;
use crate::terminal::view::conversation_ended_tombstone_view::{
    ConversationEndedTombstoneEvent, ConversationEndedTombstoneView,
};
use crate::terminal::view::{PendingUserQueryKind, RichContentInsertionPosition, TerminalView};
use crate::terminal::TerminalModel;
use crate::workspaces::user_workspaces::UserWorkspaces;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TombstoneCta {
    ContinueLocally { conversation_id: AIConversationId },
    ContinueInCloud { task_id: AmbientAgentTaskId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::terminal::view) enum CloudConversationContinuationUiState {
    FollowupInput,
    Tombstone { cta: Option<TombstoneCta> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::terminal::view) enum CloudConversationContinuationError {
    MissingTask,
    ActiveTaskExecution,
    MissingConversationToken,
    MissingServerConversationMetadata,
    UnknownHarness,
    UnknownConversationAccess,
}

impl CloudConversationContinuationError {
    pub(in crate::terminal::view) fn should_fallback_to_tombstone(self) -> bool {
        !matches!(self, Self::ActiveTaskExecution)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConversationAccess {
    Edit,
    ViewOnly,
    Unknown,
}

pub(in crate::terminal::view) fn resolve_cloud_conversation_continuation_ui_state(
    terminal_view_id: EntityId,
    task_id: AmbientAgentTaskId,
    app: &AppContext,
) -> Result<CloudConversationContinuationUiState, CloudConversationContinuationError> {
    let Some(task) = AgentConversationsModel::as_ref(app).get_task_data(&task_id) else {
        return Err(CloudConversationContinuationError::MissingTask);
    };
    if task.blocks_cloud_followups() {
        return Ok(CloudConversationContinuationUiState::Tombstone { cta: None });
    }
    if task.has_active_execution() {
        return Err(CloudConversationContinuationError::ActiveTaskExecution);
    }
    let is_environment_setup_failure = task.state.is_failure_like()
        && task
            .status_message
            .as_ref()
            .is_some_and(|status_message| status_message.is_environment_setup_failure());
    if is_environment_setup_failure && task.conversation_id().is_none() {
        return Ok(CloudConversationContinuationUiState::Tombstone { cta: None });
    }
    let conversation_token = task
        .conversation_id()
        .map(|token| ServerConversationToken::new(token.to_string()));
    let history_model = BlocklistAIHistoryModel::as_ref(app);

    if let Some(conversation_token) = conversation_token.as_ref() {
        if let Some(metadata) =
            history_model.get_server_conversation_metadata_by_server_token(conversation_token)
        {
            return continuation_ui_state_for_harness_and_access(
                metadata.harness,
                conversation_access(metadata, app),
                terminal_view_id,
                Some(conversation_token),
                task_id,
                history_model,
            );
        }
    }

    let access = task_creator_access(&task, app);
    if access == ConversationAccess::Edit {
        return continuation_ui_state_for_harness_and_access(
            task_harness(&task),
            access,
            terminal_view_id,
            conversation_token.as_ref(),
            task_id,
            history_model,
        );
    }

    if conversation_token.is_none() {
        Err(CloudConversationContinuationError::MissingConversationToken)
    } else {
        Err(CloudConversationContinuationError::MissingServerConversationMetadata)
    }
}

fn continuation_ui_state_for_harness_and_access(
    harness: AIAgentHarness,
    access: ConversationAccess,
    terminal_view_id: EntityId,
    conversation_token: Option<&ServerConversationToken>,
    task_id: AmbientAgentTaskId,
    history_model: &BlocklistAIHistoryModel,
) -> Result<CloudConversationContinuationUiState, CloudConversationContinuationError> {
    match (harness, access) {
        (AIAgentHarness::Oz, ConversationAccess::Edit) => {
            Ok(CloudConversationContinuationUiState::FollowupInput)
        }
        (AIAgentHarness::Oz, ConversationAccess::ViewOnly) => {
            let cta = conversation_token
                .and_then(|conversation_token| {
                    local_conversation_id_for_local_continuation(
                        terminal_view_id,
                        conversation_token,
                        history_model,
                    )
                })
                .map(|conversation_id| TombstoneCta::ContinueLocally { conversation_id });
            Ok(CloudConversationContinuationUiState::Tombstone { cta })
        }
        (
            AIAgentHarness::ClaudeCode | AIAgentHarness::Gemini | AIAgentHarness::Codex,
            ConversationAccess::Edit,
        ) => Ok(CloudConversationContinuationUiState::Tombstone {
            cta: Some(TombstoneCta::ContinueInCloud { task_id }),
        }),
        (
            AIAgentHarness::ClaudeCode | AIAgentHarness::Gemini | AIAgentHarness::Codex,
            ConversationAccess::ViewOnly,
        ) => Ok(CloudConversationContinuationUiState::Tombstone { cta: None }),
        (AIAgentHarness::Unknown, _) => Err(CloudConversationContinuationError::UnknownHarness),
        (_, ConversationAccess::Unknown) => {
            Err(CloudConversationContinuationError::UnknownConversationAccess)
        }
    }
}

fn conversation_access(
    metadata: &ServerAIConversationMetadata,
    app: &AppContext,
) -> ConversationAccess {
    let Some(current_user_uid) = AuthStateProvider::as_ref(app).get().user_id() else {
        return ConversationAccess::Unknown;
    };

    let mut access_level = SharingAccessLevel::View;
    match metadata.permissions.space {
        Owner::User { user_uid } => {
            let is_current_user_owner = user_uid == current_user_uid;
            if is_current_user_owner {
                access_level = access_level.max(SharingAccessLevel::Full);
            }
        }
        Owner::Team { team_uid } => {
            let is_current_team_owner = UserWorkspaces::as_ref(app)
                .team_from_uid_across_all_workspaces(team_uid)
                .is_some();
            if is_current_team_owner {
                access_level = access_level.max(SharingAccessLevel::Full);
            }
        }
    }

    if let Some(link_sharing) = &metadata.permissions.anyone_link_sharing {
        access_level = access_level.max(link_sharing.access_level.into());
    }

    // Direct user and team ACLs can both apply, so use the highest matching grant.
    for guest in &metadata.permissions.guests {
        match &guest.subject {
            ServerGuestSubject::User { firebase_uid } => {
                let matches_current_user = firebase_uid == current_user_uid.as_str();
                if matches_current_user {
                    access_level = access_level.max(guest.access_level.into());
                }
            }
            ServerGuestSubject::Team { team_uid } => {
                let matches_current_team = UserWorkspaces::as_ref(app)
                    .team_from_uid_across_all_workspaces(*team_uid)
                    .is_some();
                if matches_current_team {
                    access_level = access_level.max(guest.access_level.into());
                }
            }
            ServerGuestSubject::PendingUser { .. } => {}
        }
    }
    let is_creator = metadata
        .metadata
        .creator_uid
        .as_ref()
        .is_some_and(|creator_uid| creator_uid == current_user_uid.as_str());
    if is_creator {
        access_level = access_level.max(SharingAccessLevel::Edit);
    }
    if access_level >= SharingAccessLevel::Edit {
        ConversationAccess::Edit
    } else {
        ConversationAccess::ViewOnly
    }
}

fn task_creator_access(task: &AmbientAgentTask, app: &AppContext) -> ConversationAccess {
    let Some(current_user_uid) = AuthStateProvider::as_ref(app).get().user_id() else {
        return ConversationAccess::Unknown;
    };

    if task
        .creator
        .as_ref()
        .is_some_and(|creator| creator.uid == current_user_uid.as_str())
    {
        ConversationAccess::Edit
    } else {
        ConversationAccess::Unknown
    }
}

fn task_harness(task: &AmbientAgentTask) -> AIAgentHarness {
    match task
        .agent_config_snapshot
        .as_ref()
        .and_then(|config| config.harness.as_ref())
        .map(|harness| harness.harness_type)
        .unwrap_or(Harness::Oz)
    {
        Harness::Oz => AIAgentHarness::Oz,
        Harness::Claude => AIAgentHarness::ClaudeCode,
        Harness::Gemini => AIAgentHarness::Gemini,
        Harness::Codex => AIAgentHarness::Codex,
        Harness::OpenCode | Harness::Unknown => AIAgentHarness::Unknown,
    }
}

pub(in crate::terminal::view) fn conversation_failed_before_task_creation(
    terminal_view_id: EntityId,
    history_model: &BlocklistAIHistoryModel,
) -> bool {
    if history_model.is_terminal_view_conversation_transcript_viewer(terminal_view_id) {
        return false;
    }
    history_model
        .all_live_conversations_for_terminal_view(terminal_view_id)
        .next()
        .and_then(conversation_output_status_from_conversation)
        .is_some_and(|status| matches!(status, AmbientConversationStatus::Error { .. }))
}

fn local_conversation_id_for_local_continuation(
    terminal_view_id: EntityId,
    conversation_token: &ServerConversationToken,
    history_model: &BlocklistAIHistoryModel,
) -> Option<AIConversationId> {
    history_model
        .all_live_conversations_for_terminal_view(terminal_view_id)
        .find(|conversation| conversation.server_conversation_token() == Some(conversation_token))
        .map(|conversation| conversation.id())
        .or_else(|| history_model.find_conversation_id_by_server_token(conversation_token))
}

impl TerminalView {
    pub(in crate::terminal::view) fn cloud_conversation_continuation_ui_state(
        &self,
        ctx: &AppContext,
    ) -> Option<CloudConversationContinuationUiState> {
        let task_id = {
            let model = self.model.lock();
            if !FeatureFlag::CloudModeSetupV2.is_enabled()
                || !FeatureFlag::HandoffCloudCloud.is_enabled()
            {
                return None;
            }

            let is_cloud_conversation_surface = model.is_conversation_transcript_viewer()
                || self
                    .ambient_agent_view_model
                    .as_ref()
                    .is_some_and(|model| model.as_ref(ctx).is_ambient_agent());
            if !is_cloud_conversation_surface {
                return None;
            }

            self.ambient_agent_task_id_for_details_panel_from_model(&model, ctx)
        };
        let Some(task_id) = task_id else {
            return conversation_failed_before_task_creation(
                self.id(),
                BlocklistAIHistoryModel::as_ref(ctx),
            )
            .then_some(CloudConversationContinuationUiState::Tombstone { cta: None });
        };
        match resolve_cloud_conversation_continuation_ui_state(self.id(), task_id, ctx) {
            Ok(state) => Some(state),
            Err(error) => error
                .should_fallback_to_tombstone()
                .then_some(CloudConversationContinuationUiState::Tombstone { cta: None }),
        }
    }

    pub(in crate::terminal::view) fn blocks_cloud_followups_for_ambient_agent_session_from_model(
        &self,
        model: &TerminalModel,
        ctx: &AppContext,
    ) -> bool {
        if self
            .ambient_agent_view_model
            .as_ref()
            .is_some_and(|model| model.as_ref(ctx).blocks_cloud_followups())
        {
            return true;
        }

        let Some(task_id) = self.ambient_agent_task_id_for_details_panel_from_model(model, ctx)
        else {
            return false;
        };

        AgentConversationsModel::as_ref(ctx)
            .get_task_data(&task_id)
            .is_some_and(|task| task.blocks_cloud_followups())
    }

    pub(crate) fn owned_ambient_agent_task_id(
        &self,
        ctx: &AppContext,
    ) -> Option<AmbientAgentTaskId> {
        let task_id = self.ambient_agent_task_id_for_details_panel(ctx)?;
        self.is_current_user_creator_of_ambient_task(task_id, ctx)
            .then_some(task_id)
    }

    fn is_current_user_creator_of_ambient_task(
        &self,
        task_id: AmbientAgentTaskId,
        ctx: &AppContext,
    ) -> bool {
        let Some(current_user_uid) = self.auth_state.user_id().map(|uid| uid.as_string()) else {
            return false;
        };

        AgentConversationsModel::as_ref(ctx)
            .get_task_data(&task_id)
            .and_then(|task| task.creator.map(|creator| creator.uid))
            .is_some_and(|creator_uid| creator_uid == current_user_uid)
    }

    pub(in crate::terminal::view) fn enable_cloud_followup_input(
        &mut self,
        task_id: AmbientAgentTaskId,
        ctx: &mut ViewContext<Self>,
    ) {
        self.pending_cloud_followup_task_id = Some(task_id);
        self.input.update(ctx, |input, ctx| {
            input.reset_after_cloud_followup_submission(ctx);
            input.set_input_mode_agent(true, ctx);
            input.editor().update(ctx, |editor, ctx| {
                editor.set_interaction_state(InteractionState::Editable, ctx);
            });
        });
        self.update_pane_configuration(ctx);
        ctx.notify();
    }

    fn enable_cloud_followup_input_after_conversation_end(
        &mut self,
        task_id: AmbientAgentTaskId,
        ctx: &mut ViewContext<Self>,
    ) {
        self.remove_conversation_ended_tombstone(ctx);
        self.enable_cloud_followup_input(task_id, ctx);
    }

    fn start_cloud_followup_from_tombstone(
        &mut self,
        task_id: AmbientAgentTaskId,
        ctx: &mut ViewContext<Self>,
    ) {
        if !FeatureFlag::HandoffCloudCloud.is_enabled() {
            return;
        }

        let Some(ambient_agent_view_model) = self.ambient_agent_view_model.as_ref() else {
            self.show_error_toast("Couldn't continue this cloud task.".to_string(), ctx);
            return;
        };

        if ambient_agent_view_model.as_ref(ctx).task_id() != Some(task_id) {
            self.show_error_toast("Couldn't continue this cloud task.".to_string(), ctx);
            return;
        }
        self.enable_cloud_followup_input_after_conversation_end(task_id, ctx);
        self.focus_input_box(ctx);
        ctx.notify();
    }

    pub(crate) fn insert_conversation_ended_tombstone_with_cta(
        &mut self,
        tombstone_cta: Option<TombstoneCta>,
        ctx: &mut ViewContext<Self>,
    ) {
        if self.conversation_ended_tombstone_view_id.is_some() {
            self.remove_conversation_ended_tombstone(ctx);
        }
        let task_id = self.ambient_agent_task_id_for_details_panel(ctx);
        let terminal_view_id = self.id();

        let tombstone_view_handle = ctx.add_typed_action_view(|ctx| {
            ConversationEndedTombstoneView::new(ctx, terminal_view_id, task_id, tombstone_cta)
        });
        ctx.subscribe_to_view(&tombstone_view_handle, |me, _, event, ctx| match event {
            ConversationEndedTombstoneEvent::ContinueInCloud { task_id } => {
                me.start_cloud_followup_from_tombstone(*task_id, ctx);
            }
        });
        let tombstone_view_id = tombstone_view_handle.id();
        // The cloud-mode queued-prompt block is pinned to the bottom so it stays below any
        // streaming agent output. When inserting the conversation-ended tombstone we want the
        // tombstone below the queued prompt instead, so unpin the queued prompt first.
        if self.pending_user_query_kind == Some(PendingUserQueryKind::CloudMode) {
            if let Some(pending_query_view_id) = self.pending_user_query_view_id {
                self.model
                    .lock()
                    .block_list_mut()
                    .unpin_rich_content_from_bottom(pending_query_view_id);
            }
        }
        let insertion_position = self
            .pending_user_query_view_id
            .map(RichContentInsertionPosition::AfterRichContent)
            .unwrap_or(RichContentInsertionPosition::Append {
                insert_below_long_running_block: true,
            });
        self.insert_rich_content(None, tombstone_view_handle, None, insertion_position, ctx);
        self.conversation_ended_tombstone_view_id = Some(tombstone_view_id);
    }

    pub(crate) fn insert_conversation_ended_tombstone_with_resolved_cta(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        if !FeatureFlag::HandoffCloudCloud.is_enabled() {
            self.insert_conversation_ended_tombstone_with_cta(None, ctx);
            return;
        }

        match self.cloud_conversation_continuation_ui_state(ctx) {
            Some(CloudConversationContinuationUiState::Tombstone { cta }) => {
                self.insert_conversation_ended_tombstone_with_cta(cta, ctx);
            }
            Some(CloudConversationContinuationUiState::FollowupInput) => {
                if let Some(task_id) = self.ambient_agent_task_id_for_details_panel(ctx) {
                    self.enable_cloud_followup_input_after_conversation_end(task_id, ctx);
                } else {
                    self.insert_conversation_ended_tombstone_with_cta(None, ctx);
                }
            }
            None => {
                self.insert_conversation_ended_tombstone_with_cta(None, ctx);
            }
        }
    }

    pub(in crate::terminal::view) fn remove_conversation_ended_tombstone(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(view_id) = self.conversation_ended_tombstone_view_id.take() else {
            return;
        };
        self.model
            .lock()
            .block_list_mut()
            .remove_rich_content(view_id);
        self.rich_content_views.retain(|rc| rc.view_id() != view_id);
        ctx.notify();
    }
}

#[cfg(test)]
#[path = "cloud_conversation_continuation_tests.rs"]
mod tests;
