use super::*;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

#[tokio::test]
async fn usage_tracks_real_private_program_native_save_and_backup_ownership() {
    let root = unique_test_root();
    let mut paths = test_paths(&root);
    paths.modules_root = repo_root().join("modules");
    let descriptor = app_modules::discover_modules(&paths.modules_root)
        .unwrap()
        .into_iter()
        .find(|module| module.summary.id == "palworld")
        .unwrap();
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    replenish_test_library(&paths, &descriptor).await;
    let instance = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: "Usage ownership".into(),
            module_id: descriptor.summary.id.clone(),
        },
    )
    .await
    .unwrap();
    let details = read_instance_details(&paths, &instance.summary.id)
        .await
        .unwrap();
    let instance_root = Path::new(&details.config_file_path)
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let runtime = crate::resolve_instance_runtime_root(instance_root).unwrap();
    let saves = Path::new(&details.saves_path);
    fs::create_dir_all(saves).unwrap();
    fs::create_dir_all(instance_root.join("backups")).unwrap();
    let before =
        crate::scan_storage_usage(&paths, "before".into(), Arc::new(AtomicBool::new(false)))
            .await
            .unwrap();
    assert_eq!(before.status, "complete", "{:?}", before.entries);
    fs::write(runtime.join("program.fixture"), [1; 101]).unwrap();
    fs::write(saves.join("world.fixture"), [2; 103]).unwrap();
    fs::write(instance_root.join("backups/world.fixture"), [3; 107]).unwrap();
    let after = crate::scan_storage_usage(&paths, "after".into(), Arc::new(AtomicBool::new(false)))
        .await
        .unwrap();
    assert_eq!(after.status, "complete", "{:?}", after.entries);
    for (category, delta) in [
        ("instance_program", 101),
        ("instance_data", 103),
        ("backups", 107),
    ] {
        let value = |report: &crate::StorageUsageReport| {
            report
                .entries
                .iter()
                .filter(|entry| {
                    entry.instance_id.as_deref() == Some(instance.summary.id.as_str())
                        && entry.category == category
                })
                .map(|entry| entry.logical_bytes)
                .sum::<u64>()
        };
        assert_eq!(value(&after) - value(&before), delta, "{category}");
    }
    cleanup_root(&root);
}
