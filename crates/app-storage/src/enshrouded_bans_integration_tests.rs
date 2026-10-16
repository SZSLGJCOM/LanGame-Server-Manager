use super::*;
use crate::{ApplyInstancePlayerAccessMutationInput, PlayerAccessMutationOperation};

fn native_path(fixture: &NativeSettingsFixture) -> PathBuf {
    instance_private_runtime_root(&fixture.created).join("enshrouded_server.json")
}

fn record(id: u64) -> Value {
    json!({"accountId":id,"displayName":"native name","characterName":"character",
        "banDate":{"value":123,"unknownDate":true},"futureMetadata":{"retain":true}})
}

fn write_native(fixture: &NativeSettingsFixture, records: Value) {
    let path = native_path(fixture);
    let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    document.as_object_mut().unwrap().remove("bans");
    document["bannedAccounts"] = records;
    fs::write(path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
}

fn native_document(fixture: &NativeSettingsFixture) -> Value {
    serde_json::from_slice(&fs::read(native_path(fixture)).unwrap()).unwrap()
}

fn render_managed_template(fixture: &NativeSettingsFixture, details: &InstanceDetails) {
    let native = native_path(fixture);
    let settings: Value = serde_json::from_str(&details.settings_json).unwrap();
    crate::templates::render_module_templates(
        &fixture.paths.modules_root.join("enshrouded/templates"),
        &crate::templates::ModuleTemplateRenderInput {
            config_dir: Path::new(&details.config_file_path).parent().unwrap(),
            install_root: native.parent().unwrap(),
            saves_dir: Path::new(&details.saves_path),
            instance_id: &details.summary.id,
            instance_name: &details.summary.name,
            module_id: "enshrouded",
            bind_ip: &details.summary.bind_ip,
            autostart: details.summary.autostart,
            settings: settings.as_object().unwrap(),
            ports: &details.ports,
        },
    )
    .unwrap();
}

#[tokio::test]
async fn enshrouded_native_bans_are_projected_and_preserved_by_unrelated_save_and_start() {
    let fixture = NativeSettingsFixture::new("enshrouded").await;
    let original = fixture.details().await;
    let first = record(9007199254740993);
    write_native(&fixture, json!([first]));
    let projected = fixture.details().await;
    assert_eq!(
        serde_json::from_str::<Value>(&projected.settings_json).unwrap()["banned_player_ids"],
        "9007199254740993"
    );
    let mut draft: Value = serde_json::from_str(&original.settings_json).unwrap();
    draft["server_name"] = json!("unrelated change");
    update_instance(&fixture.paths, fixture.input(&original, draft))
        .await
        .unwrap();
    let after = native_document(&fixture);
    assert_eq!(after["bannedAccounts"], json!([first]));
    assert!(after.get("bans").is_none());
    materialize_instance_configuration_for_start(&fixture.paths, &fixture.created.summary.id)
        .await
        .unwrap();
    assert_eq!(native_document(&fixture)["bannedAccounts"], json!([first]));
}

#[tokio::test]
async fn enshrouded_player_access_removes_only_the_explicit_native_account_and_adds_exact_ids() {
    let fixture = NativeSettingsFixture::new("enshrouded").await;
    let retained = record(u64::MAX);
    write_native(&fixture, json!([record(1), retained]));
    let removed = apply_instance_player_access_mutation(
        &fixture.paths,
        ApplyInstancePlayerAccessMutationInput {
            instance_id: fixture.created.summary.id.clone(),
            field_key: "banned_player_ids".to_owned(),
            operation: PlayerAccessMutationOperation::Remove,
            value: json!("1"),
            expected_value: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&removed.details.settings_json).unwrap()["banned_player_ids"],
        u64::MAX.to_string()
    );
    assert_eq!(
        native_document(&fixture)["bannedAccounts"],
        json!([retained])
    );
    apply_instance_player_access_mutation(
        &fixture.paths,
        ApplyInstancePlayerAccessMutationInput {
            instance_id: fixture.created.summary.id.clone(),
            field_key: "banned_player_ids".to_owned(),
            operation: PlayerAccessMutationOperation::Add,
            value: json!("9007199254740993"),
            expected_value: None,
        },
    )
    .await
    .unwrap();
    let after = native_document(&fixture);
    let entries = after["bannedAccounts"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["accountId"].as_u64(), Some(9007199254740993));
    assert_eq!(entries[1], retained);
    assert!(after.get("bans").is_none());
}

#[tokio::test]
async fn enshrouded_native_and_old_managed_records_are_unioned_before_key_migration() {
    let fixture = NativeSettingsFixture::new("enshrouded").await;
    let original = fixture.details().await;
    let legacy_path = Path::new(&original.config_file_path)
        .parent()
        .unwrap()
        .join("enshrouded_server.json");
    let mut legacy: Value = serde_json::from_slice(&fs::read(&legacy_path).unwrap()).unwrap();
    legacy.as_object_mut().unwrap().remove("bannedAccounts");
    let legacy_record = json!({"accountId":1,"displayName":"","characterName":"","banDate":{"value":0},"legacyUnknown":{"keep":true}});
    legacy["bans"] = json!([legacy_record]);
    fs::write(&legacy_path, serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();
    let native_record = record(2);
    write_native(&fixture, json!([native_record]));
    let projected = fixture.details().await;
    assert_eq!(
        serde_json::from_str::<Value>(&projected.settings_json).unwrap()["banned_player_ids"],
        "1\n2"
    );
    materialize_instance_configuration_for_start(&fixture.paths, &fixture.created.summary.id)
        .await
        .unwrap();
    let after = native_document(&fixture);
    assert_eq!(
        after["bannedAccounts"],
        json!([legacy_record, native_record])
    );
    assert!(after.get("bans").is_none());
    assert!(
        legacy_path
            .parent()
            .unwrap()
            .join("native-ban-recovery")
            .is_dir()
    );
}

#[tokio::test]
async fn enshrouded_external_native_changes_during_preparation_reject_the_entire_update() {
    let fixture = NativeSettingsFixture::new("enshrouded").await;
    write_native(&fixture, json!([record(1)]));
    let baseline = fixture.details().await;
    let original = fs::read(&baseline.config_file_path).unwrap();
    let managed = Path::new(&baseline.config_file_path)
        .parent()
        .unwrap()
        .join("enshrouded_server.json");
    let managed_original = fs::read(&managed).unwrap();
    let mut draft: Value = serde_json::from_str(&baseline.settings_json).unwrap();
    draft["server_name"] = json!("must not commit");
    let mut input = fixture.input(&baseline, draft);
    input.backup_retention_count += 1;
    let paths = fixture.paths.clone();
    let mut gate = test_gate::register(&fixture.paths.database_path, Point::NativeSettingsReady);
    let worker = tokio::spawn(async move { update_instance(&paths, input).await });
    gate.reached().await;
    write_native(&fixture, json!([record(1), record(2)]));
    let external = fs::read(native_path(&fixture)).unwrap();
    gate.resume();
    assert!(matches!(
        worker.await.unwrap(),
        Err(StorageError::InstanceSettingsPreconditionFailed { .. })
    ));
    assert_eq!(fs::read(&baseline.config_file_path).unwrap(), original);
    assert_eq!(fs::read(&managed).unwrap(), managed_original);
    assert_eq!(fs::read(native_path(&fixture)).unwrap(), external);
    assert_eq!(
        fixture.details().await.backup_retention_count,
        baseline.backup_retention_count
    );
}

#[tokio::test]
async fn enshrouded_native_ban_edits_require_stopped_state() {
    let fixture = NativeSettingsFixture::new("enshrouded").await;
    for status in ["starting", "running", "stopping", "error"] {
        fixture.status(status).await;
        let before = fs::read(native_path(&fixture)).unwrap();
        let error = apply_instance_player_access_mutation(
            &fixture.paths,
            ApplyInstancePlayerAccessMutationInput {
                instance_id: fixture.created.summary.id.clone(),
                field_key: "banned_player_ids".to_owned(),
                operation: PlayerAccessMutationOperation::Add,
                value: json!("1"),
                expected_value: None,
            },
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error,StorageError::InvalidModuleSetting { field,.. } if field=="banned_player_ids")
        );
        assert_eq!(fs::read(native_path(&fixture)).unwrap(), before);
    }
}

#[tokio::test]
async fn enshrouded_uint64_mutations_are_idempotent_across_leading_zero_spellings() {
    let fixture = NativeSettingsFixture::new("enshrouded").await;
    let retained = record(u64::MAX);
    write_native(&fixture, json!([retained]));
    let request = |operation, value: &str| ApplyInstancePlayerAccessMutationInput {
        instance_id: fixture.created.summary.id.clone(),
        field_key: "banned_player_ids".to_owned(),
        operation,
        value: json!(value),
        expected_value: None,
    };
    let added = apply_instance_player_access_mutation(
        &fixture.paths,
        request(PlayerAccessMutationOperation::Add, "00123"),
    )
    .await
    .unwrap();
    assert_eq!(
        added.persistent_status,
        crate::PlayerAccessPersistentStatus::Updated
    );
    let after_add = fs::read(native_path(&fixture)).unwrap();
    for spelling in ["00123", "123"] {
        let duplicate = apply_instance_player_access_mutation(
            &fixture.paths,
            request(PlayerAccessMutationOperation::Add, spelling),
        )
        .await
        .unwrap();
        assert_eq!(
            duplicate.persistent_status,
            crate::PlayerAccessPersistentStatus::Unchanged
        );
        assert_eq!(fs::read(native_path(&fixture)).unwrap(), after_add);
    }
    let removed = apply_instance_player_access_mutation(
        &fixture.paths,
        request(PlayerAccessMutationOperation::Remove, "00123"),
    )
    .await
    .unwrap();
    assert_eq!(
        removed.persistent_status,
        crate::PlayerAccessPersistentStatus::Updated
    );
    assert_eq!(
        native_document(&fixture)["bannedAccounts"],
        json!([retained])
    );
}

#[tokio::test]
async fn enshrouded_external_managed_change_after_render_preserves_external_bytes_and_db() {
    let fixture = NativeSettingsFixture::new("enshrouded").await;
    write_native(&fixture, json!([record(1)]));
    let baseline = fixture.details().await;
    let instance_original = fs::read(&baseline.config_file_path).unwrap();
    let native_original = fs::read(native_path(&fixture)).unwrap();
    let managed = Path::new(&baseline.config_file_path)
        .parent()
        .unwrap()
        .join("enshrouded_server.json");
    let mut draft: Value = serde_json::from_str(&baseline.settings_json).unwrap();
    draft["server_name"] = json!("must not commit");
    let mut input = fixture.input(&baseline, draft);
    input.backup_retention_count += 1;
    let paths = fixture.paths.clone();
    let mut gate = test_gate::register(&fixture.paths.database_path, Point::EnshroudedBanRendered);
    let worker = tokio::spawn(async move { update_instance(&paths, input).await });
    gate.reached().await;
    let mut external: Value = serde_json::from_slice(&fs::read(&managed).unwrap()).unwrap();
    external.as_object_mut().unwrap().remove("bans");
    external["bannedAccounts"] = json!([record(1), record(2)]);
    let external_bytes = serde_json::to_vec_pretty(&external).unwrap();
    fs::write(&managed, &external_bytes).unwrap();
    gate.resume();
    let error = worker.await.unwrap().unwrap_err();
    assert!(error.to_string().contains("changed"));
    assert_eq!(
        fs::read(&baseline.config_file_path).unwrap(),
        instance_original
    );
    assert_eq!(fs::read(native_path(&fixture)).unwrap(), native_original);
    assert_eq!(fs::read(&managed).unwrap(), external_bytes);
    assert_eq!(
        fixture.details().await.backup_retention_count,
        baseline.backup_retention_count
    );
    assert!(
        !managed
            .parent()
            .unwrap()
            .join("native-ban-recovery")
            .exists()
    );
}

#[tokio::test]
async fn enshrouded_external_managed_change_after_noop_render_rolls_back_db() {
    let fixture = NativeSettingsFixture::new("enshrouded").await;
    let baseline = fixture.details().await;
    // Full-record materialization normally canonicalizes the source JSON. Put
    // this source into the exact template representation to exercise a genuine
    // no-op template write before the independent external mutation.
    render_managed_template(&fixture, &baseline);
    let instance_original = fs::read(&baseline.config_file_path).unwrap();
    let native_original = fs::read(native_path(&fixture)).unwrap();
    let managed = Path::new(&baseline.config_file_path)
        .parent()
        .unwrap()
        .join("enshrouded_server.json");
    let managed_original = fs::read(&managed).unwrap();
    let mut input = fixture.input(
        &baseline,
        serde_json::from_str(&baseline.settings_json).unwrap(),
    );
    input.backup_retention_count += 1;
    let paths = fixture.paths.clone();
    let mut gate = test_gate::register(&fixture.paths.database_path, Point::EnshroudedBanRendered);
    let worker = tokio::spawn(async move { update_instance(&paths, input).await });
    gate.reached().await;
    assert_eq!(
        fs::read(&managed).unwrap(),
        managed_original,
        "fixture must exercise an exact no-op render"
    );
    let mut external: Value = serde_json::from_slice(&managed_original).unwrap();
    external.as_object_mut().unwrap().remove("bans");
    external["bannedAccounts"] = json!([record(7)]);
    let external_bytes = serde_json::to_vec_pretty(&external).unwrap();
    fs::write(&managed, &external_bytes).unwrap();
    gate.resume();
    let error = worker.await.unwrap().unwrap_err();
    assert!(error.to_string().contains("changed"));
    assert_eq!(
        fs::read(&baseline.config_file_path).unwrap(),
        instance_original
    );
    assert_eq!(fs::read(native_path(&fixture)).unwrap(), native_original);
    assert_eq!(fs::read(&managed).unwrap(), external_bytes);
    assert_eq!(
        fixture.details().await.backup_retention_count,
        baseline.backup_retention_count
    );
    assert!(
        !managed
            .parent()
            .unwrap()
            .join("native-ban-recovery")
            .exists()
    );
}

#[tokio::test]
async fn enshrouded_managed_full_records_survive_native_typed_rewrites() {
    let fixture = NativeSettingsFixture::new("enshrouded").await;
    let original_record = record(u64::MAX);
    write_native(&fixture, json!([original_record]));
    let baseline = fixture.details().await;
    let mut draft: Value = serde_json::from_str(&baseline.settings_json).unwrap();
    draft["server_name"] = json!("preserve complete metadata");
    update_instance(&fixture.paths, fixture.input(&baseline, draft))
        .await
        .unwrap();
    let managed = Path::new(&baseline.config_file_path)
        .parent()
        .unwrap()
        .join("enshrouded_server.json");
    let stored: Value = serde_json::from_slice(&fs::read(&managed).unwrap()).unwrap();
    assert_eq!(stored["bannedAccounts"], json!([original_record]));
    assert!(stored.get("bans").is_none());

    // The actual native contract probe establishes this typed rewrite behavior.
    // This storage test substitutes only that external file-writing boundary.
    let mut native = native_document(&fixture);
    let record = native["bannedAccounts"][0].as_object_mut().unwrap();
    record.remove("futureMetadata");
    record
        .get_mut("banDate")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("unknownDate");
    fs::write(
        native_path(&fixture),
        serde_json::to_vec_pretty(&native).unwrap(),
    )
    .unwrap();
    let projected = fixture.details().await;
    assert_eq!(
        serde_json::from_str::<Value>(&projected.settings_json).unwrap()["banned_player_ids"],
        u64::MAX.to_string()
    );
    let mut unrelated: Value = serde_json::from_str(&projected.settings_json).unwrap();
    unrelated["server_name"] = json!("unrelated edit after native rewrite");
    update_instance(&fixture.paths, fixture.input(&projected, unrelated))
        .await
        .unwrap();
    let restored_managed: Value = serde_json::from_slice(&fs::read(&managed).unwrap()).unwrap();
    assert_eq!(restored_managed["bannedAccounts"], json!([original_record]));
    assert_eq!(
        native_document(&fixture)["bannedAccounts"],
        json!([original_record])
    );
}
