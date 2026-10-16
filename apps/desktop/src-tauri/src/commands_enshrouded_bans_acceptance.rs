use super::*;
use std::collections::BTreeMap;

const ORIGINAL_IDS: [u64; 3] = [4_242, 9_007_199_254_740_993, u64::MAX];
const ADDED_ID: u64 = 42;

fn original_records() -> Value {
    json!([
        {"accountId": ORIGINAL_IDS[0], "displayName": "旧格式名单", "characterName": "守护者",
            "banDate": {"value": 1_700_000_001i64}, "operatorNote": {"retained": true}},
        {"accountId": ORIGINAL_IDS[1], "displayName": "既有玩家甲", "characterName": "营地守护者",
            "banDate": {"value": 1_700_000_002i64}, "operatorNote": {"retained": true}},
        {"accountId": ORIGINAL_IDS[2], "displayName": "Existing B", "characterName": "Synthetic B",
            "banDate": {"value": 1_700_000_003i64}, "operatorNote": {"retained": true}}
    ])
}

pub(super) fn assert_roster(
    runtime: &Path,
    added: bool,
    retain_unknown: bool,
) -> ProbeResult<Value> {
    let config: Value = serde_json::from_slice(&fs::read(runtime.join("enshrouded_server.json"))?)?;
    if config.get("bans").is_some() {
        return Err("legacy ban key must be absent after materialization".into());
    }
    let records = config["bannedAccounts"]
        .as_array()
        .ok_or("canonical bans must be an array")?;
    let actual = records
        .iter()
        .map(|entry| entry["accountId"].as_u64().map(|id| (id, entry)))
        .collect::<Option<BTreeMap<_, _>>>()
        .ok_or("canonical bans require exact uint64 accountId")?;
    let mut expected = ORIGINAL_IDS.to_vec();
    if added {
        expected.push(ADDED_ID);
    }
    expected.sort_unstable();
    if records.len() != expected.len() || actual.keys().copied().collect::<Vec<_>>() != expected {
        return Err("native roster lost, added, rounded or duplicated an identity".into());
    }
    for original in original_records()
        .as_array()
        .ok_or("missing synthetic originals")?
    {
        let record = actual[&original["accountId"]
            .as_u64()
            .ok_or("missing synthetic id")?];
        for field in ["displayName", "characterName", "banDate"] {
            if record[field] != original[field] {
                return Err(format!("native ban metadata changed: {field}").into());
            }
        }
        if retain_unknown && record["operatorNote"] != original["operatorNote"] {
            return Err("manager erased unknown native ban metadata".into());
        }
    }
    Ok(config["bannedAccounts"].clone())
}

async fn mutate(
    paths: &app_storage::StoragePaths,
    id: &str,
    operation: app_storage::PlayerAccessMutationOperation,
) -> ProbeResult<app_storage::PlayerAccessPersistentMutationResult> {
    Ok(app_storage::apply_instance_player_access_mutation(
        paths,
        app_storage::ApplyInstancePlayerAccessMutationInput {
            instance_id: id.into(),
            field_key: "banned_player_ids".into(),
            operation,
            value: json!(ADDED_ID.to_string()),
            expected_value: None,
        },
    )
    .await?)
}

pub(super) async fn seed_and_exercise(
    paths: &app_storage::StoragePaths,
    details: &InstanceDetails,
    runtime: &Path,
    evidence: &Path,
) -> ProbeResult<InstanceDetails> {
    let path = runtime.join("enshrouded_server.json");
    let mut config: Value = serde_json::from_slice(&fs::read(&path)?)?;
    let records = original_records();
    config["bans"] = json!([records[0].clone()]);
    config["bannedAccounts"] = json!([records[1].clone(), records[2].clone()]);
    let before = serde_json::to_vec_pretty(&config)?;
    fs::write(&path, &before)?;
    // Ordinary Configuration submits its stale ID-only projection. It cannot unban.
    let mut stale: Value = serde_json::from_str(&details.settings_json)?;
    stale["banned_player_ids"] = json!("");
    let updated = app_storage::update_instance(
        paths,
        UpdateInstanceInput {
            id: details.summary.id.clone(),
            bind_ip: "127.0.0.1".into(),
            auto_backup_on_stop: false,
            backup_retention_count: 12,
            settings_json: serde_json::to_string(&stale)?,
            ports: details.ports.clone(),
        },
    )
    .await?;
    assert_roster(runtime, false, true)?;
    let recovery = creation::instance_root(&updated)?.join("config/native-ban-recovery");
    let exact_copy = fs::read_dir(recovery)?.filter_map(Result::ok).any(|entry| {
        fs::read(entry.path().join("native-before.json")).is_ok_and(|bytes| bytes == before)
    });
    if !exact_copy {
        return Err("alias migration did not retain an exact recoverable native config".into());
    }
    app_storage::materialize_instance_configuration_for_start(paths, &details.summary.id).await?;
    assert_roster(runtime, false, true)?;
    let added = mutate(
        paths,
        &details.summary.id,
        app_storage::PlayerAccessMutationOperation::Add,
    )
    .await?;
    if added.persistent_status != app_storage::PlayerAccessPersistentStatus::Updated {
        return Err("new player ban was not persisted".into());
    }
    let repeated = mutate(
        paths,
        &details.summary.id,
        app_storage::PlayerAccessMutationOperation::Add,
    )
    .await?;
    if repeated.persistent_status != app_storage::PlayerAccessPersistentStatus::Unchanged {
        return Err("repeated player ban must be idempotent".into());
    }
    let roster = assert_roster(runtime, true, true)?;
    write_new(
        &evidence.join("manager-ban-migration.json"),
        &serde_json::to_vec_pretty(&json!({
            "synthetic_identities_only": true, "ordinary_save_preserved_existing": true,
            "prestart_preserved_existing": true, "legacy_and_native_union": true,
            "exact_recovery_copy": true, "repeated_add_idempotent": true, "roster": roster
        }))?,
    )?;
    Ok(repeated.details)
}

pub(super) async fn remove_added(
    paths: &app_storage::StoragePaths,
    details: &InstanceDetails,
    runtime: &Path,
    evidence: &Path,
) -> ProbeResult<InstanceDetails> {
    let removed = mutate(
        paths,
        &details.summary.id,
        app_storage::PlayerAccessMutationOperation::Remove,
    )
    .await?;
    if removed.persistent_status != app_storage::PlayerAccessPersistentStatus::Updated {
        return Err("explicit target unban was not persisted".into());
    }
    let roster = assert_roster(runtime, false, false)?;
    write_new(
        &evidence.join("manager-target-unban.json"),
        &serde_json::to_vec_pretty(&json!({
            "synthetic_identities_only": true, "removed_only_added_target": ADDED_ID, "roster": roster
        }))?,
    )?;
    Ok(removed.details)
}
