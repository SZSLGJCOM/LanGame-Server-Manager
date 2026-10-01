use super::*;

#[path = "tests_ark_game_logs.rs"]
mod game_logs;

struct MapFixture {
    descriptor: app_modules::ModuleDescriptor,
    root: PathBuf,
    paths: StoragePaths,
    created: InstanceProvisioning,
    details: InstanceDetails,
    entrypoint: PathBuf,
}

impl MapFixture {
    async fn new(module_id: &str) -> Self {
        let root = unique_test_root();
        let paths = test_paths(&root);
        // Use the real module's groups and validation, with only its filesystem
        // root redirected. The synthetic program is never executed.
        let mut descriptor = app_modules::discover_modules(repo_root().join("modules"))
            .unwrap()
            .into_iter()
            .find(|descriptor| descriptor.summary.id == module_id)
            .unwrap();
        descriptor.root = root.join("modules").join(module_id);
        if module_id == "arksurvivalevolved" {
            prepare_ark_environment(&root, &descriptor);
        } else {
            prepare_ark_ascended_environment(&root, &descriptor);
        }
        let entrypoint = PathBuf::from(&descriptor.process.as_ref().unwrap().executable);
        let source_program = paths
            .games_root
            .join(&descriptor.install.as_ref().unwrap().shared_game_dir);
        let synthetic_entrypoint = source_program.join(&entrypoint);
        fs::create_dir_all(synthetic_entrypoint.parent().unwrap()).unwrap();
        fs::write(
            &synthetic_entrypoint,
            format!("Synthetic {module_id} native entrypoint fixture; never execute.\n"),
        )
        .unwrap();
        initialize_database(&paths).await.unwrap();
        sync_modules(&paths, std::slice::from_ref(&descriptor))
            .await
            .unwrap();
        let created = create_instance(
            &paths,
            &descriptor,
            CreateInstanceInput {
                name: "Storage map lifecycle".into(),
                module_id: module_id.into(),
            },
        )
        .await
        .unwrap();
        let details = read_instance_details(&paths, &created.summary.id)
            .await
            .unwrap();
        Self {
            descriptor,
            root,
            paths,
            created,
            details,
            entrypoint,
        }
    }

    fn settings(&self) -> Value {
        serde_json::from_str(&self.details.settings_json).unwrap()
    }

    fn input(&self, settings: &Value) -> UpdateInstanceInput {
        UpdateInstanceInput {
            id: self.details.summary.id.clone(),
            bind_ip: self.details.summary.bind_ip.clone(),
            auto_backup_on_stop: false,
            backup_retention_count: 10,
            settings_json: settings.to_string(),
            ports: self.details.ports.clone(),
        }
    }

    async fn save(&mut self, settings: &Value) {
        self.details = update_instance(&self.paths, self.input(settings))
            .await
            .unwrap();
    }

    async fn add_two_maps(&mut self) {
        let mut settings = self.settings();
        let asa = self.details.summary.module_id == "arksurvivalascended";
        settings["cluster_id"] = json!("");
        settings["additional_maps"] = json!([
            {"id":"desert", "map_name":if asa {"ScorchedEarth_WP"} else {"ScorchedEarth_P"}, "name":"Desert", "enabled":true},
            {"id":"center", "map_name":if asa {"TheCenter_WP"} else {"TheCenter"}, "name":"Center", "enabled":true}
        ]);
        self.save(&settings).await;
    }

    fn assert_distinct_ports(&self, expected_count: usize) {
        assert_eq!(self.details.ports.len(), expected_count);
        let endpoints = self
            .details
            .ports
            .iter()
            .map(|port| (&port.protocol, port.port))
            .collect::<BTreeSet<_>>();
        assert_eq!(
            endpoints.len(),
            expected_count,
            "Each map's actual endpoints must remain independent"
        );
        assert!(self.details.ports.iter().all(|port| port.port != 0));
        if self.details.summary.module_id == "arksurvivalevolved" {
            for key in ["main", "map-desert", "map-center"] {
                let projected =
                    app_core::ark_maps::project_process(&self.details, Some(key)).unwrap();
                let game = projected
                    .ports
                    .iter()
                    .find(|port| port.name == "game")
                    .unwrap();
                let peer = projected
                    .ports
                    .iter()
                    .find(|port| port.name == "peer")
                    .unwrap();
                assert_eq!(
                    peer.port,
                    game.port + 1,
                    "ASE game/peer allocation must remain paired"
                );
            }
        }
    }
}

impl Drop for MapFixture {
    fn drop(&mut self) {
        cleanup_root(&self.root);
    }
}

#[tokio::test]
async fn ark_map_default_transfer_directory_materialization_and_report_keep_the_primary_world_root()
{
    for edition in ["arksurvivalevolved", "arksurvivalascended"] {
        let mut fixture = MapFixture::new(edition).await;
        fixture.add_two_maps().await;
        let program = instance_private_runtime_root(&fixture.created);
        let expected = program
            .join("ShooterGame/Saved")
            .join(&fixture.created.summary.id)
            .join("cluster");
        assert_eq!(fixture.settings()["cluster_directory"], "");
        assert!(
            expected.is_dir(),
            "{edition}: saving map settings must materialize the primary transfer directory"
        );
        assert!(!program.join("ShooterGame/Saved/cluster").exists());
        fs::remove_dir_all(&expected).unwrap();
        materialize_instance_configuration(&fixture.paths, &fixture.created.summary.id)
            .await
            .unwrap();
        assert!(
            expected.is_dir(),
            "{edition}: normal materialization must recreate the same transfer root"
        );
        let expected = fs::canonicalize(expected).unwrap();
        for map in app_core::ark_maps::processes(&fixture.details).unwrap() {
            let projected =
                app_core::ark_maps::project_process(&fixture.details, Some(&map.process_key))
                    .unwrap();
            let settings: Value = serde_json::from_str(&projected.settings_json).unwrap();
            assert_eq!(
                fs::canonicalize(settings["cluster_directory"].as_str().unwrap()).unwrap(),
                expected,
                "{edition}: every map must use the primary world's transfer root"
            );
        }
        let report = read_ark_cluster_report(&fixture.paths, &fixture.created.summary.id)
            .await
            .unwrap();
        assert!(!report.start_blocked, "{edition}: {:?}", report.issues);
        assert_eq!(
            fs::canonicalize(report.cluster_directory.unwrap()).unwrap(),
            expected
        );
        assert_eq!(
            report.members.len(),
            1,
            "one managed instance owns every native map process"
        );
        assert_eq!(
            fs::canonicalize(report.members[0].cluster_directory.as_deref().unwrap()).unwrap(),
            expected
        );
    }
}

#[tokio::test]
async fn ark_map_updates_keep_one_install_and_stable_ports_and_preserve_removed_worlds() {
    for edition in ["arksurvivalevolved", "arksurvivalascended"] {
        let mut fixture = MapFixture::new(edition).await;
        let program = instance_private_runtime_root(&fixture.created);
        let instance_root = Path::new(&fixture.created.config_file_path)
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        assert_eq!(
            resolve_instance_runtime_root(instance_root).unwrap(),
            program
        );
        let original_program = fs::read(program.join(&fixture.entrypoint)).unwrap();
        assert_eq!(
            original_program,
            format!("Synthetic {edition} native entrypoint fixture; never execute.\n").as_bytes()
        );
        let original_install =
            read_instance_program_install(&fixture.paths, &fixture.created.summary.id)
                .await
                .unwrap()
                .expect("instance program installation");
        fixture.add_two_maps().await;
        fixture.assert_distinct_ports(12);
        let ports = serde_json::to_value(&fixture.details.ports).unwrap();
        let cluster_id = fixture.settings()["cluster_id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            !cluster_id.is_empty(),
            "Adding maps must establish a persistent cluster identity"
        );
        let retained = program
            .join("ShooterGame/Saved")
            .join(format!("{}-map-desert", fixture.created.summary.id))
            .join("retained.ark");
        fs::create_dir_all(retained.parent().unwrap()).unwrap();
        fs::write(&retained, b"retained-map-data").unwrap();

        let mut settings = fixture.settings();
        settings["additional_maps"][0]["enabled"] = json!(false);
        fixture.save(&settings).await;
        assert_eq!(serde_json::to_value(&fixture.details.ports).unwrap(), ports);
        assert_eq!(
            app_core::ark_maps::processes(&fixture.details)
                .unwrap()
                .len(),
            2
        );
        settings["additional_maps"][0]["enabled"] = json!(true);
        fixture.save(&settings).await;
        assert_eq!(serde_json::to_value(&fixture.details.ports).unwrap(), ports);
        assert_eq!(fixture.settings()["cluster_id"], cluster_id);
        assert_eq!(
            app_core::ark_maps::processes(&fixture.details)
                .unwrap()
                .len(),
            3
        );

        let before_rejected = fixture.details.settings_json.clone();
        let mut changed_package = fixture.settings();
        changed_package["additional_maps"][0]["map_name"] =
            json!(if edition == "arksurvivalevolved" {
                "Ragnarok"
            } else {
                "Ragnarok_WP"
            });
        assert!(
            update_instance(&fixture.paths, fixture.input(&changed_package))
                .await
                .is_err()
        );
        assert_eq!(
            read_instance_details(&fixture.paths, &fixture.created.summary.id)
                .await
                .unwrap()
                .settings_json,
            before_rejected
        );

        let run_log = fixture.root.join("test-running.log");
        fs::write(&run_log, b"synthetic registered process, not executed").unwrap();
        let run = record_started_test_instance(
            &fixture.paths,
            &fixture.created.summary.id,
            9_999_997,
            &run_log.to_string_lossy(),
        )
        .await
        .unwrap();
        let mut changed_topology = fixture.settings();
        changed_topology["additional_maps"][0]["enabled"] = json!(false);
        assert!(
            update_instance(&fixture.paths, fixture.input(&changed_topology))
                .await
                .is_err(),
            "Running map topology cannot be changed"
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
        fixture.details = read_instance_details(&fixture.paths, &fixture.created.summary.id)
            .await
            .unwrap();
        assert_eq!(fixture.details.settings_json, before_rejected);

        let mut removed = fixture.settings();
        removed["additional_maps"].as_array_mut().unwrap().remove(0);
        fixture.save(&removed).await;
        assert_eq!(fixture.details.ports.len(), 8);
        assert!(
            !fixture
                .details
                .ports
                .iter()
                .any(|port| port.name.starts_with("map-desert-"))
        );
        assert_eq!(fs::read(&retained).unwrap(), b"retained-map-data");
        assert_eq!(
            fs::read(program.join(&fixture.entrypoint)).unwrap(),
            original_program
        );
        let installed = read_instance_program_install(&fixture.paths, &fixture.created.summary.id)
            .await
            .unwrap()
            .expect("instance program installation");
        assert_eq!(
            installed.install.install_root, program,
            "Adding maps must retain the one program root"
        );
        assert_eq!(
            installed.install.id, original_install.install.id,
            "Adding maps must retain the same installation record"
        );
    }
}

#[tokio::test]
async fn ark_backups_retain_main_and_removed_map_worlds_for_both_editions() {
    for module_id in ["arksurvivalevolved", "arksurvivalascended"] {
        let root = unique_test_root();
        let paths = test_paths(&root);
        let descriptor = if module_id == "arksurvivalevolved" {
            let descriptor = ark_test_descriptor(&root);
            prepare_ark_environment(&root, &descriptor);
            descriptor
        } else {
            let descriptor = ark_ascended_test_descriptor(&root);
            prepare_ark_ascended_environment(&root, &descriptor);
            descriptor
        };
        initialize_database(&paths).await.unwrap();
        sync_modules(&paths, std::slice::from_ref(&descriptor))
            .await
            .unwrap();
        let created = create_instance(
            &paths,
            &descriptor,
            CreateInstanceInput {
                name: "Retained ARK maps".into(),
                module_id: module_id.into(),
            },
        )
        .await
        .unwrap();
        let mut details = read_instance_details(&paths, &created.summary.id)
            .await
            .unwrap();
        let saved = instance_private_runtime_root(&created).join("ShooterGame/Saved");
        let main_relative = PathBuf::from(&created.summary.id).join("world.ark");
        let extra_relative =
            PathBuf::from(format!("{}-map-retained", created.summary.id)).join("world.ark");
        for (relative, bytes) in [
            (&main_relative, b"main-world".as_slice()),
            (&extra_relative, b"retained-map-world".as_slice()),
        ] {
            fs::create_dir_all(saved.join(relative).parent().unwrap()).unwrap();
            fs::write(saved.join(relative), bytes).unwrap();
        }
        let mut settings: Value = serde_json::from_str(&details.settings_json).unwrap();
        settings["additional_maps"] = json!([{
            "id": "retained", "name": "Retained map", "enabled": false,
            "map_name": if module_id == "arksurvivalevolved" { "ScorchedEarth_P" } else { "ScorchedEarth_WP" }
        }]);
        for remove_map in [false, true] {
            if remove_map {
                settings["additional_maps"] = json!([]);
            }
            details = update_instance(
                &paths,
                UpdateInstanceInput {
                    id: details.summary.id.clone(),
                    bind_ip: details.summary.bind_ip.clone(),
                    auto_backup_on_stop: false,
                    backup_retention_count: 10,
                    settings_json: settings.to_string(),
                    ports: details.ports,
                },
            )
            .await
            .unwrap();
            let backup = create_instance_backup(&paths, &created.summary.id)
                .await
                .unwrap();
            let backup_saves = Path::new(&backup.backup_path).join("saves");
            // The old main-world-only backup omitted this sibling map, both
            // while disabled and once it disappeared from configuration.
            assert_eq!(
                fs::read(backup_saves.join(&extra_relative)).unwrap(),
                b"retained-map-world",
                "{module_id}: retained map must survive configuration removal"
            );
            assert_eq!(
                fs::read(backup_saves.join(&main_relative)).unwrap(),
                b"main-world"
            );
            fs::write(saved.join(&extra_relative), b"changed-extra").unwrap();
            fs::write(saved.join(&main_relative), b"changed-main").unwrap();
            restore_instance_backup(&paths, &created.summary.id, &backup.backup_id)
                .await
                .unwrap();
            assert_eq!(
                fs::read(saved.join(&extra_relative)).unwrap(),
                b"retained-map-world"
            );
            assert_eq!(fs::read(saved.join(&main_relative)).unwrap(), b"main-world");
        }
        cleanup_root(&root);
    }
}
