# Homebrew cask for Uncaged — the account-free, bring-your-own-model fork of the
# open-source Warp terminal.
#
# This cask lives in the tap  getuncaged/homebrew-tap  (repo:
# https://github.com/getuncaged/homebrew-tap), so users install with:
#
#     brew install --cask getuncaged/tap/uncaged
#
# The `version` and `sha256` fields below are PLACEHOLDERS. On each release the
# tap is updated (see "Updating the cask" at the bottom) to point `version` at
# the new tag and `sha256` at the checksum of the universal DMG for that tag.
#
# Uncaged is AD-HOC signed (no Apple Developer ID / notarization). The
# `quarantine` note and the `caveats` below tell users how to clear Gatekeeper's
# quarantine on first launch; a notarized build would let us drop that step.

cask "uncaged" do
  # Bumped by the release process to the git tag without the leading "v"
  # (e.g. tag v0.1.0 -> version "0.1.0").
  version "0.0.0"

  # sha256 of Uncaged-macos-universal.dmg for the release named by `version`.
  # Replaced by the release process; use `sha256 :no_check` only for local testing.
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"

  # The universal DMG runs on both Apple Silicon and Intel, so one cask covers
  # every Mac. The download URL points at a versioned release asset (not
  # /latest/) so Homebrew can verify the checksum deterministically.
  url "https://github.com/getuncaged/uncaged/releases/download/v#{version}/Uncaged-macos-universal.dmg",
      verified: "github.com/getuncaged/uncaged/"

  name "Uncaged"
  desc "Account-free, bring-your-own-model fork of the Warp terminal"
  homepage "https://getuncaged.dev/"

  # No autoupdate feed: Uncaged ships with autoupdate disabled. `brew upgrade`
  # is the supported update path once the tap is bumped.
  auto_updates false
  depends_on macos: ">= :mojave" # matches the 10.14 MACOSX_DEPLOYMENT_TARGET

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
