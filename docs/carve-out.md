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
