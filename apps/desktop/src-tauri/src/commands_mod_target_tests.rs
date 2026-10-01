use super::*;

#[test]
fn package_targets_require_the_marked_instance_runtime() {
    let root = std::env::temp_dir().join(format!(
        "langame-mod-target-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let instance_root = root.join("instance");
    let config_dir = instance_root.join("config");
    let private_root = instance_root.join("runtime");
    fs::create_dir_all(&config_dir).expect("create fixture config");
    let modules_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../modules");
    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: root.join("instances").to_string_lossy().into_owned(),
        games_root: root.join("games").to_string_lossy().into_owned(),
        modules_root: modules_root.to_string_lossy().into_owned(),
        steamcmd_root: root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let descriptors = discover_modules(&modules_root).expect("read module catalog");

    for (module_id, native_mods_path) in [
        ("palworld", "Pal/Binaries/Win64/Mods/Workshop"),
        ("conanexiles", "ConanSandbox/Mods"),
        ("squad", "SquadGame/Plugins/Mods"),
    ] {
        let descriptor = descriptors
            .iter()
            .find(|descriptor| descriptor.summary.id == module_id)
            .expect("module descriptor");
        let instance = InstanceDetails {
            summary: InstanceSummary {
                id: String::from("server"),
                name: String::from("Server"),
                module_id: String::from(module_id),
                status: InstanceStatus::Stopped,
                active_process_count: 0,
                bind_ip: String::from("0.0.0.0"),
                port_count: 0,
                autostart: false,
            },
            config_file_path: config_dir
                .join("instance.json")
                .to_string_lossy()
                .into_owned(),
            saves_path: instance_root.join("saves").to_string_lossy().into_owned(),
            backup_uses_declared_saves_path: false,
            auto_backup_on_stop: false,
            backup_retention_count: 10,
            settings_json: String::from("{}"),
            ports: vec![],
            active_run: None,
        };
        let mods = module_mods_spec_from_manifest(&descriptor.manifest_toml).expect("mod settings");
        let staging = mods.manual_staging.expect("native mod target");
        assert!(
            probe_instance_mod_install(&settings, descriptor, &instance).is_err(),
            "lost private runtime must not deploy Mods into shared files"
        );
        fs::create_dir_all(&private_root).expect("create fixture runtime");
        assert!(probe_instance_mod_install(&settings, descriptor, &instance).is_err());
        fs::write(private_root.join(".langame-private-runtime"), b"managed\n")
            .expect("mark fixture runtime");
        let probe = probe_instance_mod_install(&settings, descriptor, &instance)
            .expect("resolve marked instance runtime");
        assert_eq!(Path::new(&probe.install_root), private_root);
        let target = resolve_manual_mod_target_template(
            &staging.target_template,
            &ManualModTargetContext {
                install_root: Path::new(&probe.install_root),
                instance_root: &instance_root,
                config_dir: &config_dir,
                data_dir: &instance_root.join("data"),
                logs_dir: &instance_root.join("logs"),
                saves_dir: &instance_root.join("saves"),
                instance: &instance,
                settings_json: &Value::Null,
            },
        )
        .expect("resolve target");
        assert_eq!(target, private_root.join(native_mods_path));
        fs::remove_dir_all(&private_root).expect("remove fixture private runtime");
    }
    fs::remove_dir_all(root).expect("remove fixture");
}

#[cfg(windows)]
mod windows_paths {
    use super::*;

    struct FixtureRoot(PathBuf);

    impl FixtureRoot {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "langame-canonical-mod-target-{}",
                uuid::Uuid::new_v4().simple()
            ));
            fs::create_dir_all(&root).expect("create mod target fixture");
            Self(root)
        }
    }

    impl Drop for FixtureRoot {
        fn drop(&mut self) {
            assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn resolve_target(install_root: &Path, template: &str) -> PathBuf {
        let instance = InstanceDetails {
            summary: InstanceSummary {
                id: String::from("server"),
                name: String::from("Server"),
                module_id: String::from("palworld"),
                status: InstanceStatus::Stopped,
                active_process_count: 0,
                bind_ip: String::from("0.0.0.0"),
                port_count: 0,
                autostart: false,
            },
            config_file_path: String::new(),
            saves_path: String::new(),
            backup_uses_declared_saves_path: false,
            auto_backup_on_stop: false,
            backup_retention_count: 10,
            settings_json: String::from("{}"),
            ports: vec![],
            active_run: None,
        };
        resolve_manual_mod_target_template(
            template,
            &ManualModTargetContext {
                install_root,
                instance_root: install_root,
                config_dir: install_root,
                data_dir: install_root,
                logs_dir: install_root,
                saves_dir: install_root,
                instance: &instance,
                settings_json: &Value::Null,
            },
        )
        .expect("resolve mod target")
    }

    fn staging_target(target_path: PathBuf) -> ResolvedManualModTarget {
        ResolvedManualModTarget {
            instance_id: String::from("server"),
            module_id: String::from("palworld"),
            source_label: String::from("Local"),
            target_label: String::from("Mods"),
            target_path,
            accepts: vec![String::from("pak")],
            id_strategy: None,
        }
    }

    #[test]
    fn mod_target_preserves_windows_drive_and_unc_roots() {
        for root in [
            r"C:\LGSM\runtime",
            r"\\server\share\runtime",
            r"\\?\C:\LGSM\runtime",
            r"\\?\UNC\server\share\runtime",
        ] {
            let install_root = Path::new(root);
            let target = resolve_target(install_root, "{{paths.install_root}}/Pal/Mods/Workshop");
            let expected = install_root.join("Pal").join("Mods").join("Workshop");
            assert_eq!(target.as_os_str(), expected.as_os_str(), "root: {root}");
            assert!(target.is_absolute(), "root: {root}");
            assert_eq!(target.components().next(), install_root.components().next());
        }
    }

    #[test]
    fn canonical_install_root_mod_target_supports_native_writes_and_staging() {
        let fixture = FixtureRoot::new();
        let runtime = fixture.0.join("runtime");
        fs::create_dir_all(&runtime).expect("create fixture runtime");
        let canonical_runtime = runtime.canonicalize().expect("canonicalize runtime");
        assert!(matches!(
            canonical_runtime.components().next(),
            Some(std::path::Component::Prefix(prefix)) if prefix.kind().is_verbatim()
        ));
        let target = resolve_target(
            &canonical_runtime,
            "{{paths.install_root}}/Pal/Binaries/Win64/Mods/Workshop",
        );

        // Native I/O exposes invalid mixed separators that ordinary Path equality hides.
        fs::create_dir_all(&target).expect("create canonical mod target");
        fs::write(target.join("existing.pak"), b"existing package")
            .expect("write native mod target");
        let source = fixture.0.join("downloaded.pak");
        fs::write(&source, b"downloaded package").expect("write mod source");
        let result = stage_manual_mod_sources(staging_target(target.clone()), vec![source.clone()])
            .expect("stage into canonical mod target");
        assert_eq!(result.copied_file_count, 1);
        let native_target = runtime.join("Pal/Binaries/Win64/Mods/Workshop");
        assert_eq!(
            fs::read(native_target.join("downloaded.pak")).unwrap(),
            b"downloaded package"
        );
        assert_eq!(
            fs::read(native_target.join("existing.pak")).unwrap(),
            b"existing package"
        );
        assert_eq!(
            target.canonicalize().unwrap(),
            native_target.canonicalize().unwrap()
        );

        let escaping = resolve_target(&canonical_runtime, "{{paths.install_root}}/../escaped-mods");
        let error = stage_manual_mod_sources(staging_target(escaping), vec![source])
            .expect_err("normalization must preserve traversal for the staging validator");
        assert!(error.contains("relative traversal"), "{error}");
        assert!(!fixture.0.join("escaped-mods").exists());
    }
}
