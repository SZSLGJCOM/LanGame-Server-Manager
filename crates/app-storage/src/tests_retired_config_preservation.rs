use super::*;

struct RetiredConfigFixture {
    root: PathBuf,
    paths: StoragePaths,
    original: InstanceDetails,
    native: PathBuf,
    rendered: PathBuf,
}

impl RetiredConfigFixture {
    async fn new(module_id: &str, native_relative_path: &str, rendered_name: &str) -> Self {
        let root = unique_test_root();
        let mut paths = test_paths(&root);
        paths.modules_root = repo_root().join("modules");
        let descriptor = app_modules::discover_modules(&paths.modules_root)
            .unwrap()
            .into_iter()
            .find(|module| module.summary.id == module_id)
            .unwrap();
        fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
        initialize_database(&paths).await.unwrap();
        sync_modules(&paths, std::slice::from_ref(&descriptor))
            .await
            .unwrap();
        let install_root = paths.games_root.join(module_id);
        let executable = install_root.join(&descriptor.process.as_ref().unwrap().executable);
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, b"synthetic package").unwrap();
        record_library_program_baseline(&install_root, &descriptor, true, None).unwrap();
        sync_game_installs(
            &paths,
            &[GameInstallSyncRecord {
                module_id: module_id.to_owned(),
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
                name: format!("{module_id} retained configuration"),
                module_id: module_id.to_owned(),
            },
        )
        .await
        .unwrap();
        let native = instance_private_runtime_root(&created).join(native_relative_path);
        let rendered = Path::new(&created.config_file_path)
            .parent()
            .unwrap()
            .join(rendered_name);
        let original = read_instance_details(&paths, &created.summary.id)
            .await
            .unwrap();
        Self {
            root,
            paths,
            original,
            native,
            rendered,
        }
    }

    fn seed_retired_settings(&self, retired: Value, native: &str) {
        // Reproduce a persisted instance from before these controls were retired.
        let path = Path::new(&self.original.config_file_path);
        let mut document: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        document["settings"]
            .as_object_mut()
            .unwrap()
            .extend(retired.as_object().unwrap().clone());
        fs::write(path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
        fs::create_dir_all(self.native.parent().unwrap()).unwrap();
        fs::write(&self.native, native).unwrap();
    }

    async fn rename_and_reload(&self, name: &str) -> Map<String, Value> {
        let before = read_instance_details(&self.paths, &self.original.summary.id)
            .await
            .unwrap();
        let mut settings: Map<String, Value> = serde_json::from_str(&before.settings_json).unwrap();
        settings.insert("server_name".to_owned(), json!(name));
        let saved = update_instance(
            &self.paths,
            UpdateInstanceInput {
                id: before.summary.id.clone(),
                bind_ip: before.summary.bind_ip.clone(),
                auto_backup_on_stop: before.auto_backup_on_stop,
                backup_retention_count: before.backup_retention_count,
                settings_json: serde_json::to_string(&settings).unwrap(),
                ports: before.ports.clone(),
            },
        )
        .await
        .unwrap();
        let reloaded = read_instance_details(&self.paths, &before.summary.id)
            .await
            .unwrap();
        for details in [&saved, &reloaded] {
            assert_eq!(
                serde_json::from_str::<Map<String, Value>>(&details.settings_json).unwrap(),
                settings
            );
        }
        // This publishes configuration through the start preparation API without
        // launching a server process; retired fields must survive this boundary too.
        materialize_instance_configuration_for_start(&self.paths, &before.summary.id)
            .await
            .unwrap();
        let prepared = read_instance_details(&self.paths, &before.summary.id)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Map<String, Value>>(&prepared.settings_json).unwrap(),
            settings
        );
        settings
    }
}

impl Drop for RetiredConfigFixture {
    fn drop(&mut self) {
        cleanup_root(&self.root);
    }
}

fn assert_no_native_keys(content: &str, keys: &[&str]) {
    for key in keys {
        assert!(
            !content.contains(&format!("{key}=")),
            "unexpected native key: {key}"
        );
    }
}

#[tokio::test]
async fn conan_retired_controls_preserve_old_values_without_duplicating_action_costs() {
    let fixture = RetiredConfigFixture::new(
        "conanexiles",
        "ConanSandbox/Saved/Config/WindowsServer/ServerSettings.ini",
        "ServerSettings.ini",
    )
    .await;
    assert_no_native_keys(
        &fs::read_to_string(&fixture.rendered).unwrap(),
        &[
            "PlayerStaminaCostMultiplier",
            "ValidatePhysNavWalkWithRaycast",
        ],
    );
    fixture.seed_retired_settings(
        json!({
            "player_stamina_cost_multiplier": 1.25,
            "validate_phys_nav_walk_with_raycast": true,
            "action_stamina_cost_multiplier": 0.75
        }),
        "[ServerSettings]\nPlayerStaminaCostMultiplier=1.5\nValidatePhysNavWalkWithRaycast=False\nStaminaCostMultiplier=2.5\nOperatorValue=keep\n",
    );
    let settings = fixture.rename_and_reload("Conan retained settings").await;
    assert_eq!(settings["player_stamina_cost_multiplier"], json!(1.25));
    assert_eq!(settings["validate_phys_nav_walk_with_raycast"], json!(true));
    assert_eq!(settings["action_stamina_cost_multiplier"], json!(0.75));
    let native = fs::read_to_string(&fixture.native).unwrap();
    assert_eq!(native.matches("PlayerStaminaCostMultiplier=").count(), 1);
    assert!(native.contains("PlayerStaminaCostMultiplier=1.5"));
    assert_eq!(native.matches("ValidatePhysNavWalkWithRaycast=").count(), 1);
    assert!(native.contains("ValidatePhysNavWalkWithRaycast=False"));
    assert!(
        native
            .lines()
            .any(|line| line == "StaminaCostMultiplier=0.75")
    );
    assert!(native.contains("OperatorValue=keep"));
    assert_no_native_keys(
        &fs::read_to_string(&fixture.rendered).unwrap(),
        &[
            "PlayerStaminaCostMultiplier",
            "ValidatePhysNavWalkWithRaycast",
        ],
    );
}

#[tokio::test]
async fn humanitz_retired_config_survives_database_save_and_native_publication() {
    let fixture = RetiredConfigFixture::new(
        "humanitz",
        "HumanitZServer/GameServerSettings.ini",
        "GameServerSettings.ini",
    )
    .await;
    let retired = json!({
        "only_allowed_players": 0,
        "allowed_player_steam_ids": "76561198000000000",
        "clear_infection_on_respawn": 1,
        "human_difficulty": 2,
        "loot_rarity": 2,
        "void_enabled": 1,
        "vehicle_decay_hours": 48,
        "owned_vehicle_decay_hours": 168,
        "vehicle_decay_duration_seconds": 3600,
        "max_world_vehicles": 40,
        "max_vehicles_per_player": 3,
        "vehicle_respawn_enabled": true,
        "vehicle_respawn_interval_seconds": 3600,
        "ambient_vehicle_damage": false,
        "ambient_damage_rate": 1,
        "zombies_target_vehicles": true,
        "eagle_eye_enabled": true
    });
    let assignments = [
        ("OnlyAllowedPlayers", "71"),
        ("ClearInfection", "72"),
        ("HumanDifficulty", "73"),
        ("LootRarity", "74"),
        ("Void", "75"),
        ("VehicleDecayHours", "76"),
        ("OwnedVehicleDecayHours", "77"),
        ("VehicleDecayDuration", "78"),
        ("MaxWorldVehicles", "79"),
        ("MaxVehiclesPerPlayer", "80"),
        ("VehicleRespawnEnabled", "81"),
        ("VehicleRespawnInterval", "82"),
        ("AmbientVehicleDamage", "83"),
        ("AmbientDamageRate", "84"),
        ("ZombiesTargetVehicles", "85"),
        ("EagleEye", "93"),
    ];
    let keys = assignments.map(|(key, _)| key);
    for path in [&fixture.rendered, &fixture.native] {
        assert_no_native_keys(&fs::read_to_string(path).unwrap(), &keys);
    }
    let fresh: Value = serde_json::from_str(&fixture.original.settings_json).unwrap();
    for key in retired.as_object().unwrap().keys() {
        assert!(fresh.get(key).is_none(), "unexpected new default: {key}");
    }
    let historical_roster = fixture.native.parent().unwrap().join("F_MVPAccess.txt");
    assert!(!historical_roster.exists());
    fixture.seed_retired_settings(
        retired.clone(),
        concat!(
            "[Host Settings]\nServerName=\"previous\"\nOnlyAllowedPlayers=71\n",
            "[World Settings]\nClearInfection=72\nHumanDifficulty=73\n",
            "LootRarity=74\nVoid=75\nEagleEye=93\n",
            "[VehicleSettings]\nVehicleDecayHours=76\nOwnedVehicleDecayHours=77\n",
            "VehicleDecayDuration=78\nMaxWorldVehicles=79\nMaxVehiclesPerPlayer=80\n",
            "VehicleRespawnEnabled=81\nVehicleRespawnInterval=82\n",
            "AmbientVehicleDamage=83\nAmbientDamageRate=84\nZombiesTargetVehicles=85\n",
            "[Unmanaged]\nOperatorValue=keep\n"
        ),
    );
    fs::write(&historical_roster, b"76561198011111111\r\n").unwrap();
    let settings = fixture.rename_and_reload("HumanitZ preserved").await;
    for (key, value) in retired.as_object().unwrap() {
        assert_eq!(settings.get(key), Some(value), "retired JSON value: {key}");
    }
    let native = fs::read_to_string(&fixture.native).unwrap();
    for (key, value) in assignments {
        let values = native
            .lines()
            .filter_map(|line| line.split_once('='))
            .filter_map(|(candidate, value)| (candidate.trim() == key).then_some(value.trim()))
            .collect::<Vec<_>>();
        assert_eq!(values, vec![value], "retired native value: {key}");
    }
    assert!(native.contains("ServerName=\"HumanitZ preserved\""));
    assert!(native.contains("OperatorValue=keep"));
    assert_eq!(
        fs::read(&historical_roster).unwrap(),
        b"76561198011111111\r\n"
    );
    assert!(
        !fixture
            .rendered
            .parent()
            .unwrap()
            .join("F_MVPAccess.txt")
            .exists()
    );
    assert_no_native_keys(&fs::read_to_string(&fixture.rendered).unwrap(), &keys);
}

#[tokio::test]
async fn palworld_retired_config_survives_database_save_and_native_publication() {
    let fixture = RetiredConfigFixture::new(
        "palworld",
        "Pal/Saved/Config/WindowsServer/PalWorldSettings.ini",
        "PalWorldSettings.ini",
    )
    .await;
    let retired = json!({
        "difficulty": "Easy",
        "is_multiplay": false,
        "coop_player_max_num": 4,
        "enable_defense_other_guild_player": false,
        "enable_non_login_penalty": false
    });
    let assignments = [
        ("Difficulty", "Hard"),
        ("bIsMultiplay", "True"),
        ("CoopPlayerMaxNum", "11"),
        ("bEnableDefenseOtherGuildPlayer", "True"),
        ("bEnableNonLoginPenalty", "True"),
    ];
    let keys = assignments.map(|(key, _)| key);
    for path in [&fixture.rendered, &fixture.native] {
        assert_no_native_keys(&fs::read_to_string(path).unwrap(), &keys);
    }
    let fresh: Value = serde_json::from_str(&fixture.original.settings_json).unwrap();
    for key in retired.as_object().unwrap().keys() {
        assert!(fresh.get(key).is_none(), "unexpected new default: {key}");
    }
    fixture.seed_retired_settings(
        retired.clone(),
        concat!(
            "[/Script/Pal.PalGameWorldSettings]\n",
            "OptionSettings=(Difficulty=Hard,bIsMultiplay=True,CoopPlayerMaxNum=11,",
            "bEnableDefenseOtherGuildPlayer=True,bEnableNonLoginPenalty=True,",
            "ServerName=\"previous\",FutureOption=(Name=\"keep,me\",Values=(1,2)))\n",
            "[Unmanaged]\nOperatorValue=keep\n"
        ),
    );
    let settings = fixture.rename_and_reload("Palworld preserved").await;
    for (key, value) in retired.as_object().unwrap() {
        assert_eq!(settings.get(key), Some(value), "retired JSON value: {key}");
    }
    let native = fs::read_to_string(&fixture.native).unwrap();
    for (key, value) in assignments {
        assert_eq!(
            native.matches(&format!("{key}=")).count(),
            1,
            "duplicate {key}"
        );
        assert!(
            native.contains(&format!("{key}={value},")),
            "retired native value: {key}"
        );
    }
    assert_eq!(native.matches("OptionSettings=").count(), 1);
    assert_eq!(native.matches("ServerName=").count(), 1);
    assert!(native.contains("ServerName=\"Palworld preserved\""));
    assert!(native.contains("FutureOption=(Name=\"keep,me\",Values=(1,2))"));
    assert!(native.contains("OperatorValue=keep"));
    assert_no_native_keys(&fs::read_to_string(&fixture.rendered).unwrap(), &keys);
}
