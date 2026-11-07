use crate::auth::UserUid;
use crate::channel::ChannelState;

const STRIPE_SUBSCRIPTION_INTERVAL_PAGE_PREFIX: &str = "/upgrade";

/// The plan upgrade page for `user_id`.
pub fn upgrade_link(user_id: UserUid) -> String {
    format!(
        "{}{}/{}/{}",
        ChannelState::server_root_url(),
        STRIPE_SUBSCRIPTION_INTERVAL_PAGE_PREFIX,
        "user",
        user_id.as_str()
    )
}
