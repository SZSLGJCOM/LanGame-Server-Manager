use super::*;

struct SavePolicyFixture {
    root: PathBuf,
    paths: StoragePaths,
    modules: Vec<ModuleDescriptor>,
}

impl SavePolicyFixture {
    async fn new() -> Self {
        let root = unique_test_root();
        let mut paths = test_paths(&root);
        paths.modules_root = repo_root().join("modules");
        let modules = app_modules::discover_modules(&paths.modules_root).unwrap();
        fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
        initialize_database(&paths).await.unwrap();
        sync_modules(&paths, &modules).await.unwrap();
        for module in &modules {
            let install = module.install.as_ref().unwrap();
            let install_root = paths.games_root.join(&install.shared_game_dir);
            fs::create_dir_all(&install_root).unwrap();
            if module.summary.id == "palworld" {
                fs::create_dir_all(install_root.join("Pal")).unwrap();
            }
        }
        Self {
            root,
            paths,
            modules,
        }
    }
}

impl Drop for SavePolicyFixture {
    fn drop(&mut self) {
        cleanup_root(&self.root);
    }
}

fn native_save_policy_settings(module_id: &str) -> Map<String, Value> {
    let settings = match module_id {
        "arksurvivalascended" => json!({ "auto_save_period_minutes": 23.5 }),
        "arksurvivalevolved" => json!({
            "auto_save_period_minutes": 23.5, "max_num_of_save_backups": 13
        }),
        "astroneer" => json!({
            "auto_save_interval_seconds": 1230, "backup_save_interval_seconds": 5430
        }),
        "dontstarve" => json!({ "autosaver_enabled": false, "max_snapshots": 9 }),
        "humanitz" => json!({ "save_interval_seconds": 421 }),
        "palworld" => json!({ "auto_save_span": 47.5, "use_backup_save_data": false }),
        "projectzomboid" => json!({
            "save_world_every_minutes": 13, "backups_count": 8,
            "backups_on_start": false, "backups_on_version_change": false,
            "backups_period": 71
        }),
        "rust" => json!({ "save_interval_seconds": 421 }),
        "satisfactory" => json!({ "rotating_autosaves": 8 }),
        "sonsoftheforest" => json!({ "save_interval": 421 }),
        "soulmask" => json!({
            "save_interval_seconds": 421, "backup_interval_seconds": 1261
        }),
        "terraria" => json!({ "worldrollbackstokeep": 7 }),
        "theforest" => json!({ "autosave_interval_minutes": 47 }),
        "valheim" => json!({
            "save_interval_seconds": 1261, "backup_count": 7,
            "backup_short_seconds": 5431, "backup_long_seconds": 23401
        }),
        "vrising" => json!({
            "autosave_count": 13, "autosave_interval_seconds": 421,
            "autosave_smart_keep": "10:2:1,60:1:1,1440:5:0"
        }),
        _ => json!({}),
    };
    settings.as_object().unwrap().clone()
}

fn assert_native_lines(path: &Path, expected: &[&str]) {
    let content = fs::read_to_string(path).unwrap_or_else(|error| {
        panic!("read native save configuration {}: {error}", path.display())
    });
    for expected_line in expected {
        assert!(
            content.lines().any(|line| line.trim() == *expected_line),
            "{} must contain native save policy {expected_line}",
            path.display()
        );
    }
}

fn assert_native_json(path: &Path, expected: Value) {
    let content: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    for (key, value) in expected.as_object().unwrap() {
        assert_eq!(&content[key], value, "{}.{}", path.display(), key);
    }
}

fn assert_native_save_configuration(
    module_id: &str,
    details: &InstanceDetails,
    install_root: &Path,
) {
    let config = Path::new(&details.config_file_path).parent().unwrap();
    let instance = config.parent().unwrap();
    let check = |source: &str, destination: Option<PathBuf>, expected: &[&str]| {
        assert_native_lines(&config.join(source), expected);
        if let Some(destination) = destination {
            assert_native_lines(&destination, expected);
        }
    };
    match module_id {
        "arksurvivalascended" | "arksurvivalevolved" => check(
            "GameUserSettings.ini",
            Some(install_root.join("ShooterGame/Saved/Config/WindowsServer/GameUserSettings.ini")),
            &["AutoSavePeriodMinutes=23.5"],
        ),
        "astroneer" => check(
            "Astro/Saved/Config/WindowsServer/AstroServerSettings.ini",
            Some(install_root.join("Astro/Saved/Config/WindowsServer/AstroServerSettings.ini")),
            &["AutoSaveGameInterval=1230", "BackupSaveGamesInterval=5430"],
        ),
        "dontstarve" => check(
            "clusters/main/cluster.ini",
            None,
            &["autosaver_enabled = false", "max_snapshots = 9"],
        ),
        "humanitz" => check(
            "GameServerSettings.ini",
            Some(install_root.join("HumanitZServer/GameServerSettings.ini")),
            &["SaveIntervalSec=421"],
        ),
        "palworld" => {
            for path in [
                config.join("PalWorldSettings.ini"),
                install_root.join("Pal/Saved/Config/WindowsServer/PalWorldSettings.ini"),
            ] {
                let content = fs::read_to_string(&path).unwrap();
                for token in [",AutoSaveSpan=47.5,", ",bIsUseBackupSaveData=False,"] {
                    assert!(content.contains(token), "{}: {token}", path.display());
                }
            }
        }
        "projectzomboid" => check(
            "server.ini",
            Some(config.join(format!(
                "runtime-home/Zomboid/Server/{}.ini",
                details.summary.id
            ))),
            &[
                "SaveWorldEveryMinutes=13",
                "BackupsCount=8",
                "BackupsOnStart=false",
                "BackupsOnVersionChange=false",
                "BackupsPeriod=71",
            ],
        ),
        "rust" => check(
            "server.cfg",
            Some(install_root.join(format!("server/{}/cfg/server.cfg", details.summary.id))),
            &["server.saveinterval 421"],
        ),
        "satisfactory" => check(
            "Engine.ini",
            Some(instance.join("data/Saved/Config/WindowsServer/Engine.ini")),
            &["mNumRotatingAutosaves=8"],
        ),
        "sonsoftheforest" => {
            for path in [
                config.join("dedicatedserver.cfg"),
                Path::new(&details.saves_path)
                    .parent()
                    .unwrap()
                    .join("dedicatedserver.cfg"),
            ] {
                assert_native_json(&path, json!({ "SaveInterval": 421 }));
            }
        }
        "terraria" => check("serverconfig.txt", None, &["worldrollbackstokeep=7"]),
        "theforest" => check("server.cfg", None, &["serverAutoSaveInterval 47"]),
        "vrising" => {
            for path in [
                config.join("Settings/ServerHostSettings.json"),
                instance.join("Settings/ServerHostSettings.json"),
            ] {
                assert_native_json(
                    &path,
                    json!({
                        "AutoSaveCount": 13, "AutoSaveInterval": 421,
                        "AutoSaveSmartKeep": "10:2:1,60:1:1,1440:5:0"
                    }),
                );
            }
        }
        // Soulmask and Valheim consume startup arguments. The runtime crate's
        // save-policy test checks the real launch-plan renderer for these fields
        // and ARK SE's MaxNumOfSaveBackups; no synthetic renderer is used here.
        "soulmask" | "valheim" => {}
        _ => assert!(native_save_policy_settings(module_id).is_empty()),
    }
}

#[tokio::test]
async fn save_policy_updates_persist_for_all_games_and_materialize_native_configuration() {
    let fixture = SavePolicyFixture::new().await;
    assert_eq!(
        fixture.modules.len(),
        32,
        "review save-policy coverage when the catalog changes"
    );
    let mut native_modules = 0;
    let mut native_fields = 0;
    for module in &fixture.modules {
        let module_id = module.summary.id.as_str();
        replenish_test_library(&fixture.paths, module).await;
        let created = create_instance(
            &fixture.paths,
            module,
            CreateInstanceInput {
                name: format!("{module_id} save policy"),
                module_id: module_id.to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("create {module_id}: {error}"));
        let original = read_instance_details(&fixture.paths, &created.summary.id)
            .await
            .unwrap();
        let mut expected_settings: Map<String, Value> =
            serde_json::from_str(&original.settings_json).unwrap();
        let native_settings = native_save_policy_settings(module_id);
        let schema: Value = serde_json::from_str(module.schema_json.as_ref().unwrap()).unwrap();
        if !native_settings.is_empty() {
            native_modules += 1;
            native_fields += native_settings.len();
        }
        for (key, value) in native_settings {
            assert!(
                schema["properties"][&key].is_object(),
                "{module_id}.{key} must be modeled"
            );
            assert_ne!(
                schema["properties"][&key]["default"], value,
                "{module_id}.{key} fixture must change the default"
            );
            expected_settings.insert(key, value);
        }
        update_instance_autostart(&fixture.paths, &created.summary.id, true)
            .await
            .unwrap();
        // Use the original details snapshot: saving a policy must not overwrite
        // an independent autostart change or drop unrelated settings and ports.
        for (enabled, retained) in [(true, 7), (false, 11)] {
            let updated = update_instance(
                &fixture.paths,
                UpdateInstanceInput {
                    id: created.summary.id.clone(),
                    bind_ip: original.summary.bind_ip.clone(),
                    auto_backup_on_stop: enabled,
                    backup_retention_count: retained,
                    settings_json: serde_json::to_string(&expected_settings).unwrap(),
                    ports: original.ports.clone(),
                },
            )
            .await
            .unwrap_or_else(|error| panic!("save policy for {module_id}: {error}"));
            materialize_instance_configuration(&fixture.paths, &created.summary.id)
                .await
                .unwrap_or_else(|error| panic!("materialize {module_id}: {error}"));
            let reloaded = read_instance_details(&fixture.paths, &created.summary.id)
                .await
                .unwrap();
            for details in [&updated, &reloaded] {
                assert_eq!(details.auto_backup_on_stop, enabled, "{module_id}");
                assert_eq!(details.backup_retention_count, retained, "{module_id}");
                assert!(
                    details.summary.autostart,
                    "{module_id} autostart must survive"
                );
                assert_eq!(
                    details.summary.bind_ip, original.summary.bind_ip,
                    "{module_id}"
                );
                assert_eq!(
                    serde_json::to_value(&details.ports).unwrap(),
                    serde_json::to_value(&original.ports).unwrap(),
                    "{module_id} ports"
                );
                assert_eq!(
                    serde_json::from_str::<Map<String, Value>>(&details.settings_json).unwrap(),
                    expected_settings,
                    "{module_id} settings"
                );
            }
            let document: Value =
                serde_json::from_slice(&fs::read(&created.config_file_path).unwrap()).unwrap();
            assert_eq!(
                document["settings"],
                json!(expected_settings),
                "{module_id} manager file"
            );
            assert_eq!(document["autostart"], true, "{module_id} manager mirror");
            assert_native_save_configuration(
                module_id,
                &reloaded,
                &resolve_instance_runtime_root(
                    &fixture.paths.instances_root.join(&created.summary.id),
                )
                .unwrap(),
            );
        }
    }
    assert_eq!(native_modules, 15);
    assert_eq!(native_fields, 29);
}
