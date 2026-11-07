use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use derivative::Derivative;
use serde::{Deserialize, Serialize};
use uncaged_core::features::FeatureFlag;

use crate::auth::UserUid;
use crate::drive::sharing::{SharingAccessLevel, Subject, TeamKind, UserKind};
use crate::ids::{FolderId, ServerId, SyncId};
use crate::time::ServerTimestamp;

mod actions;
mod generic_cloud_object;
mod generic_string_model;
pub mod models;
mod server_object;

pub use actions::*;
pub use generic_cloud_object::*;
pub use generic_string_model::*;
pub use server_object::*;
/// The type of object id each ObjectType corresponds to.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ObjectIdType {
    Notebook,
    Workflow,
    Folder,
    GenericStringObject,
}

impl ObjectIdType {
    /// Returns the prefix for server IDs as we store them in sqlite. The prefix for these
    /// objects is in title case unlike how we store the object types, which is why two different
    /// APIs are needed.
    pub fn sqlite_prefix(&self) -> &'static str {
        match self {
            ObjectIdType::Notebook => "Notebook",
            ObjectIdType::Workflow => "Workflow",
            ObjectIdType::Folder => "Folder",
            ObjectIdType::GenericStringObject => "GenericStringObject",
        }
    }
}

/// A type for communicating the type of cloud object to/from the server, absent of the object itself.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize)]
pub enum ObjectType {
    Notebook,
    Workflow,
    Folder,
    GenericStringObject(GenericStringObjectFormat),
}

impl ObjectType {
    /// Returns the serialized string for the object type, to be used for storing object_type in sqlite.
    pub fn sqlite_object_type_as_str(&self) -> Cow<'_, str> {
        match self {
            ObjectType::Notebook => "NOTEBOOK".into(),
            ObjectType::Workflow => "WORKFLOW".into(),
            ObjectType::Folder => "FOLDER".into(),
            ObjectType::GenericStringObject(format) => format.to_string().into(),
        }
    }
}

const NOTEBOOK_OBJECT_STRING: &str = "notebook";
const WORKFLOW_OBJECT_STRING: &str = "workflow";
const PROMPT_OBJECT_STRING: &str = "prompt";
const FOLDER_OBJECT_STRING: &str = "folder";
const ENV_VAR_COLLECTION_STRING: &str = "env-vars";

impl FromStr for ObjectType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            NOTEBOOK_OBJECT_STRING => Ok(Self::Notebook),
            WORKFLOW_OBJECT_STRING => Ok(Self::Workflow),
            PROMPT_OBJECT_STRING => Ok(Self::Workflow),
            FOLDER_OBJECT_STRING => Ok(Self::Folder),
            ENV_VAR_COLLECTION_STRING => Ok(Self::GenericStringObject(
                GenericStringObjectFormat::Json(JsonObjectType::EnvVarCollection),
            )),
            _ => Err(anyhow!("Unexpected object type")),
        }
    }
}

impl fmt::Display for ObjectType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ObjectType::Notebook => write!(f, "{NOTEBOOK_OBJECT_STRING}"),
            ObjectType::Workflow => write!(f, "{WORKFLOW_OBJECT_STRING}"),
            ObjectType::Folder => write!(f, "{FOLDER_OBJECT_STRING}"),
            ObjectType::GenericStringObject(GenericStringObjectFormat::Json(
                JsonObjectType::EnvVarCollection,
            )) => write!(f, "{ENV_VAR_COLLECTION_STRING}"),
            ObjectType::GenericStringObject(GenericStringObjectFormat::Json(
                JsonObjectType::AIFact,
            )) => write!(f, "rule"),
            ObjectType::GenericStringObject(_) => write!(f, "string_object_placeholder"), // placeholder value
        }
    }
}

impl From<ObjectType> for ObjectIdType {
    fn from(value: ObjectType) -> Self {
        match value {
            ObjectType::Notebook => ObjectIdType::Notebook,
            ObjectType::Workflow => ObjectIdType::Workflow,
            ObjectType::Folder => ObjectIdType::Folder,
            ObjectType::GenericStringObject(_) => ObjectIdType::GenericStringObject,
        }
    }
}

/// The object type prefix for generic string objects.
pub const GENERIC_STRING_OBJECT_PREFIX: &str = "GENERIC_STRING_";

/// The object type prefix for json objects.
pub const JSON_OBJECT_PREFIX: &str = "JSON_";

/// The data format for the generic string object type.
/// Right now we only support json, but this is left
/// open to support markdown, yaml and other text based types.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Hash)]
pub enum GenericStringObjectFormat {
    Json(JsonObjectType),
}

/// Represents a unique key for a generic string object. The server enforces that
/// no two generic string objects have the same key.
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct GenericStringObjectUniqueKey {
    /// The unique key. E.g. for cloud prefs this is the storage key of the pref.
    pub key: String,

    /// Whether this key is unique for all generic string objects, or unique per user.
    pub unique_per: UniquePer,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum UniquePer {
    User,
}

// Temporarily suppress clippy warnings about the `ToString` impl until we
// move `ObjectType` away from using `std::fmt::Display` for serialization.
#[allow(clippy::to_string_trait_impl)]
impl ToString for GenericStringObjectFormat {
    fn to_string(&self) -> String {
        match self {
            GenericStringObjectFormat::Json(json_object_type) => format!(
                "{}{}{}",
                GENERIC_STRING_OBJECT_PREFIX,
                JSON_OBJECT_PREFIX,
                json_object_type.as_str()
            ),
        }
    }
}

/// An object sub-type for objects that implement the JsonModel trait.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Hash)]
pub enum JsonObjectType {
    EnvVarCollection,
    WorkflowEnum,
    AIFact,
    MCPServer,
    AIExecutionProfile,
    TemplatableMCPServer,
}

impl JsonObjectType {
    pub fn as_str(&self) -> &'static str {
        match self {
            JsonObjectType::EnvVarCollection => "ENVVARCOLLECTION",
            JsonObjectType::WorkflowEnum => "WORKFLOWENUM",
            JsonObjectType::AIFact => "AIFACT",
            JsonObjectType::MCPServer => "MCPSERVER",
            JsonObjectType::AIExecutionProfile => "AIEXECUTIONPROFILE",
            JsonObjectType::TemplatableMCPServer => "TEMPLATABLEMCPSERVER",
        }
    }
}

impl TryFrom<&str> for JsonObjectType {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        match value {
            "ENVVARCOLLECTION" => Ok(JsonObjectType::EnvVarCollection),
            "WORKFLOWENUM" => Ok(JsonObjectType::WorkflowEnum),
            "AIFACT" => Ok(JsonObjectType::AIFact),
            "MCPSERVER" => Ok(JsonObjectType::MCPServer),
            "AIEXECUTIONPROFILE" => Ok(JsonObjectType::AIExecutionProfile),
            "TEMPLATABLEMCPSERVER" => Ok(JsonObjectType::TemplatableMCPServer),
            _ => Err(anyhow!("could not convert unknown json object type")),
        }
    }
}

/// The revision timestamp at which an object was edited. This is used by the server
/// to determine if an edit to an object was at the latest revision. Edits at older
/// revisions are rejected by the server.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, PartialOrd, Ord)]
pub struct Revision(ServerTimestamp);

impl Revision {
    pub fn from_unix_timestamp_micros(ms_since_epoch: i64) -> Result<Self> {
        let ts = ServerTimestamp::from_unix_timestamp_micros(ms_since_epoch)?;
        Ok(Self(ts))
    }

    pub fn timestamp_micros(&self) -> i64 {
        self.0.timestamp_micros()
    }

    pub fn utc(&self) -> DateTime<Utc> {
        self.0.utc()
    }

    /// Returns the inner `ServerTimestamp`.
    pub fn timestamp(&self) -> ServerTimestamp {
        self.0
    }

    /// A revision for an edit made now.
    pub fn now() -> Self {
        Self(ServerTimestamp::new(Utc::now()))
    }
}

impl From<Revision> for ServerTimestamp {
    fn from(revision: Revision) -> Self {
        revision.0
    }
}

impl From<ServerTimestamp> for Revision {
    fn from(time: ServerTimestamp) -> Self {
        Revision(time)
    }
}

#[cfg(any(test, feature = "test-util"))]
impl From<DateTime<Utc>> for Revision {
    fn from(time: DateTime<Utc>) -> Self {
        Self(ServerTimestamp::new(time))
    }
}

/// The uid that owns every object in the local user's personal drive. Uncaged
/// has no accounts, so this never changes; Drive rows in sqlite persist it as
/// their owner, so it must stay exactly this string.
pub const LOCAL_USER_UID: &str = "test_user_uid";

/// The owner for a given object.
#[derive(Copy, Clone, Debug, Eq, Serialize, Deserialize, Derivative)]
#[derivative(PartialEq)]
pub enum Owner {
    /// The owner of the object is a user (the object is in their personal drive).
    User { user_uid: UserUid },
    /// The owner of the object is a team (the object is in a team drive).
    Team { team_uid: ServerId },
}

impl Owner {
    /// The owner of the local user's personal drive.
    pub fn local_user() -> Owner {
        Owner::User {
            user_uid: UserUid::new(LOCAL_USER_UID),
        }
    }

    /// A mock [`Owner`] ID for testing.
    #[cfg(any(test, feature = "test-util"))]
    pub fn mock_current_user() -> Owner {
        Owner::local_user()
    }
}

impl From<Owner> for Option<ServerId> {
    fn from(owner: Owner) -> Option<ServerId> {
        match owner {
            Owner::User { .. } => None,
            Owner::Team { team_uid, .. } => Some(team_uid),
        }
    }
}

/// Server representation of an object's container. This corresponds to the `Container` GraphQL
/// type.
///
/// Containers are similar to, but not quite the same as, the [`CloudObjectLocation`] type.
/// Locations depend on object and user state - an object might currently be in the trash, or
/// it could be in one user's [shared space](Space::Shared) but another's
/// [team space](Space::Team). Containers, on the other hand, represent an object's canonical
/// parent - its one parent folder or drive that permissions are inherited from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerObjectContainer {
    Folder { folder_uid: ServerId },
    Drive { owner: Owner },
}

/// Server representation of a user object guest, as part of [`ServerObjectGuest`].
#[derive(Clone, Debug, PartialEq)]
pub enum ServerGuestSubject {
    User { firebase_uid: String },
    PendingUser { email: Option<String> },
    Team { team_uid: ServerId },
}

/// Server representation of a link-sharing setting.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerLinkSharing {
    pub access_level: SharingAccessLevel,
    pub source: Option<ServerObjectContainer>,
}

/// Server representation of an object guest. This corresponds to the `ObjectGuest` GraphQL type.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerObjectGuest {
    pub subject: ServerGuestSubject,
    pub access_level: SharingAccessLevel,
    /// If this guest is inherited, this is the ancestor that it's inherited from.
    pub source: Option<ServerObjectContainer>,
}

/// Metadata for a cloud object that was fetched from the server.
#[derive(Clone, Debug)]
pub struct ServerMetadata {
    pub uid: ServerId,
    pub revision: Revision,
    pub metadata_last_updated_ts: ServerTimestamp,
    pub trashed_ts: Option<ServerTimestamp>,
    pub folder_id: Option<FolderId>,
    pub is_welcome_object: bool,
    pub creator_uid: Option<String>,
    pub last_editor_uid: Option<String>,
    pub current_editor_uid: Option<String>,
}

/// Permissions for a cloud object that was fetched from the server.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerPermissions {
    /// The GraphQL definition of a `Space` is closer to the client's definition of an `Owner` (due
    /// to sharing). This is also going to migrate back to [ServerMetadata] as part of the
    /// `Container` migration.
    pub space: Owner,
    pub guests: Vec<ServerObjectGuest>,
    pub anyone_link_sharing: Option<ServerLinkSharing>,
    pub permissions_last_updated_ts: ServerTimestamp,
}

impl ServerPermissions {
    #[cfg(any(test, feature = "test-util"))]
    pub fn mock_personal() -> Self {
        Self {
            space: Owner::mock_current_user(),
            guests: Vec::new(),
            anyone_link_sharing: None,
            permissions_last_updated_ts: DateTime::<Utc>::default().into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CloudObjectPermissions {
    pub owner: Owner,
    pub permissions_last_updated_ts: Option<ServerTimestamp>,
    pub anyone_with_link: Option<CloudLinkSharing>,
    pub guests: Vec<CloudObjectGuest>,
}

impl CloudObjectPermissions {
    pub fn new_from_server(server_permissions: ServerPermissions) -> Self {
        let guests = if FeatureFlag::SharedWithMe.is_enabled() {
            server_permissions
                .guests
                .into_iter()
                .map(CloudObjectGuest::from_server)
                .collect()
        } else {
            Vec::new()
        };

        let anyone_with_link = if FeatureFlag::SharedWithMe.is_enabled() {
            server_permissions
                .anyone_link_sharing
                .map(CloudLinkSharing::from_server)
        } else {
            None
        };

        Self {
            owner: server_permissions.space,
            permissions_last_updated_ts: Some(server_permissions.permissions_last_updated_ts),
            guests,
            anyone_with_link,
        }
    }

    /// Mock permissions for a personal object.
    #[cfg(any(test, feature = "test-util"))]
    pub fn mock_personal() -> Self {
        Self {
            owner: Owner::mock_current_user(),
            permissions_last_updated_ts: Some(Utc::now().into()),
            guests: Vec::new(),
            anyone_with_link: None,
        }
    }

    /// Returns `true` if the given user has direct personal access to this object —
    /// either via an explicit user guest ACL entry or via link sharing.
    /// Returns `false` if the only access is through a team guest ACL.
    pub fn has_direct_user_access(&self, user_uid: UserUid) -> bool {
        self.anyone_with_link.is_some() || self.guests.iter().any(|g| g.subject.is_user(user_uid))
    }

    /// Updates self from new permissions information received from the server
    pub fn update_from_new_permissions_ts(&mut self, server_permissions: ServerPermissions) {
        self.owner = server_permissions.space;
        self.permissions_last_updated_ts = Some(server_permissions.permissions_last_updated_ts);
        if FeatureFlag::SharedWithMe.is_enabled() {
            self.guests = server_permissions
                .guests
                .into_iter()
                .map(CloudObjectGuest::from_server)
                .collect();
            self.anyone_with_link = server_permissions
                .anyone_link_sharing
                .map(CloudLinkSharing::from_server);
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CloudLinkSharing {
    pub access_level: SharingAccessLevel,
    // If this sharing setting was inherited, the `source` identifies the container it's inherited
    // from.
    pub source: Option<ServerObjectContainer>,
}

impl CloudLinkSharing {
    pub fn from_server(server_link_sharing: ServerLinkSharing) -> Self {
        Self {
            access_level: server_link_sharing.access_level,
            source: server_link_sharing.source,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CloudObjectGuest {
    pub subject: Subject,
    pub access_level: SharingAccessLevel,
    /// If this guest was added to a container object, the `source` identifies that object.
    pub source: Option<ServerObjectContainer>,
}

impl CloudObjectGuest {
    pub fn from_server(server_guest: ServerObjectGuest) -> Self {
        let subject = match server_guest.subject {
            ServerGuestSubject::User { firebase_uid } => {
                Subject::User(UserKind::Account(UserUid::new(&firebase_uid)))
            }
            ServerGuestSubject::PendingUser { email } => Subject::PendingUser { email },
            ServerGuestSubject::Team { team_uid } => Subject::Team(TeamKind::Team { team_uid }),
        };

        Self {
            subject,
            access_level: server_guest.access_level,
            source: server_guest.source,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CloudObjectMetadata {
    pub revision: Option<Revision>,
    pub metadata_last_updated_ts: Option<ServerTimestamp>,
    pub current_editor_uid: Option<String>,
    pub trashed_ts: Option<ServerTimestamp>,
    pub folder_id: Option<SyncId>,
    /// Welcome objects are created on the server when a user first receives
    /// access to Warp Drive as part of onboarding.
    pub is_welcome_object: bool,
    pub last_editor_uid: Option<String>,
    pub creator_uid: Option<String>,
    /// The "last used" timestamp for this environment.
    ///
    /// This is populated via `GetCloudEnvironments` from
    /// `CloudEnvironment.lastTaskCreated.createdAt`.
    /// Only applicable for CloudEnvironment objects.
    pub last_task_run_ts: Option<ServerTimestamp>,
}

impl CloudObjectMetadata {
    pub fn new_from_server(server_metadata: ServerMetadata) -> Self {
        Self {
            revision: Some(server_metadata.revision),
            current_editor_uid: server_metadata.current_editor_uid,
            metadata_last_updated_ts: Some(server_metadata.metadata_last_updated_ts),
            trashed_ts: server_metadata.trashed_ts,
            folder_id: server_metadata.folder_id.map(|id| id.into()),
            is_welcome_object: server_metadata.is_welcome_object,
            creator_uid: server_metadata.creator_uid,
            last_editor_uid: server_metadata.last_editor_uid,
            // last_task_run_ts is populated separately via GetCloudEnvironments query
            last_task_run_ts: None,
        }
    }

    /// Creates a new set of metadata with reasonable defaults for a test:
    /// * Content and metadata timestamps set to now
    /// * No editor information
    /// * No parent folder
    /// * Not trashed
    #[cfg(any(test, feature = "test-util"))]
    pub fn mock() -> Self {
        Self {
            revision: Some(Revision::now()),
            current_editor_uid: None,
            metadata_last_updated_ts: Some(Utc::now().into()),
            trashed_ts: None,
            folder_id: None,
            is_welcome_object: false,
            last_editor_uid: None,
            creator_uid: None,
            last_task_run_ts: None,
        }
    }

    pub fn set_current_editor(&mut self, editor_uid: Option<String>) {
        self.current_editor_uid = editor_uid;
    }
}

// A newtype for a serialized model that wraps a plain string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerializedModel(String);

impl SerializedModel {
    pub fn new(s: String) -> Self {
        Self(s)
    }

    pub fn model_as_str(&self) -> &str {
        &self.0
    }

    pub fn take(self) -> String {
        self.0
    }
}

impl From<String> for SerializedModel {
    fn from(s: String) -> Self {
        Self(s)
    }
}
