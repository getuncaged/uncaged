# Uncaged — downloads manifest

Handoff spec for the website (`getuncaged.dev`) download grid. Mirrors the set of
formats Warp offers, mapped to Uncaged artifacts.

All release assets are published to **GitHub Releases** on
[`getuncaged/uncaged`](https://github.com/getuncaged/uncaged) by
[`.github/workflows/uncaged-release.yml`](.github/workflows/uncaged-release.yml)
when a tag `v*` is pushed. The website's Download buttons should point at the
**latest** release so they never need updating per version:

```
https://github.com/getuncaged/uncaged/releases/latest/download/<asset-name>
```

Asset naming is uniform: `Uncaged-<os>-<arch>.<ext>`
(`os` ∈ macos·linux·windows, `arch` ∈ aarch64·x86_64).

## Status legend
- **live** — produced by the release workflow today.
- **wired** — the packaging script exists and is added to the workflow; produced
  on the next release once its CI job is shaken out (see `RELEASING.md`).
- **planned** — needs a package repo / store submission that only takes effect
  after the first public release (Homebrew tap, winget-pkgs, AUR).

---

## macOS

| Format | Arch | Asset | Status |
|---|---|---|---|
| Disk image | Apple Silicon | `Uncaged-macos-aarch64.dmg` | live |
| Disk image | Intel | `Uncaged-macos-x86_64.dmg` | live |
| App zip | Apple Silicon | `Uncaged-macos-aarch64.zip` | live |
| App zip | Intel | `Uncaged-macos-x86_64.zip` | live |

Homebrew (planned — needs the `getuncaged/homebrew-tap` tap, formula in
[`packaging/homebrew/uncaged.rb`](packaging/homebrew/uncaged.rb)):

```bash
brew install --cask getuncaged/tap/uncaged
```

> The macOS build is **ad-hoc signed** (no Apple account). Gatekeeper quarantines
> it on first download; clear once with
> `xattr -dr com.apple.quarantine /Applications/Uncaged.app`. A notarized build
> is a drop-in once Apple signing secrets are added (see `RELEASING.md`).

## Linux

Requires x86_64 and aarch64. Built from the existing bundle scripts
(`script/linux/bundle*`).

| Format | Distros | Arch | Asset | Status |
|---|---|---|---|---|
| `.deb` | Debian, Ubuntu | x86_64 | `Uncaged-linux-x86_64.deb` | wired |
| `.deb` | Debian, Ubuntu | aarch64 | `Uncaged-linux-aarch64.deb` | wired |
| `.rpm` | Red Hat, Fedora, SUSE | x86_64 | `Uncaged-linux-x86_64.rpm` | wired |
| `.rpm` | Red Hat, Fedora, SUSE | aarch64 | `Uncaged-linux-aarch64.rpm` | wired |
| `.pkg.tar.zst` | Arch Linux | x86_64 | `Uncaged-linux-x86_64.pkg.tar.zst` | wired |
| `.pkg.tar.zst` | Arch Linux | aarch64 | `Uncaged-linux-aarch64.pkg.tar.zst` | wired |
| AppImage | any | x86_64 | `Uncaged-linux-x86_64.AppImage` | wired |
| AppImage | any | aarch64 | `Uncaged-linux-aarch64.AppImage` | wired |
| Tarball | any | x86_64 | `Uncaged-linux-x86_64.tar.gz` | live |

## Windows

| Format | Arch | Asset | Status |
|---|---|---|---|
| Installer `.exe` | x86_64 (Win 11/10) | `Uncaged-windows-x86_64-setup.exe` | wired |
| Installer `.exe` | ARM64 (Win 11/10) | `Uncaged-windows-aarch64-setup.exe` | wired |
| Zip | x86_64 | `Uncaged-windows-x86_64.zip` | live |

winget (planned — needs an `Uncaged.Uncaged` manifest submitted to
`microsoft/winget-pkgs`, template in [`packaging/winget/`](packaging/winget/)):

```powershell
winget install Uncaged.Uncaged
```

---

## Machine-readable (for the website's download component)

```json
{
  "repo": "getuncaged/uncaged",
  "latest_base_url": "https://github.com/getuncaged/uncaged/releases/latest/download",
  "install_commands": {
    "homebrew": "brew install --cask getuncaged/tap/uncaged",
    "winget": "winget install Uncaged.Uncaged"
  },
  "platforms": {
    "macos": [
      { "label": "Apple Silicon (.dmg)", "arch": "aarch64", "asset": "Uncaged-macos-aarch64.dmg", "status": "live" },
      { "label": "Intel (.dmg)", "arch": "x86_64", "asset": "Uncaged-macos-x86_64.dmg", "status": "live" }
    ],
    "linux": [
      { "label": ".deb (Debian, Ubuntu)", "arch": "x86_64", "asset": "Uncaged-linux-x86_64.deb", "status": "wired" },
      { "label": ".deb (Debian, Ubuntu)", "arch": "aarch64", "asset": "Uncaged-linux-aarch64.deb", "status": "wired" },
      { "label": ".rpm (Red Hat, Fedora, SUSE)", "arch": "x86_64", "asset": "Uncaged-linux-x86_64.rpm", "status": "wired" },
      { "label": ".rpm (Red Hat, Fedora, SUSE)", "arch": "aarch64", "asset": "Uncaged-linux-aarch64.rpm", "status": "wired" },
      { "label": ".pkg.tar.zst (Arch)", "arch": "x86_64", "asset": "Uncaged-linux-x86_64.pkg.tar.zst", "status": "wired" },
      { "label": ".pkg.tar.zst (Arch)", "arch": "aarch64", "asset": "Uncaged-linux-aarch64.pkg.tar.zst", "status": "wired" },
      { "label": "AppImage", "arch": "x86_64", "asset": "Uncaged-linux-x86_64.AppImage", "status": "wired" },
      { "label": "AppImage", "arch": "aarch64", "asset": "Uncaged-linux-aarch64.AppImage", "status": "wired" }
    ],
    "windows": [
      { "label": "Installer (Win 11/10 x64)", "arch": "x86_64", "asset": "Uncaged-windows-x86_64-setup.exe", "status": "wired" },
      { "label": "Installer (Win 11/10 ARM64)", "arch": "aarch64", "asset": "Uncaged-windows-aarch64-setup.exe", "status": "wired" }
    ]
  }
}
```
