use uncaged_cli::agent::Harness;

/// Whether the harness takes an auth secret (an API key or cloud credentials) for remote runs.
pub fn harness_has_auth_secret_types(harness: Harness) -> bool {
    match harness {
        Harness::Claude | Harness::Codex => true,
        Harness::Oz | Harness::OpenCode | Harness::Gemini | Harness::Unknown => false,
    }
}
