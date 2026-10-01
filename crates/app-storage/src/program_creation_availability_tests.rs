use super::*;
use app_core::InstanceProgramSource;

async fn inspect(
    fixture: &CleanCreationFixture,
    source: InstanceProgramSource,
    include_archives: bool,
) -> crate::InstanceProgramCreationPlan {
    crate::program_exclusive::inspect_creation_sources(
        &fixture.paths,
        &fixture.descriptor,
        Some(InstanceProgramMode::Independent),
        source,
        None,
        include_archives,
    )
    .await
    .unwrap()
}

async fn commit_uninstall(fixture: &CleanCreationFixture) {
    let owner = crate::read_program_install_owner(&fixture.paths, &fixture.source)
        .await
        .unwrap()
        .unwrap();
    let mut removal = crate::program_removals_db::ProgramRemovalRecord {
        operation_id: uuid::Uuid::new_v4().to_string(),
        module_id: fixture.descriptor.summary.id.clone(),
        install_id: owner.id,
        source_root: fixture.source.clone(),
        phase: "prepared".into(),
        journal_json: "{}".into(),
    };
    crate::program_removals_db::begin(&fixture.paths, &removal)
        .await
        .unwrap();
    crate::program_removals_db::commit(&fixture.paths, &removal)
        .await
        .unwrap();
    removal.phase = "committed".into();
    crate::program_removals_db::finish(&fixture.paths, &removal)
        .await
        .unwrap();
    assert_eq!(
        crate::read_program_install_owner(&fixture.paths, &fixture.source)
            .await
            .unwrap()
            .unwrap()
            .install_state,
        InstallState::NotInstalled
    );
}

#[tokio::test]
async fn creation_availability_requires_a_download_after_explicit_uninstall_without_sources() {
    let fixture = CleanCreationFixture::new().await;
    fixture.record();
    register(&fixture).await;
    commit_uninstall(&fixture).await;
    // Uninstall can preserve private data in the old directory. Its presence
    // must not turn an explicit NotInstalled record into a repair request.
    let before = crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap();
    let pending = inspect(&fixture, InstanceProgramSource::Verified, false).await;
    assert!(pending.requires_archive_inventory);
    assert!(!pending.can_create);
    for source in [
        InstanceProgramSource::Verified,
        InstanceProgramSource::Local,
    ] {
        let complete = inspect(&fixture, source, true).await;
        assert!(!complete.requires_archive_inventory);
        assert!(
            !complete.can_create,
            "explicit uninstall needs a new download or a reusable source"
        );
    }
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap(),
        before
    );
}

#[tokio::test]
async fn creation_availability_preserves_repair_for_interrupted_or_missing_installations() {
    for state in [
        InstallState::Incomplete,
        InstallState::Corrupted,
        InstallState::Installed,
    ] {
        let fixture = CleanCreationFixture::new().await;
        sync_game_installs(
            &fixture.paths,
            &[GameInstallSyncRecord {
                module_id: fixture.descriptor.summary.id.clone(),
                install_root: fixture.source.to_string_lossy().into_owned(),
                install_state: state.clone(),
                current_version: None,
                mark_verified: false,
            }],
        )
        .await
        .unwrap();
        if state == InstallState::Installed {
            fs::rename(
                &fixture.source,
                fixture.root.join("unexpectedly-moved-library"),
            )
            .unwrap();
        }
        let verified = inspect(&fixture, InstanceProgramSource::Verified, true).await;
        assert!(verified.can_create, "{state:?} must remain repairable");
        assert!(!verified.requires_archive_inventory);
        assert!(
            !inspect(&fixture, InstanceProgramSource::Local, true)
                .await
                .can_create
        );
    }
}

#[tokio::test]
async fn creation_availability_reuses_instances_and_archives_after_explicit_uninstall() {
    let fixture = CleanCreationFixture::new().await;
    fixture.record();
    register(&fixture).await;
    let created = fixture.create(None).await.unwrap();
    let binding =
        crate::read_instance_program_install(&fixture.paths, &created.provisioning.summary.id)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(binding.install.scope, ProgramInstallScope::Instance);
    commit_uninstall(&fixture).await;
    let instance = inspect(&fixture, InstanceProgramSource::Verified, false).await;
    assert!(instance.can_create);
    assert!(!instance.requires_archive_inventory);
    assert_eq!(
        fs::canonicalize(instance.program_path).unwrap(),
        fs::canonicalize(&created.effective_install_root).unwrap()
    );

    crate::archive_instance(&fixture.paths, &created.provisioning.summary.id)
        .await
        .unwrap();
    assert!(list_instances(&fixture.paths).await.unwrap().is_empty());
    let pending = inspect(&fixture, InstanceProgramSource::Verified, false).await;
    assert!(!pending.can_create);
    assert!(pending.requires_archive_inventory);
    let archive = inspect(&fixture, InstanceProgramSource::Verified, true).await;
    assert!(archive.can_create);
    assert!(!archive.requires_archive_inventory);
    let sources = crate::read_archived_program_sources(&fixture.paths)
        .await
        .unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(
        fs::canonicalize(archive.program_path).unwrap(),
        fs::canonicalize(&sources[0].install_root).unwrap()
    );
}
