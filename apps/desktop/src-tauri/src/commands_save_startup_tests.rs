use super::*;
use app_storage::{StoragePaths, bootstrap_storage_with_paths};
use std::collections::{BTreeMap, BTreeSet};

const AUTOMATIC_MODULES: [&str; 14] = [
    "abioticfactor",
    "barotrauma",
    "conanexiles",
    "corekeeper",
    "enshrouded",
    "nightingale",
    "returntomoria",
    "runescapedragonwilds",
    "scum",
    "sonsoftheforest",
    "theforest",
    "valheim",
    "vrising",
    "windrose",
];

const NATIVE_COMMAND_MODULES: [&str; 13] = [
    "minecraft",
    "projectzomboid",
    "terraria",
    "sevendaystodie",
    "unturned",
    "necesse",
    "romestead",
    "palworld",
    "humanitz",
    "astroneer",
    "rimworld",
    "arksurvivalevolved",
    "arksurvivalascended",
];

#[test]
fn automatic_save_rejects_explicit_alternate_unreal_mod_directories_case_insensitively() {
    for key in ["UE4SS_MODS_PATHS", "ue4ss_mods_paths"] {
        let overrides = BTreeMap::from([(key.to_owned(), "another-mod-directory".to_owned())]);
        assert!(validate_unreal_environment(&overrides).is_err());
    }
}

#[test]
fn dragonwilds_save_launch_identity_rejects_configured_overrides() {
    for key in ["LGSM_DRAGON_SAVE_RUN", "lgsm_dragon_save_run"] {
        let overrides = BTreeMap::from([(key.to_owned(), "operator-value".to_owned())]);
        let error = crate::live_players::dragonwilds_control::validate_save_environment(&overrides)
            .unwrap_err();
        assert!(error.contains("preserved"));
        assert_eq!(overrides[key], "operator-value");
    }
    let values = BTreeMap::from([("UNRELATED".to_owned(), "kept".to_owned())]);
    crate::live_players::dragonwilds_control::validate_save_environment(&values).unwrap();
}

fn instance(module: &str, root: &Path) -> InstanceDetails {
    serde_json::from_value(json!({
        "summary": {
            "id": "automatic-save", "name": "Automatic save", "module_id": module,
            "status": "Stopped", "bind_ip": "127.0.0.1", "port_count": 0,
            "autostart": false
        },
        "config_file_path": root.join("config/instance.json"),
        "saves_path": root.join("data/worlds"),
        "auto_backup_on_stop": false, "backup_retention_count": 0,
        "settings_json": "{}", "ports": [], "active_run": null
    }))
    .unwrap()
}

#[test]
fn automatic_save_setup_covers_every_loader_module_and_preserves_other_startups() {
    let modules =
        discover_modules(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../modules"))
            .unwrap();
    let actual = modules
        .iter()
        .filter(|descriptor| requires_private_program(&descriptor.summary.id))
        .map(|descriptor| descriptor.summary.id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, BTreeSet::from(AUTOMATIC_MODULES));
    for module in ["squad", "dontstarve", "soulmask", "missing"] {
        assert!(!requires_private_program(module), "{module}");
    }
}

#[test]
fn automatic_save_program_reservation_contains_source_and_private_target_once() {
    let base = std::env::temp_dir().join("lgsm-save-startup-path-contract");
    let root = base.join("instances/server");
    let library = base.join("program/corekeeper");
    for module in AUTOMATIC_MODULES {
        let details = instance(module, &root);
        let expected = root.join("runtime");
        assert_eq!(
            program_roots(&details, &library).unwrap(),
            [library.clone(), expected.clone()]
        );
        assert_eq!(program_roots(&details, &expected).unwrap(), [expected]);
    }
    for module in ["squad", "dontstarve", "soulmask", "missing"] {
        assert_eq!(
            program_roots(&instance(module, &root), &library).unwrap(),
            std::slice::from_ref(&library)
        );
    }
}

#[test]
fn native_save_commands_keep_shared_programs_and_do_not_restrict_runtime_variants() {
    let base = std::env::temp_dir().join("lgsm-native-save-startup-path-contract");
    let library = base.join("program/library");
    for module in NATIVE_COMMAND_MODULES {
        assert!(!requires_private_program(module), "{module}");
        for settings in [
            r#"{}"#,
            r#"{"eac_enabled":true,"server_runtime":"tmodloader","mod_ids_csv":"123","active_mod_ids":"123","map_name":"CustomMap","custom_launch_flags":"-operator"}"#,
        ] {
            let mut details = instance(module, &base.join("instances/server"));
            details.settings_json = settings.into();
            assert_eq!(
                program_roots(&details, &library).unwrap(),
                std::slice::from_ref(&library),
                "{module}"
            );
            assert_eq!(details.settings_json, settings);
        }
    }
}

struct FixtureRoot(PathBuf);
impl FixtureRoot {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lgsm-save-startup-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for FixtureRoot {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn copy_module(source: &Path, target: &Path) -> std::io::Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        assert!(
            !kind.is_symlink(),
            "the test copies only ordinary module source files"
        );
        if kind.is_dir() {
            copy_module(&entry.path(), &target.join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), target.join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn snapshot(root: &Path) -> std::io::Result<BTreeMap<PathBuf, Vec<u8>>> {
    fn collect(
        root: &Path,
        directory: &Path,
        files: &mut BTreeMap<PathBuf, Vec<u8>>,
    ) -> std::io::Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                collect(root, &entry.path(), files)?;
            } else {
                files.insert(
                    entry.path().strip_prefix(root).unwrap().to_owned(),
                    fs::read(entry.path())?,
                );
            }
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    collect(root, root, &mut files)?;
    Ok(files)
}

fn reserve(state: &DesktopState, instance_id: &str) -> RuntimeStartReservationLease {
    match state
        .try_reserve_runtime_start(instance_id, "manual")
        .unwrap()
    {
        crate::state::RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
        other => panic!("unexpected startup reservation: {other:?}"),
    }
}

/// Opt-in native package probe: sources are read only, and every generated
/// loader/configuration/bridge file belongs to a new disposable program root.
#[tokio::test]
#[ignore = "requires reviewed local Windows game packages and official release download"]
async fn automatic_save_installs_real_unreal_runtime_and_bridge_from_a_clean_program() {
    let sources: BTreeMap<String, PathBuf> = serde_json::from_str(
        &std::env::var("LGSM_AUTO_SAVE_NATIVE_ROOTS")
            .expect("provide reviewed source roots as JSON"),
    )
    .unwrap();
    let projects = [
        (
            "abioticfactor",
            "AbioticFactor",
            "AbioticFactorServer-Win64-Shipping.exe",
            "version.dll",
        ),
        (
            "returntomoria",
            "Moria",
            "MoriaServer-Win64-Shipping.exe",
            "dwmapi.dll",
        ),
        (
            "nightingale",
            "NWX",
            "NWXServer-Win64-Shipping.exe",
            "dwmapi.dll",
        ),
        (
            "conanexiles",
            "ConanSandbox",
            "ConanSandboxServer-Win64-Shipping.exe",
            "dsound.dll",
        ),
        ("scum", "SCUM", "SCUMServer.exe", "dwmapi.dll"),
        (
            "windrose",
            "R5",
            "WindroseServer-Win64-Shipping.exe",
            "dwmapi.dll",
        ),
    ];
    let fixture = FixtureRoot::new();
    let state = DesktopState::default();
    for (module, project, image, proxy) in projects {
        let base = fixture.0.join(module);
        let runtime = base.join("runtime");
        let relative = PathBuf::from(project).join("Binaries/Win64");
        let win64 = runtime.join(&relative);
        fs::create_dir_all(&win64).unwrap();
        fs::write(runtime.join(".langame-private-runtime"), b"managed\n").unwrap();
        fs::copy(
            sources[module].join(&relative).join(image),
            win64.join(image),
        )
        .unwrap();
        if module == "conanexiles" {
            fs::copy(
                sources[module].join(&relative).join("Dreamworld.dll"),
                win64.join("Dreamworld.dll"),
            )
            .unwrap();
        }
        let details = instance(module, &base);
        let operation = state
            .begin_storage_context_operation("isolated automatic save installation probe")
            .unwrap();
        assert!(
            !crate::unreal_loader::installed(&runtime, module).unwrap(),
            "{module}"
        );
        let first = ensure_unreal(&details, &operation)
            .await
            .unwrap_or_else(|error| panic!("{module}: {error}"));
        assert_eq!(first.is_some(), module != "windrose");
        assert!(
            crate::unreal_loader::installed(&runtime, module).unwrap(),
            "{module}"
        );
        assert!(win64.join(proxy).is_file());
        assert!(win64.join("ue4ss/Mods/LgsmWorldSave/enabled.txt").is_file());
        assert!(
            win64
                .join("ue4ss/Mods/LgsmWorldSave/Scripts/main.lua")
                .is_file()
        );
        let owner = fs::read(win64.join(".langame-ue4ss-loader.json")).unwrap();
        let core = fs::read(win64.join("ue4ss/UE4SS.dll")).unwrap();
        // The second call takes the installed=true branch and cannot download.
        ensure_unreal(&details, &operation)
            .await
            .unwrap_or_else(|error| panic!("{module} repeat: {error}"));
        assert_eq!(
            fs::read(win64.join(".langame-ue4ss-loader.json")).unwrap(),
            owner
        );
        assert_eq!(fs::read(win64.join("ue4ss/UE4SS.dll")).unwrap(), core);
        println!("automatic save loader and bridge: {module} first install and repeat passed");
    }
}

#[tokio::test]
async fn automatic_save_start_detaches_a_shared_program_once_without_changing_source_or_peer()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = FixtureRoot::new();
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let modules_root = fixture.0.join("modules");
    copy_module(
        &workspace.join("modules/corekeeper"),
        &modules_root.join("corekeeper"),
    )?;
    let storage = bootstrap_storage_with_paths(StoragePaths {
        app_data_root: fixture.0.join("app-data"),
        settings_path: fixture.0.join("app-data/settings.json"),
        database_path: fixture.0.join("app-data/db/test.db"),
        logs_root: fixture.0.join("logs"),
        modules_root,
        migrations_root: workspace.join("migrations"),
        steamcmd_root: fixture.0.join("steamcmd"),
        games_root: fixture.0.join("games"),
        instances_root: fixture.0.join("instances"),
        archives_root: fixture.0.join("instances/.trash"),
    })?;
    initialize_database(&storage.paths).await?;
    let descriptors = discover_modules(&storage.paths.modules_root)?;
    sync_modules(&storage.paths, &descriptors).await?;
    let descriptor = find_descriptor(&descriptors, "corekeeper")?;
    let library = storage.paths.games_root.join("corekeeper");
    fs::create_dir_all(library.join("CoreKeeperServer_Data/Managed"))?;
    fs::write(
        library.join("CoreKeeperServer.exe"),
        b"fixture server, never executed",
    )?;
    fs::write(
        library.join("CoreKeeperServer_Data/Managed/dependency.dll"),
        b"fixture dependency",
    )?;
    app_storage::record_library_program_baseline(&library, descriptor, true, None)?;
    sync_game_installs(
        &storage.paths,
        &[GameInstallSyncRecord {
            module_id: "corekeeper".into(),
            install_root: library.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some("fixture-build".into()),
            mark_verified: true,
        }],
    )
    .await?;
    // Reproduce persisted instances created when the program was shared. Current
    // module discovery and the startup coordinator use the real module contract.
    let mut creation_descriptor = descriptor.clone();
    creation_descriptor.storage.program_sharing = app_modules::ModuleProgramSharing::Shared;
    let mut created = Vec::new();
    for name in ["Save extension instance", "Shared peer"] {
        let details = app_storage::create_instance_with_options(
            &storage.paths,
            &creation_descriptor,
            CreateInstanceInput {
                name: name.into(),
                module_id: "corekeeper".into(),
            },
            app_storage::InstanceCreationOptions {
                program_mode: Some(app_core::InstanceProgramMode::Shared),
                ..Default::default()
            },
        )
        .await?;
        created
            .push(read_instance_details(&storage.paths, &details.provisioning.summary.id).await?);
    }
    let mut instances = created.into_iter();
    let first = instances.next().unwrap();
    let peer = instances.next().unwrap();
    let first_root = super::super::commands_program_storage::instance_root(&first)?.to_owned();
    let peer_root = super::super::commands_program_storage::instance_root(&peer)?.to_owned();
    assert_eq!(
        app_storage::instance_program_mode(&first_root)?,
        app_storage::InstanceProgramMode::Shared
    );
    assert_eq!(
        app_storage::instance_program_mode(&peer_root)?,
        app_storage::InstanceProgramMode::Shared
    );
    fs::create_dir_all(&first.saves_path)?;
    fs::create_dir_all(&peer.saves_path)?;
    fs::write(
        Path::new(&first.saves_path).join("existing.sav"),
        b"first existing world",
    )?;
    fs::write(
        Path::new(&peer.saves_path).join("existing.sav"),
        b"peer existing world",
    )?;
    let source_before = snapshot(&library)?;
    let peer_before = snapshot(&peer_root)?;
    let first_config = fs::read(&first.config_file_path)?;
    let peer_install = app_storage::read_instance_program_install(&storage.paths, &peer.summary.id)
        .await?
        .unwrap();
    let state = DesktopState::from_storage(&storage);
    let reservation = reserve(&state, &first.summary.id);
    let mutation = state.acquire_instance_mutation(&first.summary.id).await;
    let roots = program_roots(&first, &library)?;
    let install =
        app_steamcmd::acquire_game_install_lifecycle(&state.install_locks, "corekeeper", &roots)
            .await?;
    let (guards, detached) = ensure_private_program(
        &storage,
        descriptor,
        first,
        &reservation,
        (mutation, Some(install)),
    )
    .await?;
    let private = app_storage::resolve_instance_runtime_root(&first_root)?;
    assert_eq!(private, first_root.join("runtime"));
    assert_eq!(
        app_storage::instance_program_mode(&first_root)?,
        app_storage::InstanceProgramMode::Independent
    );
    assert_eq!(
        fs::read(private.join("CoreKeeperServer.exe"))?,
        b"fixture server, never executed"
    );
    assert_eq!(
        fs::read(private.join("CoreKeeperServer_Data/Managed/dependency.dll"))?,
        b"fixture dependency"
    );
    let owned = app_storage::read_instance_program_install(&storage.paths, &detached.summary.id)
        .await?
        .unwrap();
    assert_eq!(owned.runtime_mode, "independent");
    assert_eq!(
        owned.install.owner_instance_id.as_deref(),
        Some(detached.summary.id.as_str())
    );
    fs::write(
        private.join("save-extension.txt"),
        b"instance save extension",
    )?;
    let private_before = snapshot(&private)?;
    let owned_id = owned.install.id;
    let (guards, again) =
        ensure_private_program(&storage, descriptor, detached, &reservation, guards).await?;
    assert_eq!(snapshot(&private)?, private_before);
    assert_eq!(
        app_storage::read_instance_program_install(&storage.paths, &again.summary.id)
            .await?
            .unwrap()
            .install
            .id,
        owned_id
    );
    assert_eq!(snapshot(&library)?, source_before);
    assert_eq!(snapshot(&peer_root)?, peer_before);
    assert_eq!(fs::read(&again.config_file_path)?, first_config);
    assert_eq!(
        fs::read(Path::new(&again.saves_path).join("existing.sav"))?,
        b"first existing world"
    );
    assert_eq!(
        app_storage::instance_program_mode(&peer_root)?,
        app_storage::InstanceProgramMode::Shared
    );
    assert_eq!(
        app_storage::resolve_instance_runtime_root(&peer_root)?,
        fs::canonicalize(&library)?
    );
    let peer_after = app_storage::read_instance_program_install(&storage.paths, &peer.summary.id)
        .await?
        .unwrap();
    assert_eq!(peer_after.runtime_mode, "shared");
    assert_eq!(peer_after.install.id, peer_install.install.id);
    assert_eq!(
        peer_after.install.install_root,
        peer_install.install.install_root
    );
    drop(guards);
    drop(reservation);
    drop(state);
    Ok(())
}
