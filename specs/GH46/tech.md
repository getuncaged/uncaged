# GH46: Tech Spec — Per-connection reasoning effort for Uncaged models

## Context

The product behaviour is specified in `specs/GH46/product.md`. Line references are at `main` @ `d6d12fb2`. #39 (multiple custom connections) rewrites parts of the roster and editor code referenced here, so rebase line references once it lands.

### Engine (`crates/uncaged_engine/src`)

- **`connections.rs:31-60`, `Connection`**: the persisted roster entry in `~/.uncaged/connections.json` (`roster_path`, `:234`). It has a single `model`, plus `max_tokens` (default 8192, `:26`).
- **`connections.rs:125-150`, `to_provider()`**: projects the active connection onto `ProviderConfig` in `engine.json`. `update()` (`:326`) is the edit entry point.
- **`config.rs:59-95`, `ProviderConfig`**:
  - `Anthropic { api_key, model, base_url, max_tokens }`
  - `OpenAiCompatible { base_url, api_key, model, max_tokens, label }`
  - `Acp { command, model }`
- **`providers/mod.rs:45-75`**: `build()` creates providers from that config only. Nothing from the client request reaches them.
- **`providers/openai.rs:49-59`**: the request body is `model`, `messages`, `stream`, `max_tokens`, plus `tools` and `tool_choice` when tools are present.
  - The stream reader takes only `delta.content` (`:101`) and tool-call deltas.
- **`providers/anthropic.rs:44-51`**: the request body is `model`, `max_tokens`, `system`, `messages`, `tools`, `stream`.
  - Sends `anthropic-version: 2023-06-01` (`:57`).
  - The stream reader handles `text_delta` (`:109`) and tool-use blocks. `thinking` blocks are ignored.
- **`providers/acp.rs:217-246`**: `session/new`, then a best-effort `session/set_model`. The `configOptions` in the `session/new` result are not read.
- **`catalog.rs:76-105`, `Preset`**: each preset has a static `models` list (`:93`). Presets live at `:121-349`.
- **`request_parse.rs:86-88`**: reasoning messages in history are not sent to the model.

### App

- **`app/src/uncaged.rs:41-67`, `ConnectionView`**: `connection_view` (`:196`) builds the menu's model list from `preset.models` plus the current model (`:210-213`).
- **`app/src/terminal/profile_model_selector.rs:289-340`**: `refresh_uncaged_model_menu` lists connections and the active connection's models, with no effort UI. It replaces Warp's menu whenever the engine is active (`:1079`).
  - Warp's reasoning-level submenu is `refresh_model_spec_sidecar_for_model` (`:1299`), which groups server `LLMInfo` entries by `base_model_name`.
- **`app/src/settings_view/ai_page.rs:2460-2472`, `commit_uncaged_connection`**: the editor reuses Warp's custom-endpoint modal (`app/src/settings_view/custom_inference_modal.rs`). It keeps only the first model row.
- **`script/uncaged-setup:49-60`**: the only existing `/models` discovery, done in shell.

### Backend facts

Taken from official docs as of 2026-11-27; summarized in #46.

| Backend | How effort is sent | Where levels are discovered |
|---|---|---|
| OpenAI and most OpenAI-compatible servers | top-level `reasoning_effort` (string) | — |
| OpenRouter | `reasoning: {effort}` | `GET /api/v1/models` → `reasoning.supported_efforts`, `default_effort`, `mandatory` |
| Anthropic | `thinking: {type: "adaptive"}` + `output_config: {effort}`; older models take `thinking: {type: "enabled", budget_tokens}` | `GET /v1/models` → `capabilities.effort.<level>.supported`, `capabilities.thinking.types.<type>.supported` |
| Ollama | `reasoning_effort` on `/v1` | native `POST /api/show` → `thinking.values` and `default` |
| LM Studio | `reasoning_effort` | `GET /api/v1/models` → `capabilities.reasoning.allowed_options` |
| llama.cpp | `reasoning_effort` | `GET /props` → `chat_template_caps.reasoning_effort_levels` |
| ACP | `session/set_config_option` | `session/new` → `configOptions` with category `thought_level` |

## Proposed changes

### Phase 1: storage and sending (manual levels only)

**1. Connection fields** (`connections.rs`). All fields are `#[serde(default)]` so existing files load unchanged:

```rust
/// `None` = Default: send nothing.
pub effort: Option<String>,
/// Manual levels; overrides discovery when non-empty.
pub effort_levels: Vec<String>,
/// Overrides the preset's effort style (custom endpoints).
pub effort_style: Option<EffortStyle>,
```

- `update()` gains the three fields.
- Validation on load: an `effort` string is accepted as-is. Unknown names are allowed (product invariant 7), so the only invalid values are wrong types (e.g. a number or object). Plain `#[serde(default)]` would fail the whole roster load on those, so the three fields use a lenient `deserialize_with` that logs a warning and falls back to the default (invariant 18).

**2. `EffortStyle`**, a new enum in `catalog.rs`, stored on each `Preset` as `effort_style`:
- `ReasoningEffort`: top-level `reasoning_effort`. Used for openai, groq, xai, google, deepseek, mistral, together, ollama, lmstudio and custom.
- `OpenRouterReasoning`: openrouter.
- `AnthropicOutputConfig`: anthropic.
- `AcpConfigOption`: claude-code, codex, gemini-cli.
- `None`: never send.

A Custom connection can override the style, for example to `None` for a proxy that rejects unknown parameters.

**3. Projection.**
- `ProviderConfig::{Anthropic, OpenAiCompatible, Acp}` each gain `effort: Option<String>`. `OpenAiCompatible` also gains `effort_style`.
- `to_provider()` resolves the effective level with `effective_effort(saved, levels)`, which implements product invariant 9:
  - `levels` is the manual list, else the cached discovery result, else the preset table;
  - it returns the highest supported level at or below the saved one in the fixed ordering, else `None`;
  - unknown manual names sort as exact matches only.
- Because the roster's active connection is already projected on each save/activate, the engine stays stateless.

**4. Sending.**
- **`openai.rs`**: after building `body`, match on the style:
  - `ReasoningEffort` sets `body["reasoning_effort"] = level`;
  - `OpenRouterReasoning` sets `body["reasoning"] = {"effort": level}`;
  - `None` sends nothing.
- **`anthropic.rs`**, when `effort` is `Some`:
  - Adaptive-capable models (known from the cached capability tree, or assumed when it's missing): send `thinking: {type: "adaptive"}` and `output_config: {effort: level}`.
  - Budget-only models (open question 4): send `thinking: {type: "enabled", budget_tokens: B}`, with `B` from a level table (low 1024, medium 2048, high 4096), and raise `max_tokens` to at least `B + 4096`.
  - `off` sends nothing for adaptive models where disabling is a 400. Discovery hides `off` unless `thinking.types.disabled` is supported.
  - The `anthropic-version` header stays at `2023-06-01` unless verification (Risks) shows `output_config` or `capabilities` need a newer version.
- **`acp.rs`**: after `session/new`, read `configOptions`.
  - If an option with category `thought_level` exists and `effort` is `Some`, call `session/set_config_option {sessionId, configId, value}`, best-effort like `set_model`.
  - Prefer `configOptions` with category `model` over `session/set_model` when present.

**5. Rejection retry** (product invariant 10).
- In each HTTP provider's `run()`, a non-2xx response currently `bail!`s.
- Instead, when the status is 400, effort was sent, and the body mentions one of `reasoning_effort`, `reasoning`, `output_config`, `effort` or `thinking`:
  - log, retry once without the parameter;
  - record `{connection_id, model, effort_unsupported: true}` in the discovery cache (phase 2 file; in phase 1 an in-memory set).
- Provider errors are already surfaced to the client as text (`engine.rs:119-121`). The retry happens before that, so a successful retry is invisible to the conversation.

**6. UI.**
- `ConnectionView` gains `effort`, `effort_levels` (resolved), `effective_effort` and `effort_unsupported`.
- `refresh_uncaged_model_menu` adds an effort submenu under the active model row, reusing the sidecar pattern from `refresh_model_spec_sidecar_for_model`, with a new action `SelectUncagedEffort { connection_id, effort: Option<String> }`.
- In the editor, add an effort dropdown and a levels text field to the modal when it is opened for an Uncaged connection. This touches the same code as #39; implement on top of it.

### Phase 2: discovery and model lists

**7. `crates/uncaged_engine/src/discovery.rs`**, a new module:

```rust
pub struct ModelInfo {
    id: String,
    effort: Option<EffortSpec>,
}

pub struct EffortSpec {
    levels: Vec<String>,
    default: Option<String>,
    can_disable: bool,
    adaptive: Option<bool>,
}

pub async fn discover(conn: &Connection) -> anyhow::Result<Vec<ModelInfo>>
```

It dispatches on the preset:
- **anthropic:** `GET {base}/v1/models`, paginated, reading `capabilities` (Context table).
- **openrouter:** `GET {base}/models`, reading `reasoning.*`.
- **ollama:** `GET {host}/api/tags`, then `POST {host}/api/show` per model; `{host}` is `base_url` without `/v1`.
- **lmstudio:** `GET {host}/api/v1/models`.
- **custom:** `GET {base}/models` for ids. As a best effort it also tries llama.cpp `GET {host}/props` and LiteLLM `GET {base}/model_group/info` (`supports_reasoning` only; no levels).
- **Other presets:** `GET {base}/models` for ids, with levels from a static per-preset table in `catalog.rs`, keyed by model-id prefix.
- **CLI presets:** none; levels come from `configOptions` at session start (phase 3).

Requests use the connection's base URL and key only (product invariant 14), with a short timeout. Parsers are pure functions over `serde_json::Value`, so they are unit-testable with fixtures.

**8. Cache file**, `~/.uncaged/model_cache.json`, separate from `connections.json`:

```json
{
  "<connection_id>": {
    "fetched_at": "...",
    "error": null,
    "models": [
      { "id": "...", "effort": { "...": "..." }, "effort_unsupported": false }
    ]
  }
}
```

- Kept separate because #39 added draft-claiming and concurrent-editor handling on the roster. Background discovery must not rewrite that file.
- The cache is disposable: deleting it only loses discovered data.
- Written atomically (temp file plus rename).

**9. Triggers.**
- `uncaged::connect` / `activate` / `update` (`app/src/uncaged.rs:253`, `:260`, `:275`) spawn discovery in the background.
- The editor gets a Refresh button.
- `connection_view` merges the cached model ids into `models` (product invariant 13).

### Phase 3: CLI agents

**10.** Cache the `configOptions` returned by `session/new` per connection, so the picker can show the agent's levels after the first session. Before that first session, the connection has no levels (product invariant 8).

### Phase 4 (separate issue): reasoning output

**11.**
- Parse `thinking` blocks (Anthropic) and `reasoning` / `reasoning_content` / `reasoning_details` (OpenAI-compatible) from the stream.
- Emit them to the client as `AgentReasoning` messages; the client already renders them (`app/src/ai/agent/api/convert_from.rs:279`).
- Lower them back from history in `request_parse.rs` so they are replayed on tool turns.
- Needs a place for Anthropic `signature`s. Check whether `AgentReasoning` can carry one.

## Risks

1. **Reasoning not replayed** (product open question 1). Enabling effort on Anthropic with tools, or on DeepSeek, Kimi or OpenRouter reasoning models, may cause 400s or lose interleaved reasoning in tool loops.
   - Mitigation: test each in phase 1.
   - If they fail, ship with `EffortStyle::None` for those presets until phase 4.
2. **`anthropic-version`.** Verify with a live request that `output_config.effort` and `/v1/models` `capabilities` work under `2023-06-01` before bumping it. Newer versions change other request rules, such as sampling parameters, which the engine doesn't send today.
3. **Retry heuristic.** Matching parameter names in error bodies is approximate.
   - Limit it to one retry, only when effort was sent.
   - Never retry on other statuses.
4. **Static level tables go stale.** Keep them small, keyed by prefix, and always let the manual list override.

## Testing and validation

### Unit tests

- `connections_tests.rs`:
  - an old-format roster loads with `effort = None` and empty levels;
  - a round-trip preserves the new fields;
  - `effective_effort` covers every case in product invariant 9.
- Provider request builders, extracted as pure functions:
  - **Default (all three wires):** the body equals the current output (invariant 2; success criterion 1);
  - **`ReasoningEffort` and `OpenRouterReasoning`:** the field has the expected shape;
  - **Anthropic, adaptive model:** sends `thinking` and `output_config`;
  - **Anthropic, budget-only model:** `budget_tokens` is set and `max_tokens` is raised.
- `discovery.rs` parsers: fixture JSON for Anthropic `/v1/models`, OpenRouter `/models`, Ollama `/api/show`, LM Studio `/api/v1/models`, and llama.cpp `/props`, including missing or `null` fields.
- Retry: a fake HTTP server returns 400 naming `reasoning_effort` on the first call and 200 on the second. Assert one retry, and that the second body has no effort field. A 400 without the parameter name is not retried.
- ACP:
  - a fake agent advertises a `thought_level` option; assert `session/set_config_option` is sent with the chosen value;
  - with Default, it is not sent.

### Manual testing (attach evidence to the PR, per CONTRIBUTING)

- Anthropic (one adaptive model, one budget-only model), OpenAI (one reasoning model, one non-reasoning model, to confirm the retry), OpenRouter, Ollama with a thinking model, LM Studio, and Claude Code via ACP.
- For each:
  - discovery result;
  - picker submenu;
  - request body captured with a local proxy or debug logging;
  - a multi-turn conversation with a tool call at a non-Default level, to exercise Risk 1.
