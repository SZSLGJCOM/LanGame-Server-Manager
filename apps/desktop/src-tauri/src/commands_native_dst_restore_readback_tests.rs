use super::super::super::{creation, workspace_root};
use super::*;
use crate::state::DesktopState;
use app_core::{CreateInstanceInput, UpdateInstanceInput};
use app_storage::{StorageBootstrap, StoragePaths};
use serde_json::json;

struct StorageFixture {
    root: PathBuf,
    storage: StorageBootstrap,
    state: DesktopState,
    details: InstanceDetails,
}

impl StorageFixture {
    async fn new() -> Self {
        let root = crate::commands::tests::temp_test_dir("native-dst-readback");
        fs::create_dir_all(root.join("db")).unwrap();
        let source = workspace_root();
        let paths = StoragePaths {
            app_data_root: root.clone(),
            settings_path: root.join("settings.json"),
            database_path: root.join("db/lgs.db"),
            logs_root: root.join("logs"),
            modules_root: source.join("modules"),
            migrations_root: source.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances/.trash"),
        };
        app_storage::initialize_database(&paths).await.unwrap();
        let descriptors = app_modules::discover_modules(&paths.modules_root).unwrap();
        let descriptor = crate::commands::find_descriptor(&descriptors, "dontstarve").unwrap();
        app_storage::sync_modules(&paths, std::slice::from_ref(descriptor))
            .await
            .unwrap();
        // Inert registration only: this regression never launches a game or tool.
        crate::commands::tests::prepare_fake_registered_program(&paths, descriptor)
            .await
            .unwrap();
        let created = app_storage::create_instance(
            &paths,
            descriptor,
            CreateInstanceInput {
                name: "DST restored readback".into(),
                module_id: "dontstarve".into(),
            },
        )
        .await
        .unwrap();
        let details = app_storage::read_instance_details(&paths, &created.summary.id)
            .await
            .unwrap();
        let mut settings: Value = serde_json::from_str(&details.settings_json).unwrap();
        for (key, value) in [
            ("offline_cluster", json!(true)),
            ("enable_caves", json!(true)),
            ("master_world_size", json!("small")),
            ("caves_world_size", json!("small")),
        ] {
            settings[key] = value;
        }
        let details = app_storage::update_instance_if_current(
            &paths,
            UpdateInstanceInput {
                id: details.summary.id.clone(),
                bind_ip: details.summary.bind_ip.clone(),
                auto_backup_on_stop: false,
                backup_retention_count: 8,
                settings_json: settings.to_string(),
                ports: details.ports.clone(),
            },
            &details.settings_json,
        )
        .await
        .unwrap();
        let storage = StorageBootstrap {
            settings: paths.settings(),
            storage_status: paths.probe_status(),
            paths,
        };
        let state = DesktopState::from_storage(&storage);
        Self {
            root,
            storage,
            state,
            details,
        }
    }
}

impl Drop for StorageFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn dst_restored_readback_carries_predicted_settings_and_rejects_stale_or_changed_details() {
    let fixture = StorageFixture::new().await;
    let before = &fixture.details;
    let root = Path::new(&before.saves_path);
    let managed_ini = capture_managed_ini(root).unwrap();
    let samples: Value = serde_json::from_str(include_str!(
        "../../../../modules/dontstarve/reference-fixtures/2026-10-31-native_world_options_envelope_761239.json"
    ))
    .unwrap();
    for shard in ["Master", "Caves"] {
        let save = root.join(shard).join("save");
        fs::create_dir_all(save.join("session/OLD")).unwrap();
        fs::write(
            save.join("shardindex"),
            "return {session_id='OLD',enabled_mods={}}",
        )
        .unwrap();
        fs::write(save.join("session/OLD/0000000001"), b"saved world\0\x01").unwrap();
        fs::write(save.join("session/OLD/0000000001.meta"), b"world metadata").unwrap();
        let captured = samples["captured_variants"]
            .as_array()
            .unwrap()
            .iter()
            .find(|sample| {
                sample["format"] == "native_uncompressed_text_envelope"
                    && sample["retained"].as_str().unwrap().starts_with(shard)
            })
            .unwrap();
        fs::write(
            root.join(shard).join("worldgenoverride.lua"),
            captured["utf8"].as_str().unwrap(),
        )
        .unwrap();
    }
    let config = fs::read(&before.config_file_path).unwrap();
    let backup = app_storage::create_instance_backup(&fixture.storage.paths, &before.summary.id)
        .await
        .unwrap();
    let backup_root = Path::new(&backup.backup_path).join("saves");
    let expected = NoModsRestore::capture(
        &backup_root,
        &config,
        &fixture
            .storage
            .paths
            .modules_root
            .join("dontstarve/templates/clusters/main"),
        &managed_ini,
    )
    .unwrap();
    let original = inventory::save_inventory(&backup_root).unwrap();
    fs::write(
        root.join("Master/save/session/OLD/0000000001"),
        b"changed world",
    )
    .unwrap();
    let prepared = app_storage::prepare_instance_backup_restore(
        &fixture.storage.paths,
        &before.summary.id,
        &backup.backup_id,
    )
    .await
    .unwrap();
    let operation = fixture
        .state
        .begin_storage_context_operation("DST readback regression")
        .unwrap();
    let _instance = fixture
        .state
        .acquire_instance_mutation(&before.summary.id)
        .await;
    let started = SystemTime::now();
    // The real restore path performs publication and rematerialization. Its
    // empty Mod list returns before any Steam transport; no callback is mocked.
    crate::commands::commands_dst_import_operation::restore_dst_backup_locked(
        &fixture.state.install_locks,
        &fixture.storage,
        &operation,
        &before.summary.id,
        prepared,
        app_network::SourcePreference::ChinaFirst,
    )
    .await
    .unwrap();
    let finished = SystemTime::now();
    let after = app_storage::read_instance_details(&fixture.storage.paths, &before.summary.id)
        .await
        .unwrap();
    expected.verify_saves(root, &original).unwrap();
    expected
        .verify_configuration(
            &fs::read(&after.config_file_path).unwrap(),
            started,
            finished,
        )
        .unwrap();
    let program = creation::program_root(before).unwrap();
    assert!(
        settings_readback::verify_stopped(before, &after, &program).is_err(),
        "the creation-time baseline must reject intentional canonical restoration"
    );
    expected
        .verify_stopped_details(before, &after, &program)
        .unwrap();
    let reloaded = app_storage::read_instance_details(&fixture.storage.paths, &before.summary.id)
        .await
        .unwrap();
    settings_readback::verify_stopped(&after, &reloaded, &program).unwrap();

    let mut altered = after.clone();
    let mut settings: Value = serde_json::from_str(&altered.settings_json).unwrap();
    settings["unexpected_setting"] = json!(true);
    altered.settings_json = serde_json::to_string_pretty(&settings).unwrap();
    assert!(
        expected
            .verify_stopped_details(before, &altered, &program)
            .is_err()
    );
    for mutation in 0..4 {
        let mut altered = after.clone();
        match mutation {
            0 => altered.summary.id.push_str("-other"),
            1 => altered.ports[0].port += 1,
            2 => altered.config_file_path.push_str(".other"),
            3 => altered.saves_path.push_str(".other"),
            _ => unreachable!(),
        }
        assert!(
            expected
                .verify_stopped_details(before, &altered, &program)
                .is_err()
        );
    }
}
