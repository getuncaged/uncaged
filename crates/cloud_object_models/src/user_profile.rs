use cloud_objects::UserUid;
use cloud_objects::ids::ServerId;

/// Public struct for storing all the UserProfile data that's fed in from either sqlite or the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserProfileWithUID {
    pub firebase_uid: UserUid,
    pub display_name: Option<String>,
    pub email: String,
    pub photo_url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserProfileIdAndName {
    pub user_uid: UserUid,
    pub display_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeamProfileIdAndName {
    pub team_uid: ServerId,
    pub display_name: String,
}
