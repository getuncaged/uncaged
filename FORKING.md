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
- **`themes_url!` macro + `THEMES_REPO_*` constants** — the *community themes*
  repo (a separate repo from the main one) that the theme gallery browses and the
  theme editor opens PRs against. The `owner/repo` slug lives in one macro;
  `THEMES_REPO_URL` / `THEMES_RAW_BASE` / `THEMES_API_BASE`, the gallery-ref env
  var name, and the HTTP `User-Agent` all derive from `brand.rs`. Point the slug at
  your own gallery and the whole theme-download/share flow follows.
- `ember` / `ground` color modules — your accent gradient and neutral palette.
  (The bundled `Uncaged` / `Midnight` themes in `themes/default_themes.rs` are
  hand-tuned near these values but not bound to them, so change a default theme's
  look in that file, not here.)

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

## Removing a feature — everything Uncaged added is deletable

Uncaged's additions on top of Warp are built to come out cleanly. Each is a set
of **self-contained files** plus a few references in shared files. You don't need
a map of the references: **delete the files and remove the one or two enum
variants, then run `cargo build` — the compiler enumerates every remaining site**
(non-exhaustive `match`, unresolved name), and the tree is clean when it compiles.
Rust's exhaustiveness is the strip guide.

- **Theme gallery + "Explore themes" page** (browse/download/install community
  themes). Delete `app/src/themes/theme_gallery.rs`, `theme_explorer_body.rs`,
  `theme_gallery_tests.rs`, `test_data/gallery_index.json`, and
  `app/src/settings_view/theme_gallery_page.rs`; drop the `theme_explorer_body` /
  `theme_gallery` lines from `app/src/themes/mod.rs`; remove the `ThemeGallery`
  variant from `SettingsSection` (`settings_view/mod.rs`) and from
  `SettingsPageViewHandle` (`settings_view/settings_page.rs`). Build; fix what it
  names.
- **In-app theme editor page** (colour wheel, gradients, image, share-as-PR).
  Delete `app/src/settings_view/theme_creator_page.rs` and remove the
  `ThemeCreator` variant from the same two enums. Note the editor *widget*
  (`themes/theme_creator_body.rs`) is shared with the pre-existing
  `theme_creator_modal.rs`, so it stays unless you also strip the modal.
- **Background-image import** (`themes/theme_background_image.rs`) and **theme
  provenance grouping** (`ThemeGroup` in `themes/theme.rs`, the picker's group
  filter in `theme_chooser.rs`). These are prerequisites of the gallery, so remove
  them only after it, and again let the compiler point at the call sites.
- **Agent-vs-command triggering** is a config toggle, not a hard-wire: it lives
  behind the default-on `prefer_shell_for_known_commands` setting and the
  `agent_trigger` setting (`app/src/settings/ai.rs`). Set the default off, or
  remove the two settings and the guard in `crates/input_classifier`, to disable
  it. No brand strings involved.
- **Skills viewer** — `app/src/workspace/view/skills_panel.rs` plus its rows in
  `left_panel.rs`.

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
