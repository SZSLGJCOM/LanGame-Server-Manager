use super::*;
use std::fs::OpenOptions;

#[tokio::test]
async fn archived_program_readers_share_inventory_without_mutating_archives() {
    let fixture = Fixture::new().await;
    let before = tree_snapshot(&fixture.archive).unwrap();
    let metadata = fixture.snapshot_json().await;
    let lock_path = fixture
        .paths
        .instances_root
        .join(".langame/locks/instance-settings/archive-inventory.lock");
    let reader = OpenOptions::new()
        .read(true)
        .write(true)
        .open(lock_path)
        .unwrap();
    reader.try_lock_shared().unwrap();

    // Hold a real read lease while the eight module requests used by the
    // desktop enumerate the same archive inventory concurrently.
    let mut requests = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let paths = fixture.paths.clone();
        requests.spawn(async move { read_archived_program_sources(&paths).await });
    }
    let mut results = Vec::new();
    while let Some(result) = requests.join_next().await {
        results.push(result.unwrap());
    }
    assert!(matches!(
        inventory_lock(&fixture.paths),
        Err(StorageError::InstanceSettingsLocked { .. })
    ));
    drop(reader);

    assert_eq!(results.len(), 8);
    for result in results {
        let sources = result.expect("independent archive readers must share the read lease");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].archive_id, fixture.archive_id);
        assert_eq!(sources[0].install_root, fixture.archive.join("runtime"));
    }
    let writer = inventory_lock(&fixture.paths).unwrap();
    assert_eq!(tree_snapshot(&fixture.archive).unwrap(), before);
    assert_eq!(fixture.snapshot_json().await, metadata);
    drop(writer);
}

#[tokio::test]
async fn archived_program_reader_does_not_bypass_exclusive_inventory_writer() {
    let fixture = Fixture::new().await;
    let writer = inventory_lock(&fixture.paths).unwrap();
    assert!(matches!(
        read_archived_program_sources(&fixture.paths).await,
        Err(StorageError::InstanceSettingsLocked { .. })
    ));
    drop(writer);
    let sources = read_archived_program_sources(&fixture.paths).await.unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].archive_id, fixture.archive_id);
}
