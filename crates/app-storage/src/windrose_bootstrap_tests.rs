use super::*;
use app_core::{CreateInstanceInput, PortBinding};
use serde_json::{Value, json};

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
    instance: InstanceDetails,
    runtime: PathBuf,
    original: Vec<u8>,
}

impl Fixture {
    async fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("windrose-bootstrap-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let paths = StoragePaths {
            app_data_root: root.join("app"),
            settings_path: root.join("app/settings.json"),
            database_path: root.join("app/db/test.db"),
            logs_root: root.join("logs"),
            modules_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules"),
            migrations_root: root.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances").join(".trash"),
        };
        fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
        crate::initialize_database(&paths).await.unwrap();
        let descriptor = app_modules::discover_modules(&paths.modules_root)
            .unwrap()
            .into_iter()
            .find(|module| module.summary.id == "windrose")
            .unwrap();
        crate::sync_modules(&paths, std::slice::from_ref(&descriptor))
            .await
            .unwrap();
        let library = paths.games_root.join("windrose");
        fs::create_dir_all(&library).unwrap();
        fs::write(
            library.join("WindroseServer.exe"),
            b"synthetic program only",
        )
        .unwrap();
        let created = crate::create_instance(
            &paths,
            &descriptor,
            CreateInstanceInput {
                name: "My private Windrose".into(),
                module_id: "windrose".into(),
            },
        )
        .await
        .unwrap();
        let ports = [
            PortBinding {
                name: "direct".into(),
                protocol: "udp".into(),
                port: 28017,
            },
            PortBinding {
                name: "direct_tcp".into(),
                protocol: "tcp".into(),
                port: 28017,
            },
        ];
        crate::update_instance_ports(&paths, &created.summary.id, &ports)
            .await
            .unwrap();
        let instance = crate::read_instance_details(&paths, &created.summary.id)
            .await
            .unwrap();
        let runtime = Path::new(&instance.config_file_path)
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("runtime");
        let original = fs::read(runtime.join(SERVER)).unwrap();
        Self {
            root,
            paths,
            instance,
            runtime,
            original,
        }
    }

    async fn prepare(&self) -> WindroseBootstrap {
        prepare_windrose_bootstrap(&self.paths, &self.instance.summary.id)
            .await
            .unwrap()
            .unwrap()
    }

    fn journal(&self) -> PathBuf {
        Path::new(&self.instance.config_file_path)
            .parent()
            .unwrap()
            .join(JOURNAL)
    }

    fn generated_world(&self) -> PathBuf {
        let server = json!({ "Version": 1, "DeploymentId": "native-build", "ServerDescription_Persistent": {
            "PersistentServerId": "NATIVE-SERVER", "WorldIslandId": "NATIVE-WORLD",
            "InviteCode": "NativeInvite", "ServerName": "Native default", "UseDirectConnection": false,
            "DirectConnectionServerPort": 7777, "UnknownNativeField": "preserve"
        }});
        fs::write(
            self.runtime.join(SERVER),
            serde_json::to_vec(&server).unwrap(),
        )
        .unwrap();
        let world = self.runtime.join("R5/Saved/SaveProfiles/Default/RocksDB_v2/0.10.0/Worlds/NATIVE-WORLD/WorldDescription.json");
        fs::create_dir_all(world.parent().unwrap()).unwrap();
        let document = json!({ "Version": 1, "WorldDescription": {
            "islandId": "NATIVE-WORLD", "WorldName": "The Archipelago", "WorldPresetType": "Medium",
            "WorldSettings": {
                "BoolParameters": {
                    "{\"TagName\": \"WDS.Parameter.Coop.SharedQuests\"}": true,
                    "{\"TagName\": \"WDS.Parameter.EasyExplore\"}": false
                },
                "FloatParameters": {
                    "{\"TagName\": \"WDS.Parameter.MobHealthMultiplier\"}": 1.0,
                    "{\"TagName\": \"WDS.Parameter.MobDamageMultiplier\"}": 1.0,
                    "{\"TagName\": \"WDS.Parameter.ShipsHealthMultiplier\"}": 1.0,
                    "{\"TagName\": \"WDS.Parameter.ShipsDamageMultiplier\"}": 1.0,
                    "{\"TagName\": \"WDS.Parameter.BoardingDifficultyMultiplier\"}": 1.0,
                    "{\"TagName\": \"WDS.Parameter.Coop.StatsCorrectionModifier\"}": 1.0,
                    "{\"TagName\": \"WDS.Parameter.Coop.ShipStatsCorrectionModifier\"}": 0.0
                },
                "TagParameters": {
                    "{\"TagName\": \"WDS.Parameter.CombatDifficulty\"}": {
                        "TagName": "WDS.Parameter.CombatDifficulty.Normal"
                    }
                }
            }
        }});
        fs::write(&world, serde_json::to_vec(&document).unwrap()).unwrap();
        world
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[tokio::test]
async fn windrose_bootstrap_stages_only_native_config_and_abort_restores_exact_bytes() {
    let fixture = Fixture::new().await;
    let session = fixture.prepare().await;
    assert_eq!(session.install_root(), fixture.runtime);
    assert!(!fixture.runtime.join(SERVER).exists());
    assert_eq!(
        session.journal.original.as_deref(),
        Some(fixture.original.as_slice())
    );
    assert!(fixture.journal().is_file());
    assert!(matches!(
        prepare_windrose_bootstrap(&fixture.paths, &fixture.instance.summary.id).await,
        Err(StorageError::InstanceSettingsLocked { .. })
    ));
    assert_eq!(
        fs::read(fixture.runtime.join("WindroseServer.exe")).unwrap(),
        b"synthetic program only"
    );
    session.abort_after_stopped().await.unwrap();
    assert_eq!(
        fs::read(fixture.runtime.join(SERVER)).unwrap(),
        fixture.original
    );
    assert!(!fixture.journal().exists());
}

#[tokio::test]
async fn windrose_bootstrap_journal_failure_preserves_config_and_allows_retry() {
    let fixture = Fixture::new().await;
    crate::atomic_file::fail_next_atomic_write_for_test(&fixture.journal());
    let result = prepare_windrose_bootstrap(&fixture.paths, &fixture.instance.summary.id).await;
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("failed journal publication must not stage native configuration"),
    };
    assert!(
        error
            .to_string()
            .contains("cannot publish bootstrap recovery journal")
    );
    assert!(!fixture.journal().exists());
    assert_eq!(
        fs::read(fixture.runtime.join(SERVER)).unwrap(),
        fixture.original
    );

    let retry = fixture.prepare().await;
    let journal: Journal = serde_json::from_slice(&fs::read(fixture.journal()).unwrap()).unwrap();
    assert_eq!(
        journal.original.as_deref(),
        Some(fixture.original.as_slice())
    );
    retry.abort_after_stopped().await.unwrap();
    assert!(!fixture.journal().exists());
    assert_eq!(
        fs::read(fixture.runtime.join(SERVER)).unwrap(),
        fixture.original
    );
}

#[tokio::test]
async fn windrose_bootstrap_rejects_uppercase_world_identity_and_preserves_recovery_data() {
    let fixture = Fixture::new().await;
    let session = fixture.prepare().await;
    let world = fixture.generated_world();
    assert!(session.inspect_native_state().unwrap().world_ready);
    let mut document: Value = serde_json::from_slice(&fs::read(&world).unwrap()).unwrap();
    let description = document["WorldDescription"].as_object_mut().unwrap();
    let identity = description.remove("islandId").unwrap();
    description.insert("IslandId".into(), identity);
    fs::write(&world, serde_json::to_vec(&document).unwrap()).unwrap();
    let world_before = fs::read(&world).unwrap();
    let server_before = fs::read(fixture.runtime.join(SERVER)).unwrap();
    let journal_before = fs::read(fixture.journal()).unwrap();
    assert!(!session.inspect_native_state().unwrap().world_ready);
    assert!(session.finish_after_stopped().await.is_err());
    assert_eq!(fs::read(&world).unwrap(), world_before);
    assert_eq!(
        fs::read(fixture.runtime.join(SERVER)).unwrap(),
        server_before
    );
    assert_eq!(fs::read(fixture.journal()).unwrap(), journal_before);
}

#[tokio::test]
async fn windrose_bootstrap_finish_preserves_native_world_identity_and_applies_instance_transport()
{
    let fixture = Fixture::new().await;
    let session = fixture.prepare().await;
    let world = fixture.generated_world();
    let world_bytes = fs::read(&world).unwrap();
    let observation = session.inspect_native_state().unwrap();
    assert!(observation.world_ready);
    assert_eq!(observation.use_direct_connection, Some(false));
    assert_eq!(observation.direct_connection_server_port, Some(7777));
    session.finish_after_stopped().await.unwrap();
    let server: Value =
        serde_json::from_slice(&fs::read(fixture.runtime.join(SERVER)).unwrap()).unwrap();
    let persistent = &server["ServerDescription_Persistent"];
    assert_eq!(persistent["PersistentServerId"], "NATIVE-SERVER");
    assert_eq!(persistent["WorldIslandId"], "NATIVE-WORLD");
    assert_eq!(persistent["InviteCode"], "NativeInvite");
    assert_eq!(persistent["UnknownNativeField"], "preserve");
    assert_eq!(persistent["ServerName"], "My private Windrose");
    assert_eq!(persistent["UseDirectConnection"], true);
    assert_eq!(persistent["DirectConnectionServerPort"], 28017);
    assert_eq!(fs::read(world).unwrap(), world_bytes);
    let persisted = crate::read_instance_details(&fixture.paths, &fixture.instance.summary.id)
        .await
        .unwrap();
    let settings: Value = serde_json::from_str(&persisted.settings_json).unwrap();
    assert_eq!(settings["world_island_id"], "NATIVE-WORLD");
    assert!(!fixture.journal().exists());
    assert!(
        prepare_windrose_bootstrap(&fixture.paths, &fixture.instance.summary.id)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn windrose_bootstrap_binds_generated_world_and_stages_user_world_parameters() {
    let fixture = Fixture::new().await;
    let config_path = Path::new(&fixture.instance.config_file_path);
    let mut config: Value = serde_json::from_slice(&fs::read(config_path).unwrap()).unwrap();
    config["settings"]["world_name"] = json!("Operator's archipelago");
    config["settings"]["combat_difficulty"] = json!("Hard");
    config["settings"]["mob_health_multiplier"] = json!(2.5);
    fs::write(config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let session = fixture.prepare().await;
    let world = fixture.generated_world();
    let original_world = fs::read(&world).unwrap();
    let original_server = fs::read(fixture.runtime.join(SERVER)).unwrap();
    session.finish_after_stopped().await.unwrap();

    let details = crate::read_instance_details(&fixture.paths, &fixture.instance.summary.id)
        .await
        .unwrap();
    let settings: Value = serde_json::from_str(&details.settings_json).unwrap();
    assert_eq!(settings["world_island_id"], "NATIVE-WORLD");
    assert_eq!(settings["world_name"], "Operator's archipelago");
    let plan_path = config_path
        .parent()
        .unwrap()
        .join("windrose-world-update-plan.json");
    let plan_bytes = fs::read(&plan_path).unwrap();
    let plan: Value = serde_json::from_slice(&plan_bytes).unwrap();
    assert_eq!(plan["world_island_id"], "NATIVE-WORLD");
    assert_eq!(
        plan["world_parameters"]["world_name"],
        "Operator's archipelago"
    );
    assert_eq!(plan["world_parameters"]["combat_difficulty"], "Hard");
    assert_eq!(plan["world_parameters"]["mob_health_multiplier"], 2.5);
    // The official updater owns the coordinated native-file mutation. Missing
    // updater must fail the normal prestart rather than silently ignore choices.
    assert!(
        crate::materialize_instance_configuration_for_start(
            &fixture.paths,
            &fixture.instance.summary.id,
        )
        .await
        .is_err()
    );
    assert_eq!(fs::read(&world).unwrap(), original_world);
    assert_eq!(
        fs::read(fixture.runtime.join(SERVER)).unwrap(),
        original_server
    );
    assert_eq!(fs::read(&plan_path).unwrap(), plan_bytes);
}

#[tokio::test]
async fn windrose_bootstrap_resumes_committed_identity_with_uncleared_journal() {
    let fixture = Fixture::new().await;
    let session = fixture.prepare().await;
    let world = fixture.generated_world();
    let native_world = fs::read(&world).unwrap();
    let journal = fs::read(fixture.journal()).unwrap();
    session.finish_after_stopped().await.unwrap();
    // Simulate the durable settings commit preceding journal cleanup.
    fs::write(fixture.journal(), &journal).unwrap();
    let retry = fixture.prepare().await;
    assert!(retry.inspect_native_state().unwrap().world_ready);
    retry.finish_after_stopped().await.unwrap();
    assert_eq!(fs::read(&world).unwrap(), native_world);
    assert!(!fixture.journal().exists());
}

#[tokio::test]
async fn windrose_bootstrap_accepts_a_completed_world_after_a_crashed_run() {
    let fixture = Fixture::new().await;
    let world = fixture.generated_world();
    let world_bytes = fs::read(&world).unwrap();
    let server_bytes = fs::read(fixture.runtime.join(SERVER)).unwrap();
    let pool = crate::storage_db::connect_pool(&fixture.paths)
        .await
        .unwrap();
    sqlx::query("UPDATE instances SET status = 'error' WHERE id = ?1")
        .bind(&fixture.instance.summary.id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let details = crate::read_instance_details(&fixture.paths, &fixture.instance.summary.id)
        .await
        .unwrap();
    assert!(matches!(details.summary.status, InstanceStatus::Error));
    assert!(details.active_run.is_none());
    assert!(
        prepare_windrose_bootstrap(&fixture.paths, &fixture.instance.summary.id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(fs::read(&world).unwrap(), world_bytes);
    assert_eq!(
        fs::read(fixture.runtime.join(SERVER)).unwrap(),
        server_bytes
    );
    assert!(!fixture.journal().exists());
}

#[tokio::test]
async fn windrose_bootstrap_recovers_a_pending_world_after_a_crashed_run() {
    let fixture = Fixture::new().await;
    let session = fixture.prepare().await;
    let world = fixture.generated_world();
    let world_bytes = fs::read(&world).unwrap();
    let journal_bytes = fs::read(fixture.journal()).unwrap();
    drop(session);
    let pool = crate::storage_db::connect_pool(&fixture.paths)
        .await
        .unwrap();
    sqlx::query("UPDATE instances SET status = 'error' WHERE id = ?1")
        .bind(&fixture.instance.summary.id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let retry = fixture.prepare().await;
    assert!(retry.inspect_native_state().unwrap().world_ready);
    assert_eq!(fs::read(fixture.journal()).unwrap(), journal_bytes);
    retry.finish_after_stopped().await.unwrap();
    let details = crate::read_instance_details(&fixture.paths, &fixture.instance.summary.id)
        .await
        .unwrap();
    let settings: Value = serde_json::from_str(&details.settings_json).unwrap();
    assert_eq!(settings["world_island_id"], "NATIVE-WORLD");
    assert!(matches!(details.summary.status, InstanceStatus::Error));
    assert!(details.active_run.is_none());
    assert_eq!(fs::read(&world).unwrap(), world_bytes);
    assert!(!fixture.journal().exists());
}

#[tokio::test]
async fn windrose_bootstrap_partial_output_survives_abort_drop_and_retry() {
    let fixture = Fixture::new().await;
    let session = fixture.prepare().await;
    let partial = b"{\"Version\":1,\"ServerDescription_Persistent\":";
    fs::write(fixture.runtime.join(SERVER), partial).unwrap();
    assert!(!session.inspect_native_state().unwrap().world_ready);
    session.abort_after_stopped().await.unwrap();
    assert_eq!(fs::read(fixture.runtime.join(SERVER)).unwrap(), partial);
    let journal = fs::read(fixture.journal()).unwrap();
    let session = fixture.prepare().await;
    assert_eq!(fs::read(fixture.runtime.join(SERVER)).unwrap(), partial);
    drop(session);
    assert_eq!(fs::read(fixture.journal()).unwrap(), journal);
    let session = fixture.prepare().await;
    fixture.generated_world();
    session.finish_after_stopped().await.unwrap();
    assert!(!fixture.journal().exists());
}

#[tokio::test]
async fn windrose_bootstrap_recovers_crash_before_native_config_was_staged() {
    let fixture = Fixture::new().await;
    let session = fixture.prepare().await;
    // This is the durable boundary between journal fsync and config CAS.
    fs::write(fixture.runtime.join(SERVER), &fixture.original).unwrap();
    drop(session);
    let retry = fixture.prepare().await;
    assert!(!fixture.runtime.join(SERVER).exists());
    retry.abort_after_stopped().await.unwrap();
    assert_eq!(
        fs::read(fixture.runtime.join(SERVER)).unwrap(),
        fixture.original
    );
}

#[tokio::test]
async fn windrose_bootstrap_refuses_running_explicit_world_and_foreign_journal() {
    let fixture = Fixture::new().await;
    let pool = crate::storage_db::connect_pool(&fixture.paths)
        .await
        .unwrap();
    sqlx::query("UPDATE instances SET status = 'running' WHERE id = ?1")
        .bind(&fixture.instance.summary.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        prepare_windrose_bootstrap(&fixture.paths, &fixture.instance.summary.id)
            .await
            .is_err()
    );
    assert_eq!(
        fs::read(fixture.runtime.join(SERVER)).unwrap(),
        fixture.original
    );
    sqlx::query("UPDATE instances SET status = 'stopped' WHERE id = ?1")
        .bind(&fixture.instance.summary.id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let config_path = Path::new(&fixture.instance.config_file_path);
    let config_bytes = fs::read(config_path).unwrap();
    let mut config: Value = serde_json::from_slice(&config_bytes).unwrap();
    config["settings"]["world_island_id"] = json!("operator-selected-world");
    fs::write(config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    assert!(
        prepare_windrose_bootstrap(&fixture.paths, &fixture.instance.summary.id)
            .await
            .is_err()
    );
    assert!(!fixture.journal().exists());
    fs::write(config_path, config_bytes).unwrap();
    let session = fixture.prepare().await;
    let mut journal: Value = serde_json::from_slice(&fs::read(fixture.journal()).unwrap()).unwrap();
    journal["instance_id"] = json!("another-instance");
    let foreign = serde_json::to_vec(&journal).unwrap();
    fs::write(fixture.journal(), &foreign).unwrap();
    drop(session);
    assert!(
        prepare_windrose_bootstrap(&fixture.paths, &fixture.instance.summary.id)
            .await
            .is_err()
    );
    assert_eq!(fs::read(fixture.journal()).unwrap(), foreign);
}

#[tokio::test]
async fn windrose_bootstrap_refuses_existing_world_and_oversized_document_without_mutation() {
    let fixture = Fixture::new().await;
    let world = fixture.generated_world();
    fs::write(fixture.runtime.join(SERVER), &fixture.original).unwrap();
    let before = fs::read(&world).unwrap();
    assert!(
        prepare_windrose_bootstrap(&fixture.paths, &fixture.instance.summary.id)
            .await
            .is_err()
    );
    assert_eq!(fs::read(&world).unwrap(), before);
    assert_eq!(
        fs::read(fixture.runtime.join(SERVER)).unwrap(),
        fixture.original
    );
    assert!(!fixture.journal().exists());
    fs::write(
        fixture.runtime.join(SERVER),
        vec![b' '; MAX_DOCUMENT as usize + 1],
    )
    .unwrap();
    assert!(
        prepare_windrose_bootstrap(&fixture.paths, &fixture.instance.summary.id)
            .await
            .is_err()
    );
    assert_eq!(
        fs::metadata(fixture.runtime.join(SERVER)).unwrap().len(),
        MAX_DOCUMENT + 1
    );
    assert!(!fixture.journal().exists());
}
