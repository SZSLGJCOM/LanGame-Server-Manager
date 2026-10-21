//! Opt-in installation acceptance against local native SDKs and official loaders.
//! This probe never launches a game or changes the supplied source packages.
use super::*;
use app_storage::{StoragePaths, bootstrap_storage_with_paths};
use std::collections::BTreeSet;

const MODULES: [&str; 5] = [
    "corekeeper",
    "vrising",
    "sonsoftheforest",
    "barotrauma",
    "valheim",
];

fn copy_native_files(source: &Path, target: &Path, module: &str) -> BTreeMap<String, String> {
    let paths = if module == "valheim" {
        let mut files = super::super::commands_valheim_control::NATIVE_PREPARATION_FILES
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        files.insert("valheim_server.exe".into());
        files
    } else {
        let game = profile(module).unwrap();
        let mut files = payload::native_files(game)
            .unwrap()
            .into_iter()
            .map(|(relative, _)| relative.to_owned())
            .collect::<BTreeSet<_>>();
        files.insert(game.image.into());
        files
    };
    let mut hashes = BTreeMap::new();
    for relative in paths {
        let path = source.join(&relative);
        let _parents = io::pin_directories(path.parent().unwrap()).unwrap();
        let (_pin, bytes) = io::pin_regular(&path, 512 * 1024 * 1024).unwrap();
        let destination = target.join(&relative);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(&destination, &bytes).unwrap();
        hashes.insert(relative, io::digest(&bytes));
    }
    if module == "barotrauma" {
        // The instance factory materializes these package directories. Empty
        // fixture directories suffice because this probe never runs the game.
        fs::create_dir_all(target.join("Content")).unwrap();
        fs::create_dir_all(target.join("Data")).unwrap();
    }
    hashes
}

fn check_loader_is_reusable(runtime: &Path, module: &str) {
    if matches!(module, "vrising" | "valheim") {
        assert!(
            !super::super::commands_mod_staging::runtime_loader_requires_archive(module, runtime)
                .unwrap()
        );
    } else if module == "sonsoftheforest" {
        assert!(!redloader::requires_archive(runtime).unwrap());
        redloader::verify(runtime).unwrap();
    }
}

async fn prepare(
    storage: &StorageBootstrap,
    details: &InstanceDetails,
    operation: &crate::state::StorageContextOperationGuard,
) -> (String, String) {
    let mut environment = if details.summary.module_id == "valheim" {
        super::super::commands_valheim_control::ensure_for_start(storage, details, operation)
            .await
            .map(|entry| entry.into_iter().collect::<Vec<_>>())
    } else {
        ensure_for_start(storage, details, operation).await
    }
    .unwrap_or_else(|error| panic!("{}: {error}", details.summary.module_id));
    assert_eq!(
        environment.len(),
        1,
        "existing single-variable startup contract"
    );
    environment.pop().unwrap()
}

#[tokio::test]
#[ignore = "requires six reviewed local Windows game packages and official loader downloads"]
async fn automatic_managed_save_installs_real_loaders_compiles_sdk_and_rotates_startup_session() {
    let sources: BTreeMap<String, PathBuf> = serde_json::from_str(
        &std::env::var("LGSM_AUTO_SAVE_MANAGED_ROOTS")
            .expect("provide reviewed local game roots as JSON"),
    )
    .unwrap();
    assert_eq!(
        sources.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from(MODULES)
    );
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    for module in MODULES {
        let fixture = tests::Fixture::new();
        let storage = bootstrap_storage_with_paths(StoragePaths {
            app_data_root: fixture.0.join("app-data"),
            settings_path: fixture.0.join("app-data/settings.json"),
            database_path: fixture.0.join("app-data/db/probe.db"),
            logs_root: fixture.0.join("logs"),
            modules_root: workspace.join("modules"),
            migrations_root: workspace.join("migrations"),
            steamcmd_root: fixture.0.join("steamcmd"),
            games_root: fixture.0.join("games"),
            instances_root: fixture.0.join("instances"),
            archives_root: fixture.0.join("instances/.trash"),
        })
        .unwrap();
        initialize_database(&storage.paths).await.unwrap();
        let descriptors = discover_modules(&storage.paths.modules_root).unwrap();
        sync_modules(&storage.paths, &descriptors).await.unwrap();
        let descriptor = find_descriptor(&descriptors, module).unwrap();
        let library = storage.paths.games_root.join(module);
        let source_hashes = copy_native_files(&sources[module], &library, module);
        app_storage::record_library_program_baseline(&library, descriptor, true, None).unwrap();
        sync_game_installs(
            &storage.paths,
            &[GameInstallSyncRecord {
                module_id: module.into(),
                install_root: library.to_string_lossy().into_owned(),
                install_state: InstallState::Installed,
                current_version: Some("local-native-install-probe".into()),
                mark_verified: true,
            }],
        )
        .await
        .unwrap();
        let created = app_storage::create_instance_with_options(
            &storage.paths,
            descriptor,
            CreateInstanceInput {
                name: "Automatic native save installation probe".into(),
                module_id: module.into(),
            },
            app_storage::InstanceCreationOptions {
                program_mode: Some(app_core::InstanceProgramMode::Independent),
                ..Default::default()
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{module} fixture: {error}"));
        let details = read_instance_details(&storage.paths, &created.provisioning.summary.id)
            .await
            .unwrap();
        // Production startup renders Barotrauma's config-side entry before
        // preparing the hook. Creation alone intentionally does not copy it.
        let details = if module == "barotrauma" {
            app_storage::materialize_instance_configuration_for_start(
                &storage.paths,
                &details.summary.id,
            )
            .await
            .unwrap()
        } else {
            details
        };
        let instance_root =
            super::super::commands_program_storage::instance_root(&details).unwrap();
        assert_eq!(
            app_storage::instance_program_mode(instance_root).unwrap(),
            app_storage::InstanceProgramMode::Independent
        );
        let runtime = app_storage::resolve_instance_runtime_root(instance_root).unwrap();
        let state = DesktopState::from_storage(&storage);
        let operation = state
            .begin_storage_context_operation("isolated automatic managed save installation probe")
            .unwrap();
        let _mutation = state.acquire_instance_mutation(&details.summary.id).await;
        let _install = app_steamcmd::acquire_game_install_lifecycle(
            &state.install_locks,
            module,
            &[runtime.clone()],
        )
        .await
        .unwrap();
        let (plugin, session, expected_key) = if module == "valheim" {
            (
                "BepInEx/plugins/LanGame.Valheim.Control.dll",
                ".langame-valheim-control/session.json",
                "LGSM_VALHEIM_CONTROL_ROOT",
            )
        } else {
            let game = profile(module).unwrap();
            (
                game.plugin,
                ".langame-world-save/session.txt",
                game.environment,
            )
        };
        assert!(!runtime.join(plugin).exists());
        let first = prepare(&storage, &details, &operation).await;
        assert_eq!(first.0, expected_key);
        let plugin_bytes = io::read_regular(&runtime.join(plugin), 1024 * 1024).unwrap();
        assert_eq!(plugin_bytes.get(..2), Some(b"MZ".as_slice()));
        let initial_session = io::read_regular(&runtime.join(session), 4096).unwrap();
        check_loader_is_reusable(&runtime, module);
        let second = prepare(&storage, &details, &operation).await;
        assert_eq!(second, first);
        assert_eq!(
            io::read_regular(&runtime.join(plugin), 1024 * 1024).unwrap(),
            plugin_bytes
        );
        assert_ne!(
            io::read_regular(&runtime.join(session), 4096).unwrap(),
            initial_session
        );
        check_loader_is_reusable(&runtime, module);
        if module != "valheim" {
            assert!(payload::verify_prepared(&runtime, profile(module).unwrap()).is_ok());
        }
        if module == "barotrauma" {
            let config = Path::new(&details.config_file_path).parent().unwrap();
            let image = config.join("DedicatedServer.exe");
            let game = profile(module).unwrap();
            process::execution_pins(&details, &runtime, game, &image).unwrap();
            let assembly = config.join("DedicatedServer.dll");
            let original = io::read_regular(&assembly, 512 * 1024 * 1024).unwrap();
            let mut changed = original.clone();
            changed[0] ^= 1;
            fs::write(&assembly, &changed).unwrap();
            assert!(process::execution_pins(&details, &runtime, game, &image).is_err());
            fs::write(&assembly, &original).unwrap();
            assert!(process::execution_pins(&details, &runtime, game, &image).is_ok());
        }
        for (relative, original) in source_hashes {
            let source = sources[module].join(relative);
            assert_eq!(
                io::digest(&io::read_regular(&source, 512 * 1024 * 1024).unwrap()),
                original
            );
        }
        eprintln!(
            "{module}: native SDK + official loader + compiled save extension + repeated startup session passed; no game was launched"
        );
    }
}
