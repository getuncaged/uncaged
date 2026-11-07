use futures::future::BoxFuture;
use futures::FutureExt;
use uncaged_ui::{Entity, ModelContext};

use super::{ActionExecution, AnyActionExecution, ExecuteActionInput, PreprocessActionInput};
use crate::ai::agent::{
    AIAgentAction, AIAgentActionResultType, AIAgentActionType, SendMessageToAgentResult,
};

/// Agent-to-agent messages were delivered by Warp's server. Uncaged has no
/// such server (and runs no child agents), so every send fails at once.
const SEND_MESSAGE_UNSUPPORTED: &str =
    "Sending messages to other agents is not supported in Uncaged.";

pub struct SendMessageToAgentExecutor;

impl SendMessageToAgentExecutor {
    pub fn new() -> Self {
        Self
    }

    pub(super) fn should_autoexecute(
        &self,
        _input: ExecuteActionInput,
        _ctx: &mut ModelContext<Self>,
    ) -> bool {
        true
    }

    pub(super) fn execute(
        &mut self,
        input: ExecuteActionInput,
        _ctx: &mut ModelContext<Self>,
    ) -> AnyActionExecution {
        let AIAgentAction {
            action: AIAgentActionType::SendMessageToAgent { .. },
            ..
        } = input.action
        else {
            return ActionExecution::<()>::InvalidAction.into();
        };

        ActionExecution::<()>::Sync(AIAgentActionResultType::SendMessageToAgent(
            SendMessageToAgentResult::Error(SEND_MESSAGE_UNSUPPORTED.to_string()),
        ))
        .into()
    }

    pub(super) fn preprocess_action(
        &mut self,
        _action: PreprocessActionInput,
        _ctx: &mut ModelContext<Self>,
    ) -> BoxFuture<'static, ()> {
        futures::future::ready(()).boxed()
    }
}

impl Default for SendMessageToAgentExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for SendMessageToAgentExecutor {
    type Event = ();
}
