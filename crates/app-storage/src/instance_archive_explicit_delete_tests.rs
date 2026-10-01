use super::*;

async fn pending_delete_waits_for_explicit_retry(moved: bool, committed: bool) {
    let fixture = Fixture::new().await;
    let before = fixture.snapshot().await;
    let id = stage_archiving(&fixture, moved).await;
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let (instances, trash) = files::parent_identities(&fixture.paths).unwrap();
    sqlx::query("UPDATE instance_archives SET purpose='delete',instances_root_identity_json=?2,archive_parent_identity_json=?3 WHERE archive_id=?1")
        .bind(&id).bind(instances).bind(trash).execute(&pool).await.unwrap();
    if committed {
        let _inventory = inventory_lock(&fixture.paths).unwrap();
        let lock =
            acquire_instance_settings_mutation_lock(&fixture.paths, "archive-instance").unwrap();
        let archive = store::load(&pool, &id).await.unwrap();
        transactions::finish_archiving(&pool, &fixture.paths, &archive, &lock)
            .await
            .unwrap();
    }
    let archive_root = files::archive_path(&fixture.paths, &id).unwrap();
    let retained_root = if moved || committed {
        &archive_root
    } else {
        &fixture.instance_root
    };
    let bytes = crate::test_file_snapshot::tree_snapshot(retained_root).unwrap();
    let operation = store::load(&pool, &id).await.unwrap();
    let state = if committed { "purging" } else { "archiving" };
    assert_eq!(operation.state, state);
    pool.close().await;

    assert!(
        pending_instance_archive_ids(&fixture.paths)
            .await
            .unwrap()
            .is_empty()
    );
    crate::initialize_database(&fixture.paths).await.unwrap();
    recover_instance_archives(&fixture.paths).await.unwrap();
    let inventory = list_instance_archives(&fixture.paths).await.unwrap();
    assert!(inventory.archives.is_empty());
    assert_eq!(inventory.pending_deletions.len(), 1);
    assert_eq!(inventory.pending_deletions[0].operation_id, id);
    assert!(inventory.pending_deletions[0].can_retry);
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(retained_root).unwrap(),
        bytes
    );
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let untouched = store::load(&pool, &id).await.unwrap();
    assert_eq!(untouched.state, operation.state);
    assert_eq!(untouched.purpose, operation.purpose);
    assert_eq!(untouched.snapshot, operation.snapshot);
    assert_eq!(untouched.snapshot_hash, operation.snapshot_hash);
    assert_eq!(untouched.problem, operation.problem);
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM instances WHERE id='archive-instance'")
            .fetch_one(&pool)
            .await
            .unwrap();
    if committed {
        assert_eq!(count, 0);
    } else {
        assert_eq!(count, 1);
        let mut connection = pool.acquire().await.unwrap();
        let after = store::capture(
            &mut connection,
            "archive-instance",
            before.config_sha256.clone(),
        )
        .await
        .unwrap();
        assert_eq!(after, before);
    }
    pool.close().await;

    crate::delete_instance(&fixture.paths, "archive-instance")
        .await
        .unwrap();
    assert!(!fixture.instance_root.exists());
    assert!(!archive_root.exists());
    assert!(
        list_instance_archives(&fixture.paths)
            .await
            .unwrap()
            .pending_deletions
            .is_empty()
    );
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let completed = store::load(&pool, &id).await.unwrap();
    assert_eq!(completed.state, "purged");
    assert!(completed.snapshot.is_none());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM instances WHERE id='archive-instance'")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    pool.close().await;
}

#[tokio::test]
async fn admitted_delete_waits_for_explicit_retry_without_moving_files() {
    pending_delete_waits_for_explicit_retry(false, false).await;
}

#[tokio::test]
async fn moved_delete_waits_for_explicit_retry_without_removing_database_rows() {
    pending_delete_waits_for_explicit_retry(true, false).await;
}

#[tokio::test]
async fn committed_delete_waits_for_explicit_retry_without_purging_files() {
    pending_delete_waits_for_explicit_retry(true, true).await;
}

#[cfg(windows)]
#[tokio::test]
async fn committed_delete_preflights_links_before_removal_and_preserves_retry() {
    let fixture = Fixture::new().await;
    let id = stage_archiving(&fixture, true).await;
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let (instances, trash) = files::parent_identities(&fixture.paths).unwrap();
    sqlx::query("UPDATE instance_archives SET purpose='delete',instances_root_identity_json=?2,archive_parent_identity_json=?3 WHERE archive_id=?1")
        .bind(&id).bind(instances).bind(trash).execute(&pool).await.unwrap();
    {
        let _inventory = inventory_lock(&fixture.paths).unwrap();
        let lock =
            acquire_instance_settings_mutation_lock(&fixture.paths, "archive-instance").unwrap();
        let archive = store::load(&pool, &id).await.unwrap();
        transactions::finish_archiving(&pool, &fixture.paths, &archive, &lock)
            .await
            .unwrap();
    }
    let committed = store::load(&pool, &id).await.unwrap();
    assert_eq!(committed.state, "purging");
    pool.close().await;
    let archive_root = files::archive_path(&fixture.paths, &id).unwrap();
    let owned_before = crate::test_file_snapshot::tree_snapshot(&archive_root).unwrap();
    let external = fixture.root.join("external-world");
    fs::create_dir(&external).unwrap();
    fs::write(external.join("sentinel.dat"), b"external world survives").unwrap();
    let external_before = crate::test_file_snapshot::tree_snapshot(&external).unwrap();
    let link = archive_root.join("external-link");
    let created = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(link.to_string_lossy().replace('/', "\\"))
        .arg(external.to_string_lossy().replace('/', "\\"))
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );

    let result = crate::delete_instance(&fixture.paths, "archive-instance").await;
    // Unlink the test junction before assertions or fixture teardown can fail.
    fs::remove_dir(&link).unwrap();
    assert!(result.is_err());
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&archive_root).unwrap(),
        owned_before,
        "the final preflight must finish before removing any owned files"
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&external).unwrap(),
        external_before
    );
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let pending = store::load(&pool, &id).await.unwrap();
    assert_eq!(pending.state, "purging");
    assert_eq!(pending.snapshot, committed.snapshot);
    assert_eq!(pending.snapshot_hash, committed.snapshot_hash);
    assert_eq!(pending.identity, committed.identity);
    assert!(
        pending.problem.is_some(),
        "final preflight failure is recorded"
    );
    pool.close().await;
    let inventory = list_instance_archives(&fixture.paths).await.unwrap();
    assert_eq!(inventory.pending_deletions.len(), 1);
    assert_eq!(inventory.pending_deletions[0].operation_id, id);
    assert!(inventory.pending_deletions[0].can_retry);

    crate::delete_instance(&fixture.paths, "archive-instance")
        .await
        .unwrap();
    assert!(!archive_root.exists());
    assert!(!fixture.instance_root.exists());
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&external).unwrap(),
        external_before
    );
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let completed = store::load(&pool, &id).await.unwrap();
    assert_eq!(completed.state, "purged");
    assert!(completed.snapshot.is_none());
    assert!(completed.problem.is_none());
    pool.close().await;
}
