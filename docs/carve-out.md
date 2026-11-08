# Carving Uncaged out of Warp

Uncaged started as a fork of Warp's open-source client. The goal of this plan is to
make it fully our own: no code that talks to, waits on, or only makes sense with
Warp's servers, no crates we don't use, and our own names on everything. The
payoff is a smaller app, faster builds, a faster cold start and less to maintain.

## What "cutting ties" can and can't mean

Warp's client code is licensed under AGPL-3.0 (most crates) and MIT (`warpui`,
`warpui_core`). Removing code, renaming crates and reworking what's left are all fine.
Two things stay:

- `LICENSE-AGPL`, `LICENSE-MIT` and `NOTICE`, plus the copyright notices in files
  that came from Warp. Uncaged remains a derivative work for the code it keeps.
- Under the AGPL, the source of every release stays public. It already is.

## Where we start (2026-11-07, v0.3.4)

| Measure | Now | Target |
| --- | --- | --- |
| Workspace crates | 78 | about 45 |
| Crates compiled for the app (macOS) | 943 | under 600 |
| Rust in `app/src` | 1.06M lines | under 700k |
| macOS arm64 DMG | 136 MB | under 90 MB |
| Launch to first frame (release) | 549 ms | under 250 ms |

Startup breakdown for the last release: settings 134 ms, SQLite 99 ms, singletons
56 ms, window creation 99 ms, first frame 147 ms (`Startup timing:` in
`~/Library/Logs/uncaged.log`; `warp-oss.log` before the rename).

What still ties us to Warp, by size (files in `app/src` that use it):

| Area | Files | Notes |
| --- | --- | --- |
| Server API (`server_api`) | 213 | GraphQL client, every cloud call |
| Telemetry events | 189 | about 2,460 calls; nothing is sent (no telemetry config) |
| Accounts (`AuthStateProvider`, Firebase) | 141 + 36 | there are no accounts in Uncaged |
| Shared sessions | 122 | needs Warp's session-sharing server |
| Drive sync (`cloud_objects`, `UpdateManager`) | 110 + 98 | Drive UI stays, backed locally or by gist |
| Warp server GraphQL (`warp_graphql`, schema codegen) | 78 | likely the biggest single compile cost |
| Remote server (SSH helper binary) | 71 | downloads Warp's binary |
| Agent wire format (`warp_multi_agent_api`, protobuf) | 58 | doubles as the conversation data model |
| AWS SDK (Bedrock credentials) | 2 | credentials only ever went to Warp's server |
| Sentry, OpenTelemetry | 20 + 3 | crash and trace reporting to Warp |

## Where we ended (2026-11-08, branch `uncaged-carve-phase1` at 88cbc18, not released)

| Measure | Start (v0.3.4) | Now | Target |
| --- | --- | --- | --- |
| Workspace crates | 78 | 59 | about 45 |
| Crates compiled for the app (macOS) | 943 | 787 | under 600 |
| Packages in `Cargo.lock` | 1,465 | 1,354 | |
| Rust in `app/src` | 1.06M lines | 769k lines | under 700k |
| Launch to first frame (debug bundle, real data, idle) | ~700 ms | ~430-490 ms | |
| macOS arm64 DMG, release launch time | 136 MB, 549 ms | not measured yet (needs a release build) | under 90 MB, under 250 ms |

63 commits since v0.3.4: 3,304 files changed, 378,981 lines deleted, 32,442 added.

Gone: AWS/Sentry/OpenTelemetry, telemetry, accounts and Firebase, teams and workspaces,
settings sync, shared sessions, the SSH remote server, Oz/cloud agents, the AI server
client, server experiments, ServerApi and the whole GraphQL stack (cynic, schema codegen,
websocket/graphql-ws), the IAP and Warp client headers, `remote_tty`, voice input, and the
git dependency on Warp's protobuf repo (now `crates/uncaged_agent_api`, checked-in
generated code, no protoc for normal builds). Drive is a local store (Trash, restore and
delete work and persist). Crates are `uncaged_*`; the app is `uncaged` / `dev.uncaged.Uncaged`
with a one-time data migration and compat names so v0.3.4 can still update (see RELEASING.md,
"The identity rename").

Since then (2026-11-08, cleanup pass; app crates 786, dead-code warnings 89 → 86): the `session-sharing-protocol` git dependency, the Oz,
OpenWarp and orchestration launch modals, the agents view's Oz cloud setup guide, the wasm
"Download Warp Desktop" dialog, the Linux updater's Warp pacman/apt commands, Warp's internal
opencode-plugin debug actions, GEAP credential state in `crates/ai`, and the integration tests
that needed Warp's GCP SSH VMs are gone.

Still open:
- About 86 dead-code warnings (`cargo check -p uncaged --bin uncaged` without `-A dead_code`).
  Almost all are items only unit tests still reach: the cloud ambient-task model
  (`ai/ambient_agents`, the task map in `AgentConversationsModel`, which is empty outside
  tests), orchestration lifecycle-event helpers, the Oz platform-plugin checks in
  `cli_agent_sessions/plugin_manager`, `llms.rs` server model updates, and small UI builder
  leftovers. Each needs its tests deleted with it, so they were left for a deliberate pass.
- The free-AI-removal notice modal and the HOA onboarding flow (never opened; the modal's view
  also backs the live prompt-suggestions-unavailable modal), the changelog-only resource
  center, and server-sentinel links (`ServerConversationToken::conversation_link`/`debug_link`).
- Cleanup-backlog leftovers: harness auth-secret UI, enterprise secrets in the terminal model,
  web-intent parsing (wasm only), and `RequestParams.api_keys`/`custom_model_providers`
  always `None`.
- The CLI-agent plugin installers still point at Warp's public `claude-code-warp`,
  `codex-warp` and `gemini-cli-warp` repos; Uncaged has no plugins of its own yet.
- The Linux secure-storage key seed string that names `releases.warp.dev` must stay (changing
  it would make stored secrets unreadable).

## Rules for every phase

- Each phase lands on `uncaged-0.3` green: `cargo fmt`, both clippy runs from
  `RELEASING.md`, the unit suite, Uncaged CI on macOS, Linux and Windows, and a
  debug bundle smoke-tested in `/Applications`.
- Delete, don't stub, wherever no Uncaged feature uses the code. Where a feature we
  keep sits on Warp's plumbing (Drive, agent conversations), replace the plumbing
  with ours first, then delete Warp's.
- Settings and data on users' machines keep working across every release. Anything
  that moves gets a one-time migration.
- Measure after each phase and update the table above.

## Phases

### 1. Dead weight with no product surface
Remove what nothing in Uncaged uses:
- AWS SDK and the Bedrock credential plumbing (and the `aws-lc-sys` C build).
- OpenTelemetry, Sentry and crash reporting to Warp.
- Warp's internal channel binaries (`local`, `dev`, `stable`, `preview`) and the
  channel config generator plumbing; Uncaged ships one binary.
- Workspace crates the app never reaches: `serve-wasm`, `managed_secrets_wasm`,
  `warp_tui`, and anything else `cargo metadata` shows unreachable.
- `warp_isolation_platform` and `warp_managed_secrets` (cloud agent sandboxes).

### 2. Telemetry
Remove the telemetry event enum, the `send_telemetry_*` macros and every call
site. Mechanical but wide (189 files).

### 3. Accounts and the Warp server
- Remove `AuthStateProvider`, Firebase and the login and onboarding-auth flows;
  Uncaged has exactly one implicit local user.
- Remove the Warp server client: `warp_graphql`, `warp_graphql_schema`, `cynic`,
  `warp_server_client`, `warp_server_auth`, the websocket client.
- Remove the settings pages and UI already hidden in the fork: teams, billing,
  referrals, Oz cloud keys, environments, Warpify.

### 4. Cloud features
- Drive: keep the UI, back it with a local store (and gist sharing) instead of
  `cloud_objects` sync.
- Shared sessions: remove (needs Warp's server).
- Remote server for SSH: remove; SSH sessions keep working as plain shells.

### 5. Our own agent data model
Replace the `warp_multi_agent_api` protobuf types that the agent UI and history use
as their data model with Uncaged's own types, then drop `prost` and the generated
code. Saved conversations get migrated.

### 6. Start-up and runtime
Profile cold start and fix the biggest phases: settings load (134 ms), SQLite open
and migrations (99 ms), singleton registration, window creation and the first
frame. Target: under 250 ms to first frame.

### 7. Our names
- Crates `warp_*` → `uncaged_*`, binary `warp-oss` → `uncaged`, log file
  `warp-oss.log` → `uncaged.log`.
- Bundle id `dev.uncaged.WarpOss` → `dev.uncaged.Uncaged`, with a one-time move of
  the application-support, cache and preferences folders so nobody loses settings.
  Done: see "The identity rename" in RELEASING.md.
- Sweep remaining Warp names out of UI strings, docs and code comments.

## Decided defaults

- Shared sessions and the SSH remote server go, because both need Warp's servers.
  Either can come back later, built on our own infrastructure.
- Drive stays, backed locally.
- Upstream Warp changes are no longer merged (already true for `uncaged-0.3`);
  anything useful from upstream is ported by hand.
