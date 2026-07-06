# Frequently Asked Questions

This FAQ covers the questions we hear most often about using Uncaged, connecting a model, and contributing to this repository. Uncaged is a free, account-free, bring-your-own-model fork of the open-source Warp terminal, licensed under AGPL-3.0. For the full contribution flow, see [CONTRIBUTING.md](CONTRIBUTING.md). For engineering details — build setup, code style, testing — see [AGENTS.md](AGENTS.md).

## Using Uncaged

### How is Uncaged different from Warp?

Uncaged is an independent fork of the open-source Warp client. The differences that matter:

- **No account, no login.** Uncaged runs with its own local identity and never asks you to sign in.
- **Bring your own model.** Instead of a hosted, server-side agent, Uncaged drives Agent Mode with a local Rust crate (`uncaged_engine`) that talks to a model endpoint you configure.
- **No cloud dependency.** No telemetry, no analytics, no cloud sync, no autoupdate or phone-home.
- **AGPL-3.0.** Uncaged is a community fork, based on Warp (© Denver Technologies, Inc.); attribution lives in the repository's `NOTICE` file.

### Do I need an account?

No. Uncaged has no accounts and no login. It runs entirely on your machine with its own local profile.

### How do I connect a model?

Open **Settings → AI Models** and connect one of:

- an **API key** for a hosted model provider (OpenAI, Anthropic, Google, Mistral, DeepSeek, Groq, Together, OpenRouter, and similar),
- a **local runtime** such as LM Studio or Ollama,
- or a **CLI agent over ACP** (for example Claude Code, Codex, or Gemini CLI).

You only need one connection to start using Agent Mode. A setup helper is also available at `script/uncaged-setup`.

### Does Uncaged send my data anywhere?

No — not to any Uncaged or Warp servers. Uncaged never sends your prompts or terminal data to a central service. The only outbound network traffic is to the model endpoint you configure yourself in **Settings → AI Models**. There are no accounts, no telemetry, no analytics, and no cloud sync.

### Where is my configuration stored?

Locally, under `~/.uncaged/`. That directory holds `engine.json`, `connections.json`, and `settings.toml`.

### macOS says the app is from an unidentified developer — how do I open it?

Release builds are **ad-hoc signed**, because the project has no Apple Developer ID. After copying the app to `/Applications`, clear the quarantine attribute once on first launch:

```bash
xattr -dr com.apple.quarantine /Applications/Uncaged.app
```

You can then open the app normally.

## Contributing

### How do I contribute?

Start with a GitHub issue on [the Uncaged repository](https://github.com/getuncaged/uncaged/issues). Bug reports can go straight to a code PR once they are actionable; feature requests are best discussed on an issue first, and larger changes benefit from a short spec PR before code is written. The full flow is documented in [CONTRIBUTING.md](CONTRIBUTING.md).

### How do I file a good bug report or feature request?

Use the [issue templates](https://github.com/getuncaged/uncaged/issues/new/choose). For bugs, include reproduction steps, expected vs. actual behavior, your Uncaged version (`Settings → About`), and OS. For features, describe the user-facing problem before proposing an implementation.

### Why do larger changes benefit from a spec PR before code?

Specs make scope, behavior, and architecture reviewable on their own, before someone writes code that may need to be thrown away. A spec PR adds a `product.md` (desired behavior) and a `tech.md` (implementation plan) under `specs/GH<issue-number>/`. See [Opening a Spec PR](CONTRIBUTING.md#opening-a-spec-pr) for what each document should contain.

### How do I build and run Uncaged from source?

```bash
./script/bootstrap   # platform-specific setup
cargo run            # build and run Uncaged
./script/presubmit   # fmt, clippy, and tests
```

To produce a macOS app bundle, run `./script/bundle app --channel oss --selfsign`. See [AGENTS.md](AGENTS.md) for the full engineering guide.

### Can I use my own coding agent to contribute?

Yes. Use whatever you like — Uncaged's built-in Agent Mode, Claude Code, Codex, Gemini CLI, others, or no agent at all. The repo ships agent-readable context (skills under [`.agents/skills/`](.agents/skills/), specs under [`specs/`](specs/), and [`AGENTS.md`](AGENTS.md)) that any harness supporting these formats can pick up.

## Licensing

### What license is Uncaged under?

The Uncaged client is licensed under [AGPL-3.0](LICENSE-AGPL); the `warpui` / `warpui_core` UI crates remain [MIT](LICENSE-MIT). Uncaged is a fork of the open-source Warp client (© Denver Technologies, Inc.); the upstream attribution is preserved in the repository's `NOTICE` file.

### Why AGPL?

AGPL keeps derivatives open: it prevents someone from forking the client, making changes, and shipping a closed-source product back to users, and it closes the network-use loophole that plain GPL leaves open, so a hosted derivative is also covered. Keeping Uncaged under AGPL means improvements to the client stay available to everyone.

### Can I fork Uncaged?

Yes — that's what AGPL is for. The license prevents fully-proprietary relaunches; open derivatives are welcome.

## Help and security

### Where do I get help?

- [GitHub Issues](https://github.com/getuncaged/uncaged/issues) for bug reports and feature requests.
- [AGENTS.md](AGENTS.md) and [CONTRIBUTING.md](CONTRIBUTING.md) for build setup and the contribution flow.

### How do I report a security vulnerability?

Please don't open a public GitHub issue. See [SECURITY.md](SECURITY.md) — report privately by opening a [GitHub Security Advisory](https://github.com/getuncaged/uncaged/security/advisories/new) on the Uncaged repository.
