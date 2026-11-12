use super::*;
use crate::storage_migration as migration;
use std::sync::{Arc, atomic::AtomicBool};

async fn migrate_and_restore(interrupt_publication: bool) {
    let fixture = Fixture::new();
    let paths = fixture_paths(&fixture.root);
    crate::bootstrap_storage_with_paths(paths.clone()).unwrap();
    let original = fixture.inventory();
    let backup = transaction::snapshot(&fixture.plan, "manual").unwrap();
    let manifest_path = Path::new(&backup.summary.backup_path).join("manifest.json");
    let mut before: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    before["note"] = serde_json::json!("preserved snapshot annotation");
    transaction::write_json(&manifest_path, &before).unwrap();

    let mut next = paths.settings();
    next.servers_root = fixture
        .root
        .join("moved-instances")
        .to_string_lossy()
        .into_owned();
    next.archives_root = Path::new(&next.servers_root)
        .join(".trash")
        .to_string_lossy()
        .into_owned();
    let prepared = migration::preview(
        &paths,
        next.clone(),
        uuid::Uuid::new_v4().to_string(),
        Vec::new(),
        Arc::new(AtomicBool::new(false)),
        Arc::new(|_| {}),
    )
    .await
    .unwrap();
    assert_eq!(
        prepared.status().phase,
        app_core::StorageMigrationPhase::Ready
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&manifest_path).unwrap()).unwrap(),
        before
    );
    assert!(!Path::new(&next.servers_root).exists());

    if interrupt_publication {
        crate::atomic_file::fail_next_atomic_write_for_test(&paths.settings_path);
    }
    let result = migration::execute(
        &paths,
        prepared,
        Arc::new(AtomicBool::new(false)),
        Arc::new(|_| {}),
    )
    .await;
    if interrupt_publication {
        assert!(result.is_err());
        assert_eq!(
            migration::read_status(&paths).unwrap().unwrap().phase,
            app_core::StorageMigrationPhase::Applying
        );
        assert_eq!(
            fixture.inventory(),
            original,
            "publication failure must retain every source"
        );
    } else {
        assert_eq!(
            result.unwrap().phase,
            app_core::StorageMigrationPhase::Completed
        );
    }
    let reboot = crate::bootstrap_storage_with_paths(paths.clone()).unwrap();
    assert_eq!(reboot.settings.servers_root, next.servers_root);
    let status = migration::read_status(&paths).unwrap().unwrap();
    assert_eq!(status.phase, app_core::StorageMigrationPhase::Completed);
    assert!(
        status.cleanup_warnings.is_empty(),
        "{:?}",
        status.cleanup_warnings
    );
    assert!(!paths.instances_root.exists());

    let after: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    let mut unchanged = after.clone();
    for (index, member) in fixture.plan.report.members.iter().enumerate() {
        let instance = Path::new(&next.servers_root).join(&member.summary.id);
        for (offset, relative) in ["config", "runtime/ShooterGame/Saved"].iter().enumerate() {
            let scope = index * 2 + offset;
            assert_eq!(
                Path::new(after["scopes"][scope]["target"].as_str().unwrap()),
                instance.join(relative)
            );
            unchanged["scopes"][scope]["target"] = before["scopes"][scope]["target"].clone();
        }
    }
    assert_eq!(
        unchanged, before,
        "only member destinations may change; checksums and membership stay intact"
    );
    for scope in &backup.scopes {
        assert_eq!(
            files::inventory(&Path::new(&backup.summary.backup_path).join(&scope.key)).unwrap(),
            scope.entries
        );
    }

    let mut report = fixture.plan.report.clone();
    for member in &mut report.members {
        let instance = Path::new(&next.servers_root).join(&member.summary.id);
        member.config_file_path = instance
            .join("config/instance.json")
            .to_string_lossy()
            .into_owned();
        member.saves_path = instance
            .join("runtime/ShooterGame/Saved/world")
            .to_string_lossy()
            .into_owned();
    }
    let moved_plan = build_plan(&reboot.paths, report, &[]).unwrap();
    assert_eq!(moved_plan.root, fixture.plan.root);
    assert_eq!(
        transaction::list(
            &moved_plan.root,
            moved_plan.report.identity.as_ref().unwrap()
        )
        .unwrap()
        .len(),
        1
    );
    for scope in &moved_plan.scopes {
        fs::write(
            scope.target.join("changed-after-migration"),
            b"later game data",
        )
        .unwrap();
    }
    let restored = restore::restore(&moved_plan, &backup.summary.backup_id).unwrap();
    assert!(restored.cleanup_warnings.is_empty());
    for (scope, original) in moved_plan.scopes.iter().zip(original) {
        assert_eq!(files::inventory(&scope.target).unwrap(), original);
    }
}

#[tokio::test]
async fn workspace_migration_preserves_cluster_snapshot_payloads_and_restores_all_members() {
    migrate_and_restore(false).await;
}

#[tokio::test]
async fn interrupted_workspace_migration_replays_cluster_snapshot_targets_before_restore() {
    migrate_and_restore(true).await;
}

#[tokio::test]
async fn snapshot_custom_paths_block_migration_without_changing_the_backup() {
    let fixture = Fixture::new();
    let paths = fixture_paths(&fixture.root);
    crate::bootstrap_storage_with_paths(paths.clone()).unwrap();
    let config_path = Path::new(&fixture.plan.report.members[0].config_file_path);
    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(config_path).unwrap()).unwrap();
    config["settings"]["custom_launch_flags"] = serde_json::json!(format!(
        "-modsdir={}",
        paths.instances_root.join("custom-mods").display()
    ));
    transaction::write_json(config_path, &config).unwrap();
    let backup = transaction::snapshot(&fixture.plan, "manual").unwrap();
    let manifest_path = Path::new(&backup.summary.backup_path).join("manifest.json");
    let before = fs::read(&manifest_path).unwrap();
    let mut next = paths.settings();
    next.servers_root = fixture
        .root
        .join("moved-instances")
        .to_string_lossy()
        .into_owned();
    next.archives_root = Path::new(&next.servers_root)
        .join(".trash")
        .to_string_lossy()
        .into_owned();
    let error = migration::preview(
        &paths,
        next.clone(),
        uuid::Uuid::new_v4().to_string(),
        Vec::new(),
        Arc::new(AtomicBool::new(false)),
        Arc::new(|_| {}),
    )
    .await
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("snapshot configuration.settings.custom_launch_flags"),
        "{error}"
    );
    assert!(!Path::new(&next.servers_root).exists());
    assert_eq!(fs::read(manifest_path).unwrap(), before);
    assert_eq!(
        files::inventory(&Path::new(&backup.summary.backup_path).join("member-000-config"))
            .unwrap(),
        backup.scopes[0].entries
    );
}
