use super::*;
use crate::test_file_snapshot::tree_snapshot;

#[tokio::test]
async fn curseforge_membership_survives_reload_and_rejects_invalid_or_stale_writes() {
    let root = unique_test_root();
    let mut paths = test_paths(&root);
    paths.modules_root = repo_root().join("modules");
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    let descriptor = app_modules::discover_modules(&paths.modules_root)
        .unwrap()
        .into_iter()
        .find(|entry| entry.summary.id == "arksurvivalascended")
        .unwrap();
    fs::create_dir_all(
        paths
            .games_root
            .join(&descriptor.install.as_ref().unwrap().shared_game_dir),
    )
    .unwrap();
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: "ASA membership fixture".into(),
            module_id: descriptor.summary.id.clone(),
        },
    )
    .await
    .unwrap();
    let initial = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let input_for = |settings: &Value| UpdateInstanceInput {
        id: initial.summary.id.clone(),
        bind_ip: initial.summary.bind_ip.clone(),
        auto_backup_on_stop: initial.auto_backup_on_stop,
        backup_retention_count: initial.backup_retention_count,
        settings_json: settings.to_string(),
        ports: initial.ports.clone(),
    };
    let instance_root = Path::new(&initial.config_file_path)
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let runtime = crate::resolve_instance_private_runtime_root(instance_root).unwrap();
    let payload =
        runtime.join("ShooterGame/Binaries/Win64/ShooterGame/Mods/83374/1346144/payload.sentinel");
    fs::create_dir_all(payload.parent().unwrap()).unwrap();
    fs::write(&payload, b"retained Mod payload").unwrap();
    let mut settings: Value = serde_json::from_str(&initial.settings_json).unwrap();
    settings["curseforge_disabled_mod_ids"] = json!(["1346144"]);
    settings["curseforge_disabled_passive_mod_ids"] = json!(["1346144", "1346145"]);
    settings["curseforge_removed_mod_ids"] = json!(["1346146"]);
    let saved = update_instance_if_current(&paths, input_for(&settings), &initial.settings_json)
        .await
        .unwrap();
    // A fresh storage read must preserve disabled ownership and both original modes.
    let reloaded = read_instance_details(&paths, &initial.summary.id)
        .await
        .unwrap();
    let loaded: Value = serde_json::from_str(&reloaded.settings_json).unwrap();
    for field in [
        "curseforge_disabled_mod_ids",
        "curseforge_disabled_passive_mod_ids",
        "curseforge_removed_mod_ids",
    ] {
        assert_eq!(loaded[field], settings[field]);
    }
    assert_eq!(loaded["mod_ids_csv"], json!(""));
    assert_eq!(loaded["passive_mod_ids_csv"], json!(""));
    assert_eq!(fs::read(&payload).unwrap(), b"retained Mod payload");
    let before_rejection = tree_snapshot(instance_root).unwrap();
    let mut invalid = settings.clone();
    invalid["curseforge_removed_mod_ids"] = json!(["../1346144"]);
    assert!(matches!(
        update_instance_if_current(&paths, input_for(&invalid), &saved.settings_json).await,
        Err(StorageError::InvalidModuleSetting { .. })
    ));
    assert!(
        update_instance_if_current(&paths, input_for(&settings), &initial.settings_json)
            .await
            .is_err()
    );
    assert_eq!(tree_snapshot(instance_root).unwrap(), before_rejection);
    assert_eq!(
        read_instance_details(&paths, &initial.summary.id)
            .await
            .unwrap()
            .settings_json,
        reloaded.settings_json
    );
    cleanup_root(&root);
}
