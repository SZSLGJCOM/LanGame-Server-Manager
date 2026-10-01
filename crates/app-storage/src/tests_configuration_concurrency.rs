use super::*;
use crate::instance_archive::test_gate::{self, Point};
use crate::instance_settings_lock::acquire_instance_settings_mutation_lock;
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
    descriptor: ModuleDescriptor,
}

impl Fixture {
    async fn new() -> Self {
        let root = unique_test_root();
        let paths = test_paths(&root);
        let descriptor = barotrauma_test_descriptor(&root);
        prepare_barotrauma_environment(&root, &descriptor);
        initialize_database(&paths).await.unwrap();
        sync_modules(&paths, std::slice::from_ref(&descriptor))
            .await
            .unwrap();
        let library = paths.games_root.join("barotrauma");
        fs::create_dir_all(&library).unwrap();
        fs::create_dir_all(library.join("Content")).unwrap();
        fs::create_dir_all(library.join("Data")).unwrap();
        fs::write(library.join("DedicatedServer.exe"), b"fixture").unwrap();
        fs::write(
            library.join("config_player.xml"),
            b"<config><contentpackages><regularpackages /></contentpackages></config>",
        )
        .unwrap();
        let cache = paths
            .steamcmd_root
            .join("steamapps/workshop/content/602960/123456789");
        fs::create_dir_all(&cache).unwrap();
        fs::write(
            cache.join("filelist.xml"),
            b"<contentpackage name=\"fixture\" />",
        )
        .unwrap();
        fs::write(cache.join("payload.bin"), b"independently copied package").unwrap();
        sync_game_installs(
            &paths,
            &[GameInstallSyncRecord {
                module_id: "barotrauma".to_owned(),
                install_root: library.to_string_lossy().into_owned(),
                install_state: InstallState::Installed,
                current_version: None,
                mark_verified: true,
            }],
        )
        .await
        .unwrap();
        Self {
            root,
            paths,
            descriptor,
        }
    }

    async fn create(&self) -> InstanceProvisioning {
        create_instance(
            &self.paths,
            &self.descriptor,
            CreateInstanceInput {
                name: "Concurrent configuration".to_owned(),
                module_id: "barotrauma".to_owned(),
            },
        )
        .await
        .unwrap()
    }

    fn input(&self, created: &InstanceProvisioning) -> UpdateInstanceInput {
        UpdateInstanceInput {
            id: created.summary.id.clone(),
            bind_ip: "0.0.0.0".to_owned(),
            auto_backup_on_stop: false,
            backup_retention_count: 10,
            settings_json: r#"{"server_name":"Prepared server","mod_workshop_ids":"123456789"}"#
                .to_owned(),
            ports: created.ports.clone(),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        cleanup_root(&self.root);
    }
}

async fn write_unrelated(paths: &StoragePaths) -> Result<(), sqlx::Error> {
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&paths.database_path)
            .busy_timeout(Duration::from_millis(100)),
    )
    .await?;
    let result = sqlx::query(
        "INSERT INTO modules(id,name,version) VALUES('parallel-package','Parallel','1')",
    )
    .execute(&mut connection)
    .await;
    connection.close().await?;
    result.map(|_| ())
}

#[tokio::test]
async fn creation_package_preparation_allows_unrelated_writes() {
    let fixture = Fixture::new().await;
    let mut gate = test_gate::register(&fixture.paths.database_path, Point::Packages);
    let paths = fixture.paths.clone();
    let descriptor = fixture.descriptor.clone();
    let worker = tokio::spawn(async move {
        create_instance(
            &paths,
            &descriptor,
            CreateInstanceInput {
                name: "Concurrent creation".to_owned(),
                module_id: "barotrauma".to_owned(),
            },
        )
        .await
    });
    gate.reached().await;
    let write = write_unrelated(&fixture.paths).await;
    gate.resume();
    let result = worker.await.unwrap().unwrap();
    write.expect("package preparation must release SQLite's writer");
    assert!(Path::new(&result.config_file_path).is_file());
}

#[tokio::test]
async fn explicit_creation_cancel_during_package_preparation_leaves_no_instance() {
    let fixture = Fixture::new().await;
    let mut gate = test_gate::register(&fixture.paths.database_path, Point::Packages);
    let cancellation = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let token = cancellation.clone();
    let paths = fixture.paths.clone();
    let descriptor = fixture.descriptor.clone();
    let worker = tokio::spawn(async move {
        create_instance_with_options(
            &paths,
            &descriptor,
            CreateInstanceInput {
                name: "Cancelled package creation".to_owned(),
                module_id: "barotrauma".to_owned(),
            },
            InstanceCreationOptions {
                cancellation: Some(token),
                ..Default::default()
            },
        )
        .await
    });
    gate.reached().await;
    cancellation.store(true, Ordering::Release);
    gate.resume();
    assert!(matches!(
        worker.await.unwrap(),
        Err(StorageError::InstanceCreationCancelled)
    ));
    assert!(list_instances(&fixture.paths).await.unwrap().is_empty());
    assert!(managed_instance_directories(&fixture.paths).is_empty());
    assert!(
        fixture
            .paths
            .games_root
            .join("barotrauma/DedicatedServer.exe")
            .is_file()
    );
}

#[tokio::test]
async fn update_package_preparation_allows_writes_and_keeps_instance_lease_on_cancellation() {
    let fixture = Fixture::new().await;
    let created = fixture.create().await;
    let input = fixture.input(&created);
    let mut gate = test_gate::register(&fixture.paths.database_path, Point::Packages);
    let paths = fixture.paths.clone();
    let caller = tokio::spawn(async move { update_instance(&paths, input).await });
    gate.reached().await;
    let write = write_unrelated(&fixture.paths).await;
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    assert!(matches!(
        acquire_instance_settings_mutation_lock(&fixture.paths, &created.summary.id),
        Err(StorageError::InstanceSettingsLocked { .. })
    ));
    gate.resume();
    write.expect("a cancelled waiter's package worker must not retain SQLite's writer");
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if acquire_instance_settings_mutation_lock(&fixture.paths, &created.summary.id).is_ok()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let config = Path::new(&created.config_file_path).parent().unwrap();
    assert_eq!(
        fs::read(config.join("LocalMods/123456789/payload.bin")).unwrap(),
        b"independently copied package"
    );
    assert!(
        fs::read_to_string(config.join("config_player.xml"))
            .unwrap()
            .contains("123456789")
    );
    let saved = read_instance_details(&fixture.paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&saved.settings_json).unwrap()["server_name"],
        "Prepared server"
    );
}

#[tokio::test]
async fn prestart_package_preparation_allows_unrelated_writes() {
    let fixture = Fixture::new().await;
    let created = fixture.create().await;
    update_instance(&fixture.paths, fixture.input(&created))
        .await
        .unwrap();
    let mut gate = test_gate::register(&fixture.paths.database_path, Point::Packages);
    let paths = fixture.paths.clone();
    let id = created.summary.id.clone();
    let worker =
        tokio::spawn(
            async move { materialize_instance_configuration_for_start(&paths, &id).await },
        );
    gate.reached().await;
    let write = write_unrelated(&fixture.paths).await;
    gate.resume();
    worker.await.unwrap().unwrap();
    write.expect("start preparation must release SQLite's writer");
}

#[tokio::test]
async fn configuration_reallocates_port_group_claimed_during_package_preparation() {
    let fixture = Fixture::new().await;
    let created = fixture.create().await;
    let peer = fixture.create().await;
    let mut input = fixture.input(&created);
    for (index, port) in input.ports.iter_mut().enumerate() {
        port.port = 28_000 + index as u16;
    }
    let peer_ports = input.ports.clone();
    let mut gate = test_gate::register(&fixture.paths.database_path, Point::Packages);
    let paths = fixture.paths.clone();
    let worker = tokio::spawn(async move { update_instance(&paths, input).await });
    gate.reached().await;
    let claim = update_instance_ports(&fixture.paths, &peer.summary.id, &peer_ports).await;
    gate.resume();
    claim.unwrap();
    let result = worker.await.unwrap().unwrap();
    assert_eq!(
        result
            .ports
            .iter()
            .map(|port| port.port)
            .collect::<Vec<_>>(),
        vec![28_002, 28_003]
    );
    let saved = read_instance_details(&fixture.paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(
        saved.ports.iter().map(|port| port.port).collect::<Vec<_>>(),
        vec![28_002, 28_003]
    );
    let peer_saved = read_instance_details(&fixture.paths, &peer.summary.id)
        .await
        .unwrap();
    assert_eq!(
        peer_saved
            .ports
            .iter()
            .map(|port| port.port)
            .collect::<Vec<_>>(),
        vec![28_000, 28_001]
    );
}

#[tokio::test]
async fn changed_activation_file_rejects_publication_and_rolls_back_database_and_templates() {
    let fixture = Fixture::new().await;
    let created = fixture.create().await;
    let config = Path::new(&created.config_file_path).parent().unwrap();
    let original_config = fs::read(&created.config_file_path).unwrap();
    let original_native = fs::read(config.join("serversettings.xml")).unwrap();
    let mut input = fixture.input(&created);
    input.bind_ip = "127.0.0.1".to_owned();
    let mut gate = test_gate::register(&fixture.paths.database_path, Point::PackagesReady);
    let paths = fixture.paths.clone();
    let worker = tokio::spawn(async move { update_instance(&paths, input).await });
    gate.reached().await;
    let concurrent_config = b"<config operator-change=\"preserve\" />";
    fs::write(config.join("config_player.xml"), concurrent_config).unwrap();
    gate.resume();
    let error = worker.await.unwrap().unwrap_err();
    assert!(
        matches!(error, StorageError::ModuleSupportMaterialization { .. }),
        "{error}"
    );
    assert_eq!(
        fs::read(config.join("config_player.xml")).unwrap(),
        concurrent_config
    );
    assert_eq!(
        fs::read(config.join("serversettings.xml")).unwrap(),
        original_native
    );
    assert_eq!(
        fs::read(&created.config_file_path).unwrap(),
        original_config
    );
    let saved = read_instance_details(&fixture.paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(saved.summary.bind_ip, "0.0.0.0");
}

#[tokio::test]
async fn runtime_change_during_package_preparation_rejects_configuration_publication() {
    let fixture = Fixture::new().await;
    let created = fixture.create().await;
    let original = fs::read(&created.config_file_path).unwrap();
    let config = Path::new(&created.config_file_path).parent().unwrap();
    let activation = fs::read(config.join("config_player.xml")).unwrap();
    let input = fixture.input(&created);
    let mut gate = test_gate::register(&fixture.paths.database_path, Point::Packages);
    let paths = fixture.paths.clone();
    let worker = tokio::spawn(async move { update_instance(&paths, input).await });
    gate.reached().await;
    let pool = crate::storage_db::connect_pool(&fixture.paths)
        .await
        .unwrap();
    sqlx::query("UPDATE instances SET status='starting' WHERE id=?1")
        .bind(&created.summary.id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    gate.resume();
    let error = worker.await.unwrap().unwrap_err();
    assert!(error.to_string().contains("runtime changed"), "{error}");
    assert_eq!(fs::read(&created.config_file_path).unwrap(), original);
    assert_eq!(
        fs::read(config.join("config_player.xml")).unwrap(),
        activation
    );
    assert!(acquire_instance_settings_mutation_lock(&fixture.paths, &created.summary.id).is_ok());
}
