use std::sync::Mutex;

use futures::FutureExt as _;

use super::*;

/// A search that records what it was asked and answers with `answer`.
fn recording_search(answer: Result<String, String>) -> (SearchFn, Arc<Mutex<Vec<SearchRequest>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = seen.clone();
    let search: SearchFn = Arc::new(move |request| {
        recorded.lock().unwrap().push(request);
        let answer = answer.clone();
        async move { answer }.boxed()
    });
    (search, seen)
}

fn call(arguments: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "tools/call",
        "params": { "name": TOOL_NAME, "arguments": arguments },
    })
}

fn text_of(response: &Value) -> &str {
    response["result"]["content"][0]["text"].as_str().unwrap()
}

#[tokio::test]
async fn initialize_advertises_tools() {
    let (search, _) = recording_search(Ok(String::new()));
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": { "protocolVersion": "2025-03-26", "capabilities": {} },
    });
    let response = handle_message(&request, "/repo", &search).await.unwrap();
    assert_eq!(response["id"], 1);
    assert!(response["result"]["capabilities"]["tools"].is_object());
    assert_eq!(response["result"]["serverInfo"]["name"], SERVER_NAME);
}

#[tokio::test]
async fn notifications_get_no_response() {
    let (search, _) = recording_search(Ok(String::new()));
    let notification = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    assert!(
        handle_message(&notification, "/repo", &search)
            .await
            .is_none()
    );
}

#[tokio::test]
async fn lists_the_search_tool() {
    let (search, _) = recording_search(Ok(String::new()));
    let request = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" });
    let response = handle_message(&request, "/repo", &search).await.unwrap();
    let tools = response["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], TOOL_NAME);
    assert_eq!(tools[0]["inputSchema"]["required"][0], "query");
}

#[tokio::test]
async fn search_runs_in_the_session_folder() {
    let (search, seen) = recording_search(Ok("app/src/tab.rs:10-20".into()));
    let response = handle_message(&call(json!({ "query": " tab drag " })), "/repo", &search)
        .await
        .unwrap();
    assert_eq!(text_of(&response), "app/src/tab.rs:10-20");
    assert_eq!(response["result"]["isError"], false);
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        [SearchRequest {
            query: "tab drag".into(),
            directory: "/repo".into(),
        }]
    );
}

#[tokio::test]
async fn path_argument_overrides_the_session_folder() {
    let (search, seen) = recording_search(Ok(String::new()));
    handle_message(
        &call(json!({ "query": "parser", "path": "/other" })),
        "/repo",
        &search,
    )
    .await
    .unwrap();
    assert_eq!(seen.lock().unwrap()[0].directory, "/other");
}

#[tokio::test]
async fn failures_come_back_as_tool_errors() {
    let (search, _) = recording_search(Err("This folder isn't indexed.".into()));
    let response = handle_message(&call(json!({ "query": "parser" })), "/repo", &search)
        .await
        .unwrap();
    assert_eq!(response["result"]["isError"], true);
    assert_eq!(text_of(&response), "This folder isn't indexed.");

    let (search, seen) = recording_search(Ok(String::new()));
    let response = handle_message(&call(json!({ "query": "  " })), "/repo", &search)
        .await
        .unwrap();
    assert_eq!(response["result"]["isError"], true);
    assert!(seen.lock().unwrap().is_empty());

    let response = handle_message(&call(json!({ "query": "parser" })), "", &search)
        .await
        .unwrap();
    assert_eq!(response["result"]["isError"], true);
}

#[tokio::test]
async fn unknown_methods_and_tools_are_errors() {
    let (search, _) = recording_search(Ok(String::new()));
    let request = json!({ "jsonrpc": "2.0", "id": 3, "method": "resources/list" });
    let response = handle_message(&request, "/repo", &search).await.unwrap();
    assert_eq!(response["error"]["code"], -32601);

    let request = json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": { "name": "rm_rf", "arguments": {} },
    });
    let response = handle_message(&request, "/repo", &search).await.unwrap();
    assert_eq!(response["error"]["code"], -32602);
}

#[test]
fn recognises_permission_requests_for_its_own_tool() {
    let claude_code = json!({
        "sessionId": "s",
        "toolCall": {
            "toolCallId": "t",
            "title": "mcp__uncaged__search_codebase",
            "rawInput": { "query": "tabs" },
        },
        "options": [],
    });
    assert!(is_own_tool_permission(&claude_code));

    let gemini = json!({
        "toolCall": { "title": "search_codebase (uncaged MCP Server)" },
    });
    assert!(is_own_tool_permission(&gemini));

    let bash = json!({
        "toolCall": {
            "title": "rm -rf build",
            // The input is the agent's own text and must not count.
            "rawInput": { "command": "echo mcp__uncaged__search_codebase" },
        },
    });
    assert!(!is_own_tool_permission(&bash));
    assert!(!is_own_tool_permission(&json!({})));
}
