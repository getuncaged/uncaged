#[cfg(not(target_family = "wasm"))]
use std::path::PathBuf;

#[cfg(test)]
#[path = "upload_artifact_tests.rs"]
mod tests;

use futures::future::BoxFuture;
use futures::FutureExt;
#[cfg(not(target_family = "wasm"))]
use warpui::SingletonEntity;
use warpui::{Entity, EntityId, ModelContext, ModelHandle};

use super::{ActionExecution, AnyActionExecution, ExecuteActionInput, PreprocessActionInput};
#[cfg(not(target_family = "wasm"))]
use crate::ai::{
    agent::{AIAgentAction, AIAgentActionResultType, AIAgentActionType, UploadArtifactResult},
    blocklist::BlocklistAIPermissions,
    paths::host_native_absolute_path,
};
use crate::terminal::model::session::active_session::ActiveSession;

#[cfg(not(target_family = "wasm"))]
const UPLOAD_ARTIFACT_UNSUPPORTED_MESSAGE: &str = "Uploading artifacts is not supported in Uncaged";

pub struct UploadArtifactExecutor {
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    active_session: ModelHandle<ActiveSession>,
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    terminal_view_id: EntityId,
}

impl UploadArtifactExecutor {
    pub fn new(active_session: ModelHandle<ActiveSession>, terminal_view_id: EntityId) -> Self {
        Self {
            active_session,
            terminal_view_id,
        }
    }

    #[cfg_attr(target_family = "wasm", allow(unused_variables), allow(dead_code))]
    pub(super) fn should_autoexecute(
        &self,
        input: ExecuteActionInput,
        ctx: &mut ModelContext<Self>,
    ) -> bool {
        #[cfg(target_family = "wasm")]
        {
            false
        }

        #[cfg(not(target_family = "wasm"))]
        {
            let ExecuteActionInput {
                action:
                    AIAgentAction {
                        action: AIAgentActionType::UploadArtifact(request),
                        ..
                    },
                conversation_id,
            } = input
            else {
                return false;
            };

            let resolved_path = self.resolve_path(&request.file_path, ctx);
            BlocklistAIPermissions::as_ref(ctx)
                .can_read_files_with_conversation(
                    &conversation_id,
                    vec![resolved_path],
                    Some(self.terminal_view_id),
                    ctx,
                )
                .is_allowed()
        }
    }

    #[cfg_attr(target_family = "wasm", allow(unused_variables), allow(dead_code))]
    pub(super) fn execute(&self, input: ExecuteActionInput) -> AnyActionExecution {
        #[cfg(target_family = "wasm")]
        {
            ActionExecution::<()>::InvalidAction.into()
        }

        #[cfg(not(target_family = "wasm"))]
        {
            let ExecuteActionInput { action, .. } = input;
            let AIAgentAction {
                action: AIAgentActionType::UploadArtifact(_),
                ..
            } = action
            else {
                return ActionExecution::<()>::InvalidAction.into();
            };

            // Artifacts were uploaded to Warp's servers; Uncaged has none.
            ActionExecution::<()>::Sync(AIAgentActionResultType::UploadArtifact(
                UploadArtifactResult::Error(UPLOAD_ARTIFACT_UNSUPPORTED_MESSAGE.to_string()),
            ))
            .into()
        }
    }

    pub(super) fn preprocess_action(
        &mut self,
        _input: PreprocessActionInput,
        _ctx: &mut ModelContext<Self>,
    ) -> BoxFuture<'static, ()> {
        futures::future::ready(()).boxed()
    }

    #[cfg(not(target_family = "wasm"))]
    fn resolve_path(&self, file_path: &str, ctx: &ModelContext<Self>) -> PathBuf {
        let current_working_directory = self
            .active_session
            .as_ref(ctx)
            .current_working_directory()
            .cloned();
        let shell = self.active_session.as_ref(ctx).shell_launch_data(ctx);

        PathBuf::from(host_native_absolute_path(
            file_path,
            &shell,
            &current_working_directory,
        ))
    }
}

impl Entity for UploadArtifactExecutor {
    type Event = ();
}
