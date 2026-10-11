use super::*;

#[tokio::test]
async fn asa_mod_configuration_storage_cas_restart_and_running_guard() {
    let mut fixture = MapFixture::new("arksurvivalascended").await;
    let initial_json = fixture.details.settings_json.clone();
    let mut settings = fixture.settings();
    settings["mod_ids_csv"] = json!("1254808");
    settings["asa_mod_configurations"] =
        json!({"1254808":{"game_user_settings_ini":"[BetterVariants]\nDebug=True\n"}});
    fixture.details =
        update_instance_if_current(&fixture.paths, fixture.input(&settings), &initial_json)
            .await
            .unwrap();
    fixture.details = read_instance_details(&fixture.paths, &fixture.created.summary.id)
        .await
        .unwrap();
    assert_eq!(
        fixture.settings()["asa_mod_configurations"],
        settings["asa_mod_configurations"]
    );
    let config = PathBuf::from(&fixture.created.config_file_path);
    let before = fs::read(&config).unwrap();
    assert!(
        update_instance_if_current(&fixture.paths, fixture.input(&settings), &initial_json)
            .await
            .is_err()
    );
    let mut invalid = settings.clone();
    invalid["asa_mod_configurations"]["1254808"]["game_user_settings_ini"] = json!("Debug=False");
    assert!(matches!(
        update_instance(&fixture.paths, fixture.input(&invalid)).await,
        Err(StorageError::InvalidModuleSetting { .. })
    ));
    assert_eq!(fs::read(&config).unwrap(), before);
    materialize_instance_configuration_for_start(&fixture.paths, &fixture.created.summary.id)
        .await
        .unwrap();
    let live_ini = instance_private_runtime_root(&fixture.created)
        .join("ShooterGame/Saved/Config/WindowsServer/GameUserSettings.ini");
    assert!(
        fs::read_to_string(&live_ini)
            .unwrap()
            .contains("Debug=True")
    );

    let run_log = fixture.root.join("mod-config-running.log");
    fs::write(&run_log, b"synthetic registered process, not executed").unwrap();
    let run = record_started_test_instance(
        &fixture.paths,
        &fixture.created.summary.id,
        9_999_997,
        &run_log.to_string_lossy(),
    )
    .await
    .unwrap();
    // Starting legitimately regenerates instance.json (including its timestamp).
    // The rejected edit must preserve the state immediately before that edit.
    let before_running_edit = fs::read(&config).unwrap();
    let mut changed = settings.clone();
    changed["asa_mod_configurations"]["1254808"]["game_user_settings_ini"] =
        json!("[BetterVariants]\nDebug=False");
    assert!(matches!(
        update_instance(&fixture.paths, fixture.input(&changed)).await,
        Err(StorageError::InvalidModuleSetting { .. })
    ));
    assert_eq!(fs::read(&config).unwrap(), before_running_edit);
    assert!(
        fs::read_to_string(&live_ini)
            .unwrap()
            .contains("Debug=True")
    );
    mark_instance_process_stopped(
        &fixture.paths,
        &fixture.created.summary.id,
        run.run_id,
        Some(0),
        false,
    )
    .await
    .unwrap();
}
