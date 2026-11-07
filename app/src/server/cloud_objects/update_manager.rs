use std::collections::HashSet;
use std::future::Future;
use std::sync::mpsc::SyncSender;
use std::sync::Arc;

use chrono::Utc;
use lazy_static::lazy_static;
use regex::Regex;
use warp_graphql::scalars::time::ServerTimestamp;
use warp_util::sync::Condition;
use warpui::{Entity, ModelContext, SingletonEntity};

use crate::ai::execution_profiles::{AIExecutionProfile, CloudAIExecutionProfileModel};
use crate::ai::facts::{AIFact, CloudAIFactModel};
#[cfg(not(target_family = "wasm"))]
use crate::ai::mcp::templatable::{CloudTemplatableMCPServerModel, TemplatableMCPServer};
use crate::cloud_object::model::actions::{ObjectActionType, ObjectActions};
use crate::cloud_object::model::generic_string_model::GenericStringObjectId;
use crate::cloud_object::model::persistence::{CloudModel, CloudModelEvent, UpdateSource};
use crate::cloud_object::{
    CloudModelType, CloudObject, CloudObjectLocation, CloudObjectSyncStatus, GenericCloudObject,
    GenericStringObjectFormat, JsonObjectType, ObjectIdType, Owner, Revision, Space,
};
use crate::drive::folders::CloudFolderModel;
use crate::drive::CloudObjectTypeAndId;
use crate::env_vars::{CloudEnvVarCollectionModel, EnvVarCollection};
use crate::notebooks::{CloudNotebookModel, NotebookId};
use crate::persistence::ModelEvent;
use crate::server::ids::{ClientId, HashableId, ObjectUid, SyncId, ToServerId};
use crate::workflows::workflow::Workflow;
use crate::workflows::workflow_enum::{CloudWorkflowEnumModel, WorkflowEnum};
use crate::workflows::{CloudWorkflowModel, WorkflowId};

lazy_static! {
    static ref DUPLICATE_OBJECT_NAME_REGEX: Regex =
        Regex::new(r" \((\d+)\)$").expect("regex should not fail to compile");
}

#[derive(Clone, Debug, PartialEq)]
pub enum OperationSuccessType {
    Success,
    Failure,
    Rejection,
    Denied(String),
    FeatureNotAvailable,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ObjectOperation {
    Create { initiated_by: InitiatedBy },
    Update,
    MoveToFolder,
    MoveToDrive,
    Trash,
    TakeEditAccess,
    Untrash,
    Delete { initiated_by: InitiatedBy },
    EmptyTrash,
    UpdatePermissions,
    Leave,
}

#[derive(Clone, Debug)]
pub struct ObjectOperationResult {
    pub success_type: OperationSuccessType,
    pub operation: ObjectOperation,
    /// The object the operation applied to. `None` for operations on a whole space, such as
    /// emptying the trash.
    pub id: Option<SyncId>,
    pub num_objects: Option<i32>, // counts number of objects (including descendants) deleted for permadeletion
}

#[derive(Debug)]
pub enum UpdateManagerEvent {
    ObjectOperationComplete { result: ObjectOperationResult },
}

/// An enum that defines whether the action was initiated by the user or the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitiatedBy {
    User,
    System,
}

/// The UpdateManager applies every Warp Drive mutation (e.g. from a user interaction). Uncaged's
/// Drive is a local store, so each mutation
/// - updates the in-memory state in the CloudModel used by the object views,
/// - writes the object to SQLite, and
/// - reports completion with [`UpdateManagerEvent::ObjectOperationComplete`] right away.
///
/// Local mutations never mark an object as pending: SQLite skips writing the trash state, folder
/// and editor while `has_pending_metadata_change` is set, so a pending flag would lose them on
/// restart.
pub struct UpdateManager {
    model_event_sender: Option<SyncSender<ModelEvent>>,
    has_initial_load: Condition,
}

impl UpdateManager {
    pub fn new(model_event_sender: Option<SyncSender<ModelEvent>>) -> Self {
        // Everything is loaded from SQLite before the UpdateManager is created.
        let has_initial_load = Condition::new();
        has_initial_load.set();

        Self {
            model_event_sender,
            has_initial_load,
        }
    }

    fn save_to_db(&self, events: impl IntoIterator<Item = ModelEvent>) {
        if let Some(model_event_sender) = &self.model_event_sender {
            for event in events {
                if let Err(e) = model_event_sender.send(event) {
                    log::error!("Error saving to database: {e:?}");
                }
            }
        }
    }

    /// Writes the in-memory object, with all of its content, metadata and permissions, to SQLite.
    fn save_in_memory_object_to_sqlite(&self, cloud_model: &CloudModel, uid: &ObjectUid) {
        if let Some(cloud_object) = cloud_model.get_by_uid(uid) {
            self.save_to_db([cloud_object.upsert_event()]);
        }
    }

    fn emit_success(
        operation: ObjectOperation,
        id: Option<SyncId>,
        num_objects: Option<i32>,
        ctx: &mut ModelContext<Self>,
    ) {
        ctx.emit(UpdateManagerEvent::ObjectOperationComplete {
            result: ObjectOperationResult {
                success_type: OperationSuccessType::Success,
                operation,
                id,
                num_objects,
            },
        });
    }

    /// Wait for the initial load to complete. Drive objects are loaded from SQLite before the
    /// UpdateManager exists, so this resolves immediately.
    pub fn initial_load_complete(&self) -> impl Future<Output = ()> {
        // We're not using `async fn` here so that the returned Future doesn't borrow self.
        self.has_initial_load.wait()
    }

    /// Replace an object's data with the conflicting version from the server. If the object does
    /// not have a conflict, this has no effect.
    pub fn replace_object_with_conflict(&mut self, uid: &ObjectUid, ctx: &mut ModelContext<Self>) {
        let cloud_model_handle = CloudModel::handle(ctx);

        // Update the in-memory model first, and check for conflicts.
        let had_conflicts = cloud_model_handle.update(ctx, |cloud_model, ctx| {
            match cloud_model.get_mut_by_uid(uid) {
                Some(object) if object.has_conflicting_changes() => {
                    object.replace_object_with_conflict();
                    ctx.emit(CloudModelEvent::ObjectUpdated {
                        type_and_id: object.cloud_object_type_and_id(),
                        source: UpdateSource::Server,
                    });
                    true
                }
                _ => false,
            }
        });

        // Update SQLite, but only if the in-memory model was updated.
        if had_conflicts {
            self.save_in_memory_object_to_sqlite(cloud_model_handle.as_ref(ctx), uid);
        }
    }

    pub fn update_ai_fact(
        &mut self,
        ai_fact: AIFact,
        ai_fact_id: SyncId,
        ctx: &mut ModelContext<Self>,
    ) {
        self.update_object(CloudAIFactModel::new(ai_fact), ai_fact_id, ctx);
    }

    #[cfg(not(target_family = "wasm"))]
    pub fn update_templatable_mcp_server(
        &mut self,
        templatable_mcp_server: TemplatableMCPServer,
        templatable_mcp_server_id: SyncId,
        ctx: &mut ModelContext<Self>,
    ) {
        self.update_object(
            CloudTemplatableMCPServerModel::new(templatable_mcp_server),
            templatable_mcp_server_id,
            ctx,
        );
    }

    pub fn update_workflow(
        &mut self,
        workflow: Workflow,
        workflow_id: SyncId,
        ctx: &mut ModelContext<Self>,
    ) {
        self.update_object(CloudWorkflowModel::new(workflow), workflow_id, ctx);
    }

    pub fn update_workflow_enum(
        &mut self,
        workflow_enum: WorkflowEnum,
        workflow_enum_id: SyncId,
        ctx: &mut ModelContext<Self>,
    ) {
        self.update_object(
            CloudWorkflowEnumModel::new(workflow_enum),
            workflow_enum_id,
            ctx,
        );
    }

    pub fn update_env_var_collection(
        &mut self,
        env_var_collection: EnvVarCollection,
        env_var_collection_id: SyncId,
        ctx: &mut ModelContext<Self>,
    ) {
        self.update_object(
            CloudEnvVarCollectionModel::new(env_var_collection),
            env_var_collection_id,
            ctx,
        );
    }

    pub fn update_notebook_data(
        &mut self,
        data: Arc<String>,
        notebook_id: SyncId,
        ctx: &mut ModelContext<Self>,
    ) {
        let cloud_model = CloudModel::as_ref(ctx);
        if let Some(notebook) = cloud_model.get_notebook(&notebook_id) {
            let new_notebook = CloudNotebookModel {
                title: notebook.model().title.to_owned(),
                data: data.to_string(),
                ai_document_id: notebook.model().ai_document_id,
                conversation_id: notebook.model().conversation_id.clone(),
            };
            self.update_object(new_notebook, notebook_id, ctx);
        } else {
            log::warn!("Expected notebook to be in model with id {notebook_id:?}");
        }
    }

    pub fn update_notebook_title(
        &mut self,
        title: Arc<String>,
        notebook_id: SyncId,
        ctx: &mut ModelContext<Self>,
    ) {
        let cloud_model = CloudModel::as_ref(ctx);
        if let Some(notebook) = cloud_model.get_notebook(&notebook_id) {
            let new_notebook = CloudNotebookModel {
                title: title.to_string(),
                data: notebook.model().data.to_owned(),
                ai_document_id: notebook.model().ai_document_id,
                conversation_id: notebook.model().conversation_id.clone(),
            };
            self.update_object(new_notebook, notebook_id, ctx);
        } else {
            log::warn!("Expected notebook to be in model with id {notebook_id:?}");
        }
    }

    /// Moves an object from its current location to a new location. Moving into the trash
    /// trashes the object. Objects stay in their drive: moving to a space only moves the object
    /// to the root of its current drive.
    pub fn move_object_to_location(
        &mut self,
        object_id: CloudObjectTypeAndId,
        new_location: CloudObjectLocation,
        ctx: &mut ModelContext<Self>,
    ) {
        let new_folder_id = match new_location {
            CloudObjectLocation::Trash => return self.trash_object(object_id, ctx),
            // Moving to a space root clears the folder parent.
            CloudObjectLocation::Space(_) => None,
            CloudObjectLocation::Folder(folder_id) => Some(folder_id),
        };

        let uid = object_id.uid();
        CloudModel::handle(ctx).update(ctx, |model, ctx| {
            model.update_object_location(&uid, None, new_folder_id, ctx);
        });
        self.save_in_memory_object_to_sqlite(CloudModel::as_ref(ctx), &uid);
        ctx.notify();
    }

    pub fn duplicate_object(
        &mut self,
        cloud_object_type_and_id: &CloudObjectTypeAndId,
        ctx: &mut ModelContext<Self>,
    ) {
        match cloud_object_type_and_id {
            CloudObjectTypeAndId::Notebook(notebook_id) => {
                self.duplicate_object_internal::<NotebookId, CloudNotebookModel>(notebook_id, ctx);
            }
            CloudObjectTypeAndId::Workflow(workflow_id) => {
                self.duplicate_object_internal::<WorkflowId, CloudWorkflowModel>(workflow_id, ctx);
            }
            CloudObjectTypeAndId::GenericStringObject { object_type, id } => {
                if let GenericStringObjectFormat::Json(JsonObjectType::EnvVarCollection) =
                    object_type
                {
                    self.duplicate_object_internal::<GenericStringObjectId, CloudEnvVarCollectionModel>(
                        id, ctx,
                    );
                } else {
                    log::error!("Tried to duplicate an unsupported type: json object");
                    debug_assert!(false, "Tried to duplicate an unsupported type: json object");
                }
            }
            CloudObjectTypeAndId::Folder(_) => {
                // Duplicating folders not currently supported.
                log::error!("Tried to duplicate an unsupported type: folder");
                debug_assert!(false, "Tried to duplicate an unsupported type: folder");
            }
        }
    }

    fn duplicate_object_internal<K, M>(&mut self, id: &SyncId, ctx: &mut ModelContext<Self>)
    where
        K: HashableId
            + ToServerId
            + std::fmt::Debug
            + Into<String>
            + Clone
            + Copy
            + Send
            + Sync
            + 'static,
        M: CloudModelType<IdType = K, CloudObjectType = GenericCloudObject<K, M>> + 'static,
    {
        let (duplicate_model, owner, initial_folder_id) = {
            let cloud_model = CloudModel::as_ref(ctx);
            let object: GenericCloudObject<K, M> = cloud_model
                .get_object_of_type(id)
                .expect("object should exist in order to be duplicated")
                .clone();
            let mut duplicate_model = object.model().clone();
            let duplicate_name =
                self.get_next_duplicate_object_name(&object as &dyn CloudObject, cloud_model);
            duplicate_model.set_display_name(&duplicate_name);
            (
                duplicate_model,
                object.permissions.owner,
                object.metadata.folder_id,
            )
        };
        self.create_object(
            duplicate_model,
            owner,
            ClientId::new(),
            true,
            initial_folder_id,
            // When adding the initiated_by parameter to this function call, InitiatedBy::User was set as a default value.
            // This can be changed to InitiatedBy::System if this action was automatically kicked off by the system and we do not want a user facing toast.
            InitiatedBy::User,
            ctx,
        );
    }

    pub fn create_ai_fact(
        &mut self,
        ai_fact: AIFact,
        client_id: ClientId,
        owner: Owner,
        ctx: &mut ModelContext<Self>,
    ) {
        self.create_object(
            CloudAIFactModel::new(ai_fact),
            owner,
            client_id,
            false,
            None,
            // When adding the initiated_by parameter to this function call, InitiatedBy::User was set as a default value.
            // This can be changed to InitiatedBy::System if this action was automatically kicked off by the system and we do not want a user facing toast.
            InitiatedBy::User,
            ctx,
        );
    }

    #[cfg(not(target_family = "wasm"))]
    pub fn create_templatable_mcp_server(
        &mut self,
        templatable_mcp_server: TemplatableMCPServer,
        client_id: ClientId,
        owner: Owner,
        initiated_by: InitiatedBy,
        ctx: &mut ModelContext<Self>,
    ) {
        self.create_object(
            CloudTemplatableMCPServerModel::new(templatable_mcp_server),
            owner,
            client_id,
            false,
            None,
            initiated_by,
            ctx,
        );
    }

    #[allow(dead_code)]
    pub fn create_ai_execution_profile(
        &mut self,
        ai_execution_profile: AIExecutionProfile,
        client_id: ClientId,
        owner: Owner,
        ctx: &mut ModelContext<Self>,
    ) {
        self.create_object(
            CloudAIExecutionProfileModel::new(ai_execution_profile),
            owner,
            client_id,
            false,
            None,
            // When adding the initiated_by parameter to this function call, InitiatedBy::User was set as a default value.
            // This can be changed to InitiatedBy::System if this action was automatically kicked off by the system and we do not want a user facing toast.
            InitiatedBy::User,
            ctx,
        );
    }

    #[allow(dead_code)]
    pub fn update_ai_execution_profile(
        &mut self,
        ai_execution_profile: AIExecutionProfile,
        ai_execution_profile_id: SyncId,
        ctx: &mut ModelContext<Self>,
    ) {
        self.update_object(
            CloudAIExecutionProfileModel::new(ai_execution_profile),
            ai_execution_profile_id,
            ctx,
        );
    }

    pub fn delete_ai_execution_profile(
        &mut self,
        ai_execution_profile_id: SyncId,
        ctx: &mut ModelContext<Self>,
    ) {
        self.delete_object_by_user(
            CloudObjectTypeAndId::GenericStringObject {
                object_type: GenericStringObjectFormat::Json(JsonObjectType::AIExecutionProfile),
                id: ai_execution_profile_id,
            },
            ctx,
        );
    }

    pub fn create_notebook(
        &mut self,
        client_id: ClientId,
        owner: Owner,
        initial_folder_id: Option<SyncId>,
        model: CloudNotebookModel,
        force_expand: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        self.create_object(
            model,
            owner,
            client_id,
            force_expand,
            initial_folder_id,
            // When adding the initiated_by parameter to this function call, InitiatedBy::User was set as a default value.
            // This can be changed to InitiatedBy::System if this action was automatically kicked off by the system and we do not want a user facing toast.
            InitiatedBy::User,
            ctx,
        );
    }

    fn get_next_duplicate_object_name(
        &self,
        original_cloud_object: &dyn CloudObject,
        cloud_model: &CloudModel,
    ) -> String {
        let original_name = original_cloud_object.display_name();

        // Iterate through items in the same folder as the original object that are of the
        // same type, and populate a hashset with those names.
        let same_type_and_folder_names = cloud_model
            .active_cloud_objects_in_location_without_descendents(
                original_cloud_object.location(cloud_model),
            )
            .filter(|&object| object.object_type() == original_cloud_object.object_type())
            .map(|object| object.display_name())
            .collect::<HashSet<String>>();

        // Start with "{original_object_name} ({original_object_name's count + 1})".
        // Keep incrementing by one if there already exists an object of the same type in
        // the same folder (using the hashset generated above).
        let mut duplicate_name = get_duplicate_object_name(&original_name);
        while same_type_and_folder_names.contains(&duplicate_name) {
            duplicate_name = get_duplicate_object_name(&duplicate_name);
        }
        duplicate_name
    }

    pub fn create_workflow(
        &mut self,
        workflow: Workflow,
        owner: Owner,
        initial_folder_id: Option<SyncId>,
        client_id: ClientId,
        force_expand: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        self.create_object(
            CloudWorkflowModel::new(workflow),
            owner,
            client_id,
            force_expand,
            initial_folder_id,
            // When adding the initiated_by parameter to this function call, InitiatedBy::User was set as a default value.
            // This can be changed to InitiatedBy::System if this action was automatically kicked off by the system and we do not want a user facing toast.
            InitiatedBy::User,
            ctx,
        );
    }

    pub fn create_workflow_enum(
        &mut self,
        workflow_enum: WorkflowEnum,
        owner: Owner,
        client_id: ClientId,
        force_expand: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        self.create_object(
            CloudWorkflowEnumModel::new(workflow_enum),
            owner,
            client_id,
            force_expand,
            None,
            // When adding the initiated_by parameter to this function call, InitiatedBy::User was set as a default value.
            // This can be changed to InitiatedBy::System if this action was automatically kicked off by the system and we do not want a user facing toast.
            InitiatedBy::User,
            ctx,
        );
    }

    pub fn create_env_var_collection(
        &mut self,
        client_id: ClientId,
        owner: Owner,
        initial_folder_id: Option<SyncId>,
        model: CloudEnvVarCollectionModel,
        force_expand: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        self.create_object(
            model,
            owner,
            client_id,
            force_expand,
            initial_folder_id,
            // When adding the initiated_by parameter to this function call, InitiatedBy::User was set as a default value.
            // This can be changed to InitiatedBy::System if this action was automatically kicked off by the system and we do not want a user facing toast.
            InitiatedBy::User,
            ctx,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_folder(
        &mut self,
        name: String,
        owner: Owner,
        client_id: ClientId,
        initial_folder_id: Option<SyncId>,
        force_expand: bool,
        initiated_by: InitiatedBy,
        ctx: &mut ModelContext<Self>,
    ) {
        self.create_object(
            // TODO(INT-789): support creating folders as warp packs
            CloudFolderModel::new(&name, false),
            owner,
            client_id,
            force_expand,
            initial_folder_id,
            initiated_by,
            ctx,
        );
    }

    /// Generic function for creating a new cloud object with a given model. The object is keyed
    /// by `client_id` for good: there is no server to assign it another id.
    #[allow(clippy::too_many_arguments)]
    pub fn create_object<K, M>(
        &mut self,
        model: M,
        owner: Owner,
        client_id: ClientId,
        force_expand: bool,
        initial_folder_id: Option<SyncId>,
        initiated_by: InitiatedBy,
        ctx: &mut ModelContext<Self>,
    ) where
        K: HashableId
            + ToServerId
            + std::fmt::Debug
            + Into<String>
            + Clone
            + Copy
            + Send
            + Sync
            + 'static,
        M: CloudModelType<IdType = K, CloudObjectType = GenericCloudObject<K, M>> + 'static,
    {
        let object_id = SyncId::ClientId(client_id);

        // Update in-memory model.
        CloudModel::handle(ctx).update(ctx, |cloud_model, ctx| {
            let object =
                GenericCloudObject::<K, M>::new_local(model, owner, initial_folder_id, client_id);
            cloud_model.create_object(object_id, object, ctx);

            if force_expand {
                cloud_model.force_expand_object_and_ancestors(object_id, ctx);
            }
        });

        // Update sqlite.
        self.save_in_memory_object_to_sqlite(CloudModel::as_ref(ctx), &object_id.uid());

        Self::emit_success(
            ObjectOperation::Create { initiated_by },
            Some(object_id),
            None,
            ctx,
        );
    }

    /// Generic function for updating a cloud object with a new model.
    pub fn update_object<K, M>(&mut self, model: M, object_id: SyncId, ctx: &mut ModelContext<Self>)
    where
        K: HashableId
            + ToServerId
            + std::fmt::Debug
            + Into<String>
            + Clone
            + Copy
            + Send
            + Sync
            + 'static,
        M: CloudModelType<IdType = K, CloudObjectType = GenericCloudObject<K, M>> + 'static,
    {
        // Update in-memory model.
        let updated = CloudModel::handle(ctx).update(ctx, |cloud_model, ctx| {
            let Some(object) = cloud_model.get_object_of_type_mut::<K, M>(&object_id) else {
                return false;
            };
            object.metadata.revision = Some(Revision::now());
            object.metadata.pending_changes_statuses.content_sync_status =
                CloudObjectSyncStatus::NoLocalChanges;
            cloud_model.update_object_from_edit(model, object_id, ctx);
            true
        });
        if !updated {
            log::warn!("Tried to update unknown object {object_id:?}");
            return;
        }

        // Update sqlite.
        self.save_in_memory_object_to_sqlite(CloudModel::as_ref(ctx), &object_id.uid());

        Self::emit_success(ObjectOperation::Update, Some(object_id), None, ctx);
    }

    // Takes a generic SyncId and records the action.
    pub fn record_object_action(
        &mut self,
        id_and_type: CloudObjectTypeAndId,
        action_type: ObjectActionType,
        data: Option<String>,
        ctx: &mut ModelContext<Self>,
    ) {
        // Take the action timestamp from the client.
        let action_timestamp = Utc::now();

        // Update in-memory model.
        let object_action = ObjectActions::handle(ctx).update(ctx, |object_actions_model, ctx| {
            object_actions_model.insert_action(
                id_and_type.uid(),
                id_and_type.sqlite_uid_hash(),
                action_type,
                data,
                action_timestamp,
                ctx,
            )
        });

        // Update sqlite.
        self.save_to_db([ModelEvent::InsertObjectAction { object_action }]);
    }

    /// The local user can always edit a notebook. Editors recorded on notebooks synced before
    /// the fork are cleared, so the notebook no longer looks like someone else is editing it.
    /// When taking access from such an editor (`optimistically_grant_access` false), reports
    /// success so the notebook switches into edit mode.
    pub fn grab_notebook_edit_access(
        &mut self,
        notebook_id: SyncId,
        optimistically_grant_access: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        let cleared_editor = CloudModel::handle(ctx).update(ctx, |cloud_model, ctx| {
            let notebook = cloud_model.get_notebook_mut(&notebook_id)?;
            notebook.metadata.current_editor_uid.take()?;
            ctx.notify();
            Some(())
        });
        if cleared_editor.is_some() {
            self.save_in_memory_object_to_sqlite(CloudModel::as_ref(ctx), &notebook_id.uid());
        }

        if !optimistically_grant_access {
            Self::emit_success(
                ObjectOperation::TakeEditAccess,
                Some(notebook_id),
                None,
                ctx,
            );
        }
    }

    pub fn trash_object(&mut self, id: CloudObjectTypeAndId, ctx: &mut ModelContext<Self>) {
        let uid = id.uid();
        let trashed = CloudModel::handle(ctx).update(ctx, |cloud_model, ctx| {
            let Some(object) = cloud_model.get_mut_by_uid(&uid) else {
                return false;
            };
            let timestamp = ServerTimestamp::new(Utc::now());
            object.metadata_mut().trashed_ts = Some(timestamp);
            object.metadata_mut().metadata_last_updated_ts = Some(timestamp);
            ctx.emit(CloudModelEvent::ObjectTrashed {
                type_and_id: object.cloud_object_type_and_id(),
                source: UpdateSource::Local,
            });
            ctx.notify();
            true
        });
        if !trashed {
            return;
        }

        self.save_in_memory_object_to_sqlite(CloudModel::as_ref(ctx), &uid);
        Self::emit_success(ObjectOperation::Trash, Some(id.sync_id()), None, ctx);
    }

    /// Restores a trashed object. If its folder is gone or still in the trash, the object is
    /// restored to the root of its drive.
    pub fn untrash_object(&mut self, id: CloudObjectTypeAndId, ctx: &mut ModelContext<Self>) {
        let uid = id.uid();
        let restored = CloudModel::handle(ctx).update(ctx, |cloud_model, ctx| {
            let folder_id = cloud_model.get_by_uid(&uid)?.metadata().folder_id;
            let folder_unavailable = folder_id.is_some_and(|folder_id| {
                cloud_model
                    .get_folder(&folder_id)
                    .is_none_or(|folder| folder.is_trashed(cloud_model))
            });

            let object = cloud_model.get_mut_by_uid(&uid)?;
            object.metadata_mut().trashed_ts = None;
            object.metadata_mut().metadata_last_updated_ts = Some(ServerTimestamp::new(Utc::now()));
            ctx.emit(CloudModelEvent::ObjectUntrashed {
                type_and_id: object.cloud_object_type_and_id(),
                source: UpdateSource::Local,
            });
            if folder_unavailable {
                cloud_model.update_object_location(&uid, None, None, ctx);
            }
            ctx.notify();
            Some(())
        });
        if restored.is_none() {
            return;
        }

        self.save_in_memory_object_to_sqlite(CloudModel::as_ref(ctx), &uid);
        Self::emit_success(ObjectOperation::Untrash, Some(id.sync_id()), None, ctx);
    }

    pub fn delete_object_by_user(
        &mut self,
        id: CloudObjectTypeAndId,
        ctx: &mut ModelContext<Self>,
    ) {
        self.delete_object_with_initiated_by(id, InitiatedBy::User, ctx);
    }

    /// Deletes an object, and everything in it if it is a folder, for good.
    pub fn delete_object_with_initiated_by(
        &mut self,
        id: CloudObjectTypeAndId,
        initiated_by: InitiatedBy,
        ctx: &mut ModelContext<Self>,
    ) {
        let deleted = self.delete_objects_and_descendants([id.uid()], ctx);
        if deleted == 0 {
            return;
        }

        Self::emit_success(
            ObjectOperation::Delete { initiated_by },
            Some(id.sync_id()),
            Some(deleted),
            ctx,
        );
    }

    /// Deletes every object the user trashed in `space`, with the contents of trashed folders.
    pub fn empty_trash(&mut self, space: Space, ctx: &mut ModelContext<Self>) {
        let trashed_uids: Vec<ObjectUid> = CloudModel::as_ref(ctx)
            .directly_trashed_cloud_objects_in_space(space)
            .map(|object| object.uid())
            .collect();
        let deleted = self.delete_objects_and_descendants(trashed_uids, ctx);

        let success_type = if deleted == 0 {
            // Shows a toast that there are no objects in the Trash.
            OperationSuccessType::Rejection
        } else {
            OperationSuccessType::Success
        };
        ctx.emit(UpdateManagerEvent::ObjectOperationComplete {
            result: ObjectOperationResult {
                success_type,
                operation: ObjectOperation::EmptyTrash,
                id: None,
                num_objects: Some(deleted),
            },
        });
    }

    /// Removes the objects, the descendants of any folders among them and all of their actions,
    /// in memory and in SQLite. Returns the number of objects deleted.
    fn delete_objects_and_descendants(
        &mut self,
        uids: impl IntoIterator<Item = ObjectUid>,
        ctx: &mut ModelContext<Self>,
    ) -> i32 {
        let deleted: Vec<(SyncId, ObjectIdType)> =
            CloudModel::handle(ctx).update(ctx, |cloud_model, ctx| {
                uids.into_iter()
                    .flat_map(|uid| cloud_model.delete_object_and_descendants(uid, ctx))
                    .collect()
            });
        if deleted.is_empty() {
            return 0;
        }

        ObjectActions::handle(ctx).update(ctx, |object_actions, ctx| {
            for (sync_id, _) in &deleted {
                object_actions.delete_actions_for_object(&sync_id.uid(), ctx);
            }
        });

        let num_deleted = deleted.len() as i32;
        // Deleting the objects from SQLite also deletes their actions.
        self.save_to_db([ModelEvent::DeleteObjects { ids: deleted }]);
        num_deleted
    }

    pub fn rename_folder(
        &mut self,
        folder_id: SyncId,
        new_name: String,
        ctx: &mut ModelContext<Self>,
    ) {
        let cloud_model = CloudModel::as_ref(ctx);
        if let Some(folder) = cloud_model.get_folder(&folder_id) {
            let new_folder = CloudFolderModel {
                name: new_name,
                is_open: folder.model().is_open,
                is_warp_pack: folder.model().is_warp_pack,
            };
            self.update_object(new_folder, folder_id, ctx);
        } else {
            log::warn!("Attempted to rename folder that doesn't exist with id: {folder_id:?}");
        }
    }
}

/// Return the newly duplicated object's name based on the original object's name. E.g.:
/// - "my object name" -> "my object name (1)"
pub fn get_duplicate_object_name(original_name: &str) -> String {
    match DUPLICATE_OBJECT_NAME_REGEX
        .captures(original_name)
        .and_then(|caps| caps.get(1))
        .and_then(|num| num.as_str().parse::<usize>().ok())
    {
        Some(num) => {
            let new_num = num.saturating_add(1);

            // edge case check for when the duplicate number is usize::MAX
            if new_num == usize::MAX {
                format!("{original_name} (1)")
            } else {
                DUPLICATE_OBJECT_NAME_REGEX
                    .replace(original_name, format!(" ({new_num})"))
                    .to_string()
            }
        }
        None => format!("{original_name} (1)"),
    }
}

impl Entity for UpdateManager {
    type Event = UpdateManagerEvent;
}

impl SingletonEntity for UpdateManager {}

#[cfg(test)]
#[path = "update_manager_tests.rs"]
mod tests;
