pub use cloud_object_models::{CloudFolder, CloudFolderModel};
// Re-exported from warp_server_client.
pub use warp_server_client::ids::FolderId;

// Re-exported from warp_server_client.
use super::items::folder::WarpDriveFolder;
use super::items::WarpDriveItem;
use super::CloudObjectTypeAndId;
use crate::appearance::Appearance;
use crate::cloud_object::{
    CloudModelType, CloudObjectUpsertParams, ObjectType, SerializedModel, Space,
};
use crate::persistence::ModelEvent;
use crate::server::ids::SyncId;

impl CloudModelType for CloudFolderModel {
    type CloudObjectType = CloudFolder;
    type IdType = FolderId;

    fn model_type_name(&self) -> &'static str {
        "Folder"
    }

    fn object_type(&self) -> ObjectType {
        ObjectType::Folder
    }

    fn cloud_object_type_and_id(&self, id: SyncId) -> CloudObjectTypeAndId {
        CloudObjectTypeAndId::Folder(id)
    }

    fn display_name(&self) -> String {
        self.name.clone()
    }

    fn upsert_event(params: CloudObjectUpsertParams<Self>) -> ModelEvent {
        ModelEvent::UpsertFolder {
            folder: CloudFolder::from(params),
        }
    }

    fn bulk_upsert_event(objects: Vec<CloudObjectUpsertParams<Self>>) -> ModelEvent {
        ModelEvent::UpsertFolders(objects.into_iter().map(CloudFolder::from).collect())
    }

    fn serialized(&self) -> SerializedModel {
        SerializedModel::new(self.name.to_owned())
    }

    fn can_move_to_space(&self, current_space: Space, new_space: Space) -> bool {
        // We don't currently support moving folders across spaces.
        current_space == new_space
    }

    fn supports_linking(&self) -> bool {
        true
    }

    fn renders_in_warp_drive(&self) -> bool {
        true
    }

    fn to_warp_drive_item(
        &self,
        id: SyncId,
        _appearance: &Appearance,
        folder: &CloudFolder,
    ) -> Option<Box<dyn WarpDriveItem>> {
        Some(Box::new(WarpDriveFolder::new(
            self.cloud_object_type_and_id(id),
            folder.clone(),
        )))
    }
}
