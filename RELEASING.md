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

## 5. The full release matrix (all platforms)

The steps above are the local macOS path. In practice a release is cut by
**pushing a `v*` tag**, which triggers
[`.github/workflows/uncaged-release.yml`](.github/workflows/uncaged-release.yml).
That workflow builds every download format Uncaged offers on **standard
GitHub-hosted runners** (no self-hosted infra, no Warp secrets) and attaches
them to the GitHub Release. Asset names are the contract in
[`DOWNLOADS.md`](DOWNLOADS.md) — do not rename them.

| Platform | Runner | Formats | Status |
|---|---|---|---|
| macOS | `macos-latest` | `Uncaged-macos-{aarch64,x86_64,universal}.dmg` + `Uncaged-macos-aarch64.zip` | **first-class** (fails the release if broken) |
| Linux x86_64 | `ubuntu-latest` | `.deb`, `.rpm`, `.pkg.tar.zst`, `.AppImage`, `.tar.gz` | best-effort |
| Linux aarch64 | `ubuntu-24.04-arm` (native) | `.deb`, `.rpm`, `.pkg.tar.zst`, `.AppImage`, `.tar.gz` | best-effort |
| Windows x86_64 | `windows-latest` | `…-setup.exe` (Inno Setup) + `.zip` | best-effort |
| Windows aarch64 | `windows-latest` (cross) | `…-setup.exe` | best-effort |

Every non-macOS job is `continue-on-error: true`, so a release still publishes
whatever succeeds. All three macOS DMGs are ad-hoc signed via
`./script/bundle --channel oss --selfsign [--arch <arch>]` (universal omits
`--arch`).

### Linux packaging — what the fork changed

The Linux packages are built by the real `script/linux/bundle*` scripts, with
the Warp-only pieces stripped for the `oss` channel:

- **No apt/rpm repo wiring.** `script/linux/bundle_deb` skips appending the
  `releases.warp.dev` apt source + GPG key (`postinst.repo.template` /
  `postrm.repo.template`) for `oss`. The `.rpm` uses a dedicated
  `resources/linux/rpm/app/warp-oss.spec.template` whose `%post` only refreshes
  the desktop database — no GPG key install, no yum/zypper repo.
- **No PGP signing.** `script/linux/bundle_rpm` skips the `rpmsign` step for
  `oss` (Uncaged has no private signing key; packages are distributed unsigned
  via GitHub Releases).
- **Bundle id.** `script/linux/bundle` sets `BUNDLE_ID=dev.uncaged.WarpOss` for
  `oss`, matching the Rust runtime identity and the shipped desktop file
  `app/channels/oss/dev.uncaged.WarpOss.desktop`.

Runner tooling installed by the workflow: `dpkg-dev`+`fakeroot` (deb), `rpm`
(rpm), `libfuse2`+`linuxdeploy` (AppImage), `protoc`, and the graphics/X11 dev
libs. **Arch `.pkg.tar.zst`** needs `makepkg`, which is not present on
`ubuntu-latest`; the workflow builds it only when `makepkg` is available and
otherwise logs a skip. **aarch64** builds natively on GitHub's `ubuntu-24.04-arm`
runner (free for public repos), mirroring the x86_64 job — same deps, the same
`arduino/setup-protoc`, the same bundle driver. Kept best-effort so a slow or
flaky arm64 build never blocks a release. (It previously ran under QEMU
emulation, which silently used the host x86_64 toolchain and never truly
targeted arm64.)

### Windows packaging — what the fork changed

`script/windows/bundle.ps1 -Channel oss` builds the Inno Setup installer. For
`oss` it sets `APP_NAME=Uncaged` and `BUNDLE_ID=dev.uncaged.WarpOss`, and passes
the bundle id into `windows-installer.iss` as `/DBundleId=…` so the Start-menu /
desktop shortcut's `AppUserModelID` matches the running app (whose id is
hardcoded in Rust as `dev.uncaged.WarpOss`). The runner installs Inno Setup with
`choco install innosetup`. aarch64 cross-compiles fine but is kept best-effort
until validated on ARM hardware.

### Homebrew cask (macOS)

The cask lives in the tap **`getuncaged/homebrew-tap`** (repo
`https://github.com/getuncaged/homebrew-tap`), sourced from
[`packaging/homebrew/uncaged.rb`](packaging/homebrew/uncaged.rb). Users install
with:

```bash
brew install --cask getuncaged/tap/uncaged
```

**Updating the cask on release:** after a tag's release assets are published,
copy `packaging/homebrew/uncaged.rb` into the tap as `Casks/uncaged.rb` and set:

- `version` → the tag without the leading `v` (e.g. `0.1.0`),
- `sha256` → `shasum -a 256 Uncaged-macos-universal.dmg` for that release.

Then commit to the tap. (The cask points at the **universal** DMG so one formula
covers Apple Silicon and Intel.) This is a manual/scriptable step in the tap
repo; it does not run from this repo's workflow.

### winget (Windows)

The manifest skeleton lives under
[`packaging/winget/`](packaging/winget/) as the three-file form
(`Uncaged.Uncaged.yaml`, `.installer.yaml`, `.locale.en-US.yaml`) for package
identifier **`Uncaged.Uncaged`**. Users install with:

```powershell
winget install Uncaged.Uncaged
```

**Submitting on release:** replace the `0.0.0` placeholder version, the
`InstallerUrl`s (versioned release-asset URLs for the tag), and the
`InstallerSha256` values (uppercase SHA-256 of each `…-setup.exe`), then open a
PR to [`microsoft/winget-pkgs`](https://github.com/microsoft/winget-pkgs) under
`manifests/u/Uncaged/Uncaged/<version>/`. The `wingetcreate` tool can automate
the version bump and PR.

## Notes

- The private-use glyph at U+E500 that upstream used for the Warp wordmark is
  **not rendered anywhere** in Uncaged (the loading text uses U+203A `›`), so the
  copy of it that remains in the bundled Roboto font is dormant.
- `script/uncaged-setup` and the in-app gallery both write `~/.uncaged/engine.json`;
  no build step is needed to change the active model.
