//! Tests for the connections roster and its projection into `engine.json`.
//!
//! The disk-touching tests share process-global env vars (`UNCAGED_CONFIG`,
//! `UNCAGED_CONNECTIONS`) and the config module's in-memory cache, so they run
//! under a mutex. Each gets its own temp directory.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

use crate::config;
use crate::config::ProviderConfig;
use crate::config::UncagedConfig;
use crate::connections;

static ENV_LOCK: Mutex<()> = Mutex::new(());
static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A unique temp dir + `UNCAGED_CONFIG` / `UNCAGED_CONNECTIONS` pointed into it.
/// Returns the dir; the caller holds the env lock for the test's duration.
fn isolate() -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("uncaged-test-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // SAFETY: all env-mutating tests hold ENV_LOCK, so no other thread reads or
    // writes these vars concurrently.
    unsafe {
        std::env::set_var("UNCAGED_CONFIG", dir.join("engine.json"));
        std::env::set_var("UNCAGED_CONNECTIONS", dir.join("connections.json"));
        // Make sure a stray UNCAGED_* provider override in the environment can't
        // leak into these tests.
        for k in ["UNCAGED_PROVIDER", "UNCAGED_ENABLED", "UNCAGED_API_KEY"] {
            std::env::remove_var(k);
        }
    }
    dir
}

// ── pure logic (no disk) ────────────────────────────────────────────────────

#[test]
fn usable_rules() {
    let cli = connections::Connection {
        id: "c".into(),
        preset: "claude-code".into(),
        label: "Claude Code".into(),
        wire: "cli".into(),
        base_url: String::new(),
        api_key: None,
        model: "sonnet".into(),
        cli_command: vec!["claude-code-acp".into()],
        max_tokens: 8192,
        needs_key: false,
        local: true,
    };
    assert!(cli.usable(), "a CLI with a command is usable");

    let mut cli_empty = cli.clone();
    cli_empty.cli_command.clear();
    assert!(!cli_empty.usable(), "a CLI with no command is not usable");

    let needs_key = connections::Connection {
        id: "o".into(),
        preset: "openai".into(),
        label: "OpenAI".into(),
        wire: "openai_compatible".into(),
        base_url: "https://api.openai.com/v1".into(),
        api_key: None,
        model: "gpt-4o".into(),
        cli_command: vec![],
        max_tokens: 8192,
        needs_key: true,
        local: false,
    };
    assert!(!needs_key.usable(), "missing required key -> not usable");
    assert_eq!(needs_key.status(), "Needs key");

    let mut with_key = needs_key.clone();
    with_key.api_key = Some("sk-123".into());
    assert!(with_key.usable());
    assert_eq!(with_key.status(), "Ready");
}

#[test]
fn to_provider_maps_each_wire() {
    let anthropic = connections::Connection {
        id: "a".into(),
        preset: "anthropic".into(),
        label: "Anthropic".into(),
        wire: "anthropic".into(),
        base_url: String::new(),
        api_key: Some("sk-ant".into()),
        model: "claude-sonnet-4-5".into(),
        cli_command: vec![],
        max_tokens: 4096,
        needs_key: true,
        local: false,
    };
    match anthropic.to_provider() {
        ProviderConfig::Anthropic {
            model,
            base_url,
            max_tokens,
            ..
        } => {
            assert_eq!(model, "claude-sonnet-4-5");
            // Empty base URL falls back to the public Anthropic endpoint.
            assert_eq!(base_url, config::ANTHROPIC_DEFAULT_BASE_URL);
            assert_eq!(max_tokens, 4096);
        }
        other => panic!("expected Anthropic, got {other:?}"),
    }

    let cli = connections::Connection {
        id: "c".into(),
        preset: "gemini-cli".into(),
        label: "Gemini".into(),
        wire: "cli".into(),
        base_url: String::new(),
        api_key: None,
        model: String::new(),
        cli_command: vec!["gemini".into(), "--experimental-acp".into()],
        max_tokens: 8192,
        needs_key: false,
        local: true,
    };
    match cli.to_provider() {
        ProviderConfig::Acp { command, model } => {
            assert_eq!(command, vec!["gemini", "--experimental-acp"]);
            assert!(model.is_none(), "blank model -> None (use CLI default)");
        }
        other => panic!("expected Acp, got {other:?}"),
    }
}

// ── full roster flow (disk, serialized) ─────────────────────────────────────

#[test]
fn roster_flow_projects_active_into_engine_json() {
    let _guard = ENV_LOCK.lock().unwrap();
    isolate();

    // A local model is usable immediately -> becomes active + projected.
    let id = connections::add("lmstudio").unwrap();
    assert_eq!(id, "lmstudio");
    let r = connections::load();
    assert_eq!(r.connections.len(), 1);
    assert_eq!(r.active_id.as_deref(), Some("lmstudio"));

    let engine = config::read_persisted().expect("engine.json written for a usable active");
    assert!(engine.enabled);
    match engine.provider {
        ProviderConfig::OpenAiCompatible { base_url, .. } => {
            assert_eq!(base_url, "http://localhost:1234/v1");
        }
        other => panic!("expected OpenAiCompatible, got {other:?}"),
    }

    // Add a key-less Anthropic: not usable yet, doesn't steal active.
    connections::add("anthropic").unwrap();
    let r = connections::load();
    assert_eq!(r.connections.len(), 2);
    assert_eq!(r.active_id.as_deref(), Some("lmstudio"), "active unchanged");

    // Switch active to the not-yet-usable Anthropic -> engine sleeps.
    connections::set_active("anthropic").unwrap();
    assert!(
        config::read_persisted().is_none(),
        "an unconfigured active connection puts the engine to sleep"
    );

    // Configure it -> becomes usable -> projected as Anthropic.
    connections::update(
        "anthropic",
        "My Claude".into(),
        String::new(),
        "claude-opus-4-8".into(),
        Some("sk-ant-live".into()),
        vec![],
    )
    .unwrap();
    let engine = config::read_persisted().expect("engine.json rewritten once usable");
    match engine.provider {
        ProviderConfig::Anthropic { model, .. } => assert_eq!(model, "claude-opus-4-8"),
        other => panic!("expected Anthropic, got {other:?}"),
    }

    // Removing the active one falls back to the remaining usable connection.
    connections::remove("anthropic").unwrap();
    let r = connections::load();
    assert_eq!(r.active_id.as_deref(), Some("lmstudio"));
    match config::read_persisted().unwrap().provider {
        ProviderConfig::OpenAiCompatible { base_url, .. } => {
            assert_eq!(base_url, "http://localhost:1234/v1")
        }
        other => panic!("expected OpenAiCompatible fallback, got {other:?}"),
    }
}

#[test]
fn cli_connection_round_trips_edited_command() {
    // The whole point of the CLI editor: the edited command line (invocation +
    // flags) must persist onto the connection and reach the ACP provider. This
    // walks the same path the settings modal drives — `update` with the command
    // as argv — then reads it back both from the roster and from the projection
    // into engine.json.
    let _guard = ENV_LOCK.lock().unwrap();
    isolate();

    // A CLI connection is usable as soon as it has a command, so it becomes
    // active + projected immediately.
    let id = connections::add("claude-code").unwrap();
    assert_eq!(id, "claude-code");

    // Edit the command to add flags (what "choose model/thinking" boils down to)
    // and pick a model. `commit_uncaged_connection` splits on whitespace; mirror
    // that here. base_url stays empty and the key is irrelevant for a CLI.
    let edited: Vec<String> = "claude-code-acp --model opus --thinking"
        .split_whitespace()
        .map(str::to_string)
        .collect();
    connections::update(
        &id,
        "My Claude".into(),
        String::new(),
        "opus".into(),
        Some(String::new()),
        edited.clone(),
    )
    .unwrap();

    // Persisted onto the roster connection as argv (base_url untouched).
    let conn = connections::load()
        .connections
        .into_iter()
        .find(|c| c.id == id)
        .expect("connection still present");
    assert_eq!(conn.cli_command, edited, "edited command persisted as argv");
    assert!(conn.base_url.is_empty(), "CLI base_url stays empty");

    // And it reaches the ACP provider projected into engine.json.
    let engine = config::read_persisted().expect("engine.json written for a usable CLI");
    match engine.provider {
        ProviderConfig::Acp { command, model } => {
            assert_eq!(command, edited, "ACP provider spawns the edited command");
            assert_eq!(model.as_deref(), Some("opus"), "model flows through to ACP");
        }
        other => panic!("expected Acp, got {other:?}"),
    }
}

#[test]
fn seeds_roster_from_existing_engine_json() {
    let _guard = ENV_LOCK.lock().unwrap();
    isolate();

    // Simulate a setup made by hand / by the setup script, using 127.0.0.1
    // (what people actually type) rather than the preset's `localhost`.
    config::save(&UncagedConfig {
        enabled: true,
        provider: ProviderConfig::OpenAiCompatible {
            base_url: "http://127.0.0.1:1234/v1".into(),
            api_key: None,
            model: "local-model".into(),
            max_tokens: 8192,
            label: Some("lmstudio".into()),
        },
    })
    .unwrap();

    // No connections.json yet -> first open seeds it from engine.json.
    assert!(connections::load().connections.is_empty());
    let seeded = connections::load_or_seed();
    assert_eq!(seeded.connections.len(), 1, "seeded one connection");
    let conn = &seeded.connections[0];
    assert_eq!(
        conn.preset, "lmstudio",
        "matched the LM Studio preset despite the 127.0.0.1 vs localhost host"
    );
    assert!(conn.local);
    assert_eq!(seeded.active_id.as_deref(), Some(conn.id.as_str()));
    // Seeding must not disturb the existing engine.json.
    assert!(config::read_persisted().unwrap().enabled);
}

#[test]
fn connect_or_focus_never_duplicates() {
    let _guard = ENV_LOCK.lock().unwrap();
    isolate();

    let id1 = connections::connect_or_focus("lmstudio").unwrap();
    let id2 = connections::connect_or_focus("lmstudio").unwrap();
    assert_eq!(
        id1, id2,
        "connecting the same preset twice reuses the connection"
    );
    let r = connections::load();
    assert_eq!(r.connections.len(), 1, "no duplicate created");
    assert_eq!(r.active_id.as_deref(), Some(id1.as_str()));

    // `custom` is exempt — each custom endpoint is a distinct connection.
    let custom1 = connections::connect_or_focus("custom").unwrap();
    configure_custom(&custom1, "http://localhost:8000/v1", "model-a");
    connections::connect_or_focus("custom").unwrap();
    let r = connections::load();
    assert_eq!(
        r.connections
            .iter()
            .filter(|c| c.preset == "custom")
            .count(),
        2,
        "custom presets are allowed to repeat"
    );
}

/// Give a custom connection the URL + model it needs to be usable, keeping the
/// label it was created with.
fn configure_custom(id: &str, base_url: &str, model: &str) {
    let label = connections::load()
        .connections
        .into_iter()
        .find(|c| c.id == id)
        .unwrap()
        .label;
    connections::update(
        id,
        label,
        base_url.to_string(),
        model.to_string(),
        None,
        Vec::new(),
    )
    .unwrap();
}

#[test]
fn only_custom_preset_allows_multiple_connections() {
    assert!(crate::catalog::allows_multiple("custom"));
    for preset in crate::catalog::PRESETS.iter().filter(|p| p.id != "custom") {
        assert!(
            !crate::catalog::allows_multiple(preset.id),
            "{} should stay one-connection-per-preset",
            preset.id
        );
    }
}

#[test]
fn multiple_custom_connections_are_distinct_and_keep_the_active_one() {
    let _guard = ENV_LOCK.lock().unwrap();
    isolate();

    let first = connections::connect_or_focus("custom").unwrap();
    configure_custom(&first, "http://localhost:8000/v1", "model-a");
    connections::set_active(&first).unwrap();

    let second = connections::connect_or_focus("custom").unwrap();
    assert_ne!(first, second, "a second custom connection was added");
    configure_custom(&second, "https://proxy.example.com/v1", "model-b");

    let third = connections::connect_or_focus("custom").unwrap();
    assert_ne!(third, first);
    assert_ne!(third, second);

    let r = connections::load_or_seed();
    let customs: Vec<_> = r
        .connections
        .iter()
        .filter(|c| c.preset == "custom")
        .collect();
    assert_eq!(
        customs.len(),
        3,
        "custom connections survive the dedupe pass"
    );

    let labels: std::collections::HashSet<&str> =
        customs.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(
        labels.len(),
        3,
        "each custom connection gets a distinct label"
    );
    assert_eq!(customs[0].label, "Custom (OpenAI-compatible)");
    assert_eq!(customs[1].label, "Custom (OpenAI-compatible) 2");

    // Adding more custom endpoints never steals the active connection, and the
    // engine keeps running against it.
    assert_eq!(r.active_id.as_deref(), Some(first.as_str()));
    match config::read_persisted().unwrap().provider {
        ProviderConfig::OpenAiCompatible {
            base_url, model, ..
        } => {
            assert_eq!(base_url, "http://localhost:8000/v1");
            assert_eq!(model, "model-a");
        }
        other => panic!("expected the first custom endpoint, got {other:?}"),
    }
}

#[test]
fn connecting_custom_reuses_an_unfinished_custom_connection() {
    let _guard = ENV_LOCK.lock().unwrap();
    isolate();

    // Clicking Connect and dismissing the editor leaves a blank custom row;
    // clicking Connect again should resume it rather than add another.
    let blank = connections::connect_or_focus("custom").unwrap();
    let again = connections::connect_or_focus("custom").unwrap();
    assert_eq!(blank, again, "the unfinished custom connection was reused");
    assert_eq!(connections::load().connections.len(), 1);
}

#[test]
fn reusing_a_custom_draft_leaves_the_running_connection_active() {
    let _guard = ENV_LOCK.lock().unwrap();
    isolate();

    let lmstudio = connections::connect_or_focus("lmstudio").unwrap();
    connections::set_active(&lmstudio).unwrap();
    let engine_before = std::fs::read_to_string(config::config_path()).unwrap();

    let draft = connections::connect_or_focus("custom").unwrap();
    let again = connections::connect_or_focus("custom").unwrap();
    assert_eq!(draft, again, "the blank custom draft was reused");

    let r = connections::load();
    assert_eq!(r.active_id.as_deref(), Some(lmstudio.as_str()));
    assert_eq!(
        std::fs::read_to_string(config::config_path()).unwrap(),
        engine_before,
        "engine.json still points at the running connection"
    );
}

#[test]
fn partially_configured_custom_rows_are_not_treated_as_drafts() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = isolate();

    // A custom endpoint with a URL + key but no model (e.g. seeded from a
    // hand-written engine.json), and an unrecognised CLI command, which is
    // stored under the `custom` fallback preset. Neither is usable, and neither
    // is a blank draft: Connect must add a fresh row, never hand these out.
    let roster = r#"{
      "active_id": null,
      "connections": [
        {"id":"custom","preset":"custom","label":"Company proxy","wire":"openai_compatible","base_url":"https://proxy.example.com/v1","api_key":"secret","model":"","max_tokens":8192,"needs_key":false,"local":false},
        {"id":"cli-agent","preset":"custom","label":"my-agent","wire":"cli","base_url":"","model":"","cli_command":[],"max_tokens":8192,"needs_key":false,"local":true}
      ]
    }"#;
    std::fs::write(dir.join("connections.json"), roster).unwrap();

    let id = connections::connect_or_focus("custom").unwrap();
    assert_ne!(id, "custom");
    assert_ne!(id, "cli-agent");
    let r = connections::load();
    assert_eq!(r.connections.len(), 3, "a new custom row was added");
    let added = r.connections.iter().find(|c| c.id == id).unwrap();
    assert_eq!(added.wire, "openai_compatible");
    assert!(added.api_key.is_none(), "the new row carries no credential");
    let proxy = r.connections.iter().find(|c| c.id == "custom").unwrap();
    assert_eq!(proxy.base_url, "https://proxy.example.com/v1");
    assert_eq!(proxy.api_key.as_deref(), Some("secret"));
}

#[test]
fn changing_the_server_drops_the_stored_key_unless_a_new_one_is_given() {
    let _guard = ENV_LOCK.lock().unwrap();
    isolate();

    let id = connections::connect_or_focus("custom").unwrap();
    let update = |url: &str, key: Option<&str>| {
        connections::update(
            &id,
            "Proxy".to_string(),
            url.to_string(),
            "model-a".to_string(),
            key.map(str::to_string),
            Vec::new(),
        )
        .unwrap()
    };
    let stored_key = || {
        connections::load()
            .connections
            .into_iter()
            .find(|c| c.id == id)
            .unwrap()
            .api_key
    };

    update("https://proxy.example.com/v1", Some("secret"));
    assert_eq!(stored_key().as_deref(), Some("secret"));

    // Same server, different path, blank key field: the key is kept.
    update("https://proxy.example.com/v2", Some(""));
    assert_eq!(stored_key().as_deref(), Some("secret"));

    // A different server with a blank key field: the old key is not carried
    // over, and the caller is told so.
    let outcome = update("http://192.168.1.50:8000/v1", Some(""));
    assert!(outcome.key_cleared);
    assert_eq!(stored_key(), None);

    // A different server with a new key: the new key is stored.
    update("https://other.example.com/v1", Some("other-secret"));
    assert_eq!(stored_key().as_deref(), Some("other-secret"));

    // Spelling out the default port, or surrounding whitespace, is the same
    // server: the key is kept (and the stored URL is trimmed).
    update(" https://other.example.com:443/v1 ", Some(""));
    assert_eq!(stored_key().as_deref(), Some("other-secret"));
    let stored_url = connections::load()
        .connections
        .into_iter()
        .find(|c| c.id == id)
        .unwrap()
        .base_url;
    assert_eq!(stored_url, "https://other.example.com:443/v1");

    // Extra slashes after the scheme still name a host (the HTTP client's URL
    // parser ignores them), so moving between such URLs is a server change.
    update("https:///trusted.example.com/v1", Some("trusted-secret"));
    assert_eq!(stored_key().as_deref(), Some("trusted-secret"));
    update("https:///attacker.example.com/v1", Some(""));
    assert_eq!(stored_key(), None);

    // A URL that doesn't parse never keeps a key...
    update("https://other.example.com/v1", Some("other-secret"));
    update("not a url", Some(""));
    assert_eq!(stored_key(), None);

    // ...but re-saving an unchanged URL (e.g. only the model changed, as the
    // model picker does) keeps the key even if that URL doesn't parse.
    update("not a url", Some("kept-secret"));
    let outcome = update("not a url", Some(""));
    assert!(!outcome.key_cleared);
    assert_eq!(stored_key().as_deref(), Some("kept-secret"));
}

#[test]
fn dedupe_collapses_existing_duplicate_roster() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = isolate();

    // Mirror the real-world mess: lmstudio ×3, claude-code ×2, active = lmstudio-3.
    let messy = r#"{
      "active_id": "lmstudio-3",
      "connections": [
        {"id":"lmstudio","preset":"lmstudio","label":"lmstudio","wire":"openai_compatible","base_url":"http://127.0.0.1:1234/v1","model":"qwen3-coder","max_tokens":8192,"needs_key":false,"local":true},
        {"id":"claude-code","preset":"claude-code","label":"Claude Code","wire":"cli","base_url":"","model":"sonnet","cli_command":["claude-code-acp"],"max_tokens":8192,"needs_key":false,"local":true},
        {"id":"claude-code-2","preset":"claude-code","label":"Claude Code","wire":"cli","base_url":"","model":"sonnet","cli_command":["claude-code-acp"],"max_tokens":8192,"needs_key":false,"local":true},
        {"id":"codex","preset":"codex","label":"Codex CLI","wire":"cli","base_url":"","model":"","cli_command":["codex","acp"],"max_tokens":8192,"needs_key":false,"local":true},
        {"id":"lmstudio-2","preset":"lmstudio","label":"LM Studio (local)","wire":"openai_compatible","base_url":"http://localhost:1234/v1","model":"local-model","max_tokens":8192,"needs_key":false,"local":true},
        {"id":"openrouter","preset":"openrouter","label":"OpenRouter","wire":"openai_compatible","base_url":"https://openrouter.ai/api/v1","model":"anthropic/claude-sonnet-4-5","max_tokens":8192,"needs_key":true,"local":false},
        {"id":"lmstudio-3","preset":"lmstudio","label":"LM Studio (local)","wire":"openai_compatible","base_url":"http://localhost:1234/v1","model":"local-model","max_tokens":8192,"needs_key":false,"local":true}
      ]
    }"#;
    std::fs::write(dir.join("connections.json"), messy).unwrap();

    // load_or_seed self-heals: one connection per preset, active preserved.
    let r = connections::load_or_seed();
    assert_eq!(r.connections.len(), 4, "collapsed to one per preset");
    assert_eq!(r.active_id.as_deref(), Some("lmstudio-3"), "active kept");
    let presets: Vec<&str> = r.connections.iter().map(|c| c.preset.as_str()).collect();
    assert_eq!(presets.iter().filter(|p| **p == "lmstudio").count(), 1);
    assert_eq!(presets.iter().filter(|p| **p == "claude-code").count(), 1);
    // The active lmstudio row survives; the other two are gone.
    assert!(r.connections.iter().any(|c| c.id == "lmstudio-3"));
    assert!(
        !r.connections
            .iter()
            .any(|c| c.id == "lmstudio" || c.id == "lmstudio-2")
    );
}

#[test]
fn moving_a_key_required_connection_to_another_server_needs_a_new_key() {
    let _guard = ENV_LOCK.lock().unwrap();
    isolate();

    // A working OpenAI connection, running.
    let id = connections::connect_or_focus("openai").unwrap();
    connections::update(
        &id,
        "OpenAI".to_string(),
        "https://api.openai.com/v1".to_string(),
        "gpt-4o".to_string(),
        Some("sk-original".to_string()),
        Vec::new(),
    )
    .unwrap();
    connections::set_active(&id).unwrap();
    let roster_before = std::fs::read_to_string(connections::roster_path()).unwrap();
    let engine_before = std::fs::read_to_string(config::config_path()).unwrap();

    // Pointing it at another server with a blank key field is refused, and
    // nothing is written: the running model keeps running.
    let err = connections::update(
        &id,
        "OpenAI".to_string(),
        "https://proxy.example.com/v1".to_string(),
        "gpt-4o".to_string(),
        Some(String::new()),
        Vec::new(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("API key"), "{err}");
    assert_eq!(
        std::fs::read_to_string(connections::roster_path()).unwrap(),
        roster_before
    );
    assert_eq!(
        std::fs::read_to_string(config::config_path()).unwrap(),
        engine_before
    );
    assert!(config::read_persisted().unwrap().enabled);

    // With a new key the move goes through.
    let outcome = connections::update(
        &id,
        "OpenAI".to_string(),
        "https://proxy.example.com/v1".to_string(),
        "gpt-4o".to_string(),
        Some("sk-proxy".to_string()),
        Vec::new(),
    )
    .unwrap();
    assert!(!outcome.key_cleared);
    let conn = connections::load()
        .connections
        .into_iter()
        .find(|c| c.id == id)
        .unwrap();
    assert_eq!(conn.api_key.as_deref(), Some("sk-proxy"));
    assert!(conn.usable());
}

#[test]
fn a_draft_claimed_twice_saves_to_separate_rows() {
    let _guard = ENV_LOCK.lock().unwrap();
    isolate();

    // Two editors on the same blank draft (e.g. Edit on a row onboarding left,
    // in two windows).
    let draft = connections::connect_or_focus("custom").unwrap();
    assert_eq!(connections::connect_or_focus("custom").unwrap(), draft);
    assert!(connections::is_draft(&draft));

    // Window A saves first: the draft is still untouched, so A keeps it.
    let a = connections::claim_draft(&draft, "custom").unwrap();
    assert_eq!(a, draft);
    configure_custom(&a, "http://localhost:8000/v1", "model-a");
    assert!(!connections::is_draft(&a));

    // Window B saves next: the draft has been configured, so B gets a new row
    // and A's endpoint survives.
    let b = connections::claim_draft(&draft, "custom").unwrap();
    assert_ne!(b, a);
    configure_custom(&b, "http://192.168.1.50:8000/v1", "model-b");

    let r = connections::load();
    let url_of = |id: &str| {
        r.connections
            .iter()
            .find(|c| c.id == id)
            .unwrap()
            .base_url
            .clone()
    };
    assert_eq!(url_of(&a), "http://localhost:8000/v1");
    assert_eq!(url_of(&b), "http://192.168.1.50:8000/v1");

    // A draft removed in the meantime is also replaced by a fresh row.
    let gone = connections::connect_or_focus("custom").unwrap();
    connections::remove(&gone).unwrap();
    let fresh = connections::claim_draft(&gone, "custom").unwrap();
    assert!(
        connections::load()
            .connections
            .iter()
            .any(|c| c.id == fresh)
    );

    // Only drafts of repeatable presets count.
    let lmstudio = connections::connect_or_focus("lmstudio").unwrap();
    assert!(!connections::is_draft(&lmstudio));
}

/// Whether the on-disk roster has a connection with this id.
fn in_roster(id: &str) -> bool {
    connections::load().connections.iter().any(|c| c.id == id)
}

#[test]
fn discard_draft_removes_only_an_untouched_row() {
    let _guard = ENV_LOCK.lock().unwrap();
    isolate();

    // A working connection the user already has.
    let lmstudio = connections::connect_or_focus("lmstudio").unwrap();
    assert_eq!(
        connections::load().active_id.as_deref(),
        Some(lmstudio.as_str())
    );

    // A blank row for a key provider (e.g. added by a save that then failed)
    // is taken back out, and the running connection stays active.
    let openai = connections::connect_or_focus("openai").unwrap();
    assert!(connections::discard_draft(&openai).unwrap());
    assert!(!in_roster(&openai));
    assert_eq!(
        connections::load().active_id.as_deref(),
        Some(lmstudio.as_str())
    );
    // Discarding again is a no-op.
    assert!(!connections::discard_draft(&openai).unwrap());

    // Same for a blank custom endpoint.
    let custom = connections::connect_or_focus("custom").unwrap();
    assert!(connections::discard_draft(&custom).unwrap());
    assert!(!in_roster(&custom));

    // A row configured since (another window saved it, or a failed save had
    // already written to it) is kept, and nothing is written.
    let custom = connections::connect_or_focus("custom").unwrap();
    configure_custom(&custom, "http://localhost:8000/v1", "qwen");
    let before = std::fs::read_to_string(connections::roster_path()).unwrap();
    assert!(!connections::discard_draft(&custom).unwrap());
    assert!(in_roster(&custom));
    assert_eq!(
        std::fs::read_to_string(connections::roster_path()).unwrap(),
        before
    );

    // So is a key provider whose key was entered.
    let openai = connections::connect_or_focus("openai").unwrap();
    let preset = crate::catalog::preset_by_id("openai");
    connections::update(
        &openai,
        "OpenAI".to_string(),
        preset.base_url.to_string(),
        preset.model.to_string(),
        Some("sk-test".to_string()),
        Vec::new(),
    )
    .unwrap();
    assert!(!connections::discard_draft(&openai).unwrap());
    assert!(in_roster(&openai));

    // The user's working connection was never touched.
    assert!(in_roster(&lmstudio));
}

#[test]
fn discarding_the_active_draft_repoints_the_active_connection() {
    let _guard = ENV_LOCK.lock().unwrap();
    isolate();

    // On an empty roster the first connection becomes active even unfinished;
    // discarding it as the only row leaves none active.
    let openai = connections::add("openai").unwrap();
    assert_eq!(
        connections::load().active_id.as_deref(),
        Some(openai.as_str())
    );
    assert!(connections::discard_draft(&openai).unwrap());
    let r = connections::load();
    assert!(r.connections.is_empty());
    assert_eq!(r.active_id, None);

    // With other rows, the first usable one takes over...
    let openai = connections::add("openai").unwrap();
    let custom = connections::add("custom").unwrap();
    let lmstudio = connections::add("lmstudio").unwrap();
    assert!(connections::discard_draft(&openai).unwrap());
    assert_eq!(
        connections::load().active_id.as_deref(),
        Some(lmstudio.as_str())
    );

    // ...else the first remaining one.
    connections::remove(&lmstudio).unwrap();
    let openai = connections::add("openai").unwrap();
    connections::set_active(&openai).unwrap();
    assert!(connections::discard_draft(&openai).unwrap());
    assert_eq!(
        connections::load().active_id.as_deref(),
        Some(custom.as_str())
    );
}

#[test]
fn saving_a_new_connection_picks_its_row_without_switching_the_active_one() {
    let _guard = ENV_LOCK.lock().unwrap();
    isolate();

    let lmstudio = connections::connect_or_focus("lmstudio").unwrap();
    let engine_before = std::fs::read_to_string(config::config_path()).unwrap();

    // A key provider with no connection yet gets a new row; the running
    // connection stays active and engine.json isn't rewritten.
    let (openai, created) = connections::claim_for_preset("openai").unwrap();
    assert!(created);
    assert_eq!(
        connections::load().active_id.as_deref(),
        Some(lmstudio.as_str())
    );
    assert_eq!(
        std::fs::read_to_string(config::config_path()).unwrap(),
        engine_before
    );

    // Once it exists (e.g. another window saved it first), a save reuses it.
    assert_eq!(
        connections::claim_for_preset("openai").unwrap(),
        (openai.clone(), false)
    );

    // Custom: an untouched draft (e.g. left by onboarding) is reused...
    let draft = connections::connect_or_focus("custom").unwrap();
    assert_eq!(
        connections::claim_for_preset("custom").unwrap(),
        (draft.clone(), false)
    );
    // ...but a configured custom endpoint never is: the next save adds a row.
    configure_custom(&draft, "http://localhost:8000/v1", "qwen");
    let (second, created) = connections::claim_for_preset("custom").unwrap();
    assert!(created);
    assert_ne!(second, draft);

    // None of this switched the running connection.
    assert_eq!(
        connections::load().active_id.as_deref(),
        Some(lmstudio.as_str())
    );
}

#[test]
fn saving_onto_a_removed_connection_fails_and_writes_nothing() {
    let _guard = ENV_LOCK.lock().unwrap();
    isolate();

    let custom = connections::connect_or_focus("custom").unwrap();
    connections::remove(&custom).unwrap();
    let before = std::fs::read_to_string(connections::roster_path()).unwrap();
    let err = connections::update(
        &custom,
        "Custom".to_string(),
        "http://localhost:8000/v1".to_string(),
        "qwen".to_string(),
        None,
        Vec::new(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("no longer exists"), "{err}");
    assert_eq!(
        std::fs::read_to_string(connections::roster_path()).unwrap(),
        before
    );
}

#[test]
fn saving_the_first_connection_does_not_activate_its_blank_row() {
    let _guard = ENV_LOCK.lock().unwrap();
    isolate();

    // A save on an empty roster adds a blank row that only becomes active once
    // the save has made it usable (the caller activates it then). Until then
    // engine.json isn't touched, so the engine never runs a blank connection.
    let (openai, created) = connections::claim_for_preset("openai").unwrap();
    assert!(created);
    let r = connections::load();
    assert_eq!(r.active_id, None);
    assert!(in_roster(&openai));
    assert!(!config::config_path().exists());

    // So is the fresh row `claim_draft` adds on an empty roster (the draft the
    // editor was opened on has been removed meanwhile).
    connections::remove(&openai).unwrap();
    assert!(connections::load().connections.is_empty());
    let fresh = connections::claim_draft("custom", "custom").unwrap();
    assert!(in_roster(&fresh));
    assert_eq!(connections::load().active_id, None);
    assert!(!config::config_path().exists());

    // `add` itself still makes the first row of an empty roster active.
    connections::remove(&fresh).unwrap();
    let first = connections::add("lmstudio").unwrap();
    assert_eq!(
        connections::load().active_id.as_deref(),
        Some(first.as_str())
    );
}

#[test]
fn a_saved_edit_whose_engine_config_fails_is_still_saved() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = isolate();

    // engine.json can't be written (its path is a directory).
    let engine_dir = dir.join("engine-dir");
    std::fs::create_dir_all(&engine_dir).unwrap();
    // SAFETY: the env lock is held (see `isolate`).
    unsafe {
        std::env::set_var("UNCAGED_CONFIG", &engine_dir);
    }

    // The roster write lands, so `update` succeeds and reports the engine.json
    // failure separately: the caller knows the edit was saved.
    let (custom, _) = connections::claim_for_preset("custom").unwrap();
    let outcome = connections::update(
        &custom,
        "Custom".to_string(),
        "http://localhost:8000/v1".to_string(),
        "qwen".to_string(),
        None,
        Vec::new(),
    )
    .unwrap();
    assert!(outcome.engine_error.is_some());
    let saved = connections::load()
        .connections
        .into_iter()
        .find(|c| c.id == custom)
        .unwrap();
    assert_eq!(saved.base_url, "http://localhost:8000/v1");
    assert_eq!(saved.model, "qwen");

    // A refused edit is still an error, with nothing written.
    let err = connections::update(
        "missing",
        "x".to_string(),
        String::new(),
        String::new(),
        None,
        Vec::new(),
    );
    assert!(err.is_err());
}

#[test]
fn a_failed_roster_write_leaves_the_previous_roster_intact() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = isolate();

    let (custom, _) = connections::claim_for_preset("custom").unwrap();
    let roster_path = dir.join("connections.json");
    let before = std::fs::read(&roster_path).unwrap();

    // The staging file can't be written (its path is a directory), so the
    // write fails before anything replaces the roster.
    let tmp = connections::roster_temp_path(&roster_path);
    std::fs::create_dir_all(&tmp).unwrap();
    let err = connections::update(
        &custom,
        "Custom".to_string(),
        "http://localhost:8000/v1".to_string(),
        "qwen".to_string(),
        None,
        Vec::new(),
    );
    assert!(err.is_err());
    assert_eq!(std::fs::read(&roster_path).unwrap(), before);

    // Once it can be written, the save lands and leaves no staging file behind.
    std::fs::remove_dir(&tmp).unwrap();
    connections::update(
        &custom,
        "Custom".to_string(),
        "http://localhost:8000/v1".to_string(),
        "qwen".to_string(),
        None,
        Vec::new(),
    )
    .unwrap();
    assert!(!tmp.exists());
    let saved = connections::load()
        .connections
        .into_iter()
        .find(|c| c.id == custom)
        .unwrap();
    assert_eq!(saved.model, "qwen");
}

#[test]
fn discarding_an_inactive_draft_does_not_touch_the_engine_config() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = isolate();

    // An active connection, then a blank row added for a save (never active).
    let active = connections::add("openai").unwrap();
    let (draft, created) = connections::claim_for_preset("custom").unwrap();
    assert!(created);
    assert_eq!(connections::load().active_id.as_deref(), Some(active.as_str()));

    // engine.json can't be written now; discarding the inactive draft doesn't
    // need it, so the cleanup succeeds.
    let engine_dir = dir.join("engine-dir");
    std::fs::create_dir_all(&engine_dir).unwrap();
    // SAFETY: the env lock is held (see `isolate`).
    unsafe {
        std::env::set_var("UNCAGED_CONFIG", &engine_dir);
    }
    assert!(connections::discard_draft(&draft).unwrap());
    let roster = connections::load();
    assert!(roster.connections.iter().all(|c| c.id != draft));
    assert_eq!(roster.active_id.as_deref(), Some(active.as_str()));
}
