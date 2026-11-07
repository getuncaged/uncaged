use std::fmt::Debug;

use warpui::ModelContext;

use crate::cloud_object::model::persistence::CloudModel;
use crate::cloud_object::{CloudModelType, GenericCloudObject, GenericServerObject};
use crate::server::ids::{HashableId, ToServerId};

/// Adds an object as it was synced from Warp's server before the fork, emitting
/// [`CloudModelEvent::ObjectCreated`](crate::cloud_object::model::persistence::CloudModelEvent).
pub fn add_server_object<K, M>(
    cloud_model: &mut CloudModel,
    object: GenericServerObject<K, M>,
    ctx: &mut ModelContext<CloudModel>,
) where
    K: HashableId + ToServerId + Debug + Into<String> + Clone + 'static,
    M: CloudModelType<IdType = K, CloudObjectType = GenericCloudObject<K, M>> + 'static,
{
    let id = object.id;
    cloud_model.create_object(id, GenericCloudObject::new_from_server(object), ctx);
}
