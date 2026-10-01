use super::*;
use app_storage::{StoragePaths, bootstrap_storage_with_paths};
use std::env;
use std::future::Future;
use std::task::{Context, Poll, Waker};

struct FixtureRoot(PathBuf);

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(env::temp_dir().as_path()));
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn program_update_policy_lease_serializes_program_writes_and_rejects_pending_starts()
-> Result<(), Box<dyn std::error::Error>> {
    let root = FixtureRoot(env::temp_dir().join(format!(
        "lgsm-program-policy-{}",
        uuid::Uuid::new_v4().simple()
    )));
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let storage = bootstrap_storage_with_paths(StoragePaths {
        app_data_root: root.0.join("app-data"),
        settings_path: root.0.join("app-data/settings.json"),
        database_path: root.0.join("app-data/db/lgs.db"),
        logs_root: root.0.join("app-data/logs"),
        modules_root: workspace.join("modules"),
        migrations_root: workspace.join("migrations"),
        steamcmd_root: root.0.join("steamcmd"),
        games_root: root.0.join("games"),
        instances_root: root.0.join("instances"),
        archives_root: root.0.join("instances/.trash"),
    })?;
    initialize_database(&storage.paths).await?;
    let descriptors = discover_modules(&storage.paths.modules_root)?;
    sync_modules(&storage.paths, &descriptors).await?;
    let descriptor = find_descriptor(&descriptors, "minecraft")?;
    let library = storage.paths.games_root.join("minecraft");
    fs::create_dir_all(library.join("jre/bin"))?;
    fs::write(library.join("server.jar"), b"inert server fixture")?;
    fs::write(library.join("jre/bin/java.exe"), b"inert java fixture")?;
    app_storage::record_library_program_baseline(&library, descriptor, true, None)?;
    sync_game_installs(
        &storage.paths,
        &[GameInstallSyncRecord {
            module_id: "minecraft".into(),
            install_root: library.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: None,
            mark_verified: true,
        }],
    )
    .await?;
    let created = app_storage::create_instance(
        &storage.paths,
        descriptor,
        CreateInstanceInput {
            name: "Policy lease fixture".into(),
            module_id: "minecraft".into(),
        },
    )
    .await?;
    let current = read_instance_details(&storage.paths, &created.summary.id).await?;
    let state = DesktopState::default();
    let _instance = state.acquire_instance_mutation(&current.summary.id).await;
    let pinned = r#"{"program_update":{"policy":"pinned"}}"#;
    let policy = acquire_policy_change(&state, &storage, &current, pinned)
        .await?
        .expect("policy changes must retain an install lease until saved");

    let roots = [library];
    let mut update = Box::pin(app_steamcmd::acquire_game_install_lifecycle(
        "minecraft",
        &roots,
    ));
    assert!(matches!(
        update
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
    drop(policy);
    let _updater = tokio::time::timeout(Duration::from_secs(5), update).await??;

    let _pending = match state.try_reserve_runtime_start(&current.summary.id, "manual")? {
        RuntimeStartReservationAttempt::Reserved(lease) => lease,
        other => panic!("expected a pending start, got {other:?}"),
    };
    assert!(
        acquire_policy_change(&state, &storage, &current, pinned)
            .await
            .is_err()
    );
    assert!(
        acquire_policy_change(&state, &storage, &current, &current.settings_json)
            .await?
            .is_none(),
        "unchanged policy must not add restrictions to ordinary settings saves"
    );
    Ok(())
}
