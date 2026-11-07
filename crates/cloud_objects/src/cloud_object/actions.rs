use chrono::{DateTime, Utc};

use crate::ids::{HashedSqliteId, ObjectUid};

/// The type of action that occurred on an object, such as an execution, selection, so on
/// and so forth.
#[derive(Clone, Debug, PartialEq)]
pub enum ObjectActionType {
    Execute,
}

// In order to convert from a graphql type and from a SQLite read, the action type
// implements to_string().
//
// Temporarily suppress clippy warnings about the `ToString` impl until we
// move `ObjectType` away from using `std::fmt::Display` for serialization.
#[allow(clippy::to_string_trait_impl)]
impl ToString for ObjectActionType {
    fn to_string(&self) -> String {
        match self {
            ObjectActionType::Execute => String::from("EXECUTE"),
        }
    }
}

impl ObjectActionType {
    pub fn singular(&self) -> String {
        match self {
            ObjectActionType::Execute => "run".to_string(),
        }
    }

    pub fn plural(&self) -> String {
        match self {
            ObjectActionType::Execute => "runs".to_string(),
        }
    }
}

/// We track object actions, both those that have been sent to the server and not, through this
/// type. A single ObjectAction represents an object_id, action pair and a subtype that contains data
/// about the action(s). Each ObjectAction either represents one action or a summary of identical actions
/// that occurred at different times. We summarize old actions in order to save memory footprint on the client.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectAction {
    pub action_type: ObjectActionType,
    pub uid: ObjectUid,
    pub hashed_sqlite_id: HashedSqliteId,
    // This action either represents one action or a consolidation of multiple actions.
    pub action_subtype: ObjectActionSubtype,
}

impl ObjectAction {
    pub fn is_pending(&self) -> bool {
        match self.action_subtype {
            ObjectActionSubtype::SingleAction { pending, .. } => pending,
            ObjectActionSubtype::BundledActions { .. } => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ObjectActionSubtype {
    SingleAction {
        timestamp: DateTime<Utc>,
        processed_at_timestamp: Option<DateTime<Utc>>,
        data: Option<String>,
        pending: bool,
    },
    BundledActions {
        count: i32,
        oldest_timestamp: DateTime<Utc>,
        latest_timestamp: DateTime<Utc>,
        latest_processed_at_timestamp: DateTime<Utc>,
    },
}
