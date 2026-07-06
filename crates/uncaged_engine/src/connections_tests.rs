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
    assert_eq!(id1, id2, "connecting the same preset twice reuses the connection");
    let r = connections::load();
    assert_eq!(r.connections.len(), 1, "no duplicate created");
    assert_eq!(r.active_id.as_deref(), Some(id1.as_str()));

    // `custom` is exempt — each custom endpoint is a distinct connection.
    connections::connect_or_focus("custom").unwrap();
    connections::connect_or_focus("custom").unwrap();
    let r = connections::load();
    assert_eq!(
        r.connections.iter().filter(|c| c.preset == "custom").count(),
        2,
        "custom presets are allowed to repeat"
    );
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
    assert!(!r.connections.iter().any(|c| c.id == "lmstudio" || c.id == "lmstudio-2"));
}
