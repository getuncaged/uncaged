-- Uncaged: indexes for the queries that run on the input hot path.
-- The schema previously had no indexes beyond rowid primary keys, so
-- next/previous-command lookups and same-command-in-context lookups were
-- full table scans of `commands`, and restoring or pruning a pane scanned
-- all of `blocks`.

-- get_next_command / get_previous_commands: filter on session_id, range/order on id.
CREATE INDEX IF NOT EXISTS idx_commands_session_id_id ON commands (session_id, id);

-- get_same_commands_from_history: filter on command + pwd, order by id.
CREATE INDEX IF NOT EXISTS idx_commands_command_pwd ON commands (command, pwd);

-- Block-list restore/delete for a pane: filter on pane_leaf_uuid.
CREATE INDEX IF NOT EXISTS idx_blocks_pane_leaf_uuid ON blocks (pane_leaf_uuid);
