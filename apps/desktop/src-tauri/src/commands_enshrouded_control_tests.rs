use super::*;

struct Scratch(std::path::PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lgsm-enshrouded-install-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn unknown_executable_never_installs_a_hook() {
    let root = Scratch::new();
    fs::write(root.0.join("enshrouded_server.exe"), b"unknown build").unwrap();
    install(&root.0, b"our extension").unwrap();
    assert!(!root.0.join(DLL).exists());
    assert!(!root.0.join(io::OWNER).exists());
}

#[test]
fn user_loader_is_never_replaced() {
    let root = Scratch::new();
    fs::write(root.0.join(DLL), b"user loader").unwrap();
    install_verified(&root.0, b"our extension").unwrap();
    assert_eq!(fs::read(root.0.join(DLL)).unwrap(), b"user loader");
    assert!(!root.0.join(io::OWNER).exists());
}

#[test]
fn an_updated_unknown_executable_retires_only_the_owned_proxy() {
    let root = Scratch::new();
    install_verified(&root.0, b"our extension").unwrap();
    fs::write(
        root.0.join("enshrouded_server.exe"),
        b"updated unknown build",
    )
    .unwrap();
    install(&root.0, b"our extension").unwrap();
    assert!(!root.0.join(DLL).exists());
    assert!(root.0.join(io::OWNER).exists());
    fs::write(root.0.join(DLL), b"user replacement").unwrap();
    install(&root.0, b"our extension").unwrap();
    assert_eq!(fs::read(root.0.join(DLL)).unwrap(), b"user replacement");
}

#[test]
fn interrupted_owned_publication_is_recovered_before_enable() {
    let root = Scratch::new();
    install_verified(&root.0, b"our extension").unwrap();
    let temporary = root.0.join(io::DIRECTORY).join("players.json.tmp");
    fs::write(&temporary, b"interrupted packet").unwrap();
    install_verified(&root.0, b"our extension").unwrap();
    assert!(!temporary.exists());
    assert_eq!(
        fs::read(root.0.join(io::DIRECTORY).join("enable")).unwrap(),
        io::ENABLE
    );
}

#[test]
fn owned_installation_is_recoverable_and_idempotent() {
    let root = Scratch::new();
    install_verified(&root.0, b"first extension").unwrap();
    install_verified(&root.0, b"next extension").unwrap();
    install_verified(&root.0, b"next extension").unwrap();
    assert_eq!(fs::read(root.0.join(DLL)).unwrap(), b"next extension");
    assert_eq!(
        fs::read(root.0.join(io::DIRECTORY).join("enable")).unwrap(),
        io::ENABLE
    );
    let owner: Ownership =
        serde_json::from_slice(&fs::read(root.0.join(io::OWNER)).unwrap()).unwrap();
    assert_eq!(owner.plugin_sha256, Some(io::digest(b"next extension")));
    assert!(owner.pending_plugin_sha256.is_none());
    fs::write(root.0.join(DLL), b"external change").unwrap();
    install_verified(&root.0, b"next extension").unwrap();
    assert_eq!(fs::read(root.0.join(DLL)).unwrap(), b"external change");
}

#[test]
fn unowned_control_directory_is_preserved() {
    let root = Scratch::new();
    fs::create_dir(root.0.join(io::DIRECTORY)).unwrap();
    fs::write(root.0.join(io::DIRECTORY).join("keep"), b"data").unwrap();
    assert!(install_verified(&root.0, b"extension").is_err());
    assert_eq!(
        fs::read(root.0.join(io::DIRECTORY).join("keep")).unwrap(),
        b"data"
    );
    assert!(!root.0.join(DLL).exists());
}

#[test]
fn optional_preparation_failure_preserves_a_user_loader_and_disables_moderation() {
    let root = Scratch::new();
    fs::write(root.0.join(DLL), b"user loader").unwrap();
    // Missing executable makes preparation fail. The optional setup reports the
    // issue as a value, preserving the ordinary game-start control flow.
    let (prepared, issue) = prepare_optional_install(&root.0, &"f".repeat(64), None);
    assert!(!prepared);
    assert!(issue.is_some());
    assert_eq!(fs::read(root.0.join(DLL)).unwrap(), b"user loader");
    let state: serde_json::Value = serde_json::from_slice(
        &fs::read(root.0.join(".langame-enshrouded-control-state.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(state["extension_status"], "not_installed");
}

#[test]
fn optional_preparation_failure_retires_only_the_verified_owned_proxy() {
    let root = Scratch::new();
    install_verified(&root.0, b"our extension").unwrap();
    let (prepared, issue) = prepare_optional_install(&root.0, &"f".repeat(64), None);
    assert!(!prepared);
    assert!(issue.is_some());
    assert!(!root.0.join(DLL).exists());
    assert!(root.0.join(io::OWNER).exists());
}

fn startup_fixture() -> (
    Scratch,
    StorageBootstrap,
    InstanceDetails,
    app_storage::InstanceProgramInstall,
) {
    let scratch = Scratch::new();
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let storage = app_storage::bootstrap_storage_with_paths(app_storage::StoragePaths {
        app_data_root: scratch.0.join("app-data"),
        settings_path: scratch.0.join("app-data/settings.json"),
        database_path: scratch.0.join("app-data/db/test.db"),
        logs_root: scratch.0.join("logs"),
        modules_root: workspace.join("modules"),
        migrations_root: workspace.join("migrations"),
        steamcmd_root: scratch.0.join("steamcmd"),
        games_root: scratch.0.join("games"),
        instances_root: scratch.0.join("instances"),
        archives_root: scratch.0.join("instances/.trash"),
    })
    .unwrap();
    let instance_root = scratch.0.join("instances/server");
    let runtime = instance_root.join("runtime");
    fs::create_dir_all(&runtime).unwrap();
    fs::write(runtime.join(".langame-private-runtime"), b"managed\n").unwrap();
    fs::write(runtime.join("enshrouded_server.exe"), b"unknown build").unwrap();
    let details: InstanceDetails = serde_json::from_value(serde_json::json!({
        "summary": {"id":"optional-save", "name":"Optional save", "module_id":"enshrouded",
            "status":"Stopped", "bind_ip":"127.0.0.1", "port_count":0, "autostart":false},
        "config_file_path": instance_root.join("config/instance.json"),
        "saves_path": instance_root.join("saves"), "auto_backup_on_stop":false,
        "backup_retention_count":0, "settings_json":"{}", "ports":[], "active_run":null
    }))
    .unwrap();
    let binding = app_storage::InstanceProgramInstall {
        instance_id: details.summary.id.clone(),
        instance_name: details.summary.name.clone(),
        runtime_mode: "independent".into(),
        install: app_storage::ProgramInstallRecord {
            id: 1,
            module_id: "enshrouded".into(),
            install_root: runtime,
            install_state: app_core::InstallState::Installed,
            current_version: None,
            scope: ProgramInstallScope::Instance,
            owner_instance_id: Some(details.summary.id.clone()),
        },
    };
    (scratch, storage, details, binding)
}

#[tokio::test]
async fn optional_enshrouded_save_start_preserves_native_start_without_save_readiness() {
    for loader in ["absent", "owned", "operator"] {
        let (_scratch, storage, details, binding) = startup_fixture();
        let runtime = binding.install.install_root.clone();
        match loader {
            "owned" => install_verified(&runtime, b"previous owned extension").unwrap(),
            "operator" => fs::write(runtime.join(DLL), b"operator loader").unwrap(),
            _ => {}
        }
        let (_, native_extension_prepared) =
            prepare_local(&storage, &details, Ok(Some(binding)), None)
                .expect("an unsupported optional extension must preserve native startup")
                .expect("the actual executable was inspected");
        let manager: serde_json::Value = serde_json::from_slice(
            &fs::read(runtime.join(".langame-enshrouded-control-state.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manager["extension_status"], "unsupported_game_build");
        assert_eq!(manager["executable_sha256"], io::digest(b"unknown build"));
        if loader == "operator" {
            assert_eq!(fs::read(runtime.join(DLL)).unwrap(), b"operator loader");
        } else {
            assert!(!runtime.join(DLL).exists());
        }
        let state = crate::state::DesktopState::default();
        let operation = state
            .begin_storage_context_operation("optional save startup")
            .unwrap();
        let save = super::super::commands_save_startup::ensure_for_start(
            &storage,
            &details,
            &operation,
            None,
            native_extension_prepared,
        )
        .await
        .expect("optional extension unavailability must not block the native server");
        assert!(
            !save.available,
            "{loader}: an unavailable extension must not enable manual save"
        );
        assert!(save.environment.is_empty());
    }
}

#[tokio::test]
async fn enshrouded_save_readiness_uses_owned_payload_publication() {
    for operator_loader in [false, true] {
        let (_scratch, storage, details, binding) = startup_fixture();
        let runtime = &binding.install.install_root;
        if operator_loader {
            fs::write(runtime.join(DLL), b"operator loader").unwrap();
        }
        // Exercise the installer result used after payload compatibility has
        // been verified; the fixture is not a supported game executable.
        let native_extension_prepared =
            install_payload(runtime, b"verified payload", None).unwrap();
        assert_eq!(native_extension_prepared, !operator_loader);
        let state = crate::state::DesktopState::default();
        let operation = state
            .begin_storage_context_operation("prepared save startup")
            .unwrap();
        let save = super::super::commands_save_startup::ensure_for_start(
            &storage,
            &details,
            &operation,
            None,
            native_extension_prepared,
        )
        .await
        .unwrap();
        assert_eq!(save.available, !operator_loader);
        assert!(save.environment.is_empty());
        assert_eq!(
            fs::read(runtime.join(DLL)).unwrap(),
            if operator_loader {
                b"operator loader".as_slice()
            } else {
                b"verified payload".as_slice()
            }
        );
    }
}

#[tokio::test]
async fn optional_enshrouded_preparation_error_does_not_enable_save() {
    let (_scratch, storage, details, binding) = startup_fixture();
    let runtime = binding.install.install_root;
    fs::write(runtime.join(DLL), b"operator loader").unwrap();
    // This instance has no database program binding. Preparation reports its
    // ownership failure while retaining the existing optional-start contract.
    let native_extension_prepared = prepare(&storage, &details, None).await.unwrap();
    assert!(!native_extension_prepared);
    let state = crate::state::DesktopState::default();
    let operation = state
        .begin_storage_context_operation("failed optional save startup")
        .unwrap();
    let save = super::super::commands_save_startup::ensure_for_start(
        &storage,
        &details,
        &operation,
        None,
        native_extension_prepared,
    )
    .await
    .unwrap();
    assert!(!save.available);
    assert!(save.environment.is_empty());
    assert_eq!(fs::read(runtime.join(DLL)).unwrap(), b"operator loader");
}
