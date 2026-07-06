# Moving from Warp to Uncaged

Uncaged is a fork of the open-source Warp client, so the two feel identical — but
they are **completely isolated on disk**. Installing Uncaged changes nothing about
an existing Warp install, and Uncaged never reads Warp's account, history, or
credentials. This guide explains that isolation and shows what you can optionally
carry over.

## Why they don't share anything

Uncaged runs as its own macOS app (`dev.uncaged.WarpOss`) on its own release
channel, so it uses its own directories:

| | Warp (Stable) | Uncaged |
|---|---|---|
| User config (themes, workflows, keybindings, MCP) | `~/.warp/` | `~/.uncaged/` |
| App state (history DB, caches) | `~/Library/Application Support/dev.warp.Warp-Stable/` | `~/Library/Application Support/dev.uncaged.WarpOss/` |
| Model / engine config | *(server-side, in your Warp account)* | `~/.uncaged/engine.json`, `~/.uncaged/connections.json` |

Because the paths are keyed to the app identity, the two installs can coexist and
neither touches the other. This isolation is also a privacy guarantee: Uncaged has
no path to your Warp account state even if you have Warp installed.

## What you can carry over (optional)

These are plain, user-editable files in the same format, so you can copy them from
`~/.warp/` to `~/.uncaged/`:

| Item | From | To |
|---|---|---|
| Custom themes | `~/.warp/themes/` | `~/.uncaged/themes/` |
| Workflows | `~/.warp/workflows/` | `~/.uncaged/workflows/` |
| Keybindings | `~/.warp/keybindings.yaml` | `~/.uncaged/keybindings.yaml` |
| Launch configurations | `~/.warp/launch_configurations/` | `~/.uncaged/launch_configurations/` |
| MCP servers | `~/.warp/.mcp.json` | `~/.uncaged/.mcp.json` |

```bash
# Example: bring your themes and workflows across (nothing is overwritten in ~/.warp)
mkdir -p ~/.uncaged
cp -R ~/.warp/themes ~/.uncaged/ 2>/dev/null || true
cp -R ~/.warp/workflows ~/.uncaged/ 2>/dev/null || true
cp ~/.warp/keybindings.yaml ~/.uncaged/ 2>/dev/null || true
cp ~/.warp/.mcp.json ~/.uncaged/ 2>/dev/null || true
```

## What does NOT carry over — and why

- **Your Warp account / login.** Uncaged has no accounts and no login. There is
  nothing to sign into and nothing to import.
- **Warp Drive / cloud objects, shared sessions, team data.** These live on Warp's
  servers, which Uncaged never contacts. They stay in Warp.
- **AI / agent history from Warp's cloud agent.** Uncaged drives Agent Mode from
  *your* model (see below); it does not import cloud conversation history.
- **Stored credentials / API keys from Warp's keychain entries.** Uncaged manages
  its own model connection and does not read Warp's secrets.
- **Command history DB.** Kept per-app under Application Support; not migrated, so
  your Uncaged history starts clean.

## The one thing you must set up: a model

Warp's agent ran on Warp's servers behind your subscription. Uncaged runs on a
model **you** provide. Pick any one:

- **A hosted API key** — OpenAI, Anthropic, OpenRouter, Google, Groq, DeepSeek,
  Mistral, xAI, Together, or any OpenAI-compatible endpoint.
- **A local model** — Ollama, LM Studio, llama.cpp, vLLM.
- **A CLI agent you already run** — Claude Code, Gemini CLI, Codex (over ACP).

Set it up in **Settings → AI Models**, or from a terminal:

```bash
./script/uncaged-setup     # menu: choose a backend, paste a key or pick a local model
```

Either path writes `~/.uncaged/engine.json`, read live by the app. From that point
Agent Mode works exactly like Warp's — same UI, same native tool execution (your
shell, files, MCP servers) — with the only difference that inference happens on the
endpoint you chose, and **nothing is ever sent to Warp**.
