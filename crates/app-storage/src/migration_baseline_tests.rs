use super::*;

#[tokio::test]
async fn program_removal_migration_preserves_existing_v4_database() {
    let fixture = TestDatabase::new("program-removal-upgrade");
    let previous = fixture.root.join("previous-migrations");
    fs::create_dir_all(&previous).unwrap();
    for migration in MIGRATOR.iter().filter(|migration| migration.version <= 4) {
        let name = format!(
            "{:04}_{}.sql",
            migration.version,
            migration.description.replace(' ', "_")
        );
        fs::write(previous.join(name), migration.sql.as_str().as_bytes()).unwrap();
    }
    let pool = fixture.open().await;
    sqlx::migrate::Migrator::new(previous.as_path())
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
    assert_eq!(pragma_user_version(&pool).await, 4);
    insert_relational_sentinels(&pool).await;
    let counts = sentinel_counts(&pool).await;
    let history = history_snapshot(&pool).await;
    pool.close().await;

    initialize_database(&fixture.paths).await.unwrap();
    let pool = fixture.open().await;
    assert_eq!(pragma_user_version(&pool).await, 5);
    assert_eq!(sentinel_counts(&pool).await, counts);
    assert_eq!(&history_snapshot(&pool).await[..history.len()], history);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM program_removals")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    assert_foreign_keys_are_consistent(&pool).await;
    pool.close().await;
}

#[tokio::test]
async fn baseline_initialization_preserves_every_related_row_on_reopen() {
    let fixture = TestDatabase::new("baseline-rows");
    let pool = fixture.open().await;
    MIGRATOR.run(&pool).await.unwrap();
    insert_relational_sentinels(&pool).await;
    let counts_before = sentinel_counts(&pool).await;
    pool.close().await;
    initialize_database(&fixture.paths).await.unwrap();
    let pool = fixture.open().await;
    assert_eq!(sentinel_counts(&pool).await, counts_before);
    assert_foreign_keys_are_consistent(&pool).await;
    pool.close().await;
}

#[tokio::test]
async fn baseline_deletion_cascades_to_all_instance_owned_rows() {
    let fixture = TestDatabase::new("baseline-cascade");
    let pool = fixture.open().await;
    MIGRATOR.run(&pool).await.unwrap();
    insert_relational_sentinels(&pool).await;
    sqlx::query("DELETE FROM instances WHERE id = 'instance-sentinel'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        sentinel_counts(&pool)
            .await
            .iter()
            .all(|(_, count)| *count == 0)
    );
    assert_foreign_keys_are_consistent(&pool).await;
    pool.close().await;
}

#[tokio::test]
async fn baseline_allows_multiple_instances_of_one_module() {
    let fixture = TestDatabase::new("baseline-multiplicity");
    let pool = fixture.open().await;
    MIGRATOR.run(&pool).await.unwrap();
    insert_relational_sentinels(&pool).await;
    sqlx::query(
        "INSERT INTO instances (id, name, module_id, data_path, config_path, logs_path, saves_path)          SELECT 'second-instance', name, module_id, 'second-data', 'second-config', 'second-logs', 'second-saves'          FROM instances WHERE id = 'instance-sentinel'"
    ).execute(&pool).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM instances")
            .fetch_one(&pool)
            .await
            .unwrap(),
        2
    );
    assert_foreign_keys_are_consistent(&pool).await;
    pool.close().await;
}

#[tokio::test]
async fn baseline_contains_only_active_domain_tables() {
    let fixture = TestDatabase::new("baseline-tables");
    let pool = fixture.open().await;
    MIGRATOR.run(&pool).await.unwrap();
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name"
    ).fetch_all(&pool).await.unwrap();
    assert_eq!(
        tables,
        [
            "_sqlx_migrations",
            "app_settings",
            "game_installs",
            "instance_archives",
            "instance_broadcast_events",
            "instance_broadcast_policies",
            "instance_ports",
            "instance_runs",
            "instances",
            "modules",
            "program_removals"
        ]
    );
    pool.close().await;
}

async fn insert_relational_sentinels(pool: &SqlitePool) {
    sqlx::query(
        "INSERT INTO modules (id, name, version, supported_platforms) \
         VALUES ('migration-child-test', 'Migration Child Test', '1', 'windows')",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO instances \
         (id, name, module_id, data_path, config_path, logs_path, saves_path) \
         VALUES ('instance-sentinel', 'Instance Sentinel', 'migration-child-test', \
         'data', 'config', 'logs', 'saves')",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO instance_ports (instance_id, name, port, protocol) \
         VALUES ('instance-sentinel', 'game', 27777, 'udp')",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO instance_runs (instance_id, status) \
         VALUES ('instance-sentinel', 'stopped')",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO instance_broadcast_policies \
         (instance_id, enabled, rules_json, updated_at_unix_ms) \
         VALUES ('instance-sentinel', 1, '{}', 1)",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO instance_broadcast_events \
         (event_id, instance_id, module_id, source, message, status, created_at_unix_ms) \
         VALUES ('event-sentinel', 'instance-sentinel', 'migration-child-test', \
         'test', 'must survive', 'sent', 1)",
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn sentinel_counts(pool: &SqlitePool) -> Vec<(String, i64)> {
    let mut counts = Vec::new();
    for (table, query) in [
        ("instances", "SELECT COUNT(*) FROM instances"),
        ("instance_ports", "SELECT COUNT(*) FROM instance_ports"),
        ("instance_runs", "SELECT COUNT(*) FROM instance_runs"),
        (
            "instance_broadcast_policies",
            "SELECT COUNT(*) FROM instance_broadcast_policies",
        ),
        (
            "instance_broadcast_events",
            "SELECT COUNT(*) FROM instance_broadcast_events",
        ),
    ] {
        let count: i64 = sqlx::query_scalar(query).fetch_one(pool).await.unwrap();
        counts.push((table.to_string(), count));
    }
    counts
}

async fn assert_foreign_keys_are_consistent(pool: &SqlitePool) {
    let violations = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(pool)
        .await
        .unwrap();
    assert!(violations.is_empty());
    let enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(enabled, 1);
}
