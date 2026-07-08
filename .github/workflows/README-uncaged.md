# Uncaged CI/CD workflows

These `uncaged-*.yml` workflows are Uncaged's own, standard GitHub-hosted-runner
pipelines for [getuncaged/uncaged](https://github.com/getuncaged/uncaged). They
are intentionally separate from the upstream Warp workflows (`ci.yml`,
`create_release.yml`, etc.), which target Warp's private, self-hosted
infrastructure and secrets and are **not** used here.

Uncaged builds the GUI binary with `cargo build --bin warp-oss` (channel `oss`,
bundle id `dev.uncaged.WarpOss`). See `RELEASING.md` for the full release story.

## Workflows

| File | Trigger | What it does |
|------|---------|--------------|
| `uncaged-ci.yml` | push / PR to `master` or `main` | Builds `warp-oss` on macOS (gating) plus best-effort Linux/Windows builds; runs `cargo fmt --check`, and best-effort `clippy` + `test`. |
| `uncaged-review.yml` | PRs, and `@claude` comments | Runs Anthropic's `claude-code-action@v1` to review the diff for bugs and quality. |
| `uncaged-release.yml` | pushed tag `v*` | Builds per-OS packages and attaches them to a GitHub Release. |

macOS is the reference platform because the UI's shaders need Apple's Metal
toolchain (shipped with Xcode). Linux and Windows jobs are `continue-on-error`
best-effort builds — they may need extra system libraries — and never block CI.

## Required repo secrets

| Secret | Used by | Required? |
|--------|---------|-----------|
| `ANTHROPIC_API_KEY` | `uncaged-review.yml` | **Yes**, for Claude PR review. Add under Settings → Secrets and variables → Actions. Without it, only the review workflow fails; CI and release are unaffected. |
| `GITHUB_TOKEN` | all | Provided automatically by GitHub Actions; no setup needed. |

### Optional — Apple signing / notarization

The macOS release ships an **ad-hoc-signed** `.app`/`.dmg` (via
`./script/bundle --channel oss --selfsign`), which needs no Apple account.
Because it is not signed with an Apple Developer ID, macOS Gatekeeper quarantines
it on download; users clear it once with:

```bash
xattr -dr com.apple.quarantine /Applications/Uncaged.app
```

To ship a notarized build instead, add these repo secrets and switch the bundle
flags to the CODESIGN/notarize path in `script/macos/bundle`:

- `APPLE_CERT_P12_BASE64` — base64 of the Developer ID `.p12`
- `APPLE_CERT_PASSWORD` — password for that `.p12`
- `APPLE_TEAM_ID` — Apple Developer Team ID
- `APPLE_NOTARIZE_APPLE_ID` — Apple ID used for notarization
- `APPLE_NOTARIZE_PASSWORD` — app-specific password for that Apple ID

The release workflow guards this path and stays a no-op ad-hoc build until those
secrets exist.

## Release assets

`uncaged-release.yml` builds the **full download matrix** on standard
GitHub-hosted runners and attaches it to the GitHub Release. Assets are named
`Uncaged-<os>-<arch>.<ext>` — the exact names are the contract in
[`../../DOWNLOADS.md`](../../DOWNLOADS.md), so don't rename them in the workflow.

| Platform | Runner | Assets | Job status |
|---|---|---|---|
| macOS | `macos-latest` | `Uncaged-macos-aarch64.dmg`, `Uncaged-macos-x86_64.dmg`, `Uncaged-macos-universal.dmg`, `Uncaged-macos-aarch64.zip` | **first-class** (blocks the release) |
| Linux x86_64 | `ubuntu-latest` | `Uncaged-linux-x86_64.{deb,rpm,pkg.tar.zst,AppImage,tar.gz}` | `continue-on-error` |
| Linux aarch64 | `ubuntu-latest` (QEMU) | `Uncaged-linux-aarch64.{deb,rpm}` (best-effort) | `continue-on-error` |
| Windows x86_64 | `windows-latest` | `Uncaged-windows-x86_64-setup.exe`, `Uncaged-windows-x86_64.zip` | `continue-on-error` |
| Windows aarch64 | `windows-latest` | `Uncaged-windows-aarch64-setup.exe` | `continue-on-error` |

Every non-macOS job is `continue-on-error: true`, so a release still publishes
whatever succeeds. The three macOS DMGs are ad-hoc signed
(`./script/bundle --channel oss --selfsign`); the universal DMG omits `--arch`.

The Linux packages come from the real `script/linux/bundle*` scripts with the
Warp-only bits stripped for the `oss` channel (no `releases.warp.dev` apt/rpm
repo, no PGP signing); the Windows installer comes from
`script/windows/bundle.ps1 -Channel oss` (Inno Setup, installed on the runner
via `choco install innosetup`). See [`../../RELEASING.md`](../../RELEASING.md)
for the details, plus the Homebrew-cask and winget update steps.

Genuinely uncertain on hosted runners (best-effort, expect to shake out on a
real release run): the Arch `.pkg.tar.zst` (needs `makepkg`, absent on
`ubuntu-latest`), and both **aarch64** paths (Linux via QEMU emulation, Windows
via cross-compile).

They are published to the tag's release page, and the "latest" release — what the
website's Download button should link to — lives at:

<https://github.com/getuncaged/uncaged/releases/latest>
