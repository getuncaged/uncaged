use cloud_object_persistence::{
    CloudObjectReadContext, id_from_metadata, read_generic_string_object_rows,
    to_cloud_object_metadata,
};
use cloud_objects::cloud_object::{
    GENERIC_STRING_OBJECT_PREFIX, GenericStringObjectFormat, JSON_OBJECT_PREFIX, JsonObjectType,
    ObjectType,
};
use cloud_objects::ids::GenericStringObjectId;
use diesel::SqliteConnection;
use diesel::result::Error;

use crate::{
    CloudAIExecutionProfile, CloudAIExecutionProfileModel, CloudAIFact, CloudAIFactModel,
    CloudEnvVarCollection, CloudEnvVarCollectionModel, CloudMCPServer, CloudMCPServerModel,
    CloudTemplatableMCPServer, CloudTemplatableMCPServerModel, CloudWorkflowEnum,
    CloudWorkflowEnumModel,
};

pub enum PersistedGenericStringObject {
    EnvVarCollection(CloudEnvVarCollection),
    WorkflowEnum(CloudWorkflowEnum),
    AIFact(CloudAIFact),
    MCPServer(CloudMCPServer),
    TemplatableMCPServer(CloudTemplatableMCPServer),
    AIExecutionProfile(CloudAIExecutionProfile),
}

/// Parses the JSON object type out of an `object_metadata.object_type` column value.
///
/// Rows written by older builds for object types this build no longer has (for example the
/// removed cloud environments, scheduled cloud agents, cloud agent configs and settings-sync
/// preferences) yield `None`, so
/// the loader skips them instead of failing to load Warp Drive.
fn json_object_type_from_metadata(object_type: &str) -> Option<JsonObjectType> {
    let json_object_type = object_type.strip_prefix(&format!(
        "{GENERIC_STRING_OBJECT_PREFIX}{JSON_OBJECT_PREFIX}"
    ))?;
    match JsonObjectType::try_from(json_object_type) {
        Ok(json_object_type) => Some(json_object_type),
        Err(_) => {
            log::debug!(
                "Skipping persisted object with unknown JSON object type {json_object_type}"
            );
            None
        }
    }
}

pub fn read_generic_string_objects(
    conn: &mut SqliteConnection,
    read_context: &CloudObjectReadContext,
) -> Result<Vec<PersistedGenericStringObject>, Error> {
    Ok(read_generic_string_object_rows(conn)?
        .into_iter()
        .filter_map(|object| {
            let metadata = read_context.metadata_for_object(
                object.id,
                ObjectType::GenericStringObject(GenericStringObjectFormat::Json(
                    JsonObjectType::EnvVarCollection,
                )),
            )?;
            let object_id = id_from_metadata::<GenericStringObjectId>(metadata)?;
            let cloud_object_permissions = read_context.permissions_for_metadata(metadata)?;
            let json_object_type = json_object_type_from_metadata(&metadata.object_type)?;
            match json_object_type {
                JsonObjectType::EnvVarCollection => {
                    let model = CloudEnvVarCollectionModel::deserialize_owned(&object.data);
                    model.ok().map(|model| {
                        PersistedGenericStringObject::EnvVarCollection(CloudEnvVarCollection::new(
                            object_id,
                            model,
                            to_cloud_object_metadata(metadata),
                            cloud_object_permissions,
                        ))
                    })
                }
                JsonObjectType::WorkflowEnum => {
                    let model = CloudWorkflowEnumModel::deserialize_owned(&object.data);
                    model.ok().map(|model| {
                        PersistedGenericStringObject::WorkflowEnum(CloudWorkflowEnum::new(
                            object_id,
                            model,
                            to_cloud_object_metadata(metadata),
                            cloud_object_permissions,
                        ))
                    })
                }
                JsonObjectType::AIFact => {
                    let model = CloudAIFactModel::deserialize_owned(&object.data);
                    model.ok().map(|model| {
                        PersistedGenericStringObject::AIFact(CloudAIFact::new(
                            object_id,
                            model,
                            to_cloud_object_metadata(metadata),
                            cloud_object_permissions,
                        ))
                    })
                }
                JsonObjectType::MCPServer => {
                    let model = CloudMCPServerModel::deserialize_owned(&object.data);
                    model.ok().map(|model| {
                        PersistedGenericStringObject::MCPServer(CloudMCPServer::new(
                            object_id,
                            model,
                            to_cloud_object_metadata(metadata),
                            cloud_object_permissions,
                        ))
                    })
                }
                JsonObjectType::TemplatableMCPServer => {
                    let model = CloudTemplatableMCPServerModel::deserialize_owned(&object.data);
                    model.ok().map(|model| {
                        PersistedGenericStringObject::TemplatableMCPServer(
                            CloudTemplatableMCPServer::new(
                                object_id,
                                model,
                                to_cloud_object_metadata(metadata),
                                cloud_object_permissions,
                            ),
                        )
                    })
                }
                JsonObjectType::AIExecutionProfile => {
                    let model = CloudAIExecutionProfileModel::deserialize_owned(&object.data);
                    model.ok().map(|model| {
                        PersistedGenericStringObject::AIExecutionProfile(
                            CloudAIExecutionProfile::new(
                                object_id,
                                model,
                                to_cloud_object_metadata(metadata),
                                cloud_object_permissions,
                            ),
                        )
                    })
                }
            }
        })
        .collect())
}

#[cfg(test)]
#[path = "persistence_tests.rs"]
mod tests;
