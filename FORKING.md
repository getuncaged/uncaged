# Forking Uncaged — make it your own

Uncaged is built to be re-forked. It's already a fork of Warp with the identity
factored out, so standing up your own account-free, bring-your-own-model terminal
is a short, bounded edit — not a scavenger hunt. This is the whole point of the
project: **one file for identity, one file for providers, and the powerful Warp
engine underneath, untouched.**

If you just want to *use* or *modify behavior*, see [MODIFYING.md](MODIFYING.md).
This doc is specifically the **rebrand checklist**.

---

## The 10-minute rebrand

### 1. Identity — `app/src/brand.rs` (the single source of truth)

Change these and the whole app follows:

- `NAME`, `NAME_LOWER` — your product name (display + machine forms).
- `TAGLINE` — the one-liner.
- **`repo_url!` macro** — the base repository URL lives here in **exactly one
  place**. `HOME_URL`, `README_URL`, `ISSUES_URL`, `NEW_ISSUE_URL`,
  `DISCUSSIONS_URL`, and `PRIVACY_URL` all derive from it, and every in-app link
  (~190 call sites across 46 files) reads those constants. Change the one literal
  in the macro → every "docs / issues / source / learn-more" link in the app
  repoints. No sweep required.
- `ember` / `ground` color modules — your accent gradient and neutral palette.

### 2. The mark — `app/assets/bundled/svg/brand/`

- `uncaged-mark.svg` — the in-app monochrome glyph (drawn with the `rgb(255,0,0)`
  recolor sentinel; the app tints it to the theme).
- `uncaged-icon.svg` — the full-color app-icon master → `.icns` / PNGs.
- `app/assets/bundled/svg/uncaged-logo-{light,dark}-title.svg` — the horizontal
  "mark + wordmark" lockup shown on the About page (light/dark title variants).

Rename these to your product if you like — the paths are referenced from
`brand.rs` (mark/icon) and `about_page.rs` (lockups).

### 3. The models you offer — `crates/uncaged_engine/src/catalog.rs`

The `PRESETS` list is the "Connect a model" gallery. Add, remove, or reorder
providers here — CLI agents, local runtimes, and hosted APIs. Most new providers
are just one `Preset` entry (an OpenAI-compatible `base_url`); only a genuinely
new *protocol* needs engine code (see MODIFYING.md §3).

### 4. App identity (only if you want your own installable bundle)

These make the OS treat it as a distinct app (own prefs, own config dir, own URL
scheme) so it never collides with Warp or with Uncaged:

- `crates/warp_core/src/channel/state.rs` — `AppId::new("dev","uncaged","WarpOss")`.
- `crates/warp_core/src/paths.rs` — the `.uncaged` home-config dir name.
- `crates/warp_core/src/channel/mod.rs` — CLI command names + URL scheme.
- `app/Cargo.toml` `[package.metadata.bundle.bin.warp-oss]` + `app/src/bin/oss.rs`
  embedded plist — bundle id, display name, scheme, copyright.
- `crates/warp_core/src/channel/config.rs` — `uncaged_sentinel()` (the fail-closed
  Warp URLs); no change needed unless you rename the constructors.

### 5. Docs & legal

`README.md`, `NOTICE` (keep the upstream Warp attribution — AGPL requires it),
`SECURITY.md`, `CONTRIBUTING.md`, `FAQ.md`, `RELEASING.md`.

---

## What you get for free — do NOT rebrand these

- **The internal `warp_*` crates** (`warp_core`, `warpui`, `warp_multi_agent_*`,
  ~27 packages). They're the upstream engine; renaming them is ~2,000 files of
  churn for zero user benefit and it breaks every upstream merge. Leave them.
  (Zap, the other major fork, made the same call.) The product layer and the code
  layer are intentionally separate.
- **The privacy gates** — the fail-closed egress sentinel, disabled autoupdate,
  refused login, no-telemetry defaults, and the local-only agent seam all keep
  working for your fork automatically. See MODIFYING.md §6 to audit them after an
  upstream merge.
- **The agent seam + `uncaged_engine`** — your fork inherits the whole
  bring-your-own-model layer.

---

## Honest residuals

After the URL centralization, a handful of the old repo string still appears in
places that don't affect the running app, so a pedantic fork can mop them up but
doesn't have to:

- Two source **comments** (`resource_center/keybindings_page.rs`, `utils.rs`).
- Two **log/status messages** in the disabled cloud-agent SDK
  (`ai/agent_sdk/{ambient,driver}.rs`) — unreachable in Uncaged.
- One **config-template comment** (`user_config/mod.rs`).
- **Test fixtures** (`*_tests.rs`) that use the URL as sample link-detection data.
- Four **lower-crate** references (`warp_cli`, `onboarding`, `warpui`) that can't
  import the app's `brand` module — 2 doc comments, a CLI `--help` line, and a
  `TOS_URL` const in onboarding. Update these by hand if you want a spotless tree.

Everything a user actually clicks routes through `brand.rs`.
