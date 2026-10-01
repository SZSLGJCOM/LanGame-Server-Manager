use super::*;
use app_core::{InstallState, InstanceStatus, InstanceSummary};

#[test]
fn generated_entrypoints_are_preparable_only_when_their_source_program_exists() {
    let modules_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules");
    let modules = app_modules::discover_modules(modules_root).expect("module catalog");
    for (module_id, source_name) in [
        ("barotrauma", "DedicatedServer.exe"),
        ("romestead", "Server.exe"),
    ] {
        let root = crate::test_support::unique_test_root();
        let install_root = root.join("games").join(module_id);
        let config_dir = root.join("instances/one/config");
        fs::create_dir_all(&install_root).expect("create installation");
        fs::create_dir_all(&config_dir).expect("create configuration directory");
        let settings = AppSettings {
            games_root: root.join("games").to_string_lossy().into_owned(),
            ..AppSettings::default()
        };
        let descriptor = modules
            .iter()
            .find(|module| module.summary.id == module_id)
            .expect("supported module");
        let mut module = ModuleDetails {
            summary: descriptor.summary.clone(),
            schema_json: descriptor.schema_json.clone(),
            default_ports: descriptor.default_ports.clone(),
            install: descriptor.install.clone(),
            process: descriptor.process.clone(),
            workshop: descriptor.workshop.clone(),
            mods: None,
            runtime: descriptor.runtime.clone(),
        };
        module.summary.install_state = InstallState::Installed;
        let instance = InstanceDetails {
            summary: InstanceSummary {
                id: String::from("one"),
                name: String::from("Launch preparation"),
                module_id: String::from(module_id),
                status: InstanceStatus::Stopped,
                active_process_count: 0,
                bind_ip: String::from("127.0.0.1"),
                port_count: 0,
                autostart: false,
            },
            config_file_path: config_dir
                .join("instance.json")
                .to_string_lossy()
                .into_owned(),
            saves_path: root.join("saves").to_string_lossy().into_owned(),
            backup_uses_declared_saves_path: true,
            auto_backup_on_stop: false,
            backup_retention_count: 10,
            settings_json: String::from(r#"{"extra_launch_args":""}"#),
            ports: vec![],
            active_run: None,
        };

        let missing = build_launch_plan(&settings, &module, &instance).expect("missing source");
        assert!(!missing.ready_to_launch, "{module_id}");
        assert!(missing.validation_issues.iter().any(|issue| {
            issue.code == "launch_executable_missing" && issue.severity == "error"
        }));

        fs::write(install_root.join(source_name), []).expect("create source program fixture");
        if module_id == "barotrauma" {
            for directory in ["Content", "Data"] {
                let incomplete = build_launch_plan(&settings, &module, &instance)
                    .expect("incomplete server package");
                assert!(!incomplete.ready_to_launch, "missing {directory}");
                fs::create_dir_all(install_root.join(directory)).expect("create required content");
            }
            let nested_config = install_root.join("instance/config");
            fs::create_dir_all(&nested_config).expect("create overlapping configuration");
            let mut overlapping = instance.clone();
            overlapping.config_file_path = nested_config
                .join("instance.json")
                .to_string_lossy()
                .into_owned();
            let overlap = build_launch_plan(&settings, &module, &overlapping)
                .expect("overlapping runtime roots");
            assert!(!overlap.ready_to_launch);
        }
        let preparable =
            build_launch_plan(&settings, &module, &instance).expect("installed source");
        assert!(preparable.ready_to_launch, "{module_id}");
        assert!(
            !preparable.executable_exists,
            "preview must not write files"
        );
        assert_eq!(preparable.validation_issues.len(), 1);
        assert_eq!(
            preparable.validation_issues[0].code,
            "launch_preparation_required"
        );
        assert_eq!(preparable.validation_issues[0].severity, "info");

        module.process.as_mut().expect("process").executable = String::from("custom-server.exe");
        let custom = build_launch_plan(&settings, &module, &instance).expect("custom executable");
        assert!(!custom.ready_to_launch);
        assert!(
            !custom
                .validation_issues
                .iter()
                .any(|issue| issue.code == "launch_preparation_required")
        );
        fs::remove_dir_all(root).expect("remove preparation fixture");
    }
}

#[cfg(windows)]
#[test]
fn minecraft_launch_normalizes_verbatim_jar_arguments_and_still_rejects_missing_files() {
    let root = crate::test_support::unique_test_root();
    let install_root = root.join("games/minecraft");
    let config_dir = root.join("instances/one/config");
    fs::create_dir_all(install_root.join("jre/bin")).expect("create Java fixture directory");
    fs::create_dir_all(&config_dir).expect("create configuration directory");
    fs::write(install_root.join("jre/bin/java.exe"), b"never executed")
        .expect("create Java fixture");
    let jar = install_root.join("server.jar");
    let jar_bytes = b"synthetic jar, never executed";
    fs::write(&jar, jar_bytes).expect("create jar fixture");
    let canonical = fs::canonicalize(&install_root).expect("canonical installation root");
    assert!(
        matches!(canonical.components().next(), Some(std::path::Component::Prefix(prefix))
        if matches!(prefix.kind(), std::path::Prefix::VerbatimDisk(_)))
    );

    let modules_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules");
    let modules = app_modules::discover_modules(modules_root).expect("module catalog");
    let descriptor = modules
        .iter()
        .find(|module| module.summary.id == "minecraft")
        .expect("Minecraft module");
    let mut module = ModuleDetails {
        summary: descriptor.summary.clone(),
        schema_json: descriptor.schema_json.clone(),
        default_ports: descriptor.default_ports.clone(),
        install: descriptor.install.clone(),
        process: descriptor.process.clone(),
        workshop: descriptor.workshop.clone(),
        mods: None,
        runtime: descriptor.runtime.clone(),
    };
    module.summary.install_state = InstallState::Installed;
    let url_argument = String::from("--status-url=https://example.invalid/health");
    let text_argument = format!("--display={}/unchanged", canonical.display());
    module
        .process
        .as_mut()
        .expect("process")
        .args_template
        .extend([url_argument.clone(), text_argument.clone()]);
    let settings = AppSettings {
        games_root: root.join("games").to_string_lossy().into_owned(),
        ..AppSettings::default()
    };
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("one"),
            name: String::from("Verbatim Java launch"),
            module_id: String::from("minecraft"),
            status: InstanceStatus::Stopped,
            active_process_count: 0,
            bind_ip: String::from("127.0.0.1"),
            port_count: 0,
            autostart: false,
        },
        config_file_path: config_dir
            .join("instance.json")
            .to_string_lossy()
            .into_owned(),
        saves_path: root.join("saves").to_string_lossy().into_owned(),
        backup_uses_declared_saves_path: true,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: String::from(r#"{"memory_min_mb":1024,"memory_max_mb":2048}"#),
        ports: vec![],
        active_run: None,
    };
    let plan = build_launch_plan_with_override(&settings, &module, &instance, canonical.to_str())
        .expect("verbatim installation plan");
    assert!(plan.ready_to_launch, "{:?}", plan.validation_issues);
    assert!(
        !plan.executable_path.starts_with(r"\\?\"),
        "Java cannot discover its runtime image through a verbatim executable path"
    );
    assert_eq!(
        fs::canonicalize(&plan.executable_path).expect("resolved Java executable"),
        canonical.join("jre/bin/java.exe")
    );
    let jar_argument = &plan
        .args
        .windows(2)
        .find(|pair| pair[0] == "-jar")
        .expect("Java jar argument")[1];
    assert!(!jar_argument.starts_with(r"\\?\"));
    assert_eq!(
        fs::canonicalize(jar_argument).expect("resolved Java jar argument"),
        canonical.join("server.jar")
    );
    assert_eq!(
        fs::read(jar_argument).expect("open actual Java argument"),
        jar_bytes
    );
    assert!(plan.args.contains(&url_argument));
    assert!(plan.args.contains(&text_argument));
    let ordinary =
        build_launch_plan_with_override(&settings, &module, &instance, install_root.to_str())
            .expect("ordinary installation plan");
    assert!(ordinary.ready_to_launch, "{:?}", ordinary.validation_issues);
    assert!(
        ordinary
            .args
            .contains(&format!("{}/server.jar", install_root.display()))
    );
    let relative_args = vec![String::from("-jar"), String::from("server.jar")];
    assert!(collect_required_argument_file_issues(&canonical, &relative_args).is_empty());

    fs::remove_file(&jar).expect("remove jar fixture");
    let missing =
        build_launch_plan_with_override(&settings, &module, &instance, canonical.to_str())
            .expect("missing jar plan");
    assert!(!missing.ready_to_launch);
    assert_eq!(missing.validation_issues.len(), 1);
    assert_eq!(
        missing.validation_issues[0].code,
        "launch_required_file_missing"
    );
    assert_eq!(missing.validation_issues[0].severity, "error");
    fs::remove_dir_all(root).expect("remove Java fixture");
}

#[cfg(windows)]
#[test]
fn java_path_compatibility_preserves_distinct_names_and_other_executables() {
    let root = crate::test_support::unique_test_root();
    fs::create_dir_all(root.join("ordinary")).expect("create ordinary directory");
    let root = fs::canonicalize(root).expect("canonical test root");
    let special = root.join("ordinary.");
    fs::create_dir(&special).expect("create distinct verbatim directory");
    for directory in [root.join("ordinary"), special.clone()] {
        fs::write(
            directory.join("java.exe"),
            directory.to_string_lossy().as_bytes(),
        )
        .expect("create distinct Java fixture");
    }
    let special_java = special.join("java.exe");
    assert_eq!(
        compatible_java_executable_path(special_java.clone()),
        special_java
    );
    let ordinary_java = root.join("ordinary/java.exe");
    let compatible = compatible_java_executable_path(ordinary_java.clone());
    assert!(!compatible.to_string_lossy().starts_with(r"\\?\"));
    assert_eq!(
        fs::read(compatible).unwrap(),
        fs::read(ordinary_java).unwrap()
    );
    for name in ["JAVA.EXE", "JavaW.exe"] {
        let executable = root.join(name);
        fs::write(&executable, b"Java filename variants").expect("create Java variant");
        let compatible = compatible_java_executable_path(executable.clone());
        assert!(!compatible.to_string_lossy().starts_with(r"\\?\"));
        assert_eq!(
            fs::canonicalize(&compatible).unwrap(),
            fs::canonicalize(&executable).unwrap()
        );
        assert_eq!(
            compatible_java_executable_path(compatible.clone()),
            compatible
        );
    }
    let root_length = root
        .to_str()
        .unwrap()
        .strip_prefix(r"\\?\")
        .unwrap()
        .encode_utf16()
        .count();
    let boundary = root.join("x".repeat(247 - root_length - "\\\\java.exe".len()));
    fs::create_dir(&boundary).expect("create long Java directory");
    let boundary_java = boundary.join("java.exe");
    fs::write(&boundary_java, b"long Java path").expect("create long Java fixture");
    assert_eq!(
        compatible_java_executable_path(boundary_java.clone()),
        boundary_java
    );

    let other = root.join("server.exe");
    fs::write(&other, b"not Java").expect("create other executable");
    assert_eq!(compatible_java_executable_path(other.clone()), other);
    for unchanged in [
        root.join("missing/java.exe"),
        PathBuf::from(r"\\?\UNC\server\share\java.exe"),
        root.join("long".repeat(90)).join("java.exe"),
    ] {
        assert_eq!(
            compatible_java_executable_path(unchanged.clone()),
            unchanged
        );
    }
    fs::remove_dir_all(root).expect("remove Java path fixtures");
}
