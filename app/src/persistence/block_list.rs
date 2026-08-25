//! Manages how we write to and read from our SQLite database for our AI features.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{Local, NaiveDateTime, TimeZone};
use diesel::prelude::*;
use diesel::result::Error;
use diesel::sqlite::SqliteConnection;
use itertools::Itertools;

use super::model::Block;
use super::{model, schema};
use crate::ai::blocklist::{PersistedAIInput, SerializedBlockListItem};
use crate::app_state::PaneUuid;
use crate::persistence::schema::ai_queries;
use crate::terminal::model::block::{SerializedAgentViewVisibility, SerializedBlock};

const MAX_TERMINAL_BLOCKS_TO_PERSIST_PER_SESSION: i64 = 100;

type PersistedBlocks = HashMap<PaneUuid, Vec<SerializedBlockListItem>>;

/// An AI query read from the SQLite DB.
#[derive(Identifiable, Insertable, Queryable, Selectable)]
#[diesel(table_name = ai_queries)]
#[diesel(primary_key(id))]
pub(super) struct AIQuery {
    pub(super) id: i32,
    pub(super) exchange_id: String,
    pub(super) conversation_id: String,
    pub(super) start_ts: NaiveDateTime,
    pub(super) output_status: String,
    pub(super) input: String,
    pub(super) working_directory: Option<String>,
    pub(super) model_id: String,
    pub(super) coding_model_id: String,

    // Planning model selection is deprecated and unused.
    #[allow(unused)]
    pub(super) planning_model_id: String,
}

impl TryFrom<AIQuery> for PersistedAIInput {
    type Error = anyhow::Error;

    fn try_from(value: AIQuery) -> Result<Self, Self::Error> {
        Ok(Self {
            start_ts: Local.from_utc_datetime(&value.start_ts),
            inputs: serde_json::from_str(&value.input)?,
            exchange_id: value.exchange_id.try_into()?,
            conversation_id: value.conversation_id.try_into()?,
            output_status: serde_json::from_str(&value.output_status)?,
            working_directory: value.working_directory,
            model_id: value.model_id.into(),
            coding_model_id: value.coding_model_id.into(),
        })
    }
}

/// A new AI query to be inserted into the SQLite DB.
#[derive(Insertable, AsChangeset)]
#[diesel(table_name = ai_queries)]
#[diesel(treat_none_as_null = true)]
pub(super) struct NewAIQuery {
    pub(super) exchange_id: String,
    pub(super) conversation_id: String,
    pub(super) start_ts: NaiveDateTime,
    pub(super) output_status: String,
    pub(super) input: String,
    pub(super) working_directory: Option<String>,
    pub(super) model_id: String,
}

impl TryFrom<&PersistedAIInput> for NewAIQuery {
    type Error = anyhow::Error;

    fn try_from(value: &PersistedAIInput) -> Result<Self, Self::Error> {
        Ok(Self {
            start_ts: value.start_ts.naive_utc(),
            input: serde_json::to_string(&value.inputs)?,
            working_directory: value.working_directory.clone(),
            exchange_id: value.exchange_id.to_string(),
            conversation_id: value.conversation_id.to_string(),
            output_status: serde_json::to_string(&value.output_status)?,
            model_id: value.model_id.clone().into(),
        })
    }
}

pub(super) fn read_ai_queries(
    conn: &mut SqliteConnection,
) -> Result<Vec<PersistedAIInput>, diesel::result::Error> {
    // Only load at most 100 AI queries; there's a very low chance that the user
    // will ever try rerunning AI queries older than this duration and loading
    // all AI queries in perpetuity has performance implications on app startup.
    // TOOD(alokedesai): Consider loading all AI queries by paginating the SQL query.
    const MAX_AI_QUERIES_TO_READ: i64 = 100;
    Ok(schema::ai_queries::table
        .select(AIQuery::as_select())
        .order_by(schema::ai_queries::columns::start_ts.desc())
        .limit(MAX_AI_QUERIES_TO_READ)
        .load::<AIQuery>(conn)?
        .into_iter()
        .filter_map(|ai_query| PersistedAIInput::try_from(ai_query).ok())
        .rev()
        .collect_vec())
}

const AI_QUERIES_COUNT_LIMIT: i64 = 10_000;

pub(super) fn upsert_ai_query(
    conn: &mut SqliteConnection,
    query: Arc<PersistedAIInput>,
) -> anyhow::Result<()> {
    upsert_ai_query_with_limit(conn, query, AI_QUERIES_COUNT_LIMIT)
}

/// Upserts an AI query while keeping the `ai_queries` table capped at `limit` rows by evicting
/// the oldest queries (FIFO by `id`). Split out from [`upsert_ai_query`] so tests can exercise the
/// eviction path with a small limit instead of inserting `AI_QUERIES_COUNT_LIMIT` rows.
fn upsert_ai_query_with_limit(
    conn: &mut SqliteConnection,
    query: Arc<PersistedAIInput>,
    limit: i64,
) -> anyhow::Result<()> {
    use schema::ai_queries::dsl::*;

    let new_ai_query = NewAIQuery::try_from(query.as_ref())?;

    Ok(conn.transaction::<_, Error, _>(|conn| {
        // Only a genuinely new exchange grows the table.
        let is_new_exchange = ai_queries
            .filter(exchange_id.eq(&new_ai_query.exchange_id))
            .count()
            .first::<i64>(conn)?
            == 0;
        if is_new_exchange {
            let query_count: i64 = ai_queries.count().first(conn)?;
            // add 1 because we are about to insert a new row.
            let diff = query_count - limit + 1;
            if diff > 0 {
                // Find the oldest row to keep and evict everything older (FIFO).
                let last_kept_id: Option<i32> = ai_queries
                    .select(id)
                    .order(id.asc())
                    .offset(diff)
                    .limit(1)
                    .first(conn)
                    .optional()?;
                if let Some(last_kept_id) = last_kept_id {
                    diesel::delete(ai_queries.filter(id.lt(last_kept_id))).execute(conn)?;
                }
            }
        }

        diesel::insert_into(ai_queries)
            .values(&new_ai_query)
            .on_conflict(exchange_id)
            .do_update()
            .set(&new_ai_query)
            .execute(conn)?;

        Ok(())
    })?)
}

/// Returns the most recent [`MAX_BLOCK_COUNT_PER_SESSION`] block list items for each session. The
/// items are in chronological order.
pub(super) fn get_all_restored_blocks(
    conn: &mut SqliteConnection,
) -> Result<PersistedBlocks, diesel::result::Error> {
    let terminal_sessions = schema::terminal_panes::table
        .select(model::TerminalSession::as_select())
        .load::<model::TerminalSession>(conn)?;

    let block_lists = Block::belonging_to(&terminal_sessions)
        .select(Block::as_select())
        .order_by(schema::blocks::columns::id.asc())
        .load::<Block>(conn)?
        .grouped_by(&terminal_sessions);

    let mut all_block_items_by_pane = block_lists
        .into_iter()
        .zip(terminal_sessions)
        .map(|(blocks, terminal_pane)| {
            (
                PaneUuid(terminal_pane.uuid),
                blocks.into_iter().map(Into::into).collect(),
            )
        })
        .collect::<HashMap<_, Vec<SerializedBlockListItem>>>();

    for (_, blocks) in all_block_items_by_pane.iter_mut() {
        blocks.sort_by_key(|item| item.start_ts());
        // Only keep most recent command blocks
        blocks.drain(
            0..blocks
                .len()
                .saturating_sub(MAX_TERMINAL_BLOCKS_TO_PERSIST_PER_SESSION as usize),
        );
    }

    Ok(all_block_items_by_pane)
}

/// Uncaged one-history: the turn_index-driven restore read.
///
/// Blocks are returned in `turn_index.seq` order per pane, with turns below the
/// pane's Cmd-K watermark filtered out. Blocks without a turn row (written
/// while the flag was off, or legacy) are appended after the indexed ones in
/// rowid order so nothing is ever silently dropped. `get_all_restored_blocks`
/// stays byte-identical as the kill-switch fallback.
pub(super) fn get_all_restored_blocks_v2(
    conn: &mut SqliteConnection,
) -> Result<PersistedBlocks, diesel::result::Error> {
    use std::collections::HashMap as StdHashMap;

    // q1: watermarks.
    let watermarks: StdHashMap<Vec<u8>, i64> = schema::pane_clear_watermarks::table
        .select(model::PaneClearWatermark::as_select())
        .load::<model::PaneClearWatermark>(conn)?
        .into_iter()
        .map(|row| (row.pane_leaf_uuid, row.cleared_before_seq))
        .collect();

    // q2: shell turn rows, seq-ordered.
    let turns: Vec<model::TurnIndexEntry> = schema::turn_index::table
        .select(model::TurnIndexEntry::as_select())
        .filter(schema::turn_index::dsl::kind.eq("shell"))
        .order_by((
            schema::turn_index::dsl::pane_leaf_uuid.asc(),
            schema::turn_index::dsl::seq.asc(),
        ))
        .load(conn)?;

    // q3: all blocks (same shape as v1), joined in memory by turn_id/block_id.
    let terminal_sessions = schema::terminal_panes::table
        .select(model::TerminalSession::as_select())
        .load::<model::TerminalSession>(conn)?;
    let block_lists = Block::belonging_to(&terminal_sessions)
        .select(Block::as_select())
        .order_by(schema::blocks::columns::id.asc())
        .load::<Block>(conn)?
        .grouped_by(&terminal_sessions);

    let mut all_block_items_by_pane: PersistedBlocks = HashMap::new();
    for (blocks, terminal_pane) in block_lists.into_iter().zip(terminal_sessions) {
        let pane_uuid = terminal_pane.uuid.clone();
        let watermark = watermarks.get(&pane_uuid).copied().unwrap_or(i64::MIN);

        let mut by_block_id: StdHashMap<String, Block> = blocks
            .into_iter()
            .map(|block| (block.block_id.clone(), block))
            .collect();

        let mut ordered: Vec<SerializedBlockListItem> = Vec::new();
        for turn in turns
            .iter()
            .filter(|turn| turn.pane_leaf_uuid == pane_uuid && turn.seq >= watermark)
        {
            if let Some(block_id) = turn.block_id.as_deref() {
                if let Some(block) = by_block_id.remove(block_id) {
                    ordered.push(block.into());
                }
            }
        }
        // Un-indexed leftovers (flag-off writes, legacy rows): keep them, in
        // rowid order, after the indexed timeline -- but only when the pane was
        // never cleared, since a cleared pane's leftovers predate the clear.
        if watermarks.get(&pane_uuid).is_none() {
            let mut leftovers: Vec<Block> = by_block_id.into_values().collect();
            leftovers.sort_by_key(|block| block.id);
            ordered.extend(leftovers.into_iter().map(Into::into));
        }

        // Same cap semantics as v1 (shell blocks only, most recent kept).
        let overflow = ordered
            .len()
            .saturating_sub(MAX_TERMINAL_BLOCKS_TO_PERSIST_PER_SESSION as usize);
        ordered.drain(0..overflow);

        all_block_items_by_pane.insert(PaneUuid(pane_uuid), ordered);
    }

    Ok(all_block_items_by_pane)
}

pub(super) fn save_block(
    conn: &mut SqliteConnection,
    pane_id: Vec<u8>,
    block: &SerializedBlock,
    is_local_block: bool,
) -> Result<(), Error> {
    use schema::blocks::dsl::*;
    conn.transaction::<_, Error, _>(|conn| {
        let saved_blocks_count: i64 = schema::blocks::dsl::blocks
            .filter(pane_leaf_uuid.eq(pane_id.clone()))
            .filter(id.is_not_null())
            .filter(is_background.ne(true))
            .count()
            .first(conn)?;

        // add 1 because we are about to save a new block
        let diff = saved_blocks_count - MAX_TERMINAL_BLOCKS_TO_PERSIST_PER_SESSION + 1;
        if diff > 0 {
            // Find the oldest block to keep.
            let last_kept_id: Option<i32> = schema::blocks::dsl::blocks
                .filter(pane_leaf_uuid.eq(pane_id.clone()))
                .filter(id.is_not_null())
                .filter(is_background.ne(true))
                .select(id)
                .order(id.asc())
                .offset(diff)
                .limit(1)
                .first(conn)?;

            if let Some(last_kept_id) = last_kept_id {
                // Uncaged one-history: evicted blocks take their turn rows
                // with them, in the same transaction.
                let evicted_block_ids: Vec<String> = schema::blocks::dsl::blocks
                    .filter(id.lt(last_kept_id))
                    .filter(pane_leaf_uuid.eq(pane_id.clone()))
                    .select(block_id)
                    .load(conn)?;
                diesel::delete(
                    schema::turn_index::dsl::turn_index
                        .filter(schema::turn_index::dsl::turn_id.eq_any(&evicted_block_ids)),
                )
                .execute(conn)?;
                diesel::delete(
                    schema::blocks::dsl::blocks
                        .filter(id.lt(last_kept_id))
                        .filter(pane_leaf_uuid.eq(pane_id.clone())),
                )
                .execute(conn)?;
            }
        }

        let serialized_block_id = block.id.to_string();
        let block_start_ts = block.start_ts.map(|ts| ts.naive_utc());
        let block_conversation_id = block
            .ai_metadata
            .as_ref()
            .and_then(|meta| serde_json::from_str::<serde_json::Value>(meta).ok())
            .and_then(|value| {
                value
                    .get("conversation_id")
                    .and_then(|conv| conv.as_str())
                    .map(|conv| conv.to_string())
            });
        let new_block = create_block(pane_id.clone(), block, is_local_block);
        diesel::insert_into(schema::blocks::dsl::blocks)
            .values(new_block)
            .execute(conn)?;

        // Uncaged one-history: record the shell turn in the shared timeline,
        // same transaction. turn_id == block_id; idempotent on re-saves.
        {
            use diesel::sql_types::{Binary, Nullable, Text, Timestamp};
            diesel::sql_query(
                "INSERT INTO turn_index (turn_id, pane_leaf_uuid, seq, kind, block_id, conversation_id, exchange_ord, created_ts) \
                 VALUES (?, ?, (SELECT COALESCE(MAX(seq), -1) + 1 FROM turn_index WHERE pane_leaf_uuid = ?), 'shell', ?, ?, NULL, ?) \
                 ON CONFLICT(turn_id) DO NOTHING",
            )
            .bind::<Text, _>(serialized_block_id.clone())
            .bind::<Binary, _>(pane_id.clone())
            .bind::<Binary, _>(pane_id)
            .bind::<Text, _>(serialized_block_id)
            .bind::<Nullable<Text>, _>(block_conversation_id)
            .bind::<Nullable<Timestamp>, _>(block_start_ts)
            .execute(conn)?;
        }
        Ok(())
    })
}

// TODO(vorporeal): can move this to a `to_persisted_block()` function on `SerializedBlock`
// to get it out of the persistence layer.
fn create_block<'a>(
    pane_leaf_uuid: Vec<u8>,
    block: &'a SerializedBlock,
    is_local: bool,
) -> model::NewBlock<'a> {
    model::NewBlock {
        block_id: block.id.as_str(),
        pane_leaf_uuid,
        stylized_command: &block.stylized_command,
        stylized_output: &block.stylized_output,
        pwd: block.pwd.as_ref(),
        // This sqlite column still uses the legacy `git_branch` name, but it now stores the
        // block's git head for backwards compatibility with existing persisted data.
        git_branch: block.git_head.as_ref(),
        git_branch_name: block.git_branch_name.as_ref(),
        virtual_env: block.virtual_env.as_ref(),
        conda_env: block.conda_env.as_ref(),
        exit_code: block.exit_code.value(),
        did_execute: block.did_execute,
        completed_ts: block.completed_ts.map(|ts| ts.naive_utc()),
        start_ts: block.start_ts.map(|ts| ts.naive_utc()),
        ps1: block.ps1.as_ref(),
        rprompt: block.rprompt.as_ref(),
        honor_ps1: block.honor_ps1,
        is_background: block.is_background,
        shell: block.shell_host.as_ref().map(|host| host.shell_type.name()),
        user: block.shell_host.as_ref().map(|host| host.user.as_str()),
        host: block.shell_host.as_ref().map(|host| host.hostname.as_str()),
        prompt_snapshot: block.prompt_snapshot.as_ref(),
        ai_metadata: block.ai_metadata.as_ref(),
        is_local: Some(is_local),
        agent_view_visibility: block
            .agent_view_visibility
            .as_ref()
            .and_then(|v| serde_json::to_string(v).ok()),
        // Uncaged one-history: a shell turn's id IS its block id.
        turn_id: Some(block.id.as_str()),
    }
}

pub(super) fn delete_blocks(conn: &mut SqliteConnection, pane_id: Vec<u8>) -> Result<(), Error> {
    use schema::blocks::dsl::*;
    conn.transaction::<_, Error, _>(|conn| {
        diesel::delete(schema::blocks::dsl::blocks.filter(pane_leaf_uuid.eq(pane_id.clone())))
            .execute(conn)?;
        // Uncaged one-history: a hard pane delete also drops the pane's
        // timeline and watermark (this path is pane close, not Cmd-K).
        diesel::delete(
            schema::turn_index::dsl::turn_index
                .filter(schema::turn_index::dsl::pane_leaf_uuid.eq(pane_id.clone())),
        )
        .execute(conn)?;
        diesel::delete(
            schema::pane_clear_watermarks::dsl::pane_clear_watermarks
                .filter(schema::pane_clear_watermarks::dsl::pane_leaf_uuid.eq(pane_id)),
        )
        .execute(conn)?;
        Ok(())
    })
}

/// Uncaged one-history: inserts an agent exchange's turn row, allocating the
/// pane's next seq. Idempotent on turn_id (streaming updates re-emit); the
/// single writer thread makes MAX(seq)+1 race-free.
pub(super) fn upsert_agent_turn(
    conn: &mut SqliteConnection,
    pane_uuid: Vec<u8>,
    conversation_id: String,
    turn_id: String,
    exchange_ord: i64,
    created_ts: Option<chrono::NaiveDateTime>,
) -> Result<(), Error> {
    use diesel::sql_types::{BigInt, Binary, Nullable, Text, Timestamp};
    diesel::sql_query(
        "INSERT INTO turn_index (turn_id, pane_leaf_uuid, seq, kind, block_id, conversation_id, exchange_ord, created_ts) \
         VALUES (?, ?, (SELECT COALESCE(MAX(seq), -1) + 1 FROM turn_index WHERE pane_leaf_uuid = ?), 'agent', NULL, ?, ?, ?) \
         ON CONFLICT(turn_id) DO UPDATE SET exchange_ord = excluded.exchange_ord",
    )
    .bind::<Text, _>(turn_id)
    .bind::<Binary, _>(pane_uuid.clone())
    .bind::<Binary, _>(pane_uuid)
    .bind::<Text, _>(conversation_id)
    .bind::<BigInt, _>(exchange_ord)
    .bind::<Nullable<Timestamp>, _>(created_ts)
    .execute(conn)?;
    Ok(())
}

/// Uncaged one-history (B5): moves the pane's clear watermark past every
/// existing turn. Seq keeps growing monotonically across clears.
pub(super) fn set_cleared_before_seq(
    conn: &mut SqliteConnection,
    pane_id: Vec<u8>,
) -> Result<(), Error> {
    use diesel::sql_types::Binary;
    diesel::sql_query(
        "INSERT INTO pane_clear_watermarks (pane_leaf_uuid, cleared_before_seq) \
         VALUES (?, (SELECT COALESCE(MAX(seq), -1) + 1 FROM turn_index WHERE pane_leaf_uuid = ?)) \
         ON CONFLICT(pane_leaf_uuid) DO UPDATE SET cleared_before_seq = excluded.cleared_before_seq",
    )
    .bind::<Binary, _>(pane_id.clone())
    .bind::<Binary, _>(pane_id)
    .execute(conn)?;
    Ok(())
}

pub(super) fn update_block_agent_view_visibility(
    conn: &mut SqliteConnection,
    target_block_id: &str,
    visibility: &SerializedAgentViewVisibility,
) -> anyhow::Result<()> {
    use schema::blocks::dsl::*;
    let visibility_json = serde_json::to_string(visibility)?;
    diesel::update(blocks.filter(block_id.eq(target_block_id)))
        .set(agent_view_visibility.eq(visibility_json))
        .execute(conn)?;
    Ok(())
}

pub(super) fn delete_ai_conversation(
    conn: &mut SqliteConnection,
    conversation_id_str: &str,
) -> anyhow::Result<()> {
    use schema::ai_queries::dsl as queries_dsl;

    conn.transaction::<_, Error, _>(|conn| {
        // Delete the AI query
        diesel::delete(
            queries_dsl::ai_queries.filter(queries_dsl::conversation_id.eq(conversation_id_str)),
        )
        .execute(conn)?;

        // Uncaged one-history: drop the conversation's agent turn rows too.
        diesel::delete(
            schema::turn_index::dsl::turn_index
                .filter(schema::turn_index::dsl::conversation_id.eq(conversation_id_str))
                .filter(schema::turn_index::dsl::kind.eq("agent")),
        )
        .execute(conn)?;

        Ok(())
    })?;

    Ok(())
}

#[cfg(test)]
#[path = "block_list_tests.rs"]
mod tests;
