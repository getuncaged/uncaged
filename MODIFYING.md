# Modifying Uncaged

The practical manual for changing Uncaged: how it's wired, and the exact, direct
way to **add any AI provider**, **rebrand**, **reskin**, **audit the privacy
gates**, and **build/run/ship**. Read the first two sections once; after that this
is a task-indexed cookbook — jump to the recipe you need.

For the strategic picture (why the fork exists, what we deliberately do
differently from upstream and from the sibling project Zap) see
[DIRECTION.md](DIRECTION.md). For the build/sign/DMG mechanics see
[RELEASING.md](RELEASING.md). For the fork design rationale see
[UNCAGED.md](UNCAGED.md).

---

## 1. The one idea that makes Uncaged tractable

Warp is a huge Rust monorepo (~77 crates). We did **not** rewrite it. Uncaged is
Warp's open client with **one inference seam re-pointed at your machine** and a
handful of **egress paths cut off**. Almost everything you'll ever want to change
lives in a small number of files; the other ~2,000 files are upstream Warp and
should stay untouched so we can still merge upstream fixes.

Every Agent Mode turn in Warp funnels through a single function:

```
crates/warp_multi_agent_client/src/lib.rs → generate_multi_agent_output()
```

Uncaged adds a few lines at the top of it:

```
user types ──▶ generate_multi_agent_output()
                     │
        uncaged_engine::active()?  (is a local model configured?)
             │ yes                         │ no
             ▼                             ▼
     uncaged_engine::run_turn()     on Channel::Oss: HARD-FAIL with a
             │                      "connect a model" error — NEVER POST
     your backend (API / local /    to app.warp.dev
     CLI), streamed back as the
     same ResponseEvents the UI
     already renders
```

That's the whole trick. The request builder, response renderer, **native tool
execution** (your shell, your files, your MCP servers), and the permission UI are
all stock Warp. Only the model call moved onto your machine.

---

## 2. Where everything lives

| You want to touch… | File / dir |
|---|---|
| **The inference seam** (Warp → local engine) | `crates/warp_multi_agent_client/src/lib.rs` |
| **The engine** (the whole BYO-model backend) | `crates/uncaged_engine/src/` |
| ↳ config + provider enum + `~/.uncaged/engine.json` | `uncaged_engine/src/config.rs` |
| ↳ the "Connect a model" gallery presets | `uncaged_engine/src/catalog.rs` |
| ↳ saved connections roster (`connections.json`) | `uncaged_engine/src/connections.rs` |
| ↳ provider implementations | `uncaged_engine/src/providers/{anthropic,openai,acp,sse}.rs` |
| ↳ turn orchestration / provider dispatch | `uncaged_engine/src/engine.rs`, `providers/mod.rs` |
| ↳ system prompt + tool schemas | `uncaged_engine/src/system_prompt.rs`, `tools.rs` |
| **Brand: name, URL, palette, logo paths** | `app/src/brand.rs` (single source of truth) |
| **Logo art** | `app/assets/bundled/svg/brand/uncaged-mark.svg` (in-app mono), `uncaged-icon.svg` (color app-icon master) |
| **App identity** (channel, bundle id, binary, scheme) | `crates/warp_core/src/channel/state.rs` + `mod.rs` |
| **The fail-closed server config** (sentinel URLs) | `crates/warp_core/src/channel/config.rs` |
| **The "Connect a model" Settings UI** | `app/src/settings_view/ai_page.rs` |
| **Terminal setup helper** | `script/uncaged-setup` |
| **Bundle / DMG pipeline** | `script/macos/bundle`, `script/bundle` |
| **User config at runtime** | `~/.uncaged/{engine.json, connections.json, settings.toml}` |

---

## 3. Recipe: add or change an AI provider  ← the core goal

**The mental model:** the engine has exactly three provider *shapes* in
`config.rs`'s `ProviderConfig` enum, and one of them (`OpenAiCompatible`) already
covers the entire universe of "any OpenAI Chat-Completions endpoint." So most
"new providers" are **not code at all** — they're a base URL.

### 3a. The common case — a new hosted API or local runtime that speaks OpenAI

If the provider exposes an OpenAI-compatible `/v1/chat/completions` endpoint
(OpenAI, OpenRouter, Groq, DeepSeek, Mistral, xAI, Together, Ollama, LM Studio,
llama.cpp, vLLM, and almost everything else), you add **zero engine code**. You
only add a **gallery preset** so it shows up in Settings → AI Models:

1. Open `crates/uncaged_engine/src/catalog.rs`.
2. Add a `Preset` to the `PRESETS` list: id, display name, `Group` (e.g.
   *Connect with an API key* / *Run a model locally*), default `base_url`,
   default model(s), and `Wire::OpenAiCompatible`.
3. (Optional) add a brand icon: drop an SVG in `app/assets/bundled/svg/` and add
   an `Icon` enum entry in `crates/warp_core/src/ui/icons.rs`, then reference it
   from the preset.

That's it — the user picks it in the gallery, pastes a key (or not, for local),
and it writes `~/.uncaged/engine.json` with `kind: "openai_compatible"`. No
rebuild of the engine's request path is needed.

### 3b. A provider with its own protocol (like Anthropic's Messages API)

If the API is *not* OpenAI-shaped, you add a real backend:

1. **Add a variant** to `ProviderConfig` in `config.rs` (copy the `Anthropic`
   variant as a template — note the `#[serde(rename = "…")]` tag becomes the
   `"kind"` in `engine.json`).
2. **Implement the `Provider` trait** in a new `providers/yourprovider.rs`. Look
   at `providers/anthropic.rs` for the pattern: build the HTTP request, stream
   the response, and emit `ProviderEvent`s (`Text`, `ToolCall`, `Done`, …). The
   SSE helper in `providers/sse.rs` handles server-sent-event framing.
3. **Wire the dispatch** in `providers/mod.rs`'s `build()` — add your `match` arm
   mapping the new `ProviderConfig` variant to your provider struct.
4. **Add env parsing** in `config.rs`'s `read_env_config()` so
   `UNCAGED_PROVIDER=yourprovider` works for quick testing.
5. **Add a catalog preset** (3a step 2) so it appears in the gallery.

The engine owns only *inference + tool-call events*. Warp's client still executes
every tool locally, so you never implement shell/file/MCP execution here.

### 3c. Delegate to a CLI agent you already run (ACP)

`ProviderConfig::Acp { command, model }` shells out to any
[Agent Client Protocol](https://agentclientprotocol.com) binary over stdio
(Claude Code, Gemini CLI, Codex, etc.). To add one, usually just a catalog preset
with the right `command` argv. (This is a place Uncaged is broader than most
forks — keep it.)

### Test a provider without the UI

```bash
UNCAGED_ENABLED=1 UNCAGED_PROVIDER=ollama UNCAGED_MODEL=qwen2.5-coder:32b cargo run
# or edit ~/.uncaged/engine.json directly, or run ./script/uncaged-setup
```

Engine unit/live tests: `crates/uncaged_engine/src/{tests,live_tests,connections_tests}.rs`.

---

## 4. Recipe: rebrand (name, URL, colors, logo)

Everything textual/visual about the brand is centralized so a rebrand is a small,
known edit — **do not scatter literals**, always reach for these:

1. **`app/src/brand.rs`** — the single source of truth. Change `NAME`,
   `NAME_LOWER`, `TAGLINE`, `HOME_URL`, and the `ember` / `ground` color modules.
   > ⚠️ Known gap: `HOME_URL` and ~194 in-app links currently point at a
   > placeholder repo. See DIRECTION.md → "Repo identity" — this is a pending
   > decision, not an accident.
2. **`app/assets/bundled/svg/brand/uncaged-mark.svg`** — the in-app monochrome
   glyph. It's drawn with an `rgb(255,0,0)` **recolor sentinel**: the framework
   swaps that red for the current theme ink, so keep shapes red.
3. **`app/assets/bundled/svg/brand/uncaged-icon.svg`** — the full-color app-icon
   master. After editing, regenerate the `.icns` (see RELEASING / the
   `script/compile_icon` path) — macOS caches icons aggressively, so a dock
   refresh may need `killall Dock Finder`.
4. **App identity** (only when you also want a new bundle): `channel/state.rs`
   `AppId::new("dev","uncaged","WarpOss")`, `channel/mod.rs` cli command names,
   `channel/state.rs` url scheme, and the `[package.metadata.bundle.bin.warp-oss]`
   block + `app/src/bin/oss.rs` plist in `app/Cargo.toml`.

**Do NOT rename the internal `warp_*` crates or the `warp`/`warp-oss` package
names.** That's ~2,000 files of churn for zero user benefit and it breaks every
upstream merge. The sibling project (Zap) independently made the same call. The
product/identity layer and the code layer are intentionally separate.

---

## 5. Recipe: reskin / themes

The default look is the **Uncaged ember** theme (`ThemeKind::Uncaged`), applied to
fresh profiles. The palette originates in `brand.rs` (`ember` gold→orange→red +
`ground` warm-black neutrals). An existing profile keeps whatever theme is saved
in `~/.uncaged/settings.toml` (`theme = "uncaged"`, `system_theme = false`), so to
*see* a theme change on a profile you've already used, set those keys or start
from a clean `~/.uncaged`.

To ship more themes, add them to the theme set the app registers (grep for
`ThemeKind::Uncaged` to find the registration point) and, if you want them in the
picker, the appearance settings page (`app/src/settings_view/appearance_page.rs`).

---

## 6. Recipe: the privacy gates (audit + extend)

Uncaged's promise is **"the only outbound traffic is to the model endpoint you
configure."** That's enforced in code, not by trust. Each gate and where it lives:

| Egress | Where it's cut |
|---|---|
| Agent Mode → Warp servers | `warp_multi_agent_client/src/lib.rs` — hard-fails on Oss instead of POSTing |
| **All Warp URLs (defense-in-depth)** | `warp_core/src/channel/config.rs` — `uncaged_sentinel()` points every endpoint at the unroutable `192.0.2.0:9`; wired in `state.rs::init()` |
| Autoupdate poll / version check | `app/src/autoupdate/mod.rs` + `channel_versions.rs` — gated off for Oss |
| Login / accounts | `app/src/auth/auth_manager.rs` — refuses auth on Oss (so every account-gated call is unreachable) |
| Telemetry (Rudderstack) | default off (`app/src/settings/privacy.rs`), and `telemetry_config: None` for Oss |
| Crash reporting (Sentry) | `cocoa_sentry` feature dropped for oss in `script/macos/bundle`; `crash_reporting_config: None` |
| AI analytics | removed from default features in `app/Cargo.toml` |
| Fallback-font download | `app/src/font_fallback.rs` — returns `None` on Oss, uses OS system fonts |

**How to audit after an upstream merge:** grep the diff for new `server_root_url`,
`server_api`, `reqwest`, `.post(`, `warp.dev`, `rudderstack`, `sentry`, and any new
`autoupdate`/`fetch_*` call. If a new path can fire on Oss without auth, gate it or
rely on the sentinel (it will fail to connect, but you want *fast* failure and no
attempt at all for known paths). The structural guarantee: **login is disabled, so
anything behind `on_user_fetched`/auth can't fire** — the risk is only
*unauthenticated* auto-fetches (like the font one we already caught).

---

## 7. Recipe: build, run, deploy, ship

```bash
# --- dev loop (debug) ---
source "$HOME/.cargo/env"
cargo run                                   # build + launch Uncaged
CARGO_INCREMENTAL=0 cargo build --bin warp-oss   # compile-check only

# --- point it at a model ---
./script/uncaged-setup                      # menu; or Settings → AI Models; or edit ~/.uncaged/engine.json

# --- release artifact (ad-hoc signed .app + .dmg) ---
./script/bundle --channel oss --selfsign --arch aarch64
# → target/aarch64-apple-darwin/release-lto/bundle/osx/Uncaged.{app,dmg}
```

- Release profile is `release-lto` (LTO, `debug_assertions` OFF → debug menus
  compile out). The `oss` channel drops Sentry and enables no analytics.
- No Apple Developer ID → ad-hoc signed → users run
  `xattr -dr com.apple.quarantine /Applications/Uncaged.app` once. Full details +
  the cross-platform-CI plan are in RELEASING.md and DIRECTION.md.
- Local deploy of a fresh debug build: `pkill -9 -f "Uncaged.app/.../warp-oss"`,
  copy `target/debug/warp-oss` into the bundle's `Contents/MacOS/`,
  `codesign --force --deep --sign - ~/Applications/Uncaged.app`, relaunch.

---

## 8. Config files (`~/.uncaged/`)

| File | Written by | Holds |
|---|---|---|
| `engine.json` | Settings UI, `uncaged-setup`, or hand | the **active** backend (`UncagedConfig` → `enabled` + `ProviderConfig`) |
| `connections.json` | the roster (`connections.rs`) | all saved connections; the active one is projected into `engine.json` |
| `settings.toml` | the app | theme, keybindings, terminal prefs, etc. |

Env overrides beat the files for quick experiments:
`UNCAGED_ENABLED`, `UNCAGED_PROVIDER`, `UNCAGED_API_KEY`, `UNCAGED_MODEL`,
`UNCAGED_BASE_URL`, `UNCAGED_ACP_COMMAND`, or `$UNCAGED_CONFIG` to relocate the file.

> Security note: API keys currently live in `connections.json` in plaintext.
> Moving them to the OS keychain is a tracked follow-up (DIRECTION.md → backlog);
> the infra (`warpui_extras` `secure_storage` / `security-framework`) is already
> in the tree.

---

## 9. Gotchas (things that will waste an hour if you don't know them)

- **Shell searches for "Warp" lie.** The `ctx-wire` wrapper scrubs the word "Warp"
  from `grep`/`rg` stdout. Use the editor's file reader, or `rg … > /tmp/x && cat`
  a fresh file, when auditing Warp strings.
- **Release-only behavior.** `release_bundle` force-enables the `Autoupdate` flag
  and compiles out `#[cfg(debug_assertions)]` blocks, so the release build differs
  from `cargo run`. Verify with `./script/bundle --channel oss --selfsign --arch aarch64 --check-only`.
- **Icon cache is stubborn.** After changing the app icon, macOS may show the old
  one until `killall Dock Finder` (sometimes a full icon-services cache wipe).
- **Disk.** A `release-lto` build wants >10 GB free on top of an existing
  `target/`. `cargo clean` if tight.
- **Don't fight upstream.** Keep changes concentrated in the files in §2. The more
  you touch stock Warp code, the more painful the next upstream merge.
