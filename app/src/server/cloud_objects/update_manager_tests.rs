use chrono::Utc;
use warpui::{App, SingletonEntity};

use super::{get_duplicate_object_name, InitiatedBy, ObjectOperation, ObjectOperationResult};
use crate::cloud_object::model::actions::{ObjectActionType, ObjectActions};
use crate::cloud_object::model::persistence::{CloudModel, CloudModelEvent, UpdateSource};
use crate::cloud_object::{
    CloudObject, CloudObjectLocation, CloudObjectMetadata, CloudObjectPermissions, ObjectIdType,
    Owner, Space,
};
use crate::drive::folders::{CloudFolder, CloudFolderModel};
use crate::drive::CloudObjectTypeAndId;
use crate::notebooks::{CloudNotebook, CloudNotebookModel};
use crate::persistence::ModelEvent;
use crate::server::cloud_objects::test_utils::{
    create_update_manager_struct, initialize_app, UpdateManagerStruct,
};
use crate::server::ids::{ClientId, ServerId, SyncId};
use crate::workflows::workflow::Workflow;
use crate::workflows::{CloudWorkflow, CloudWorkflowModel};
use crate::ASSETS;

fn db_events(harness: &UpdateManagerStruct) -> Vec<ModelEvent> {
    harness.receiver.try_iter().collect()
}

fn cloud_events(harness: &UpdateManagerStruct) -> Vec<CloudModelEvent> {
    let mut events = Vec::new();
    while let Ok(event) = harness.cloud_model_events.try_recv() {
        events.push(event);
    }
    events
}

fn operation_results(harness: &UpdateManagerStruct) -> Vec<ObjectOperationResult> {
    let mut results = Vec::new();
    while let Ok(result) = harness.operation_results.try_recv() {
        results.push(result);
    }
    results
}

/// The metadata written by an upsert event.
fn upserted_metadata(event: &ModelEvent) -> CloudObjectMetadata {
    match event {
        ModelEvent::UpsertWorkflow { workflow } => workflow.metadata.clone(),
        ModelEvent::UpsertFolder { folder } => folder.metadata.clone(),
        ModelEvent::UpsertNotebook { notebook } => notebook.metadata.clone(),
        other => panic!("expected an upsert event, got {other:?}"),
    }
}

fn assert_success(
    result: &ObjectOperationResult,
    operation: ObjectOperation,
    id: Option<SyncId>,
    num_objects: Option<i32>,
) {
    assert_eq!(result.operation, operation);
    assert_eq!(result.id, id);
    assert_eq!(result.num_objects, num_objects);
}

fn create_workflow(
    app: &mut App,
    harness: &UpdateManagerStruct,
    name: &str,
    folder_id: Option<SyncId>,
) -> SyncId {
    let client_id = ClientId::new();
    harness.update_manager.update(app, |update_manager, ctx| {
        update_manager.create_workflow(
            Workflow::new(name, "echo hi"),
            Owner::local_user(),
            folder_id,
            client_id,
            false,
            ctx,
        );
    });
    SyncId::ClientId(client_id)
}

fn create_folder(
    app: &mut App,
    harness: &UpdateManagerStruct,
    name: &str,
    folder_id: Option<SyncId>,
) -> SyncId {
    let client_id = ClientId::new();
    harness.update_manager.update(app, |update_manager, ctx| {
        update_manager.create_folder(
            name.to_string(),
            Owner::local_user(),
            client_id,
            folder_id,
            false,
            InitiatedBy::User,
            ctx,
        );
    });
    SyncId::ClientId(client_id)
}

/// Adds a workflow synced before the fork, keyed by its server id.
fn add_server_workflow(app: &mut App, id: i64) -> SyncId {
    let sync_id = SyncId::ServerId(ServerId::from(id));
    CloudModel::handle(app).update(app, |cloud_model, _| {
        cloud_model.add_object(
            sync_id,
            CloudWorkflow::new(
                sync_id,
                CloudWorkflowModel::new(Workflow::new("legacy", "ls")),
                CloudObjectMetadata::mock(),
                CloudObjectPermissions::mock_personal(),
            ),
        );
    });
    sync_id
}

fn metadata(app: &App, id: SyncId) -> CloudObjectMetadata {
    CloudModel::handle(app).read(app, |cloud_model, _| {
        cloud_model
            .get_by_uid(&id.uid())
            .expect("object should exist")
            .metadata()
            .clone()
    })
}

fn exists(app: &App, id: SyncId) -> bool {
    CloudModel::handle(app).read(app, |cloud_model, _| {
        cloud_model.get_by_uid(&id.uid()).is_some()
    })
}

fn is_trashed(app: &App, id: SyncId) -> bool {
    CloudModel::handle(app).read(app, |cloud_model, _| {
        cloud_model
            .get_by_uid(&id.uid())
            .expect("object should exist")
            .is_trashed(cloud_model)
    })
}

fn workflow(app: &App, id: SyncId) -> CloudWorkflow {
    CloudModel::handle(app).read(app, |cloud_model, _| {
        cloud_model
            .get_workflow(&id)
            .expect("workflow should exist")
            .clone()
    })
}

fn workflow_ref(id: SyncId) -> CloudObjectTypeAndId {
    CloudObjectTypeAndId::Workflow(id)
}

fn folder_ref(id: SyncId) -> CloudObjectTypeAndId {
    CloudObjectTypeAndId::Folder(id)
}

#[test]
fn test_get_duplicate_object_name() {
    assert_eq!(
        get_duplicate_object_name("my object name"),
        "my object name (1)"
    );
    assert_eq!(
        get_duplicate_object_name("my object name (1)"),
        "my object name (2)"
    );
    assert_eq!(
        get_duplicate_object_name("my object name (23)"),
        "my object name (24)"
    );
    assert_eq!(
        get_duplicate_object_name("my object name(1234)"),
        "my object name(1234) (1)"
    );
    assert_eq!(
        get_duplicate_object_name("my object name (0)"),
        "my object name (1)"
    );
    assert_eq!(
        get_duplicate_object_name("my object name (-3)"),
        "my object name (-3) (1)"
    );
    assert_eq!(
        get_duplicate_object_name("my object name (18446744073709551615)"),
        "my object name (18446744073709551615) (1)"
    );
    assert_eq!(
        get_duplicate_object_name("my object name (18446744073709551616)"),
        "my object name (18446744073709551616) (1)"
    );
}

#[test]
fn test_create_objects_are_saved_locally() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let harness = create_update_manager_struct(&mut app);

        let workflow_id = create_workflow(&mut app, &harness, "workflow", None);
        let folder_id = create_folder(&mut app, &harness, "folder", None);
        let notebook_client_id = ClientId::new();
        harness
            .update_manager
            .update(&mut app, |update_manager, ctx| {
                update_manager.create_notebook(
                    notebook_client_id,
                    Owner::local_user(),
                    Some(folder_id),
                    CloudNotebookModel::default(),
                    false,
                    ctx,
                );
            });
        let notebook_id = SyncId::ClientId(notebook_client_id);

        for id in [workflow_id, folder_id, notebook_id] {
            let metadata = metadata(&app, id);
            assert!(metadata.revision.is_some());
        }
        assert_eq!(metadata(&app, notebook_id).folder_id, Some(folder_id));

        let events = db_events(&harness);
        assert_eq!(events.len(), 3, "one upsert per object: {events:?}");
        assert!(matches!(events[0], ModelEvent::UpsertWorkflow { .. }));
        assert!(matches!(events[1], ModelEvent::UpsertFolder { .. }));
        assert!(matches!(events[2], ModelEvent::UpsertNotebook { .. }));

        let results = operation_results(&harness);
        assert_eq!(results.len(), 3);
        for (result, id) in results.iter().zip([workflow_id, folder_id, notebook_id]) {
            assert_success(
                result,
                ObjectOperation::Create {
                    initiated_by: InitiatedBy::User,
                },
                Some(id),
                None,
            );
        }
    })
}

#[test]
fn test_update_leaves_object_saved() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let harness = create_update_manager_struct(&mut app);
        let workflow_id = create_workflow(&mut app, &harness, "before", None);
        let created_revision = metadata(&app, workflow_id).revision;
        db_events(&harness);
        operation_results(&harness);

        harness
            .update_manager
            .update(&mut app, |update_manager, ctx| {
                update_manager.update_workflow(
                    Workflow::new("after", "echo bye"),
                    workflow_id,
                    ctx,
                );
            });

        let workflow = workflow(&app, workflow_id);
        assert_eq!(workflow.display_name(), "after");
        assert!(workflow.metadata.revision >= created_revision);

        let events = db_events(&harness);
        assert_eq!(events.len(), 1);

        let results = operation_results(&harness);
        assert_eq!(results.len(), 1);
        assert_success(
            &results[0],
            ObjectOperation::Update,
            Some(workflow_id),
            None,
        );
    })
}

#[test]
fn test_rename_folder() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let harness = create_update_manager_struct(&mut app);
        let folder_id = create_folder(&mut app, &harness, "old name", None);
        db_events(&harness);
        operation_results(&harness);

        harness
            .update_manager
            .update(&mut app, |update_manager, ctx| {
                update_manager.rename_folder(folder_id, "new name".to_string(), ctx);
            });

        let folder = CloudModel::handle(&app).read(&app, |cloud_model, _| {
            cloud_model
                .get_folder(&folder_id)
                .expect("folder should exist")
                .clone()
        });
        assert_eq!(folder.model().name, "new name");

        let events = db_events(&harness);
        assert_eq!(events.len(), 1);
        let ModelEvent::UpsertFolder { folder } = &events[0] else {
            panic!("expected a folder upsert, got {:?}", events[0]);
        };
        assert_eq!(folder.model().name, "new name");
        assert_success(
            &operation_results(&harness)[0],
            ObjectOperation::Update,
            Some(folder_id),
            None,
        );
    })
}

/// Trashes the workflow and checks the trash state is written without any pending flag, so
/// it survives a restart.
fn trash_and_assert_persisted(app: &mut App, harness: &UpdateManagerStruct, workflow_id: SyncId) {
    db_events(harness);
    cloud_events(harness);
    operation_results(harness);

    harness.update_manager.update(app, |update_manager, ctx| {
        update_manager.trash_object(workflow_ref(workflow_id), ctx);
    });

    let metadata = metadata(app, workflow_id);
    assert!(metadata.trashed_ts.is_some());
    assert_eq!(metadata.metadata_last_updated_ts, metadata.trashed_ts);
    assert!(is_trashed(app, workflow_id));

    let events = db_events(harness);
    assert_eq!(events.len(), 1);
    let upserted = upserted_metadata(&events[0]);
    assert_eq!(upserted.trashed_ts, metadata.trashed_ts);

    assert!(
        cloud_events(harness).contains(&CloudModelEvent::ObjectTrashed {
            type_and_id: workflow_ref(workflow_id),
            source: UpdateSource::Local,
        })
    );
    let results = operation_results(harness);
    assert_eq!(results.len(), 1);
    assert_success(&results[0], ObjectOperation::Trash, Some(workflow_id), None);
}

#[test]
fn test_trash_local_object() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let harness = create_update_manager_struct(&mut app);
        let workflow_id = create_workflow(&mut app, &harness, "workflow", None);

        trash_and_assert_persisted(&mut app, &harness, workflow_id);
    })
}

#[test]
fn test_trash_object_synced_before_the_fork() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let harness = create_update_manager_struct(&mut app);
        let workflow_id = add_server_workflow(&mut app, 42);

        trash_and_assert_persisted(&mut app, &harness, workflow_id);
    })
}

#[test]
fn test_untrash_object() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let harness = create_update_manager_struct(&mut app);
        let folder_id = create_folder(&mut app, &harness, "folder", None);
        let workflow_id = create_workflow(&mut app, &harness, "workflow", Some(folder_id));
        harness
            .update_manager
            .update(&mut app, |update_manager, ctx| {
                update_manager.trash_object(workflow_ref(workflow_id), ctx);
            });
        db_events(&harness);
        cloud_events(&harness);
        operation_results(&harness);

        harness
            .update_manager
            .update(&mut app, |update_manager, ctx| {
                update_manager.untrash_object(workflow_ref(workflow_id), ctx);
            });

        // The folder is still there, so the workflow goes back into it.
        let metadata = metadata(&app, workflow_id);
        assert!(metadata.trashed_ts.is_none());
        assert_eq!(metadata.folder_id, Some(folder_id));

        let events = db_events(&harness);
        assert_eq!(events.len(), 1);
        let upserted = upserted_metadata(&events[0]);
        assert!(upserted.trashed_ts.is_none());
        assert_eq!(upserted.folder_id, Some(folder_id));

        assert!(
            cloud_events(&harness).contains(&CloudModelEvent::ObjectUntrashed {
                type_and_id: workflow_ref(workflow_id),
                source: UpdateSource::Local,
            })
        );
        assert_success(
            &operation_results(&harness)[0],
            ObjectOperation::Untrash,
            Some(workflow_id),
            None,
        );
    })
}

#[test]
fn test_untrash_child_of_trashed_folder_moves_it_to_the_root() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let harness = create_update_manager_struct(&mut app);
        let folder_id = create_folder(&mut app, &harness, "folder", None);
        let workflow_id = create_workflow(&mut app, &harness, "workflow", Some(folder_id));
        harness
            .update_manager
            .update(&mut app, |update_manager, ctx| {
                update_manager.trash_object(folder_ref(folder_id), ctx);
            });
        db_events(&harness);
        cloud_events(&harness);

        harness
            .update_manager
            .update(&mut app, |update_manager, ctx| {
                update_manager.untrash_object(workflow_ref(workflow_id), ctx);
            });

        assert_eq!(metadata(&app, workflow_id).folder_id, None);
        assert!(!is_trashed(&app, workflow_id));
        // The folder stays in the trash.
        assert!(is_trashed(&app, folder_id));

        let events = db_events(&harness);
        assert_eq!(events.len(), 1);
        assert_eq!(upserted_metadata(&events[0]).folder_id, None);
        assert!(
            cloud_events(&harness).contains(&CloudModelEvent::ObjectMoved {
                type_and_id: workflow_ref(workflow_id),
                source: UpdateSource::Local,
                from_folder: Some(folder_id),
                to_folder: None,
            })
        );
    })
}

#[test]
fn test_delete_folder_with_children() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let harness = create_update_manager_struct(&mut app);
        let folder_id = create_folder(&mut app, &harness, "folder", None);
        let subfolder_id = create_folder(&mut app, &harness, "subfolder", Some(folder_id));
        let workflow_id = create_workflow(&mut app, &harness, "workflow", Some(subfolder_id));
        let other_workflow_id = create_workflow(&mut app, &harness, "other", None);
        harness
            .update_manager
            .update(&mut app, |update_manager, ctx| {
                update_manager.record_object_action(
                    workflow_ref(workflow_id),
                    ObjectActionType::Execute,
                    None,
                    ctx,
                );
            });
        db_events(&harness);
        operation_results(&harness);

        harness
            .update_manager
            .update(&mut app, |update_manager, ctx| {
                update_manager.delete_object_by_user(folder_ref(folder_id), ctx);
            });

        for id in [folder_id, subfolder_id, workflow_id] {
            assert!(!exists(&app, id));
        }
        assert!(exists(&app, other_workflow_id));
        ObjectActions::handle(&app).update(&mut app, |object_actions, _| {
            assert_eq!(
                object_actions.count_actions_for_object(&workflow_id.uid()),
                0
            );
        });

        let events = db_events(&harness);
        assert_eq!(
            events.len(),
            1,
            "one delete for the whole folder: {events:?}"
        );
        let ModelEvent::DeleteObjects { ids } = &events[0] else {
            panic!("expected a delete event, got {:?}", events[0]);
        };
        let mut ids = ids.clone();
        ids.sort_by_key(|(id, _)| id.uid());
        let mut expected = vec![
            (folder_id, ObjectIdType::Folder),
            (subfolder_id, ObjectIdType::Folder),
            (workflow_id, ObjectIdType::Workflow),
        ];
        expected.sort_by_key(|(id, _)| id.uid());
        assert_eq!(ids, expected);

        let results = operation_results(&harness);
        assert_eq!(results.len(), 1);
        assert_success(
            &results[0],
            ObjectOperation::Delete {
                initiated_by: InitiatedBy::User,
            },
            Some(folder_id),
            Some(3),
        );
    })
}

#[test]
fn test_delete_missing_object_does_nothing() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let harness = create_update_manager_struct(&mut app);

        harness
            .update_manager
            .update(&mut app, |update_manager, ctx| {
                update_manager
                    .delete_object_by_user(workflow_ref(SyncId::ClientId(ClientId::new())), ctx);
            });

        assert!(db_events(&harness).is_empty());
        assert!(operation_results(&harness).is_empty());
    })
}

#[test]
fn test_empty_trash() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let harness = create_update_manager_struct(&mut app);
        let folder_id = create_folder(&mut app, &harness, "folder", None);
        let child_id = create_workflow(&mut app, &harness, "child", Some(folder_id));
        let trashed_id = create_workflow(&mut app, &harness, "trashed", None);
        let active_id = create_workflow(&mut app, &harness, "active", None);
        harness
            .update_manager
            .update(&mut app, |update_manager, ctx| {
                update_manager.trash_object(folder_ref(folder_id), ctx);
                update_manager.trash_object(workflow_ref(trashed_id), ctx);
            });
        db_events(&harness);
        operation_results(&harness);

        harness
            .update_manager
            .update(&mut app, |update_manager, ctx| {
                update_manager.empty_trash(Space::Personal, ctx);
            });

        for id in [folder_id, child_id, trashed_id] {
            assert!(!exists(&app, id));
        }
        assert!(exists(&app, active_id));

        let events = db_events(&harness);
        assert_eq!(events.len(), 1);
        let ModelEvent::DeleteObjects { ids } = &events[0] else {
            panic!("expected a delete event, got {:?}", events[0]);
        };
        assert_eq!(ids.len(), 3);
        let results = operation_results(&harness);
        assert_eq!(results.len(), 1);
        assert_success(&results[0], ObjectOperation::EmptyTrash, None, Some(3));

        // With nothing left in the trash, emptying it reports zero objects and deletes nothing.
        harness
            .update_manager
            .update(&mut app, |update_manager, ctx| {
                update_manager.empty_trash(Space::Personal, ctx);
            });
        assert!(db_events(&harness).is_empty());
        let results = operation_results(&harness);
        assert_eq!(results.len(), 1);
        assert_success(&results[0], ObjectOperation::EmptyTrash, None, Some(0));
        assert!(exists(&app, active_id));
    })
}

#[test]
fn test_move_object_into_folder_and_back_to_root() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let harness = create_update_manager_struct(&mut app);
        let folder_id = create_folder(&mut app, &harness, "folder", None);
        let workflow_id = create_workflow(&mut app, &harness, "workflow", None);
        db_events(&harness);
        cloud_events(&harness);
        operation_results(&harness);

        harness
            .update_manager
            .update(&mut app, |update_manager, ctx| {
                update_manager.move_object_to_location(
                    workflow_ref(workflow_id),
                    CloudObjectLocation::Folder(folder_id),
                    ctx,
                );
            });
        let moved = metadata(&app, workflow_id);
        assert_eq!(moved.folder_id, Some(folder_id));
        let events = db_events(&harness);
        assert_eq!(events.len(), 1);
        assert_eq!(upserted_metadata(&events[0]).folder_id, Some(folder_id));
        assert!(
            cloud_events(&harness).contains(&CloudModelEvent::ObjectMoved {
                type_and_id: workflow_ref(workflow_id),
                source: UpdateSource::Local,
                from_folder: None,
                to_folder: Some(folder_id),
            })
        );

        harness
            .update_manager
            .update(&mut app, |update_manager, ctx| {
                update_manager.move_object_to_location(
                    workflow_ref(workflow_id),
                    CloudObjectLocation::Space(Space::Personal),
                    ctx,
                );
            });
        assert_eq!(metadata(&app, workflow_id).folder_id, None);
        let events = db_events(&harness);
        assert_eq!(events.len(), 1);
        assert_eq!(upserted_metadata(&events[0]).folder_id, None);
        // Moves don't report completion, so they show no toast.
        assert!(operation_results(&harness).is_empty());
    })
}

#[test]
fn test_move_to_trash_trashes_the_object() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let harness = create_update_manager_struct(&mut app);
        let workflow_id = create_workflow(&mut app, &harness, "workflow", None);

        harness
            .update_manager
            .update(&mut app, |update_manager, ctx| {
                update_manager.move_object_to_location(
                    workflow_ref(workflow_id),
                    CloudObjectLocation::Trash,
                    ctx,
                );
            });

        assert!(metadata(&app, workflow_id).trashed_ts.is_some());
    })
}

#[test]
fn test_duplicate_object() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let harness = create_update_manager_struct(&mut app);
        let folder_id = create_folder(&mut app, &harness, "folder", None);
        let workflow_id = create_workflow(&mut app, &harness, "workflow", Some(folder_id));
        db_events(&harness);
        operation_results(&harness);

        harness
            .update_manager
            .update(&mut app, |update_manager, ctx| {
                update_manager.duplicate_object(&workflow_ref(workflow_id), ctx);
            });

        let results = operation_results(&harness);
        assert_eq!(results.len(), 1);
        let duplicate_id = results[0].id.expect("duplicate should have an id");
        assert!(matches!(duplicate_id, SyncId::ClientId(_)));
        assert_ne!(duplicate_id, workflow_id);
        assert_success(
            &results[0],
            ObjectOperation::Create {
                initiated_by: InitiatedBy::User,
            },
            Some(duplicate_id),
            None,
        );

        let duplicate = workflow(&app, duplicate_id);
        assert_eq!(duplicate.display_name(), "workflow (1)");
        assert_eq!(duplicate.metadata.folder_id, Some(folder_id));
        assert_eq!(db_events(&harness).len(), 1);
    })
}

#[test]
fn test_record_object_action() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let harness = create_update_manager_struct(&mut app);
        let workflow_id = create_workflow(&mut app, &harness, "workflow", None);
        db_events(&harness);

        harness
            .update_manager
            .update(&mut app, |update_manager, ctx| {
                update_manager.record_object_action(
                    workflow_ref(workflow_id),
                    ObjectActionType::Execute,
                    None,
                    ctx,
                );
            });

        ObjectActions::handle(&app).update(&mut app, |object_actions, _| {
            assert_eq!(
                object_actions.count_actions_for_object(&workflow_id.uid()),
                1
            );
        });
        let events = db_events(&harness);
        assert_eq!(events.len(), 1);
        let ModelEvent::InsertObjectAction { object_action } = &events[0] else {
            panic!("expected an action insert, got {:?}", events[0]);
        };
        assert_eq!(object_action.uid, workflow_id.uid());
        assert_eq!(object_action.action_type, ObjectActionType::Execute);
    })
}

#[test]
fn test_taking_edit_access_clears_a_recorded_editor() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let harness = create_update_manager_struct(&mut app);
        let notebook_id = SyncId::ServerId(ServerId::from(7));
        let mut notebook_metadata = CloudObjectMetadata::mock();
        notebook_metadata.current_editor_uid = Some("someone_else".to_string());
        CloudModel::handle(&app).update(&mut app, |cloud_model, _| {
            cloud_model.add_object(
                notebook_id,
                CloudNotebook::new(
                    notebook_id,
                    CloudNotebookModel::default(),
                    notebook_metadata,
                    CloudObjectPermissions::mock_personal(),
                ),
            );
        });

        harness
            .update_manager
            .update(&mut app, |update_manager, ctx| {
                update_manager.grab_notebook_edit_access(notebook_id, false, ctx);
            });

        assert_eq!(metadata(&app, notebook_id).current_editor_uid, None);
        let events = db_events(&harness);
        assert_eq!(events.len(), 1);
        assert_eq!(upserted_metadata(&events[0]).current_editor_uid, None);
        let results = operation_results(&harness);
        assert_eq!(results.len(), 1);
        assert_success(
            &results[0],
            ObjectOperation::TakeEditAccess,
            Some(notebook_id),
            None,
        );

        // Opening a notebook with no recorded editor writes nothing and reports nothing.
        harness
            .update_manager
            .update(&mut app, |update_manager, ctx| {
                update_manager.grab_notebook_edit_access(notebook_id, true, ctx);
            });
        assert!(db_events(&harness).is_empty());
        assert!(operation_results(&harness).is_empty());
    })
}

#[test]
fn test_initial_load_is_complete_immediately() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let harness = create_update_manager_struct(&mut app);

        let initial_load_complete = harness.update_manager.read(&app, |update_manager, _| {
            update_manager.initial_load_complete()
        });
        futures::FutureExt::now_or_never(initial_load_complete)
            .expect("initial load should already be complete");
    })
}

#[test]
fn test_new_folder_is_complete_as_created() {
    let folder = CloudFolder::new_local(
        CloudFolderModel::new("folder", false),
        Owner::local_user(),
        None,
        ClientId::new(),
    );
    assert!(folder
        .metadata
        .revision
        .is_some_and(|revision| revision.utc() <= Utc::now()));
}
