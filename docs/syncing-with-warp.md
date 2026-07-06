# Keeping Uncaged in sync with upstream Warp

Uncaged is a downstream fork of the open-source [Warp](https://github.com/warpdotdev/warp)
client. Warp keeps improving the core (renderer, blocks, editor, CLI-agent
detection, …); we want those improvements for free while keeping our changes
(the local `uncaged_engine`, the account/cloud/telemetry removals, the rebrand,
the local Drive). This doc is how we do that with the least pain.

## The one thing that determines fork health: a *thin, isolated* diff

Every line we change in a file Warp also maintains is a potential merge conflict
**forever**. So the whole strategy is to keep our diff from upstream as small and
as isolated as possible. Four rules:

1. **Add, don't edit.** Put new behavior in *new* files/crates. These never
   conflict. We already do this: [`crates/uncaged_engine`](../crates/uncaged_engine),
   `app/src/gist_sync.rs`, `app/src/drive/config_panel.rs`,
   `app/src/settings_view/about_page.rs` additions, the brand SVGs, the
   `.github/workflows/uncaged-*.yml`.
2. **Gate, don't rewrite.** When you *must* touch a Warp-maintained file, wrap
   the change in the tightest possible
   `if matches!(warp_core::channel::ChannelState::channel(), Channel::Oss) { … }`
   branch instead of rewriting the surrounding logic. Upstream edits then merge
   cleanly *around* our small inserted block. Every auth/Drive gate we added is a
   short `if Oss { return; }` for exactly this reason.
3. **Centralize the brand.** All names/marks/icons live in
   [`app/src/brand.rs`](../app/src/brand.rs) + a few SVGs, so re-skinning is one
   isolated file, not scattered string edits across the tree.
4. **Never touch upstream files cosmetically.** No reformatting, no reordering,
   no drive-by cleanups in Warp-owned code — that manufactures conflicts for zero
   benefit.

If you keep the diff thin, a sync is usually a clean merge plus a handful of
trivial conflict resolutions.

## Repo topology

- **Dev repo of record** — a *full-history* clone of Warp with two remotes:
  - `origin` → our fork (`antonarnaudov/warp` today; can be a
    `getuncaged/uncaged-dev` later)
  - `upstream` → `https://github.com/warpdotdev/warp.git`
  This is the **only** place `git merge upstream` works, because it shares
  history with Warp.
- **Public repo** — `getuncaged/uncaged` is published with **fresh history**
  (clean, standalone, no "forked from" banner; attribution lives in `NOTICE` +
  `README`). It has *no* common ancestor with `warpdotdev/warp`, so you **cannot**
  `git merge upstream` there. It receives **published snapshots** from the dev
  repo, not merges.

## The sync workflow

Run in the **full-history dev repo** (not the public snapshot):

```bash
# 1. Get the latest upstream
git fetch upstream

# 2. See what's new since our last sync
git log --oneline HEAD..upstream/main | head -50

# 3. Merge upstream into our Uncaged branch
git switch <uncaged-branch>
git merge upstream/main
#    Resolve conflicts — they'll be concentrated in the few Warp files we gated
#    (see "Where conflicts land" below). Keep BOTH: upstream's new logic AND our
#    `if Oss { … }` guard. Never drop a gate to make a conflict go away.

# 4. Build + run the checks — a clean text-merge can still break behavior
cargo build --bin warp-oss
cargo test -p uncaged_engine            # + any live tests you rely on

# 5. Smoke-test the running app (no sign-up modal, Drive move, model connect)

# 6. Publish a snapshot to the public repo (see script/publish-public.sh idea below)
```

`git merge` (not `rebase`) for a **published** line — it doesn't rewrite history,
so nobody's clone breaks. Keep a private rebased branch too if you like seeing
our patch set cleanly on top of upstream, but publish via merge/snapshot.

## Where conflicts land (our current edit surface)

New files never conflict. The Warp-maintained files we currently touch — watch
these during a merge:

- `app/src/workspace/view.rs` — titlebar launchers, auth-modal backstops, AISettings subscription
- `app/src/drive/index.rs` — Oss gates on move/menu/online-only ops
- `app/src/server/cloud_objects/update_manager.rs` — the local move-persist branch
- `app/src/settings_view/ai_page.rs` — CLI-agent list + experimental gate
- `app/src/auth/auth_manager.rs`, `crates/warp_server_auth/src/auth_state.rs` — account-gate no-ops
- `app/src/terminal/cli_agent.rs` — install/visibility model
- `app/src/server/cloud_objects/listener.rs` — Oss websocket skip

Because each of these is a small gated insertion, the merge markers are usually
"upstream changed the function body; we added an early `if Oss` return at the
top" — trivial to reconcile.

## Cadence

- Sync on **Warp releases / weekly**, not daily — batch the churn.
- Pin to an upstream **tag** when Warp tags releases; otherwise track `main`.
- After each sync, cut a new Uncaged release snapshot to the public repo so the
  website's download link picks it up.

## Helper

`script/sync-upstream.sh` automates steps 1–3 (fetch, show the delta, attempt the
merge, and report any conflicting files) so a sync starts with one command.
