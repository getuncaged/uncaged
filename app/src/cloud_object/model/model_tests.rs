use std::sync::Arc;

use chrono::{DateTime, Utc};
use cloud_object_models::{ServerNotebook, ServerWorkflow};
use cloud_objects::time::ServerTimestamp;
use cloud_objects::UserUid;
use warpui::{App, ModelHandle};

use super::*;
use crate::cloud_object::model::actions::ObjectActions;
use crate::cloud_object::model::view::{
    CloudViewModel, EditorState, UpdateTimestamp, EDITOR_TIMEOUT_DURATION_MINUTES,
};
use crate::cloud_object::{
    CloudObjectMetadata, CloudObjectPermissions, Owner, ServerMetadata, ServerPermissions,
    LOCAL_USER_UID,
};
use crate::drive::folders::CloudFolderModel;
use crate::drive::DriveIndexVariant;
use crate::features::FeatureFlag;
use crate::notebooks::CloudNotebookModel;
use crate::server::ids::{ClientId, ServerId};
use crate::server::server_api::ServerApiProvider;
use crate::server::shared_http_client::SharedHttpClient;
use crate::settings::init_and_register_user_preferences;
use crate::system::SystemStats;
use crate::workflows::workflow::Workflow;
use crate::workflows::CloudWorkflowModel;
use crate::workspaces::user_profiles::UserProfiles;
use crate::{NetworkStatus, UpdateManager};

fn create_cloud_model(
    app: &mut App,
    objects: Vec<Box<dyn CloudObject>>,
) -> ModelHandle<CloudModel> {
    // Make sure to register the CloudModel singleton - some CloudObject methods
    // find it and other dependencies via the AppContext.
    app.add_singleton_model(|_ctx| CloudModel::new(None, objects))
}

fn initialize_app(app: &mut App, cached_objects: Vec<Box<dyn CloudObject>>) {
    // Add the necessary singleton models to the App
    app.add_singleton_model(|_| NetworkStatus::new());
    app.add_singleton_model(|_| SystemStats::new());
    app.add_singleton_model(|_| ServerApiProvider::new_for_test());
    app.add_singleton_model(|_| SharedHttpClient::new_for_test());
    app.add_singleton_model(|_ctx| CloudModel::new(None, cached_objects));
    app.add_singleton_model(|_| UpdateManager::new(None));
    app.add_singleton_model(|_| UserProfiles::new(Vec::new()));
    app.add_singleton_model(CloudViewModel::new);
    app.add_singleton_model(|_| ObjectActions::new(Vec::new()));
}

fn mock_server_metadata() -> ServerMetadata {
    ServerMetadata {
        uid: ServerId::default(),
        revision: Revision::now(),
        metadata_last_updated_ts: Utc::now().into(),
        trashed_ts: None,
        folder_id: None,
        is_welcome_object: false,
        creator_uid: None,
        last_editor_uid: None,
        current_editor_uid: None,
    }
}

fn mock_server_permissions(owner: Owner) -> ServerPermissions {
    ServerPermissions {
        space: owner,
        guests: Vec::new(),
        permissions_last_updated_ts: Utc::now().into(),
        anyone_link_sharing: None,
    }
}

fn mock_permissions() -> CloudObjectPermissions {
    CloudObjectPermissions {
        owner: Owner::mock_current_user(),
        guests: Vec::new(),
        permissions_last_updated_ts: None,
        anyone_with_link: None,
    }
}

fn mock_server_workflows(
    start_id: i64,
    owner: Owner,
    number_of_workflows: i64,
) -> Vec<ServerWorkflow> {
    (0..number_of_workflows)
        .map(|idx| {
            ServerWorkflow::new(
                SyncId::ServerId((start_id + idx).into()),
                CloudWorkflowModel::new(Workflow::new(
                    format!("w{}", start_id + idx),
                    format!("c{}", start_id + idx),
                )),
                mock_server_metadata(),
                mock_server_permissions(owner),
            )
        })
        .collect()
}

fn mock_server_notebooks() -> Vec<ServerNotebook> {
    let owner = Owner::mock_current_user();
    vec![
        ServerNotebook::new(
            SyncId::ServerId(1.into()),
            CloudNotebookModel {
                title: "t1".to_string(),
                data: "d1".to_string(),
                ai_document_id: None,
                conversation_id: None,
            },
            mock_server_metadata(),
            mock_server_permissions(owner),
        ),
        ServerNotebook::new(
            SyncId::ServerId(2.into()),
            CloudNotebookModel {
                title: "t2".to_string(),
                data: "d2".to_string(),
                ai_document_id: None,
                conversation_id: None,
            },
            mock_server_metadata(),
            mock_server_permissions(owner),
        ),
        ServerNotebook::new(
            SyncId::ServerId(3.into()),
            CloudNotebookModel {
                title: "t3".to_string(),
                data: "d3".to_string(),
                ai_document_id: None,
                conversation_id: None,
            },
            mock_server_metadata(),
            mock_server_permissions(owner),
        ),
        ServerNotebook::new(
            SyncId::ServerId(4.into()),
            CloudNotebookModel {
                title: "t4".to_string(),
                data: "d4".to_string(),
                ai_document_id: None,
                conversation_id: None,
            },
            mock_server_metadata(),
            mock_server_permissions(owner),
        ),
    ]
}

fn mock_cloud_folder(id: SyncId, name: String, folder_id: Option<SyncId>) -> CloudFolder {
    CloudFolder::new(
        id,
        CloudFolderModel {
            name,
            is_open: true,
            is_warp_pack: false,
        },
        CloudObjectMetadata {
            folder_id,
            revision: Default::default(),
            metadata_last_updated_ts: Default::default(),
            current_editor_uid: Default::default(),
            trashed_ts: Default::default(),
            is_welcome_object: false,
            creator_uid: None,
            last_editor_uid: None,
            last_task_run_ts: None,
        },
        mock_permissions(),
    )
}

fn mock_cloud_notebook(id: SyncId, title: String, folder_id: Option<SyncId>) -> CloudNotebook {
    CloudNotebook::new(
        id,
        CloudNotebookModel {
            title,
            data: "test".into(),
            ai_document_id: None,
            conversation_id: None,
        },
        CloudObjectMetadata {
            folder_id,
            revision: Default::default(),
            metadata_last_updated_ts: Default::default(),
            current_editor_uid: Default::default(),
            trashed_ts: Default::default(),
            is_welcome_object: false,
            creator_uid: None,
            last_editor_uid: None,
            last_task_run_ts: None,
        },
        mock_permissions(),
    )
}

fn mock_trashed_cloud_folder(id: SyncId, name: String, folder_id: Option<SyncId>) -> CloudFolder {
    let mut folder = mock_cloud_folder(id, name, folder_id);
    folder.metadata.trashed_ts = Some(ServerTimestamp::from_unix_timestamp_micros(10).unwrap());
    folder
}

fn folder_from_cloud_model(model: &CloudModel, id: SyncId) -> &CloudFolder {
    model.get_folder_by_uid(&id.uid()).expect("is a folder")
}

fn notebook_revision(id: ServerId, app: &App) -> DateTime<Utc> {
    CloudModel::handle(app).read(app, |cloud_model, _| {
        cloud_model
            .get_by_uid(&id.uid())
            .and_then(|object| object.metadata().revision.clone())
            .expect("object should have a revision")
            .utc()
    })
}

fn move_object(id: ServerId, location: CloudObjectLocation, app: &mut App) {
    UpdateManager::handle(app).update(app, |update_manager, ctx| {
        update_manager.move_object_to_location(
            CloudObjectTypeAndId::Notebook(id.into()),
            location,
            ctx,
        )
    });
}

#[test]
fn test_collapse_all_in_location() {
    /*
       the folder structure looks like:

       test1
        ↳ test 4
         ↳ test 5
       test 2
        ↳ test 6
         ↳ test 7
       test 3

    */
    let folder_1_id: SyncId = SyncId::ServerId(1.into());
    let folder_2_id: SyncId = SyncId::ServerId(2.into());
    let folder_3_id: SyncId = SyncId::ServerId(3.into());
    let folder_4_id: SyncId = SyncId::ServerId(4.into());
    let folder_5_id: SyncId = SyncId::ServerId(5.into());
    let folder_6_id: SyncId = SyncId::ServerId(6.into());
    let folder_7_id: SyncId = SyncId::ServerId(7.into());

    let folders = vec![
        mock_cloud_folder(folder_1_id, "test1".to_string(), None),
        mock_cloud_folder(folder_2_id, "test2".to_string(), None),
        mock_cloud_folder(folder_3_id, "test3".to_string(), None),
        mock_cloud_folder(folder_4_id, "test4".to_string(), Some(folder_1_id)),
        mock_cloud_folder(folder_5_id, "test5".to_string(), Some(folder_4_id)),
        mock_cloud_folder(folder_6_id, "test6".to_string(), Some(folder_2_id)),
        mock_cloud_folder(folder_7_id, "test7".to_string(), Some(folder_6_id)),
    ]
    .into_iter()
    .map(|o| Box::new(o) as Box<dyn CloudObject>)
    .collect();

    App::test((), |mut app| async move {
        let cloud_model = create_cloud_model(&mut app, folders);

        cloud_model.update(&mut app, |model, ctx| {
            // first, collapse all folders in folder 1
            model.collapse_all_in_location(
                CloudObjectLocation::Folder(folder_1_id),
                DriveIndexVariant::MainIndex,
                ctx,
            );

            // folders 1, 4, and 5 should be collapsed
            let folder_1 = folder_from_cloud_model(model, folder_1_id);
            let folder_4 = folder_from_cloud_model(model, folder_4_id);
            let folder_5 = folder_from_cloud_model(model, folder_5_id);
            assert!(!folder_1.model().is_open);
            assert!(!folder_4.model().is_open);
            assert!(!folder_5.model().is_open);
            // but the others are still open
            let folder_2 = folder_from_cloud_model(model, folder_2_id);
            let folder_3 = folder_from_cloud_model(model, folder_3_id);
            let folder_6 = folder_from_cloud_model(model, folder_6_id);
            let folder_7 = folder_from_cloud_model(model, folder_7_id);
            assert!(folder_2.model().is_open);
            assert!(folder_3.model().is_open);
            assert!(folder_6.model().is_open);
            assert!(folder_7.model().is_open);

            model.collapse_all_in_location(
                CloudObjectLocation::Space(Default::default()),
                DriveIndexVariant::MainIndex,
                ctx,
            );
            // now all folders in this space are collapsed
            let folder_1 = folder_from_cloud_model(model, folder_1_id);
            let folder_2 = folder_from_cloud_model(model, folder_2_id);
            let folder_3 = folder_from_cloud_model(model, folder_3_id);
            let folder_4 = folder_from_cloud_model(model, folder_4_id);
            let folder_5 = folder_from_cloud_model(model, folder_5_id);
            let folder_6 = folder_from_cloud_model(model, folder_6_id);
            let folder_7 = folder_from_cloud_model(model, folder_7_id);
            assert!(!folder_1.model().is_open);
            assert!(!folder_2.model().is_open);
            assert!(!folder_3.model().is_open);
            assert!(!folder_4.model().is_open);
            assert!(!folder_5.model().is_open);
            assert!(!folder_6.model().is_open);
            assert!(!folder_7.model().is_open);
        });
    })
}

#[test]
fn test_collapse_all_in_trash() {
    /*
       the folder structure looks like:

       test1 -- trashed by user
        ↳ test 4
         ↳ test 5 -- trashed by user
       test 2 -- trashed by user
        ↳ test 6
         ↳ test 7
       test 3 -- trashed by user

       the structure in the trash index looks like:

       test1 -- trashed by user
        ↳ test 4
       test 5 -- trashed by user
       test 2 -- trashed by user
        ↳ test 6
         ↳ test 7
       test 3 -- trashed by user

    */
    let folder_1_id: SyncId = SyncId::ServerId(1.into());
    let folder_2_id: SyncId = SyncId::ServerId(2.into());
    let folder_3_id: SyncId = SyncId::ServerId(3.into());
    let folder_4_id: SyncId = SyncId::ServerId(4.into());
    let folder_5_id: SyncId = SyncId::ServerId(5.into());
    let folder_6_id: SyncId = SyncId::ServerId(6.into());
    let folder_7_id: SyncId = SyncId::ServerId(7.into());

    let folders = vec![
        mock_trashed_cloud_folder(folder_1_id, "test1".to_string(), None),
        mock_trashed_cloud_folder(folder_2_id, "test2".to_string(), None),
        mock_trashed_cloud_folder(folder_3_id, "test3".to_string(), None),
        mock_cloud_folder(folder_4_id, "test4".to_string(), Some(folder_1_id)),
        mock_trashed_cloud_folder(folder_5_id, "test5".to_string(), Some(folder_4_id)),
        mock_cloud_folder(folder_6_id, "test6".to_string(), Some(folder_2_id)),
        mock_cloud_folder(folder_7_id, "test7".to_string(), Some(folder_6_id)),
    ]
    .into_iter()
    .map(|o| Box::new(o) as Box<dyn CloudObject>)
    .collect();

    App::test((), |mut app| async move {
        let cloud_model = create_cloud_model(&mut app, folders);

        cloud_model.update(&mut app, |model, ctx| {
            // first, collapse all folders in folder 1
            model.collapse_all_in_location(
                CloudObjectLocation::Folder(folder_1_id),
                DriveIndexVariant::Trash,
                ctx,
            );

            // folders 1, 4 should be collapsed
            let folder_1 = folder_from_cloud_model(model, folder_1_id);
            let folder_4 = folder_from_cloud_model(model, folder_4_id);
            assert!(!folder_1.model().is_open);
            assert!(!folder_4.model().is_open);
            // but the others, including folder 5, are still open
            let folder_2 = folder_from_cloud_model(model, folder_2_id);
            let folder_3 = folder_from_cloud_model(model, folder_3_id);
            let folder_5 = folder_from_cloud_model(model, folder_5_id);
            let folder_6 = folder_from_cloud_model(model, folder_6_id);
            let folder_7 = folder_from_cloud_model(model, folder_7_id);
            assert!(folder_2.model().is_open);
            assert!(folder_3.model().is_open);
            assert!(folder_5.model().is_open);
            assert!(folder_6.model().is_open);
            assert!(folder_7.model().is_open);

            model.collapse_all_in_location(
                CloudObjectLocation::Space(Default::default()),
                DriveIndexVariant::Trash,
                ctx,
            );
            // now all folders in this space are collapsed
            let folder_1 = folder_from_cloud_model(model, folder_1_id);
            let folder_2 = folder_from_cloud_model(model, folder_2_id);
            let folder_3 = folder_from_cloud_model(model, folder_3_id);
            let folder_4 = folder_from_cloud_model(model, folder_4_id);
            let folder_5 = folder_from_cloud_model(model, folder_5_id);
            let folder_6 = folder_from_cloud_model(model, folder_6_id);
            let folder_7 = folder_from_cloud_model(model, folder_7_id);
            assert!(!folder_1.model().is_open);
            assert!(!folder_2.model().is_open);
            assert!(!folder_3.model().is_open);
            assert!(!folder_4.model().is_open);
            assert!(!folder_5.model().is_open);
            assert!(!folder_6.model().is_open);
            assert!(!folder_7.model().is_open);
        });
    })
}

#[test]
fn test_object_editor_timeout() {
    App::test((), |mut app| async move {
        // Setup the app and APIs
        initialize_app(&mut app, Vec::new());
        let notebook_id: SyncId = SyncId::ServerId(1.into());
        let cloud_notebook = mock_cloud_notebook(notebook_id, "test1".into(), None);

        CloudModel::handle(&app).update(&mut app, |model, _ctx| {
            // Add a notebook to CloudModel
            model.add_object(notebook_id, cloud_notebook.clone());

            let notebook = model
                .get_notebook_mut(&notebook_id)
                .expect("notebook should exist");

            // Set the editor to be somebody else.
            notebook.metadata.current_editor_uid = Some("ian@warp.dev".to_string());
        });

        let current_editor = CloudViewModel::handle(&app).read(&app, |view_model, ctx| {
            view_model
                .object_current_editor(&notebook_id.uid(), ctx)
                .expect("expect editor to be set")
        });
        // Assert that the current editor is an active other user
        assert_eq!(current_editor.state, EditorState::OtherUserActive);

        CloudModel::handle(&app).update(&mut app, |model, _ctx| {
            let notebook = model
                .get_notebook_mut(&notebook_id)
                .expect("notebook should exist");

            // Set the notebook timesteps to be more than the timeout
            let timeout_timestamp = Utc::now()
                - chrono::Duration::minutes(EDITOR_TIMEOUT_DURATION_MINUTES)
                - chrono::Duration::seconds(1);
            notebook.metadata.revision = Some(Revision::from(timeout_timestamp));
            notebook.metadata.metadata_last_updated_ts = Some(timeout_timestamp.into());
        });

        let current_editor = CloudViewModel::handle(&app).read(&app, |view_model, ctx| {
            view_model
                .object_current_editor(&notebook_id.uid(), ctx)
                .expect("expect editor to be set")
        });
        // Assert that the current editor is an idle other user
        assert_eq!(current_editor.state, EditorState::OtherUserIdle);
    });
}

#[test]
fn test_breadcrumbs() {
    let folder_1_id: SyncId = SyncId::ServerId(1.into());
    let folder_2_id: SyncId = SyncId::ServerId(2.into());
    let folder_3_id: SyncId = SyncId::ServerId(3.into());

    let folders = vec![
        mock_cloud_folder(folder_1_id, "test1".to_string(), None),
        mock_cloud_folder(folder_2_id, "test2".to_string(), Some(folder_1_id)),
        mock_cloud_folder(folder_3_id, "test3".to_string(), Some(folder_2_id)),
    ]
    .into_iter()
    .map(|f| Box::new(f) as Box<dyn CloudObject>)
    .collect::<Vec<_>>();

    App::test((), |mut app| async move {
        initialize_app(&mut app, folders.clone());

        CloudModel::handle(&app).read(&app, |_, ctx| {
            assert_eq!("Personal".to_string(), folders[0].breadcrumbs(ctx));
            assert_eq!("Personal / test1".to_string(), folders[1].breadcrumbs(ctx));
            assert_eq!(
                "Personal / test1 / test2".to_string(),
                folders[2].breadcrumbs(ctx)
            );
        });
    });
}

/// Asserts that the object with the given ID has the expected sorting timestamp.
#[track_caller]
fn assert_sorting_timestamp(id: ServerId, expected_ts: impl Into<ServerTimestamp>, app: &App) {
    let sorting_timestamp = app.read(|ctx| {
        let object = CloudModel::as_ref(ctx).get_by_uid(&id.uid())?;
        CloudViewModel::as_ref(ctx).object_sorting_timestamp(object, UpdateTimestamp::Revision, ctx)
    });
    assert_eq!(
        sorting_timestamp,
        Some(expected_ts.into()),
        "Unexpected timestamp for {}",
        id.uid()
    );
}

/// Test that, if an object is updated, we recalculate its ancestors' sorting timestamps too. This
/// way, the folders containing the updated object move to the top of the Warp Drive index if it's
/// sorted by last updated.
#[test]
fn test_update_folder_timestamp_from_child_update() {
    App::test((), |mut app| async move {
        initialize_app(&mut app, Vec::new());

        let folder_id: ServerId = 123.into();
        let parent_folder_id: ServerId = 456.into();
        let notebook_id: ServerId = 789.into();
        let initial_ts = Utc::now();

        CloudModel::handle(&app).update(&mut app, |cloud_model, _ctx| {
            let mut folder = mock_cloud_folder(
                folder_id.into(),
                "Folder".to_string(),
                Some(parent_folder_id.into()),
            );
            folder.metadata.revision = Some(initial_ts.into());

            let mut parent_folder =
                mock_cloud_folder(parent_folder_id.into(), "Parent Folder".to_string(), None);
            parent_folder.metadata.revision = Some(initial_ts.into());

            let mut notebook = mock_cloud_notebook(
                notebook_id.into(),
                "Test Notebook".to_string(),
                Some(folder_id.into()),
            );
            notebook.metadata.revision = Some(initial_ts.into());

            cloud_model.add_object(folder_id.into(), folder);
            cloud_model.add_object(parent_folder_id.into(), parent_folder);
            cloud_model.add_object(notebook_id.into(), notebook);
        });

        // At first, all 3 objects should have the initial sorting timestamp.
        assert_sorting_timestamp(folder_id, initial_ts, &app);
        assert_sorting_timestamp(parent_folder_id, initial_ts, &app);
        assert_sorting_timestamp(notebook_id, initial_ts, &app);

        // After updating the notebook, all 3 timestamps should change.
        UpdateManager::handle(&app).update(&mut app, |update_manager, ctx| {
            update_manager.update_notebook_data(
                Arc::new("test2".to_string()),
                notebook_id.into(),
                ctx,
            );
        });
        let new_ts = notebook_revision(notebook_id, &app);
        assert!(new_ts >= initial_ts);

        assert_sorting_timestamp(folder_id, new_ts, &app);
        assert_sorting_timestamp(parent_folder_id, new_ts, &app);
        assert_sorting_timestamp(notebook_id, new_ts, &app);
    });
}

/// Tests that, if an object is moved from one folder to another, we recalculate the sorting
/// timestamps of both. If the object was the most-recently-updated in its old folder, the old
/// folder's sorting timestamp will decrease. If it's the most-recently-updated object in the new
/// folder, the new folder's sorting timestamp will increase.
#[test]
fn test_update_folder_timestamp_from_object_move() {
    App::test((), |mut app| async move {
        initialize_app(&mut app, Vec::new());

        let folder_a_id: ServerId = 123.into();
        let folder_b_id: ServerId = 456.into();
        let notebook_id: ServerId = 789.into();

        let t1 = Utc::now() - chrono::Duration::seconds(10);
        let t2 = t1 + chrono::Duration::seconds(5);
        CloudModel::handle(&app).update(&mut app, |cloud_model, _ctx| {
            let mut folder_a = mock_cloud_folder(folder_a_id.into(), "Folder A".to_string(), None);
            folder_a.metadata.revision = Some(t1.into());

            let mut folder_b = mock_cloud_folder(folder_b_id.into(), "Folder B".to_string(), None);
            folder_b.metadata.revision = Some(t1.into());

            let mut notebook = mock_cloud_notebook(
                notebook_id.into(),
                "Test Notebook".to_string(),
                Some(folder_a_id.into()),
            );
            notebook.metadata.revision = Some(t2.into());
            notebook.metadata.metadata_last_updated_ts = Some(t2.into());

            cloud_model.add_object(folder_a_id.into(), folder_a);
            cloud_model.add_object(folder_b_id.into(), folder_b);
            cloud_model.add_object(notebook_id.into(), notebook);
        });

        // At first, folder A and the notebook sort by the notebook's timestamp, and folder B sorts
        // by its older timestamp.
        assert_sorting_timestamp(folder_a_id, t2, &app);
        assert_sorting_timestamp(folder_b_id, t1, &app);
        assert_sorting_timestamp(notebook_id, t2, &app);

        // Move the workflow to folder B, so it now has the newer sort timestamp.
        move_object(
            notebook_id,
            CloudObjectLocation::Folder(folder_b_id.into()),
            &mut app,
        );

        assert_sorting_timestamp(folder_a_id, t1, &app);
        assert_sorting_timestamp(folder_b_id, t2, &app);

        // Move the workflow into the root, so both folders have the older sort timestamp.
        move_object(
            notebook_id,
            CloudObjectLocation::Space(Space::Personal),
            &mut app,
        );
        assert_sorting_timestamp(folder_a_id, t1, &app);
        assert_sorting_timestamp(folder_b_id, t1, &app);
    });
}

/// Tests that, if an object is created in a folder, we recalculate its ancestors' sorting
/// timestamp. The new object will likely be the most-recently-updated child of the folder, so the
/// folder's sorting timestamp will increase.
#[test]
fn test_update_folder_timestamp_from_new_child() {
    App::test((), |mut app| async move {
        initialize_app(&mut app, Vec::new());

        let folder_id: ServerId = 123.into();
        let parent_folder_id: ServerId = 456.into();
        let t1 = Utc::now() - chrono::Duration::seconds(10);

        CloudModel::handle(&app).update(&mut app, |cloud_model, _ctx| {
            let mut folder = mock_cloud_folder(
                folder_id.into(),
                "Folder".to_string(),
                Some(parent_folder_id.into()),
            );
            folder.metadata.revision = Some(t1.into());

            let mut parent_folder =
                mock_cloud_folder(parent_folder_id.into(), "Parent Folder".to_string(), None);
            parent_folder.metadata.revision = Some(t1.into());

            cloud_model.add_object(folder_id.into(), folder);
            cloud_model.add_object(parent_folder_id.into(), parent_folder);
        });

        // At first, only the two folders exist.
        assert_sorting_timestamp(folder_id, t1, &app);
        assert_sorting_timestamp(parent_folder_id, t1, &app);

        // Create a notebook inside the folder.
        let notebook_id = ClientId::new();
        UpdateManager::handle(&app).update(&mut app, |update_manager, ctx| {
            update_manager.create_notebook(
                notebook_id,
                Owner::mock_current_user(),
                Some(folder_id.into()),
                CloudNotebookModel {
                    title: "Test Notebook".to_string(),
                    data: "test".to_string(),
                    ai_document_id: None,
                    conversation_id: None,
                },
                false,
                ctx,
            );
        });
        let t2 = CloudModel::handle(&app).read(&app, |cloud_model, _| {
            cloud_model
                .get_notebook(&SyncId::ClientId(notebook_id))
                .and_then(|notebook| notebook.metadata.revision.clone())
                .expect("new notebook should have a revision")
                .utc()
        });
        assert!(t2 > t1);

        // The notebook timestamp is now the sort timestamp for the folders.
        assert_sorting_timestamp(folder_id, t2, &app);
        assert_sorting_timestamp(parent_folder_id, t2, &app);
    });
}

/// Tests that, if an object is trashed or untrashed, we recalculate its folder's sorting timestamp.
/// Only untrashed children count towards a folder's sorting timestamp, so trashing/untrashing
/// effectively changes the folder's contents.
#[test]
fn test_update_folder_timestamp_from_child_trash() {
    App::test((), |mut app| async move {
        initialize_app(&mut app, Vec::new());

        let notebook_id: ServerId = 123.into();
        let folder_id: ServerId = 456.into();

        let t1 = Utc::now() - chrono::Duration::seconds(10);
        let t2 = t1 + chrono::Duration::seconds(1);

        CloudModel::handle(&app).update(&mut app, |cloud_model, _ctx| {
            let mut folder = mock_cloud_folder(folder_id.into(), "Folder".to_string(), None);
            folder.metadata.revision = Some(t1.into());

            let mut notebook = mock_cloud_notebook(
                notebook_id.into(),
                "Notebook".to_string(),
                Some(folder_id.into()),
            );
            notebook.metadata.revision = Some(t2.into());
            notebook.metadata.metadata_last_updated_ts = Some(t2.into());

            cloud_model.add_object(folder_id.into(), folder);
            cloud_model.add_object(notebook_id.into(), notebook);
        });

        assert_sorting_timestamp(folder_id, t2, &app);

        // Trash the notebook so that it no longer counts towards the folder's sort timestamp.
        let notebook = CloudObjectTypeAndId::Notebook(notebook_id.into());
        UpdateManager::handle(&app).update(&mut app, |update_manager, ctx| {
            update_manager.trash_object(notebook, ctx);
        });

        assert_sorting_timestamp(folder_id, t1, &app);

        // Untrash the notebook, updating the folder timestamp.
        UpdateManager::handle(&app).update(&mut app, |update_manager, ctx| {
            update_manager.untrash_object(notebook, ctx);
        });

        assert_sorting_timestamp(folder_id, t2, &app);
    });
}

#[test]
fn test_shared_personal_object() {
    let _guard = FeatureFlag::SharedWithMe.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_app(&mut app, Vec::new());

        let other_user = UserUid::new("other_user");
        let shared_notebook_id = SyncId::ServerId(123.into());
        let shared_notebook = CloudNotebook::new(
            shared_notebook_id,
            CloudNotebookModel {
                title: "Shared Notebook".to_string(),
                data: "Hello".to_string(),
                ai_document_id: None,
                conversation_id: None,
            },
            CloudObjectMetadata::new_from_server(mock_server_metadata()),
            CloudObjectPermissions {
                owner: Owner::User {
                    user_uid: other_user,
                },
                guests: Vec::new(),
                permissions_last_updated_ts: None,
                anyone_with_link: None,
            },
        );

        CloudModel::handle(&app).update(&mut app, |cloud_model, _| {
            cloud_model.add_object(shared_notebook_id, shared_notebook);

            let space = cloud_model
                .get_notebook(&shared_notebook_id)
                .expect("Notebook is in CloudModel")
                .space();
            assert_eq!(space, Space::Shared);
        });
    });
}

#[test]
fn test_unshared_personal_object() {
    let _guard = FeatureFlag::SharedWithMe.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_app(&mut app, Vec::new());

        let shared_notebook_id = SyncId::ServerId(123.into());
        let shared_notebook = CloudNotebook::new(
            shared_notebook_id,
            CloudNotebookModel {
                title: "Shared Notebook".to_string(),
                data: "Hello".to_string(),
                ai_document_id: None,
                conversation_id: None,
            },
            CloudObjectMetadata::new_from_server(mock_server_metadata()),
            CloudObjectPermissions {
                owner: Owner::User {
                    user_uid: UserUid::new(LOCAL_USER_UID),
                },
                guests: Vec::new(),
                permissions_last_updated_ts: None,
                anyone_with_link: None,
            },
        );

        CloudModel::handle(&app).update(&mut app, |cloud_model, _| {
            cloud_model.add_object(shared_notebook_id, shared_notebook);

            let space = cloud_model
                .get_notebook(&shared_notebook_id)
                .expect("Notebook is in CloudModel")
                .space();
            assert_eq!(space, Space::Personal);
        });
    });
}

#[test]
fn test_shared_team_object() {
    let _guard = FeatureFlag::SharedWithMe.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_app(&mut app, Vec::new());

        // The user is not on this team.
        let team_uid = ServerId::from(456);

        let shared_notebook_id = SyncId::ServerId(123.into());
        let shared_notebook = CloudNotebook::new(
            shared_notebook_id,
            CloudNotebookModel {
                title: "Shared Notebook".to_string(),
                data: "Hello".to_string(),
                ai_document_id: None,
                conversation_id: None,
            },
            CloudObjectMetadata::new_from_server(mock_server_metadata()),
            CloudObjectPermissions {
                owner: Owner::Team { team_uid },
                guests: Vec::new(),
                permissions_last_updated_ts: None,
                anyone_with_link: None,
            },
        );

        CloudModel::handle(&app).update(&mut app, |cloud_model, _| {
            cloud_model.add_object(shared_notebook_id, shared_notebook);

            let space = cloud_model
                .get_notebook(&shared_notebook_id)
                .expect("Notebook is in CloudModel")
                .space();
            assert_eq!(space, Space::Shared);
        });
    });
}

#[test]
fn test_shared_object_in_unshared_folder() {
    let _guard = FeatureFlag::SharedWithMe.override_enabled(true);
    App::test((), |mut app| async move {
        app.update(init_and_register_user_preferences);
        initialize_app(&mut app, Vec::new());

        let other_user = UserUid::new("other_user");
        let unshared_folder_id = SyncId::ServerId(567.into());
        let shared_notebook_id = SyncId::ServerId(123.into());
        let mut shared_notebook = CloudNotebook::new(
            shared_notebook_id,
            CloudNotebookModel {
                title: "Shared Notebook".to_string(),
                data: "Hello".to_string(),
                ai_document_id: None,
                conversation_id: None,
            },
            CloudObjectMetadata::new_from_server(mock_server_metadata()),
            CloudObjectPermissions {
                owner: Owner::User {
                    user_uid: other_user,
                },
                guests: Vec::new(),
                permissions_last_updated_ts: None,
                anyone_with_link: None,
            },
        );
        shared_notebook.metadata_mut().folder_id = Some(unshared_folder_id);

        CloudModel::handle(&app).update(&mut app, |cloud_model, _| {
            cloud_model.add_object(shared_notebook_id, shared_notebook);
            let notebook = cloud_model
                .get_notebook(&shared_notebook_id)
                .expect("Notebook is in CloudModel");

            // Check space-based APIs.
            assert_eq!(notebook.space(), Space::Shared);
            assert!(notebook.is_in_space(Space::Shared));

            // Check location-based APIs.
            assert_eq!(
                notebook.location(cloud_model),
                CloudObjectLocation::Space(Space::Shared)
            );
            assert!(notebook.metadata.folder_id.is_some());

            // Despite the missing parent folder, the notebook is not trashed.
            assert!(!notebook.is_trashed(cloud_model));

            // Check that iteration APIs include the notebook where it's expected.
            assert!(cloud_model
                .active_cloud_objects_in_space(Space::Shared)
                .any(|obj| obj.uid() == notebook.uid()));
            assert!(cloud_model
                .active_cloud_objects_in_location_without_descendents(CloudObjectLocation::Space(
                    Space::Shared
                ))
                .any(|obj| obj.uid() == notebook.uid()));
            assert_eq!(
                cloud_model
                    .trashed_cloud_objects_in_space(Space::Shared)
                    .count(),
                0
            );
            assert_eq!(
                cloud_model
                    .trashed_cloud_objects_in_location_without_descendents(
                        CloudObjectLocation::Space(Space::Shared)
                    )
                    .count(),
                0
            );

            let folder_location = CloudObjectLocation::Folder(unshared_folder_id);
            assert_eq!(
                cloud_model
                    .active_cloud_objects_in_location_without_descendents(folder_location)
                    .count(),
                0
            );
            assert_eq!(
                cloud_model
                    .trashed_cloud_objects_in_location_without_descendents(folder_location)
                    .count(),
                0
            );
        });
    });
}

/// Helper: compute active UIDs using the naive (non-memoized) is_trashed approach.
fn naive_active_object_uids(model: &CloudModel) -> HashSet<String> {
    model
        .as_cloud_objects()
        .filter(|obj| !obj.is_trashed(model))
        .map(|obj| obj.uid())
        .collect()
}

#[test]
fn active_object_uids_matches_naive_with_no_trashed_objects() {
    let folder_id = SyncId::ServerId(1.into());
    let objects: Vec<Box<dyn CloudObject>> = vec![
        Box::new(mock_cloud_folder(folder_id, "Folder".into(), None)),
        Box::new(mock_cloud_notebook(
            SyncId::ServerId(2.into()),
            "Notebook".into(),
            Some(folder_id),
        )),
    ];

    App::test((), |mut app| async move {
        let cloud_model = create_cloud_model(&mut app, objects);
        cloud_model.read(&app, |model, _| {
            assert_eq!(model.active_object_uids(), naive_active_object_uids(model));
            assert_eq!(model.active_object_uids().len(), 2);
        });
    });
}

#[test]
fn active_object_uids_matches_naive_with_directly_trashed_object() {
    let trashed_folder_id = SyncId::ServerId(1.into());
    let active_notebook_id = SyncId::ServerId(2.into());
    let objects: Vec<Box<dyn CloudObject>> = vec![
        Box::new(mock_trashed_cloud_folder(
            trashed_folder_id,
            "Trashed Folder".into(),
            None,
        )),
        Box::new(mock_cloud_notebook(
            active_notebook_id,
            "Active Notebook".into(),
            None,
        )),
    ];

    App::test((), |mut app| async move {
        let cloud_model = create_cloud_model(&mut app, objects);
        cloud_model.read(&app, |model, _| {
            let active = model.active_object_uids();
            assert_eq!(active, naive_active_object_uids(model));
            assert_eq!(active.len(), 1);
            assert!(active.contains(&active_notebook_id.uid()));
            assert!(!active.contains(&trashed_folder_id.uid()));
        });
    });
}

#[test]
fn active_object_uids_matches_naive_with_indirectly_trashed_children() {
    // A trashed folder with a non-trashed notebook inside it.
    // The notebook should be considered trashed (indirectly) by both approaches.
    let trashed_folder_id = SyncId::ServerId(1.into());
    let child_notebook_id = SyncId::ServerId(2.into());
    let active_notebook_id = SyncId::ServerId(3.into());
    let objects: Vec<Box<dyn CloudObject>> = vec![
        Box::new(mock_trashed_cloud_folder(
            trashed_folder_id,
            "Trashed Folder".into(),
            None,
        )),
        Box::new(mock_cloud_notebook(
            child_notebook_id,
            "Child in Trashed Folder".into(),
            Some(trashed_folder_id),
        )),
        Box::new(mock_cloud_notebook(
            active_notebook_id,
            "Top-level Notebook".into(),
            None,
        )),
    ];

    App::test((), |mut app| async move {
        let cloud_model = create_cloud_model(&mut app, objects);
        cloud_model.read(&app, |model, _| {
            let active = model.active_object_uids();
            assert_eq!(active, naive_active_object_uids(model));
            assert_eq!(active.len(), 1);
            assert!(active.contains(&active_notebook_id.uid()));
        });
    });
}

#[test]
fn active_object_uids_matches_naive_with_nested_trashed_folder() {
    // folder_a (trashed) -> folder_b (not trashed) -> notebook (not trashed)
    // Both folder_b and notebook should be indirectly trashed.
    let folder_a_id = SyncId::ServerId(1.into());
    let folder_b_id = SyncId::ServerId(2.into());
    let notebook_id = SyncId::ServerId(3.into());
    let active_notebook_id = SyncId::ServerId(4.into());
    let objects: Vec<Box<dyn CloudObject>> = vec![
        Box::new(mock_trashed_cloud_folder(
            folder_a_id,
            "Folder A (trashed)".into(),
            None,
        )),
        Box::new(mock_cloud_folder(
            folder_b_id,
            "Folder B".into(),
            Some(folder_a_id),
        )),
        Box::new(mock_cloud_notebook(
            notebook_id,
            "Deeply nested".into(),
            Some(folder_b_id),
        )),
        Box::new(mock_cloud_notebook(
            active_notebook_id,
            "Active".into(),
            None,
        )),
    ];

    App::test((), |mut app| async move {
        let cloud_model = create_cloud_model(&mut app, objects);
        cloud_model.read(&app, |model, _| {
            let active = model.active_object_uids();
            assert_eq!(active, naive_active_object_uids(model));
            assert_eq!(active.len(), 1);
            assert!(active.contains(&active_notebook_id.uid()));
        });
    });
}

#[test]
fn active_object_uids_matches_naive_with_empty_model() {
    App::test((), |mut app| async move {
        let cloud_model = create_cloud_model(&mut app, vec![]);
        cloud_model.read(&app, |model, _| {
            let active = model.active_object_uids();
            assert_eq!(active, naive_active_object_uids(model));
            assert!(active.is_empty());
        });
    });
}
