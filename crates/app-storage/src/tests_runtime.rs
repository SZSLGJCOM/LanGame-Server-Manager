use super::*;

#[tokio::test]
async fn dontstarve_guided_world_edits_round_trip_with_optimistic_saves_and_disabled_caves() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("DST synchronized world settings"),
            module_id: String::from("dontstarve"),
        },
    )
    .await
    .unwrap();
    let raw = "return {override_enabled=true,preset='SURVIVAL_TOGETHER',overrides={world_size='default',custom_note='preserved'}}";
    let caves = "return {override_enabled=true,preset='DST_CAVE',overrides={world_size='small'}}";
    let mut input = UpdateInstanceInput {
        id: created.summary.id.clone(), bind_ip: String::from("0.0.0.0"),
        auto_backup_on_stop: false, backup_retention_count: 3, ports: created.ports.clone(),
        settings_json: json!({"enable_caves":false,"master_worldgenoverride_lua":raw,"caves_worldgenoverride_lua":caves,"master_ocean_bullkelp":"ocean_default"}).to_string(),
    };
    let mut details = update_instance(&paths, input.clone()).await.unwrap();
    for (field, value) in [
        ("master_day", "onlynight"),
        ("master_day", "default"),
        ("cluster_description", "saved again"),
    ] {
        let mut incoming: Value = serde_json::from_str(&details.settings_json).unwrap();
        incoming[field] = json!(value);
        input.settings_json = incoming.to_string();
        details = update_instance_if_current(&paths, input.clone(), &details.settings_json)
            .await
            .unwrap();
        let read_back = read_instance_details(&paths, &created.summary.id)
            .await
            .unwrap();
        assert_eq!(read_back.settings_json, details.settings_json);
        let saved: Value = serde_json::from_str(&read_back.settings_json).unwrap();
        assert_eq!(saved[field], value);
        assert_eq!(saved["enable_caves"], false);
        assert_eq!(saved["caves_worldgenoverride_lua"], caves);
        assert!(
            saved["master_worldgenoverride_lua"]
                .as_str()
                .unwrap()
                .contains("custom_note='preserved'")
        );
        let output = fs::read_to_string(
            Path::new(&details.config_file_path)
                .parent()
                .unwrap()
                .join("Master/worldgenoverride.lua"),
        )
        .unwrap();
        assert_eq!(
            output.trim(),
            saved["master_worldgenoverride_lua"]
                .as_str()
                .unwrap()
                .trim()
        );
    }
    cleanup_root(&root);
}

#[tokio::test]
async fn dontstarve_master_worldgenoverride_raw_lua_replaces_generated_master_worldgen() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("DST Raw Master Worldgen"),
            module_id: String::from("dontstarve"),
        },
    )
    .await
    .unwrap();

    let raw_master_worldgen = "return {\n  override_enabled = true,\n  preset = \"SURVIVAL_TOGETHER\",\n  overrides = {\n    world_size = \"huge\",\n    layout_mode = \"LinkNodesByKeys\",\n    task_set = \"classic\",\n  }\n}\n";

    update_instance(
        &paths,
        UpdateInstanceInput {
            id: created.summary.id.clone(),
            bind_ip: String::from("0.0.0.0"),
            auto_backup_on_stop: false,
            backup_retention_count: 3,
            settings_json: serde_json::json!({
                "cluster_name": "DST Raw Master Worldgen",
                "master_world_size": "small",
                "master_worldgenoverride_lua": raw_master_worldgen
            })
            .to_string(),
            ports: created.ports.clone(),
        },
    )
    .await
    .unwrap();

    let master_worldgen_path = root
        .join("instances")
        .join(&created.summary.id)
        .join("config")
        .join("Master")
        .join("worldgenoverride.lua");
    assert_eq!(
        fs::read_to_string(&master_worldgen_path).unwrap(),
        raw_master_worldgen
    );
    assert_file_has_no_utf8_bom(&master_worldgen_path);

    cleanup_root(&root);
}

#[tokio::test]
async fn dontstarve_enabled_workshop_mods_are_added_to_instance_download_setup() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("DST Enabled Mods Download"),
            module_id: String::from("dontstarve"),
        },
    )
    .await
    .unwrap();

    update_instance(
        &paths,
        UpdateInstanceInput {
            id: created.summary.id.clone(),
            bind_ip: String::from("0.0.0.0"),
            auto_backup_on_stop: false,
            backup_retention_count: 3,
            settings_json: serde_json::json!({
                "cluster_name": "DST Enabled Mods Download",
                "shared_workshop_mod_ids": "",
                "shared_workshop_collection_ids": "",
                "master_enabled_workshop_mod_ids": "2039181790",
                "caves_enabled_workshop_mod_ids": "1909182187"
            })
            .to_string(),
            ports: created.ports.clone(),
        },
    )
    .await
    .unwrap();

    let setup_path =
        instance_private_runtime_root(&created).join("mods/dedicated_server_mods_setup.lua");
    let setup_text = fs::read_to_string(setup_path).unwrap();
    assert!(setup_text.contains("ServerModSetup(\"2039181790\")"));
    assert!(setup_text.contains("ServerModSetup(\"1909182187\")"));
    assert_eq!(setup_text.matches("ServerModSetup(").count(), 2);
    assert!(!setup_text.contains("ServerModCollectionSetup("));
    assert!(
        !paths
            .games_root
            .join("dontstarve/mods/dedicated_server_mods_setup.lua")
            .exists()
    );

    cleanup_root(&root);
}

#[tokio::test]
async fn dontstarve_backup_and_restore_follow_the_declared_cluster_path() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("DST Legacy"),
            module_id: String::from("dontstarve"),
        },
    )
    .await
    .unwrap();

    let instance_root = root.join("instances").join(&created.summary.id);
    let effective_saves_root = instance_root.join("config").join("clusters").join("main");
    let legacy_saves_root = instance_root.join("saves");
    let world_file = effective_saves_root.join("Master").join("saveindex");
    fs::create_dir_all(world_file.parent().unwrap()).unwrap();
    fs::write(&world_file, "legacy-world").unwrap();
    fs::create_dir_all(&legacy_saves_root).unwrap();

    let pool = connect_pool(&paths).await.unwrap();
    sqlx::query(
        r#"
            UPDATE instances
            SET saves_path = ?2
            WHERE id = ?1
            "#,
    )
    .bind(&created.summary.id)
    .bind(legacy_saves_root.to_string_lossy().into_owned())
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    let details = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(PathBuf::from(&details.saves_path), effective_saves_root);

    let backup = create_instance_backup(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(PathBuf::from(&backup.saves_path), effective_saves_root);
    assert_eq!(
        fs::read_to_string(
            PathBuf::from(&backup.backup_path)
                .join("saves")
                .join("Master")
                .join("saveindex"),
        )
        .unwrap(),
        "legacy-world"
    );

    fs::write(&world_file, "mutated-world").unwrap();

    let restored = restore_instance_backup(&paths, &created.summary.id, &backup.backup_id)
        .await
        .unwrap();
    assert_eq!(PathBuf::from(&restored.saves_path), effective_saves_root);
    assert_eq!(fs::read_to_string(&world_file).unwrap(), "legacy-world");
    assert!(
        PathBuf::from(&restored.safeguard_backup_path)
            .join("saves")
            .join("Master")
            .join("saveindex")
            .is_file()
    );

    cleanup_root(&root);
}

#[tokio::test]
async fn missing_module_keeps_existing_saves_through_updates_materialization_and_backup() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("DST module recovery"),
            module_id: String::from("dontstarve"),
        },
    )
    .await
    .unwrap();
    let original = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let saves_dir = PathBuf::from(&original.saves_path);
    fs::create_dir_all(saves_dir.join("Master")).unwrap();
    fs::write(saves_dir.join("Master/saveindex"), "preserved-world").unwrap();
    fs::rename(
        paths.modules_root.join("dontstarve"),
        root.join("unavailable-module"),
    )
    .unwrap();
    write_module_descriptor_files(&declared_save_path_test_descriptor(&root));

    let updated = update_instance(
        &paths,
        UpdateInstanceInput {
            id: created.summary.id.clone(),
            bind_ip: original.summary.bind_ip.clone(),
            auto_backup_on_stop: false,
            backup_retention_count: 10,
            settings_json: original.settings_json,
            ports: original.ports,
        },
    )
    .await
    .unwrap();
    assert_eq!(PathBuf::from(updated.saves_path), saves_dir);
    materialize_instance_configuration(&paths, &created.summary.id)
        .await
        .unwrap();
    let materialized = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(PathBuf::from(materialized.saves_path), saves_dir);
    let backup = create_instance_backup(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(PathBuf::from(backup.saves_path), saves_dir);
    assert_eq!(
        fs::read_to_string(PathBuf::from(backup.backup_path).join("saves/Master/saveindex"))
            .unwrap(),
        "preserved-world"
    );
    cleanup_root(&root);
}

#[tokio::test]
async fn declared_save_path_template_drives_instance_details_and_backups() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = declared_save_path_test_descriptor(&root);
    prepare_declared_save_path_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Declared Save Root"),
            module_id: String::from("savepathtest"),
        },
    )
    .await
    .unwrap();

    let expected_saves_root = root
        .join("instances")
        .join(&created.summary.id)
        .join("config")
        .join("savegame");
    let world_file = expected_saves_root.join("slot1").join("world.dat");
    fs::create_dir_all(world_file.parent().unwrap()).unwrap();
    fs::write(&world_file, "declared-save-root").unwrap();

    let details = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(PathBuf::from(&details.saves_path), expected_saves_root);
    assert!(details.backup_uses_declared_saves_path);

    let backup = create_instance_backup(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(PathBuf::from(&backup.saves_path), expected_saves_root);
    assert_eq!(
        fs::read_to_string(
            PathBuf::from(&backup.backup_path)
                .join("saves")
                .join("slot1")
                .join("world.dat"),
        )
        .unwrap(),
        "declared-save-root"
    );

    fs::write(&world_file, "mutated-world").unwrap();

    let restored = restore_instance_backup(&paths, &created.summary.id, &backup.backup_id)
        .await
        .unwrap();
    assert_eq!(PathBuf::from(&restored.saves_path), expected_saves_root);
    assert_eq!(
        fs::read_to_string(&world_file).unwrap(),
        "declared-save-root"
    );

    cleanup_root(&root);
}

#[tokio::test]
async fn backup_can_store_display_name_and_be_deleted() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = declared_save_path_test_descriptor(&root);
    prepare_declared_save_path_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Named Backup Root"),
            module_id: String::from("savepathtest"),
        },
    )
    .await
    .unwrap();

    let expected_saves_root = root
        .join("instances")
        .join(&created.summary.id)
        .join("config")
        .join("savegame");
    let world_file = expected_saves_root.join("slot1").join("world.dat");
    fs::create_dir_all(world_file.parent().unwrap()).unwrap();
    fs::write(&world_file, "named-save-root").unwrap();

    let backup = create_instance_backup(&paths, &created.summary.id)
        .await
        .unwrap();
    assert!(backup.display_name.is_none());

    let renamed = rename_instance_backup(
        &paths,
        &created.summary.id,
        &backup.backup_id,
        Some(String::from("Friday world before mods")),
    )
    .await
    .unwrap();
    assert_eq!(renamed.backup_id, backup.backup_id);
    assert_eq!(
        renamed.display_name.as_deref(),
        Some("Friday world before mods")
    );
    assert_eq!(PathBuf::from(&renamed.saves_path), expected_saves_root);

    let listed = list_instance_backups(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0].display_name.as_deref(),
        Some("Friday world before mods")
    );

    let manifest_text =
        fs::read_to_string(PathBuf::from(&backup.backup_path).join("backup.json")).unwrap();
    assert!(manifest_text.contains("\"display_name\": \"Friday world before mods\""));

    let cleared = rename_instance_backup(
        &paths,
        &created.summary.id,
        &backup.backup_id,
        Some(String::from("   ")),
    )
    .await
    .unwrap();
    assert!(cleared.display_name.is_none());

    let deleted = delete_instance_backup(&paths, &created.summary.id, &backup.backup_id)
        .await
        .unwrap();
    assert_eq!(deleted.backup_id, backup.backup_id);
    assert!(!PathBuf::from(&deleted.backup_path).exists());
    assert!(
        list_instance_backups(&paths, &created.summary.id)
            .await
            .unwrap()
            .is_empty()
    );

    cleanup_root(&root);
}

#[tokio::test]
async fn multi_process_session_keeps_current_session_context() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("DST Dual"),
            module_id: String::from("dontstarve"),
        },
    )
    .await
    .unwrap();

    let logs_dir = root.join("logs");
    fs::create_dir_all(&logs_dir).unwrap();
    let master_log_path = logs_dir.join("run-master.log");
    let caves_log_path = logs_dir.join("run-caves.log");
    fs::write(
        &master_log_path,
        "master boot
master world
master ready
",
    )
    .unwrap();
    fs::write(
        &caves_log_path,
        "caves boot
caves sync
caves ready
",
    )
    .unwrap();

    let master_identity = ProcessIdentity {
        creation_time: 1_786_620_000,
        image_path: String::from(r"c:\servers\dontstarve_dedicated_server_nullrenderer.exe"),
    };
    let master = mark_instance_process_started_with_identity(
        &paths,
        &StartedInstanceProcess {
            instance_id: &created.summary.id,
            session_id: Some("session-dual"),
            process_key: "master",
            display_name: "Master",
            pid: 7001,
            log_path: &master_log_path.to_string_lossy(),
            is_primary: true,
        },
        Some(&master_identity),
    )
    .await
    .unwrap();
    let caves = mark_instance_process_started_with_identity(
        &paths,
        &StartedInstanceProcess {
            instance_id: &created.summary.id,
            session_id: Some("session-dual"),
            process_key: "caves",
            display_name: "Caves",
            pid: 7002,
            log_path: &caves_log_path.to_string_lossy(),
            is_primary: false,
        },
        None,
    )
    .await
    .unwrap();

    let active_before_error = read_active_instance_run(&paths, &created.summary.id)
        .await
        .unwrap()
        .expect("active run before caves error");
    assert_eq!(active_before_error.process_count, 2);
    assert_eq!(master.process_identity.as_ref(), Some(&master_identity));
    assert_eq!(
        active_before_error.processes[0].process_identity.as_ref(),
        Some(&master_identity)
    );
    let active_entries = list_active_instance_runs(&paths).await.unwrap();
    assert!(active_entries.iter().any(|entry| {
        entry.run_id == master.run_id && entry.process_identity.as_ref() == Some(&master_identity)
    }));
    assert_eq!(
        active_before_error.session_id.as_deref(),
        Some("session-dual")
    );

    let master_document =
        read_instance_log_document(&paths, &created.summary.id, 10, Some(master.run_id))
            .await
            .unwrap();
    assert_eq!(
        master_document.source_path,
        Some(master_log_path.to_string_lossy().into_owned())
    );
    assert!(
        master_document
            .lines
            .iter()
            .any(|line| line.contains("master ready"))
    );

    let caves_document =
        read_instance_log_document(&paths, &created.summary.id, 10, Some(caves.run_id))
            .await
            .unwrap();
    assert_eq!(
        caves_document.source_path,
        Some(caves_log_path.to_string_lossy().into_owned())
    );
    assert!(
        caves_document
            .lines
            .iter()
            .any(|line| line.contains("caves ready"))
    );

    let degraded =
        mark_instance_process_stopped(&paths, &created.summary.id, caves.run_id, Some(17), true)
            .await
            .unwrap();
    assert!(matches!(degraded.status, InstanceStatus::Error));
    assert_eq!(degraded.active_process_count, 1);
    let degraded_details = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(degraded_details.summary.active_process_count, 1);
    let degraded_list_entry = list_instances(&paths)
        .await
        .unwrap()
        .into_iter()
        .find(|instance| instance.id == created.summary.id)
        .expect("listed degraded instance");
    assert_eq!(degraded_list_entry.active_process_count, 1);

    let active_after_error = read_active_instance_run(&paths, &created.summary.id)
        .await
        .unwrap()
        .expect("active run after caves error");
    assert_eq!(active_after_error.run_id, master.run_id);
    assert_eq!(active_after_error.process_count, 2);
    assert!(
        active_after_error
            .processes
            .iter()
            .any(|process| process.process_key == "master" && process.status == "running")
    );
    assert!(
        active_after_error
            .processes
            .iter()
            .any(|process| process.process_key == "caves" && process.status == "error")
    );

    let overview = read_instance_runtime_overview(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(overview.recent_runs[0].status, "error");
    assert_eq!(overview.recent_runs[0].process_count, 2);

    let final_summary =
        mark_instance_process_stopped(&paths, &created.summary.id, master.run_id, Some(0), false)
            .await
            .unwrap();
    assert!(matches!(final_summary.status, InstanceStatus::Error));
    assert_eq!(final_summary.active_process_count, 0);
    assert!(
        read_active_instance_run(&paths, &created.summary.id)
            .await
            .unwrap()
            .is_none()
    );

    cleanup_root(&root);
}

#[tokio::test]
async fn initialize_database_refuses_untracked_existing_database_without_modifying_it() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();

    let options = SqliteConnectOptions::new()
        .filename(&paths.database_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(30));
    let legacy_pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();

    legacy_pool
        .execute(
            r#"
                CREATE TABLE modules (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    version TEXT NOT NULL,
                    description TEXT
                )
                "#,
        )
        .await
        .unwrap();
    legacy_pool
        .execute("PRAGMA user_version = 2;")
        .await
        .unwrap();
    legacy_pool
        .execute(
            "INSERT INTO modules (id, name, version, description) \
             VALUES ('sentinel', 'Sentinel', '1', 'must survive')",
        )
        .await
        .unwrap();
    legacy_pool.close().await;
    let before = fs::read(&paths.database_path).unwrap();

    let error = initialize_database(&paths).await.unwrap_err();
    assert!(matches!(
        error,
        StorageError::IncompatibleMigrationHistory { .. }
    ));
    assert!(paths.database_path.is_file());
    assert_eq!(fs::read(&paths.database_path).unwrap(), before);

    let backups_root = paths.database_path.parent().unwrap().join("backups");
    assert!(!backups_root.exists());

    let verification_pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&paths.database_path)
                .journal_mode(SqliteJournalMode::Wal)
                .foreign_keys(true),
        )
        .await
        .unwrap();
    let sentinel: String =
        sqlx::query_scalar("SELECT description FROM modules WHERE id='sentinel'")
            .fetch_one(&verification_pool)
            .await
            .unwrap();
    assert_eq!(sentinel, "must survive");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA user_version")
            .fetch_one(&verification_pool)
            .await
            .unwrap(),
        2
    );
    verification_pool.close().await;

    cleanup_root(&root);
}

#[tokio::test]
async fn initialize_database_refuses_schema_mismatch_without_modifying_it() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    let options = SqliteConnectOptions::new()
        .filename(&paths.database_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(30));
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    MIGRATOR.run(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO modules (id, name, version, supported_platforms, root_path) \
         VALUES ('sentinel', 'Sentinel', '1', 'windows', 'must survive')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("ALTER TABLE modules RENAME COLUMN root_path TO unexpected_root_path")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let before = fs::read(&paths.database_path).unwrap();

    let error = initialize_database(&paths).await.unwrap_err();
    assert!(matches!(error, StorageError::SchemaMismatch { .. }));
    assert!(paths.database_path.is_file());
    assert_eq!(fs::read(&paths.database_path).unwrap(), before);
    assert!(
        !paths
            .database_path
            .parent()
            .unwrap()
            .join("backups")
            .exists()
    );

    let verification_pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&paths.database_path)
                .journal_mode(SqliteJournalMode::Wal)
                .foreign_keys(true),
        )
        .await
        .unwrap();
    let sentinel: String =
        sqlx::query_scalar("SELECT unexpected_root_path FROM modules WHERE id='sentinel'")
            .fetch_one(&verification_pool)
            .await
            .unwrap();
    assert_eq!(sentinel, "must survive");
    verification_pool.close().await;

    cleanup_root(&root);
}
