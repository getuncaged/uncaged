# Releasing Uncaged (macOS)

How to cut a distributable Uncaged build. Uncaged ships as an **ad-hoc signed**
`.app` inside a `.dmg`; there is no Apple Developer ID and no notarization, so
first-launch on another Mac needs a one-time quarantine clear (documented below
and in the README).

## 0. Prerequisites

- macOS on Apple Silicon (the primary target).
- Full **Xcode** installed and selected, with the Metal toolchain — the UI needs
  the Metal compiler:
  ```bash
  sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
  sudo xcodebuild -license accept
  xcodebuild -downloadComponent MetalToolchain
  ```
- The Rust toolchain pinned in `rust-toolchain.toml` (installed automatically by
  cargo on first build).
- [`create-dmg`](https://github.com/create-dmg/create-dmg) for the DMG step:
  `brew install create-dmg`.
- For a **universal** (arm64 + x86_64) build only:
  `rustup target add x86_64-apple-darwin`.
- Disk: a clean `release-lto` build needs well over 10 GB of free space on top of
  any existing `target/` directory. If space is tight, `cargo clean` first.

## 1. Set the version

The version lives in two places — keep them in sync:

- `app/Cargo.toml` → `[package] version = "X.Y.Z"`
- `app/src/bin/oss.rs` → `CFBundleShortVersionString` string

Then tag the release: `git tag vX.Y.Z && git push origin vX.Y.Z`.

Current version: **0.1.0**.

## 2. Build the app + DMG

The channel that produces Uncaged is **`oss`**. It builds the `warp-oss` binary
as the app named **Uncaged** (bundle id `dev.uncaged.WarpOss`), and — unlike the
upstream Warp channels — drops the `cocoa_sentry` feature, so **no crash
reporting / Sentry** is compiled in.

Single-architecture (Apple Silicon), ad-hoc signed — the common local release:

```bash
./script/bundle --channel oss --selfsign --arch aarch64
```

Universal binary (arm64 + x86_64), ad-hoc signed — for distributing one app that
runs on both architectures (requires the x86_64 target added above, ~2× build):

```bash
./script/bundle --channel oss --selfsign
```

What these flags do:

- `--channel oss` → profile `release-lto` (LTO, `debug_assertions` off, debug
  menus compiled out), features `release_bundle,extern_plist,gui,nld_classifier_v3,nld_heuristic_v2`.
- `--selfsign` → sign with a local *Apple Development* cert if one is in the
  keychain, otherwise **fall back to ad-hoc** (`codesign --sign -`). No company
  signing key, notarization, or Apple secrets are used or required.
- Omitting both `--selfsign` and `--nosign` would try the CI Developer-ID path
  and fail without Warp's secrets — always pass `--selfsign` for local releases.

Outputs (for `--arch aarch64`):

```
target/aarch64-apple-darwin/release-lto/bundle/osx/Uncaged.app
target/aarch64-apple-darwin/release-lto/bundle/osx/Uncaged.dmg
```

The DMG uses the volume name **Uncaged** and the Uncaged install background
(`app/assets/resources/mac/uncaged_install_image.png`), with a drag-to-Applications
layout.

> Verify compilation only, without producing a bundle:
> `./script/bundle --channel oss --selfsign --arch aarch64 --check-only`

## 3. Smoke-test the artifact

```bash
open target/aarch64-apple-darwin/release-lto/bundle/osx/Uncaged.app
```

Confirm on a clean profile (`~/.uncaged/` absent or renamed):

- App launches with the Uncaged icon and ember theme; no account / sign-in UI.
- **Settings → AI Models** connects a model (API key, local runtime, or CLI agent).
- Agent Mode works once a model is connected, and **fails with a clear "connect a
  model" message** when none is — it must never fall back to a remote server.
- No network egress except to the model endpoint you configured. (Autoupdate,
  analytics, telemetry, crash reporting, and login are all disabled on this
  channel; the agent seam hard-fails rather than POSTing to `app.warp.dev`.)

## 4. Distribute

Attach `Uncaged.dmg` to a GitHub Release on
<https://github.com/getuncaged/uncaged/releases> for the tag from step 1.

Because the app is ad-hoc signed (no Apple Developer ID), macOS Gatekeeper marks
it as quarantined on download. Tell users to clear it once after dragging the app
to Applications:

```bash
xattr -dr com.apple.quarantine /Applications/Uncaged.app
```

This is expected for an independent, source-available fork without a paid Apple
Developer account. If Uncaged ever obtains a Developer ID, switch the build to the
`CODESIGN`/notarize path in `script/macos/bundle` and drop this step.

## Notes

- The private-use glyph at U+E500 that upstream used for the Warp wordmark is
  **not rendered anywhere** in Uncaged (the loading text uses U+203A `›`), so the
  copy of it that remains in the bundled Roboto font is dormant.
- `script/uncaged-setup` and the in-app gallery both write `~/.uncaged/engine.json`;
  no build step is needed to change the active model.
