use super::*;

#[path = "instance_archive_details_files_tests.rs"]
mod retained_files;

async fn journal(paths: &StoragePaths, id: &str) -> (String, String, i64, i64, i64) {
    let pool = connect_pool(paths).await.unwrap();
    let result = sqlx::query_as(
        "SELECT snapshot_json,state,(SELECT COUNT(*) FROM instances),(SELECT COUNT(*) FROM instance_ports),(SELECT COUNT(*) FROM instance_archives) FROM instance_archives WHERE archive_id=?1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    pool.close().await;
    result
}

async fn replace_snapshot(fixture: &Fixture, id: &str, update: impl FnOnce(&mut store::Snapshot)) {
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let mut snapshot = store::snapshot(&store::load(&pool, id).await.unwrap()).unwrap();
    update(&mut snapshot);
    let text = serde_json::to_string(&snapshot).unwrap();
    sqlx::query(
        "UPDATE instance_archives SET snapshot_json=?1,snapshot_sha256=?2 WHERE archive_id=?3",
    )
    .bind(&text)
    .bind(store::digest(text.as_bytes()))
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;
}

#[tokio::test]
async fn archive_details_preview_reads_preserved_values_without_writes_or_registration() {
    let fixture = Fixture::new().await;
    let settings = serde_json::json!({
        "motd": "Saved world 42",
        "max_players": 17,
        "password": "fixture-server-password",
        "unmodeled": { "keep": [false, 3, "original"] }
    });
    fs::write(
        fixture.instance_root.join("config/instance.json"),
        serde_json::to_vec(&serde_json::json!({"settings": settings, "autostart": true})).unwrap(),
    )
    .unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instances SET bind_ip='192.0.2.11' WHERE id='archive-instance'")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let archived = fixture.archive().await.unwrap();
    let root = Path::new(archived.archived_instance_root.as_ref().unwrap());
    let files_before = crate::test_file_snapshot::tree_snapshot(root).unwrap();
    let journal_before = journal(&fixture.paths, &archived.archive_id).await;
    // A preview must not inventory and register unrelated retained directories.
    fs::create_dir(fixture.paths.archives_root.join("unregistered-history")).unwrap();
    let preview = read_instance_archive_details(&fixture.paths, &archived.archive_id)
        .await
        .unwrap();
    assert_eq!(preview.archive_id, archived.archive_id);
    assert_eq!(preview.instance.summary.id, "archive-instance");
    assert_eq!(preview.instance.summary.name, "Preserved world");
    assert_eq!(preview.instance.summary.module_id, "archive-fixture");
    assert_eq!(preview.instance.summary.bind_ip, "192.0.2.11");
    assert!(matches!(
        preview.instance.summary.status,
        app_core::InstanceStatus::Stopped
    ));
    assert!(preview.instance.summary.autostart);
    assert_eq!(preview.instance.summary.port_count, 1);
    assert_eq!(preview.instance.summary.active_process_count, 0);
    assert!(preview.instance.active_run.is_none());
    assert_eq!(
        Path::new(&preview.instance.config_file_path),
        root.join("config/instance.json")
    );
    assert_eq!(preview.instance.saves_path, archived.effective_saves_path);
    assert!(!preview.instance.backup_uses_declared_saves_path);
    assert_eq!(preview.instance.ports.len(), 1);
    assert_eq!(preview.instance.ports[0].name, "game");
    assert_eq!(preview.instance.ports[0].protocol, "udp");
    assert_eq!(preview.instance.ports[0].port, 27981);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&preview.instance.settings_json).unwrap(),
        settings
    );
    assert_eq!(
        journal(&fixture.paths, &archived.archive_id).await,
        journal_before
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(root).unwrap(),
        files_before
    );
    assert!(!fixture.instance_root.exists());
}

#[tokio::test]
async fn archive_details_preview_reuses_normal_instance_details_contract() {
    let mut fixture = Fixture::new().await;
    fixture.paths.modules_root = fixture.root.join("modules");
    fs::create_dir_all(fixture.paths.modules_root.join("archive-fixture")).unwrap();
    fs::write(
        fixture.paths.modules_root.join("archive-fixture/module.toml"),
        "id='archive-fixture'\nname='Archive fixture'\nversion='1'\n[storage]\nsaves_path_template='{{paths.instance_root}}/saves'\n",
    )
    .unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instances SET status='error',autostart=0,auto_backup_on_stop=1,backup_retention_count=7 WHERE id='archive-instance'")
        .execute(&pool).await.unwrap();
    pool.close().await;
    let mut normal = crate::read_instance_details(&fixture.paths, "archive-instance")
        .await
        .unwrap();
    assert!(normal.backup_uses_declared_saves_path);
    assert!(matches!(
        normal.summary.status,
        app_core::InstanceStatus::Error
    ));
    let archived = fixture.archive().await.unwrap();
    let root = Path::new(archived.archived_instance_root.as_ref().unwrap());
    let before = crate::test_file_snapshot::tree_snapshot(root).unwrap();
    let details = read_instance_archive_details(&fixture.paths, &archived.archive_id)
        .await
        .unwrap();
    normal.config_file_path = root
        .join("config/instance.json")
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        serde_json::to_value(&details.instance).unwrap(),
        serde_json::to_value(normal).unwrap(),
        "The same stopped-instance fields must have the same values after archiving."
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(root).unwrap(),
        before
    );
    assert!(!fixture.instance_root.exists());
}

#[tokio::test]
async fn archive_details_preview_displays_saved_external_location_without_opening_it() {
    let fixture = Fixture::new().await;
    let external = fixture.root.join("external-world");
    let preserved = fixture.root.join("external-world-moved");
    fs::create_dir(&external).unwrap();
    fs::write(external.join("world.dat"), b"saved external data").unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instances SET saves_path=?1 WHERE id='archive-instance'")
        .bind(external.to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let archived = fixture.archive().await.unwrap();
    fs::rename(&external, &preserved).unwrap();
    let root = Path::new(archived.archived_instance_root.as_ref().unwrap());
    let before = crate::test_file_snapshot::tree_snapshot(root).unwrap();
    for use_effective_path in [true, false] {
        if !use_effective_path {
            replace_snapshot(&fixture, &archived.archive_id, |snapshot| {
                snapshot.effective_saves_path = None;
            })
            .await;
        }
        let details = read_instance_archive_details(&fixture.paths, &archived.archive_id)
            .await
            .unwrap();
        assert_eq!(Path::new(&details.instance.saves_path), external);
        assert!(!external.exists());
        assert_eq!(
            fs::read(preserved.join("world.dat")).unwrap(),
            b"saved external data"
        );
        assert_eq!(
            crate::test_file_snapshot::tree_snapshot(root).unwrap(),
            before
        );
    }
}

#[tokio::test]
async fn archive_details_preview_ignores_restore_port_and_missing_library_conflicts() {
    let mut fixture = Fixture::new().await;
    fixture.paths.modules_root = fixture.root.join("modules");
    fs::create_dir_all(fixture.paths.modules_root.join("archive-fixture")).unwrap();
    fs::write(
        fixture
            .paths
            .modules_root
            .join("archive-fixture/module.toml"),
        "id='archive-fixture'\nname='Archive fixture'\nversion='1'\n",
    )
    .unwrap();
    let descriptor = app_modules::discover_modules(&fixture.paths.modules_root)
        .unwrap()
        .remove(0);
    let library = fixture.paths.games_root.join("archive-fixture");
    fs::create_dir(&library).unwrap();
    fs::write(library.join("server.exe"), b"official program").unwrap();
    crate::record_library_program_baseline(&library, &descriptor, true, None).unwrap();
    fs::write(
        fixture.instance_root.join("runtime/server.exe"),
        b"official program",
    )
    .unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("INSERT INTO game_installs (module_id,install_root,install_state,current_version,scope) VALUES ('archive-fixture',?1,'installed','fixture-build','library')")
        .bind(library.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
    pool.close().await;
    let archived = fixture.archive().await.unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let snapshot =
        store::snapshot(&store::load(&pool, &archived.archive_id).await.unwrap()).unwrap();
    assert!(
        snapshot.program.is_some(),
        "fixture must omit genuinely rebuildable files"
    );
    let peer = fixture.paths.instances_root.join("peer");
    sqlx::query("INSERT INTO instances (id,name,module_id,data_path,config_path,logs_path,saves_path) VALUES ('peer','Peer','archive-fixture',?1,?2,?3,?4)")
        .bind(peer.join("data").to_string_lossy().as_ref()).bind(peer.join("config").to_string_lossy().as_ref())
        .bind(peer.join("logs").to_string_lossy().as_ref()).bind(peer.join("saves").to_string_lossy().as_ref()).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO instance_ports (instance_id,name,port,protocol) VALUES ('peer','game',27981,'udp')")
        .execute(&pool).await.unwrap();
    pool.close().await;
    fs::rename(&library, fixture.root.join("unavailable-library")).unwrap();
    let archives = list_instance_archives(&fixture.paths).await.unwrap();
    assert!(!archives.archives[0].can_restore);
    let before = journal(&fixture.paths, &archived.archive_id).await;
    let preview = read_instance_archive_details(&fixture.paths, &archived.archive_id)
        .await
        .unwrap();
    assert_eq!(preview.instance.ports[0].port, 27981);
    assert!(preview.instance.settings_json.contains("retained world"));
    assert_eq!(journal(&fixture.paths, &archived.archive_id).await, before);
    assert!(!fixture.instance_root.exists());
}

#[tokio::test]
async fn archive_details_preview_rejects_unknown_ids_and_incomplete_states() {
    let fixture = Fixture::new().await;
    let mut missing_database = fixture.paths.clone();
    missing_database.database_path = fixture.root.join("not-created.db");
    assert!(
        read_instance_archive_details(&missing_database, &uuid::Uuid::new_v4().to_string())
            .await
            .is_err()
    );
    assert!(
        !missing_database.database_path.exists(),
        "preview must not initialize a database"
    );
    assert!(
        read_instance_archive_details(&fixture.paths, "../config")
            .await
            .is_err()
    );
    assert!(
        read_instance_archive_details(&fixture.paths, &uuid::Uuid::new_v4().to_string())
            .await
            .is_err()
    );
    let archived = fixture.archive().await.unwrap();
    for state in [
        "archiving",
        "restoring",
        "purging",
        "restored",
        "purged",
        "missing_metadata",
        "unrecognized",
    ] {
        let pool = connect_pool(&fixture.paths).await.unwrap();
        sqlx::query("UPDATE instance_archives SET state=?1 WHERE archive_id=?2")
            .bind(state)
            .bind(&archived.archive_id)
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        let error = read_instance_archive_details(&fixture.paths, &archived.archive_id)
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Only completed instance archives"),
            "{state}: {error}"
        );
    }
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query(
        "UPDATE instance_archives SET state='archived',purpose='delete' WHERE archive_id=?1",
    )
    .bind(&archived.archive_id)
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;
    assert!(
        read_instance_archive_details(&fixture.paths, &archived.archive_id)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn archive_details_preview_rejects_snapshot_and_configuration_tampering() {
    let fixture = Fixture::new().await;
    let archived = fixture.archive().await.unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let saved = store::load(&pool, &archived.archive_id).await.unwrap();
    sqlx::query("UPDATE instance_archives SET snapshot_json=NULL WHERE archive_id=?1")
        .bind(&archived.archive_id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert!(
        read_instance_archive_details(&fixture.paths, &archived.archive_id)
            .await
            .unwrap_err()
            .to_string()
            .contains("no complete recovery metadata")
    );
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instance_archives SET snapshot_json='{}' WHERE archive_id=?1")
        .bind(&archived.archive_id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert!(
        read_instance_archive_details(&fixture.paths, &archived.archive_id)
            .await
            .unwrap_err()
            .to_string()
            .contains("checksum")
    );
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instance_archives SET snapshot_json=?1 WHERE archive_id=?2")
        .bind(saved.snapshot)
        .bind(&archived.archive_id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let path =
        Path::new(archived.archived_instance_root.as_ref().unwrap()).join("config/instance.json");
    let changed = br#"{"settings":{"motd":"external edit must remain untouched"}}"#;
    fs::write(&path, changed).unwrap();
    assert!(
        read_instance_archive_details(&fixture.paths, &archived.archive_id)
            .await
            .unwrap_err()
            .to_string()
            .contains("differs from its recovery snapshot")
    );
    assert_eq!(fs::read(&path).unwrap(), changed);
}

#[tokio::test]
async fn archive_details_preview_rejects_escaping_paths_and_changed_identity() {
    let fixture = Fixture::new().await;
    let archived = fixture.archive().await.unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let original = store::load(&pool, &archived.archive_id).await.unwrap();
    sqlx::query("UPDATE instance_archives SET archive_leaf='../escape' WHERE archive_id=?1")
        .bind(&archived.archive_id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert!(
        read_instance_archive_details(&fixture.paths, &archived.archive_id)
            .await
            .is_err()
    );
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instance_archives SET archive_leaf=?1 WHERE archive_id=?2")
        .bind(&original.leaf)
        .bind(&archived.archive_id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    replace_snapshot(&fixture, &archived.archive_id, |snapshot| {
        snapshot.tables.get_mut("instances").unwrap()[0].insert(
            "config_path".into(),
            serde_json::json!(fixture.root.join("outside/config").to_string_lossy()),
        );
    })
    .await;
    assert!(
        read_instance_archive_details(&fixture.paths, &archived.archive_id)
            .await
            .is_err()
    );
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query(
        "UPDATE instance_archives SET snapshot_json=?1,snapshot_sha256=?2 WHERE archive_id=?3",
    )
    .bind(original.snapshot)
    .bind(original.snapshot_hash)
    .bind(&archived.archive_id)
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;
    let root = Path::new(archived.archived_instance_root.as_ref().unwrap());
    fs::rename(root, fixture.root.join("retained-original-archive")).unwrap();
    fs::create_dir(root).unwrap();
    assert!(
        read_instance_archive_details(&fixture.paths, &archived.archive_id)
            .await
            .unwrap_err()
            .to_string()
            .contains("identity changed")
    );
}

#[tokio::test]
async fn archive_details_preview_bounds_configuration_and_requires_settings_object() {
    let fixture = Fixture::new().await;
    fs::write(
        fixture.instance_root.join("config/instance.json"),
        b"{\"settings\":[]}",
    )
    .unwrap();
    let archived = fixture.archive().await.unwrap();
    assert!(
        read_instance_archive_details(&fixture.paths, &archived.archive_id)
            .await
            .unwrap_err()
            .to_string()
            .contains("settings must be an object")
    );
    let path =
        Path::new(archived.archived_instance_root.as_ref().unwrap()).join("config/instance.json");
    fs::write(&path, vec![b' '; 4 * 1024 * 1024 + 1]).unwrap();
    assert!(
        read_instance_archive_details(&fixture.paths, &archived.archive_id)
            .await
            .unwrap_err()
            .to_string()
            .contains("exceeds 4 MiB")
    );
    assert_eq!(fs::metadata(path).unwrap().len(), 4 * 1024 * 1024 + 1);
}

#[cfg(windows)]
#[tokio::test]
async fn archive_details_preview_rejects_configuration_junction_escape() {
    let fixture = Fixture::new().await;
    let archived = fixture.archive().await.unwrap();
    let root = Path::new(archived.archived_instance_root.as_ref().unwrap());
    let config = root.join("config");
    let outside = fixture.root.join("outside-config");
    fs::rename(&config, &outside).unwrap();
    let created = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(config.to_string_lossy().replace('/', "\\"))
        .arg(outside.to_string_lossy().replace('/', "\\"))
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let result = read_instance_archive_details(&fixture.paths, &archived.archive_id).await;
    // Unlink the fixture-owned junction before the fixture recursively cleans up.
    fs::remove_dir(&config).unwrap();
    assert!(result.is_err());
    assert!(outside.join("instance.json").is_file());
}

#[tokio::test]
async fn archive_details_moria_projects_retained_permissions_without_changing_archive() {
    for external in [false, true] {
        let mode = if external {
            "external program payload"
        } else {
            "private runtime"
        };
        let mut fixture = Fixture::new().await;
        fixture.paths.modules_root = fixture.root.join("modules");
        let module = fixture.paths.modules_root.join("returntomoria");
        fs::create_dir_all(&module).unwrap();
        fs::write(
            module.join("module.toml"),
            "id='returntomoria'\nname='Moria fixture'\nversion='1'\n",
        )
        .unwrap();
        let pool = connect_pool(&fixture.paths).await.unwrap();
        for statement in [
            "INSERT INTO modules (id,name,version) VALUES ('returntomoria','Moria fixture','1')",
            "UPDATE instances SET module_id='returntomoria' WHERE id='archive-instance'",
            "UPDATE game_installs SET module_id='returntomoria' WHERE id=17",
            "UPDATE instance_broadcast_events SET module_id='returntomoria' WHERE instance_id='archive-instance'",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }
        pool.close().await;
        let stale = "Default = AllStorage\n";
        let retained = "\u{feff}Default = AllStorage\r\nAlice = AllStorage\r\n";
        fs::write(fixture.instance_root.join("config/instance.json"), serde_json::to_vec(
        &serde_json::json!({"settings":{"permissions_lines":stale,"world_name":"Preserved world"}})
    ).unwrap()).unwrap();
        let filename = crate::instance_native_settings::MORIA_PERMISSIONS_FILE;
        let native_root = if external {
            let library = fixture.paths.games_root.join("returntomoria");
            fs::create_dir_all(&library).unwrap();
            fs::remove_file(
                fixture
                    .instance_root
                    .join("runtime/.langame-private-runtime"),
            )
            .unwrap();
            fs::remove_dir(fixture.instance_root.join("runtime")).unwrap();
            crate::program_runtime::prepare_exclusive_program_reference(
                &library,
                &fixture.instance_root,
                "returntomoria",
            )
            .unwrap();
            crate::program_runtime::record_exclusive_program_use(&library, &fixture.instance_root)
                .unwrap();
            let pool = connect_pool(&fixture.paths).await.unwrap();
            sqlx::query("UPDATE game_installs SET scope='library',owner_instance_id=NULL,install_root=?1 WHERE id=17")
            .bind(library.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
            pool.close().await;
            library
        } else {
            fixture.instance_root.join("runtime")
        };
        fs::write(native_root.join(filename), retained).unwrap();
        let archived = fixture.archive().await.unwrap();
        let root = Path::new(archived.archived_instance_root.as_ref().unwrap());
        let before = crate::test_file_snapshot::tree_snapshot(root).unwrap();
        let journal_before = journal(&fixture.paths, &archived.archive_id).await;
        let snapshot: store::Snapshot = serde_json::from_str(&journal_before.0).unwrap();
        assert_eq!(snapshot.external_program.is_some(), external, "{mode}");
        let permissions_path = if let Some(plan) = &snapshot.external_program {
            let index = plan
                .files
                .keys()
                .position(|relative| relative == filename)
                .unwrap();
            root.join(external::PAYLOAD).join(format!("{index:06}"))
        } else {
            root.join("runtime").join(filename)
        };
        assert_eq!(
            fs::read(&permissions_path).unwrap(),
            retained.as_bytes(),
            "{mode}: native file must be retained before preview"
        );
        // The historical location may already belong to a different instance.
        fs::create_dir_all(&native_root).unwrap();
        fs::write(native_root.join(filename), "Other instance").unwrap();
        let preview = read_instance_archive_details(&fixture.paths, &archived.archive_id)
            .await
            .unwrap();
        let settings: serde_json::Value =
            serde_json::from_str(&preview.instance.settings_json).unwrap();
        assert_eq!(settings["permissions_lines"], retained, "{mode}");
        assert_eq!(settings["world_name"], "Preserved world", "{mode}");
        assert_eq!(
            crate::test_file_snapshot::tree_snapshot(root).unwrap(),
            before,
            "{mode}: preview changed archived files"
        );
        assert_eq!(
            journal(&fixture.paths, &archived.archive_id).await,
            journal_before,
            "{mode}: preview changed archive metadata"
        );

        if external {
            fs::write(&permissions_path, b"Changed archived permissions").unwrap();
            let error = read_instance_archive_details(&fixture.paths, &archived.archive_id)
                .await
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("permissions differ from their recovery snapshot"),
                "{mode}: {error}"
            );
            fs::write(&permissions_path, retained).unwrap();

            let payload = root.join(external::PAYLOAD);
            let preserved_payload = root.join("preserved-test-payload");
            fs::rename(&payload, &preserved_payload).unwrap();
            fs::create_dir(&payload).unwrap();
            let error = read_instance_archive_details(&fixture.paths, &archived.archive_id)
                .await
                .unwrap_err();
            fs::remove_dir(&payload).unwrap();
            fs::rename(&preserved_payload, &payload).unwrap();
            assert!(
                error.to_string().contains("identity changed"),
                "{mode}: {error}"
            );
        }

        // Invalid history must be rejected before consulting native permissions.
        fs::write(root.join("config/instance.json"), b"{}").unwrap();
        fs::write(&permissions_path, [0xff]).unwrap();
        let error = read_instance_archive_details(&fixture.paths, &archived.archive_id)
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("differs from its recovery snapshot"),
            "{mode}: {error}"
        );
    }
}
