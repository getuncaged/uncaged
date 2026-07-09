# Homebrew cask for Uncaged — the account-free, bring-your-own-model fork of the
# open-source Warp terminal.
#
# This cask lives in the tap  getuncaged/homebrew-tap  (repo:
# https://github.com/getuncaged/homebrew-tap), so users install with:
#
#     brew install --cask getuncaged/tap/uncaged
#
# The `version` and the two per-arch `sha256` fields are PLACEHOLDERS. On each
# release the tap is updated (see "Updating the cask" at the bottom) to point
# `version` at the new tag and each `sha256` at the checksum of the matching
# per-arch DMG for that tag.
#
# Uncaged is AD-HOC signed (no Apple Developer ID / notarization). The
# `quarantine` caveats below tell users how to clear Gatekeeper's quarantine on
# first launch; a notarized build would let us drop that step.

cask "uncaged" do
  # Bumped by the release process to the git tag without the leading "v"
  # (e.g. tag v0.2.3 -> version "0.2.3"). Pinned to the current release; on each
  # new release, bump `version` and both per-arch `sha256` below to that tag's
  # DMG checksums (see "Updating the cask").
  version "0.2.4"

  # Per-arch DMGs (there is no universal DMG). Homebrew picks the block matching
  # the host. The download URL points at a versioned release asset (not /latest/)
  # so Homebrew can verify the checksum deterministically.
  on_arm do
    sha256 "aaa7dae799b2337b02077874f2998809d85ee82cd4d2fc7bd6405744532704fe"
    url "https://github.com/getuncaged/uncaged/releases/download/v#{version}/Uncaged-macos-aarch64.dmg",
        verified: "github.com/getuncaged/uncaged/"
  end
  on_intel do
    sha256 "2a0446f274c0ee435c8c28a5beba2d25dfcbce35f121bc0377d6407718e94cfd"
    url "https://github.com/getuncaged/uncaged/releases/download/v#{version}/Uncaged-macos-x86_64.dmg",
        verified: "github.com/getuncaged/uncaged/"
  end

  name "Uncaged"
  desc "Account-free, bring-your-own-model fork of the Warp terminal"
  homepage "https://getuncaged.dev/"

  # No autoupdate feed: Uncaged ships with autoupdate disabled. `brew upgrade`
  # is the supported update path once the tap is bumped.
  auto_updates false
  # NOTE: no `depends_on macos:` minimum — modern Homebrew disabled the
  # minimum-version constraint for casks (there is no replacement). The app
  # targets macOS 10.14+ (MACOSX_DEPLOYMENT_TARGET); older systems simply
  # can't launch it, which macOS itself enforces.

  app "Uncaged.app"

  # Because the app is ad-hoc signed, macOS quarantines it on download. Homebrew
  # normally strips the quarantine attribute for casks it installs; if Gatekeeper
  # still complains, the caveats below give the manual one-liner.
  caveats <<~EOS
    Uncaged is ad-hoc signed (no Apple Developer ID). If macOS Gatekeeper blocks
    it on first launch, clear the quarantine attribute once:

      xattr -dr com.apple.quarantine "#{appdir}/Uncaged.app"
  EOS

  # Remove Uncaged's user data on `brew uninstall --zap uncaged`. Uncaged stores
  # everything under ~/.uncaged and namespaces app-support/caches/prefs by its
  # bundle id dev.uncaged.WarpOss.
  zap trash: [
    "~/.uncaged",
    "~/Library/Application Support/dev.uncaged.WarpOss",
    "~/Library/Caches/dev.uncaged.WarpOss",
    "~/Library/Preferences/dev.uncaged.WarpOss.plist",
    "~/Library/Saved Application State/dev.uncaged.WarpOss.savedState",
    "~/Library/Logs/dev.uncaged.WarpOss",
  ]
end
