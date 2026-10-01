use super::*;

async fn fixture() -> (DeletionFixture, InstanceDetails) {
    let descriptor = repo_descriptors()
        .into_iter()
        .find(|item| item.summary.id == "astroneer")
        .unwrap();
    let fixture = DeletionFixture::new(&descriptor).await;
    let instance = fixture.create("Reconcile missing directory").await;
    (fixture, instance)
}

#[tokio::test]
async fn reconciliation_removes_missing_registration_and_preserves_external_data_and_peer() {
    let (fixture, instance) = fixture().await;
    let peer = fixture.create("Retained peer").await;
    let external = fixture.root.join("external-world");
    fs::create_dir_all(&external).unwrap();
    fs::write(external.join("world"), b"retained external world").unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instances SET saves_path=?2 WHERE id=?1")
        .bind(&instance.summary.id)
        .bind(external.to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let root = instance_root(&instance);
    fs::remove_dir_all(&root).unwrap();
    assert_eq!(
        list_instances(&fixture.paths).await.unwrap().len(),
        2,
        "listing stays read-only"
    );
    assert_eq!(
        list_missing_instance_candidates(&fixture.paths)
            .await
            .unwrap(),
        vec![instance.summary.id.clone()]
    );
    assert!(
        reconcile_missing_instance(&fixture.paths, &instance.summary.id)
            .await
            .unwrap()
    );
    assert!(
        !reconcile_missing_instance(&fixture.paths, &instance.summary.id)
            .await
            .unwrap()
    );
    assert_eq!(
        list_instances(&fixture.paths).await.unwrap()[0].id,
        peer.summary.id
    );
    assert!(
        list_missing_instance_candidates(&fixture.paths)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fs::read(external.join("world")).unwrap(),
        b"retained external world"
    );
    assert_eq!(
        fs::read(fixture.install_root.join("package.fixture")).unwrap(),
        b"immutable package"
    );
    assert!(!root.exists());
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let ports: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM instance_ports WHERE instance_id=?1")
        .bind(&instance.summary.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let installs: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM game_installs WHERE owner_instance_id=?1")
            .bind(&instance.summary.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((ports, installs), (0, 0));
    pool.close().await;
    // Reopen the database through the public API after the mutation pool closed.
    assert!(
        read_instance_details(&fixture.paths, &peer.summary.id)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn reconciliation_preserves_partial_deletion_and_reappeared_roots() {
    let (fixture, instance) = fixture().await;
    let root = instance_root(&instance);
    fs::remove_dir_all(root.join("runtime")).unwrap();
    assert!(
        list_missing_instance_candidates(&fixture.paths)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        !reconcile_missing_instance(&fixture.paths, &instance.summary.id)
            .await
            .unwrap()
    );
    fs::remove_dir_all(&root).unwrap();
    assert_eq!(
        list_missing_instance_candidates(&fixture.paths)
            .await
            .unwrap()
            .len(),
        1
    );
    fs::create_dir(&root).unwrap();
    fs::write(root.join("returned-data"), b"keep").unwrap();
    assert!(
        !reconcile_missing_instance(&fixture.paths, &instance.summary.id)
            .await
            .unwrap()
    );
    assert_eq!(fs::read(root.join("returned-data")).unwrap(), b"keep");
    assert_eq!(list_instances(&fixture.paths).await.unwrap().len(), 1);
}

#[tokio::test]
async fn reconciliation_preserves_active_states_runs_and_journals() {
    let (fixture, instance) = fixture().await;
    fs::remove_dir_all(instance_root(&instance)).unwrap();
    for status in ["starting", "running", "stopping"] {
        fixture.set_status(&instance, status).await;
        assert!(
            list_missing_instance_candidates(&fixture.paths)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            !reconcile_missing_instance(&fixture.paths, &instance.summary.id)
                .await
                .unwrap()
        );
    }
    fixture.set_status(&instance, "stopped").await;
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("INSERT INTO instance_runs(instance_id,status) VALUES (?1,'running')")
        .bind(&instance.summary.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        list_missing_instance_candidates(&fixture.paths)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        !reconcile_missing_instance(&fixture.paths, &instance.summary.id)
            .await
            .unwrap()
    );
    sqlx::query("DELETE FROM instance_runs WHERE instance_id=?1")
        .bind(&instance.summary.id)
        .execute(&pool)
        .await
        .unwrap();
    for state in ["archiving", "restoring", "purging", "archived"] {
        sqlx::query("INSERT INTO instance_archives(archive_id,instance_id,archive_leaf,state) VALUES ('pending',?1,'pending',?2)")
            .bind(&instance.summary.id).bind(state).execute(&pool).await.unwrap();
        assert!(
            list_missing_instance_candidates(&fixture.paths)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            !reconcile_missing_instance(&fixture.paths, &instance.summary.id)
                .await
                .unwrap()
        );
        sqlx::query("DELETE FROM instance_archives WHERE archive_id='pending'")
            .execute(&pool)
            .await
            .unwrap();
    }
    pool.close().await;
    assert_eq!(list_instances(&fixture.paths).await.unwrap().len(), 1);
}

#[tokio::test]
async fn reconciliation_preserves_missing_parent_and_contended_instance() {
    let (fixture, instance) = fixture().await;
    let held = crate::instance_settings_lock::acquire_instance_settings_mutation_lock(
        &fixture.paths,
        &instance.summary.id,
    )
    .unwrap();
    fs::remove_dir_all(instance_root(&instance)).unwrap();
    assert!(matches!(
        reconcile_missing_instance(&fixture.paths, &instance.summary.id).await,
        Err(StorageError::InstanceSettingsLocked { .. })
    ));
    drop(held);
    fs::remove_dir_all(&fixture.paths.instances_root).unwrap();
    assert!(
        list_missing_instance_candidates(&fixture.paths)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        !reconcile_missing_instance(&fixture.paths, &instance.summary.id)
            .await
            .unwrap()
    );
    assert!(
        !fixture.paths.instances_root.exists(),
        "reconciliation must not recreate an unavailable parent"
    );
    assert_eq!(list_instances(&fixture.paths).await.unwrap().len(), 1);
}

#[tokio::test]
async fn reconciliation_rejects_outside_and_overlapping_registered_paths() {
    let (fixture, instance) = fixture().await;
    let peer = fixture.create("Peer owner").await;
    let root = instance_root(&instance);
    fs::remove_dir_all(&root).unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instances SET config_path=?2 WHERE id=?1")
        .bind(&instance.summary.id)
        .bind(
            fixture
                .root
                .join("outside/config")
                .to_string_lossy()
                .as_ref(),
        )
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        reconcile_missing_instance(&fixture.paths, &instance.summary.id)
            .await
            .is_err()
    );
    sqlx::query("UPDATE instances SET config_path=?2 WHERE id=?1")
        .bind(&instance.summary.id)
        .bind(root.join("config").to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE instances SET saves_path=?2 WHERE id=?1")
        .bind(&peer.summary.id)
        .bind(root.join("peer-saves").to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert!(
        reconcile_missing_instance(&fixture.paths, &instance.summary.id)
            .await
            .is_err()
    );
    assert_eq!(list_instances(&fixture.paths).await.unwrap().len(), 2);
}

#[cfg(windows)]
#[tokio::test]
async fn reconciliation_rejects_junction_parent_without_touching_registration() {
    use std::os::windows::process::CommandExt;
    let (fixture, instance) = fixture().await;
    fs::remove_dir_all(instance_root(&instance)).unwrap();
    let outside = fixture.root.join("relocated-instances");
    fs::rename(&fixture.paths.instances_root, &outside).unwrap();
    let output = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&fixture.paths.instances_root)
        .arg(&outside)
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(output.status.success());
    let result = reconcile_missing_instance(&fixture.paths, &instance.summary.id).await;
    fs::remove_dir(&fixture.paths.instances_root).unwrap();
    assert!(result.is_err());
    assert_eq!(list_instances(&fixture.paths).await.unwrap().len(), 1);
}

#[tokio::test]
async fn reconciliation_prefilter_skips_invalid_peer_and_missing_shared_root_can_be_deleted() {
    let descriptor = repo_descriptors()
        .into_iter()
        .find(|item| item.summary.id == "minecraft")
        .unwrap();
    let fixture = DeletionFixture::new(&descriptor).await;
    let instance = fixture.create("Deleted shared instance").await;
    let peer = fixture.create("Invalid peer").await;
    assert_eq!(
        instance_program_mode(&instance_root(&instance)).unwrap(),
        InstanceProgramMode::Shared
    );
    fs::remove_dir_all(instance_root(&instance)).unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instances SET config_path=?2 WHERE id=?1")
        .bind(&peer.summary.id)
        .bind(
            fixture
                .root
                .join("outside/config")
                .to_string_lossy()
                .as_ref(),
        )
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert_eq!(
        list_missing_instance_candidates(&fixture.paths)
            .await
            .unwrap(),
        vec![instance.summary.id.clone()]
    );
    let plan = inspect_instance_removal(&fixture.paths, &instance.summary.id)
        .await
        .unwrap();
    assert!(!plan.remove_program);
    assert!(plan.owned_data_paths.is_empty());
    delete_instance(&fixture.paths, &instance.summary.id)
        .await
        .unwrap();
    assert_eq!(
        fs::read(fixture.install_root.join("package.fixture")).unwrap(),
        b"immutable package"
    );
    assert_eq!(
        list_instances(&fixture.paths).await.unwrap()[0].id,
        peer.summary.id
    );
}

#[tokio::test]
async fn stored_settings_remain_readable_without_runtime_and_default_only_when_file_is_missing() {
    let (fixture, instance) = fixture().await;
    let config: Value =
        serde_json::from_slice(&fs::read(&instance.config_file_path).unwrap()).unwrap();
    fs::remove_dir_all(instance_root(&instance).join("runtime")).unwrap();
    let (summary, settings) = read_instance_stored_settings(&fixture.paths, &instance.summary.id)
        .await
        .unwrap();
    assert_eq!(summary.name, instance.summary.name);
    assert_eq!(
        serde_json::from_str::<Value>(&settings).unwrap(),
        config["settings"]
    );
    fs::write(&instance.config_file_path, b"broken JSON").unwrap();
    assert!(
        read_instance_stored_settings(&fixture.paths, &instance.summary.id)
            .await
            .is_err()
    );
    fs::remove_dir_all(instance_root(&instance)).unwrap();
    assert_eq!(
        read_instance_stored_settings(&fixture.paths, &instance.summary.id)
            .await
            .unwrap()
            .1,
        "{}"
    );
    assert!(!instance_root(&instance).exists());
}

#[tokio::test]
async fn explicit_deletion_handles_empty_root_and_invalid_configuration() {
    for empty in [true, false] {
        let (fixture, instance) = fixture().await;
        let root = instance_root(&instance);
        if empty {
            fs::remove_dir_all(&root).unwrap();
            fs::create_dir(&root).unwrap();
        } else {
            fs::write(&instance.config_file_path, b"broken JSON").unwrap();
        }
        assert!(
            !reconcile_missing_instance(&fixture.paths, &instance.summary.id)
                .await
                .unwrap()
        );
        assert!(
            archive_instance(&fixture.paths, &instance.summary.id)
                .await
                .is_err()
        );
        inspect_instance_removal(&fixture.paths, &instance.summary.id)
            .await
            .unwrap();
        read_instance_retirement_resources(&fixture.paths, &instance.summary.id)
            .await
            .unwrap();
        delete_instance(&fixture.paths, &instance.summary.id)
            .await
            .unwrap();
        assert!(!root.exists());
        assert!(list_instances(&fixture.paths).await.unwrap().is_empty());
        assert_eq!(
            fs::read(fixture.install_root.join("package.fixture")).unwrap(),
            b"immutable package"
        );
    }
}

#[tokio::test]
async fn explicit_deletion_detects_configuration_reappearing_after_admission() {
    use crate::instance_archive::test_gate::{self, Point};
    let (fixture, instance) = fixture().await;
    let root = instance_root(&instance);
    fs::remove_dir_all(&root).unwrap();
    fs::create_dir(&root).unwrap();
    let mut gate = test_gate::register(&fixture.paths.database_path, Point::Archiving);
    let paths = fixture.paths.clone();
    let id = instance.summary.id.clone();
    let worker = tokio::spawn(async move { delete_instance(&paths, &id).await });
    gate.reached().await;
    fs::create_dir(root.join("config")).unwrap();
    fs::write(&instance.config_file_path, b"new configuration").unwrap();
    gate.resume();
    let error = worker.await.unwrap().unwrap_err();
    assert!(
        error.to_string().contains("configuration changed"),
        "{error}"
    );
    assert_eq!(
        fs::read(&instance.config_file_path).unwrap(),
        b"new configuration"
    );
    assert_eq!(list_instances(&fixture.paths).await.unwrap().len(), 1);
    assert_eq!(
        list_instance_archives(&fixture.paths)
            .await
            .unwrap()
            .pending_deletions
            .len(),
        1
    );
}

#[tokio::test]
async fn explicit_deletion_missing_configuration_preserves_library_and_uses_registered_saves() {
    for module in ["minecraft", "abioticfactor"] {
        let descriptor = repo_descriptors()
            .into_iter()
            .find(|item| item.summary.id == module)
            .unwrap();
        let fixture = DeletionFixture::new(&descriptor).await;
        let instance = fixture.create("Deleted configuration").await;
        fs::remove_file(&instance.config_file_path).unwrap();
        assert!(
            archive_instance(&fixture.paths, &instance.summary.id)
                .await
                .is_err()
        );
        inspect_instance_removal(&fixture.paths, &instance.summary.id)
            .await
            .unwrap();
        let resources = read_instance_retirement_resources(&fixture.paths, &instance.summary.id)
            .await
            .unwrap();
        assert!(resources.program_roots.iter().any(|path| {
            path == &crate::instance_isolation::paths::normalize_path(Path::new(
                &instance.saves_path,
            ))
            .unwrap()
        }));
        delete_instance(&fixture.paths, &instance.summary.id)
            .await
            .unwrap();
        assert!(list_instances(&fixture.paths).await.unwrap().is_empty());
        assert_eq!(
            fs::read(fixture.install_root.join("package.fixture")).unwrap(),
            b"immutable package"
        );
    }
}
