# Branch strategy (decided 2026-08-21)

**This branch is the split.** `uncaged-0.3` is the divergence line from the Warp codebase
(it absorbs `test-combined` plus the theme-editor guard, the CI fix, and the Warp-identity
removal; versioned 0.3.0, unreleased):
nothing on it merges to `main` and nothing releases until the split is deliberate and named.
`main` stays a clean mirror-plus-minimal-patches of upstream for as long as that is useful.
All open PRs against `main` (#22, #27, #29–#33) are converted to draft so nothing lands by
reflex; they now serve as CI vehicles and documentation of the work, not as merge requests.
When the split gets its real name, rename this branch once and retarget the PRs or close them.

# Progress ledger (updated as work lands — measurements, not intentions)

## Landed, with measurements

| commit | change | measured effect |
|---|---|---|
| `1a4a89f` | ONNX classifier built on first use; `render_terminal_input` deleted (301 lines, proven unreachable) | startup no longer decodes 17.5 MB on the main thread |
| `d60d922` + `9beec6f` | 27 cloud features out of `default` | **no measurable RSS/thread change** — they gate UI, not runtime; kept as build hygiene |
| `d943c6d` | ONNX model out of all bundles | binary −18.7 MB |
| `68fc0a8` | watcher debounce 200 ms/500 ms → 2 s (repo_metadata kept tight) | **idle wakeups ~5.0/s → 0.3/s** — the Activity Monitor energy fix |
| menu-unification commit | `render_active_inline_menu` shared across surfaces | fixes the slash-menu regression the routing change shipped; first time /rewind and /plan render outside the agent view |
| font/checkbox commit | font enumeration deferred to the import UI; NLD checkbox out of the zero state | malloc_history attributed **~1.17 GB of font-file mmaps** to the startup import scan — top four allocation sites in the process |

Baseline (before tonight): binary 401 MB · `ps` RSS 376 MB · physical footprint 196.8 MB · 61 threads · ~5 wakeups/s.
After the watcher+ONNX build: binary 359 MB · footprint 196.7 MB · wakeups 0.3/s. Font-deferral build: measurement pending.

### Final-build memory readings (for the next session's baseline)

Last build of the night, restored session incl. Settings tab: peak footprint **383.8 MB**
(was 399.9 before the font fix — the startup spike shrank), 303.5 MB at T+7 min and still
decaying; the earlier 196.7 MB reading was the same session at T+34 min. Compare at equal
uptime only. Heap attribution (99.6 MB `MALLOC_SMALL` vs Terminal.app 7.8 MB) is the next
target; the font mappings no longer pollute the profile, so a fresh `MallocStackLogging=lite`
run will now show the real heap owners directly.

### Heap attribution (MSL run, post-font-fix — question closed)

Top live owners: GridStorage 10.0 MB (scrollback, 7 panes — legitimate), sqlite cache 5.2 MB,
ANSI parser states 3.4 MB (167 × 20 KB — count worth a look someday), compiled regexes 3.4 MB,
parsed skills 3.2 MB, then a long tail of 1–2 MB entries. **No single dominant owner.** Heap
micro-optimisation is diminishing returns; the remaining structural lever is compiled-code
residency (drive/cloud_object gating, in progress on this branch).

### drive/cloud_object gating: attempted, measured, withdrawn

Gating `mod drive;` produced 169 first-wave errors (unresolved imports only — the usage wave
behind them is larger). Before paying that, the payoff check: vmmap shows only 72.5 MB of the
274 MB `__TEXT` resident — **pages that never execute are never paged in**, so dead cloud code
costs disk, not RAM. Gating buys a few MB of binary and ~zero footprint, against 169+ risky
edits through live UI files. Withdrawn. The survey's "terminal is a rounding error next to the
cloud client" was about compiled size; compiled-dead is runtime-free. If binary size ever
matters (download weight), revisit with the stub-module approach and budget a full day.

### Step 9 slice 1 (this session): per-keystroke and per-frame constants

Landed together, compile- and test-verified:

- **A1** `update_rich_content_heights` returns early on an empty map (`blocks.rs`) — kills the
  ≥2 full SumTree rebuilds per layout at idle (§4.4 item 1, "cheapest large frame-cost win").
- **A2** `find_potential_autosuggestions_from_history` returns **deduplicated command Strings**
  instead of cloning whole 8-String `HistoryEntry`s per match (§4.3 item 1). Dedup matters as
  much as the clone: repeated history commands were re-validated one by one by the consumer.
  Both consumers (input.rs, next_command_model.rs) take the first `.command` that validates.
- **A3 (reshaped)** the plan said "use `last_buffer_text()` at the comparison sites" — **wrong,
  do not do this**: `last_buffer_text` is the *pre-edit* text (set in `start_batch` before the
  edit applies; see `test_last_buffer_text`). Instead `run_input_background_jobs` now builds
  the buffer String **once** per tick instead of three times (decorations.rs). The rebuild
  inside `apply_decorations` stays — it is the race-guard against the async parse snapshot.
- **A4** `filtered_blocks()` skips the per-render scan of every block via a self-healing
  `may_have_filtered_blocks: Cell<bool>` over-approximation (§4.4 item 3). `bookmarked_blocks`
  was already a keys-copy of a maintained map — left alone.
- **A6** `parse_markdown_cached` (new, in `markdown_parser`) — bounded thread-local memo;
  the three per-render parse sites (zero-state, inline action header, ask-user question) now
  hit it (§4.4 item 8).
- **A8** first-ever SQLite indexes: `commands(session_id, id)`, `commands(command, pwd)`,
  `blocks(pane_leaf_uuid)` (migration `2026-08-22-000000_add_hot_path_indexes`, §3.2/§4.3 item 3).
- **A9** `set_zero_state_hint_text` static-command hint placeholders precomputed in a
  `LazyLock` instead of re-walking `COMMAND_REGISTRY` with fresh `format!`s ×16 call sites (§4.3 item 5).

Deliberately **not** done, with reasons:

- **WarpTheme per-element clone (§4.4 item 7): skipped.** Measured the type: `AssetSource` is
  path/`Arc`-based, so a clone is ~2 small heap allocs, and both sites run once per render
  pass, not per block. Not worth an `Arc<WarpTheme>` churn through `GridRenderParams`.
- **Overhang pass dirty-set (§4.4 item 2): deferred.** `rich_content_elements` are rebuilt
  fresh each frame (`with_rich_content` inserts new `Box<dyn Element>`s), so there is no
  cached size to reuse; skipping non-dirty items needs a real dirty signal from the view
  layer. Wrong-height regressions (clipping/scroll) are worse than the cost. Revisit with §3's
  model-known heights, which deletes this pass entirely.

### §5 "Explicitly dropped" — landed (this session)

- **Share-block permalink: removed.** The context-menu "Share..." item, the cmd-shift-S
  binding (`CustomAction::CreateBlockPermalink`), the app-menu entry, `ShareBlockModal`
  (1,566 lines, deleted), the pane-group modal plumbing, and the three telemetry variants
  that could only fire from it. This was the last UI path that uploaded terminal content
  (command + output) to Warp's servers — the README/FAQ claim "never sends your prompts or
  terminal data to a central service" is now true rather than aspirational. The GraphQL
  client (`server_api/block.rs`, `server/block.rs`) stays as dead code: the hidden
  Shared-blocks settings page still constructs against it (constructed-but-unnavigable
  pattern), and dead code costs disk, not RAM.
- **Voice input: removed from shipped builds.** `gui = []` in `app/Cargo.toml` (was
  `["voice_input"]`) — dictation recorded audio and POSTed it to
  `{server_root_url}/ai/transcribe` / Wispr. Every render site already had a
  `#[cfg(not(feature = "voice_input"))]` arm (headless builds prove the config), so this
  compiles the mic out of both footers and the settings section; the toolbar-configurator
  lists needed four small `cfg!` gates so "Voice Input" is no longer offered as an addable
  item that renders nothing. The feature still exists behind an explicit flag if a *local*
  transcriber ever backs it.
- **Warp Drive: kept, deliberately.** It holds local workflows/notebooks (real CLI sugar,
  "features we like"), its cloud sync is already gone, and its dead cloud code is
  disk-weight only. Revisit only if the user asks for it to go.

### §1.6 — ESC/Ctrl-C stop being mode exits (this session)

"Modes, not places" is now enforced at every exit path:

- **ESC never exits the agent view.** The 9-branch exit ladder in the view's Escape
  handler is gone. ESC keeps only its genuine navigations: child agent → parent
  (same affordance as the header's "for Orchestrator" button) and popping a nested
  cloud-mode pane — a real pane pushed onto the nav stack — back to its parent
  terminal. Tag-out of a long-running command survives, minus its follow-on exit.
- **Ctrl-C never exits the agent view.** A press that clears the prompt buffer is
  still swallowed; nothing arms an exit.
- **The double-press exit confirmation is deleted** (`PendingConfirmation::Exit`,
  `ExitConfirmationTrigger`, `exit_agent_view_with_required_confirmation`,
  `exit_confirmation_message`, the `ExitConfirmed` event). Exiting = mode switch:
  instant, and it never stops the conversation (which keeps running in the history
  model — verified: no `ExitConfirmed` handler ever cancelled anything; they were
  all `=> {}`). The *enter*/new-conversation confirmation (Cmd-Enter over a
  non-empty conversation) is intentionally kept.
- **The "for terminal" back-button persona is gone** — the header button renders
  only for nav-stack pushes (child agents see "for Orchestrator"). The zero-state
  "go back to terminal" row and the shortcuts-panel row are deleted.
- Tests updated to guard the new invariant (`ctrl_c_never_exits_agent_view` etc.);
  the nested-cloud ESC-pop test passes unchanged.

§1.7 (fullscreen stops being automatic; display modes → explicit view scope;
`AgentViewEntryBlock` deletion; pill bar out of fullscreen-only) is the next slice —
bigger redesign, not started here.

### Adversarial review + runtime verification of the split slices (this session)

A 15-agent review (4 lenses, every finding adversarially verified) over the three
slices produced 11 raw findings, 8 confirmed, all fixed:

- **Child-agent "for Orchestrator" button had regressed** (high): child panes are
  pane-tree *swaps* (`Event::SwapPaneToConversation` → `replace_pane`), not
  nav-stack pushes, so the depth>1 gate missed them. Fixed with an explicit
  `is_child_agent` predicate (parent conversation exists).
- **Autosuggestion dedup demoted same-dir commands** (medium, found by two lenses):
  a global seen-set classified each command by its newest occurrence anywhere.
  Dedup is now per bucket; two regression tests guard the cross-directory case.
- **`update_session_sharing_enablement` defaulted the share flag ON for teamless
  users** — every user in this fork; one reachable server event would have
  resurrected the session-upload UI at runtime. Now a documented no-op.
- **`shared_block_title_generation` out of default features** (its only consumer
  was the deleted share modal); the greyed "Share session…" context-menu item is
  hidden behind `CreatingSharedSessions` (was rendering permanently disabled).
- **`voice_input` kept compilable by a CI check** (no shipped build enables it now).
- Ctrl-C tests now also assert the fall-through (ETX reaches the PTY with a
  long-running block); dead `has_parent_terminal`/`exit`-handle plumbing removed.

**Runtime verification (debug .app, driven live):** launch OK; toggle switches
modes with agent tooling (slash/@/attach/model chip) only in AI mode; **ESC ×2 and
Ctrl-C ×2 stay in AI mode**; context menu has no "Share…"; **no mic anywhere**;
"/" renders the unified command menu (no `/create-environment` — NEVER works);
zero state has no "escape to go back" row.

**Found only by running the debug build:** `Availability::NEVER` overloaded
`AGENT_VIEW|TERMINAL_VIEW`, which `Registry::new`'s debug_assert rejects — every
debug-assertions build panicked at startup. NEVER is now its own bit. Lesson
recorded: run a debug (assertions-on) build before calling a slice verified;
release builds skip the checks that would have caught this.

Also: `incremental = false` in `.cargo/config.toml` — per-feature-combo
incremental caches (16-24 GB each) filled the dev disk three times tonight.

### §1.7 first step + §2 item 3 (this session, after the review round)

- **Pill bar / breadcrumbs ungated from fullscreen** — both gates
  (`pane_impl.rs` secondary row, `render_orchestration_breadcrumbs`) now require
  only an *active* agent view. `pill_specs` never depended on fullscreen (verified;
  PLAN risk #6 closed), and it still returns None → Empty when the conversation has
  no orchestration children. This is the §1.7 prerequisite: multi-agent navigation
  no longer depends on a display mode.
- **The permanently-empty "What's new in Uncaged" zero-state section is gone**
  (§2 item 3): `get_current_changelog` returns `Ok(None)` for Channel::Oss before
  any network, so `oz_updates` could never be non-empty in a shipped build. Deleted:
  the render fn + props + gating helper (~230 lines), the ChangelogModel
  subscription, the expand state, `ToggleOzUpdates`, its four tests, AND the live
  settings toggle ("Show Uncaged changelog in new conversation view") that
  controlled the invisible section, with its action/binding/context-flag plumbing.
  The two TOML settings definitions stay (inert; removing settings fields touches
  serde surface for no user-visible gain).

### §1.7 core — fullscreen stops being automatic (landed)

Mapped by a 5-agent workflow (145 consumer sites, 4 clusters + architect synthesis),
then implemented as designed:

- **`AgentViewDisplayMode::Chronological`** is the new default: `try_enter_agent_view`
  derives the mode from origin. Dedicated panes — `ChildAgent`, `CloudAgent`,
  `ThirdPartyCloudAgent`, `Cli` — keep FullScreen (hiding non-conversation blocks is
  their presentation contract). `Inline` stays LRC-tag-in only; reusing it was rejected
  because ~14 `is_inline()` consumers carry tag-in semantics (header, locked input,
  tag-out, the inline keymap set).
- **Nothing is hidden outside fullscreen**: `should_hide_block` shows Agent-origin
  blocks in Chronological/Inline/Inactive; conversation rich content is visible in
  Chronological and Inactive (Inline keeps hiding it so tag-in monitoring is not
  duplicated); the viewport iterator mirrors the predicate. The LRC entry guard only
  applies to fullscreen-deriving origins — chronological entry is never blocked.
- **`AgentViewEntryBlock` deleted** (the "you had a conversation here" card — turns
  now stay visible after exit, so the card would render history twice), along with its
  context-menu variant, metadata/type variants, the LRC-finish and restore insertion
  paths, and the "continue conversation" message-bar producer that could only trigger
  off the card. `render_block_container` survives in agent_view/mod.rs (ambient entry
  uses it).
- **Input semantics follow the conversation, not the display mode**: new
  `is_conversational()` (fullscreen ∨ chronological) applied at 17 gates — `&` handoff
  prefix, `!` shell-mode lock/exit/indicator, autodetection enable/re-enable/unlock,
  keymap context flags (chronological gets the regular ACTIVE_AGENT_VIEW binding set),
  auto-attach block context, pane-restore conversation capture, cloud-pane handoff
  exits. The first mapping pass missed ten of these; the cloud-handoff test family
  caught them — trust tests over maps.
- **Cmd-K in a chronological conversation** takes the normal clear path: the screen
  clears and the on-screen conversation closes but survives in history (empty ones are
  dropped as always). §3's watermark design makes this fully non-destructive later.
- **Verified zero regressions by name-level diff** against the recorded 115-failure
  baseline (full suite, stash-compared). New invariant tests: default entry is
  chronological and changes no block's visibility; the four origins derive fullscreen;
  mid-conversation commands stay terminal-visibility; LRC never blocks entry.

Still open from §1.7: the explicit "scope" pill (filtering as an opt-in view, backed
by the surviving FullScreen machinery) and §2's zero-state merge for the empty-
conversation affordance.

### Full-suite baseline (measured 2026-08-22, both sides of the split slices)

`cargo test -p warp --lib` fails **115 tests at 4e1fc87 and 115 at a39c838** —
identical set, all pre-existing, none introduced by the split slices. They are
upstream tests broken by deliberate fork changes: auth flows (accounts disabled),
drive section counts (cloud sections removed), the URL rebrand in notebook link
tests, a missing notebook fixture, and a family of `CLIAgentInstallModel`
singleton-registration setup panics. **Do not read a failing full suite on this
branch as a regression signal until the triage task lands** — compare failure
sets against this baseline instead. Targeted filters over changed areas are the
regression check that works today.

## Survey claims corrected by measurement — do not re-chase these

- "34 tree-sitter grammars, 44–51 MB": the lockfile has **one** tree-sitter package. Wrong.
- "42 MB onboarding PNGs in the binary": `Contents/Resources` is 3.3 MB; the PNGs are not embedded. Wrong.
- "`get_all_restored_blocks` loads everything": true in code, irrelevant in practice — this DB is 79 rows / 0.2 MB.
- "29.3 MB command-signature JSON in the binary": `completions_v2` is not in `default`; repo weight only.
- `ps` RSS overstates real memory by ~2×: use `vmmap` physical footprint (fair comparison: Terminal.app 32.5 MB).

## Where the remaining memory actually is (malloc_history + vmmap, measured)

1. **Font-file mmaps, ~1.17 GB mapped** from startup font enumeration — fixed by deferral; verify next launch shows no "Computing available system fonts".
2. Heap ~99.6 MB (Terminal.app: 7.8 MB) — next attribution target after the font fix lands.
3. GPU surfaces 43.8 MB — the price of GPU-rendered blocks; architectural, not waste.
4. Binary resident 72.5 MB of 274 MB mapped — shrinking the binary pays ~25 ¢/$; deprioritised.
5. Thread stacks: 28 tokio blocking + 26 rayon ≈ 55 MB mapped — why a terminal needs a full rayon pool at idle is unexplained; candidate.

## Queue, in order

1. Verify the consolidated build: slash menu renders in terminal mode; no font-enumeration log line at launch; re-measure footprint/wakeups; zero state has no NLD checkbox.
2. Full test-suite pass over the same filters (255 passed at last run) + push.
3. Delete `render_cloud_mode_v2_*` + `cloud_mode_v2_history_menu.rs` (~580 lines, dead behind two commented-out features; guard fn stays).
4. Zero-state unification (PLAN §2): 1,669 lines across two files → one.
5. Rayon/tokio pool sizing at idle.
6. Attended (needs a human or daylight): gating `drive`/`cloud_object` (42.5 K lines, 13.5 K symbols, 500+ refs); folding `agent.rs` into the unified surface (needs the §3 history work).

---

# PLAN.md — Uncaged: one surface, one zero state, one history

**Status:** proposal for approval. Nothing here is implemented.
**Ground rules used throughout:** shipped Oss = `default` features from `app/Cargo.toml:506-705` **plus** `local_fs`/`local_tty`/`iterm_images` injected by `app/build.rs:225-233` **plus** `gui,nld_classifier_v3,nld_heuristic_v2` appended by `script/macos/bundle:358` / `script/linux/bundle:227`. `FeatureFlag::AgentView` is on in every shipped build (`app/Cargo.toml:625` → `app/src/features.rs:332-333`), so every `if AgentView` branch is live and every `else` is dead. Unit tests run `AgentView = false` and are **not** evidence about shipped behaviour — every verification step below is a running-build check.

---

## 0. The shape of the problem, in one paragraph

There is already **one list**: `BlockList.block_heights: SumTree<BlockHeightItem>` (`app/src/terminal/model/blocks.rs:227`) interleaves shell blocks and agent turns chronologically. "Fullscreen agent view" is not a second screen — it is that same list with a per-item predicate, `Block::should_hide_block` (`app/src/terminal/model/block.rs:1376-1428`), returning zero height for anything outside the active conversation. So the "10 screen types" the user is complaining about are **one list wearing N filters, N input renderers, N zero states and N footers**. The work is not to build a unified surface. It is to delete the duplicates that were bolted onto a surface that was already unified, and to remove the mode-as-place machinery (`ESC to go back`) that made them feel like places.

---

## 1. ONE SURFACE

### 1.1 What survives

**`render_universal_developer_input`** (`app/src/terminal/input/universal.rs:28-227`) becomes the body of `Input::render` (`app/src/terminal/input.rs:16330-16376`). It already carries the mode toggle, attachment chips, the agent tooling, and it already hides the tooling in Terminal mode (`app/src/terminal/universal_developer_input.rs:864-870`) — i.e. it already does *"contents change with the mode"* without a second layout. The commit comment at `input.rs:16351-16366` says as much.

Target end state of `Input::render`:

```rust
fn render(&self, app: &AppContext) -> Box<dyn Element> {
    self.render_input_surface(app)   // one function, mode-parameterised
}
```

### 1.2 What gets deleted outright (no behaviour to fold in)

| Renderer | Location | Why dead |
|---|---|---|
| `render_terminal_input` | `app/src/terminal/input/terminal.rs:27-289` (301 lines) | **Provably unreachable for every setting combination.** Verified: `is_classic_input_enabled` ⟹ `honor_ps1` (`app/src/settings/input.rs:214-220`), so `should_render_ps1_prompt` (`app/src/terminal/prompt_render_helper.rs:59-72`) reduces to exactly `is_classic_input_enabled`. The guard `!should_render_ps1_prompt` at `input.rs:16349` is therefore identical to `if is_universal_input` at `input.rs:16367`, and the `else` at `16369` can never be taken. |
| `render_ambient_agent_status_footer` | `app/src/terminal/input/agent.rs:678-717` | `FeatureFlag::CloudMode` — commented out at `app/Cargo.toml:617`. |
| `render_cloud_mode_v2_composing_input` + `_content` / `_history_menu` / `_top_row` / `_input_container` | `agent.rs:345-676` (~330 lines) | `CloudModeInputV2` — commented out at `app/Cargo.toml:691`. |
| `cloud_mode_v2_history_menu.rs` | whole file, 252 lines | same |
| `render_cloud_mode_v2_footer`, `should_render_cloud_mode_v2` | `agent_input_footer/mod.rs:931-1000` | same |
| the `!FeatureFlag::AgentView` arms | `input.rs:16371-16374` | AgentView always on |

Also delete the now-dead callers of `is_cloud_mode_input_v2_composing`: `view.rs:20148, 21454, 27523`, `workspace/view.rs:25597`, and the 9 `CLOUD_MODE_V2_*` constants at `agent.rs:34-58`.

Deleting `terminal.rs` also removes the duplicate `styles::default_border_color` (`terminal.rs:290-299`, second copy at `agent.rs:721`) and the semantic drift between focus predicates — `terminal.rs:96-99` reads `focus_handle` directly and treats a missing handle as focused, while `universal.rs:116-137` uses `is_pane_focused`/`is_active_session` with opposite defaults (`input.rs:6190-6199`). That drift is the evidence that keeping four renderers in sync was not happening.

**Est. size: −1,100 lines, zero user-visible change.** This is the single highest-value, lowest-risk deletion in the plan and should ship first.

### 1.3 What must be folded in before deleting

**`render_agent_input`** (`app/src/terminal/input/agent.rs:77-337`) contributes, and each must land in `universal.rs` first:
1. `agent_input_footer` in place of the button bar (see §1.4).
2. The **11-way inline menu selector** (`agent.rs:243-284`) — the superset. Classic has 7 (`classic.rs:290-395`, repeated 4× for the three input modes), terminal 6, cli_agent 3 (`cli_agent.rs:135-142`).
3. `render_agent_shortcuts_view` (`agent.rs:286-336` → `agent_view/shortcuts/mod.rs:108-240`).
4. `agent_status_view` — note `universal.rs:215-222` and `agent.rs` render it in **different column positions**; pick one (recommend universal's).
5. Queued-prompts panel, buy-credits banner.
6. Border colour by state (`agent.rs:205-219`). The magenta handoff border is dead (`handoff_compose_state` needs `OzHandoff`+`HandoffLocalCloud`, both commented out at `app/Cargo.toml:685-687`) — keep only the AI/shell distinction.
7. Inline background fill when agent view is inline.

**`render_classic_input`** (`app/src/terminal/input/classic.rs:32-395`) contributes exactly two things nobody else draws:
- Real PS1 lprompt/rprompt placement via `rprompt_render_offset` (`classic.rs:76-90`).
- The warpify subshell flag and flag-pole (`classic.rs:157-190`).

**Recommendation: keep PS1, delete the renderer.** PS1 becomes a *prompt-row variant* inside the unified surface — `universal.rs` already calls `render_universal_developer_input_prompt`. Do **not** delete PS1 support: it is genuinely user-reachable via three live paths — the Appearance radio (`app/src/settings_view/appearance_page.rs:3489-3501`), the un-gated command-palette entry "Toggle Input Mode" (`appearance_page.rs:244-252`), and `terminal.input.honor_ps1` in settings.toml (`app/src/terminal/session_settings.rs:297-305`). Deleting it is a behaviour change, not dead-code removal.

Once PS1 is a prompt mode, `InputBoxType`, `input_type()` (`app/src/settings/input.rs:188-221`), `is_classic_input_enabled`, `is_universal_developer_input_enabled` (11 call sites) and the two settings-sync hooks (`app/src/settings/init.rs:191-208`, `app/src/settings/initializer.rs:79-84`) all collapse — `honor_ps1` alone drives the prompt row.

**`render_cli_agent_input`** (`app/src/terminal/input/cli_agent.rs:28-146`) contributes three things and re-implements the whole scaffold to get them: its own editor max-height/padding constants, the alt-screen-matched background sampled from the CLI agent's inferred bg colour (`cli_agent.rs:104-113`), and the CLI-mode footer. Fold as a **mode** (`SurfaceMode::CliAgent`), not a renderer. Its editor-config helpers `update_cli_agent_editor_text_colors` (`cli_agent.rs:155`) and `update_cli_agent_enter_settings` (`cli_agent.rs:203`) are orthogonal to layout and stay as-is.

### 1.4 The two footers → one

`UniversalDeveloperInputButtonBar` (`app/src/terminal/universal_developer_input.rs:299-935`, render at `:828`) and `AgentInputFooter::render` (`agent_input_footer/mod.rs:2254-2340`, 3,019-line file) are two independent implementations of the same row — both own a mic button, file button, model selector and prompt alert.

**Recommendation: keep `AgentInputFooter`'s toolbar engine, delete `UniversalDeveloperInputButtonBar`.** Reason: the configurable toolbar is a shipped, persisted, user-facing feature (`agent_toolbar_editor` and `configurable_toolbar` are both in `default`; selection persisted in `session_settings`), and the button bar's fixed layout is a strict subset of what it can express. Losing configurability is a regression; losing a hardcoded layout is not.

Required changes to the toolbar model (`agent_input_footer/toolbar_item.rs:48-95`, verified):
- Add `AgentToolbarItemKind::ModeToggle` — the segmented Terminal/Agent control — pinned left and non-removable. **This is the affordance that replaces ESC.**
- Replace `ToolbarAvailability::{Both, AgentViewOnly, CLIAgentOnly}` (`toolbar_item.rs:78-90`) with a **mode** filter `{Always, TerminalMode, AgentMode, CliAgentMode}`. That enum currently encodes exactly the screen split we are erasing.
- Delete `ShareSession` (needs `CreatingSharedSessions` + `HOARemoteControl`, both off — `mod.rs:1475-1480`) and `HandoffToCloud` (needs `OzHandoff` + `HandoffLocalCloud`, both off — `app/src/settings/ai.rs:1873-1890`).
- Fix the stale doc comment at `universal_developer_input.rs:627-633`, which claims this bar is unreachable. As of HEAD the opposite is true.

### 1.5 Inline menus → one helper

Replace the four divergent copies with a single `render_active_inline_menu(&self, app)`. **This fixes a real shipped bug**: `/rewind` and the plan menu have no surface to render on outside agent view, because only `agent.rs:243-284` offers them. Est. −200 lines and every menu becomes available in every mode.

### 1.6 ESC-to-terminal and the entry/exit machinery — the user's actual complaint

ESC is not a keybinding. It is the bottom rung of a 9-branch ladder in `Input::editor_escape` (`app/src/terminal/input.rs:9079-9160`, emits `Event::Escape` at `9159`), handled at `app/src/terminal/view.rs:21362-21434`, which — verified in source — tries parent-conversation navigation, then `can_exit_agent_view`, then, if the input buffer is non-empty, requires a **second ESC within 1 second** (`ENTER_OR_EXIT_CONFIRMATION_WINDOW`, `app/src/ai/blocklist/agent_view/controller.rs:65`).

**Recommendation: Option A — ESC stops being an exit at all.**
Rejected alternative (Option B: keep exit, drop the confirmation) — it keeps the two-screen mental model alive, which is precisely what the user asked to remove.

Delete:
- The whole agent-view arm of the Escape handler, `view.rs:21367-21411`.
- `PendingConfirmation::Exit`, `ExitConfirmationTrigger`, `exit_agent_view_with_required_confirmation` (`controller.rs:71-92, 494-593, 860, 1025-1063`), `ExitAgentViewError`, and the `EphemeralMessageModel` usage for exit messages.
- Ctrl-C exit (`view.rs:8585-8592`, `should_ctrl_c_exit_agent_view` at `view.rs:8767`).
- The three affordances that advertise it: the header back button labelled "for terminal" with a hardcoded `escape` keystroke (`view.rs:4212-4231`), the zero-state row (`agent_view/zero_state_block.rs:767-785`), the shortcuts-panel row (`agent_view/shortcuts/mod.rs:238-249`).

Keep in `editor_escape`: the dismiss rungs (vim escape, AI context menu, slash menu, inline menu, input suggestions, workflow, clear attached context). ESC dismisses things. It does not navigate.

**What replaces "exit":** the mode toggle in the footer, and the fact that there is nowhere to exit *from*. Which requires:

### 1.7 Fullscreen stops being automatic

`AgentViewState::Active { conversation_id, origin, display_mode, original_conversation_length }` (`controller.rs:244`) mirrored onto the block list (`controller.rs:838, 954`) is what makes the terminal disappear. `AgentViewEntryOrigin` has 40 variants (`controller.rs:102-204`), three already documented as dead (`:196-204`) and several cloud-only.

**Recommendation:**
- `AgentViewDisplayMode::FullScreen` stops being entered automatically. Starting an agent conversation focuses the input in Agent mode and appends the turn to the one list — no filter, no chrome swap.
- Keep conversation filtering as an **explicit, reversible view scope** — a pill in the breadcrumb row that the user clicks into and clicks out of ("All"). Never entered by a keystroke, never with a confirmation window. `Block::should_hide_block` (`block.rs:1376`) keeps its machinery but its default is "show everything".
- `AgentViewDisplayMode::Inline` and `InlineAgentViewHeader` (`agent_view/inline_agent_view_header.rs:1-175`, inserted at `view.rs:3149-3168`) collapse into that same scope concept — inline is just "not scoped".
- `AgentViewEntryBlock` (`agent_view/agent_view_block.rs:1-527`) — the "you had a conversation here" card — has no job once the conversation is inline. Delete it and its `cached_title` workaround.
- Fix the six fullscreen-only chrome sites: `view.rs:27477-27481` (forces `InputMode::PinnedToBottom`, silently overriding the user's Waterfall/PinnedToTop preference — a concrete symptom of "the screen decides, not you"), `view.rs:27955-27962`, `28104-28112`, `28289`, `pane_impl.rs:242-267, 378-390, 500-535`.

**Must not regress:** `OrchestrationPillBar` (`agent_view/orchestration_pill_bar.rs`, 2,537 lines, rendered at `pane_impl.rs:519-534`) currently only renders in fullscreen. It is real multi-agent navigation. It moves to the scope/breadcrumb row and renders whenever the conversation has orchestration children (it already short-circuits to Empty otherwise, `:1057`).

---

## 2. ONE ZERO STATE

### 2.1 The variants that exist

Two blocks, ~15 rendered variations, inside a family of ~11 more surfaces repeating the same content in different chrome:

| # | Surface | Location | Verdict |
|---|---|---|---|
| 1 | `TerminalViewZeroStateBlock` | `app/src/terminal/view/zero_state_block.rs:53-395` | **survivor skeleton** |
| 2 | `AgentViewZeroStateBlock` | `agent_view/zero_state_block.rs:72-484` (1,274 lines) | merge in |
| 3 | Oz "What's new in Uncaged" section | `agent_view/zero_state_block.rs:978-1206` | **delete** — `ChangelogModel.oz_updates` is permanently empty: `get_current_changelog` returns `Ok(None)` for `Channel::Oss` (`app/src/autoupdate/changelog.rs:34-36`) and `ChannelState::init()` hardcodes Oss (`crates/warp_core/src/channel/state.rs:39`). ~230 lines. |
| 4 | Cloud header + `CloudModeWithDocsLink` description | `zero_state_block.rs:400-406, 558-570, 631-673` | delete (`CloudMode` off) |
| 5 | Cloud empty-body + no-bottom-border branches | `:716-718, :468` | delete |
| 6 | `render_ambient_credits_banner` | `:1208-1256` | delete (needs server credit data) |
| 7 | Escape hint row | `:767-785` | delete (§1.6) |
| 8 | NLD checkbox | `terminal/view/zero_state_block.rs:257-275, 354-388` | **delete** — the gate `should_render_nld_checkbox` (`:126`) is a tautology, and the classifier it enables is off by default (`app/src/settings/ai.rs:836`) and is a per-keystroke BERT-tiny pass. The setting stays reachable from the AI settings page; no capability lost. |
| 9 | Terminal hint list (2–3 items) + pinned-to-top reversal | `:183-246` | merge — express as data |
| 10 | Agent hint list (3 items) | `agent_view/zero_state_block.rs:729-788` | merge items 1-2, delete item 3 |
| 11 | Recent-activity section | `:831-976` | **keep, but make additive** |
| 12 | `/init` callout chip | `:790-826` | keep, merge chrome |
| 13 | Terminal input message bar | `input/terminal_message_bar.rs:31-465` | merge into shared hint vocabulary |
| 14 | Agent message bar `ZeroStateMessageProducer` | `agent_view/agent_message_bar.rs:349-363, 504+` | merge |
| 15 | Agent shortcuts panel | `agent_view/shortcuts/mod.rs:107-266` | merge — it is the **superset** |
| 16 | Rotating `AgentTip` | `app/src/ai/agent_tips.rs:87+`, rendered `blocklist/block/status_bar.rs:705-743` | merge or delete |
| 17 | Onboarding callout `UniversalInput` flow | `crates/onboarding/src/callout/view.rs:54-96` | **delete** — unreachable in release (selected via `else` on `AgentView`, `workspace/view/onboarding.rs:200-207`; debug bindings need `enable_debug_features()`, false on Oss) |
| 18 | `AIAssistantPanelView::render_zero_state` | `app/src/ai_assistant/panel.rs:890-1008` | **delete the whole legacy panel** — gated `!FeatureFlag::AgentMode`, and `agent_mode` is default-on |
| 19 | `GetStartedView` "Welcome to Uncaged" pane | `app/src/pane_group/pane/get_started_view.rs:50-389` | **delete** — auto-trigger requires `!is_anonymous_or_logged_out` (`workspace/view.rs:7897`), impossible on account-free Uncaged; only a debug binding remains |
| 20 | HOA welcome banner | `workspace/hoa_onboarding/welcome_banner.rs` | keep — it is the real first-run welcome |
| 21 | Input placeholder "zero state hint text" | `app/src/terminal/input.rs:6787-6879` | merge live branches, delete the two cloud branches and the `!AgentMode` else |

Plus the dead enum variants: `RichContentMetadata::AIOnboardingBlock` (matched at `view.rs:2287`, constructed nowhere), `AgentViewEntryOrigin::AgentModeHomepage` (doc comment literally says "(tab zero state)"), `ZeroStatePromptSuggestionTriggeredFrom::AgentModeHomepage`. Residue of a previous unification pass left half-finished.

And two settings that provably do nothing: `should_show_oz_updates_in_zero_state` + `should_expand_oz_updates` (`app/src/settings/ai.rs:1356-1373`), their **visible, clickable, keybindable** toggle "Show Uncaged changelog in new conversation view" (`app/src/settings_view/ai_page.rs:7599-7608`), the action variant (`ai_page.rs:4352-4359`) and the keymap flag (`workspace/view.rs:23302-23306`). A settings switch that can never change anything is worse than no switch.

### 2.2 The survivor

One `ZeroStateBlock` view, driven by a declarative hint registry:

```rust
struct HintSpec { keystroke: KeystrokeSource, text: &'static str, action: Action, modes: ModeSet }
```

It renders:
1. **Header** — one shared `render_zero_state_header(icon, title, description)`. Today both files independently define `CONTAINER_VERTICAL_PADDING = 16.`, `TITLE_MARGIN_BOTTOM = 8.`, `title_font_size = monospace + 6.`, an icon-in-`ConstrainedBox` + 8px + bold `Text` row, and the same `Container` + 1px top/bottom `Border` (`terminal/view/zero_state_block.rs:145-172, 297-318, 391-395` vs `agent_view/zero_state_block.rs:591-620, 467-478, 1258-1270`).
2. **2–4 hints**, filtered by mode, drawn from the shared registry — the same registry the shortcuts panel, the message bar producers and the input placeholder read from. Today item 1 ("start a new agent conversation") is byte-identical across both files.
3. **Recent activity when present — additive, not exclusive.** Today it is an either/or (`agent_view/zero_state_block.rs:718` vs the `else` at `:729`): a user who has worked in this directory *never* sees the keyboard hints, and a new user never sees recent activity. That alone is two of the "10 variations".
4. **`/init` callout when applicable** — same row chrome as a hint, not a fourth tinted-chip style. Merge with `AgentModeSetupSpeedbumpBanner` (`terminal/view/inline_banner/agent_mode_setup.rs:15-17`) and `codebase_index_speedbump_banner.rs` — **three UIs asking for one action**.
5. **One dismiss link** — shared component. Today there are two independent "Don't show again" links with two settings and two hover implementations (`terminal/view/zero_state_block.rs:284` and `terminal/view/use_agent_footer/mod.rs:1109`).

Also fix `AgentViewZeroStateBlock::should_hide` (`:279-306`), where a field and a method share a name and are compared to each other (`me.should_hide != me.should_hide(ctx)`); it takes a `FairMutex` on the terminal model inside an event handler on **every completed user block**.

**Est. size: ~1,670 lines across the two files → ~350.** Plus the deletions above.

---

## 3. ONE HISTORY

### 3.1 The honest situation

Rendering is **already unified**. Persistence and identity are not. The same conversation lives in four stores with no shared ordering key and no shared identity:

- `blocks` (`crates/persistence/src/schema.rs:79`) — per-pane, autoincrement PK, 100 rows/pane cap (`app/src/persistence/block_list.rs:19`), **deleted from disk on Cmd-K**.
- `agent_conversations` + `agent_tasks` (`schema.rs:11, :19`) — global, UUID-keyed, tasks stored as raw `api::Task` protobuf, 200-conversation cap, **never deleted on Cmd-K**.
- `ai_queries` (`schema.rs:48`) — 10,000-row FIFO keyed by an exchange UUID that ceases to exist after the next restart.
- `commands` (`schema.rs:142`) — unbounded shell history, disjoint from `blocks`.

Five blockers, all verified:

- **B1 — exchange IDs are not stable.** `create_exchange_from_messages` calls `AIAgentExchangeId::new()` on every restore (`app/src/ai/agent/api/convert_conversation.rs:1816`). The agent half of history has **no durable per-turn identity**. Consequence: `hidden_exchanges` (`conversation.rs:262`) can never be persisted, and every `ai_queries.exchange_id` dangles after the first restart.
- **B2 — block IDs are not unique and may be empty.** `block_id TEXT NOT NULL DEFAULT ""` added by `2024-01-19-010001` with **no unique index** (verified: the only `CREATE INDEX` statements in all 137 migrations are on `mcp_server_installations`, `agent_conversations`, `agent_tasks`, `project_rules`). `update_block_agent_view_visibility` filters `block_id.eq(target)` (`block_list.rs:302`) — for a legacy `""` row that is a mass update.
- **B3 — the same shell command is materialised twice and which copy you get is accidental.** `terminal_view_restored_blocks` (`terminal_view_adaptor.rs:98`) prefers `blocks` rows and `.or_else()`s to `conversation.to_serialized_blocklist_items()` (`conversation.rs:3728`), which mints a **fresh `BlockId::new()`** per command. After Cmd-K or a 100-block eviction the same command reappears with a different identity, no git info and no prompt snapshot. Forking re-persists those synthesised blocks as new rows (`load_ai_conversation.rs:822-845`) — a third copy.
- **B4 — ordering is a timestamp heuristic re-run every startup.** `find_block_indices_for_exchange_timestamps` (`load_ai_conversation.rs:1229-1265`) scans command blocks backwards with a documented early-break that assumes the tail of the blocklist is a sorted conversation group. `get_all_restored_blocks` separately re-sorts by `start_ts` where `None` sorts *first* (`block_list.rs:194`). **Nothing on disk records that block X came before exchange Y.**
- **B5 — "clear" means two things.** `clear_buffer` (`view.rs:18690`) SQL-DELETEs every `blocks` row for the pane (`terminal_pane.rs:981-987`) but only moves conversation IDs between two in-memory HashMaps (`history_model.rs:1923-1959`).

Plus **B6** non-deterministic conversation→pane ownership (`history_model.rs:912` linear-scans a HashMap and returns an arbitrary match) and **B7** the perf consequence in §4.

### 3.2 Recommendation: rendering change over two stores, plus one additive migration

**Do not build a single `turns` table that absorbs `agent_tasks`.** That means re-encoding a foreign protobuf schema (`api::Task`, owned by `warp_multi_agent_api`) into a local format, forward-only and irreversible (`run_pending_migrations`, `sqlite.rs:396`; `down.sql` files exist but are never run), for **zero user-visible gain**. The content stores are fine. What is missing is *identity* and *order*.

So: keep both content stores, add one thin index that records the timeline, and delete every code path that re-derives it.

**Migration (additive only — `blocks` uses positional `Queryable`, `crates/persistence/src/model.rs:740-771`, so new columns go at the end and nothing is reordered):**

1. `UPDATE blocks SET block_id = 'legacy-' || id WHERE block_id = '';` then `CREATE UNIQUE INDEX ux_blocks_block_id ON blocks(block_id);` — fixes B2 deterministically.
2. `CREATE TABLE turn_index (turn_id TEXT PRIMARY KEY, pane_leaf_uuid BLOB, seq INTEGER NOT NULL, kind TEXT NOT NULL, block_id TEXT, conversation_id TEXT, exchange_ord INTEGER, created_ts TIMESTAMP);` plus `CREATE INDEX idx_turn_index_pane_seq ON turn_index(pane_leaf_uuid, seq);` — **one row per turn, shell or agent, never duplicating content.** This is the shared timeline. Fixes B4 and B6.
3. `ALTER TABLE blocks ADD COLUMN turn_id TEXT;` (appended last).
4. `ALTER TABLE terminal_panes ADD COLUMN cleared_before_seq INTEGER;` — the Cmd-K watermark (see below). Fixes B5.
5. Free perf indexes while we're here: `CREATE INDEX idx_blocks_pane ON blocks(pane_leaf_uuid);`, `CREATE INDEX idx_commands_lookup ON commands(command, pwd);`, `CREATE INDEX idx_commands_session ON commands(session_id, id);`.

**Durable exchange IDs without a new store (fixes B1):** stop calling `AIAgentExchangeId::new()` at `convert_conversation.rs:1816` and derive the ID deterministically from the exchange's first `MessageId`/`request_id` (the same value `into_exchanges` already uses to cut exchange boundaries at `:346-372`). Same ID on every restore, nothing extra persisted, proto stays the source of truth. This is safe here specifically because on Uncaged the "server" doing the batching is the local engine — we control it. Once IDs are stable, `hidden_exchanges` becomes persistable and `ai_queries.exchange_id` stops dangling (at which point consider the standing TODO at `persistence.rs:38-39` to delete `ai_queries` and read prompts from tasks).

**Canonical copy of agent-executed commands (fixes B3): the `blocks` row wins.** Delete `to_serialized_blocklist_items` (`conversation.rs:3728`), the `.or_else()` fork (`terminal_view_adaptor.rs:98`), the live-blocklist injection (`load_ai_conversation.rs:554-580`), and `persist_blocks_for_forked_conversation` (`:822`). Forking copies `turn_index` rows and block rows explicitly, once.

**Cmd-K (fixes B5): make it non-destructive on both sides.** `clear_buffer` sets `terminal_panes.cleared_before_seq` and stops issuing `DELETE FROM blocks`. Both halves then behave identically and nothing is lost. Rejected alternative: make Cmd-K delete conversations too — that is data loss and users do not expect Cmd-K to destroy an agent transcript.

**What happens to existing sessions and stored conversations at upgrade:**
- Backfill runs **once**, at migration time. For blocks carrying `ai_metadata` (`SerializedAIMetadata` → `conversation_id`, `serialized_block.rs:94-115`) or non-NULL `agent_view_visibility`, the conversation link is read directly.
- For everything else, the backfill uses the same timestamp interleave as `find_block_indices_for_exchange_timestamps` — **but it runs once and writes the answer down**, instead of being recomputed on every startup forever. That is the whole point.
- Two known NULL populations, stated plainly: rows written before `2024-01-19` may have had empty `block_id` (now `legacy-N`); rows written between `2025-12-25` and `2026-01-15` **lost their conversation link entirely** when `2026-01-15-163534-0000` did `ALTER TABLE blocks DROP COLUMN agent_view_conversation_id` (verified) rather than converting it. Those come back as plain terminal turns ordered by `blocks.id`. Acceptable — that is already today's behaviour via `restore_block` mapping NULL to `clear_conversation_id()`.
- `SerializedBlockListItem` (`app/src/ai/blocklist/persistence.rs:458`) is a **one-variant enum** whose own TODO says "now that there is no AI serialized block, consider removing this enum wrapper" — direct evidence of an abandoned unification, killed when `2025-11-19-224140` did `DROP TABLE ai_blocks`. It becomes the real unified item type again, this time backed by `turn_index` rather than by duplicated content.

**Downgrade note:** a user who takes the merged build and reverts to an older one hits a DB with extra columns and one extra table. Diesel tolerates that for `blocks` **only because we append**. Never reorder.

**Good news to build on, not obstacles:** up-arrow history is *already* unified — `HistoryInputSuggestion` (`input_suggestions.rs:1129`) is a clean two-variant union merged and deduped by `up_arrow_suggestions_for_terminal_view` (`history/up_arrow.rs:74-128`). Find is already a union too (`BlockListMatch`, `find/model/block_list.rs:254`). Both are the shape the blocklist item should have.

**Still to unify after the above:** selection. `BlockList.selection` is point-based over grids; `rich_content_selections: Vec<EntityId>` (`blocks.rs:275`) tracks AI blocks that manage their own. The 10-line comment at `:266-274` is an explicit admission that the block list cannot derive selection for half its contents — so **cross-boundary select-and-copy (shell output + agent reply) is currently not expressible**. Fixing that is a follow-on, not a blocker.

---

## 4. PERFORMANCE

Ordered by win. Measurements marked **[M]** were taken this session; the rest are estimates from source structure.

### 4.1 Binary size

Shipped bundle is `lipo`'d universal (`script/macos/bundle:484-486`) — **every byte below counts twice** in the delivered `.app`.

| # | Item | Size | Action |
|---|---|---|---|
| 1 | Onboarding imagery, 55 PNGs | **42.4 MB [M]** (`app/assets/async/png/onboarding` = 41,516 KB) | Collapse the vertical/horizontal pairs (each customize/theme slide ships the same screenshot twice), downscale. Worst single file: `onboarding_bg.png` 1,867,637 B. |
| 2 | Tree-sitter parse tables, 34 grammars | ~44–51 MB **[M: 2,331 `ts_*` symbols]** | Trim the grammar list at `Cargo.toml:299-330`. `cpp` 4.89 + `objc` 4.75 + `kotlin` 4.36 + `c_sharp` 4.19 + `scala` 2.47 + `starlark` 2.42 ≈ **18 MB** for six grammars. Pure Cargo feature edit, no code. |
| 3 | Command-signature JSON, 1,159 files | **29.3 MB [M]** | Gate it. It is currently *target*-cfg'd (`crates/warp_completer/Cargo.toml:44-47` and `app/Cargo.toml:344`), so **no app feature can turn it off** — that is the bug. Lookup is lazy and memoised, so this is pure binary weight. |
| 4 | `bert_tiny_v3.onnx` + tokenizer | **18.3 MB** (17,582,121 + 711,661) | Drop `nld_classifier_v3` from `script/macos/bundle:358`, `script/linux/bundle:227`, `script/windows/bundle.ps1:123`. |
| 5 | Cloud/billing modal imagery | 5.5 MB | Delete with the modals (`launch_modal/oz_launch.rs:115-124`, `free_tier_limit_hit_modal.rs:354`, `build_plan_migration_modal.rs:407`, `cloud_agent_capacity_modal/mod.rs:330`). |
| 6 | Orphan images | **3,611,357 B [M, verified zero code refs]** | Delete `Trial-Image.png` (1,419,656), `code_launch_swe_bench.png` (659,426), `agents_3_*.png` (4 files, 1,532,275). Zero regression risk. |
| 7 | Windows `.pdb` files | **77 MB [M]** | Repo weight only — never copied by `app/build.rs:436-462`, never embedded. Delete. Also makes `cargo:rerun-if-changed=assets/windows` stop stat'ing 126 MB per build. |

**Cargo default features Uncaged does not need** (all currently in `default`, all cloud/account-dependent, all compiled in): `cloud_conversations` (`:611`), `viewing_shared_sessions` (`:508`), `shared_with_me` (`:510`), `session_sharing_acls` (`:511`), `loginless_conversion` (`:522`), `warp_packs` (`:523`), `usage_based_pricing` (`:544`), `api_key_authentication`/`api_key_management` (`:577-578` — provably dead, `app/src/lib.rs:1252-1256` requires `is_dogfood()`), `agent_shared_sessions` (`:587`), `cloud_environments` (`:591`), `ambient_agents_*` (`:598, :599, :615`), `scheduled_ambient_agents` (`:600`), `warp_managed_secrets` (`:601`), `team_api_keys` (`:605`), `oz_platform_skills` (`:621`), `oz_identity_federation` (`:622`), `sync_ambient_plans` (`:623`), `oz_changelog_updates` (`:638` — the flag is on but never gets data), `orchestration_viewer_streamer` / `owner_orchestration_ancestor_streamer` (`:685-686` — these live inside session sharing, **not** local `run_agents`), `remote_codebase_indexing` (`:694`), `billing_and_usage_page_v2` (`:699`), `get_started_tab`.

Scale of the surfaces behind them, non-test lines: `app/src/drive` 20,684; `app/src/terminal/shared_session` 11,594; `app/src/server/cloud_objects` 5,835; `app/src/cloud_object` 4,461; `teams_page.rs` 4,468; `billing_and_usage_page*.rs` 5,867.

Also compiled in and worth a decision against a number: AWS SDK 2.76 MB (three of four clients are sign-in flows), `warp_graphql` + `cynic` 2.30 MB (client for a server pointed at an unroutable sentinel — `app/src/settings/initializer.rs:200-203`), tantivy family 1.48 MB. For contrast: `warp_terminal` 0.265 MB, `warpui` 0.202 MB, `warp_core` 0.283 MB. **The terminal is a rounding error next to the cloud client.**

### 4.2 Startup (main thread, before first frame)

1. **ONNX decode + 30,522-entry tokenizer build.** `app/src/lib.rs:2259` registers `InputClassifierModel::new` with **no cfg, no flag, no setting** (verified), and `add_singleton_model` is eager (`crates/warpui_core/src/core/app.rs:2219` — `let model = build_model(&mut ctx);`). That prost-decodes 17.5 MB of protobuf and builds the vocab synchronously, before window creation, **for a classifier that is off by default** (`app/src/settings/ai.rs:836`; `initializer.rs:203-224` even force-resets pre-existing `true` values). Largest remaining single main-thread startup cost. **Delete it, or make it lazy on first classification.**
2. `get_all_restored_blocks` (`block_list.rs:169-204`) loads **every** row of `blocks` for **every** pane — full `stylized_output` terminal text included — and only then trims to 100 per pane in memory. Move the cap into the query. There is no index on `blocks(pane_leaf_uuid)` (§3.2 fixes that).
3. `read_agent_conversations` (`app/src/persistence/agent.rs:205-249`) prost-decodes **every** persisted task. `read_agent_conversation_by_id` already exists and does it lazily for one — use it.
4. `commands` loaded unbounded (`sqlite.rs:2716-2722`), up to 10,000 rows with 6 strings each. The sibling `read_ai_queries` already does this right with `MAX_AI_QUERIES_TO_READ = 100` and a comment explaining why (`block_list.rs:90-107`) — apply the same fix.
5. 131 eager singletons (`app/src/lib.rs:1198-2290`); several do real work in the constructor. Many are cloud/account singletons inert in Uncaged (`SyncQueue` `:1919`, `ServerExperiments` `:1479`, `OrchestrationEventStreamer` `:1968`, `LocalAgentTaskSyncModel` `:1965`, `RemoteCodebaseIndexModel` `:1601`).

### 4.3 Per keystroke

1. `find_potential_autosuggestions_from_history` (`app/src/ai/predict/next_command_model.rs:806-831`, **read this session**) calls `entry.clone()` on **every** matching entry with no `.take(n)` — `HistoryEntry` has ~8 owned String fields — on the UI thread, from `maybe_generate_autosuggestion` (`input.rs:9401-9411`), which is called from the `Edited` handler. Consumer at `input.rs:9533+` only needs the first valid candidate. **Early-exit iterator over borrowed entries; both the Vec and every clone disappear.**
2. HISTFILE is read whole and held whole with no cap (`app/src/terminal/model/session.rs:1345-1402` → `history.rs:189-191`). `HISTSIZE=100000` means 100k `Arc<HistoryEntry>` resident and a 100k-element Vec allocated per keystroke by `History::commands()`.
3. Up to 26 SQLite queries per keystroke (`next_command_model.rs:198, 209` → `commands.rs:43-70`) against a table with **no index** (verified). Background executor, so not UI-blocking — but the indexes in §3.2 turn 26 scans into 26 lookups for free.
4. `buffer_text()` rebuilds a `String` char-by-char from a CRDT rope (`app/src/editor/view/model/buffer/mod.rs:1135-1137`); `buffer_text(` appears **103 times in `input.rs` alone**, and `run_input_background_jobs` calls it four more times per tick just to compare against a cached value (`input/decorations.rs:167, 189, 238, 266`). `last_buffer_text()` (`editor/view/model/mod.rs:2805-2807`) already returns `&str`.
5. `set_zero_state_hint_text` (`input.rs:6787-6879`) re-walks `COMMAND_REGISTRY.all_commands()` on **every** call, from 16 call sites, several keystroke-adjacent. Directly relevant to §2.
6. Decoration debounce is 10 ms (`input.rs:373`) — normal typing never coalesces. Sibling AI-prediction debounce is 250 ms (`:374`).

### 4.4 Per frame — the multiplier

Cursor blink at 500 ms (`app/src/editor/view/mod.rs:135, 7455-7482`) means everything below runs **twice a second at idle while focused**, and `Presenter::build_scene` lays out and paints the **entire window tree from the root**, in a `for iter in 1..=3` loop (`crates/warpui_core/src/core/app.rs:2884-2936`, whose own comment at `:2909-2912` concedes the design).

1. **`update_rich_content_heights` is called unconditionally twice per layout** — `block_list_element.rs:3211` and `:3313`, **verified: no early return on an empty map** — and it delegates straight to `update_blocks_and_sumtree` (`blocks.rs:2344` → `:2147`), which **builds a brand-new `SumTree` by walking every item and pushing into `SumTree::new()`** (verified at `blocks.rs:2158-2185`). During streaming that is a full O(all blocklist items) rebuild at least twice per frame. **Adding `if updated_heights.is_empty() { return; }` is a two-line change and probably the single cheapest large frame-cost win in the codebase.**
2. The 20-line-overhang second pass lays out **every** visible RichContent item even though a dirty-set exists for exactly this purpose (`block_list_element.rs:3255-3298`; the dirty set is taken at `:3193`).
3. `filtered_blocks()` (`blocks.rs:3184-3190`) and `bookmarked_blocks()` allocate a fresh `HashSet` and scan every block in the pane, per render (`view.rs:23878-23879`) — in an otherwise properly virtualised path. Both sets are almost always empty.
4. `rich_content_views: Vec<RichContent>` (`view.rs:2682`) — ~80 touch sites, nearly all `.iter().find()`. Should be a HashMap keyed by the unified turn ID (§3).
5. `mark_agent_view_rich_content_dirty` (`blocks.rs:1665`) creates a **new SumTree cursor per item**. Entering agent view costs O(m log n) seeks **plus** a full O(n) rebuild **plus** a re-layout of every dirtied AI block. §1.7 removes most of the reasons to enter.
6. `update_block_height_indices` (`blocks.rs:1226`) shifts every entry of `removable_blocklist_item_positions` on every insert/remove, with a load-bearing 20-line comment (`:1252-1271`) explaining that gaps and banners interpret `TotalIndex` differently (`>=` vs `>`). `maintain_pinned_to_bottom` (`:1069`) pays it twice per insertion.
7. `WarpTheme` cloned into an owned field on every `BlockListElement` construction (`block_list_element.rs:928`, again at `:3752`) — `Appearance::theme()` already returns a borrow.
8. `parse_markdown(&description_item).expect(...)` on static copy, per render, in zero-state (`agent_view/zero_state_block.rs:626-632`), `inline_action_header.rs:232`, `ask_user_question_view.rs:1913`.

**Structural note:** items 1, 2, 4 and 5 all exist because the model does not know an AI turn's height — `RichContentItem` (`blocks.rs:64`) holds only a `view_id` and a cached height, while `Block` derives height from its grids in O(1). §3's unified turn type with a model-known height deletes the entire multi-pass layout.

---

## 5. WHAT WE KEEP — and which step endangers each

| Feature | Location | Endangered by | Guard |
|---|---|---|---|
| **PS1 prompt rendering** | `classic.rs:32-395`, `prompt_render_helper.rs:59-95` | Step 3 (delete classic renderer) | Land the PS1 prompt-row variant in `universal.rs` and verify with `honor_ps1 = true` **before** deleting `classic.rs`. |
| **Warpify subshell flag + flag-pole** | `classic.rs:157-190` | Step 3 | Same. Verify in a running SSH/subshell session. |
| **CLI-agent rich input** (Claude Code/Codex/OpenCode) | `cli_agent.rs`, `cli_agent_sessions/`, `blocklist/block/cli.rs` | Step 4 (fold into modes) | Alt-screen background sampling (`cli_agent.rs:104-113`) must survive; CLI blocks are a different block type entirely. This is the hardest fold. |
| **Configurable agent toolbar** | `agent_input_footer/`, `toolbar_item.rs`, `header_toolbar_editor.rs` | Step 4 (footer merge) | Keep the toolbar engine; port the mode toggle *into* it. Existing persisted user configs must still deserialise (note the `#[serde(alias = "ImageAttach")]` precedent at `toolbar_item.rs:66`). |
| **Vim mode + status bar** | `crates/vim/`, `input/common.rs:46-110` | Step 3 | `agent.rs:148, 622` hardcode `/*show_vim_status=*/ false` while `universal.rs:108-112` shows it. **Decide explicitly** — today it is an accident of which renderer you got. |
| **Local multi-agent orchestration** (`run_agents`) | `orchestration_pill_bar.rs`, `pane_group/child_agent/` | Step 5 (fullscreen removal) | The pill bar renders **only** in fullscreen today (`pane_impl.rs:501-503`). Move it to the scope/breadcrumb row first. Do not confuse it with `orchestration_viewer_streamer`, which is session-sharing and dead. |
| **Child-agent parent navigation** | `view.rs:21371` (`try_navigate_to_parent_conversation`) | Step 6 (ESC removal) | ESC currently navigates to parent *before* any exit gating. Rebind to an explicit control in the breadcrumb row before deleting the ESC arm. |
| **Rewind / checkpoints, plan menu, profile selector, user-query menu** | `input/rewind/`, `agent.rs:243-284` | Step 2 (menu unification) — actually *fixed* by it | These are currently unavailable outside agent view. Verify each renders in Terminal mode after unification. |
| **Code review pane + inline review → agent handoff** | `app/src/code_review/`, `AgentViewEntryOrigin::InlineCodeReview` (`controller.rs:137-138`) | Step 5 (origin taxonomy pruning) | The review pane must still be able to open/attach to a conversation. Keep this origin. |
| **Prompt suggestions via MAA** | `passive_suggestions/maa.rs`, `view.rs:10009-10050` | Step 8 (dead-code sweep) | This is Uncaged-enabled and live. The *legacy* model (`passive_suggestions/legacy.rs`, `view.rs:5362, 5658-5700`) is the dead one — do not delete the wrong half. |
| **Skills panel, SSH panel, Config panel** | `workspace/view/skills_panel.rs`, `left_panel.rs:112-120` | Step 8 | Uncaged additions, always offered (`view.rs:23698-23703`). |
| **Quake mode / global hotkey** | `root_view.rs:275-500`, `keys_settings.rs:24-32` | nothing | Window-level, orthogonal. Do not touch. |
| **Grid renderer / ANSI / kitty + iTerm images / block filter / bookmarks / find** | `grid_renderer.rs`, `model/kitty.rs`, `block_filter.rs`, `find/` | Step 9 (perf) | These are the terminal. Any perf change to `BlockListElement::layout` must be verified against a long, filtered, image-bearing scrollback. |
| **Session restore** | `general.restore_session` default `true` (`general_settings.rs:28-30`) | **Step 7 (migration)** | Highest-risk step in the plan. See §7. |

**Explicitly dropped** (Warp-server dependent, currently live and can only fail): the block **"Share…"** permalink + `share_block_modal.rs` (1,566 lines) — the menu item is pushed unconditionally at `view.rs:16573` and POSTs to Warp's server (`server/server_api/block.rs:103, 146-148`), bound to cmd-shift-S and in the Blocks menu. And **voice input** — compiled in via `gui = ["voice_input"]` (`app/Cargo.toml:733`) + `script/macos/bundle:358`, setting defaults **true** (`app/src/settings/ai.rs:983`), so the mic button ships visible and enabled, and the only `Transcriber` impl POSTs to `{server_root_url}/ai/transcribe` with a bearer token (`server/server_api.rs:1033-1050`). Either wire a local model or remove both mic buttons.

**Needs a decision, not a default:** Warp Drive is half-disabled — keybindings hard-disabled in four places (`workspace/mod.rs:794, 822, 1165, 1211`), Drive menu removed (`app_menus.rs:73`), settings page hidden — but `enable_warp_drive` still defaults `true` (`drive/settings.rs:26-34`), so the left-panel tab, the View-menu item, the @-menu Workflows/Notebooks/Plans/Rules categories and the block "Save as workflow" button are all live. Pick fully-in or fully-out.

---

## 6. SEQUENCING

Each step is independently shippable and independently revertable. **Verification is always in a running build** — `./script/run` or a `--channel oss` bundle — never a unit test.

---

**Step 1 — Delete the unreachable input renderers.** *~1,100 lines removed, no behaviour change.*
Delete `input/terminal.rs`, `render_ambient_agent_status_footer`, all `render_cloud_mode_v2_*`, `cloud_mode_v2_history_menu.rs`, `render_cloud_mode_v2_footer`, the `CLOUD_MODE_V2_*` constants, and the dead `!AgentView` arms at `input.rs:16371-16374`. Collapse `Input::render` to three arms.
**Verify in a running build:** (a) default install — terminal input renders identically (screenshot diff); (b) Settings ▸ Appearance ▸ Input ▸ "Shell (PS1)" — classic input still renders, prompt on the same line; (c) Ctrl-G on a running `claude` — CLI rich input still opens; (d) cmd-enter — agent input still renders.
**Belt-and-braces:** before deleting, ship one build with `debug_assert!(false, "render_terminal_input reached")` at `terminal.rs:28` and dogfood it for a day.

---

**Step 2 — One inline-menu helper.** *~200 lines removed, one bug fixed.*
Replace the four copies with `render_active_inline_menu(&self, app)` carrying agent view's 11-menu superset.
**Verify:** in **Terminal mode** (not agent view), `/rewind` opens the rewind menu and `/plan` opens the plan menu — both currently impossible. Model selector, profile selector and user-query menu also render in Terminal mode.

---

**Step 3 — PS1 becomes a prompt-row mode; delete `classic.rs`.** *~400 lines removed.*
Port lprompt/rprompt placement (`classic.rs:76-90`) and the warpify flag-pole (`:157-190`) into `render_universal_developer_input_prompt`. Then collapse `InputBoxType`, `input_type()`, `is_classic_input_enabled`, `is_universal_developer_input_enabled` (11 sites) and the two sync hooks.
**Verify:** with `terminal.input.honor_ps1 = true` in `~/.uncaged/settings.toml`: prompt renders on the same line as the input; `$PS1` colour codes intact; rprompt right-aligned; compact mode spacing preserved. In an SSH/subshell session: the warpify flag renders. Toggling the Appearance radio and the command-palette "Toggle Input Mode" both still work.

---

**Step 4 — One footer.** *~1,000 lines removed net.*
Port the segmented Terminal/Agent control into `AgentToolbarItemKind::ModeToggle` (pinned, non-removable); swap `ToolbarAvailability` for a mode filter; delete `UniversalDeveloperInputButtonBar` and the `ShareSession`/`HandoffToCloud` items. Fold CLI-agent input into `SurfaceMode::CliAgent`.
**Verify:** existing toolbar configs still load (test with a customised toolbar written by the previous build); the mode toggle appears in Terminal, Agent and CLI-agent modes; the mic/file/model-selector behave identically in all three; the alt-screen background still matches a running `claude`.

---

**Step 5 — Fullscreen becomes an explicit scope.** *Behaviour change — flag it in release notes.*
Stop auto-entering `FullScreen`. Turn conversation filtering into a breadcrumb pill with an "All" exit. Move `OrchestrationPillBar` and parent-navigation into that row. Delete `AgentViewEntryBlock`, `InlineAgentViewHeader`, and the dead `AgentViewEntryOrigin` variants (`controller.rs:196-204` and the cloud-only ones). Stop forcing `InputMode::PinnedToBottom` (`view.rs:27477-27481`).
**Verify:** start an agent conversation from a terminal with visible scrollback — **the shell blocks stay on screen**; the reply appends below. With `input_mode = PinnedToTop`, starting a conversation does **not** silently flip to PinnedToBottom. `run_agents` spawning children still shows the pill bar. Child agent → parent navigation still works from the breadcrumb.

---

**Step 6 — ESC stops being an exit.** *Small diff, big feel change.*
Delete `view.rs:21367-21411`, `PendingConfirmation::Exit` / `ExitConfirmationTrigger` / `exit_agent_view_with_required_confirmation` (`controller.rs:71-92, 494-593, 860, 1025-1063`), Ctrl-C exit (`view.rs:8585-8592, 8767`), the header back button (`view.rs:4212-4231`), and the two escape hint rows.
**Verify:** in an agent conversation with text in the input — ESC clears the input and does **not** show "press escape again"; ESC with a slash menu open closes the menu; ESC with nothing open does nothing visible. Ctrl-C interrupts the agent rather than exiting. No UI anywhere says "escape — go back to terminal".

---

**Step 7 — One history: migration + identity + ordering.** *Highest risk. Ship alone.*
The migration in §3.2, deterministic exchange IDs at `convert_conversation.rs:1816`, delete `to_serialized_blocklist_items` and the `.or_else()` fork, Cmd-K watermark, delete `find_block_indices_for_exchange_timestamps` and the `start_ts` re-sort.
**Verify — on a copy of a real, months-old `warp.sqlite`, not a fresh one:** (a) restart with `restore_session = true`; every previously visible block and agent turn comes back, in the same order, once each; (b) an agent-executed command appears **exactly once**, with its git branch and prompt snapshot intact; (c) Cmd-K, then restart — cleared content stays cleared and nothing else vanishes; (d) fork a conversation — the forked pane shows one copy of each command; (e) hide an exchange, restart — it stays hidden (impossible today); (f) up-arrow still merges shell commands and prompts; (g) `sqlite3 warp.sqlite "SELECT count(*) FROM blocks WHERE block_id = ''"` returns 0.
**Rollback:** the migration is additive, so an old binary still reads the DB. Keep the old read path behind a runtime kill-switch for one release.

---

**Step 8 — One zero state.** *~1,300 lines removed.*
Everything in §2: one `ZeroStateBlock` + hint registry; delete Oz section + its two settings + toggle + keymap flag, the cloud branches, the credits banner, the NLD checkbox, the escape hint, the dead `StateHandles` fields and enum variants, the `UniversalInput` onboarding flow, the legacy AI-assistant panel, `GetStartedView`. Fold the shortcuts panel, message-bar producers and input placeholder onto the same registry.
**Verify:** fresh profile, engine configured → new terminal tab shows one zero state with hints; start an agent conversation → the same block, agent-mode hints, no second panel; in a directory with prior conversations → recent activity **and** hints (both, not either); `/init` callout appears once per repo; Settings ▸ AI no longer offers "Show Uncaged changelog in new conversation view"; shift-? shortcuts panel lists the same hints in the same words.

---

**Step 9 — Frame and keystroke perf.** *Small diffs, measurable.*
`if updated_heights.is_empty() { return; }` in `update_rich_content_heights`; skip the overhang pass for non-dirty items; incremental `filtered_blocks`/`bookmarked_blocks`; `Arc<WarpTheme>`; early-exit autosuggestion iterator; `last_buffer_text()` at the comparison sites; `OnceLock` the static `parse_markdown` calls; cap retained HISTFILE.
**Verify:** attach a profiler (or `jemalloc_pprof`, already a feature) to a running build with a 2,000-block scrollback and a streaming agent reply. Expect: SumTree rebuilds per frame drop from ≥2 to ~0 at idle; idle CPU with a focused blinking cursor drops measurably. Type a 1-char prefix with `HISTSIZE=100000` and confirm no multi-ms UI-thread stall.

---

**Step 10 — Startup and binary.** *Independent of everything above; can be done in parallel by a second person.*
Drop `nld_classifier_v3` from the three bundle scripts and delete the eager `InputClassifierModel` registration (`lib.rs:2259`); cap `get_all_restored_blocks` in-query; lazy `read_agent_conversations`; limit the `commands` read; delete the orphan images and the Windows `.pdb`s; trim the tree-sitter grammar list; gate the command-signature embed; prune the cloud default features.
**Verify:** `ls -l` the built binary before and after (baseline this session: 373,721,016 B for `target/release/warp-oss`); time from launch to first frame (`WINDOWS_CREATED` mark, `lib.rs:2797`) with a real restored session. Confirm each pruned feature by launching and exercising a feature that *should* still work — the risk is a cross-dependency, not the deletion.

---

## 7. RISKS AND UNKNOWNS

**Could not establish from source — needs a running build or an owner:**

1. **Does `local_tty` come only from `build.rs`?** Verified `app/build.rs:225-228` emits `cargo:rustc-cfg=feature="local_fs"` / `"local_tty"` for every non-wasm target, and neither appears in `default`. **Any audit of "live features" that reads only `Cargo.toml` will be wrong.** Settle: `cargo build -v` and grep the rustc invocation. This matters for every feature-pruning decision in Step 10.

2. **Exact binary section sizes for the shipped `release-lto` universal build.** All §4.1 numbers came from a `release` (non-LTO, single-arch) binary or from on-disk asset sizes. Settle: build one `--channel oss` universal bundle, `size -m` each slice, before and after Step 10.

3. **Whether `cloud_conversations` is truly dormant.** The flag is in `default` (`app/Cargo.toml:611`) and `load_conversation_from_server` gates on it (`history_model/conversation_loader.rs:104`), but every call also needs a `ServerConversationToken` that only exists for conversations that reached Warp's backend. If it is genuinely unreachable, `has_cloud_data`, `server_conversation_token`, `server_metadata`, `forked_from_server_conversation_token` and `CLIAgentConversation` all become dead weight in the merged history model — a meaningful simplification. **Settle: ask the Uncaged auth owner.** Do not guess.

4. **Real-world `warp.sqlite` shape.** The Step 7 backfill's behaviour depends on how many rows fall into the two known-NULL populations (pre-`2024-01-19` empty `block_id`; `2025-12-25`–`2026-01-15` dropped conversation link). Settle: run the counting queries against several real profiles before writing the migration:
   `SELECT count(*) FROM blocks WHERE block_id = '';`
   `SELECT count(*) FROM blocks WHERE agent_view_visibility IS NULL AND ai_metadata IS NULL;`

5. **Whether deterministic exchange IDs are actually stable.** They derive from `request_id`, and exchange boundaries are inferred by watching `request_id` change (`convert_conversation.rs:346-372`). For a local engine we control the batching — but I could not confirm that `uncaged_engine` assigns `request_id` deterministically across a restore of the same proto. **Settle: restore the same conversation twice in a running build and diff the derived IDs.** If they differ, fall back to persisting exchange IDs in `turn_index` (one extra column, no schema redesign).

6. **Does removing fullscreen break `OrchestrationPillBar`'s `pill_specs`?** The pill bar short-circuits to Empty when `pill_specs` returns None (`orchestration_pill_bar.rs:1057`), but I did not trace whether `pill_specs` itself depends on `is_fullscreen()`. Settle: read `orchestration_pill_bar_model.rs` before Step 5.

7. **CLI-agent block rendering under a unified list.** `blocklist/block/cli.rs` is 88 KB and CLI-agent blocks are a genuinely different block type. Whether they can share `BlockHeightItem` cleanly with a model-known height is the one part of §3 I could not verify. Settle: prototype the height computation for one CLI block before committing to Step 7's unified turn type.

**Known-and-accepted risks:**

- **Step 7 is the only step that can lose user data.** Everything before it is reversible by `git revert`. Ship it alone, on its own release, with the old read path behind a kill-switch.
- **Unit tests will not catch regressions here.** They run `AgentView = false`, a configuration nobody ships. Treat a green suite as evidence of nothing. Every verification above is a running-build check for that reason — and it is worth adding a small number of *integration* tests (`crates/integration`, which does init feature flags) for the Step 7 restore paths specifically.
- **`app/src/ai/blocklist/block.rs` is 292 KB**, the largest file in the AI tree, and instantiates one heavyweight warpui view per agent turn with 8+ child-view maps. Nothing in this plan reduces that. It is the other half of the perf story and deserves its own plan.