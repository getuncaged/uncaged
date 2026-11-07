use warp_core::errors::AnyhowErrorExt as _;

use super::HttpStatusError;

#[test]
fn shared_status_error_actionability_ignores_retryable_client_failures() {
    let error = anyhow::Error::new(HttpStatusError {
        status: 429,
        body: "retry later".to_string(),
    })
    .context("Public API request failed");

    assert!(!error.is_actionable());
}
