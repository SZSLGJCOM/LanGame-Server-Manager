use super::*;
use crate::commands::commands_instance_retirement::{Retirement, retire_instance};

pub(in crate::commands) async fn retirement_fixture(
    external_saves: bool,
) -> Result<
    (
        ProgramDataEnvGuard,
        tauri::App<tauri::test::MockRuntime>,
        StorageBootstrap,
        InstanceProvisioning,
    ),
    Box<dyn std::error::Error>,
> {
    let root = temp_test_dir("retirement");
    let environment = ProgramDataEnvGuard::set(&root.join("programdata"));
    let module_root = root.join("modules/retirementfixture");
    fs::create_dir_all(&module_root)?;
    let save_template = if external_saves {
        root.join("external-world").to_string_lossy().into_owned()
    } else {
        String::from("{{paths.instance_root}}/saves")
    };
    fs::write(
        module_root.join("module.toml"),
        format!(
            "id = \"retirementfixture\"\nname = \"Retirement fixture\"\nversion = \"1.0.0\"\n[storage]\nsaves_path_template = {}\n",
            serde_json::to_string(&save_template)?,
        ),
    )?;
    save_app_settings(AppSettings {
        archives_root: String::new(),
        servers_root: root.join("instances").to_string_lossy().into_owned(),
        games_root: root.join("games").to_string_lossy().into_owned(),
        modules_root: root.join("modules").to_string_lossy().into_owned(),
        steamcmd_root: root.join("steamcmd").to_string_lossy().into_owned(),
    })?;
    let storage = bootstrap_storage()?;
    initialize_database(&storage.paths).await?;
    // The desktop uses its bundled catalog. Register this inert fixture directly;
    // retirement must preserve recorded paths even when a module is unavailable.
    let descriptors = discover_modules(root.join("modules"))?;
    sync_modules(&storage.paths, &descriptors).await?;
    let descriptor = find_descriptor(&descriptors, "retirementfixture")?;
    let library = storage.paths.games_root.join("retirementfixture");
    fs::create_dir_all(&library)?;
    fs::write(
        library.join("server.bin"),
        b"inert retirement fixture program",
    )?;
    app_storage::record_library_program_baseline(&library, descriptor, true, None)?;
    sync_game_installs(
        &storage.paths,
        &[GameInstallSyncRecord {
            module_id: "retirementfixture".into(),
            install_root: library.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some("fixture-v1".into()),
            mark_verified: true,
        }],
    )
    .await?;
    let created = create_instance(
        &storage.paths,
        descriptor,
        CreateInstanceInput {
            name: "Retirement fixture".into(),
            module_id: "retirementfixture".into(),
        },
    )
    .await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    Ok((environment, app, storage, created))
}

#[tokio::test(flavor = "current_thread")]
async fn disconnected_delete_finishes_storage_cache_and_success_log()
-> Result<(), Box<dyn std::error::Error>> {
    let _lock = command_smoke_lock().lock().await;
    let (_environment, app, storage, created) = retirement_fixture(false).await?;
    let instance_id = created.summary.id;
    let instance_root = storage.paths.instances_root.join(&instance_id);
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let (completed_tx, completed_rx) = tokio::sync::oneshot::channel();
    let mut command = Box::pin(retire_instance(
        app.handle().clone(),
        instance_id.clone(),
        Retirement::Delete,
        move |paths, id, _install| async move {
            started_tx
                .send(())
                .expect("test waits for the admitted worker");
            release_rx.await.expect("test releases the admitted worker");
            let result = app_storage::delete_instance(&paths, &id).await;
            let _ = completed_tx.send(());
            result
        },
    ));
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::select! {
            started = started_rx => started.expect("worker should enter its owned operation"),
            result = &mut command => panic!("retirement completed before its gate: {result:?}"),
        }
    })
    .await?;
    assert!(instance_root.is_dir());
    assert!(
        app.state::<DesktopState>()
            .app_state
            .read()
            .unwrap()
            .instances
            .iter()
            .any(|item| item.id == instance_id)
    );
    drop(command);
    release_tx
        .send(())
        .expect("owned worker must survive caller disconnection");
    tokio::time::timeout(std::time::Duration::from_secs(10), completed_rx).await??;
    let state = app.state::<DesktopState>();
    let completed = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        state.acquire_instance_mutation(&instance_id),
    )
    .await?;
    assert!(!instance_root.exists());
    assert!(
        state
            .app_state
            .read()
            .unwrap()
            .instances
            .iter()
            .all(|item| item.id != instance_id)
    );
    assert!(list_instances(&storage.paths).await?.is_empty());
    let actions = read_desktop_log_actions(&desktop_app_log_path(&storage))?;
    assert!(
        actions
            .iter()
            .any(|action| action == "instance.delete.success")
    );
    drop(completed);
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn permanent_delete_preserves_external_saves_and_the_registered_library()
-> Result<(), Box<dyn std::error::Error>> {
    let _lock = command_smoke_lock().lock().await;
    let (_environment, app, storage, created) = retirement_fixture(true).await?;
    let details = read_instance_details(&storage.paths, &created.summary.id).await?;
    let saves = PathBuf::from(&details.saves_path);
    let instance_root = storage.paths.instances_root.join(&created.summary.id);
    assert!(!saves.starts_with(&instance_root));
    fs::create_dir_all(&saves)?;
    fs::write(saves.join("world.dat"), b"external player world")?;
    let library = app_storage::read_library_program_install(&storage.paths, "retirementfixture")
        .await?
        .unwrap();
    let result = delete_instance_record(app.handle().clone(), created.summary.id.clone()).await?;
    assert_eq!(
        result.preserved_external_saves_path.as_deref(),
        Some(saves.to_string_lossy().as_ref())
    );
    assert_eq!(PathBuf::from(result.deleted_instance_root), instance_root);
    assert!(!instance_root.exists());
    assert_eq!(fs::read(saves.join("world.dat"))?, b"external player world");
    assert_eq!(
        fs::read(library.install_root.join("server.bin"))?,
        b"inert retirement fixture program"
    );
    let current = app_storage::read_library_program_install(&storage.paths, "retirementfixture")
        .await?
        .unwrap();
    assert_eq!(current.id, library.id);
    assert_eq!(current.install_root, library.install_root);
    assert_eq!(current.install_state, InstallState::Installed);
    assert_eq!(current.current_version, library.current_version);
    assert_eq!(current.scope, app_storage::ProgramInstallScope::Library);
    assert_eq!(current.owner_instance_id, None);
    let archives = app_storage::list_instance_archives(&storage.paths).await?;
    assert!(archives.archives.is_empty());
    assert!(archives.pending_deletions.is_empty());
    assert!(
        app.state::<DesktopState>()
            .app_state
            .read()
            .unwrap()
            .instances
            .is_empty()
    );
    Ok(())
}
