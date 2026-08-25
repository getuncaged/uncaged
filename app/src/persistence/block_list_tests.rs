//! Unit tests for the `ai_queries` persistence layer in [`super`].
//!
//! Covers the FIFO eviction cap added to [`super::upsert_ai_query`] and the empty-input filter
//! that drives the persistence skip in `handle_ai_history_event`.

use std::sync::Arc;

use chrono::Local;
use diesel::sqlite::SqliteConnection;
use diesel::{Connection, ExpressionMethods, QueryDsl, RunQueryDsl};
use diesel_migrations::MigrationHarness;

use super::upsert_ai_query_with_limit;
use crate::ai::agent::conversation::AIConversationId;
use crate::ai::agent::{AIAgentExchangeId, AIAgentInput, UserQueryMode};
use crate::ai::blocklist::{AIQueryHistoryOutputStatus, PersistedAIInput, PersistedAIInputType};
use crate::ai::llms::LLMId;

/// Builds an in-memory SQLite database with all migrations applied.
fn test_connection() -> SqliteConnection {
    let mut conn =
        SqliteConnection::establish(":memory:").expect("in-memory sqlite connection should open");
    conn.run_pending_migrations(::persistence::MIGRATIONS)
        .expect("migrations should run");
    conn
}

/// Builds a query-bearing [`PersistedAIInput`] with a fresh, unique `exchange_id`.
fn make_query(text: &str) -> Arc<PersistedAIInput> {
    Arc::new(PersistedAIInput {
        exchange_id: AIAgentExchangeId::new(),
        conversation_id: AIConversationId::new(),
        start_ts: Local::now(),
        inputs: vec![PersistedAIInputType::Query {
            text: text.to_string(),
            context: Default::default(),
            referenced_attachments: Default::default(),
        }],
        output_status: AIQueryHistoryOutputStatus::Completed,
        working_directory: None,
        model_id: LLMId::from("test-model"),
        coding_model_id: LLMId::from("test-coding-model"),
    })
}

fn ai_query_count(conn: &mut SqliteConnection) -> i64 {
    use crate::persistence::schema::ai_queries::dsl::ai_queries;
    ai_queries
        .count()
        .first(conn)
        .expect("count query should succeed")
}

/// Returns the persisted `exchange_id`s ordered by `id` ascending (i.e. insertion / FIFO order).
fn remaining_exchange_ids(conn: &mut SqliteConnection) -> Vec<String> {
    use crate::persistence::schema::ai_queries::dsl::{ai_queries, exchange_id, id};
    ai_queries
        .select(exchange_id)
        .order(id.asc())
        .load::<String>(conn)
        .expect("load query should succeed")
}

fn input_json_for_exchange(conn: &mut SqliteConnection, exchange: &str) -> String {
    use crate::persistence::schema::ai_queries::dsl::{ai_queries, exchange_id, input};
    ai_queries
        .filter(exchange_id.eq(exchange))
        .select(input)
        .first::<String>(conn)
        .expect("row for exchange should exist")
}

#[test]
fn upsert_ai_query_caps_table_and_evicts_oldest_first() {
    let mut conn = test_connection();
    let limit = 3;

    // Insert five distinct exchanges into a table capped at three.
    let queries: Vec<Arc<PersistedAIInput>> =
        (0..5).map(|i| make_query(&format!("q{i}"))).collect();
    let exchange_ids: Vec<String> = queries.iter().map(|q| q.exchange_id.to_string()).collect();

    for query in &queries {
        upsert_ai_query_with_limit(&mut conn, query.clone(), limit).expect("upsert should succeed");
    }

    // The table never exceeds the limit.
    assert_eq!(ai_query_count(&mut conn), limit);

    // The two oldest (q0, q1) are evicted; the three newest remain in insertion order.
    assert_eq!(
        remaining_exchange_ids(&mut conn),
        exchange_ids[2..].to_vec()
    );
}

#[test]
fn upsert_ai_query_stays_below_limit_without_evicting() {
    let mut conn = test_connection();
    let limit = 3;

    // Filling exactly up to the limit should not evict anything.
    let queries: Vec<Arc<PersistedAIInput>> =
        (0..3).map(|i| make_query(&format!("q{i}"))).collect();
    let exchange_ids: Vec<String> = queries.iter().map(|q| q.exchange_id.to_string()).collect();

    for query in &queries {
        upsert_ai_query_with_limit(&mut conn, query.clone(), limit).expect("upsert should succeed");
    }

    assert_eq!(ai_query_count(&mut conn), limit);
    assert_eq!(remaining_exchange_ids(&mut conn), exchange_ids);
}

#[test]
fn upsert_ai_query_updates_existing_exchange_without_evicting() {
    let mut conn = test_connection();
    let limit = 2;

    // Fill the table to its limit with two distinct exchanges.
    let first = make_query("first");
    let second = make_query("second");
    upsert_ai_query_with_limit(&mut conn, first.clone(), limit).expect("upsert should succeed");
    upsert_ai_query_with_limit(&mut conn, second.clone(), limit).expect("upsert should succeed");
    assert_eq!(ai_query_count(&mut conn), limit);

    // Re-upsert the oldest exchange (same `exchange_id`) repeatedly. Because this is an update of
    // an existing exchange rather than a new one, it must update in place and never evict.
    let updated_first = Arc::new(PersistedAIInput {
        inputs: vec![PersistedAIInputType::Query {
            text: "first-updated".to_string(),
            context: Default::default(),
            referenced_attachments: Default::default(),
        }],
        ..(*first).clone()
    });
    for _ in 0..5 {
        upsert_ai_query_with_limit(&mut conn, updated_first.clone(), limit)
            .expect("upsert should succeed");
    }

    // Still exactly two rows, and both original exchanges survive (the oldest was not evicted).
    assert_eq!(ai_query_count(&mut conn), limit);
    assert_eq!(
        remaining_exchange_ids(&mut conn),
        vec![
            first.exchange_id.to_string(),
            second.exchange_id.to_string()
        ]
    );

    // The in-place update took effect.
    let input_json = input_json_for_exchange(&mut conn, &first.exchange_id.to_string());
    assert!(
        input_json.contains("first-updated"),
        "existing row should have been updated in place, got: {input_json}"
    );
}

#[test]
fn empty_input_skip_filters_out_non_query_inputs() {
    // Mirrors the filter in `handle_ai_history_event`: only query-bearing inputs are persisted.
    // An exchange whose inputs are all non-query types collapses to an empty `inputs` vec, which
    // is the exact condition that skips persistence.
    let user_query = AIAgentInput::UserQuery {
        query: "hello".to_string(),
        context: Default::default(),
        static_query_type: None,
        referenced_attachments: Default::default(),
        user_query_mode: UserQueryMode::default(),
        running_command: None,
        intended_agent: None,
    };
    let non_query = AIAgentInput::ResumeConversation {
        context: Default::default(),
    };

    // A query input is persistable; a non-query input is not.
    assert!(PersistedAIInputType::try_from(&user_query).is_ok());
    assert!(PersistedAIInputType::try_from(&non_query).is_err());

    // An exchange carrying only non-query inputs collapses to empty -> skipped.
    let only_non_query = [non_query];
    let persisted: Vec<_> = only_non_query
        .iter()
        .filter_map(|input| PersistedAIInputType::try_from(input).ok())
        .collect();
    assert!(persisted.is_empty());

    // An exchange carrying a query input is persisted.
    let with_query = [user_query];
    let persisted: Vec<_> = with_query
        .iter()
        .filter_map(|input| PersistedAIInputType::try_from(input).ok())
        .collect();
    assert_eq!(persisted.len(), 1);
}

/// One-history migration backfill (§3 Stage 1): dirty pre-migration data must
/// migrate without failing (a failed migration bricks persistence every boot),
/// with deterministic block-id repair and a faithful shell-turn timeline.
#[test]
fn one_history_migration_backfills_dirty_blocks() {
    use diesel::sql_query;

    let mut conn = test_connection();
    // Roll back the one_history migration so we can seed pre-migration data.
    conn.revert_last_migration(::persistence::MIGRATIONS)
        .expect("down.sql should revert the one_history migration");

    // Seed: (a) empty block_ids, (b) a fork-duplicated block_id across two
    // panes, (c) malformed ai_metadata that json_extract would choke on.
    for (pane, block_id, ai_metadata) in [
        ("p1", "", "NULL"),
        ("p1", "", "NULL"),
        ("p1", "dup-1", "'{\"conversation_id\":\"c-1\"}'"),
        ("p2", "dup-1", "NULL"),
        ("p2", "ok-2", "'this is not json'"),
    ] {
        sql_query(format!(
            "INSERT INTO blocks (pane_leaf_uuid, stylized_command, stylized_output, exit_code, \
             did_execute, is_background, honor_ps1, block_id, ai_metadata, start_ts) \
             VALUES (x'{}', x'00', x'00', 0, 1, 0, 0, '{}', {}, CURRENT_TIMESTAMP)",
            hex::encode(pane.as_bytes()),
            block_id,
            ai_metadata,
        ))
        .execute(&mut conn)
        .expect("seed insert should succeed");
    }

    // The migration must run cleanly on the dirty data.
    conn.run_pending_migrations(::persistence::MIGRATIONS)
        .expect("one_history migration should succeed on dirty data");

    use crate::persistence::schema::blocks::dsl as b;
    use crate::persistence::schema::turn_index::dsl as t;

    // Block ids are unique and non-empty.
    let ids: Vec<String> = b::blocks.select(b::block_id).load(&mut conn).unwrap();
    assert_eq!(ids.len(), 5);
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), 5, "block_ids must be unique after backfill");
    assert!(ids.iter().all(|id| !id.is_empty()));
    // The empty and duplicated rows were rewritten to legacy-<rowid>; the
    // canonical (lowest-rowid) duplicate keeps its id.
    assert!(ids.contains(&"dup-1".to_string()));
    assert!(ids.contains(&"ok-2".to_string()));
    assert_eq!(ids.iter().filter(|id| id.starts_with("legacy-")).count(), 3);

    // One shell turn row per block, seq in rowid order per pane, turn_id ==
    // block_id, and the malformed ai_metadata degraded to NULL.
    let turns: Vec<(String, Vec<u8>, i64, Option<String>)> = t::turn_index
        .select((t::turn_id, t::pane_leaf_uuid, t::seq, t::conversation_id))
        .order((t::pane_leaf_uuid.asc(), t::seq.asc()))
        .load(&mut conn)
        .unwrap();
    assert_eq!(turns.len(), 5);
    for pane in [b"p1".to_vec(), b"p2".to_vec()] {
        let seqs: Vec<i64> = turns
            .iter()
            .filter(|(_, p, _, _)| *p == pane)
            .map(|(_, _, s, _)| *s)
            .collect();
        assert_eq!(seqs, (0..seqs.len() as i64).collect::<Vec<_>>());
    }
    assert_eq!(
        turns
            .iter()
            .filter(|(_, _, _, conv)| conv.as_deref() == Some("c-1"))
            .count(),
        1,
        "the valid ai_metadata row keeps its conversation link"
    );
    let turn_ids: std::collections::HashSet<_> =
        turns.iter().map(|(id, _, _, _)| id.clone()).collect();
    assert_eq!(turn_ids, unique.into_iter().cloned().collect());

    // blocks.turn_id mirrors block_id.
    let pairs: Vec<(String, Option<String>)> = b::blocks
        .select((b::block_id, b::turn_id))
        .load(&mut conn)
        .unwrap();
    assert!(pairs
        .iter()
        .all(|(block_id, turn_id)| turn_id.as_deref() == Some(block_id.as_str())));
}
