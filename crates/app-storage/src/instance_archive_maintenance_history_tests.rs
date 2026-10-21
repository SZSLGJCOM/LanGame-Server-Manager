use super::*;

#[tokio::test]
async fn archive_preserves_daily_policies_and_paired_player_history_without_rearming_tasks() {
    let fixture = Fixture::new().await;
    let pool = connect_pool(&fixture.paths).await.unwrap();
    for statement in [
        "INSERT INTO instance_maintenance_schedules(instance_id,action,enabled,time_local) VALUES('archive-instance','restart',1,'04:00')",
        "INSERT INTO player_history_cursors(run_id,byte_offset,generation,source_identity) VALUES(29,999,2,'original-log-identity')",
        "INSERT INTO player_history_events(id,instance_id,run_id,process_key,generation,byte_offset,kind,player_name,log_time,elapsed_ms) VALUES(100,'archive-instance',29,'master',2,10,'join','Player','00:01',1000)",
        "INSERT INTO player_history_events(id,instance_id,run_id,process_key,generation,byte_offset,kind,player_name,log_time,elapsed_ms,paired_event_id,duration_ms) VALUES(101,'archive-instance',29,'master',2,20,'leave','Player','00:02',2000,100,1000)",
        "UPDATE player_history_events SET paired_event_id=101 WHERE id=100",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }
    pool.close().await;
    let before = fixture.snapshot().await;
    fixture.archive().await.unwrap();
    let archives = list_instance_archives(&fixture.paths)
        .await
        .unwrap()
        .archives;
    assert!(archives[0].can_restore, "{:?}", archives[0].issues);
    restore_instance_archive(&fixture.paths, &archives[0].archive_id)
        .await
        .unwrap();
    let after = fixture.snapshot().await;
    assert_eq!(
        after.tables["player_history_events"],
        before.tables["player_history_events"]
    );
    assert_eq!(
        after.tables["player_history_cursors"],
        before.tables["player_history_cursors"]
    );
    assert_eq!(
        after.tables["player_history_cursors"][0]["source_identity"],
        "original-log-identity"
    );
    assert_eq!(
        after.tables["instance_maintenance_schedules"][0]["time_local"],
        "04:00"
    );
    assert_eq!(
        after.tables["instance_maintenance_schedules"][0]["enabled"],
        0
    );
}

#[tokio::test]
async fn archive_metadata_from_before_maintenance_and_history_tables_remains_readable() {
    let fixture = Fixture::new().await;
    fixture.archive().await.unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let id = store::archive_ids(&pool).await.unwrap().remove(0);
    let mut archive = store::load(&pool, &id).await.unwrap();
    let mut snapshot = store::snapshot(&archive).unwrap();
    for name in [
        "instance_maintenance_schedules",
        "player_history_cursors",
        "player_history_events",
    ] {
        snapshot.tables.remove(name);
    }
    let text = serde_json::to_string(&snapshot).unwrap();
    archive.snapshot_hash = Some(store::digest(text.as_bytes()));
    archive.snapshot = Some(text);
    assert!(store::snapshot(&archive).is_ok());
    pool.close().await;
}
