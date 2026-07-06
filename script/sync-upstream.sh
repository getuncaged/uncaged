#!/usr/bin/env bash
#
# Sync the Uncaged fork with upstream Warp.
#
# Run this in the FULL-HISTORY dev repo (the clone that shares history with
# warpdotdev/warp), NOT the published snapshot repo. See docs/syncing-with-warp.md.
#
# It fetches upstream, shows what's new since the current branch, and attempts a
# merge — stopping cleanly and listing the conflicting files if there are any so
# you can resolve them (keep BOTH upstream's new logic and our `if Oss` guards).
#
# Usage:
#   script/sync-upstream.sh [upstream-ref]
#   script/sync-upstream.sh                # merges upstream/main
#   script/sync-upstream.sh upstream/v1.2  # merges a specific upstream tag/branch

set -u

UPSTREAM_URL="https://github.com/warpdotdev/warp.git"
UPSTREAM_REF="${1:-upstream/main}"

# Ensure the upstream remote exists (idempotent).
if ! git remote get-url upstream >/dev/null 2>&1; then
  echo "→ adding 'upstream' remote ($UPSTREAM_URL)"
  git remote add upstream "$UPSTREAM_URL"
fi

echo "→ fetching upstream…"
git fetch upstream --tags || { echo "✗ fetch failed"; exit 1; }

CURRENT_BRANCH="$(git rev-parse --abbrev-ref HEAD)"
echo
echo "Current branch : $CURRENT_BRANCH"
echo "Merging from   : $UPSTREAM_REF"
echo

# Refuse to run on a dirty tree — a merge needs a clean starting point.
if ! git diff --quiet || ! git diff --cached --quiet; then
  echo "✗ working tree is dirty. Commit or stash first, then re-run."
  exit 1
fi

NEW_COUNT="$(git rev-list --count "HEAD..$UPSTREAM_REF" 2>/dev/null || echo 0)"
if [ "$NEW_COUNT" -eq 0 ]; then
  echo "✓ already up to date with $UPSTREAM_REF — nothing to merge."
  exit 0
fi

echo "$NEW_COUNT new upstream commit(s) since this branch:"
git log --oneline "HEAD..$UPSTREAM_REF" | head -50
echo

echo "→ attempting merge (no fast-forward, no auto-commit so you can review)…"
if git merge --no-ff --no-commit "$UPSTREAM_REF"; then
  echo
  echo "✓ merged cleanly (staged, not committed). Review, then:"
  echo "    cargo build --bin warp-oss && cargo test -p uncaged_engine"
  echo "    git commit"
else
  echo
  echo "✗ merge has conflicts. Conflicting files:"
  git diff --name-only --diff-filter=U | sed 's/^/    /'
  echo
  echo "Resolve each: keep BOTH upstream's new logic AND our Uncaged guards"
  echo "(the small \`if matches!(ChannelState::channel(), Channel::Oss) { … }\`"
  echo "blocks). Then: git add -A && git commit"
  echo "To bail out entirely:  git merge --abort"
  exit 1
fi
