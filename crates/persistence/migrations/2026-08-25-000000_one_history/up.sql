-- Uncaged one-history (§3): one persisted timeline for shell blocks and agent
-- turns, stable block identity, and a non-destructive Cmd-K watermark.
-- Additive only: `blocks` gains a trailing column; two new tables; no rewrites
-- of existing columns beyond the block_id backfills below.

-- B2a: backfill legacy empty block_ids. `id` is the INTEGER PRIMARY KEY rowid
-- alias and is never NULL, so the derived ids are unique.
UPDATE blocks SET block_id = 'legacy-' || id WHERE block_id = '';

-- B2b: dedupe fork-duplicated block_ids. persist_blocks_for_forked_conversation
-- used to re-insert the SAME block_id under a new pane uuid, so real databases
-- contain duplicates; a bare unique index would fail the migration and brick
-- persistence on every boot. Keep the lowest rowid as canonical.
UPDATE blocks SET block_id = 'legacy-' || id
WHERE id NOT IN (SELECT MIN(id) FROM blocks GROUP BY block_id);

CREATE UNIQUE INDEX ux_blocks_block_id ON blocks(block_id);

-- The shared timeline (B4/B6). One row per turn, shell or agent; content stays
-- in `blocks` / `agent_tasks` — this table records identity and order only.
-- No FOREIGN KEY clauses on purpose: PRAGMA foreign_keys=ON plus the blocks
-- 100-row eviction and the terminal_panes snapshot rewrite make FKs untenable
-- (see the NewBlock precedent in crates/persistence/src/model.rs).
CREATE TABLE turn_index (
    turn_id TEXT PRIMARY KEY NOT NULL,
    pane_leaf_uuid BLOB NOT NULL,
    seq BIGINT NOT NULL,
    kind TEXT NOT NULL,
    block_id TEXT,
    conversation_id TEXT,
    exchange_ord BIGINT,
    created_ts TIMESTAMP
);
CREATE INDEX idx_turn_index_pane_seq ON turn_index(pane_leaf_uuid, seq);
CREATE INDEX idx_turn_index_conversation ON turn_index(conversation_id, exchange_ord);

-- B5: the Cmd-K watermark. Deliberately NOT a column on terminal_panes --
-- save_app_state deletes and re-inserts every terminal_panes row on every
-- snapshot, which would silently wipe the column.
CREATE TABLE pane_clear_watermarks (
    pane_leaf_uuid BLOB PRIMARY KEY NOT NULL,
    cleared_before_seq BIGINT NOT NULL
);

ALTER TABLE blocks ADD COLUMN turn_id TEXT;

-- Shell-turn backfill: runs once, writes the timeline down. seq is the row's
-- position by rowid within its pane. json_valid guards json_extract: malformed
-- ai_metadata must degrade to NULL, not fail the migration.
INSERT INTO turn_index (turn_id, pane_leaf_uuid, seq, kind, block_id, conversation_id, exchange_ord, created_ts)
SELECT b.block_id,
       b.pane_leaf_uuid,
       ROW_NUMBER() OVER (PARTITION BY b.pane_leaf_uuid ORDER BY b.id) - 1,
       'shell',
       b.block_id,
       CASE WHEN b.ai_metadata IS NOT NULL AND json_valid(b.ai_metadata)
            THEN json_extract(b.ai_metadata, '$.conversation_id') ELSE NULL END,
       NULL,
       b.start_ts
FROM blocks b;

UPDATE blocks SET turn_id = block_id;
