<div align="center">

# Uncaged  `[ ❯_ ]`

**The open terminal you own. Bring your own AI — free forever.**

Uncaged is a fork of the open-source [Warp](https://github.com/warpdotdev/warp) client, *freed*:
**no account, no subscription, no data collection.** Power its agentic terminal
with a model *you* control — a hosted API key, a local model (Ollama, LM Studio,
llama.cpp, vLLM), or a CLI agent you already pay for.

The VSCodium of Warp. Same terminal, same speed, same look — unlocked.

</div>

---

## Why Uncaged exists

Warp open-sourced its **client** under AGPL-3.0 and invites forks. But Warp's
*premium* agent runs on Warp's servers, behind a subscription and a login — and
none of that inference code is in the open repo. There was no "unlock"; the
functionality simply wasn't there.

Uncaged supplies the missing piece — a **local agent engine** ([`crates/uncaged_engine`](crates/uncaged_engine))
that re-implements the inference layer Warp keeps server-side, pointed at a
backend *you* choose — and removes the account/paywall gates so the whole thing
is free to use, forever, with nothing sent anywhere you didn't ask for.

## What Uncaged changes vs. Warp

- **No account required.** The agent works the moment you configure an engine —
  no sign-in, no Warp cloud in the loop.
- **No subscription.** Your model, your tokens (or your local GPU). Zero Warp credits.
- **No data collection.** Telemetry, crash reporting, and cloud conversation
  storage all default to **off**.
- **Your engine, your choice.** API key, local model, or a CLI agent — swap any time.
- **Rebranded** off the Warp trademark (name + logo), Warp's visual design kept.

## How it works

Warp's entire foreground Agent Mode funnels through one function,
[`generate_multi_agent_output`](crates/warp_multi_agent_client/src/lib.rs). Uncaged
adds a few lines at the top: if a local engine is configured, the request goes to
the **Uncaged engine** instead of Warp's servers. Everything else — request building,
response rendering, **native tool execution**, the permission UI — is unchanged.

The engine is deliberately small ("inference + tool-call protocol" only). Warp's
client already runs every agent tool locally — your shell commands in your PTY,
your file edits, your MCP servers — so only the model call moved onto your machine.

```
You type ──▶ Uncaged client ──▶ generate_multi_agent_output
                                   │
                     local engine configured?
                         │ yes            │ no
                         ▼                ▼
                 Uncaged engine        (Warp's servers — unused in Uncaged)
                         │
              your backend (API / local model / CLI)
                         │
        ResponseEvents ◀─┘  (text + native tool calls)
                         │
        Uncaged executes tools locally (PTY, files, MCP) ──▶ next turn
```

## Backends

| Type | `kind` | Covers | Native tools |
|---|---|---|---|
| Hosted API | `anthropic` | Claude (your key) | ✅ |
| OpenAI-compatible | `openai_compatible` | OpenAI, OpenRouter, **Ollama**, **LM Studio**, llama.cpp, vLLM | ✅ |
| CLI agent (ACP) | `acp` | Claude/Gemini/etc. ACP binaries | ⚠️ experimental, text-only* |

\* An ACP agent runs its own tool loop, so that path surfaces its text but
bypasses Uncaged's native tool execution. For the full native experience (local
models included), use `anthropic` or `openai_compatible`.

## Configure in a few clicks

**In the app:** open **Settings → Agents → AI Models → Connect a model**. Pick a
platform from the grouped gallery — *Use a local agent* (Claude Code / Gemini /
Codex CLI, driven by your own login), *Run a model locally* (Ollama / LM Studio),
or *Connect with an API key* (Anthropic, OpenAI, OpenRouter, Google, Groq,
DeepSeek, Mistral, xAI, Together, or any OpenAI-compatible endpoint). Local and
CLI options connect in one click; API providers just need a key. Keep several
connected and switch the active one with **Use**; the active model powers Agent
Mode. Your saved connections live in `~/.uncaged/connections.json` and the active
one is projected into `engine.json` — everything stays on your machine.

**From a terminal** (build-free, same result):

```bash
./script/uncaged-setup          # menu: pick a backend, paste a key or choose a local model
```

Both write `~/.uncaged/engine.json`, read live by the app (no restart). A config
made this way (or by hand) shows up automatically in the Settings gallery. You can
also use env vars (`UNCAGED_ENABLED=1 UNCAGED_PROVIDER=ollama UNCAGED_MODEL=...`)
or edit the file directly:

```jsonc
{
  "enabled": true,
  "provider": { "kind": "openai_compatible", "base_url": "http://localhost:11434/v1", "model": "qwen3-coder:30b" }
}
```

**Recommended local model** (agentic coding + reliable tool calls, great on a
32GB+ Apple Silicon Mac): **Qwen3-Coder-30B-A3B** (MLX 4-bit). Devstral-Small-24B
and gpt-oss-20b are strong alternatives.

## Build & run (macOS)

```bash
# one-time: full Xcode (App Store), then
sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
sudo xcodebuild -license accept
xcodebuild -downloadComponent MetalToolchain     # Metal compiler for warpui

cargo run                                         # builds & launches Uncaged
./script/uncaged-setup                               # point it at your model
```

Requires the toolchain pinned in `rust-toolchain.toml` (1.92.0). You do **not**
need `./script/bootstrap` (it installs Docker/gcloud/PowerShell you won't need).

## Licensing, trademark & attribution

- **AGPL-3.0.** Uncaged stays under the same license as the upstream client (see
  [`LICENSE-AGPL`](LICENSE-AGPL); `warpui`/`warpui_core` remain [MIT](LICENSE-MIT)).
  If you distribute Uncaged or run a modified version others use over a network,
  AGPL §13 requires you to offer them your complete corresponding source. Keep it open.
- **Trademark.** "Warp" and its logo are Warp's trademarks, which AGPL does not
  grant. Uncaged is renamed and re-marked accordingly; it keeps Warp's *visual design*
  (which is in the open code, not trademarked). Uncaged is **not** affiliated with or
  endorsed by Warp.
- **Attribution.** Uncaged is a derivative of Warp by Denver Technologies, Inc. See
  [`NOTICE`](NOTICE). Upstream: <https://github.com/warpdotdev/warp>.

## Status & honest limitations

This is a working fork, built and run from source and prepared for a 0.1.0
release. Straight talk on what's solid vs. what's still rough:

- **Solid / verified:** the local engine (compiles clean, tests pass against the
  real proto types), the no-account gate (Agent Mode works with no login),
  telemetry/analytics/crash-reporting/autoupdate all off, the `[ ❯_ ]` app icon
  and ember theme (visually verified on-device), the account UI removed, and the
  release build packaged as an ad-hoc-signed `.app` + `.dmg` (see
  [RELEASING.md](RELEASING.md)).
- **Enforced in code:** on the `oss` channel the agent seam hard-fails instead of
  falling back to Warp's servers, autoupdate never polls, login is refused (so
  every account-gated call is structurally unreachable), telemetry/analytics/crash
  reporting are off, and the on-demand fallback-font fetch is disabled in favor of
  OS system fonts — so a release build cannot silently phone home. The only egress
  is to the model endpoint you configure.
- **Deferred / follow-ups:** a from-scratch interactive "connect your engine"
  onboarding slide (today: rebranded copy + configure in Settings /
  `uncaged-setup`); the ACP backend is experimental (text-only, bypasses native
  tools); no Apple Developer ID yet, so distributed builds need a one-time
  `xattr -dr com.apple.quarantine`.
