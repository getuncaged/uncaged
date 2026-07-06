# Uncaged — direction

Where we're taking Uncaged, decided against a real benchmark: **Zap**
([zerx-lab/zap](https://github.com/zerx-lab/zap), site zap.zerx.dev), a sibling
account-free, BYO-model AGPL fork of the open-source Warp terminal. Zap was
formerly "OpenWarp" and is further along on visible productization. This doc
records what we learned from studying it, what we changed because of it, and what
we deliberately keep different.

> Bottom line: **We and Zap are doing substantively the same thing, and their
> independent choices confirm most of ours were right.** Zap is ahead on visible
> polish (marketing site, i18n, launch modal, cross-platform release cadence) and
> on two real technical points (a source-level egress sentinel, OS-keychain key
> storage). We are ahead on brand architecture (`brand.rs`), attribution rigor
> (NOTICE), and depth of Warp-surface removal. The move is **not to imitate Zap**
> — it's to close a short list of low-regret gaps, then package our
> equal-or-stronger substance with the polish they already have.

---

## How we compare, dimension by dimension

| Dimension | Ahead | Why |
|---|---|---|
| Positioning / marketing | **Zap** | Crisp one-line thesis + a resolving site. Ours is scattered across README/UNCAGED.md. A copywriting gap, not a substance gap. |
| Brand architecture | **Uncaged** | We centralize name+palette+URL in `brand.rs`. Zap has no equivalent and scatters "Zap" across an About literal + i18n values over Warp-named keys. |
| UI/UX rebuild | **Zap** | They productized more *visible* surface (3-language UI, an in-app launch modal, an Astro site). But they *relabeled* Warp surfaces (drive/warpify/cloud); we *removed* more. Their edge is breadth of polish, not depth. |
| Model engine / BYO | **Zap** | They delegate protocols to the `genai` crate, store keys in the OS keychain, and pull a live `models.dev` catalog. We hand-roll anthropic/openai/acp with a static catalog and plaintext keys. Our ACP CLI-agent path is broader than theirs. |
| Privacy / egress | **Zap** (was) | They neutralized `app.warp.dev` at the *source* with an unroutable sentinel; we relied on gating every call site. **Closed tonight** — we adopted the sentinel. |
| Packaging / release | **Zap** | Full cross-platform artifact matrix (dmg arm64+intel, deb, rpm, AppImage, Windows, tarballs) via CI. We ship one ad-hoc macOS DMG. Both are ad-hoc signed — the gap is the CI matrix, not signing. |
| Docs / i18n | **Zap** | Tri-lingual READMEs + docs + a migrate-from-warp guide + a roadmap. We had none — **migrate guide added tonight.** |
| Legal / tooling | **tie** | We win attribution (real NOTICE; Zap has none and left `warp-coc@warp.dev` as its CoC contact). They win supply-chain automation (cargo-about/deny/git-cliff). We already have about.toml/deny.toml in-tree. |

---

## What Zap independently validated about our approach

Studying an independent team that solved the same problem is the cheapest
confidence we'll ever get. They arrived at the same answers on:

- **The privacy default set** — account-free, telemetry/crash-reporting/autoupdate
  off, keys local, model endpoint as the only intended egress.
- **Failing the agent seam closed** — hard-return rather than falling back to
  Warp's cloud. (We both do this. It is not overzealous.)
- **Channel-level isolation** — running our own `Channel::Oss` with a distinct
  bundle id (`dev.uncaged.WarpOss` vs their `dev.zap.Zap`) and a separate config
  dir keeps Warp cloud state out.
- **Not renaming the internal `warp_*` crates** — Zap kept them `warp`-named and
  rebranded only the product/packaging layer. This confirms our leftover `warp`
  package name is *acceptable to ship*, not release-blocking debt. It's an
  intentional product-vs-code boundary.
- **The exact hook point** — both intercept `generate_multi_agent_output` and
  preserve the proto envelope so the rest of the app is unchanged.
- **Ad-hoc macOS signing** — Zap shipped ~28 releases with the same
  `xattr`-quarantine story. Developer-ID notarization is legitimately deferrable.

---

## What we changed tonight because of the research

- **Source-level egress sentinel** *(adopted from Zap)* — `WarpServerConfig::uncaged_sentinel()`
  + `OzConfig::uncaged_sentinel()` point every Warp endpoint at the unroutable
  `192.0.2.0:9` (RFC 5737) for `Channel::Oss`, so any missed path fails to
  *connect* rather than reaching `app.warp.dev`. Also blanks the shipped Firebase
  key. This converts "every call site must be gated" into defense-in-depth.
- **`docs/migrate-from-warp.md`** *(idea from Zap)* — a Warp→Uncaged isolation +
  config-portability guide that doubles as a privacy proof-point.
- **Remaining user-visible "Warp" strings** — fixed the last few stragglers
  (prompt-editor header, two setting descriptions).
- **This doc + `MODIFYING.md`** — the direction record and the modder's manual.

---

## What we deliberately do NOT copy from Zap

- **Keep `brand.rs`.** Our centralized single-source-of-truth is objectively
  cleaner than Zap's scattered literals. Imitating them here would be a regression.
- **Keep disabling login outright.** Zap forces `is_logged_in()=true` in a "local
  mode" and keeps the auth UI degrading gracefully. Our hard-disable gives the
  cleaner accounts-removed UX we want. (Trade-off: Zap's choice yields fewer
  upstream-rebase conflicts — a conscious call, not an oversight.)
- **Keep our dedicated NOTICE + rewritten SECURITY/CONTRIBUTING/FAQ.** Zap's
  attribution is a single README line, no NOTICE, stale CoC contact. Do not loosen
  ours. (We could go one better with a one-line non-affiliation/trademark
  disclaimer.)
- **Keep our self-contained `uncaged_engine`.** Zap forked *two* upstreams (`genai`
  → `lib/rust-genai`, and Warp's proto-apis) — extra maintenance burden. Adopt the
  `genai` *idea* only if hand-rolled backends actually start to hurt.
- **Keep the ACP CLI-agent backend.** Zap has no equivalent; this is a place we're
  genuinely broader. Don't drop it to converge.
- **Do not copy Zap's leftovers.** Their bundle script hardcodes
  `APPLE_TEAM_ID=2BBY89MBSN` (Warp's own team id) and their CoC still says
  `warp-coc@warp.dev` — those are rename bugs. If we adapt their CI, parameterize
  the team id and fix every contact string.

---

## Backlog — ranked, for after this release

1. **Move API keys to the OS keychain.** *(security — highest priority)*
   Verified regression: `uncaged_engine/src/connections.rs` writes keys to
   plaintext `~/.uncaged/connections.json`. Plan: store secrets as one JSON blob
   under a single keychain item via the existing `warpui_extras` `secure_storage`
   (`security-framework` on macOS, `secret-service` on Linux); keep only
   non-secret metadata (base_url/model/kind) on disk; migrate-and-scrub any
   existing plaintext key on first read; fall back to file if the keychain is
   unavailable so "connect a model" never hard-breaks. *Deferred from tonight
   because it needs interactive runtime verification (a keychain-access prompt)
   that an unattended session can't approve.*
2. **Give `HOME_URL` a real destination + fix the ~194 in-app links.**
   *(release-blocking, but pending your call)* Every "docs/issues/source" link in
   the app now points at the canonical `getuncaged/uncaged`. The repo-identity
   decision below is resolved — one `brand.rs` constant + a sweep of stale links.
3. **Cross-platform release pipeline (CI).** Prebuilt dmg (arm64+intel), deb, rpm,
   AppImage, Windows, tarballs via GitHub Actions with SHA-pinned actions. Keep
   the Developer-ID/notarize path behind an env-secret flag; ship ad-hoc by
   default. Removes the build-from-source barrier that blocks non-dev adoption.
4. **Package the pitch.** Tighten the README to a three-pillar thesis (BYO any
   provider, keys local / no account or cloud / only egress is your endpoint) and
   add a "What Uncaged adds over Warp" section that reframes the hardening work as
   features. Optionally a small static site.
5. **Translated READMEs (en/zh/ja).** Cheap subset of Zap's i18n; full in-app UI
   localization is high-effort and can wait.
6. **`git-cliff` changelog automation** + wire the existing about.toml/deny.toml
   into a CI license-sync check.
7. **`models.dev` catalog integration** *(optional)* — a live model list instead
   of our hand-curated `catalog.rs`, if maintaining presets becomes a chore.

---

## Repo identity: decided

The product is "Uncaged" everywhere, and the repo identity is now settled:
the canonical slug is `getuncaged/uncaged` under the `github.com/getuncaged`
org, on the `getuncaged.dev` domain. The app's in-app links and `brand.rs`
point at this real slug; what remains is detaching from the `warpdotdev/warp`
fork network and keeping upstream attribution in NOTICE/README. Item 2 above is
now a mechanical sweep of any stale links.

---

## The thesis, unchanged

Every AI can power it — a hosted key, a local model, or a CLI agent — with nothing
sent anywhere you didn't ask for, fully open under AGPL-3.0. Zap proves the market
and the architecture. We ship the same substance with our own identity, cleaner
brand plumbing, and (now) source-level fail-closed privacy.
