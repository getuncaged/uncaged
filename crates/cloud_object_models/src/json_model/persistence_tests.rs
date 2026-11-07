use cloud_objects::cloud_object::{
    GENERIC_STRING_OBJECT_PREFIX, JSON_OBJECT_PREFIX, JsonObjectType,
};

use super::json_object_type_from_metadata;

fn metadata_object_type(json_object_type: &str) -> String {
    format!("{GENERIC_STRING_OBJECT_PREFIX}{JSON_OBJECT_PREFIX}{json_object_type}")
}

#[test]
fn known_json_object_types_are_loaded() {
    assert!(matches!(
        json_object_type_from_metadata(&metadata_object_type("ENVVARCOLLECTION")),
        Some(JsonObjectType::EnvVarCollection)
    ));
    assert!(matches!(
        json_object_type_from_metadata(&metadata_object_type("AIEXECUTIONPROFILE")),
        Some(JsonObjectType::AIExecutionProfile)
    ));
}

#[test]
fn removed_json_object_types_are_skipped() {
    // Object types removed with Warp's cloud agents and settings sync may still be in an
    // existing database.
    for removed in [
        "PREFERENCE",
        "CLOUDENVIRONMENT",
        "SCHEDULEDAMBIENTAGENT",
        "CLOUDAGENTCONFIG",
    ] {
        assert!(
            json_object_type_from_metadata(&metadata_object_type(removed)).is_none(),
            "{removed} should be skipped"
        );
    }
    assert!(json_object_type_from_metadata(&metadata_object_type("SOMETHING_NEWER")).is_none());
}

#[test]
fn non_json_object_types_are_skipped() {
    assert!(json_object_type_from_metadata("NOTEBOOK").is_none());
}
