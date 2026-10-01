use super::*;

const NATIVE_OPTIONS: [&str; 3] = [
    "auto_pause_when_empty",
    "network_quality",
    "send_gameplay_data",
];

struct SatisfactorySaveFixture {
    root: PathBuf,
    paths: StoragePaths,
    original: InstanceDetails,
    native: PathBuf,
}

impl SatisfactorySaveFixture {
    async fn new() -> Self {
        let root = unique_test_root();
        let mut paths = test_paths(&root);
        paths.modules_root = repo_root().join("modules");
        let descriptor = app_modules::discover_modules(&paths.modules_root)
            .unwrap()
            .into_iter()
            .find(|module| module.summary.id == "satisfactory")
            .unwrap();
        fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
        initialize_database(&paths).await.unwrap();
        sync_modules(&paths, std::slice::from_ref(&descriptor))
            .await
            .unwrap();
        let install_root = paths.games_root.join("satisfactory");
        fs::create_dir_all(&install_root).unwrap();
        fs::write(install_root.join("FactoryServer.exe"), b"synthetic package").unwrap();
        record_library_program_baseline(&install_root, &descriptor, true, None).unwrap();
        sync_game_installs(
            &paths,
            &[GameInstallSyncRecord {
                module_id: "satisfactory".to_owned(),
                install_root: install_root.to_string_lossy().into_owned(),
                install_state: InstallState::Installed,
                current_version: None,
                mark_verified: true,
            }],
        )
        .await
        .unwrap();
        let created = create_instance(
            &paths,
            &descriptor,
            CreateInstanceInput {
                name: "Satisfactory native option ownership".to_owned(),
                module_id: "satisfactory".to_owned(),
            },
        )
        .await
        .unwrap();
        let original = read_instance_details(&paths, &created.summary.id)
            .await
            .unwrap();
        let native = paths
            .instances_root
            .join(&created.summary.id)
            .join("data/Saved/Config/WindowsServer/GameUserSettings.ini");
        Self {
            root,
            paths,
            original,
            native,
        }
    }

    async fn save_and_reload(&self, settings: &Map<String, Value>) -> InstanceDetails {
        let saved = update_instance(
            &self.paths,
            UpdateInstanceInput {
                id: self.original.summary.id.clone(),
                bind_ip: self.original.summary.bind_ip.clone(),
                auto_backup_on_stop: self.original.auto_backup_on_stop,
                backup_retention_count: self.original.backup_retention_count,
                settings_json: serde_json::to_string(settings).unwrap(),
                ports: self.original.ports.clone(),
            },
        )
        .await
        .unwrap();
        materialize_instance_configuration_for_start(&self.paths, &self.original.summary.id)
            .await
            .unwrap();
        let loaded = read_instance_details(&self.paths, &self.original.summary.id)
            .await
            .unwrap();
        for details in [&saved, &loaded] {
            assert_eq!(settings_of(details), *settings);
        }
        loaded
    }
}

impl Drop for SatisfactorySaveFixture {
    fn drop(&mut self) {
        cleanup_root(&self.root);
    }
}

fn settings_of(details: &InstanceDetails) -> Map<String, Value> {
    serde_json::from_str(&details.settings_json).unwrap()
}

fn assert_native_options_unmanaged(details: &InstanceDetails) {
    let settings = settings_of(details);
    for key in NATIVE_OPTIONS {
        assert!(!settings.contains_key(key), "unexpected override: {key}");
    }
}

#[tokio::test]
async fn satisfactory_native_options_database_save_readback_and_release_preserve_ownership() {
    let fixture = SatisfactorySaveFixture::new().await;
    assert_native_options_unmanaged(&fixture.original);
    assert!(!fixture.native.exists());
    let native = concat!(
        "[/Script/FactoryGame.FGGameUserSettings]\n",
        "mIntValues=((\"FG.DSAutoPause\", 0),(\"FG.NetworkQuality\", 3),",
        "(\"FG.SendGameplayData\", 0),(\"FG.FutureOption\", 17),",
        "(\"FicsitRemoteMonitoring.Server.uWS.Port\", 8081))\n",
        "[OtherSection]\nKeep=untouched\n"
    );
    fs::write(&fixture.native, native).unwrap();

    let mut settings = settings_of(&fixture.original);
    settings.insert("max_players".to_owned(), json!(12));
    let unmanaged = fixture.save_and_reload(&settings).await;
    assert_native_options_unmanaged(&unmanaged);
    assert_eq!(settings_of(&unmanaged)["max_players"], json!(12));
    assert_eq!(fs::read(&fixture.native).unwrap(), native.as_bytes());

    settings.insert("auto_pause_when_empty".to_owned(), json!(false));
    settings.insert("network_quality".to_owned(), json!(2));
    settings.insert("send_gameplay_data".to_owned(), json!(false));
    fixture.save_and_reload(&settings).await;
    let expected = native.replace("(\"FG.NetworkQuality\", 3)", "(\"FG.NetworkQuality\", 2)");
    assert_eq!(fs::read(&fixture.native).unwrap(), expected.as_bytes());

    for key in NATIVE_OPTIONS {
        settings.remove(key);
    }
    let released = fixture.save_and_reload(&settings).await;
    assert_native_options_unmanaged(&released);
    assert_eq!(settings_of(&released)["max_players"], json!(12));
    assert_eq!(fs::read(&fixture.native).unwrap(), expected.as_bytes());
}
