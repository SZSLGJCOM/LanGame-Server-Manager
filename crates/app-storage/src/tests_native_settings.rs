use super::*;
use crate::instance_archive::test_gate::{self, Point};
use crate::instance_native_settings::{MORIA_PERMISSIONS_FILE, MoriaPermissionsSnapshot};

struct NativeSettingsFixture {
    root: PathBuf,
    paths: StoragePaths,
    created: InstanceProvisioning,
}

impl NativeSettingsFixture {
    async fn new(module_id: &str) -> Self {
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
        let install_root = root.join("fixture-install").join(module_id);
        let executable = install_root.join(&descriptor.process.as_ref().unwrap().executable);
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(executable, b"inert test fixture").unwrap();
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
                name: "Native settings fixture".to_owned(),
                module_id: module_id.to_owned(),
            },
        )
        .await
        .unwrap();
        Self {
            root,
            paths,
            created,
        }
    }

    async fn details(&self) -> InstanceDetails {
        read_instance_details(&self.paths, &self.created.summary.id)
            .await
            .unwrap()
    }

    fn native_permissions(&self) -> PathBuf {
        instance_private_runtime_root(&self.created).join(MORIA_PERMISSIONS_FILE)
    }

    fn input(&self, details: &InstanceDetails, settings: Value) -> UpdateInstanceInput {
        UpdateInstanceInput {
            id: details.summary.id.clone(),
            bind_ip: details.summary.bind_ip.clone(),
            auto_backup_on_stop: details.auto_backup_on_stop,
            backup_retention_count: details.backup_retention_count,
            settings_json: settings.to_string(),
            ports: details.ports.clone(),
        }
    }

    async fn status(&self, status: &str) {
        let pool = crate::storage_db::connect_pool(&self.paths).await.unwrap();
        sqlx::query("UPDATE instances SET status = ?2 WHERE id = ?1")
            .bind(&self.created.summary.id)
            .bind(status)
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    }
}

impl Drop for NativeSettingsFixture {
    fn drop(&mut self) {
        cleanup_root(&self.root);
    }
}

#[path = "tests_program_update_policy.rs"]
mod program_update_policy;

#[tokio::test]
async fn native_settings_moria_preserves_game_names_on_details_unrelated_save_and_start() {
    let fixture = NativeSettingsFixture::new("returntomoria").await;
    let original = fixture.details().await;
    let native = "\u{feff}Default = AllConstruction,AllStorage\r\nAlice = AllStorage\r\n";
    fs::write(fixture.native_permissions(), native.as_bytes()).unwrap();
    let current = fixture.details().await;
    assert_eq!(
        serde_json::from_str::<Value>(&current.settings_json).unwrap()["permissions_lines"],
        native
    );
    let mut unrelated: Value = serde_json::from_str(&original.settings_json).unwrap();
    unrelated["server_name"] = json!("Unrelated change");
    let saved = update_instance(&fixture.paths, fixture.input(&original, unrelated))
        .await
        .unwrap();
    assert_eq!(
        fs::read(fixture.native_permissions()).unwrap(),
        native.as_bytes()
    );
    assert_eq!(
        serde_json::from_str::<Value>(&saved.settings_json).unwrap()["permissions_lines"],
        native
    );

    let extended = format!("{native}Bob = Blocked\r\n");
    fs::write(fixture.native_permissions(), extended.as_bytes()).unwrap();
    materialize_instance_configuration_for_start(&fixture.paths, &fixture.created.summary.id)
        .await
        .unwrap();
    assert_eq!(
        fs::read(fixture.native_permissions()).unwrap(),
        extended.as_bytes()
    );
    let before_conflict = fs::read(&saved.config_file_path).unwrap();
    let stale = update_instance_if_current(
        &fixture.paths,
        fixture.input(&saved, serde_json::from_str(&saved.settings_json).unwrap()),
        &saved.settings_json,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        stale,
        StorageError::InstanceSettingsPreconditionFailed { .. }
    ));
    assert_eq!(fs::read(&saved.config_file_path).unwrap(), before_conflict);
    assert_eq!(
        fs::read(fixture.native_permissions()).unwrap(),
        extended.as_bytes()
    );
}

#[tokio::test]
async fn native_settings_moria_allows_acknowledged_removal_and_explicit_clear_while_stopped() {
    let fixture = NativeSettingsFixture::new("returntomoria").await;
    fs::write(
        fixture.native_permissions(),
        b"Default = AllStorage\nAlice = AllStorage\nBob = Blocked\n",
    )
    .unwrap();
    let mut baseline = fixture.details().await;
    for replacement in ["Default = AllStorage\nBob = Blocked\n", ""] {
        let mut draft: Value = serde_json::from_str(&baseline.settings_json).unwrap();
        draft["permissions_lines"] = json!(replacement);
        baseline = update_instance_if_current(
            &fixture.paths,
            fixture.input(&baseline, draft),
            &baseline.settings_json,
        )
        .await
        .unwrap();
        assert_eq!(
            fs::read(fixture.native_permissions()).unwrap(),
            replacement.as_bytes()
        );
        assert_eq!(
            serde_json::from_str::<Value>(&fixture.details().await.settings_json).unwrap()["permissions_lines"],
            replacement
        );
    }
}

#[tokio::test]
async fn native_settings_moria_rejects_sensitive_edits_in_active_or_error_states() {
    let fixture = NativeSettingsFixture::new("returntomoria").await;
    for status in ["starting", "running", "stopping", "error"] {
        fixture.status(status).await;
        let baseline = fixture.details().await;
        for (key, value) in [
            ("permissions_lines", "Default = Blocked"),
            ("upgrade_optional_dlc_array", "DurinsFolk"),
        ] {
            let before = fs::read(&baseline.config_file_path).unwrap();
            let mut draft: Value = serde_json::from_str(&baseline.settings_json).unwrap();
            draft[key] = json!(value);
            let error = update_instance_if_current(
                &fixture.paths,
                fixture.input(&baseline, draft),
                &baseline.settings_json,
            )
            .await
            .unwrap_err();
            assert!(
                matches!(error, StorageError::InvalidModuleSetting { field, .. } if field == key)
            );
            assert_eq!(fs::read(&baseline.config_file_path).unwrap(), before);
        }
        let mut ordinary: Value = serde_json::from_str(&baseline.settings_json).unwrap();
        ordinary["server_name"] = json!(format!("Allowed ordinary edit in {status}"));
        update_instance_if_current(
            &fixture.paths,
            fixture.input(&baseline, ordinary),
            &baseline.settings_json,
        )
        .await
        .unwrap();
    }
    fixture.status("stopped").await;
    let pool = crate::storage_db::connect_pool(&fixture.paths)
        .await
        .unwrap();
    sqlx::query("INSERT INTO instance_runs (instance_id,pid,status) VALUES (?1,12345,'running')")
        .bind(&fixture.created.summary.id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let baseline = fixture.details().await;
    assert!(baseline.active_run.is_some());
    let mut draft: Value = serde_json::from_str(&baseline.settings_json).unwrap();
    draft["permissions_lines"] = json!("Default = Blocked");
    assert!(matches!(
        update_instance_if_current(
            &fixture.paths,
            fixture.input(&baseline, draft),
            &baseline.settings_json
        )
        .await,
        Err(StorageError::InvalidModuleSetting { .. })
    ));
}

#[tokio::test]
async fn native_settings_scum_wipe_flags_require_stopped_and_are_rechecked_at_publication() {
    let fixture = NativeSettingsFixture::new("scum").await;
    for status in ["starting", "running", "stopping", "error"] {
        fixture.status(status).await;
        let baseline = fixture.details().await;
        for key in ["partial_wipe", "gold_wipe", "full_wipe"] {
            let mut draft: Value = serde_json::from_str(&baseline.settings_json).unwrap();
            draft["server_general"][key] = json!(true);
            let error = update_instance_if_current(
                &fixture.paths,
                fixture.input(&baseline, draft),
                &baseline.settings_json,
            )
            .await
            .unwrap_err();
            assert!(
                matches!(error, StorageError::InvalidModuleSetting { field, .. } if field == format!("server_general.{key}"))
            );
        }
    }
    fixture.status("stopped").await;
    let baseline = fixture.details().await;
    let before = fs::read(&baseline.config_file_path).unwrap();
    let mut draft: Value = serde_json::from_str(&baseline.settings_json).unwrap();
    draft["server_general"]["partial_wipe"] = json!(true);
    let input = fixture.input(&baseline, draft);
    let paths = fixture.paths.clone();
    let expected = baseline.settings_json.clone();
    let mut gate = test_gate::register(&fixture.paths.database_path, Point::NativeSettingsReady);
    let worker =
        tokio::spawn(async move { update_instance_if_current(&paths, input, &expected).await });
    gate.reached().await;
    fixture.status("error").await;
    gate.resume();
    assert!(
        matches!(worker.await.unwrap(), Err(StorageError::InvalidModuleSetting { field, .. }) if field == "server_general.partial_wipe")
    );
    assert_eq!(fs::read(&baseline.config_file_path).unwrap(), before);
}

#[tokio::test]
async fn native_settings_moria_rejects_native_changes_during_preparation() {
    let fixture = NativeSettingsFixture::new("returntomoria").await;
    let baseline = fixture.details().await;
    let before = fs::read(&baseline.config_file_path).unwrap();
    let mut draft: Value = serde_json::from_str(&baseline.settings_json).unwrap();
    draft["permissions_lines"] = json!("Default = Blocked");
    let input = fixture.input(&baseline, draft);
    let paths = fixture.paths.clone();
    let expected = baseline.settings_json;
    let mut gate = test_gate::register(&fixture.paths.database_path, Point::NativeSettingsReady);
    let worker =
        tokio::spawn(async move { update_instance_if_current(&paths, input, &expected).await });
    gate.reached().await;
    let external = "Default = AllStorage\nNewVisitor = AllStorage\n";
    fs::write(fixture.native_permissions(), external).unwrap();
    gate.resume();
    assert!(matches!(
        worker.await.unwrap(),
        Err(StorageError::InstanceSettingsPreconditionFailed { .. })
    ));
    assert_eq!(fs::read(&baseline.config_file_path).unwrap(), before);
    assert_eq!(
        fs::read(fixture.native_permissions()).unwrap(),
        external.as_bytes()
    );
}

#[test]
fn native_settings_moria_permissions_read_is_bounded_and_cas_detects_changes() {
    let root = unique_test_root();
    fs::create_dir_all(&root).unwrap();
    let path = root.join(MORIA_PERMISSIONS_FILE);
    fs::write(&path, b"Default = AllStorage").unwrap();
    let snapshot = MoriaPermissionsSnapshot::read("returntomoria", &root)
        .unwrap()
        .unwrap();
    fs::write(&path, b"Default = Blocked").unwrap();
    assert!(matches!(
        snapshot.verify_unchanged("fixture"),
        Err(StorageError::InstanceSettingsPreconditionFailed { .. })
    ));
    fs::write(&path, vec![b'x'; 256 * 1024 + 1]).unwrap();
    assert!(MoriaPermissionsSnapshot::read("returntomoria", &root).is_err());
    cleanup_root(&root);
}
