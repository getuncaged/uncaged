# Contributing to Uncaged

Thanks for helping improve Uncaged! Uncaged is a free, account-free, bring-your-own-model fork of the open-source Warp terminal, licensed under AGPL-3.0. This guide explains how to open issues, propose changes, and get your work reviewed.

Uncaged is an independent community fork. There is no company behind it, no CLA to sign, and no account required — contributions are accepted under the project's AGPL-3.0 license.

## TL;DR

- Bug fixes are welcome once the report is actionable from the provided details.
- Feature requests are best discussed on an issue first, so scope and design can be agreed before code is written.
- Larger changes benefit from a written spec before implementation (see [Opening a Spec PR](#opening-a-spec-pr)).
- Implementation PRs must include proof of manual testing.

## How Contributing to Uncaged Works

- **Issues are the starting point for everything.** Discussion, scoping, and design happen on the issue before any PR is opened.
- **Feature requests differ from bug fixes:**
  - Features benefit from a design discussion before implementation, so it's clear what's being built and why.
  - For larger features, a written spec first (a *product spec* + *tech spec* committed under [`specs/`](specs/)) keeps scope and architecture reviewable before code is written.
  - Bug fixes can go straight to a code PR once the report is reproducible or otherwise actionable; they do not require a spec unless the scope or design is unclear.

## Filing a Good Issue

Search [existing issues](https://github.com/getuncaged/uncaged/issues) before filing to avoid duplicates. Use the issue templates when filing.

### Bug reports

A good bug report includes:

- A clear title and a one-paragraph summary of the problem.
- Steps to reproduce (with a minimal example where possible).
- Expected vs. actual behavior.
- Uncaged version and OS (see `Settings → About`).
- Logs, screenshots, or screen recordings when relevant.

### Feature requests

A good feature request describes the user-facing problem before any proposed implementation. Include:

- The user need or pain point, and who experiences it.
- The current behavior and why it falls short.
- A sketch of the desired behavior or workflow (a short example or mock is helpful but not required).
- Any relevant constraints (compatibility, related features, prior art, etc.).

## Opening a Spec PR

Larger changes are easier to review when a spec lands before the code. A spec consists of two short documents committed under [`specs/GH<issue-number>/`](specs/):

- **`product.md`** (the *product spec*) — Defines the desired behavior from the consumer's perspective (the user, an API caller, a CLI user, etc.) and stays out of implementation detail. The core is a numbered list of **testable behavior invariants** covering the happy path, user-visible states, inputs and responses, and edge cases (empty / error / loading, cancellation, offline, permission denied, races, accessibility). Optional sections: problem statement, goals / non-goals, open questions.
- **`tech.md`** (the *tech spec*) — The implementation plan, grounded in this codebase. Required sections: **Context** (the current system and relevant files with line references), **Proposed changes** (modules touched, new types / APIs / state, data flow, tradeoffs), and **Testing and validation** (how each invariant from the product spec will be verified). Optional: end-to-end flow, Mermaid diagrams, risks, parallelization, follow-ups.

To open a spec PR:

1. Add `specs/GH<issue-number>/product.md` and `specs/GH<issue-number>/tech.md`. Browse [`specs/`](specs/) for examples of well-structured specs.
2. Use the PR as the home for product and technical discussion.
3. Once the specs are approved, implementation generally continues on the same PR. In rarer cases — for example, if a large spec is merged on its own so the implementation can be broken up — it can move to a linked follow-up PR.

## Opening a Code PR

1. Branch from `master`.
2. Implement the change and add tests (see [Testing](#testing)).
3. Run `./script/presubmit` and fix any failures before pushing.
4. Open a PR using the [pull request template](.github/pull_request_template.md).
5. Keep the PR focused on a single logical change and merge `master` in before the PR enters review.

**You must include proof of [manual testing](#manual-testing)**. For small, isolated, and visual changes, you should include **before and after screenshots**. For larger, broad, or interactive changes, you should also include a **narrated screen recording**.

## Using a Coding Agent

You can use **any coding agent** to implement a contribution — for example, Uncaged's built-in Agent Mode, Claude Code, Codex, Gemini CLI, or others — or no agent at all. This repository ships agent-readable context (skills under [`.agents/skills/`](.agents/skills/), specs under [`specs/`](specs/), and [`AGENTS.md`](AGENTS.md)) that any harness supporting these formats can pick up.

While you can use coding agents for implementation, we expect contributors to **collaborate as humans**. Please talk to reviewers and other contributors directly rather than routing conversation through an agent.

## Code Review

All pull requests go through review before merge. Reviewers check for correctness, style, test coverage, and alignment with the linked issue and any associated specs. Keep PRs focused and include the manual-testing evidence described above so review can move quickly.

## Development Setup

See [README.md](README.md) and [AGENTS.md](AGENTS.md) for the full engineering guide. Quick start:

```bash
./script/bootstrap   # platform-specific setup
cargo run            # build and run Uncaged
./script/presubmit   # fmt, clippy, and tests
```

To produce a macOS app bundle:

```bash
./script/bundle app --channel oss --selfsign
```

Because the project has no Apple Developer ID, these builds are ad-hoc signed. On first launch after copying the app to `/Applications`, you may need to clear the quarantine attribute:

```bash
xattr -dr com.apple.quarantine /Applications/Uncaged.app
```

## Testing

Tests are required for most code changes:

### Manual Testing
Manual testing is required for changes that can be manually tested, and almost all changes can be manually tested. For small, isolated, and visual changes, you should include **before and after screenshots**. For larger, broad, or interactive changes, you should also include a **narrated screen recording**.

You can run the app locally using `./script/run` - see [AGENTS.md](AGENTS.md) for more details on how to get set up.

### Automated Tests
- **Bug fixes** should include a regression test that would have caught the bug.
- **Algorithmic or non-trivial logic** needs unit tests.
- **User-facing flows** should have end-to-end coverage under [`crates/integration/`](crates/integration/) whenever the behavior can be exercised that way. If a flow is worth shipping, it's usually worth an integration test.

Run unit tests with `cargo nextest run`.

## Code Style

- `./script/format --check` and `cargo clippy --workspace --all-targets --all-features --tests -- -D warnings` must pass.
- Prefer imports over path qualifiers, inline format args (`println!("{x}")`), and exhaustive `match` over `_` wildcards.
- See [AGENTS.md](AGENTS.md) for the full style guide, including WarpUI patterns and terminal model locking rules.

## Commit and Branch Conventions

- Branch names should be prefixed with your handle (e.g. `alice/fix-parser`).
- Commit messages should explain *what* and *why*, not just *what*.

## Code of Conduct

This project adopts the [Contributor Covenant](https://www.contributor-covenant.org/) (v2.1) as its code of conduct. All contributors and maintainers are expected to follow it in every project space. See [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md) for the full text.

## Reporting Security Issues

See [`SECURITY.md`](SECURITY.md) for the security disclosure policy and private reporting channels. **Do not open public issues for security vulnerabilities.**

## Getting Help

- Browse [AGENTS.md](AGENTS.md) for the engineering guide and build setup.
- Open a [GitHub issue](https://github.com/getuncaged/uncaged/issues) for bugs or feature requests.
