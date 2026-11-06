use serde_json::json;

use super::*;

#[test]
fn prefers_allow_always_then_allow_once() {
    let params = json!({
        "options": [
            { "optionId": "reject", "kind": "reject_once", "name": "Reject" },
            { "optionId": "once", "kind": "allow_once", "name": "Allow" },
            { "optionId": "always", "kind": "allow_always", "name": "Always allow" },
        ]
    });
    assert_eq!(allow_option(&params).as_deref(), Some("always"));

    let params = json!({
        "options": [
            { "optionId": "reject", "kind": "reject_once" },
            { "optionId": "once", "kind": "allow_once" },
        ]
    });
    assert_eq!(allow_option(&params).as_deref(), Some("once"));

    let params = json!({ "options": [{ "optionId": "reject", "kind": "reject_once" }] });
    assert_eq!(allow_option(&params), None);
}

#[test]
fn reads_http_mcp_support_from_initialize() {
    let claude_code = json!({
        "protocolVersion": 1,
        "agentCapabilities": { "mcpCapabilities": { "http": true, "sse": true } },
    });
    assert!(supports_http_mcp(&claude_code));
    let stdio_only = json!({ "agentCapabilities": { "mcpCapabilities": { "http": false } } });
    assert!(!supports_http_mcp(&stdio_only));
    assert!(!supports_http_mcp(&json!({})));
}

#[test]
fn only_existing_absolute_folders_are_used() {
    let here = std::env::current_dir().unwrap();
    let here = here.to_str().unwrap();
    assert_eq!(usable_directory(Some(here)).as_deref(), Some(here));
    assert_eq!(usable_directory(Some("relative/path")), None);
    assert_eq!(
        usable_directory(Some("/definitely/not/a/real/folder")),
        None
    );
    assert_eq!(usable_directory(None), None);
}
