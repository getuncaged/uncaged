DROP INDEX IF EXISTS ux_blocks_block_id;
DROP INDEX IF EXISTS idx_turn_index_pane_seq;
DROP INDEX IF EXISTS idx_turn_index_conversation;
DROP TABLE IF EXISTS turn_index;
DROP TABLE IF EXISTS pane_clear_watermarks;
ALTER TABLE blocks DROP COLUMN turn_id;
