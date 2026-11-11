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
- `crates/uncaged_app_theme/Cargo.toml` → same version (it feeds `brand::HTTP_USER_AGENT`)
- `app/src/bin/oss.rs` → `CFBundleShortVersionString` string

Then tag the release: `git tag vX.Y.Z && git push origin vX.Y.Z`.

Current version: **0.3.5**.

## 2. Build the app + DMG

The channel that produces Uncaged is **`oss`**. It builds the `uncaged` binary
as the app named **Uncaged** (bundle id `dev.uncaged.Uncaged`; up to v0.3.4 the
binary was `warp-oss` and the bundle id `dev.uncaged.WarpOss`, see
[The identity rename](#the-identity-rename-v034--next)). There is **no
crash reporting / Sentry** in the codebase.

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

**Release notes.** The release workflow's `generate_release_notes` lists merged PR
titles, and most work on the 0.3 line landed without PRs, so write the notes as
`docs/releases/vX.Y.Z.md` and apply them once the release exists:

```bash
gh release edit vX.Y.Z --notes-file docs/releases/vX.Y.Z.md
```

### Pre-release checklist

- `app/Cargo.toml` and `app/src/bin/oss.rs` (`CFBundleShortVersionString`) carry the
  same version.
- `cargo fmt --all --check` is clean.
- Clippy is clean with both feature sets, the same two runs `uncaged-ci.yml` gates on
  (each set compiles code the other doesn't):
  ```bash
  cargo clippy -p uncaged -p uncaged_engine --all-targets --tests -- -D warnings -A dead_code -A clippy::doc_lazy_continuation -A clippy::useless_conversion
  cargo clippy -p uncaged -p uncaged_engine --all-targets --all-features --tests -- -D warnings -A dead_code -A clippy::doc_lazy_continuation -A clippy::useless_conversion
  ```
- The latest Uncaged CI run on `uncaged-0.3` is green, including the Linux and Windows
  jobs (best-effort, so they don't block, but a red one there is a platform-only break).
- `cargo nextest run --no-fail-fast --workspace`:
  unit tests all pass. Integration tests that need SSH targets / containers or
  your own shell setup (`test_ssh_*`, `test_preferred_shell`, PS1 checks) fail
  locally; compare against the previous release before treating one as new.
- Build the bundle, copy it to `/Applications/Uncaged.app`, and smoke-test the mode
  toggle, the model/effort chip, Settings and a new tab.

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
| macOS | `macos-latest` | `Uncaged-macos-{aarch64,x86_64}.dmg` + `.zip` | **first-class** (fails the release if broken) |
| Linux x86_64 | `ubuntu-latest` | `.deb`, `.rpm`, `.pkg.tar.zst`, `.AppImage`, `.tar.gz` | best-effort |
| Linux aarch64 | `ubuntu-24.04-arm` (native) | `.deb`, `.rpm`, `.pkg.tar.zst`, `.AppImage`, `.tar.gz` | best-effort |
| Windows x86_64 | `windows-latest` | `…-setup.exe` (Inno Setup) + `.zip` | best-effort |
| Windows aarch64 | `windows-latest` (cross) | `…-setup.exe` | best-effort |

Every non-macOS job is `continue-on-error: true`, so a release still publishes
whatever succeeds. Both macOS DMGs are ad-hoc signed via
`./script/bundle --channel oss --selfsign --arch <arch>`.

### Linux packaging — what the fork changed

The Linux packages are built by the real `script/linux/bundle*` scripts, with
the Warp-only pieces stripped for the `oss` channel:

- **No apt/rpm repo wiring.** `script/linux/bundle_deb` skips appending the
  `releases.warp.dev` apt source + GPG key (`postinst.repo.template` /
  `postrm.repo.template`) for `oss`. The `.rpm` uses a dedicated
  `resources/linux/rpm/app/uncaged.spec.template` whose `%post` only refreshes
  the desktop database — no GPG key install, no yum/zypper repo.
- **No PGP signing.** `script/linux/bundle_rpm` skips the `rpmsign` step for
  `oss` (Uncaged has no private signing key; packages are distributed unsigned
  via GitHub Releases).
- **Bundle id.** `script/linux/bundle` sets `BUNDLE_ID=dev.uncaged.Uncaged` for
  `oss`, matching the Rust runtime identity and the shipped desktop file
  `app/channels/oss/dev.uncaged.Uncaged.desktop`.
- **Package name.** The binary is `uncaged`, but the package stays
  `warp-terminal-oss` (`PACKAGE_CHANNEL_SUFFIX=-oss`) so a new package upgrades
  an old install in place; `/opt/warpdotdev/warp-terminal-oss/warp-oss` is a
  compat symlink to `uncaged`.

Runner tooling installed by the workflow: `dpkg-dev`+`fakeroot` (deb), `rpm`
(rpm), `libfuse2`+`linuxdeploy` (AppImage), and the graphics/X11 dev
libs. **Arch `.pkg.tar.zst`** needs `makepkg`, which is not present on
`ubuntu-latest`; the workflow builds it only when `makepkg` is available and
otherwise logs a skip. **aarch64** builds natively on GitHub's `ubuntu-24.04-arm`
runner (free for public repos), mirroring the x86_64 job — same deps, the same
bundle driver. Kept best-effort so a slow or
flaky arm64 build never blocks a release. (It previously ran under QEMU
emulation, which silently used the host x86_64 toolchain and never truly
targeted arm64.)

### Windows packaging — what the fork changed

`script/windows/bundle.ps1 -Channel oss` builds the Inno Setup installer. For
`oss` it sets `APP_NAME=Uncaged`, `BINARY_NAME=uncaged.exe` and
`BUNDLE_ID=dev.uncaged.Uncaged`, and passes the bundle id into
`windows-installer.iss` as `/DBundleId=…` so the Start-menu / desktop shortcut's
`AppUserModelID` matches the running app (whose id is `dev.uncaged.Uncaged`,
set in `app/src/bin/oss.rs`). The Inno `AppId` (`warp-terminal-oss`) is
deliberately unchanged: it is what makes a new installer upgrade the existing
install instead of installing beside it. The runner installs Inno Setup with
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
edit `Casks/uncaged.rb` in the tap and set:

- `version` → the tag without the leading `v` (e.g. `0.3.0`),
- the `on_arm` `sha256` → the checksum of `Uncaged-macos-aarch64.dmg`,
- the `on_intel` `sha256` → the checksum of `Uncaged-macos-x86_64.dmg`.

GitHub records each asset's checksum, so nothing needs downloading:

```bash
gh api repos/getuncaged/uncaged/releases/tags/vX.Y.Z \
  -q '.assets[] | select(.name | endswith(".dmg")) | "\(.name) \(.digest)"'
```

Then commit to the tap. The cask uses the per-arch DMGs; there is no universal
DMG. This is a manual step in the tap repo; it does not run from this repo's
workflow.

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

## The identity rename (v0.3.4 → next)

The release after v0.3.4 renames the app's identity: binary `warp-oss` →
`uncaged`, bundle id / data domain `dev.uncaged.WarpOss` → `dev.uncaged.Uncaged`,
log `warp-oss.log` → `uncaged.log`. Everything below exists so v0.3.4 can still
update to it and so existing data comes along.

### How v0.3.4 updates to the renamed build

- **macOS.** v0.3.4 never swaps the bundle itself: it runs the *old* bundle's
  `Contents/Resources/bin/uncaged-update --restart` and quits. That script waits
  for every process under `/Applications/Uncaged.app/Contents/MacOS/`, then runs
  `brew upgrade --cask uncaged` (Homebrew installs) or
  `curl -fsSL https://getuncaged.dev/install.sh | bash`, which downloads
  `Uncaged-macos-<arch>.zip`, expects `Uncaged.app` at its top, moves it to
  `/Applications/Uncaged.app` and opens it. None of that names the executable or
  the bundle id, so the published names it relies on are: the asset names
  `Uncaged-macos-{aarch64,x86_64}.{zip,dmg}` (they also carry v0.3.4's in-app
  asset fragments `macos-aarch64` / `macos-x86_64`), the app name `Uncaged.app`
  in the zip and DMG, and `Contents/Resources/bin/uncaged-update` in the new
  bundle (for the *next* update). `app/src/autoupdate/release_names_tests.rs`
  pins all of them against the workflow and bundle scripts.
- **Linux.** v0.3.4 does not self-update on Linux; it reports the new version
  and the user updates the way they installed. The package name stays
  `warp-terminal-oss` so `apt`/`dnf`/`pacman` upgrade in place; the tarball's
  binary was already named `uncaged`.
- **Windows.** v0.3.4's in-app updater cannot download anything on this
  channel (its installer URL has no base for `oss`), so it never updates itself;
  users run the new `Uncaged-windows-<arch>-setup.exe` (or winget). The Inno
  `AppId` is unchanged so that upgrades in place.

### First launch: the data migration

`app/src/identity_migration.rs` runs first thing in `uncaged::run()` (right
after argument parsing), before logging, settings, secure storage or sqlite.
Only the GUI launch moves anything; a CLI or worker invocation that finds data
still to move uses the old identity's data for that process.

- It moves (renames) each directory the old identity owns to the new name —
  macOS `~/Library/{Application Support,Caches}/dev.uncaged.WarpOss` and
  `~/Library/Saved Application State/dev.uncaged.WarpOss.savedState`; Linux
  `~/.local/state`, `~/.local/share`, `~/.config`, `~/.cache` `warp-oss` →
  `uncaged`; Windows `%LOCALAPPDATA%` and `%APPDATA%` `uncaged\WarpOss` →
  `uncaged\Uncaged` — only if the new location does not exist or holds nothing
  but empty directories (never merges),
  then leaves a symlink (Windows: junction) at the old path so a downgrade to
  v0.3.4 finds the same data. `~/.uncaged` is not touched.
- If another `uncaged`/`warp-oss` process is running or the old `warp.sqlite`
  is open, nothing moves: that launch keeps using the old identity's data and
  the next launch retries. A failed rename puts the earlier ones back.
- NSUserDefaults (`dev.uncaged.WarpOss` → `dev.uncaged.Uncaged`) and the
  Windows registry preferences (`HKCU\Software\Warp.dev\WarpOss` → `…\Uncaged`)
  are copied once, keeping the old ones.
- Secrets stored under the old service name are read through and copied to the
  new one on first use; the old copies stay (`secure_storage/legacy.rs`).
- The outcome is logged at startup (`Identity migration: …` in `uncaged.log`).

### Compat shims and when to remove them

| Shim | Why | Remove when |
| --- | --- | --- |
| `Contents/MacOS/warp-oss` → `uncaged` symlink (macOS bundle) | Login items, scripts and the v0.3.4 updater's executable check point at the old path | No supported release predates the rename and the in-app staged-bundle check no longer accepts `warp-oss` |
| `Contents/Resources/bin/warp-oss` → `uncaged` (macOS CLI) | Users symlinked the old CLI onto `PATH` | Same, with a release note telling users to relink `bin/uncaged` |
| `/opt/warpdotdev/warp-terminal-oss/warp-oss` → `uncaged` (Linux packages, flake) | Scripts calling the old binary | Same |
| `bin\warp-oss.cmd` (Windows) | Old CLI name | Same |
| Package name `warp-terminal-oss`, Inno `AppId=warp-terminal-oss` | In-place upgrades | Never (renaming them breaks upgrades) |
| `uncagedoss://` URL scheme (still the primary one; `uncaged://` added) | Existing links and MCP OAuth redirect URIs | When no registered OAuth client uses the old redirect URI |
| Identity migration, legacy secure-storage read-through, `WarpOss` → `Warp-Oss` Linux mapping in `paths.rs` | Existing installs' data | Once no supported release predates the rename; the old-path symlinks can then be left to users |
| `app_name`/executable checks accepting both `uncaged` and `warp-oss` in `autoupdate/mac.rs` | The new updater can install a bundle from either side of the rename | With the `Contents/MacOS/warp-oss` shim |

### Release-notes draft (user-visible consequences)

> **Uncaged is now `dev.uncaged.Uncaged`.** The app's internal identity changed
> from `dev.uncaged.WarpOss` / `warp-oss`. Your settings, history, Drive objects
> and secrets move over automatically the first time the new version starts
> (quit any other Uncaged windows first; if one is still open the move waits for
> the next launch). Downgrading still works: the old locations point at the new
> ones.
>
> Because macOS treats it as a new app, you may need to:
> - re-grant **Full Disk Access**, **Accessibility**, **Automation** and other
>   privacy permissions in System Settings → Privacy & Security;
> - set Uncaged as the default terminal / "Open With" handler again;
> - re-pin it in the Dock;
> - on Windows, re-pin the taskbar shortcut (its AppUserModelID changed).
>
> The command-line name is now `uncaged`; `warp-oss` still works for now. The
> log file is `~/Library/Logs/uncaged.log` (macOS) — older logs stay in
> `warp-oss.log`.
>
> **Uncaged no longer starts at login.** The "Start Uncaged at login" setting is
> gone, and the first launch removes the login item earlier versions registered.
> If an entry is still listed (System Settings → General → Login Items on macOS,
> Startup apps on Windows), remove it there.

## Notes

- The private-use glyph at U+E500 that upstream used for the Warp wordmark is
  **not rendered anywhere** in Uncaged (the loading text uses U+203A `›`), so the
  copy of it that remains in the bundled Roboto font is dormant.
- `script/uncaged-setup` and the in-app gallery both write `~/.uncaged/engine.json`;
  no build step is needed to change the active model.
