use super::*;

pub(super) async fn exclusive_fixture(baseline: bool) -> (Fixture, PathBuf) {
    let mut fixture = Fixture::new().await;
    fixture.paths.modules_root = fixture.root.join("modules");
    fs::create_dir_all(fixture.paths.modules_root.join("archive-fixture")).unwrap();
    fs::write(fixture.paths.modules_root.join("archive-fixture/module.toml"),
        "id='archive-fixture'\nname='Archive fixture'\nversion='1'\n[storage]\nretained_paths=['native','Mods']\nruntime_copy_exclusions=['unowned']\n").unwrap();
    let descriptor = app_modules::discover_modules(&fixture.paths.modules_root)
        .unwrap()
        .remove(0);
    let library = fixture.paths.games_root.join("archive-fixture");
    fs::create_dir_all(&library).unwrap();
    fs::write(library.join("server.exe"), b"official bytes").unwrap();
    if baseline {
        crate::record_library_program_baseline(&library, &descriptor, true, None).unwrap();
    }
    for directory in ["native", "Mods", "unowned"] {
        fs::create_dir_all(library.join(directory)).unwrap();
    }
    for (file, bytes) in [
        ("native/world.dat", "world"),
        ("Mods/custom.dll", "mod"),
        ("custom.dll", "custom"),
        ("unowned/foreign.dat", "foreign"),
    ] {
        fs::write(library.join(file), bytes).unwrap();
    }
    fs::remove_file(
        fixture
            .instance_root
            .join("runtime/.langame-private-runtime"),
    )
    .unwrap();
    fs::remove_dir(fixture.instance_root.join("runtime")).unwrap();
    crate::program_runtime::prepare_exclusive_program_reference(
        &library,
        &fixture.instance_root,
        "archive-fixture",
    )
    .unwrap();
    crate::program_runtime::record_exclusive_program_use(&library, &fixture.instance_root).unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE game_installs SET scope='library',owner_instance_id=NULL,install_root=?1,install_state='installed',current_version='build-1' WHERE id=17")
        .bind(library.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
    sqlx::query("UPDATE instances SET saves_path=?1 WHERE id='archive-instance'")
        .bind(library.join("native").to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    (fixture, library)
}

async fn archived_snapshot(fixture: &Fixture, id: &str) -> store::Snapshot {
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let archive = store::load(&pool, id).await.unwrap();
    pool.close().await;
    store::snapshot(&archive).unwrap()
}

#[tokio::test]
async fn external_archive_saves_custom_bytes_removes_only_owned_data_and_restores_exact_source() {
    let (fixture, library) = exclusive_fixture(true).await;
    let before = crate::test_file_snapshot::tree_snapshot(&library).unwrap();
    let archived = fixture.archive().await.unwrap();
    let snapshot = archived_snapshot(&fixture, &archived.archive_id).await;
    let plan = snapshot.external_program.unwrap();
    assert!(plan.requires_source());
    assert!(!plan.files["server.exe"].stored);
    assert!(plan.files["custom.dll"].stored);
    assert!(!plan.files["custom.dll"].owned);
    assert!(
        !plan.files["unowned/foreign.dat"].owned,
        "copy exclusions are not deletion ownership"
    );
    assert!(!library.join("native/world.dat").exists());
    assert!(
        library.join("Mods/custom.dll").exists(),
        "retention declarations do not establish exclusive ownership"
    );
    assert_eq!(fs::read(library.join("custom.dll")).unwrap(), b"custom");
    assert_eq!(
        fs::read(library.join("unowned/foreign.dat")).unwrap(),
        b"foreign"
    );
    let summary = list_instance_archives(&fixture.paths)
        .await
        .unwrap()
        .archives
        .remove(0);
    assert_eq!(summary.program_storage, "reconstructable");
    assert!(summary.can_restore, "{:?}", summary.issues);
    assert!(
        external::ensure_program_archive_dependencies(&fixture.paths, &library)
            .await
            .is_err()
    );
    restore_instance_archive(&fixture.paths, &archived.archive_id)
        .await
        .unwrap();
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&library).unwrap(),
        before
    );
    assert!(!fixture.instance_root.join(external::PAYLOAD).exists());
    assert!(
        external::ensure_program_archive_dependencies(&fixture.paths, &library)
            .await
            .is_ok()
    );
    assert!(
        fixture.archive().await.is_ok(),
        "restoration must allow a later archive"
    );
}

#[tokio::test]
async fn external_delete_preserves_installation_and_unknown_files() {
    let (fixture, library) = exclusive_fixture(true).await;
    let preview = external::inspect_instance_removal(&fixture.paths, "archive-instance")
        .await
        .unwrap();
    assert!(!preview.remove_program);
    assert!(preview.preserved_program_path.is_some());
    assert!(preview.preserved_external_saves_path.is_none());
    assert!(
        preview
            .owned_data_paths
            .iter()
            .all(|path| !path.contains("unowned"))
    );
    crate::delete_instance(&fixture.paths, "archive-instance")
        .await
        .unwrap();
    assert!(library.join("server.exe").exists());
    assert!(library.join("custom.dll").exists());
    assert!(library.join("unowned/foreign.dat").exists());
    assert!(!library.join("native/world.dat").exists());
    assert!(library.join("Mods/custom.dll").exists());
    assert!(library.join(".langame-program-usage.json").exists());
    let pool = connect_pool(&fixture.paths).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM game_installs WHERE id=17 AND scope='library'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );
    pool.close().await;
}

#[tokio::test]
async fn external_full_archive_restores_missing_installation_without_a_download() {
    let (fixture, library) = exclusive_fixture(false).await;
    let before = crate::test_file_snapshot::tree_snapshot(&library).unwrap();
    let archived = fixture.archive().await.unwrap();
    assert_eq!(
        list_instance_archives(&fixture.paths)
            .await
            .unwrap()
            .archives[0]
            .program_storage,
        "full"
    );
    // Fixture-owned temporary installation simulates external loss of the source.
    fs::remove_dir_all(&library).unwrap();
    assert!(
        external::ensure_program_archive_dependencies(&fixture.paths, &library)
            .await
            .is_ok()
    );
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query(
        "UPDATE game_installs SET install_state='not_installed',current_version=NULL WHERE id=17",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;
    restore_instance_archive(&fixture.paths, &archived.archive_id)
        .await
        .unwrap();
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&library).unwrap(),
        before
    );
    assert!(
        crate::program_runtime::instance_uses_exclusive_program(&fixture.instance_root).unwrap()
    );
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let state: String = sqlx::query_scalar("SELECT install_state FROM game_installs WHERE id=17")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "installed");
    pool.close().await;
}

#[tokio::test]
async fn external_restore_rejects_program_changes_and_unknown_target_conflicts() {
    let (fixture, library) = exclusive_fixture(true).await;
    let archived = fixture.archive().await.unwrap();
    fs::write(library.join("server.exe"), b"new version").unwrap();
    assert!(
        restore_instance_archive(&fixture.paths, &archived.archive_id)
            .await
            .is_err()
    );
    assert_eq!(
        fs::read(library.join("server.exe")).unwrap(),
        b"new version"
    );
    assert!(!library.join("native/world.dat").exists());
    fs::write(library.join("server.exe"), b"official bytes").unwrap();
    fs::write(library.join("native/world.dat"), b"other world").unwrap();
    assert!(
        restore_instance_archive(&fixture.paths, &archived.archive_id)
            .await
            .is_err()
    );
    assert_eq!(
        fs::read(library.join("native/world.dat")).unwrap(),
        b"other world"
    );
    fs::remove_file(library.join("native/world.dat")).unwrap();
    restore_instance_archive(&fixture.paths, &archived.archive_id)
        .await
        .unwrap();
}

#[tokio::test]
async fn external_full_archive_recovers_before_missing_source_directory_is_created() {
    let (fixture, library) = exclusive_fixture(false).await;
    let before = crate::test_file_snapshot::tree_snapshot(&library).unwrap();
    let archived = fixture.archive().await.unwrap();
    fs::remove_dir_all(&library).unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    // Crash after the durable restore intent, before prepare_restore creates
    // the uninstalled program directory. All recovery bytes remain archived.
    store::set_state(&pool, &archived.archive_id, "restoring", None)
        .await
        .unwrap();
    pool.close().await;
    recover_instance_archives(&fixture.paths).await.unwrap();
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&library).unwrap(),
        before
    );
    assert!(fixture.instance_root.exists());
    assert!(!fixture.instance_root.join(external::PAYLOAD).exists());
}

#[tokio::test]
async fn external_retirement_recovers_after_compensation_discards_payload_before_move() {
    let (fixture, library) = exclusive_fixture(true).await;
    let before = crate::test_file_snapshot::tree_snapshot(&library).unwrap();
    let archived = fixture.archive().await.unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let mut archive = store::load(&pool, &archived.archive_id).await.unwrap();
    let mut snapshot = store::snapshot(&archive).unwrap();
    let root = PathBuf::from(archived.archived_instance_root.unwrap());
    // Recreate precisely the DB state left when retirement's transaction was
    // rolled back: the original rows still exist and the intent is archiving.
    let mut tx = pool.begin().await.unwrap();
    for table in [
        "instances",
        "instance_ports",
        "instance_runs",
        "instance_broadcast_policies",
        "instance_broadcast_events",
    ] {
        for row in &snapshot.tables[table] {
            store::insert_row(&mut tx, table, row).await.unwrap();
        }
    }
    tx.commit().await.unwrap();
    store::set_state(&pool, &archive.id, "archiving", None)
        .await
        .unwrap();
    archive.state = "archiving".into();
    let lock = inventory_lock(&fixture.paths).unwrap();
    external::prepare_restore(&pool, &archive, &mut snapshot, &lock)
        .await
        .unwrap();
    let plan = snapshot.external_program.as_ref().unwrap();
    external::restore_files(&root, plan).unwrap();
    external::finish_restore(&root, plan).unwrap();
    assert!(!root.join(external::PAYLOAD).exists());
    assert!(!fixture.instance_root.exists());
    drop(lock);
    pool.close().await;
    // Crash before compensation moves the instance home. Resuming the durable
    // retirement must be possible because all original bytes were restored.
    recover_instance_archives(&fixture.paths).await.unwrap();
    assert!(!library.join("native/world.dat").exists());
    assert!(
        archived_snapshot(&fixture, &archive.id)
            .await
            .external_program
            .unwrap()
            .restore_identity
            .is_none(),
        "completed retirement must release its old compensation staging identity"
    );
    restore_instance_archive(&fixture.paths, &archive.id)
        .await
        .unwrap();
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&library).unwrap(),
        before
    );
    assert!(fixture.instance_root.exists());
}

#[tokio::test]
async fn external_archive_recovers_after_data_unlink_and_database_failure() {
    let (fixture, library) = exclusive_fixture(true).await;
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_external_retirement BEFORE DELETE ON instances BEGIN SELECT RAISE(ABORT,'simulated retirement failure'); END").execute(&pool).await.unwrap();
    assert!(fixture.archive().await.is_err());
    // A reversible failure either restored the original instance or retained a
    // durable archiving journal that can safely complete after the failure clears.
    sqlx::query("DROP TRIGGER fail_external_retirement")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    recover_instance_archives(&fixture.paths).await.unwrap();
    if fixture.instance_root.exists() {
        fixture.archive().await.unwrap();
    }
    let archive = list_instance_archives(&fixture.paths)
        .await
        .unwrap()
        .archives
        .remove(0);
    restore_instance_archive(&fixture.paths, &archive.archive_id)
        .await
        .unwrap();
    assert_eq!(
        fs::read(library.join("native/world.dat")).unwrap(),
        b"world"
    );
}

#[tokio::test]
async fn external_payload_admission_recovers_empty_directory_before_identity_commit() {
    let (fixture, _) = exclusive_fixture(true).await;
    let archived = fixture.archive().await.unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let mut archive = store::load(&pool, &archived.archive_id).await.unwrap();
    let root = PathBuf::from(archived.archived_instance_root.unwrap());
    let mut snapshot = store::snapshot(&archive).unwrap();
    let plan = snapshot.external_program.as_mut().unwrap();
    let payload = root.join(external::PAYLOAD);
    files::purge_tree(&payload, plan.payload_identity.as_deref().unwrap()).unwrap();
    fs::create_dir(&payload).unwrap();
    plan.payload_identity = None;
    archive.state = "archiving".into();
    store::set_state(&pool, &archive.id, "archiving", None)
        .await
        .unwrap();
    let lock = inventory_lock(&fixture.paths).unwrap();
    external::prepare_payload(&pool, &archive, &mut snapshot, &root, &lock)
        .await
        .unwrap();
    assert!(
        snapshot
            .external_program
            .as_ref()
            .unwrap()
            .payload_identity
            .is_some()
    );
    drop(lock);
    pool.close().await;
}

#[tokio::test]
async fn external_restore_admission_recovers_only_empty_unregistered_staging() {
    let (fixture, library) = exclusive_fixture(true).await;
    let archived = fixture.archive().await.unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let archive = store::load(&pool, &archived.archive_id).await.unwrap();
    store::set_state(&pool, &archive.id, "restoring", None)
        .await
        .unwrap();
    let mut snapshot = store::snapshot(&archive).unwrap();
    let stage = library.join(format!(
        ".langame-archive-restore-{}",
        snapshot.external_program.as_ref().unwrap().restore_token
    ));
    fs::create_dir(&stage).unwrap();
    fs::write(stage.join("unknown"), b"preserve").unwrap();
    let lock = inventory_lock(&fixture.paths).unwrap();
    assert!(
        external::prepare_restore(&pool, &archive, &mut snapshot, &lock)
            .await
            .is_err()
    );
    assert_eq!(fs::read(stage.join("unknown")).unwrap(), b"preserve");
    fs::remove_file(stage.join("unknown")).unwrap();
    external::prepare_restore(&pool, &archive, &mut snapshot, &lock)
        .await
        .unwrap();
    assert!(
        snapshot
            .external_program
            .as_ref()
            .unwrap()
            .restore_identity
            .is_some()
    );
    drop(lock);
    pool.close().await;
}

#[tokio::test]
async fn external_full_archive_recovers_after_payload_cleanup_before_commit() {
    let (fixture, library) = exclusive_fixture(false).await;
    let archived = fixture.archive().await.unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let archive = store::load(&pool, &archived.archive_id).await.unwrap();
    store::set_state(&pool, &archive.id, "restoring", None)
        .await
        .unwrap();
    let mut snapshot = store::snapshot(&archive).unwrap();
    let root = PathBuf::from(archived.archived_instance_root.unwrap());
    fs::remove_dir_all(&library).unwrap();
    let lock = inventory_lock(&fixture.paths).unwrap();
    external::prepare_restore(&pool, &archive, &mut snapshot, &lock)
        .await
        .unwrap();
    let plan = snapshot.external_program.as_ref().unwrap();
    external::restore_files(&root, plan).unwrap();
    external::finish_restore(&root, plan).unwrap();
    drop(lock);
    pool.close().await;
    // Reopen the journal, just as a new process does after the final DB commit
    // failed. The original program directory identity no longer exists.
    recover_instance_archives(&fixture.paths).await.unwrap();
    assert_eq!(
        fs::read(library.join("native/world.dat")).unwrap(),
        b"world"
    );
    assert!(fixture.instance_root.exists());
    assert!(!fixture.instance_root.join(external::PAYLOAD).exists());
}

#[tokio::test]
async fn damaged_full_archive_cannot_release_its_only_program_source() {
    let (fixture, library) = exclusive_fixture(false).await;
    let archived = fixture.archive().await.unwrap();
    let snapshot = archived_snapshot(&fixture, &archived.archive_id).await;
    let plan = snapshot.external_program.unwrap();
    let index = plan
        .files
        .keys()
        .position(|path| path == "server.exe")
        .unwrap();
    let payload = PathBuf::from(archived.archived_instance_root.unwrap())
        .join(external::PAYLOAD)
        .join(format!("{index:06}"));
    assert!(
        external::ensure_program_archive_dependencies(&fixture.paths, &library)
            .await
            .is_ok()
    );
    fs::write(&payload, b"damaged archive").unwrap();
    assert!(
        external::ensure_program_archive_dependencies(&fixture.paths, &library)
            .await
            .is_err()
    );
    assert_eq!(
        fs::read(library.join("server.exe")).unwrap(),
        b"official bytes"
    );
}

#[tokio::test]
async fn independent_archive_retains_program_when_the_library_is_running() {
    let mut fixture = Fixture::new().await;
    fixture.paths.modules_root = fixture.root.join("modules");
    fs::create_dir_all(fixture.paths.modules_root.join("archive-fixture")).unwrap();
    fs::write(
        fixture
            .paths
            .modules_root
            .join("archive-fixture/module.toml"),
        "id='archive-fixture'\nname='Archive fixture'\nversion='1'\n",
    )
    .unwrap();
    let descriptor = app_modules::discover_modules(&fixture.paths.modules_root)
        .unwrap()
        .remove(0);
    let library = fixture.paths.games_root.join("running-library");
    fs::create_dir(&library).unwrap();
    fs::write(library.join("server.exe"), b"official bytes").unwrap();
    crate::record_library_program_baseline(&library, &descriptor, true, None).unwrap();
    fs::write(
        fixture.instance_root.join("runtime/server.exe"),
        b"official bytes",
    )
    .unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("INSERT INTO game_installs(id,module_id,install_root,install_state,scope) VALUES(18,'archive-fixture',?1,'installed','library')")
        .bind(library.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
    let active_root = fixture.paths.instances_root.join("active-user");
    sqlx::query("INSERT INTO instances(id,name,module_id,install_id,status,data_path,config_path,logs_path,saves_path) VALUES('active-user','Running owner','archive-fixture',18,'running',?1,?2,?3,?4)")
        .bind(active_root.join("data").to_string_lossy().as_ref()).bind(active_root.join("config").to_string_lossy().as_ref())
        .bind(active_root.join("logs").to_string_lossy().as_ref()).bind(active_root.join("saves").to_string_lossy().as_ref()).execute(&pool).await.unwrap();
    pool.close().await;
    let archived = fixture.archive().await.unwrap();
    assert!(
        archived_snapshot(&fixture, &archived.archive_id)
            .await
            .program
            .is_none()
    );
    assert_eq!(
        fs::read(
            PathBuf::from(archived.archived_instance_root.unwrap()).join("runtime/server.exe")
        )
        .unwrap(),
        b"official bytes"
    );
}

#[cfg(windows)]
#[test]
fn native_archive_publish_supports_long_windows_destinations() {
    let root = std::env::temp_dir().join(format!("archive-long-{}", uuid::Uuid::new_v4()));
    let parent = root
        .join("destination-".repeat(7))
        .join("deep-directory-".repeat(7))
        .join("payload-".repeat(7));
    fs::create_dir_all(&parent).unwrap();
    let source = root.join("source");
    let destination = parent.join("published");
    assert!(destination.as_os_str().len() > 260);
    fs::write(&source, b"long path payload").unwrap();
    files::native::open_verified_file(&source, true)
        .unwrap()
        .rename(&destination)
        .unwrap();
    assert_eq!(fs::read(destination).unwrap(), b"long path payload");
    fs::remove_dir_all(root).unwrap();
}
