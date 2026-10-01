use super::*;

#[cfg(windows)]
use app_core::{InstallState, InstanceStatus, InstanceSummary};

#[cfg(windows)]
struct Fixture(PathBuf);

#[cfg(windows)]
impl Fixture {
    fn new() -> Self {
        let root = test_support::unique_test_root();
        fs::create_dir_all(&root).expect("create isolated launch fixture");
        Self(root)
    }
}

#[cfg(windows)]
impl Drop for Fixture {
    fn drop(&mut self) {
        // Cleanup must not replace the original assertion when a test unwinds.
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(windows)]
fn module_fixture(module_id: &str) -> (ModuleDetails, Value) {
    let modules =
        app_modules::discover_modules(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules"))
            .expect("load actual module declarations");
    let descriptor = modules
        .iter()
        .find(|module| module.summary.id == module_id)
        .expect("module declaration");
    let mut summary = descriptor.summary.clone();
    summary.install_state = InstallState::Installed;
    let module = ModuleDetails {
        summary,
        schema_json: descriptor.schema_json.clone(),
        default_ports: descriptor.default_ports.clone(),
        install: descriptor.install.clone(),
        process: descriptor.process.clone(),
        workshop: descriptor.workshop.clone(),
        mods: None,
        runtime: descriptor.runtime.clone(),
    };
    let defaults = app_storage::normalize_complete_instance_settings(
        Some(descriptor),
        serde_json::Map::new(),
        "verbatim-fixture",
        "Verbatim path fixture",
        "127.0.0.1",
    )
    .expect("complete production defaults for the path fixture");
    (module, Value::Object(defaults))
}

#[cfg(windows)]
fn instance_fixture(root: &Path, module: &ModuleDetails, settings: &Value) -> InstanceDetails {
    let config_dir = root.join("instance/config");
    let saves_dir = root.join("instance/saves");
    fs::create_dir_all(&config_dir).expect("create isolated configuration directory");
    fs::create_dir_all(&saves_dir).expect("create isolated saves directory");
    let config_dir = fs::canonicalize(config_dir).expect("canonical configuration root");
    InstanceDetails {
        summary: InstanceSummary {
            id: String::from("verbatim-fixture"),
            name: String::from("Verbatim path fixture"),
            module_id: module.summary.id.clone(),
            status: InstanceStatus::Stopped,
            active_process_count: 0,
            bind_ip: String::from("127.0.0.1"),
            port_count: module.default_ports.len(),
            autostart: false,
        },
        config_file_path: config_dir
            .join("instance.json")
            .to_string_lossy()
            .into_owned(),
        saves_path: fs::canonicalize(saves_dir)
            .expect("canonical saves root")
            .to_string_lossy()
            .into_owned(),
        backup_uses_declared_saves_path: true,
        auto_backup_on_stop: false,
        backup_retention_count: 0,
        settings_json: settings.to_string(),
        ports: module.default_ports.clone(),
        active_run: None,
    }
}

#[cfg(windows)]
fn program_fixture(install_root: &Path, module: &ModuleDetails) {
    fs::create_dir_all(install_root).expect("create isolated installation");
    let install = module.install.as_ref().expect("install declaration");
    let process = module.process.as_ref().expect("process declaration");
    let mut files = Vec::new();
    if let Some(path) = &install.verification_path {
        files.push(path.as_str());
    }
    if !process.executable.contains("{{") {
        files.push(&process.executable);
    }
    if module.summary.id == "necesse" {
        files.push("Server.jar");
    }
    for relative in files {
        let path = install_root.join(normalized_relative_path(relative));
        fs::create_dir_all(path.parent().expect("program parent"))
            .expect("create program directory");
        fs::write(path, b"isolated program fixture, never executed")
            .expect("create program fixture");
    }
}

#[cfg(windows)]
#[test]
fn canonical_install_roots_keep_real_native_working_directories_usable() {
    let fixture = Fixture::new();
    for (module_id, relative_directory) in [
        ("abioticfactor", "AbioticFactor/Binaries/Win64"),
        ("arksurvivalascended", "ShooterGame/Binaries/Win64"),
        ("arksurvivalevolved", "ShooterGame/Binaries/Win64"),
        ("necesse", ""),
        ("romestead", ""),
    ] {
        let (mut module, defaults) = module_fixture(module_id);
        let install_root = fixture.0.join("Native Root With Spaces").join(module_id);
        program_fixture(&install_root, &module);
        fs::create_dir_all(install_root.join(relative_directory))
            .expect("create real working directory");
        let install_root = fs::canonicalize(install_root).expect("canonical install root");
        let expected = install_root.join(relative_directory);
        assert!(expected.is_dir());
        assert!(
            native_path_candidate(&expected).is_some(),
            "fixture needs a short native spelling"
        );
        if !relative_directory.is_empty() {
            let raw = format!("{}/{relative_directory}", install_root.display());
            assert!(
                !Path::new(&raw).exists(),
                "verbatim paths reject literal forward slashes"
            );
        }
        let instance = instance_fixture(&fixture.0.join(module_id), &module, &defaults);
        let settings = AppSettings::default();
        let plan = build_launch_plan_with_override(
            &settings,
            &module,
            &instance,
            Some(install_root.to_str().expect("fixture path")),
        )
        .expect("build real native launch plan");
        assert!(!plan.working_directory.starts_with(r"\\?\"), "{module_id}");
        assert_eq!(fs::canonicalize(&plan.working_directory).unwrap(), expected);
        assert!(
            !plan
                .validation_issues
                .iter()
                .any(|issue| issue.code == "working_directory_missing")
        );
        let executable = &module
            .process
            .as_ref()
            .expect("process declaration")
            .executable;
        assert_eq!(
            fs::canonicalize(&plan.executable_path).expect("resolved native executable"),
            install_root.join(normalized_relative_path(executable)),
            "{module_id} must retain the declared executable's file identity"
        );
        if matches!(module_id, "necesse" | "romestead") {
            assert!(
                !plan.executable_path.starts_with(r"\\?\"),
                "{module_id} requires a Java/cmd-compatible executable path"
            );
        }
        assert!(
            plan.ready_to_launch,
            "{module_id}: {:?}",
            plan.validation_issues
        );

        module.process.as_mut().unwrap().working_directory_template = Some(String::from(
            "{{paths.install_root}}/missing-working-directory",
        ));
        let missing = build_launch_plan_with_override(
            &settings,
            &module,
            &instance,
            Some(install_root.to_str().unwrap()),
        )
        .expect("build missing-directory launch plan");
        assert!(missing.executable_exists);
        assert!(!missing.ready_to_launch);
        assert!(
            missing
                .validation_issues
                .iter()
                .any(|issue| issue.code == "working_directory_missing")
        );
    }
}

#[cfg(windows)]
#[test]
fn managed_path_arguments_and_absolute_executables_preserve_native_file_identity() {
    let fixture = Fixture::new();
    let root = fs::canonicalize(&fixture.0).unwrap();
    for name in ["server.jar", "server.log", "server.exe"] {
        fs::write(root.join(name), b"never executed").unwrap();
    }
    let (module, _) = module_fixture("necesse");
    let settings = serde_json::json!({
        "custom": r"\\?\C:\custom/keep",
        "url": "https://example.test/a/b",
        "extra_launch_args": "--url=https://example.test/a/b"
    });
    let instance = instance_fixture(&fixture.0, &module, &settings);
    let context = TemplateContext {
        instance: &instance,
        settings: &settings,
        install_root: &root,
        config_dir: &root,
        data_dir: &root,
        logs_dir: &root,
        saves_dir: &root,
    };
    assert_eq!(
        expand_resolved_argument_segments("{{paths.install_root}}/server.jar", &context),
        vec![
            compatible_native_path(root.join("server.jar"))
                .to_string_lossy()
                .into_owned()
        ]
    );
    assert_eq!(
        expand_resolved_argument_segments("-abslog={{paths.logs_dir}}/server.log", &context),
        vec![format!(
            "-abslog={}",
            compatible_native_path(root.join("server.log")).display()
        )]
    );
    assert_eq!(
        resolve_process_executable(&root, &format!("{}/server.exe", root.display())),
        root.join("server.exe")
    );
    assert_eq!(
        expand_resolved_argument_segments("{{settings.custom}}", &context),
        vec![r"\\?\C:\custom/keep"]
    );
    assert_eq!(
        expand_resolved_argument_segments("{{settings.url}}", &context),
        vec!["https://example.test/a/b"]
    );
    assert_eq!(
        expand_resolved_argument_segments("{{launch.extra_args}}", &context),
        vec!["--url=https://example.test/a/b"]
    );
    let mixed = "https://example.test/{{paths.install_root}}/server.jar";
    assert_eq!(
        expand_resolved_argument_segments(mixed, &context),
        vec![resolve_template(mixed, &context)]
    );
}

#[cfg(windows)]
#[test]
fn namespace_sensitive_and_long_java_paths_remain_explicitly_blocked() {
    let fixture = Fixture::new();
    let root = fs::canonicalize(&fixture.0).unwrap();
    let special = root.join("directory.");
    fs::create_dir(&special).unwrap();
    assert_eq!(compatible_native_path(special.clone()), special);
    let absent = root.join("missing");
    assert_eq!(compatible_native_path(absent.clone()), absent);
    let long_root = root.join("x".repeat(240));
    let (module, defaults) = module_fixture("necesse");
    program_fixture(&long_root, &module);
    assert_eq!(compatible_native_path(long_root.clone()), long_root);
    let instance = instance_fixture(&fixture.0, &module, &defaults);
    let plan = build_launch_plan_with_override(
        &AppSettings::default(),
        &module,
        &instance,
        Some(long_root.to_str().unwrap()),
    )
    .unwrap();
    assert!(!plan.ready_to_launch);
    assert!(plan.working_directory.starts_with(r"\\?\"));
    assert!(
        plan.validation_issues
            .iter()
            .any(|issue| issue.code == "launch_working_directory_incompatible")
    );
}

#[cfg(windows)]
#[test]
fn romestead_batch_launch_blocks_long_and_namespace_sensitive_working_directories() {
    let fixture = Fixture::new();
    let root = fs::canonicalize(&fixture.0).unwrap();
    let (module, defaults) = module_fixture("romestead");
    for (index, install_root) in [
        root.join("x".repeat(240)),
        root.join("directory."),
        root.join("directory "),
    ]
    .into_iter()
    .enumerate()
    {
        program_fixture(&install_root, &module);
        let instance =
            instance_fixture(&fixture.0.join(format!("case-{index}")), &module, &defaults);
        let plan = build_launch_plan_with_override(
            &AppSettings::default(),
            &module,
            &instance,
            Some(install_root.to_str().unwrap()),
        )
        .expect("build real Romestead batch launch plan");
        assert!(Path::new(&plan.working_directory).is_dir());
        assert_eq!(PathBuf::from(&plan.working_directory), install_root);
        assert!(plan.executable_exists);
        assert!(!plan.ready_to_launch);
        assert!(plan.validation_issues.iter().any(|issue| {
            issue.code == "launch_working_directory_incompatible" && issue.severity == "error"
        }));
    }
}

#[cfg(windows)]
#[test]
fn cmd_working_directory_classification_rejects_unc_without_network_access() {
    for working_directory in [
        r"\\server\share\root",
        r"\\?\UNC\server\share\root",
        r"\\?\C:\root",
    ] {
        let working_directory = Path::new(working_directory);
        for executable in ["start-romestead.bat", "server.CMD"] {
            assert!(native_working_directory_is_incompatible(
                "romestead",
                Path::new(executable),
                &[],
                working_directory,
            ));
        }
        assert!(native_working_directory_is_incompatible(
            "custom",
            Path::new("cmd.exe"),
            &[String::from("/c"), String::from("server.cmd")],
            working_directory,
        ));
    }
    assert!(!native_working_directory_is_incompatible(
        "romestead",
        Path::new("start-romestead.bat"),
        &[],
        Path::new(r"C:\local\root"),
    ));
    for executable in ["server.exe", "java.exe"] {
        assert!(!native_working_directory_is_incompatible(
            "custom",
            Path::new(executable),
            &[],
            Path::new(r"\\server\share\root"),
        ));
    }
}

#[cfg(windows)]
#[test]
fn unc_path_candidates_preserve_the_server_share_and_namespace() {
    let mixed = rendered_path(String::from(r"\\?\UNC\server\share/root/bin"));
    assert_eq!(mixed, PathBuf::from(r"\\?\UNC\server\share\root\bin"));
    assert_eq!(
        native_path_candidate(&mixed),
        Some(PathBuf::from(r"\\server\share\root\bin"))
    );
    assert!(native_path_candidate(Path::new(r"\\?\UNC\server\share\special.\bin")).is_none());
    assert!(native_path_candidate(Path::new(r"\\?\Volume{1234}\root")).is_none());
}

#[cfg(not(windows))]
#[test]
fn rendered_paths_keep_non_windows_spelling() {
    for value in [
        "/srv/games/server/bin",
        r"\\?\C:\root/server",
        "relative/server",
    ] {
        assert_eq!(rendered_path(String::from(value)), PathBuf::from(value));
    }
}
