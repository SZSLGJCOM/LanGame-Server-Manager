use super::*;
use app_modules::ModuleStorageSpec;

struct LifecycleTestRoot(PathBuf);

impl LifecycleTestRoot {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-game-lifecycle-{label}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&root).expect("create lifecycle test root");
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for LifecycleTestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn direct_download_descriptor(saves_path_template: Option<&str>) -> ModuleDescriptor {
    ModuleDescriptor {
        root: PathBuf::new(),
        manifest_toml: String::new(),
        schema_json: None,
        default_ports: Vec::new(),
        install: Some(app_core::InstallSpec {
            shared_game_dir: String::from("rimworld"),
            download_url_windows: Some(String::from("https://example.invalid/server.zip")),
            download_integrity_windows: None,
            source: None,
            verification_path: Some(String::from("Server.exe")),
            minecraft: None,
        }),
        process: None,
        workshop: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
        storage: ModuleStorageSpec {
            saves_path_template: saves_path_template.map(String::from),
            ..ModuleStorageSpec::default()
        },
        summary: app_core::ModuleSummary {
            id: String::from("rimworld"),
            name: String::from("RimWorld Together"),
            version: String::from("1"),
            description: None,
            steam_app_id: None,
            install_state: InstallState::Installed,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
    }
}

#[test]
fn finishing_one_module_preserves_other_active_installations_and_program_counts() {
    let mut finished = direct_download_descriptor(None).summary;
    finished.install_state = InstallState::Installing;
    finished.instance_program_count = 2;
    finished.archived_program_count = 3;
    let mut concurrent = finished.clone();
    concurrent.id = String::from("other-game");
    concurrent.install_state = InstallState::Updating;
    let concurrent_before = serde_json::to_value(&concurrent).unwrap();
    let mut state = AppState {
        modules: vec![finished.clone(), concurrent],
        ..AppState::default()
    };

    // Success, failure and uninstall completion all update one library entry.
    for final_state in [
        InstallState::Installed,
        InstallState::Incomplete,
        InstallState::NotInstalled,
    ] {
        let mut refreshed = finished.clone();
        refreshed.install_state = final_state.clone();
        refreshed.instance_program_count = 0;
        refreshed.archived_program_count = 0;
        merge_module_install_state(&mut state, &refreshed);
        assert_eq!(state.modules.len(), 2);
        assert_eq!(state.modules[0].install_state, final_state);
        assert_eq!(state.modules[0].instance_program_count, 2);
        assert_eq!(state.modules[0].archived_program_count, 3);
        assert_eq!(
            serde_json::to_value(&state.modules[1]).unwrap(),
            concurrent_before
        );
    }
}

#[test]
fn path_containment_normalizes_parent_segments_without_prefix_confusion() {
    let root = LifecycleTestRoot::new("path-containment");
    let install_root = root.path().join("game");

    assert!(path_is_same_or_within(
        &install_root.join("data").join("..").join("saves"),
        &install_root
    ));
    assert!(path_is_same_or_within(&install_root, &install_root));
    assert!(!path_is_same_or_within(
        &root.path().join("game-old").join("saves"),
        &install_root
    ));
}

#[test]
fn protected_data_rejection_keeps_install_root_sentinel() {
    let root = LifecycleTestRoot::new("protected-sentinel");
    let install_root = root.path().join("rimworld");
    let saves_root = install_root.join("Assets");
    let sentinel = saves_root.join("user-save.sentinel");
    fs::create_dir_all(&saves_root).expect("create protected saves root");
    fs::write(&sentinel, b"must survive").expect("write protected sentinel");

    let protected = vec![ProtectedInstallDataPath {
        source: String::from("测试实例的存档"),
        path: Some(saves_root),
    }];
    let error = reject_protected_install_data(
        ModuleInstallOperation::Uninstall,
        "RimWorld Together",
        &install_root,
        &protected,
    )
    .expect_err("protected install must be rejected");

    let payload: serde_json::Value = serde_json::from_str(&error).expect("JSON error contract");
    assert_eq!(payload["code"], "install_data_protected");
    assert_eq!(payload["operation"], "uninstall");
    assert_eq!(payload["module_name"], "RimWorld Together");
    assert_eq!(
        payload["install_root"],
        install_root.to_string_lossy().as_ref()
    );
    assert_eq!(payload["protected"][0]["source"], protected[0].source);
    assert_eq!(
        payload["protected"][0]["path"],
        protected[0]
            .path
            .as_ref()
            .unwrap()
            .to_string_lossy()
            .as_ref()
    );
    assert!(
        payload["message"]
            .as_str()
            .unwrap()
            .contains("为保护存档和备份")
    );
    assert!(sentinel.is_file(), "rejected uninstall must not touch data");
    assert_eq!(fs::read(&sentinel).unwrap(), b"must survive");
}

#[test]
fn direct_download_replacement_rejects_any_nonempty_install_root() {
    let root = LifecycleTestRoot::new("direct-replacement");
    let install_root = root.path().join("rimworld");
    let sentinel = install_root.join("unknown-user-data.sentinel");
    fs::create_dir_all(&install_root).expect("create install root");
    fs::write(&sentinel, b"must survive").expect("write user sentinel");
    let settings = app_core::AppSettings {
        games_root: root.path().to_string_lossy().into_owned(),
        ..app_core::AppSettings::default()
    };
    let descriptor = direct_download_descriptor(Some("{{paths.install_root}}/Assets"));

    for (operation, expected_operation) in [
        (ModuleInstallOperation::Install, "install"),
        (ModuleInstallOperation::Update, "update"),
        (ModuleInstallOperation::Validate, "validate"),
    ] {
        let error = reject_unsafe_direct_download_replacement(operation, &settings, &descriptor)
            .expect_err("whole-directory replacement must be rejected");
        let payload: serde_json::Value = serde_json::from_str(&error).expect("JSON error contract");
        assert_eq!(payload["code"], "install_replacement_not_empty");
        assert_eq!(payload["operation"], expected_operation);
        assert_eq!(payload["module_id"], descriptor.summary.id);
        assert_eq!(payload["module_name"], descriptor.summary.name);
        assert_eq!(
            payload["install_root"],
            install_root.to_string_lossy().as_ref()
        );
        assert!(payload["message"].as_str().unwrap().contains("整包替换"));
    }
    assert!(
        sentinel.is_file(),
        "preflight rejection must preserve sentinel"
    );
}

#[test]
fn direct_download_initial_install_allows_an_empty_root() {
    let root = LifecycleTestRoot::new("direct-empty");
    fs::create_dir_all(root.path().join("rimworld")).expect("create empty install root");
    let settings = app_core::AppSettings {
        games_root: root.path().to_string_lossy().into_owned(),
        ..app_core::AppSettings::default()
    };

    reject_unsafe_direct_download_replacement(
        ModuleInstallOperation::Install,
        &settings,
        &direct_download_descriptor(None),
    )
    .expect("initial install into an empty root remains supported");
}

#[test]
fn direct_download_reinstallation_accepts_preserved_data_without_server_files() {
    let root = LifecycleTestRoot::new("retained-reinstall");
    let install_root = root.path().join("rimworld");
    fs::create_dir_all(install_root.join("Assets")).unwrap();
    fs::write(install_root.join("Assets/world.save"), b"keep").unwrap();
    app_steamcmd::mark_retained_install_data(&install_root).unwrap();
    let settings = app_core::AppSettings {
        games_root: root.path().to_string_lossy().into_owned(),
        ..app_core::AppSettings::default()
    };
    let descriptor = direct_download_descriptor(Some("{{paths.install_root}}/Assets"));
    reject_unsafe_direct_download_replacement(
        ModuleInstallOperation::Install,
        &settings,
        &descriptor,
    )
    .unwrap();
    fs::write(install_root.join("Server.exe"), b"program").unwrap();
    assert!(
        reject_unsafe_direct_download_replacement(
            ModuleInstallOperation::Update,
            &settings,
            &descriptor
        )
        .is_err()
    );
    assert_eq!(
        fs::read(install_root.join("Assets/world.save")).unwrap(),
        b"keep"
    );
}

#[test]
fn uninstall_running_instances_error_keeps_all_instance_labels() {
    let mut module = direct_download_descriptor(None).summary;
    module.name = String::from("游戏 \"A\"\\B\nC");
    let running = vec![
        String::from("服务器 \"1\"\\run\nline"),
        String::from("server-2"),
    ];
    let error = ensure_module_unused_for_uninstall(&module, &running)
        .expect_err("running instances must block uninstall");
    let payload: serde_json::Value = serde_json::from_str(&error).expect("JSON error contract");
    assert_eq!(payload["code"], "module_in_use");
    assert_eq!(payload["module_id"], module.id);
    assert_eq!(payload["module_name"], module.name);
    assert_eq!(payload["instances"], serde_json::json!(running));
    assert!(payload["message"].as_str().unwrap().contains(&module.name));
    ensure_module_unused_for_uninstall(&module, &[])
        .expect("module with no running instances remains removable");
}

#[test]
fn protected_data_error_keeps_templates_without_concrete_paths() {
    let protected = vec![ProtectedInstallDataPath {
        source: String::from("模块存档模板 \"{{paths.install_root}}/存档\"\n说明"),
        path: None,
    }];
    let install_root = Path::new("D:/server\\game");
    let module_name = "游戏 \"A\"\\B\nC";
    let error = reject_protected_install_data(
        ModuleInstallOperation::Uninstall,
        module_name,
        install_root,
        &protected,
    )
    .expect_err("declared install-root saves must block uninstall");
    let payload: serde_json::Value = serde_json::from_str(&error).expect("JSON error contract");
    assert_eq!(payload["module_name"], module_name);
    assert_eq!(payload["protected"][0]["source"], protected[0].source);
    assert!(payload["protected"][0]["path"].is_null());
    assert!(
        payload["message"]
            .as_str()
            .unwrap()
            .contains(&protected[0].source)
    );
    reject_protected_install_data(
        ModuleInstallOperation::Uninstall,
        module_name,
        install_root,
        &[],
    )
    .expect("empty protection set remains removable");
}

#[test]
fn direct_download_replacement_rejects_a_file_without_deleting_it() {
    let root = LifecycleTestRoot::new("direct-file");
    let install_root = root.path().join("rimworld");
    fs::write(&install_root, b"must survive").expect("write file at install path");
    let settings = app_core::AppSettings {
        games_root: root.path().to_string_lossy().into_owned(),
        ..app_core::AppSettings::default()
    };
    let error = reject_unsafe_direct_download_replacement(
        ModuleInstallOperation::Install,
        &settings,
        &direct_download_descriptor(None),
    )
    .expect_err("file at install path must be rejected");
    let payload: serde_json::Value = serde_json::from_str(&error).expect("JSON error contract");
    assert_eq!(payload["code"], "install_path_not_directory");
    assert_eq!(
        payload["install_root"],
        install_root.to_string_lossy().as_ref()
    );
    assert_eq!(fs::read(&install_root).unwrap(), b"must survive");
}

#[test]
fn module_declared_install_root_save_path_requires_actual_data() {
    let root = LifecycleTestRoot::new("declared-save");
    let install_root = root.path().join("rimworld");
    let descriptor = direct_download_descriptor(Some("{{paths.install_root}}/Assets"));
    fs::create_dir_all(&install_root).unwrap();
    fs::write(install_root.join("Server.exe"), b"server files").unwrap();
    assert!(
        declared_install_root_data_path(&descriptor, &install_root)
            .unwrap()
            .is_none()
    );
    fs::create_dir_all(install_root.join("Assets")).unwrap();
    fs::write(install_root.join("Assets/world.save"), b"retained world").unwrap();
    let protected = declared_install_root_data_path(&descriptor, &install_root)
        .expect("inspect declared data")
        .expect("actual save must be protected");

    assert!(protected.source.contains("paths.install_root"));
    assert_eq!(protected.path, Some(install_root.join("Assets")));
    assert!(
        declared_install_root_data_path(
            &direct_download_descriptor(Some("{{paths.instance_root}}/saves")),
            &install_root
        )
        .unwrap()
        .is_none()
    );
}
