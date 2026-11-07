//! Authentication for requests to Warp's server.
//!
//! Uncaged has no Warp accounts, so there are never credentials: every authenticated request
//! fails before it is sent. This goes with the rest of the server client.

/// Authentication and authenticated-transport conditions observed by shared client code.
#[derive(Clone, Debug)]
pub enum AuthEvent {
    /// A staging API call was blocked, which may indicate a firewall misconfiguration.
    StagingAccessBlocked,
    /// An Identity-Aware Proxy challenge was received.
    IapChallengeReceived,
}

/// A bearer token for Warp's server. Uncaged never has one; the type only lets the remaining
/// server-client code compile until it is removed.
#[derive(Debug, Clone)]
pub struct AuthToken(String);

impl AuthToken {
    /// Returns the token string to use in an Authorization header.
    pub fn as_bearer_token(&self) -> Option<&str> {
        Some(&self.0)
    }

    /// Returns the bearer token as an owned string.
    pub fn bearer_token(&self) -> Option<String> {
        Some(self.0.clone())
    }
}
