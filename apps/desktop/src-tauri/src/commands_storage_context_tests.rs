#![cfg(test)]

use super::*;
use crate::commands::commands_instance_retirement::archive_instance_record;
use crate::commands::commands_storage_management::{
    InstanceArchiveInput, list_instance_archives, restore_instance_archive,
};
use crate::commands::tests::{ProgramDataEnvGuard, prepare_fake_registered_program, temp_test_dir};

const CHILD: &str = "commands::commands_storage::context_tests::storage_context_process_fixture";
const ROOT_VARIABLE: &str = "LANGAME_STORAGE_CONTEXT_TEST_ROOT";

#[tokio::test]
async fn storage_context_commands_keep_their_data_after_environment_changes() {
    let root = temp_test_dir("context");
    // Only the child changes environment variables. It runs one fixture, so
    // neither the parent harness nor another test can observe those changes.
    let stdout_path = root.join("stdout.log");
    let stderr_path = root.join("stderr.log");
    let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            CHILD,
            "--ignored",
            "--test-threads=1",
            "--nocapture",
        ])
        .env(ROOT_VARIABLE, &root)
        .stdout(fs::File::create(&stdout_path).unwrap())
        .stderr(fs::File::create(&stderr_path).unwrap())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let completed = tokio::time::timeout(Duration::from_secs(60), child.wait()).await;
    let success = matches!(&completed, Ok(Ok(status)) if status.success());
    if !success && child.try_wait().unwrap().is_none() {
        child.kill().await.unwrap();
        child.wait().await.unwrap();
    }
    let stdout = fs::read_to_string(&stdout_path).unwrap();
    let stderr = fs::read_to_string(&stderr_path).unwrap();
    assert!(
        success && stdout.contains("test result: ok. 1 passed;"),
        "storage context fixture failed ({completed:?}); retained {}\n{stdout}\n{stderr}",
        root.display(),
    );
    fs::remove_dir_all(root).unwrap();
}

fn fixture_paths(root: &Path) -> app_storage::StoragePaths {
    let workspace = dunce::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."))
        .expect("workspace root is available to the storage fixture");
    app_storage::StoragePaths {
        app_data_root: root.join("app-data"),
        settings_path: root.join("app-data/settings.json"),
        database_path: root.join("app-data/db/lgs.db"),
        logs_root: root.join("app-data/logs"),
        modules_root: workspace.join("modules"),
        migrations_root: workspace.join("migrations"),
        steamcmd_root: root.join("steamcmd"),
        games_root: root.join("games"),
        instances_root: root.join("instances"),
        archives_root: root.join("instances/.trash"),
    }
}

#[tokio::test]
#[ignore = "child entry: changes only its own environment and disposable storage"]
async fn storage_context_process_fixture() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::var_os(ROOT_VARIABLE).ok_or("missing child fixture root")?);
    let paths = fixture_paths(&root.join("owned"));
    let storage = app_storage::bootstrap_storage_with_paths(paths.clone())?;
    app_storage::save_app_settings_with_paths(storage.settings.clone(), &paths)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;

    let _environment = ProgramDataEnvGuard::set(&root.join("ambient/programdata"));
    let ambient = bootstrap_storage()?;
    let ambient_settings = fs::read(&ambient.paths.settings_path)?;
    let status = ensure_storage_ready(app.state::<DesktopState>()).await?;
    assert_eq!(Path::new(&status.database_path), paths.database_path);
    assert!(!ambient.paths.database_path.exists());
    let descriptors = discover_modules(&paths.modules_root)?;
    let descriptor = find_descriptor(&descriptors, "astroneer")?;
    sync_modules(&paths, std::slice::from_ref(descriptor)).await?;
    prepare_fake_registered_program(&paths, descriptor).await?;
    let created = crate::commands::tests::creation_lifecycle_tests::CREATION_INSTALL_FORBIDDEN
        .scope(
            true,
            create_instance_record_inner(
                app.state::<DesktopState>(),
                CreateInstanceInput {
                    name: "Owned context".into(),
                    module_id: "astroneer".into(),
                },
            ),
        )
        .await?;
    let id = created.summary.id;
    assert!(paths.instances_root.join(&id).is_dir());
    let preview = crate::commands::commands_runtime_lifecycle::preview_instance_launch(
        app.state::<DesktopState>(),
        id.clone(),
    )
    .await?;
    assert_eq!(preview.instance_id, id);
    let preview_root = dunce::canonicalize(&preview.install_root)?;
    assert!(
        preview_root.starts_with(dunce::canonicalize(&paths.instances_root)?)
            || preview_root.starts_with(dunce::canonicalize(&paths.games_root)?)
    );
    let stopped =
        crate::commands::commands_runtime_lifecycle::stop_instance_process_with_precondition(
            app.state::<DesktopState>(),
            id.clone(),
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(
        serde_json::from_str::<Value>(&stopped)?["code"],
        "instance_not_running"
    );
    crate::commands::commands_runtime_supervision::log_frontend_event(
        app.state::<DesktopState>(),
        "info".into(),
        "storage.context.runtime".into(),
        "Runtime operations retain their application context".into(),
        None,
    )
    .await?;
    assert!(fs::read_to_string(paths.app_log_path())?.contains("storage.context.runtime"));
    assert!(!ambient.paths.app_log_path().exists());
    assert!(!ambient.paths.database_path.exists());
    let details =
        read_instance_details_from_storage(app.state::<DesktopState>(), id.clone()).await?;
    let world = Path::new(&details.saves_path).join("context-world.dat");
    fs::create_dir_all(world.parent().unwrap())?;
    fs::write(&world, b"world belongs to the original context")?;
    let backup = create_instance_backup(app.state::<DesktopState>(), id.clone()).await?;
    assert!(
        Path::new(&backup.backup_path)
            .canonicalize()?
            .starts_with(paths.instances_root.canonicalize()?)
    );
    assert_eq!(
        list_instance_backups(app.state::<DesktopState>(), id.clone())
            .await?
            .len(),
        1
    );

    let archived = archive_instance_record(app.handle().clone(), id.clone()).await?;
    let archives = list_instance_archives(app.state::<DesktopState>()).await?;
    assert_eq!(archives.archives.len(), 1);
    assert!(list_instances(&paths).await?.is_empty());
    restore_instance_archive(
        app.handle().clone(),
        InstanceArchiveInput {
            archive_id: archived.archive_id,
        },
    )
    .await?;
    assert_eq!(fs::read(&world)?, b"world belongs to the original context");
    assert_eq!(
        list_instances_from_storage(app.state::<DesktopState>())
            .await?
            .len(),
        1
    );

    let next_games = root.join("selected-games");
    let next_steamcmd = root.join("selected-steamcmd");
    let input = || AppPathSettingsInput {
        servers_root: storage.settings.servers_root.clone(),
        archives_root: storage.settings.archives_root.clone(),
        games_root: next_games.to_string_lossy().into_owned(),
        steamcmd_root: next_steamcmd.to_string_lossy().into_owned(),
    };
    let state = app.state::<DesktopState>();
    let operation = state.begin_storage_context_operation("held context fixture")?;
    let before = fs::read(&paths.settings_path)?;
    assert!(
        update_app_settings(app.state::<DesktopState>(), input())
            .await
            .is_err()
    );
    assert_eq!(fs::read(&paths.settings_path)?, before);
    drop(operation);
    let saved = update_app_settings(app.state::<DesktopState>(), input()).await?;
    assert_eq!(Path::new(&saved.games_root), next_games);
    let refreshed = state.bootstrap_storage()?;
    assert_eq!(refreshed.paths.database_path, paths.database_path);
    assert_eq!(refreshed.paths.games_root, next_games);
    assert_eq!(refreshed.paths.steamcmd_root, next_steamcmd);
    let restarted = app_storage::bootstrap_storage_with_paths(paths.clone())?;
    assert_eq!(restarted.paths.games_root, next_games);
    assert_eq!(restarted.paths.steamcmd_root, next_steamcmd);
    assert_eq!(fs::read(&world)?, b"world belongs to the original context");
    assert_eq!(fs::read(&ambient.paths.settings_path)?, ambient_settings);
    assert!(!ambient.paths.database_path.exists());

    // A missing configured container must be diagnosed before any bootstrap
    // recreates it and makes retained instance records appear to be deletions.
    let retained = root.join("retained-instances");
    fs::rename(&paths.instances_root, &retained)?;
    let missing = state.bootstrap_storage().unwrap_err();
    assert!(
        missing.contains("Configured instance storage directory is missing"),
        "{missing}"
    );
    assert!(!paths.instances_root.exists());
    fs::rename(retained, &paths.instances_root)?;
    assert!(state.bootstrap_storage().is_ok());
    assert_eq!(fs::read(world)?, b"world belongs to the original context");
    verify_saved_location_protection(&root.join("location")).await?;
    Ok(())
}

async fn verify_saved_location_protection(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let environment = ProgramDataEnvGuard::set(&root.join("locator/programdata"));
    let user_root = app_storage::StoragePaths::default().app_data_root;
    let selected_root = root.join("LanGame");
    let app_data_root = selected_root
        .join("app-data/ServerManager")
        .join(uuid::Uuid::new_v4().to_string());
    let paths = app_storage::StoragePaths {
        settings_path: app_data_root.join("settings.json"),
        database_path: app_data_root.join("db/lgs.db"),
        logs_root: app_data_root.join("logs"),
        app_data_root,
        ..fixture_paths(&selected_root)
    };
    let storage = app_storage::bootstrap_storage_with_paths(paths.clone())?;
    app_storage::save_app_settings_with_paths(storage.settings, &paths)?;
    initialize_database(&paths).await?;
    let pointer = user_root.join("storage-location.json");
    fs::write(&pointer, b"interrupted location metadata")?;
    let state = DesktopState::default();
    assert!(state.bootstrap_storage().is_err());
    #[cfg(windows)]
    let client_app = tauri::test::mock_builder()
        .manage(state.storage_source())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    drop(environment);

    // Repair the originally selected location after the ambient user root has
    // changed. The failed attempt must neither poison nor redirect this state.
    fs::write(
        pointer,
        serde_json::to_vec(&json!({
            "version": 1,
            "runtime_root": selected_root,
            "app_data_root": paths.app_data_root,
        }))?,
    )?;
    assert_eq!(
        state.bootstrap_storage()?.paths.database_path,
        paths.database_path
    );
    let app = tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    ensure_storage_ready(app.state::<DesktopState>()).await?;
    let state = app.state::<DesktopState>();
    #[cfg(windows)]
    {
        let ambient = app_storage::StoragePaths::resolve_existing_default()?
            .ok_or("ambient fixture storage is missing")?;
        assert!(!ambient.app_log_path().exists());
        for (handle, action) in [
            (app.handle(), "storage.context.service_log"),
            (client_app.handle(), "storage.context.client_log"),
        ] {
            crate::runtime_service::log_service_event(
                handle,
                "info",
                action,
                "Original data location",
            );
            assert!(fs::read_to_string(paths.app_log_path())?.contains(action));
        }
        assert!(!ambient.app_log_path().exists());
        assert!(!ambient.database_path.exists());
    }
    assert_eq!(
        state.app_state.read().unwrap().settings.games_root,
        paths.games_root.to_string_lossy()
    );
    let next_games = selected_root.join("updated-games");
    let input = || AppPathSettingsInput {
        servers_root: paths.instances_root.to_string_lossy().into_owned(),
        archives_root: paths.archives_root.to_string_lossy().into_owned(),
        games_root: next_games.to_string_lossy().into_owned(),
        steamcmd_root: paths.steamcmd_root.to_string_lossy().into_owned(),
    };
    let operation = state.begin_storage_context_operation("recovered storage fixture")?;
    let before = fs::read(&paths.settings_path)?;
    let error = update_app_settings(app.state::<DesktopState>(), input())
        .await
        .unwrap_err();
    assert!(
        error.contains("storage operation is in progress"),
        "{error}"
    );
    assert_eq!(fs::read(&paths.settings_path)?, before);
    drop(operation);
    let saved = update_app_settings(app.state::<DesktopState>(), input()).await?;
    assert_eq!(Path::new(&saved.games_root), next_games);
    assert_eq!(state.bootstrap_storage()?.paths.games_root, next_games);

    let retained = root.join("retained-selected-directory");
    fs::rename(&selected_root, &retained)?;
    assert!(state.bootstrap_storage().is_err());
    #[cfg(windows)]
    for handle in [app.handle(), client_app.handle()] {
        crate::runtime_service::log_service_event(
            handle,
            "error",
            "storage.context.missing_directory",
            "Selected storage is unavailable",
        );
    }
    assert!(!selected_root.exists());
    fs::rename(retained, &selected_root)?;
    for path in [&paths.settings_path, &paths.database_path] {
        let retained = path.with_extension("retained");
        fs::rename(path, &retained)?;
        assert!(state.bootstrap_storage().is_err());
        #[cfg(windows)]
        {
            let before_log = fs::read(paths.app_log_path())?;
            for handle in [app.handle(), client_app.handle()] {
                crate::runtime_service::log_service_event(
                    handle,
                    "error",
                    "storage.context.missing_data",
                    "Selected storage is incomplete",
                );
            }
            assert_eq!(fs::read(paths.app_log_path())?, before_log);
        }
        assert!(!path.exists());
        fs::rename(retained, path)?;
    }
    assert_eq!(
        state.bootstrap_storage()?.paths.database_path,
        paths.database_path
    );
    Ok(())
}
