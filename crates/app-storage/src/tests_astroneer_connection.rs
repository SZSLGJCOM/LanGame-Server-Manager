use super::*;

#[tokio::test]
async fn astroneer_legacy_connection_mode_survives_first_render_and_subsequent_save() {
    let root = unique_test_root();
    let mut paths = test_paths(&root);
    paths.modules_root = repo_root().join("modules");
    let descriptor = app_modules::discover_modules(&paths.modules_root)
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "astroneer")
        .unwrap();
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let install = root.join("verified-installs").join("astroneer");
    fs::create_dir_all(&install).unwrap();
    fs::write(install.join("AstroServer.exe"), b"inert fixture").unwrap();
    sync_game_installs(
        &paths,
        &[GameInstallSyncRecord {
            module_id: "astroneer".into(),
            install_root: install.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some("fixture".into()),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();
    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: "Legacy connection fixture".into(),
            module_id: "astroneer".into(),
        },
    )
    .await
    .unwrap();
    let details = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let defaults: Value = serde_json::from_str(&details.settings_json).unwrap();
    assert_eq!(defaults["connection_mode"], "public");
    assert_eq!(defaults["public_ip"], "");
    let config_path = Path::new(&details.config_file_path);
    let baseline: Value = serde_json::from_str(&fs::read_to_string(config_path).unwrap()).unwrap();
    let update = |details: &InstanceDetails| UpdateInstanceInput {
        id: details.summary.id.clone(),
        bind_ip: details.summary.bind_ip.clone(),
        auto_backup_on_stop: details.auto_backup_on_stop,
        backup_retention_count: details.backup_retention_count,
        settings_json: details.settings_json.clone(),
        ports: details.ports.clone(),
    };
    for (mode, address, expected_mode) in [
        (None, "", "public"),
        (None, "26.1.2.3", "manual"),
        (None, "1.1.1.1", "manual"),
        (Some("public"), "26.1.2.3", "public"),
        (Some("radmin"), "26.1.2.3", "radmin"),
        (Some("radmin"), "", "radmin"),
        (Some("manual"), "1.1.1.1", "manual"),
    ] {
        for render_first in [true, false] {
            // Only this isolated fixture is rewritten to simulate a persisted
            // pre-mode document; all production reads/writes below are real.
            let mut document = baseline.clone();
            let settings = document["settings"].as_object_mut().unwrap();
            settings.insert("public_ip".into(), Value::String(address.into()));
            if let Some(mode) = mode {
                settings.insert("connection_mode".into(), Value::String(mode.into()));
            } else {
                settings.remove("connection_mode");
            }
            let mut expected = Value::Object(settings.clone());
            expected["connection_mode"] = Value::String(expected_mode.into());
            fs::write(config_path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();

            let first = if render_first {
                materialize_instance_configuration_for_start(&paths, &created.summary.id)
                    .await
                    .unwrap()
            } else {
                let current = read_instance_details(&paths, &created.summary.id)
                    .await
                    .unwrap();
                update_instance(&paths, update(&current)).await.unwrap()
            };
            assert_eq!(
                serde_json::from_str::<Value>(&first.settings_json).unwrap(),
                expected
            );
            let saved = update_instance(&paths, update(&first)).await.unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&saved.settings_json).unwrap(),
                expected
            );
            let rendered =
                materialize_instance_configuration_for_start(&paths, &created.summary.id)
                    .await
                    .unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&rendered.settings_json).unwrap(),
                expected
            );
            let persisted = read_instance_details(&paths, &created.summary.id)
                .await
                .unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&persisted.settings_json).unwrap(),
                expected
            );
            let ini = fs::read_to_string(
                config_path
                    .parent()
                    .unwrap()
                    .join("Astro/Saved/Config/WindowsServer/AstroServerSettings.ini"),
            )
            .unwrap();
            assert!(
                ini.lines()
                    .any(|line| line.trim() == format!("PublicIP={address}"))
            );
            assert!(!ini.contains("connection_mode"));
        }
    }
    cleanup_root(&root);
}
