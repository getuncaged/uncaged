use uncaged_ui::AppContext;

use super::{CloudObject, ObjectType};
use crate::server::cloud_objects::update_manager::{InitiatedBy, ObjectOperation};

pub struct CloudObjectToastMessage;

impl CloudObjectToastMessage {
    pub fn toast_message(
        object: &dyn CloudObject,
        operation: &ObjectOperation,
        app: &AppContext,
    ) -> Option<String> {
        let object_name = object.model_type_name().to_owned();

        match (object.object_type(), operation) {
            // We should only show toasts for creates initiated by the user, not by the system
            (
                _,
                ObjectOperation::Create {
                    initiated_by: InitiatedBy::User,
                },
            ) => {
                let containing_object_name = object.containing_object_name(app);
                Some(format!("{object_name} saved to {containing_object_name}"))
            }
            // notebooks intentionally do not have an update message, as they are updated
            // as the user types and so toasts would be VERY noisy
            (ObjectType::Notebook, ObjectOperation::Update) => None,
            (_, ObjectOperation::Update) => Some(format!("{object_name} updated")),
            (_, ObjectOperation::Trash) => Some(format!("{object_name} trashed")),
            (_, ObjectOperation::Untrash) => Some(format!("{object_name} restored")),
            _ => None,
        }
    }

    pub fn toast_deletion_confirm_message(
        num_objects: i32,
        operation: &ObjectOperation,
    ) -> Option<String> {
        let count_objects_message = match num_objects {
            1 => "1 object".to_string(),
            n => {
                format!("{n} objects")
            }
        };
        match operation {
            // We should only show deletion toasts for user-initiated deletions.
            ObjectOperation::Delete {
                initiated_by: InitiatedBy::User,
            } => Some(format!("{count_objects_message} deleted forever")),
            ObjectOperation::EmptyTrash if num_objects == 0 => {
                Some("No objects in trash to empty".to_string())
            }
            ObjectOperation::EmptyTrash => Some(format!(
                "Trash emptied: {count_objects_message} deleted forever"
            )),
            _ => None,
        }
    }
}
