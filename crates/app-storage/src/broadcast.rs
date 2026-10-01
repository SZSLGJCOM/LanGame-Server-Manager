use super::*;
use crate::storage_db::connect_pool;

pub async fn read_instance_broadcast_policy(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<InstanceBroadcastPolicy, StorageError> {
    let pool = connect_pool(paths).await?;
    let row = sqlx::query(
        r#"
        SELECT instance_id, enabled, rules_json, updated_at_unix_ms
        FROM instance_broadcast_policies
        WHERE instance_id = ?1
        "#,
    )
    .bind(instance_id)
    .fetch_optional(&pool)
    .await?;

    let policy = match row {
        Some(row) => broadcast_policy_from_row(row)?,
        None => InstanceBroadcastPolicy {
            instance_id: instance_id.to_string(),
            enabled: false,
            rules: InstanceBroadcastRules::default(),
            updated_at_unix_ms: 0,
        },
    };

    pool.close().await;
    Ok(policy)
}

pub async fn upsert_instance_broadcast_policy(
    paths: &StoragePaths,
    input: UpdateInstanceBroadcastPolicyInput,
) -> Result<InstanceBroadcastPolicy, StorageError> {
    let pool = connect_pool(paths).await?;
    let updated_at_unix_ms = current_unix_ms();
    let rules_json = serde_json::to_string(&input.rules)?;

    sqlx::query(
        r#"
        INSERT INTO instance_broadcast_policies (
            instance_id, enabled, rules_json, updated_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT(instance_id) DO UPDATE SET
            enabled = excluded.enabled,
            rules_json = excluded.rules_json,
            updated_at_unix_ms = excluded.updated_at_unix_ms
        "#,
    )
    .bind(&input.instance_id)
    .bind(input.enabled)
    .bind(&rules_json)
    .bind(updated_at_unix_ms as i64)
    .execute(&pool)
    .await?;

    pool.close().await;
    Ok(InstanceBroadcastPolicy {
        instance_id: input.instance_id,
        enabled: input.enabled,
        rules: input.rules,
        updated_at_unix_ms,
    })
}

pub async fn insert_instance_broadcast_event(
    paths: &StoragePaths,
    input: InsertInstanceBroadcastEventInput,
) -> Result<InstanceBroadcastEvent, StorageError> {
    let pool = connect_pool(paths).await?;
    let created_at_unix_ms = current_unix_ms();
    let event_id = format!("broadcast-{}", current_unix_nanos());

    sqlx::query(
        r#"
        INSERT INTO instance_broadcast_events (
            event_id, instance_id, module_id, source, rule_id, message, ai_provider,
            ai_model, action_id, transport, command_preview, status, response_text,
            error_message, initiator, policy_snapshot_json, created_at_unix_ms
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
        "#,
    )
    .bind(&event_id)
    .bind(&input.instance_id)
    .bind(&input.module_id)
    .bind(&input.source)
    .bind(&input.rule_id)
    .bind(&input.message)
    .bind(&input.ai_provider)
    .bind(&input.ai_model)
    .bind(&input.action_id)
    .bind(&input.transport)
    .bind(&input.command_preview)
    .bind(&input.status)
    .bind(&input.response_text)
    .bind(&input.error_message)
    .bind(&input.initiator)
    .bind(&input.policy_snapshot_json)
    .bind(created_at_unix_ms as i64)
    .execute(&pool)
    .await?;

    pool.close().await;
    Ok(InstanceBroadcastEvent {
        event_id,
        instance_id: input.instance_id,
        module_id: input.module_id,
        source: input.source,
        rule_id: input.rule_id,
        message: input.message,
        ai_provider: input.ai_provider,
        ai_model: input.ai_model,
        action_id: input.action_id,
        transport: input.transport,
        command_preview: input.command_preview,
        status: input.status,
        response_text: input.response_text,
        error_message: input.error_message,
        initiator: input.initiator,
        policy_snapshot_json: input.policy_snapshot_json,
        created_at_unix_ms,
    })
}

pub async fn list_instance_broadcast_events(
    paths: &StoragePaths,
    instance_id: &str,
    limit: usize,
) -> Result<Vec<InstanceBroadcastEvent>, StorageError> {
    let pool = connect_pool(paths).await?;
    let limit = limit.clamp(1, 200) as i64;
    let rows = sqlx::query(
        r#"
        SELECT event_id, instance_id, module_id, source, rule_id, message, ai_provider,
               ai_model, action_id, transport, command_preview, status, response_text,
               error_message, initiator, policy_snapshot_json, created_at_unix_ms
        FROM instance_broadcast_events
        WHERE instance_id = ?1
        ORDER BY created_at_unix_ms DESC
        LIMIT ?2
        "#,
    )
    .bind(instance_id)
    .bind(limit)
    .fetch_all(&pool)
    .await?;

    let events = rows
        .into_iter()
        .map(broadcast_event_from_row)
        .collect::<Result<Vec<_>, _>>()?;

    pool.close().await;
    Ok(events)
}

fn broadcast_policy_from_row(row: SqliteRow) -> Result<InstanceBroadcastPolicy, StorageError> {
    let rules_json: String = row.get("rules_json");
    Ok(InstanceBroadcastPolicy {
        instance_id: row.get("instance_id"),
        enabled: row.get::<i64, _>("enabled") != 0,
        rules: serde_json::from_str(&rules_json)?,
        updated_at_unix_ms: row.get::<i64, _>("updated_at_unix_ms") as u64,
    })
}

fn broadcast_event_from_row(row: SqliteRow) -> Result<InstanceBroadcastEvent, StorageError> {
    Ok(InstanceBroadcastEvent {
        event_id: row.get("event_id"),
        instance_id: row.get("instance_id"),
        module_id: row.get("module_id"),
        source: row.get("source"),
        rule_id: row.get("rule_id"),
        message: row.get("message"),
        ai_provider: row.get("ai_provider"),
        ai_model: row.get("ai_model"),
        action_id: row.get("action_id"),
        transport: row.get("transport"),
        command_preview: row.get("command_preview"),
        status: row.get("status"),
        response_text: row.get("response_text"),
        error_message: row.get("error_message"),
        initiator: row.get("initiator"),
        policy_snapshot_json: row.get("policy_snapshot_json"),
        created_at_unix_ms: row.get::<i64, _>("created_at_unix_ms") as u64,
    })
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn current_unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}
