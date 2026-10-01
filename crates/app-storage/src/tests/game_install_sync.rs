#[tokio::test]
async fn creation_preserves_the_library_record_and_registers_exclusive_instance_ownership() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let install_root = paths.games_root.join("dontstarve");
    fs::create_dir_all(&install_root).unwrap();
    fs::write(install_root.join("program.bin"), b"downloaded program").unwrap();
    sync_game_installs(
        &paths,
        &[GameInstallSyncRecord {
            module_id: String::from("dontstarve"),
            install_root: install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some(String::from("test-build")),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();

    let modules = sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    assert!(matches!(modules[0].install_state, InstallState::Installed));
    let library_before = crate::read_library_program_install(&paths, "dontstarve")
        .await.unwrap().unwrap();
    let library_files = crate::test_file_snapshot::tree_snapshot(&install_root).unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("DST Linked"),
            module_id: String::from("dontstarve"),
        },
    )
    .await
    .unwrap();

    let pool = connect_pool(&paths).await.unwrap();
    let install_id =
        sqlx::query_scalar::<_, Option<i64>>("SELECT install_id FROM instances WHERE id = ?1")
            .bind(&created.summary.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(install_id.is_some());

    let owned = crate::read_instance_program_install(&paths, &created.summary.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(owned.runtime_mode, "independent");
    assert_eq!(owned.install.scope, crate::ProgramInstallScope::Instance);
    assert_eq!(
        owned.install.owner_instance_id.as_deref(),
        Some(created.summary.id.as_str())
    );
    assert_eq!(Some(owned.install.id), install_id);
    assert_eq!(
        owned.install.install_root,
        Path::new(&created.config_file_path)
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("runtime")
    );
    assert_eq!(owned.install.current_version.as_deref(), Some("test-build"));
    assert_eq!(owned.install.install_state, InstallState::Installed);
    assert_ne!(owned.install.id, library_before.id);
    assert_eq!(fs::read(owned.install.install_root.join("program.bin")).unwrap(), b"downloaded program");
    let library_after = crate::read_library_program_install(&paths, "dontstarve")
        .await.unwrap().unwrap();
    assert_eq!(library_after.id, library_before.id);
    assert_eq!(library_after.current_version, library_before.current_version);
    assert_eq!(library_after.install_root, install_root);
    assert_eq!(library_after.install_state, InstallState::Installed);
    assert_eq!(library_after.scope, crate::ProgramInstallScope::Library);
    assert_eq!(library_after.owner_instance_id, None);
    assert_eq!(crate::test_file_snapshot::tree_snapshot(&install_root).unwrap(), library_files);

    let install_state = sqlx::query_scalar::<_, String>(
        "SELECT install_state FROM game_installs WHERE module_id = ?1 ORDER BY id DESC LIMIT 1",
    )
    .bind("dontstarve")
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(install_state, "installed");

    pool.close().await;
    cleanup_root(&root);
}

#[tokio::test]
async fn sync_game_install_path_aliases_keep_one_owner_and_verification_history() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, &[descriptor]).await.unwrap();
    let install_root = paths.games_root.join("dontstarve");
    fs::create_dir_all(&install_root).unwrap();
    let mut record = GameInstallSyncRecord {
        module_id: "dontstarve".into(),
        install_root: install_root.to_string_lossy().into_owned(),
        install_state: InstallState::Installed,
        current_version: Some("before".into()),
        mark_verified: true,
    };
    sync_game_installs(&paths, &[record.clone()]).await.unwrap();
    let before = crate::read_program_install_owner(&paths, &install_root)
        .await
        .unwrap()
        .unwrap();
    let verified =
        read_game_install_last_verified_unix_ms(&paths, "dontstarve", &record.install_root)
            .await
            .unwrap()
            .unwrap();
    let aliases = vec![install_root.join(".").to_string_lossy().into_owned()];
    #[cfg(windows)]
    let aliases = {
        let mut aliases = aliases;
        aliases.push(
            fs::canonicalize(&install_root)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        );
        aliases.push(record.install_root.to_uppercase().replace('\\', "/"));
        aliases
    };
    for alias in aliases {
        record.install_root = alias;
        record.current_version = Some("after".into());
        record.mark_verified = false;
        sync_game_installs(&paths, &[record.clone()]).await.unwrap();
        let after = crate::read_program_install_owner(&paths, &install_root)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after.id, before.id);
        assert_eq!(after.current_version.as_deref(), Some("after"));
        assert_eq!(
            read_game_install_last_verified_unix_ms(&paths, "dontstarve", &record.install_root,)
                .await
                .unwrap(),
            Some(verified)
        );
    }
    let pool = connect_pool(&paths).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM game_installs")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    pool.close().await;
    cleanup_root(&root);
}

#[tokio::test]
async fn sync_game_install_path_aliases_are_atomic_in_batches_and_concurrent_scans() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, &[descriptor]).await.unwrap();
    let install_root = paths.games_root.join("dontstarve");
    fs::create_dir_all(&install_root).unwrap();
    let first = GameInstallSyncRecord {
        module_id: "dontstarve".into(),
        install_root: install_root.to_string_lossy().into_owned(),
        install_state: InstallState::Installed,
        current_version: Some("current".into()),
        mark_verified: true,
    };
    let second = GameInstallSyncRecord {
        install_root: install_root.join(".").to_string_lossy().into_owned(),
        ..first.clone()
    };
    let first_batch = [first.clone()];
    let second_batch = [second.clone()];
    let (left, right) = tokio::join!(
        sync_game_installs(&paths, &first_batch),
        sync_game_installs(&paths, &second_batch),
    );
    left.unwrap();
    right.unwrap();
    let owner = crate::read_program_install_owner(&paths, &install_root)
        .await
        .unwrap()
        .unwrap();
    sync_game_installs(&paths, &[first, second]).await.unwrap();
    let after = crate::read_program_install_owner(&paths, &install_root)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.id, owner.id);
    let pool = connect_pool(&paths).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM game_installs")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    pool.close().await;
    cleanup_root(&root);
}

#[tokio::test]
async fn sync_game_install_path_aliases_reject_foreign_or_ambiguous_ownership() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);
    initialize_database(&paths).await.unwrap();
    let mut other = descriptor.clone();
    other.summary.id = "other".into();
    sync_modules(&paths, &[descriptor, other]).await.unwrap();
    let install_root = paths.games_root.join("dontstarve");
    fs::create_dir_all(&install_root).unwrap();
    let mut record = GameInstallSyncRecord {
        module_id: "dontstarve".into(),
        install_root: install_root.to_string_lossy().into_owned(),
        install_state: InstallState::Installed,
        current_version: Some("before".into()),
        mark_verified: true,
    };
    sync_game_installs(&paths, &[record.clone()]).await.unwrap();
    record.module_id = "other".into();
    record.install_root = install_root.join(".").to_string_lossy().into_owned();
    record.current_version = Some("must-not-write".into());
    let error = sync_game_installs(&paths, &[record.clone()])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("belongs to another module"));
    let original = crate::read_program_install_owner(&paths, &install_root)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(original.current_version.as_deref(), Some("before"));
    let pool = connect_pool(&paths).await.unwrap();
    sqlx::query("INSERT INTO game_installs (module_id,install_root,install_state,current_version) VALUES ('dontstarve',?1,'installed','duplicate')")
        .bind(&record.install_root).execute(&pool).await.unwrap();
    record.module_id = "dontstarve".into();
    let error = sync_game_installs(&paths, &[record]).await.unwrap_err();
    assert!(error.to_string().contains("multiple installation records"));
    assert!(
        crate::read_program_install_owner(&paths, &install_root)
            .await
            .is_err()
    );
    let versions =
        sqlx::query_scalar::<_, String>("SELECT current_version FROM game_installs ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(versions, ["before", "duplicate"]);
    pool.close().await;
    cleanup_root(&root);
}

#[tokio::test]
async fn read_game_install_last_verified_unix_ms_returns_verified_install_timestamp() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let install_root = paths.games_root.join("dontstarve");
    fs::create_dir_all(&install_root).unwrap();
    let install_root_text = install_root.to_string_lossy().into_owned();
    sync_game_installs(
        &paths,
        &[GameInstallSyncRecord {
            module_id: String::from("dontstarve"),
            install_root: install_root_text.clone(),
            install_state: InstallState::Installed,
            current_version: Some(String::from("test-build")),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();

    let last_verified_ms =
        read_game_install_last_verified_unix_ms(&paths, "dontstarve", &install_root_text)
            .await
            .unwrap();

    assert!(last_verified_ms.is_some());

    cleanup_root(&root);
}

#[tokio::test]
async fn read_game_install_last_verified_unix_ms_ignores_unverified_installs() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let install_root = paths.games_root.join("dontstarve");
    fs::create_dir_all(&install_root).unwrap();
    let install_root_text = install_root.to_string_lossy().into_owned();
    sync_game_installs(
        &paths,
        &[GameInstallSyncRecord {
            module_id: String::from("dontstarve"),
            install_root: install_root_text.clone(),
            install_state: InstallState::Installed,
            current_version: Some(String::from("test-build")),
            mark_verified: false,
        }],
    )
    .await
    .unwrap();

    let last_verified_ms =
        read_game_install_last_verified_unix_ms(&paths, "dontstarve", &install_root_text)
            .await
            .unwrap();

    assert_eq!(last_verified_ms, None);

    cleanup_root(&root);
}

#[tokio::test]
async fn sync_game_install_never_backfills_or_rebinds_existing_instance_links() {
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
            name: String::from("DST Independent"),
            module_id: String::from("dontstarve"),
        },
    )
    .await
    .unwrap();

    let pool = connect_pool(&paths).await.unwrap();
    let install_id_before =
        sqlx::query_scalar::<_, Option<i64>>("SELECT install_id FROM instances WHERE id = ?1")
            .bind(&created.summary.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(install_id_before.is_some());
    let owned_root_before = crate::read_instance_program_install(&paths, &created.summary.id)
        .await
        .unwrap()
        .unwrap()
        .install
        .install_root;
    sqlx::query("INSERT INTO instances (id,name,module_id,config_path,data_path,logs_path,saves_path) VALUES ('unlinked','Unlinked','dontstarve',?1,?2,?3,?4)")
        .bind(paths.instances_root.join("unlinked/config").to_string_lossy().as_ref())
        .bind(paths.instances_root.join("unlinked/data").to_string_lossy().as_ref())
        .bind(paths.instances_root.join("unlinked/logs").to_string_lossy().as_ref())
        .bind(paths.instances_root.join("unlinked/saves").to_string_lossy().as_ref())
        .execute(&pool).await.unwrap();
    pool.close().await;

    let install_root = paths.games_root.join("dontstarve");
    fs::create_dir_all(&install_root).unwrap();
    sync_game_installs(
        &paths,
        &[GameInstallSyncRecord {
            module_id: String::from("dontstarve"),
            install_root: install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some(String::from("test-build")),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();

    let pool = connect_pool(&paths).await.unwrap();
    let install_id_after =
        sqlx::query_scalar::<_, Option<i64>>("SELECT install_id FROM instances WHERE id = ?1")
            .bind(&created.summary.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let latest_install_id = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM game_installs WHERE module_id = ?1 AND scope = 'library' ORDER BY id DESC LIMIT 1",
    )
    .bind("dontstarve")
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(install_id_after, install_id_before);
    assert_ne!(install_id_after, Some(latest_install_id));
    assert_eq!(
        sqlx::query_scalar::<_, Option<i64>>(
            "SELECT install_id FROM instances WHERE id = 'unlinked'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        None
    );
    let owned = crate::read_instance_program_install(&paths, &created.summary.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(owned.install.scope, crate::ProgramInstallScope::Instance);
    assert_eq!(owned.install.install_root, owned_root_before);

    pool.close().await;
    cleanup_root(&root);
}
#[tokio::test]
async fn missing_library_does_not_clear_a_working_instances_own_installation() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = test_descriptor(&root);
    prepare_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let install_root = paths.games_root.join("dontstarve");
    fs::create_dir_all(&install_root).unwrap();
    fs::write(install_root.join("program.bin"), b"working private program").unwrap();
    sync_game_installs(
        &paths,
        &[GameInstallSyncRecord {
            module_id: String::from("dontstarve"),
            install_root: install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some(String::from("test-build")),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();

    let library_before = crate::read_library_program_install(&paths, "dontstarve")
        .await.unwrap().unwrap();
    let library_files = crate::test_file_snapshot::tree_snapshot(&install_root).unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("DST Detached"),
            module_id: String::from("dontstarve"),
        },
    )
    .await
    .unwrap();

    let pool = connect_pool(&paths).await.unwrap();
    let install_id_before =
        sqlx::query_scalar::<_, Option<i64>>("SELECT install_id FROM instances WHERE id = ?1")
            .bind(&created.summary.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(install_id_before.is_some());
    pool.close().await;

    let private_root = Path::new(&created.config_file_path).parent().unwrap().parent().unwrap();
    let private_files = crate::test_file_snapshot::tree_snapshot(private_root).unwrap();
    assert_eq!(crate::test_file_snapshot::tree_snapshot(&install_root).unwrap(), library_files);
    let retained_library = root.join("retained-library");
    fs::rename(&install_root, &retained_library).unwrap();
    assert!(!install_root.exists());

    sync_game_installs(
        &paths,
        &[GameInstallSyncRecord {
            module_id: String::from("dontstarve"),
            install_root: install_root.to_string_lossy().into_owned(),
            install_state: InstallState::NotInstalled,
            current_version: Some(String::from("test-build")),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();

    let modules = sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    assert!(matches!(modules[0].install_state, InstallState::Installed));

    let pool = connect_pool(&paths).await.unwrap();
    let install_id_after =
        sqlx::query_scalar::<_, Option<i64>>("SELECT install_id FROM instances WHERE id = ?1")
            .bind(&created.summary.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(install_id_after, install_id_before);
    let owned = crate::read_instance_program_install(&paths, &created.summary.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(owned.install.scope, crate::ProgramInstallScope::Instance);
    assert_eq!(
        owned.install.owner_instance_id.as_deref(),
        Some(created.summary.id.as_str())
    );
    assert_eq!(owned.install.current_version.as_deref(), Some("test-build"));
    assert!(matches!(
        owned.install.install_state,
        InstallState::Installed
    ));
    assert!(owned.install.install_root.is_dir());
    assert_eq!(fs::read(owned.install.install_root.join("program.bin")).unwrap(), b"working private program");
    let details = read_instance_details(&paths, &created.summary.id).await.unwrap();
    assert_eq!(details.summary.id, created.summary.id);
    assert_eq!(crate::test_file_snapshot::tree_snapshot(private_root).unwrap(), private_files);
    let library_after = crate::read_library_program_install(&paths, "dontstarve")
        .await.unwrap().unwrap();
    assert_eq!(library_after.id, library_before.id);
    assert_eq!(library_after.current_version, library_before.current_version);
    assert_eq!(library_after.install_root, install_root);
    assert_eq!(library_after.install_state, InstallState::NotInstalled);
    assert_eq!(library_after.scope, crate::ProgramInstallScope::Library);
    assert_eq!(library_after.owner_instance_id, None);
    assert_ne!(owned.install.id, library_after.id);
    assert_eq!(crate::test_file_snapshot::tree_snapshot(&retained_library).unwrap(), library_files);
    assert_eq!(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM game_installs WHERE module_id = 'dontstarve' AND scope = 'library'")
        .fetch_one(&pool).await.unwrap(), 1);

    pool.close().await;
    cleanup_root(&root);
}
