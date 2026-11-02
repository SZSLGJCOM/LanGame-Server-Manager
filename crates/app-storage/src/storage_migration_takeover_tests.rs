use super::*;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("langame-takeover-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self(fs::canonicalize(root).unwrap())
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn write(&self, name: &str, bytes: &[u8]) {
        let path = self.path(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn plan(&self) -> FilePlan {
        let plan = super::super::plan(
            &[RootMove {
                source: self.path("source"),
                target: self.path("target"),
            }],
            &[],
            None,
            &mut |_| {},
        )
        .unwrap();
        crate::atomic_file::create_file_atomically(
            &self.path("plan.json"),
            &serde_json::to_vec(&plan).unwrap(),
        )
        .unwrap();
        plan
    }

    fn copy(&self, plan: &FilePlan) {
        copy_verified(plan, None, &mut |_| {}).unwrap();
        assert_eq!(fs::read(self.path("target/data")).unwrap(), b"source data");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(
            self.0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("langame-takeover-test-")
        );
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[cfg(windows)]
#[test]
fn volume_and_share_roots_cannot_be_taken_over() {
    for root in [
        r"D:\",
        "D:/",
        r"\\?\D:\",
        r"\\host\share\",
        r"\\?\UNC\host\share\",
    ] {
        // This syntax check runs before any destination access or recovery writes.
        let error = validate_target_syntax(Path::new(root)).unwrap_err();
        assert!(error.to_string().contains("absolute non-volume root"));
        for child in [
            "LanGame/server-files",
            "LanGame/instances",
            "LanGame/cmd/steamcmd",
        ] {
            assert!(validate_target_syntax(&Path::new(root).join(child)).is_ok());
        }
    }
}

#[test]
fn every_takeover_crash_boundary_resumes_the_same_manifest_and_backup() {
    for point in [
        TakeoverPoint::ManifestSaved,
        TakeoverPoint::OriginalPreserved,
        TakeoverPoint::BeforePublish,
        TakeoverPoint::Published,
    ] {
        let fixture = Fixture::new();
        fixture.write("source/data", b"source data");
        fixture.write("target/data", b"previous data");
        fixture.write("target/extra", b"keep all old bytes");
        fs::create_dir(fixture.path("target/empty")).unwrap();
        let plan = fixture.plan();
        let mut reached = false;
        let result = acquire_with(&plan, None, &mut |at, root| {
            if at == point {
                reached = true;
                return Err(invalid(&root.target, "injected process interruption"));
            }
            Ok(())
        });
        assert!(result.is_err());
        assert!(reached);
        assert_eq!(
            fs::read(fixture.path("source/data")).unwrap(),
            b"source data"
        );
        let manifest_before = fs::read(plan.takeovers[0].manifest()).unwrap();
        let restored: FilePlan =
            serde_json::from_slice(&fs::read(fixture.path("plan.json")).unwrap()).unwrap();
        fixture.copy(&restored);
        fixture.copy(&restored);
        assert_eq!(
            fs::read(plan.takeovers[0].manifest()).unwrap(),
            manifest_before
        );
        let preserved = super::super::preserved_destinations(&restored).unwrap();
        assert_eq!(preserved.len(), 1);
        assert_eq!(
            fs::read(preserved[0].1.join("data")).unwrap(),
            b"previous data"
        );
        assert_eq!(
            fs::read(preserved[0].1.join("extra")).unwrap(),
            b"keep all old bytes"
        );
        assert!(preserved[0].1.join("empty").is_dir());
        assert!(!fixture.path("target/extra").exists());
        assert!(!fixture.path("target/empty").exists());
        assert!(cleanup_sources(&restored, &DestinationHashes::new()).is_empty());
        assert!(!fixture.path("source").exists());
    }
}

#[test]
fn cancellation_after_preservation_keeps_source_and_can_resume() {
    let fixture = Fixture::new();
    fixture.write("source/data", b"source data");
    fixture.write("target/data", b"previous data");
    let plan = fixture.plan();
    let cancel = AtomicBool::new(false);
    let result = acquire_with(&plan, Some(&cancel), &mut |at, _| {
        if at == TakeoverPoint::OriginalPreserved {
            cancel.store(true, Ordering::Release);
        }
        Ok(())
    });
    assert!(matches!(
        result,
        Err(StorageError::StorageMigrationCancelled)
    ));
    assert!(!fixture.path("target").exists());
    assert_eq!(
        fs::read(fixture.path("source/data")).unwrap(),
        b"source data"
    );
    let backup = &super::super::preserved_destinations(&plan).unwrap()[0].1;
    assert_eq!(fs::read(backup.join("data")).unwrap(), b"previous data");
    cancel.store(false, Ordering::Release);
    fixture.copy(&plan);
    assert_eq!(fs::read(backup.join("data")).unwrap(), b"previous data");
}

#[test]
fn a_manifest_write_failure_never_moves_the_original_target() {
    let fixture = Fixture::new();
    fixture.write("source/data", b"source data");
    fixture.write("target/data", b"previous data");
    let plan = fixture.plan();
    crate::atomic_file::fail_next_atomic_write_for_test(&plan.takeovers[0].manifest());
    assert!(copy_verified(&plan, None, &mut |_| {}).is_err());
    assert_eq!(
        fs::read(fixture.path("target/data")).unwrap(),
        b"previous data"
    );
    assert_eq!(
        fs::read(fixture.path("source/data")).unwrap(),
        b"source data"
    );
    assert!(
        super::super::preserved_destinations(&plan)
            .unwrap()
            .is_empty()
    );
    // An incomplete reservation is never adopted or erased by a later attempt.
    assert!(copy_verified(&plan, None, &mut |_| {}).is_err());
    let next = super::super::plan(&plan.roots, &[], None, &mut |_| {}).unwrap();
    assert_ne!(
        next.takeovers[0].recovery_root,
        plan.takeovers[0].recovery_root
    );
    crate::atomic_file::create_file_atomically(
        &fixture.path("next-plan.json"),
        &serde_json::to_vec(&next).unwrap(),
    )
    .unwrap();
    fixture.copy(&next);
    assert!(plan.takeovers[0].recovery_root.is_dir());
}

#[test]
fn an_external_creator_in_the_publication_gap_is_never_replaced() {
    let fixture = Fixture::new();
    fixture.write("source/data", b"source data");
    fixture.write("target/data", b"previous data");
    let plan = fixture.plan();
    let result = acquire_with(&plan, None, &mut |at, root| {
        if at == TakeoverPoint::BeforePublish {
            fs::write(&root.target, b"external creator").unwrap();
        }
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(
        fs::read(fixture.path("target")).unwrap(),
        b"external creator"
    );
    assert!(copy_verified(&plan, None, &mut |_| {}).is_err());
    assert_eq!(
        fs::read(fixture.path("target")).unwrap(),
        b"external creator"
    );
    assert_eq!(
        fs::read(plan.takeovers[0].backup().join("data")).unwrap(),
        b"previous data"
    );
    assert_eq!(
        fs::read(fixture.path("source/data")).unwrap(),
        b"source data"
    );
}

#[test]
fn an_existing_backup_name_is_never_overwritten_or_treated_as_owned() {
    let fixture = Fixture::new();
    fixture.write("source/data", b"source data");
    fixture.write("target/data", b"previous data");
    let plan = fixture.plan();
    let result = acquire_with(&plan, None, &mut |at, root| {
        if at == TakeoverPoint::ManifestSaved {
            fs::write(root.backup(), b"unrelated recovery file").unwrap();
        }
        Ok(())
    });
    assert!(result.is_err());
    assert!(super::super::preserved_destinations(&plan).is_err());
    assert_eq!(
        fs::read(plan.takeovers[0].backup()).unwrap(),
        b"unrelated recovery file"
    );
    assert_eq!(
        fs::read(fixture.path("target/data")).unwrap(),
        b"previous data"
    );
    assert_eq!(
        fs::read(fixture.path("source/data")).unwrap(),
        b"source data"
    );
}

#[test]
fn a_replaced_destination_root_is_rejected_before_any_recovery_write() {
    let fixture = Fixture::new();
    fixture.write("source/data", b"source data");
    fixture.write("target/data", b"previous data");
    let plan = fixture.plan();
    fs::rename(fixture.path("target"), fixture.path("relocated-original")).unwrap();
    fixture.write("target/data", b"replacement root");
    assert!(super::super::recheck(&plan, None, &mut |_| {}).is_err());
    assert!(copy_verified(&plan, None, &mut |_| {}).is_err());
    assert!(!plan.takeovers[0].recovery_root.exists());
    assert_eq!(
        fs::read(fixture.path("target/data")).unwrap(),
        b"replacement root"
    );
    assert_eq!(
        fs::read(fixture.path("relocated-original/data")).unwrap(),
        b"previous data"
    );
}

#[test]
fn a_file_at_the_outer_target_is_preserved_before_creating_the_new_tree() {
    let fixture = Fixture::new();
    fixture.write("source/data", b"source data");
    fixture.write("target", b"old root file");
    let plan = fixture.plan();
    fixture.copy(&plan);
    assert_eq!(
        fs::read(plan.takeovers[0].backup()).unwrap(),
        b"old root file"
    );
    assert!(cleanup_sources(&plan, &DestinationHashes::new()).is_empty());
    assert!(!fixture.path("source").exists());
}

#[test]
fn nested_destinations_share_one_backup_and_ignore_old_internal_types() {
    let fixture = Fixture::new();
    fixture.write("source/data", b"source data");
    fixture.write("source/archive/retired", b"archived source data");
    fixture.write("target/container", b"old file instead of directory");
    let roots = [
        RootMove {
            source: fixture.path("source"),
            target: fixture.path("target"),
        },
        RootMove {
            source: fixture.path("source/archive"),
            target: fixture.path("target/container/archive"),
        },
    ];
    let plan = super::super::plan(&roots, &[], None, &mut |_| {}).unwrap();
    assert_eq!(plan.takeovers.len(), 1);
    assert_eq!(
        remap_path(&fixture.path("source/archive/retired"), &roots).unwrap(),
        fixture.path("target/container/archive/retired")
    );
    crate::atomic_file::create_file_atomically(
        &fixture.path("plan.json"),
        &serde_json::to_vec(&plan).unwrap(),
    )
    .unwrap();
    fixture.copy(&plan);
    assert_eq!(
        fs::read(plan.takeovers[0].backup().join("container")).unwrap(),
        b"old file instead of directory"
    );
    assert_eq!(
        fs::read(fixture.path("target/container/archive/retired")).unwrap(),
        b"archived source data"
    );
    assert!(!fixture.path("target/archive").exists());
    assert!(cleanup_sources(&plan, &DestinationHashes::new()).is_empty());
    assert!(!fixture.path("source").exists());
}

#[test]
fn an_unexpected_new_destination_entry_blocks_all_source_cleanup() {
    let fixture = Fixture::new();
    fixture.write("source/data", b"source data");
    let plan = fixture.plan();
    fixture.copy(&plan);
    fixture.write("target/foreign", b"outside write after copy");
    assert!(verify_source_and_destination(&plan, &DestinationHashes::new()).is_err());
    assert!(!cleanup_sources(&plan, &DestinationHashes::new()).is_empty());
    assert_eq!(
        fs::read(fixture.path("source/data")).unwrap(),
        b"source data"
    );
    assert_eq!(
        fs::read(fixture.path("target/foreign")).unwrap(),
        b"outside write after copy"
    );
}

#[test]
fn internal_old_target_junctions_are_preserved_without_following_them() {
    let fixture = Fixture::new();
    fixture.write("source/data", b"source data");
    fixture.write("external/sentinel", b"do not follow or change");
    fs::create_dir(fixture.path("target")).unwrap();
    let link = fixture.path("target/link");
    let output = std::process::Command::new("cmd")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(&link)
        .arg(fixture.path("external"))
        .output()
        .unwrap();
    assert!(output.status.success(), "junction fixture: {output:?}");
    let plan = fixture.plan();
    let copied = copy_verified(&plan, None, &mut |_| {});
    // Remove only the fixture junction before recursive fixture teardown.
    let relocated = plan.takeovers[0].backup().join("link");
    if relocated.exists() {
        fs::remove_dir(&relocated).unwrap();
    }
    if link.exists() {
        fs::remove_dir(&link).unwrap();
    }
    copied.unwrap();
    assert_eq!(
        fs::read(fixture.path("external/sentinel")).unwrap(),
        b"do not follow or change"
    );
    assert!(!fixture.path("target/link").exists());
}
