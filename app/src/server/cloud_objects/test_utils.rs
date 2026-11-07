use std::sync::mpsc::{sync_channel, Receiver};

use settings::manager::SettingsManager;
use uncaged_core::execution_mode::{AppExecutionMode, ExecutionMode};
use uncaged_ui::{App, ModelHandle, SingletonEntity};

use super::update_manager::{ObjectOperationResult, UpdateManager, UpdateManagerEvent};
use crate::cloud_object::model::actions::ObjectActions;
use crate::cloud_object::model::persistence::{CloudModel, CloudModelEvent};
use crate::persistence::ModelEvent;

/// The size of the bounded channel that we use to queue persistence/sqlite-related events.
const CHANNEL_SIZE: usize = 128;

pub struct UpdateManagerStruct {
    pub update_manager: ModelHandle<UpdateManager>,
    pub receiver: Receiver<ModelEvent>,
    pub cloud_model_events: async_channel::Receiver<CloudModelEvent>,
    pub operation_results: async_channel::Receiver<ObjectOperationResult>,
}

pub fn initialize_app(app: &mut App) {
    app.add_singleton_model(|ctx| AppExecutionMode::new(ExecutionMode::App, false, ctx));
    app.add_singleton_model(|_| SettingsManager::default());
    app.update(crate::settings::init_and_register_user_preferences);
    app.add_singleton_model(CloudModel::mock);
    app.add_singleton_model(|_| ObjectActions::new(Vec::new()));
}

/// Registers an [`UpdateManager`] that writes to a channel the test can read, and records the
/// events it and the [`CloudModel`] emit.
pub fn create_update_manager_struct(app: &mut App) -> UpdateManagerStruct {
    let (sender, receiver) = sync_channel(CHANNEL_SIZE);
    let update_manager = app.add_singleton_model(|_| UpdateManager::new(Some(sender)));

    let cloud_model_events = app.update(|ctx| {
        let (tx, rx) = async_channel::unbounded();
        ctx.subscribe_to_model(&CloudModel::handle(ctx), move |_, event, _| {
            let _ = tx.try_send(event.clone());
        });
        rx
    });

    let operation_results = app.update(|ctx| {
        let (tx, rx) = async_channel::unbounded();
        ctx.subscribe_to_model(&update_manager, move |_, event, _| {
            let UpdateManagerEvent::ObjectOperationComplete { result } = event;
            let _ = tx.try_send(result.clone());
        });
        rx
    });

    UpdateManagerStruct {
        update_manager,
        receiver,
        cloud_model_events,
        operation_results,
    }
}
