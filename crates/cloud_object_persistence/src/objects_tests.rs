use chrono::Utc;
use cloud_objects::auth::UserUid;
use cloud_objects::cloud_object::{
    CloudObjectGuest, CloudObjectMetadata, CloudObjectPermissions, CloudObjectSyncStatus,
    GenericStringObjectFormat, JsonObjectType, NumInFlightRequests, ObjectIdType,
    ServerObjectContainer,
};
use cloud_objects::drive::sharing::{
    LinkSharingSubjectType, SharingAccessLevel, Subject, TeamKind, UserKind,
};
use cloud_objects::ids::{ClientId, ServerId, SyncId};
use diesel::{Connection, RunQueryDsl, SqliteConnection};
use diesel_migrations::MigrationHarness;
use persistence::model::ObjectMetadata;
use persistence::schema;
use warp_graphql::scalars::time::ServerTimestamp;

use super::{
    GenericStringObjectPersistenceData, delete_cloud_object, delete_generic_string_object,
    to_cloud_object_metadata, update_object_metadata, upsert_generic_string_objects,
};

#[test]
fn test_roundtrip_guests() {
    let guests = vec![
        CloudObjectGuest {
            subject: Subject::User(UserKind::Account(UserUid::new("firebase_uid"))),
            access_level: SharingAccessLevel::Edit,
            source: None,
        },
        CloudObjectGuest {
            subject: Subject::PendingUser {
                email: Some("pending@warp.dev".to_string()),
            },
            access_level: SharingAccessLevel::View,
            source: Some(ServerObjectContainer::Folder {
                folder_uid: 123.into(),
            }),
        },
        CloudObjectGuest {
            subject: Subject::Team(TeamKind::Team {
                team_uid: ServerId::from(99),
            }),
            access_level: SharingAccessLevel::Edit,
            source: None,
        },
    ];

    let encoded = super::encode_guests(&guests).expect("encode should succeed");
    let decoded = super::decode_guests(&encoded).expect("decode should succeed");

    assert_eq!(guests, decoded);
}

#[test]
fn test_fail_unsupported_subjects() {
    let result = super::encode_guests(&[CloudObjectGuest {
        subject: Subject::AnyoneWithLink(LinkSharingSubjectType::Anyone),
        access_level: SharingAccessLevel::View,
        source: None,
    }]);
    assert!(result.is_err());
}

/// Builds an in-memory SQLite database with all migrations applied.
fn test_connection() -> SqliteConnection {
    let mut conn =
        SqliteConnection::establish(":memory:").expect("in-memory sqlite connection should open");
    conn.run_pending_migrations(persistence::MIGRATIONS)
        .expect("migrations should run");
    conn
}

fn env_var_collection(
    id: SyncId,
    metadata: CloudObjectMetadata,
) -> GenericStringObjectPersistenceData {
    GenericStringObjectPersistenceData {
        id,
        format: GenericStringObjectFormat::Json(JsonObjectType::EnvVarCollection),
        metadata,
        permissions: CloudObjectPermissions::mock_personal(),
        data: "{}".to_string(),
    }
}

fn metadata_rows(conn: &mut SqliteConnection) -> Vec<ObjectMetadata> {
    schema::object_metadata::dsl::object_metadata
        .load::<ObjectMetadata>(conn)
        .expect("metadata should load")
}

fn count_generic_string_objects(conn: &mut SqliteConnection) -> usize {
    schema::generic_string_objects::dsl::generic_string_objects
        .load::<persistence::model::GenericStringObject>(conn)
        .expect("objects should load")
        .len()
}

#[test]
fn test_pending_row_loads_without_pending_changes() {
    let mut conn = test_connection();
    let mut metadata = CloudObjectMetadata::mock();
    metadata.pending_changes_statuses.content_sync_status =
        CloudObjectSyncStatus::InFlight(NumInFlightRequests(1));
    let id = SyncId::ClientId(ClientId::new());
    upsert_generic_string_objects(&mut conn, vec![env_var_collection(id, metadata)])
        .expect("upsert should succeed");

    let rows = metadata_rows(&mut conn);
    assert_eq!(rows.len(), 1);
    // Rows written while objects waited for Warp's server are marked pending.
    assert!(rows[0].is_pending);

    let loaded = to_cloud_object_metadata(&rows[0]);
    assert!(matches!(
        loaded.pending_changes_statuses.content_sync_status,
        CloudObjectSyncStatus::NoLocalChanges
    ));
    assert!(!loaded.has_pending_content_changes());
}

#[test]
fn test_upsert_writes_trash_state_and_folder_of_existing_client_id_row() {
    let mut conn = test_connection();
    let id = SyncId::ClientId(ClientId::new());
    upsert_generic_string_objects(
        &mut conn,
        vec![env_var_collection(id, CloudObjectMetadata::mock())],
    )
    .expect("insert should succeed");

    let trashed_ts = ServerTimestamp::new(Utc::now());
    let folder_id = SyncId::ClientId(ClientId::new());
    let mut metadata = CloudObjectMetadata::mock();
    metadata.trashed_ts = Some(trashed_ts);
    metadata.metadata_last_updated_ts = Some(trashed_ts);
    metadata.folder_id = Some(folder_id);
    upsert_generic_string_objects(&mut conn, vec![env_var_collection(id, metadata)])
        .expect("update should succeed");

    let rows = metadata_rows(&mut conn);
    assert_eq!(rows.len(), 1);
    let loaded = to_cloud_object_metadata(&rows[0]);
    assert_eq!(loaded.trashed_ts, Some(trashed_ts));
    assert_eq!(loaded.folder_id, Some(folder_id));
    assert!(!rows[0].is_pending);
}

#[test]
fn test_update_object_metadata_applies_to_client_id_rows() {
    let mut conn = test_connection();
    let client_id = ClientId::new();
    let id = SyncId::ClientId(client_id);
    upsert_generic_string_objects(
        &mut conn,
        vec![env_var_collection(id, CloudObjectMetadata::mock())],
    )
    .expect("insert should succeed");

    let trashed_ts = ServerTimestamp::new(Utc::now());
    let mut metadata = CloudObjectMetadata::mock();
    metadata.trashed_ts = Some(trashed_ts);
    update_object_metadata(
        &mut conn,
        id.sqlite_uid_hash(ObjectIdType::GenericStringObject),
        metadata,
    )
    .expect("metadata update should succeed");

    let rows = metadata_rows(&mut conn);
    assert_eq!(rows[0].client_id, Some(client_id.to_string()));
    assert_eq!(
        to_cloud_object_metadata(&rows[0]).trashed_ts,
        Some(trashed_ts)
    );
}

#[test]
fn test_update_object_metadata_applies_to_server_id_rows() {
    let mut conn = test_connection();
    let id = SyncId::ServerId(ServerId::from(5));
    upsert_generic_string_objects(
        &mut conn,
        vec![env_var_collection(id, CloudObjectMetadata::mock())],
    )
    .expect("insert should succeed");

    let trashed_ts = ServerTimestamp::new(Utc::now());
    let mut metadata = CloudObjectMetadata::mock();
    metadata.trashed_ts = Some(trashed_ts);
    update_object_metadata(
        &mut conn,
        id.sqlite_uid_hash(ObjectIdType::GenericStringObject),
        metadata,
    )
    .expect("metadata update should succeed");

    assert_eq!(
        to_cloud_object_metadata(&metadata_rows(&mut conn)[0]).trashed_ts,
        Some(trashed_ts)
    );
}

#[test]
fn test_deleting_a_missing_row_does_not_abort_the_batch() {
    let mut conn = test_connection();
    let existing = SyncId::ClientId(ClientId::new());
    upsert_generic_string_objects(
        &mut conn,
        vec![env_var_collection(existing, CloudObjectMetadata::mock())],
    )
    .expect("insert should succeed");

    let missing = SyncId::ClientId(ClientId::new());
    conn.transaction::<(), diesel::result::Error, _>(|conn| {
        for id in [missing, existing] {
            delete_cloud_object(
                conn,
                id,
                ObjectIdType::GenericStringObject,
                Box::new(delete_generic_string_object),
            )?;
        }
        Ok(())
    })
    .expect("deleting a missing row should not fail");

    assert!(metadata_rows(&mut conn).is_empty());
    assert_eq!(count_generic_string_objects(&mut conn), 0);
}

#[test]
fn test_team_owned_row_keeps_loading() {
    let mut conn = test_connection();
    let team_uid = ServerId::from(77);
    let mut object = env_var_collection(
        SyncId::ServerId(ServerId::from(8)),
        CloudObjectMetadata::mock(),
    );
    object.permissions.owner = cloud_objects::cloud_object::Owner::Team { team_uid };
    upsert_generic_string_objects(&mut conn, vec![object]).expect("insert should succeed");

    let read_context =
        super::load_cloud_object_read_context(&mut conn).expect("read context should load");
    let rows = metadata_rows(&mut conn);
    let permissions = read_context
        .permissions_for_metadata(&rows[0])
        .expect("permissions should load");
    assert_eq!(
        permissions.owner,
        cloud_objects::cloud_object::Owner::Team { team_uid }
    );
}
