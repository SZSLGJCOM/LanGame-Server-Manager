use super::*;

struct EnshroudedSaveFixture {
    root: PathBuf,
    paths: StoragePaths,
    original: InstanceDetails,
    files: [PathBuf; 3],
}

impl EnshroudedSaveFixture {
    async fn new() -> Self {
        let root = unique_test_root();
        let paths = test_paths(&root);
        let descriptor = prepare_enshrouded_environment(&root);
        initialize_database(&paths).await.unwrap();
        sync_modules(&paths, std::slice::from_ref(&descriptor))
            .await
            .unwrap();
        let install_root = paths.games_root.join("enshrouded");
        fs::create_dir_all(&install_root).unwrap();
        let created = create_instance(
            &paths,
            &descriptor,
            CreateInstanceInput {
                name: "Enshrouded save rollback".to_owned(),
                module_id: "enshrouded".to_owned(),
            },
        )
        .await
        .unwrap();
        let original = read_instance_details(&paths, &created.summary.id)
            .await
            .unwrap();
        let config_path = PathBuf::from(&original.config_file_path);
        let source_path = config_path.parent().unwrap().join("enshrouded_server.json");
        let runtime_path = config_path
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("runtime/enshrouded_server.json");
        let mut runtime: Value = serde_json::from_slice(&fs::read(&runtime_path).unwrap()).unwrap();
        runtime["futureNativeOption"] = serde_json::json!({"enabled": true});
        fs::write(&runtime_path, serde_json::to_vec_pretty(&runtime).unwrap()).unwrap();
        Self {
            root,
            paths,
            original,
            files: [config_path, source_path, runtime_path],
        }
    }

    fn update_input(&self) -> UpdateInstanceInput {
        let mut settings: Value = serde_json::from_str(&self.original.settings_json).unwrap();
        settings["server_name"] = Value::String("Changed server name".to_owned());
        settings["admin_password"] = Value::String("fixture-admin-password".to_owned());
        let mut ports = self.original.ports.clone();
        ports[0].port += 100;
        UpdateInstanceInput {
            id: self.original.summary.id.clone(),
            bind_ip: "192.0.2.10".to_owned(),
            auto_backup_on_stop: !self.original.auto_backup_on_stop,
            backup_retention_count: self.original.backup_retention_count + 1,
            settings_json: serde_json::to_string(&settings).unwrap(),
            ports,
        }
    }

    fn file_snapshot(&self) -> [Vec<u8>; 3] {
        self.files.each_ref().map(|path| fs::read(path).unwrap())
    }

    async fn assert_unchanged(&self, before: &[Vec<u8>; 3]) {
        // A malformed native roster deliberately blocks the projected details
        // API. Verify persistence independently so this rollback assertion still
        // checks every DB field and every original file, including the bad JSON.
        let pool = crate::storage_db::connect_pool(&self.paths).await.unwrap();
        let record = crate::storage_db::fetch_instance_record(&pool, &self.original.summary.id)
            .await
            .unwrap();
        let ports = crate::storage_db::load_instance_ports(&pool, &self.original.summary.id)
            .await
            .unwrap();
        pool.close().await;
        assert_eq!(record.summary.bind_ip, self.original.summary.bind_ip);
        assert_eq!(record.summary.autostart, self.original.summary.autostart);
        assert_eq!(
            record.auto_backup_on_stop,
            self.original.auto_backup_on_stop
        );
        assert_eq!(
            record.backup_retention_count,
            self.original.backup_retention_count
        );
        assert_eq!(
            serde_json::to_value(&ports).unwrap(),
            serde_json::to_value(&self.original.ports).unwrap()
        );
        let persisted: Value = serde_json::from_slice(&fs::read(&self.files[0]).unwrap()).unwrap();
        let expected_settings: Value = serde_json::from_str(&self.original.settings_json).unwrap();
        assert!(
            persisted["settings"] == expected_settings,
            "failed save changed persisted settings"
        );
        for (path, expected) in self.files.iter().zip(before) {
            assert!(
                fs::read(path).unwrap() == *expected,
                "failed save changed {}",
                path.file_name().unwrap().to_string_lossy()
            );
        }
    }
}

impl Drop for EnshroudedSaveFixture {
    fn drop(&mut self) {
        cleanup_root(&self.root);
    }
}

#[tokio::test]
async fn enshrouded_1_0_settings_validate_before_writes_and_render_native_values() {
    let fixture = EnshroudedSaveFixture::new().await;
    let before = fixture.file_snapshot();
    for invalid in [
        serde_json::json!(-1),
        serde_json::json!(0),
        serde_json::json!(12),
        serde_json::json!(20),
        serde_json::json!(21),
        serde_json::json!(10.5),
        serde_json::json!("11"),
    ] {
        let mut input = fixture.update_input();
        let mut settings: Value = serde_json::from_str(&input.settings_json).unwrap();
        settings["flame_level_fog_removal"] = invalid;
        input.settings_json = settings.to_string();
        assert!(matches!(
            update_instance(&fixture.paths, input).await,
            Err(StorageError::InvalidModuleSetting { field, .. }) if field == "flame_level_fog_removal"
        ));
        fixture.assert_unchanged(&before).await;
    }
    // Existing instances created before 1.0 do not store the new setting.
    let mut omitted = fixture.update_input();
    let mut previous: Value = serde_json::from_str(&omitted.settings_json).unwrap();
    previous
        .as_object_mut()
        .unwrap()
        .remove("flame_level_fog_removal");
    omitted.settings_json = previous.to_string();
    update_instance(&fixture.paths, omitted).await.unwrap();
    materialize_instance_configuration_for_start(&fixture.paths, &fixture.original.summary.id)
        .await
        .unwrap();
    let native: Value = serde_json::from_slice(&fs::read(&fixture.files[2]).unwrap()).unwrap();
    assert_eq!(native["gameSettings"]["flameLevelFogRemoval"], 11);
    for level in [1, 11] {
        let mut input = fixture.update_input();
        let mut settings: Value = serde_json::from_str(&input.settings_json).unwrap();
        settings["game_settings_preset"] = serde_json::json!("Custom");
        settings["random_spawner_amount"] = serde_json::json!("Automatic");
        settings["flame_level_fog_removal"] = serde_json::json!(level);
        input.settings_json = settings.to_string();
        let saved = update_instance(&fixture.paths, input).await.unwrap();
        let persisted: Value = serde_json::from_str(&saved.settings_json).unwrap();
        assert_eq!(persisted["flame_level_fog_removal"], level);
        for path in &fixture.files[1..] {
            let native: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
            assert_eq!(native["gameSettings"]["flameLevelFogRemoval"], level);
            assert_eq!(native["gameSettings"]["randomSpawnerAmount"], "Automatic");
            assert_eq!(native["gameSettings"]["aggroPoolAmount"], "Normal");
        }
        let runtime: Value = serde_json::from_slice(&fs::read(&fixture.files[2]).unwrap()).unwrap();
        assert_eq!(runtime["futureNativeOption"]["enabled"], true);
    }
}

#[tokio::test]
async fn enshrouded_invalid_account_hash_save_keeps_files_and_database_unchanged() {
    let fixture = EnshroudedSaveFixture::new().await;
    let before = fixture.file_snapshot();
    for invalid in [
        serde_json::json!("76561198000000001,invalid,18446744073709551615"),
        serde_json::json!("18446744073709551616"),
        serde_json::json!(9007199254740993_u64),
    ] {
        let mut input = fixture.update_input();
        let mut settings: Value = serde_json::from_str(&input.settings_json).unwrap();
        settings["banned_player_ids"] = invalid;
        input.settings_json = settings.to_string();
        let error = update_instance(&fixture.paths, input).await.unwrap_err();
        assert!(
            matches!(error, StorageError::InvalidModuleSetting { field, .. }
            if field == "banned_player_ids")
        );
        fixture.assert_unchanged(&before).await;
    }
}

#[tokio::test]
async fn enshrouded_account_hash_save_and_prestart_keep_exact_ids() {
    let fixture = EnshroudedSaveFixture::new().await;
    let hashes = "0,9007199254740993\n76561198000000001\n18446744073709551615";
    let mut input = fixture.update_input();
    let mut settings: Value = serde_json::from_str(&input.settings_json).unwrap();
    settings["banned_player_ids"] = Value::String(hashes.to_owned());
    input.settings_json = settings.to_string();
    update_instance(&fixture.paths, input).await.unwrap();
    materialize_instance_configuration_for_start(&fixture.paths, &fixture.original.summary.id)
        .await
        .unwrap();
    let loaded = read_instance_details(&fixture.paths, &fixture.original.summary.id)
        .await
        .unwrap();
    let persisted: Value = serde_json::from_str(&loaded.settings_json).unwrap();
    assert_eq!(persisted["banned_player_ids"], hashes.replace(',', "\n"));
    for path in &fixture.files[1..] {
        let native: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        let ids: Vec<_> = native["bannedAccounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["accountId"].as_u64().unwrap())
            .collect();
        assert_eq!(ids, [0, 9007199254740993, 76561198000000001, u64::MAX]);
        assert_eq!(
            native["bannedAccounts"][3]["banDate"],
            serde_json::json!({"value": 0})
        );
    }
}

#[tokio::test]
async fn enshrouded_update_config_write_failure_keeps_files_and_database_unchanged() {
    let fixture = EnshroudedSaveFixture::new().await;
    let before = fixture.file_snapshot();
    crate::atomic_file::fail_next_atomic_write_for_test(&fixture.files[0]);
    let error = update_instance(&fixture.paths, fixture.update_input())
        .await
        .expect_err("injected instance config write failure must abort the save");
    assert!(matches!(error, StorageError::WriteConfig { .. }));
    fixture.assert_unchanged(&before).await;
}

#[tokio::test]
async fn enshrouded_update_invalid_runtime_keeps_files_and_database_unchanged() {
    let fixture = EnshroudedSaveFixture::new().await;
    fs::write(&fixture.files[2], b"{").unwrap();
    let before = fixture.file_snapshot();
    let error = update_instance(&fixture.paths, fixture.update_input())
        .await
        .expect_err("invalid operator JSON must abort the save before changing any file");
    assert!(matches!(
        error,
        StorageError::ModuleSupportMaterialization { .. }
    ));
    assert!(matches!(
        read_instance_details(&fixture.paths, &fixture.original.summary.id).await,
        Err(StorageError::ModuleSupportMaterialization { .. })
    ));
    fixture.assert_unchanged(&before).await;
}

#[tokio::test]
async fn enshrouded_update_commit_failure_keeps_files_and_database_unchanged() {
    let fixture = EnshroudedSaveFixture::new().await;
    let before = fixture.file_snapshot();
    let pool = crate::storage_db::connect_pool(&fixture.paths)
        .await
        .unwrap();
    for statement in [
        "CREATE TABLE fixture_commit_parent (id INTEGER PRIMARY KEY)",
        "CREATE TABLE fixture_commit_child (parent_id INTEGER REFERENCES fixture_commit_parent(id) DEFERRABLE INITIALLY DEFERRED)",
        "CREATE TRIGGER fixture_reject_save_commit AFTER UPDATE OF bind_ip ON instances BEGIN INSERT INTO fixture_commit_child VALUES (1); END",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }
    pool.close().await;

    let error = update_instance(&fixture.paths, fixture.update_input())
        .await
        .expect_err("deferred foreign key violation must reject the database commit");
    assert!(matches!(error, StorageError::Sqlx(_)));
    fixture.assert_unchanged(&before).await;
}

#[tokio::test]
async fn enshrouded_update_success_keeps_custom_roles_and_unknown_runtime_members() {
    let fixture = EnshroudedSaveFixture::new().await;
    let mut input = fixture.update_input();
    let mut settings: Value = serde_json::from_str(&input.settings_json).unwrap();
    settings["custom_user_groups_json"] = Value::String(
        serde_json::json!({
            "name": "Helper",
            "password": "",
            "canKickBan": false,
            "canEditWorld": false,
            "futureRoleOption": {"enabled": true}
        })
        .to_string(),
    );
    input.settings_json = serde_json::to_string(&settings).unwrap();
    let saved = update_instance(&fixture.paths, input).await.unwrap();
    assert_eq!(saved.summary.bind_ip, "192.0.2.10");
    for path in &fixture.files[1..] {
        let native: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(native["name"], "Changed server name");
        assert!(native["userGroups"][0]["password"] == "fixture-admin-password");
        assert_eq!(native["userGroups"][4]["name"], "Helper");
        assert!(native["userGroups"][4]["password"] == "");
        assert_eq!(native["userGroups"][4]["futureRoleOption"]["enabled"], true);
    }
    let runtime: Value = serde_json::from_slice(&fs::read(&fixture.files[2]).unwrap()).unwrap();
    assert_eq!(runtime["futureNativeOption"]["enabled"], true);
    let persisted: Value = serde_json::from_slice(&fs::read(&fixture.files[0]).unwrap()).unwrap();
    assert!(persisted["settings"]["admin_password"] == "fixture-admin-password");
}

#[tokio::test]
async fn enshrouded_update_invalid_public_permissions_keeps_files_and_database_unchanged() {
    let fixture = EnshroudedSaveFixture::new().await;
    let before = fixture.file_snapshot();
    let mut input = fixture.update_input();
    let mut settings: Value = serde_json::from_str(&input.settings_json).unwrap();
    settings["custom_user_groups_json"] = Value::String(
        serde_json::json!({
            "name": "Public role",
            "password": "",
            "canKickBan": false,
            "canAccessInventories": true,
            "canEditWorld": true,
            "canEditBase": true,
            "canExtendBase": true,
            "reservedSlots": 0
        })
        .to_string(),
    );
    input.settings_json = serde_json::to_string(&settings).unwrap();
    let error = update_instance(&fixture.paths, input)
        .await
        .expect_err("invalid public permissions must fail before saving files or settings");
    assert!(matches!(
        error,
        StorageError::InvalidModuleSetting { field, .. }
            if field == "custom_user_groups_json[0].password"
    ));
    fixture.assert_unchanged(&before).await;
}

#[tokio::test]
async fn enshrouded_materialize_invalid_runtime_keeps_source_and_database_unchanged() {
    let fixture = EnshroudedSaveFixture::new().await;
    fs::write(
        &fixture.files[1],
        b"{\"name\":\"previous rendered source\"}",
    )
    .unwrap();
    fs::write(&fixture.files[2], b"{ invalid native configuration").unwrap();
    let before = fixture.file_snapshot();
    for for_start in [false, true] {
        let result = if for_start {
            materialize_instance_configuration_for_start(
                &fixture.paths,
                &fixture.original.summary.id,
            )
            .await
        } else {
            materialize_instance_configuration(&fixture.paths, &fixture.original.summary.id).await
        };
        assert!(matches!(
            result,
            Err(StorageError::ModuleSupportMaterialization { .. })
        ));
        assert!(matches!(
            read_instance_details(&fixture.paths, &fixture.original.summary.id).await,
            Err(StorageError::ModuleSupportMaterialization { .. })
        ));
        fixture.assert_unchanged(&before).await;
    }
}
