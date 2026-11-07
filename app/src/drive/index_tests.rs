use warp_core::ui::appearance::Appearance;
use warpui::platform::WindowStyle;
use warpui::{AddSingletonModel, App, SingletonEntity, TypedActionView, ViewHandle};

use super::{DriveIndex, DriveIndexAction};
use crate::ai::blocklist::BlocklistAIHistoryModel;
use crate::cloud_object::model::actions::ObjectActions;
use crate::cloud_object::model::persistence::CloudModel;
use crate::cloud_object::model::view::CloudViewModel;
use crate::cloud_object::{ObjectType, Owner, Space};
use crate::drive::items::WarpDriveItemId;
use crate::drive::CloudObjectTypeAndId;
use crate::menu::MenuItem;
use crate::network::NetworkStatus;
use crate::notebooks::{CloudNotebook, CloudNotebookModel};
use crate::server::cloud_objects::update_manager::UpdateManager;
use crate::server::ids::{ClientId, SyncId};
use crate::server::server_api::ServerApiProvider;
use crate::server::sync_queue::SyncQueue;
use crate::settings_view::keybindings::KeybindingChangedNotifier;
use crate::test_util::settings::initialize_settings_for_tests;
use crate::workflows::workflow::Workflow;
use crate::workflows::{CloudWorkflow, CloudWorkflowModel};
use crate::workspaces::user_profiles::UserProfiles;
use crate::ASSETS;

fn initialize_app(app: &mut App) {
    initialize_settings_for_tests(app);

    app.add_singleton_model(CloudModel::mock);
    app.add_singleton_model(|_| NetworkStatus::new());
    app.add_singleton_model(|_| Appearance::mock());
    app.add_singleton_model(SyncQueue::mock);
    app.add_singleton_model(|_| ServerApiProvider::new_for_test());
    app.add_singleton_model(|_| UpdateManager::new(None));
    app.add_singleton_model(CloudViewModel::mock);
    app.add_singleton_model(|_| ObjectActions::new(Vec::new()));
    app.add_singleton_model(|_| UserProfiles::new(Vec::new()));
    app.add_singleton_model(|_| KeybindingChangedNotifier::mock());
    app.add_singleton_model(|_| BlocklistAIHistoryModel::new_for_test());
}

fn create_index(app: &mut App) -> ViewHandle<DriveIndex> {
    let (_, index) = app.add_window(WindowStyle::NotStealFocus, DriveIndex::new);
    index
}

fn create_workflow(app: &mut App) -> SyncId {
    CloudModel::handle(app).update(app, |cloud_model, ctx| {
        let client_id = ClientId::new();
        let sync_id = SyncId::ClientId(client_id);
        let workflow = Workflow::new("my workflow", "my command");
        cloud_model.create_object(
            sync_id,
            CloudWorkflow::new_local(
                CloudWorkflowModel::new(workflow),
                Owner::mock_current_user(),
                None,
                client_id,
            ),
            ctx,
        );
        sync_id
    })
}

fn create_notebook(app: &mut App) -> SyncId {
    CloudModel::handle(app).update(app, |cloud_model, ctx| {
        let client_id = ClientId::new();
        let sync_id = SyncId::ClientId(client_id);
        cloud_model.create_object(
            sync_id,
            CloudNotebook::new_local(
                CloudNotebookModel::default(),
                Owner::mock_current_user(),
                None,
                client_id,
            ),
            ctx,
        );
        sync_id
    })
}

fn label_for_menu_item(item: &MenuItem<DriveIndexAction>) -> &str {
    if let MenuItem::Item(item) = item {
        item.label()
    } else {
        panic!("item provided wasn't of type MenuItem::Item")
    }
}

#[test]
fn test_workflow_menu_items() {
    App::test(ASSETS, |mut app| async move {
        initialize_app(&mut app);
        let index = create_index(&mut app);
        let sync_id = create_workflow(&mut app);
        let cloud_object_type_and_id: CloudObjectTypeAndId =
            CloudObjectTypeAndId::from_id_and_type(sync_id, ObjectType::Workflow);
        let warp_drive_item_id = WarpDriveItemId::Object(cloud_object_type_and_id);

        // Uncaged (Oss): the Drive is local, so local (ClientId) objects can be trashed too.
        index.update(&mut app, |index, ctx| {
            let menu_items = index.menu_items(&Space::Personal, &warp_drive_item_id, ctx);
            assert_eq!(menu_items.len(), 5);
            assert_eq!(label_for_menu_item(&menu_items[0]), "Edit");
            assert_eq!(label_for_menu_item(&menu_items[1]), "Copy workflow text");
            assert_eq!(label_for_menu_item(&menu_items[2]), "Duplicate");
            assert_eq!(label_for_menu_item(&menu_items[3]), "Export");
            assert_eq!(label_for_menu_item(&menu_items[4]), "Trash");
        });
    })
}

#[test]
fn test_warp_drive_navigation_states() {
    use crate::drive::index::DriveIndexAction;
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let index = create_index(&mut app);
        let sync_id = create_notebook(&mut app);
        let cloud_object_type_and_id: CloudObjectTypeAndId =
            CloudObjectTypeAndId::from_id_and_type(sync_id, ObjectType::Notebook);

        index.read(&app, |index, _| {
            assert_eq!(index.selected, None, "Expect selected to be None");
            assert_eq!(
                index.focused_index,
                Some(0),
                "Expect focused_index to be initialized"
            );
        });

        index.update(&mut app, |index, ctx| {
            index.handle_action(&DriveIndexAction::OpenObject(cloud_object_type_and_id), ctx);
        });

        index.read(&app, |index, _| {
            assert_eq!(
                index.selected,
                Some(WarpDriveItemId::Object(cloud_object_type_and_id)),
                "Expect selected to have correct value"
            );
        });
    });
}
