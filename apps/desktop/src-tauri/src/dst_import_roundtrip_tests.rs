use super::*;

struct Fixture {
    root: PathBuf,
    storage: StorageBootstrap,
    state: DesktopState,
    instance: InstanceDetails,
}

impl Fixture {
    async fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("dst-import-roundtrip-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("db")).unwrap();
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let paths = app_storage::StoragePaths {
            app_data_root: root.clone(),
            settings_path: root.join("settings.json"),
            database_path: root.join("db/lgs.db"),
            logs_root: root.join("logs"),
            modules_root: workspace.join("modules"),
            migrations_root: workspace.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances/.trash"),
        };
        initialize_database(&paths).await.unwrap();
        let descriptors = discover_modules(&paths.modules_root).unwrap();
        let descriptor = find_descriptor(&descriptors, "dontstarve").unwrap();
        sync_modules(&paths, std::slice::from_ref(descriptor))
            .await
            .unwrap();
        crate::commands::tests::prepare_fake_registered_program(&paths, descriptor)
            .await
            .unwrap();
        let created = create_instance(
            &paths,
            descriptor,
            CreateInstanceInput {
                name: "DST import roundtrip".into(),
                module_id: "dontstarve".into(),
            },
        )
        .await
        .unwrap();
        let instance = read_instance_details(&paths, &created.summary.id)
            .await
            .unwrap();
        let storage = StorageBootstrap {
            settings: paths.settings(),
            storage_status: paths.probe_status(),
            paths,
        };
        Self {
            root,
            storage,
            state: DesktopState::default(),
            instance,
        }
    }

    fn target(&self) -> PathBuf {
        dontstarve_cluster_root_from_config_file_path(&self.instance.config_file_path)
    }

    async fn persist_mod(&mut self, id: &str, marker: &str) {
        let mut settings: Value = serde_json::from_str(&self.instance.settings_json).unwrap();
        settings["offline_cluster"] = json!(true);
        settings["enable_caves"] = json!(false);
        settings["master_modoverrides_lua"] = json!(format!(
            "return {{ ['workshop-{id}']={{enabled=true,configuration_options={{marker='{marker}',nested={{[4]='four',false}}}}}} }}"
        ));
        self.instance = app_storage::update_instance_if_current(
            &self.storage.paths,
            UpdateInstanceInput {
                id: self.instance.summary.id.clone(),
                bind_ip: self.instance.summary.bind_ip.clone(),
                auto_backup_on_stop: self.instance.auto_backup_on_stop,
                backup_retention_count: 8,
                settings_json: settings.to_string(),
                ports: self.instance.ports.clone(),
            },
            &self.instance.settings_json,
        )
        .await
        .unwrap();
    }

    fn source(&self) -> PathBuf {
        let root = self.root.join("incoming");
        write_saved_world(&root, "NEW");
        fs::write(root.join("Master/modoverrides.lua"), "return {['workshop-654321']={enabled=true,configuration_options={marker='incoming',nested={[4]='four',false}}}}").unwrap();
        root
    }

    async fn replace(
        &self,
        source: PathBuf,
        prepared: Option<app_storage::PreparedInstanceBackupRestore>,
    ) -> Result<DstWorldReplacement, String> {
        let operation = self
            .state
            .begin_storage_context_operation("DST roundtrip test")
            .unwrap();
        // This test substitutes only the external Steam transport. Parsing,
        // complete world copying, backup proofs and storage transactions are real.
        let _instance = self
            .state
            .acquire_instance_mutation(&self.instance.summary.id)
            .await;
        replace_dst_world_with_mod_preparation(
            &self.storage,
            &operation,
            &self.instance.summary.id,
            source,
            prepared,
            app_network::SourcePreference::ChinaFirst,
            prepared_transport,
        )
        .await
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn prepared_transport<'a>(
    _storage: &'a StorageBootstrap,
    _operation: &'a StorageContextOperationGuard,
    _details: &'a InstanceDetails,
    ids: &'a [String],
    preference: app_network::SourcePreference,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
    Box::pin(async move {
        assert_eq!(preference, app_network::SourcePreference::ChinaFirst);
        if ids
            .iter()
            .all(|id| matches!(id.as_str(), "123456" | "654321"))
        {
            Ok(())
        } else {
            Err("Unexpected Workshop request in the transport fixture.".into())
        }
    })
}

fn rejected_transport<'a>(
    _storage: &'a StorageBootstrap,
    _operation: &'a StorageContextOperationGuard,
    _details: &'a InstanceDetails,
    ids: &'a [String],
    preference: app_network::SourcePreference,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
    Box::pin(async move {
        assert!(
            !ids.is_empty(),
            "Fixture must exercise required Workshop preparation"
        );
        Err(format!("fixture route: {preference:?}"))
    })
}

#[tokio::test]
async fn dst_import_and_restore_preserve_source_preference_before_world_publication() {
    let mut fixture = Fixture::new().await;
    fixture.persist_mod("123456", "original").await;
    write_saved_world(&fixture.target(), "OLD");
    let backup =
        app_storage::create_instance_backup(&fixture.storage.paths, &fixture.instance.summary.id)
            .await
            .unwrap();
    let before = read_instance_details(&fixture.storage.paths, &fixture.instance.summary.id)
        .await
        .unwrap();
    for (locale, expected) in [
        (Some("zh-CN"), "ChinaFirst"),
        (Some("en-US"), "InternationalFirst"),
        (None, "InternationalFirst"),
    ] {
        for restore in [false, true] {
            let prepared = if restore {
                Some(
                    app_storage::prepare_instance_backup_restore(
                        &fixture.storage.paths,
                        &fixture.instance.summary.id,
                        &backup.backup_id,
                    )
                    .await
                    .unwrap(),
                )
            } else {
                None
            };
            let source = if restore {
                Path::new(&backup.backup_path).join("saves")
            } else {
                fixture.source()
            };
            let operation = fixture
                .state
                .begin_storage_context_operation("DST route fixture")
                .unwrap();
            let _instance = fixture
                .state
                .acquire_instance_mutation(&fixture.instance.summary.id)
                .await;
            let error = replace_dst_world_with_mod_preparation(
                &fixture.storage,
                &operation,
                &fixture.instance.summary.id,
                source,
                prepared,
                app_network::SourcePreference::from_locale(locale),
                rejected_transport,
            )
            .await
            .err()
            .unwrap();
            assert!(
                error.contains(&format!("fixture route: {expected}")),
                "{locale:?}, restore={restore}: {error}"
            );
            assert!(
                fixture
                    .target()
                    .join("Master/save/session/OLD/0000000001")
                    .is_file()
            );
            assert!(!fixture.target().join("Master/save/session/NEW").exists());
            assert_eq!(
                read_instance_details(&fixture.storage.paths, &fixture.instance.summary.id)
                    .await
                    .unwrap()
                    .settings_json,
                before.settings_json
            );
        }
    }
}

fn write_saved_world(cluster: &Path, session: &str) {
    let root = cluster.join("Master/save");
    fs::create_dir_all(root.join("session").join(session)).unwrap();
    fs::write(
        root.join("shardindex"),
        format!("return {{session_id='{session}',enabled_mods={{}}}}"),
    )
    .unwrap();
    fs::write(
        root.join("session").join(session).join("0000000001"),
        format!("world snapshot {session}"),
    )
    .unwrap();
    fs::write(
        root.join("session").join(session).join("0000000001.meta"),
        "world metadata",
    )
    .unwrap();
}

#[tokio::test]
async fn dst_import_backup_restores_canonical_mods_and_options_across_rematerialization() {
    let mut fixture = Fixture::new().await;
    fixture.persist_mod("123456", "original").await;
    write_saved_world(&fixture.target(), "OLD");
    let imported = fixture.replace(fixture.source(), None).await.unwrap();
    assert_eq!(imported.result.imported_workshop_mod_ids, ["654321"]);
    let backup_path = Path::new(&imported.safeguard.backup_path);
    assert!(backup_path.join("dst-instance.json").is_file());
    let backup_id = imported.safeguard.backup_id;
    let current = read_instance_details(&fixture.storage.paths, &fixture.instance.summary.id)
        .await
        .unwrap();
    app_storage::update_instance_if_current(
        &fixture.storage.paths,
        UpdateInstanceInput {
            id: current.summary.id,
            bind_ip: current.summary.bind_ip,
            auto_backup_on_stop: current.auto_backup_on_stop,
            backup_retention_count: 1,
            settings_json: current.settings_json.clone(),
            ports: current.ports,
        },
        &current.settings_json,
    )
    .await
    .unwrap();
    let prepared = app_storage::prepare_instance_backup_restore(
        &fixture.storage.paths,
        &fixture.instance.summary.id,
        &backup_id,
    )
    .await
    .unwrap();
    let restored = fixture
        .replace(backup_path.join("saves"), Some(prepared))
        .await
        .unwrap();
    assert!(
        backup_path.is_dir(),
        "The selected backup must survive retention=1 safeguard creation"
    );
    assert_eq!(
        restored.safeguard.backup_kind,
        app_core::InstanceBackupKind::PreRestore
    );
    assert!(restored.result.imported_master);
    assert!(
        fixture
            .target()
            .join("Master/save/session/OLD/0000000001")
            .is_file()
    );
    assert!(!fixture.target().join("Master/save/session/NEW").exists());
    let details = read_instance_details(&fixture.storage.paths, &fixture.instance.summary.id)
        .await
        .unwrap();
    let settings: Value = serde_json::from_str(&details.settings_json).unwrap();
    let raw = settings["master_modoverrides_lua"].as_str().unwrap();
    assert!(
        raw.contains("workshop-123456")
            && raw.contains("marker='original'")
            && raw.contains("[4]='four',false")
    );
    assert!(!raw.contains("workshop-654321"));
    assert_eq!(settings["shard_layout"], "standard");
    assert_eq!(settings["enable_caves"], false);
    assert_eq!(details.summary.bind_ip, fixture.instance.summary.bind_ip);
    let native_path = fixture.target().join("Master/modoverrides.lua");
    let native_before = fs::read(&native_path).unwrap();
    app_storage::update_instance_if_current(
        &fixture.storage.paths,
        UpdateInstanceInput {
            id: details.summary.id,
            bind_ip: details.summary.bind_ip,
            auto_backup_on_stop: details.auto_backup_on_stop,
            backup_retention_count: details.backup_retention_count,
            settings_json: details.settings_json.clone(),
            ports: details.ports,
        },
        &details.settings_json,
    )
    .await
    .unwrap();
    assert_eq!(fs::read(native_path).unwrap(), native_before);
}

#[tokio::test]
async fn dst_backup_restore_of_an_unstarted_world_clears_imported_saves_and_restores_mods() {
    let mut fixture = Fixture::new().await;
    fixture.persist_mod("123456", "before-first-start").await;
    let empty =
        app_storage::create_instance_backup(&fixture.storage.paths, &fixture.instance.summary.id)
            .await
            .unwrap();
    fixture.replace(fixture.source(), None).await.unwrap();
    let prepared = app_storage::prepare_instance_backup_restore(
        &fixture.storage.paths,
        &fixture.instance.summary.id,
        &empty.backup_id,
    )
    .await
    .unwrap();
    let result = fixture
        .replace(Path::new(&empty.backup_path).join("saves"), Some(prepared))
        .await
        .unwrap();
    assert!(!result.result.imported_master);
    assert!(!fixture.target().join("Master/save").exists());
    let current = read_instance_details(&fixture.storage.paths, &fixture.instance.summary.id)
        .await
        .unwrap();
    let settings: Value = serde_json::from_str(&current.settings_json).unwrap();
    assert!(
        settings["master_modoverrides_lua"]
            .as_str()
            .unwrap()
            .contains("before-first-start")
    );
    assert_eq!(settings["enable_caves"], false);
}

#[tokio::test]
async fn dst_confirmed_backup_restore_rejects_changed_canonical_sidecar_without_publishing() {
    let mut fixture = Fixture::new().await;
    fixture.persist_mod("123456", "original").await;
    write_saved_world(&fixture.target(), "OLD");
    let imported = fixture.replace(fixture.source(), None).await.unwrap();
    let prepared = app_storage::prepare_instance_backup_restore(
        &fixture.storage.paths,
        &fixture.instance.summary.id,
        &imported.safeguard.backup_id,
    )
    .await
    .unwrap();
    let sidecar = Path::new(&imported.safeguard.backup_path).join("dst-instance.json");
    let changed = fs::read_to_string(&sidecar)
        .unwrap()
        .replace("original", "modified");
    fs::write(sidecar, changed).unwrap();
    let before = read_instance_details(&fixture.storage.paths, &fixture.instance.summary.id)
        .await
        .unwrap();
    let error = fixture
        .replace(
            Path::new(&imported.safeguard.backup_path).join("saves"),
            Some(prepared),
        )
        .await
        .err()
        .unwrap();
    assert!(error.contains("changed after preview"), "{error}");
    assert!(
        fixture
            .target()
            .join("Master/save/session/NEW/0000000001")
            .is_file()
    );
    assert_eq!(
        read_instance_details(&fixture.storage.paths, &fixture.instance.summary.id)
            .await
            .unwrap()
            .settings_json,
        before.settings_json
    );
}
