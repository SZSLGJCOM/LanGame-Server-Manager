//! Durable, paged player history shared by player workspaces. Collection adapters
//! ingest native evidence; querying history never creates synthetic activity.
use crate::{StorageError, StoragePaths, storage_db::connect_pool};
use serde::{Deserialize, Serialize};
use sqlx::Row;

#[path = "player_history_collection.rs"]
mod collection;
#[path = "player_history_dst.rs"]
mod dst;
pub use collection::collect_player_history;

#[cfg(test)]
#[path = "player_history_tests.rs"]
mod tests;

#[derive(Debug, Deserialize)]
pub struct PlayerHistoryQuery {
    pub instance_id: String,
    pub kind: Option<String>,
    pub search: Option<String>,
    pub before_id: Option<i64>,
    pub since_unix_ms: Option<i64>,
    pub until_unix_ms: Option<i64>,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct PlayerHistoryEvent {
    pub id: i64,
    pub run_id: i64,
    pub process_key: String,
    pub kind: String,
    pub player_name: String,
    pub account_id: Option<String>,
    pub message: Option<String>,
    pub log_time: String,
    pub estimated_at_unix_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    pub session_ambiguous: bool,
}

#[derive(Debug, Serialize)]
pub struct PlayerHistoryPage {
    pub instance_id: String,
    pub supported: bool,
    pub events: Vec<PlayerHistoryEvent>,
    pub next_before_id: Option<i64>,
    pub collection_issue: Option<String>,
}

pub async fn query_player_history(
    paths: &StoragePaths,
    input: PlayerHistoryQuery,
) -> Result<PlayerHistoryPage, StorageError> {
    let pool = connect_pool(paths).await?;
    let module: Option<String> = sqlx::query_scalar("SELECT module_id FROM instances WHERE id = ?")
        .bind(&input.instance_id)
        .fetch_optional(&pool)
        .await?;
    let module = module.ok_or_else(|| StorageError::MissingInstance {
        id: input.instance_id.clone(),
    })?;
    let limit = input.limit.unwrap_or(50).clamp(1, 100) as usize;
    let search = input.search.as_deref().unwrap_or("").trim();
    // instr treats percent/underscore literally and binds all untrusted input.
    let rows = sqlx::query("SELECT * FROM player_history_events WHERE instance_id = ?1
        AND (?2 IS NULL OR kind = ?2) AND (?3 IS NULL OR id < ?3)
        AND (?4 = '' OR instr(lower(player_name), lower(?4)) > 0 OR instr(lower(COALESCE(account_id, '')), lower(?4)) > 0 OR instr(lower(COALESCE(message, '')), lower(?4)) > 0)
        AND (?5 IS NULL OR estimated_at_unix_ms >= ?5) AND (?6 IS NULL OR estimated_at_unix_ms <= ?6)
        ORDER BY id DESC LIMIT ?7")
        .bind(&input.instance_id).bind(input.kind.as_deref()).bind(input.before_id).bind(search)
        .bind(input.since_unix_ms).bind(input.until_unix_ms).bind((limit + 1) as i64).fetch_all(&pool).await?;
    let mut events = rows
        .into_iter()
        .map(|row| {
            Ok(PlayerHistoryEvent {
                id: row.try_get("id")?,
                run_id: row.try_get("run_id")?,
                process_key: row.try_get("process_key")?,
                kind: row.try_get("kind")?,
                player_name: row.try_get("player_name")?,
                account_id: row.try_get("account_id")?,
                message: row.try_get("message")?,
                log_time: row.try_get("log_time")?,
                estimated_at_unix_ms: row.try_get("estimated_at_unix_ms")?,
                duration_ms: row.try_get("duration_ms")?,
                session_ambiguous: row.try_get("session_ambiguous")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    let more = events.len() > limit;
    events.truncate(limit);
    let next_before_id = more.then(|| events.last().map(|event| event.id)).flatten();
    let collection_issue = sqlx::query_scalar::<_, String>("SELECT c.issue FROM player_history_cursors c JOIN instance_runs r ON r.id = c.run_id WHERE r.instance_id = ? AND c.issue IS NOT NULL ORDER BY c.polled_at DESC LIMIT 1")
        .bind(&input.instance_id).fetch_optional(&pool).await?;
    pool.close().await;
    Ok(PlayerHistoryPage {
        instance_id: input.instance_id,
        supported: module == "dontstarve",
        events,
        next_before_id,
        collection_issue,
    })
}
