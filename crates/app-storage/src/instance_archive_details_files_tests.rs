use super::*;
use app_core::InstanceBackupKind;

fn backup_manifest(root: &Path, id: &str, owner: &str) {
    let directory = root.join("backups").join(id);
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("backup.json"),
        serde_json::to_vec(&serde_json::json!({
            "backup_id": id,
            "instance_id": owner,
            "display_name": "Saved backup",
            "created_at_unix_ms": 987654,
            "backup_path": "D:/historical-backup-path",
            "saves_path": "D:/historical-save-path",
            "file_count": 4,
            "total_bytes": 321
        }))
        .unwrap(),
    )
    .unwrap();
}

#[tokio::test]
async fn archive_details_preview_reads_policies_history_log_and_backups_without_writes() {
    let fixture = Fixture::new().await;
    let mut log = vec![b'x'; 70 * 1024];
    log.extend_from_slice(b"\nretained last log line\n");
    fs::write(fixture.instance_root.join("logs/last.log"), &log).unwrap();
    backup_manifest(
        &fixture.instance_root,
        "auto-stop-original",
        "archive-instance",
    );
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instances SET auto_backup_on_stop=1,backup_retention_count=7,crash_restart_limit=3 WHERE id='archive-instance'")
        .execute(&pool).await.unwrap();
    for id in 30..=129 {
        sqlx::query("INSERT INTO instance_runs (id,instance_id,status,started_at,stopped_at,exit_code,crash_flag,display_name) VALUES (?1,'archive-instance','error','2026-09-28 13:01:02','2026-09-28 13:01:03',7,1,'Saved process')")
            .bind(id).execute(&pool).await.unwrap();
    }
    sqlx::query("UPDATE instance_runs SET log_path=?1 WHERE id=129")
        .bind(
            fixture
                .instance_root
                .join("logs/last.log")
                .to_string_lossy()
                .as_ref(),
        )
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let archived = fixture.archive().await.unwrap();
    let root = Path::new(archived.archived_instance_root.as_ref().unwrap());
    let before = crate::test_file_snapshot::tree_snapshot(root).unwrap();
    let journal_before = journal(&fixture.paths, &archived.archive_id).await;
    let details = read_instance_archive_details(&fixture.paths, &archived.archive_id)
        .await
        .unwrap();
    assert!(details.maintenance.autostart);
    assert!(details.maintenance.auto_backup_on_stop);
    assert_eq!(details.maintenance.backup_retention_count, 7);
    assert_eq!(details.maintenance.crash_restart_limit, 3);
    assert_eq!(details.maintenance.runtime_mode, "independent");
    assert_eq!(details.runs.total, 101);
    assert!(details.runs.truncated);
    assert_eq!(details.runs.entries.len(), 100);
    assert_eq!(details.runs.entries[0].id, 129);
    assert_eq!(details.runs.entries[99].id, 30);
    assert_eq!(
        details.runs.entries[0].started_at.as_deref(),
        Some("2026-09-28 13:01:02")
    );
    assert_eq!(
        details.runs.entries[0].stopped_at.as_deref(),
        Some("2026-09-28 13:01:03")
    );
    assert_eq!(
        details.runs.entries[0].display_name.as_deref(),
        Some("Saved process")
    );
    assert_eq!(details.runs.entries[0].exit_code, Some(7));
    assert!(details.runs.entries[0].crash_flag);
    assert_eq!(details.log.relative_path.as_deref(), Some("logs/last.log"));
    assert_eq!(details.log.text.as_bytes(), &log[log.len() - 64 * 1024..]);
    assert!(details.log.truncated);
    assert!(details.log.issues.is_empty());
    assert_eq!(details.backups.entries.len(), 1);
    assert!(details.backups.issues.is_empty());
    let backup = &details.backups.entries[0];
    assert_eq!(backup.backup_id, "auto-stop-original");
    assert_eq!(backup.backup_kind, InstanceBackupKind::AutoStop);
    assert_eq!(backup.file_count, 4);
    assert_eq!(backup.total_bytes, 321);
    assert_eq!(
        Path::new(&backup.backup_path),
        root.join("backups/auto-stop-original")
    );
    assert_eq!(
        journal(&fixture.paths, &archived.archive_id).await,
        journal_before
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(root).unwrap(),
        before
    );
    assert!(!fixture.instance_root.exists());
}

#[tokio::test]
async fn archive_details_preview_keeps_configuration_when_log_or_backup_is_invalid() {
    let fixture = Fixture::new().await;
    let outside = fixture.root.join("outside.log");
    fs::write(&outside, "outside bytes must never be read").unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instance_runs SET log_path=?1 WHERE id=29")
        .bind(outside.to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    backup_manifest(&fixture.instance_root, "good", "archive-instance");
    backup_manifest(&fixture.instance_root, "foreign", "another-instance");
    fs::create_dir_all(fixture.instance_root.join("backups/oversized")).unwrap();
    fs::write(
        fixture.instance_root.join("backups/oversized/backup.json"),
        vec![b' '; 256 * 1024 + 1],
    )
    .unwrap();
    let archived = fixture.archive().await.unwrap();
    let root = Path::new(archived.archived_instance_root.as_ref().unwrap());
    let before = crate::test_file_snapshot::tree_snapshot(root).unwrap();
    let details = read_instance_archive_details(&fixture.paths, &archived.archive_id)
        .await
        .unwrap();
    assert!(details.instance.settings_json.contains("retained world"));
    assert!(details.log.text.is_empty());
    assert!(details.log.relative_path.is_none());
    assert!(details.log.issues[0].contains("outside"));
    assert_eq!(details.backups.entries.len(), 1);
    assert_eq!(details.backups.entries[0].backup_id, "good");
    assert_eq!(details.backups.issues.len(), 2);
    assert!(
        details
            .backups
            .issues
            .iter()
            .any(|issue| issue.contains("256 KiB"))
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(root).unwrap(),
        before
    );
    for source in [
        fixture.instance_root.join("logs/missing.log"),
        fixture.instance_root.join("logs/../outside.log"),
    ] {
        replace_snapshot(&fixture, &archived.archive_id, |snapshot| {
            snapshot.tables.get_mut("instance_runs").unwrap()[0].insert(
                "log_path".into(),
                serde_json::Value::String(source.to_string_lossy().into_owned()),
            );
        })
        .await;
        let details = read_instance_archive_details(&fixture.paths, &archived.archive_id)
            .await
            .unwrap();
        assert!(details.instance.settings_json.contains("retained world"));
        assert!(details.log.text.is_empty());
        assert_eq!(details.log.issues.len(), 1);
    }
    assert_eq!(
        fs::read_to_string(&outside).unwrap(),
        "outside bytes must never be read"
    );
}

#[tokio::test]
async fn archive_details_preview_bounds_backup_listing_and_reports_truncation() {
    let fixture = Fixture::new().await;
    for index in 0..257 {
        backup_manifest(
            &fixture.instance_root,
            &format!("backup-{index:03}"),
            "archive-instance",
        );
    }
    let archived = fixture.archive().await.unwrap();
    let details = read_instance_archive_details(&fixture.paths, &archived.archive_id)
        .await
        .unwrap();
    assert_eq!(details.backups.entries.len(), 256);
    assert!(details.backups.truncated);
    assert_eq!(details.backups.issues.len(), 1);
    assert!(details.backups.issues[0].contains("256 directory entries"));
    assert!(details.log.relative_path.is_none());
    assert!(details.log.issues.is_empty());
    assert_eq!(details.runs.total, 1);
    assert!(!details.runs.truncated);
    assert!(details.runs.entries[0].started_at.is_none());
    assert!(details.runs.entries[0].stopped_at.is_none());
}

#[tokio::test]
async fn archive_details_preview_bounds_combined_backup_metadata_bytes() {
    let fixture = Fixture::new().await;
    for index in 0..16 {
        let id = format!("backup-{index:03}");
        backup_manifest(&fixture.instance_root, &id, "archive-instance");
        let path = fixture
            .instance_root
            .join("backups")
            .join(id)
            .join("backup.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["display_name"] = serde_json::Value::String("x".repeat(200 * 1024));
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    }
    let archived = fixture.archive().await.unwrap();
    let details = read_instance_archive_details(&fixture.paths, &archived.archive_id)
        .await
        .unwrap();
    assert_eq!(details.backups.entries.len(), 10);
    assert!(details.backups.truncated);
    assert_eq!(details.backups.issues.len(), 1);
    assert!(details.backups.issues[0].contains("combined 2 MiB"));
    assert!(serde_json::to_vec(&details.backups).unwrap().len() < 2 * 1024 * 1024);
    assert!(details.instance.settings_json.contains("retained world"));
}

#[cfg(windows)]
#[tokio::test]
async fn archive_details_preview_refuses_log_and_backup_junctions_without_blocking_configuration() {
    let fixture = Fixture::new().await;
    fs::write(fixture.instance_root.join("logs/last.log"), "retained log").unwrap();
    backup_manifest(&fixture.instance_root, "good", "archive-instance");
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instance_runs SET log_path=?1 WHERE id=29")
        .bind(
            fixture
                .instance_root
                .join("logs/last.log")
                .to_string_lossy()
                .as_ref(),
        )
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let archived = fixture.archive().await.unwrap();
    let root = Path::new(archived.archived_instance_root.as_ref().unwrap());
    for name in ["logs", "backups"] {
        let inside = root.join(name);
        let outside = fixture.root.join(format!("outside-{name}"));
        fs::rename(&inside, &outside).unwrap();
        let created = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(inside.to_string_lossy().replace('/', "\\"))
            .arg(outside.to_string_lossy().replace('/', "\\"))
            .output()
            .unwrap();
        assert!(
            created.status.success(),
            "{}",
            String::from_utf8_lossy(&created.stderr)
        );
        let details = read_instance_archive_details(&fixture.paths, &archived.archive_id).await;
        // Unlink the exact fixture-owned junction before any recursive cleanup.
        fs::remove_dir(&inside).unwrap();
        fs::rename(&outside, &inside).unwrap();
        let details = details.unwrap();
        assert!(details.instance.settings_json.contains("retained world"));
        if name == "logs" {
            assert!(details.log.text.is_empty());
            assert_eq!(details.log.issues.len(), 1);
            assert_eq!(details.backups.entries.len(), 1);
        } else {
            assert!(details.backups.entries.is_empty());
            assert_eq!(details.backups.issues.len(), 1);
            assert_eq!(details.log.text, "retained log");
        }
    }
}
