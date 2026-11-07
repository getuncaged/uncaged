use crate::server::ids::ServerId;

/// The server id of a Warp workspace. Uncaged has no workspaces; this only
/// tags workspace-scoped bonus grants that Warp's request-usage query returns.
#[derive(Clone, Copy, Hash, Debug, PartialEq, Eq)]
pub struct WorkspaceUid(ServerId);
impl From<String> for WorkspaceUid {
    fn from(uid: String) -> Self {
        WorkspaceUid(ServerId::from_string_lossy(uid))
    }
}
impl From<WorkspaceUid> for String {
    fn from(workspace_uid: WorkspaceUid) -> String {
        workspace_uid.0.to_string()
    }
}
impl From<ServerId> for WorkspaceUid {
    fn from(uid: ServerId) -> Self {
        WorkspaceUid(uid)
    }
}
