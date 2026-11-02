use super::*;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        Self::under(&std::env::temp_dir())
    }

    fn under(parent: &Path) -> Self {
        let path = parent.join(format!("langame-migration-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.0.join(relative)
    }

    fn write(&self, relative: &str, bytes: impl AsRef<[u8]>) {
        let path = self.path(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn roots(&self) -> Vec<RootMove> {
        vec![RootMove {
            source: self.path("source"),
            target: self.path("target"),
        }]
    }

    fn plan(&self) -> FilePlan {
        plan(&self.roots(), &[], None, &mut |_| {}).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Every fixture owns exactly this UUID directory. Link tests unlink
        // their test links before reaching this recursive fixture teardown.
        assert!(
            self.0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("langame-migration-test-")
        );
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn copy(plan: &FilePlan) {
    copy_verified(plan, None, &mut |_| {}).unwrap();
}

fn digest(path: &Path) -> FileDigest {
    let mut file = File::open(path).unwrap();
    hash_reader(&mut file, path, None, |_| {}).unwrap()
}

#[test]
fn copies_independent_verified_files_preserving_source_until_explicit_cleanup() {
    let fixture = Fixture::new();
    fixture.write("source/world/save.bin", b"saved world");
    fixture.write("source/config.json", b"{}");
    fs::create_dir_all(fixture.path("source/empty")).unwrap();
    let plan = fixture.plan();
    let serialized = serde_json::to_vec(&plan).unwrap();
    let plan: FilePlan = serde_json::from_slice(&serialized).unwrap();
    assert_eq!(plan.total_bytes, 13);
    copy(&plan);
    assert_eq!(
        fs::read(fixture.path("source/world/save.bin")).unwrap(),
        b"saved world"
    );
    assert_eq!(
        fs::read(fixture.path("target/world/save.bin")).unwrap(),
        b"saved world"
    );
    let source = checked_file(
        &fixture.path("source/world/save.bin"),
        false,
        None,
        None,
        None,
    )
    .unwrap();
    let target = checked_file(
        &fixture.path("target/world/save.bin"),
        false,
        None,
        None,
        None,
    )
    .unwrap();
    assert_ne!(source.identity().unwrap(), target.identity().unwrap());
    drop((source, target));
    verify_source_and_destination(&plan, &DestinationHashes::new()).unwrap();
    assert!(fixture.path("target/empty").is_dir());
    assert!(cleanup_sources(&plan, &DestinationHashes::new()).is_empty());
    assert!(!fixture.path("source").exists());
    assert!(cleanup_sources(&plan, &DestinationHashes::new()).is_empty());
    assert!(fixture.path("target/world/save.bin").exists());
}

#[test]
fn root_mapping_does_not_add_a_separator_and_path_keys_use_component_boundaries() {
    let fixture = Fixture::new();
    fixture.write("source/file", b"original");
    let plan = fixture.plan();
    let root = &plan.roots[0];
    let mapped = map_normalized(&root.source, &plan.roots);
    assert_eq!(mapped.as_os_str(), root.target.as_os_str());
    assert_eq!(PathKey(root.target.clone()), PathKey(root.target.join("")));
    assert_ne!(
        PathKey(root.target.clone()),
        PathKey(fixture.path("target-extra"))
    );
}

#[test]
fn stored_reference_mapping_preserves_the_configured_target_spelling() {
    let fixture = Fixture::new();
    fixture.write("source/folder/file", b"original");
    let target = std::env::temp_dir()
        .join(fixture.0.file_name().unwrap())
        .join("target");
    let roots = [RootMove {
        source: fixture.path("source"),
        target: target.clone(),
    }];
    let mapped = remap_stored_path(&fixture.path("source/folder/file"), &roots).unwrap();
    assert_eq!(
        mapped.as_os_str(),
        target.join("folder").join("file").as_os_str()
    );
    assert_eq!(
        remap_stored_path(&fixture.path("source"), &roots)
            .unwrap()
            .as_os_str(),
        target.as_os_str()
    );
}

#[test]
fn old_target_is_preserved_whole_and_retries_reuse_the_new_verified_copy() {
    let fixture = Fixture::new();
    fixture.write("source/same.txt", b"same bytes");
    fixture.write("target/same.txt", b"same bytes");
    fixture.write("target/extra.txt", b"unrelated");
    let before = native::open_verified_file(&fixture.path("target/same.txt"), false)
        .unwrap()
        .identity()
        .unwrap();
    let plan = fixture.plan();
    assert!(!plan.files[0].reused);
    assert!(preserved_destinations(&plan).unwrap().is_empty());
    copy(&plan);
    let first_copy = native::open_verified_file(&fixture.path("target/same.txt"), false)
        .unwrap()
        .identity()
        .unwrap();
    copy(&plan);
    let after = native::open_verified_file(&fixture.path("target/same.txt"), false)
        .unwrap()
        .identity()
        .unwrap();
    assert_ne!(before, after);
    assert_eq!(first_copy, after);
    let preserved = preserved_destinations(&plan).unwrap();
    assert_eq!(preserved.len(), 1);
    assert_eq!(preserved[0].0, fixture.path("target"));
    assert_eq!(
        native::open_verified_file(&preserved[0].1.join("same.txt"), false)
            .unwrap()
            .identity()
            .unwrap(),
        before
    );
    assert!(cleanup_sources(&plan, &DestinationHashes::new()).is_empty());
    assert!(!fixture.path("source").exists());
    assert!(!fixture.path("target/extra.txt").exists());
    assert_eq!(
        fs::read(preserved[0].1.join("extra.txt")).unwrap(),
        b"unrelated"
    );
}

#[test]
fn different_destination_contents_are_preserved_while_source_content_wins() {
    let fixture = Fixture::new();
    for number in 0..105 {
        fixture.write(&format!("source/{number}.txt"), b"original");
        fixture.write(&format!("target/{number}.txt"), b"unrelated");
    }
    fixture.write("source/new.txt", b"new source file");
    let plan = fixture.plan();
    assert_eq!(plan.conflict_count, 0);
    assert!(plan.conflicts.is_empty());
    copy(&plan);
    assert_eq!(
        fs::read(fixture.path("target/new.txt")).unwrap(),
        b"new source file"
    );
    let backup = &preserved_destinations(&plan).unwrap()[0].1;
    for number in 0..105 {
        assert_eq!(
            fs::read(backup.join(format!("{number}.txt"))).unwrap(),
            b"unrelated"
        );
    }
    assert_eq!(fs::read(fixture.path("target/0.txt")).unwrap(), b"original");
    assert_eq!(fs::read(fixture.path("source/0.txt")).unwrap(), b"original");
}

#[test]
fn old_internal_file_directory_types_do_not_block_whole_root_takeover() {
    let fixture = Fixture::new();
    fixture.write("source/folder/child.txt", b"child");
    fixture.write("target/folder", b"occupied");
    fixture.write("source/file.txt", b"file");
    fs::create_dir_all(fixture.path("target/file.txt")).unwrap();
    fixture.write("source/new.txt", b"new");
    let plan = fixture.plan();
    assert_eq!(plan.conflict_count, 0);
    copy(&plan);
    assert_eq!(
        fs::read(fixture.path("target/folder/child.txt")).unwrap(),
        b"child"
    );
    assert_eq!(fs::read(fixture.path("target/file.txt")).unwrap(), b"file");
    assert_eq!(fs::read(fixture.path("target/new.txt")).unwrap(), b"new");
    let backup = &preserved_destinations(&plan).unwrap()[0].1;
    assert_eq!(fs::read(backup.join("folder")).unwrap(), b"occupied");
    assert!(backup.join("file.txt").is_dir());
}

#[test]
fn nested_source_roots_are_enumerated_once_and_use_the_longest_mapping() {
    let fixture = Fixture::new();
    fixture.write("source/instance/save", b"instance");
    fixture.write("source/.archives/deleted/save", b"archived");
    let mut roots = fixture.roots();
    roots.push(RootMove {
        source: fixture.path("source/.archives"),
        target: fixture.path("archives"),
    });
    let plan = plan(&roots, &[], None, &mut |_| {}).unwrap();
    assert_eq!(plan.files.len(), 2);
    assert_eq!(
        remap_path(&fixture.path("source/.archives/deleted/save"), &plan.roots).unwrap(),
        fixture.path("archives/deleted/save")
    );
    assert_eq!(
        remap_path(&fixture.path("source-adjacent/file"), &plan.roots).unwrap(),
        fixture.path("source-adjacent/file")
    );
    copy(&plan);
    assert_eq!(
        fs::read(fixture.path("archives/deleted/save")).unwrap(),
        b"archived"
    );
    assert!(!fixture.path("target/.archives").exists());
    assert!(cleanup_sources(&plan, &DestinationHashes::new()).is_empty());
    assert!(!fixture.path("source").exists());
}

#[test]
fn rejects_any_target_overlap_with_any_source_root() {
    let fixture = Fixture::new();
    fixture.write("source/file", b"value");
    for target in [
        fixture.path("source"),
        fixture.path("source/nested"),
        fixture.0.clone(),
    ] {
        let roots = [RootMove {
            source: fixture.path("source"),
            target,
        }];
        assert!(plan(&roots, &[], None, &mut |_| {}).is_err());
    }
    let roots = [
        RootMove {
            source: fixture.path("source"),
            target: fixture.path("target"),
        },
        RootMove {
            source: fixture.path("target/nested"),
            target: fixture.path("another"),
        },
    ];
    assert!(plan(&roots, &[], None, &mut |_| {}).is_err());
    assert_eq!(fs::read(fixture.path("source/file")).unwrap(), b"value");
}

#[test]
fn rejects_relative_and_parent_traversal_paths() {
    let fixture = Fixture::new();
    // Joining onto a Windows verbatim base eagerly collapses `..`; retain its
    // raw spelling here so the test exercises the rejected input contract.
    let separator = std::path::MAIN_SEPARATOR;
    let traversal = PathBuf::from(format!(
        "{}{separator}source{separator}..{separator}other",
        fixture.0.display()
    ));
    for source in [PathBuf::from("relative"), traversal] {
        assert!(
            plan(
                &[RootMove {
                    source,
                    target: fixture.path("target")
                }],
                &[],
                None,
                &mut |_| {}
            )
            .is_err()
        );
    }
}

#[test]
fn physical_ancestor_identity_detects_overlap_and_preserves_missing_sibling_boundaries() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.path("source/inside")).unwrap();
    let source = directory_aliases(&fixture.path("source")).unwrap();
    let inside = directory_aliases(&fixture.path("source/inside/missing")).unwrap();
    let sibling = directory_aliases(&fixture.path("source-other/missing")).unwrap();
    assert!(physical_contains(&source, &inside));
    assert!(physical_overlap(&inside, &source));
    assert!(!physical_overlap(&source, &sibling));
    // A different pathname for the same existing directory has the same stable
    // anchor identity; the decision intentionally does not use its spelling.
    let aliased = vec![DirectoryAlias {
        identity: source[0].identity.clone(),
        tail: PathBuf::from("inside/new"),
    }];
    assert!(physical_contains(&source, &aliased));
    let absent = directory_aliases(&fixture.path("source/not-created")).unwrap();
    let adjacent = directory_aliases(&fixture.path("source/not-created-elsewhere")).unwrap();
    assert!(!physical_overlap(&absent, &adjacent));
}

#[test]
fn incompatible_sources_mapping_to_one_target_are_conflicts() {
    let fixture = Fixture::new();
    fixture.write("source/file", b"first");
    fixture.write("other/file", b"second");
    let roots = [
        RootMove {
            source: fixture.path("source"),
            target: fixture.path("target"),
        },
        RootMove {
            source: fixture.path("other"),
            target: fixture.path("target"),
        },
    ];
    let plan = plan(&roots, &[], None, &mut |_| {}).unwrap();
    assert_eq!(plan.conflict_count, 1);
    assert!(copy_verified(&plan, None, &mut |_| {}).is_err());
    assert!(!fixture.path("target").exists());
}

#[test]
fn incompatible_source_mapping_details_remain_bounded_before_any_target_write() {
    let fixture = Fixture::new();
    for number in 0..105 {
        fixture.write(&format!("source/{number}"), b"first source");
        fixture.write(&format!("other/{number}"), b"second source");
    }
    fixture.write("target/preserved", b"old target");
    let mut roots = fixture.roots();
    roots.push(RootMove {
        source: fixture.path("other"),
        target: fixture.path("target"),
    });
    let plan = plan(&roots, &[], None, &mut |_| {}).unwrap();
    assert_eq!(plan.conflict_count, 105);
    assert_eq!(plan.conflicts.len(), MAX_CONFLICT_DETAILS);
    assert!(copy_verified(&plan, None, &mut |_| {}).is_err());
    assert!(!plan.takeovers[0].recovery_root.exists());
    assert_eq!(
        fs::read(fixture.path("target/preserved")).unwrap(),
        b"old target"
    );
}

#[cfg(windows)]
#[test]
fn existing_volume_root_directory_guards_do_not_require_a_parent() {
    let fixture = Fixture::new();
    let volume = fixture.0.ancestors().last().unwrap();
    let before = volume_root_identity(volume).unwrap();
    let _guard = directory_guard(volume).unwrap();
    // This existing root is only opened/read, never created, renamed or deleted.
    ensure_directory(volume).unwrap();
    assert_eq!(volume_root_identity(volume).unwrap(), before);
    ensure_directory(&fixture.path("missing/parent/leaf")).unwrap();
    assert!(fixture.path("missing/parent/leaf").is_dir());
}

#[test]
fn an_original_merge_journal_can_finish_its_commit_but_cannot_start_a_new_copy() {
    let fixture = Fixture::new();
    fixture.write("source/data", b"source data");
    fixture.write("target/extra", b"old merge retained this");
    let new_plan = fixture.plan();
    let mut old_json = serde_json::to_value(&new_plan).unwrap();
    let old_object = old_json.as_object_mut().unwrap();
    old_object.remove("whole_directory");
    old_object.remove("takeovers");
    let journal = fixture.path("original-plan.json");
    crate::atomic_file::create_file_atomically(&journal, &serde_json::to_vec(&old_json).unwrap())
        .unwrap();
    let old_plan: FilePlan = serde_json::from_slice(&fs::read(journal).unwrap()).unwrap();
    assert!(!old_plan.whole_directory);
    assert!(preserved_destinations(&old_plan).unwrap().is_empty());
    assert!(recheck(&old_plan, None, &mut |_| {}).is_err());
    assert!(copy_verified(&old_plan, None, &mut |_| {}).is_err());
    assert!(!fixture.path("target/data").exists());
    assert!(!new_plan.takeovers[0].recovery_root.exists());

    // Reproduce the already-copied state of a durable original Applying journal.
    // Recovery must keep its old merge contract instead of requiring a new backup.
    fs::copy(fixture.path("source/data"), fixture.path("target/data")).unwrap();
    verify_source_and_destination(&old_plan, &DestinationHashes::new()).unwrap();
    assert!(cleanup_sources(&old_plan, &DestinationHashes::new()).is_empty());
    assert!(!fixture.path("source").exists());
    assert_eq!(
        fs::read(fixture.path("target/data")).unwrap(),
        b"source data"
    );
    assert_eq!(
        fs::read(fixture.path("target/extra")).unwrap(),
        b"old merge retained this"
    );
    assert!(cleanup_sources(&old_plan, &DestinationHashes::new()).is_empty());
    assert!(preserved_destinations(&old_plan).unwrap().is_empty());
}

#[test]
fn a_new_journal_cannot_silently_downgrade_by_losing_its_takeovers() {
    let fixture = Fixture::new();
    fixture.write("source/data", b"source data");
    let plan = fixture.plan();
    let mut missing = plan.clone();
    missing.takeovers.clear();
    assert!(recheck(&missing, None, &mut |_| {}).is_err());
    assert!(!cleanup_sources(&missing, &DestinationHashes::new()).is_empty());
    let mut mixed = plan;
    mixed.whole_directory = false;
    assert!(preserved_destinations(&mixed).is_err());
    assert!(verify_source_and_destination(&mixed, &DestinationHashes::new()).is_err());
    assert_eq!(
        fs::read(fixture.path("source/data")).unwrap(),
        b"source data"
    );
    assert!(!fixture.path("target").exists());
}

#[test]
fn intentional_lock_exclusions_keep_ancestors_without_masking_new_files() {
    for unexpected_file in [false, true] {
        let fixture = Fixture::new();
        fixture.write("source/file", b"migrate");
        fixture.write("source/.langame/locks/held", b"retained lock");
        let plan = plan(
            &fixture.roots(),
            &[fixture.path("source/.langame/locks")],
            None,
            &mut |_| {},
        )
        .unwrap();
        copy(&plan);
        if unexpected_file {
            fixture.write("source/.langame/new-after-copy", b"new content");
        }
        let warnings = cleanup_sources(&plan, &DestinationHashes::new());
        assert_eq!(warnings.is_empty(), !unexpected_file, "{warnings:?}");
        assert!(!fixture.path("source/file").exists());
        assert_eq!(
            fs::read(fixture.path("source/.langame/locks/held")).unwrap(),
            b"retained lock"
        );
        assert!(!fixture.path("target/.langame/locks").exists());
        if unexpected_file {
            assert_eq!(
                fs::read(fixture.path("source/.langame/new-after-copy")).unwrap(),
                b"new content"
            );
        }
    }
}

#[test]
fn exclusions_and_new_files_survive_cleanup_and_nonempty_roots_remain() {
    let fixture = Fixture::new();
    fixture.write("source/file", b"migrate");
    fixture.write("source/state/database", b"excluded");
    let plan = plan(
        &fixture.roots(),
        &[fixture.path("source/state")],
        None,
        &mut |_| {},
    )
    .unwrap();
    copy(&plan);
    fixture.write("source/new-after-copy", b"new data");
    let warnings = cleanup_sources(&plan, &DestinationHashes::new());
    assert!(!warnings.is_empty());
    assert!(!fixture.path("source/file").exists());
    assert_eq!(
        fs::read(fixture.path("source/state/database")).unwrap(),
        b"excluded"
    );
    assert_eq!(
        fs::read(fixture.path("source/new-after-copy")).unwrap(),
        b"new data"
    );
    assert!(!fixture.path("target/state").exists());
}

#[test]
fn source_changes_and_new_files_are_rejected_before_the_first_write() {
    for add_file in [false, true] {
        let fixture = Fixture::new();
        fixture.write("source/file", b"old");
        let plan = fixture.plan();
        if add_file {
            fixture.write("source/new", b"new");
        } else {
            fixture.write("source/file", b"changed");
        }
        assert!(copy_verified(&plan, None, &mut |_| {}).is_err());
        assert!(!fixture.path("target").exists());
        assert!(fixture.path("source/file").exists());
    }
}

#[test]
fn source_replacement_with_identical_bytes_is_detected_by_identity() {
    let fixture = Fixture::new();
    fixture.write("source/file", b"old");
    let plan = fixture.plan();
    fs::rename(fixture.path("source/file"), fixture.path("saved-original")).unwrap();
    fixture.write("source/file", b"old");
    assert!(copy_verified(&plan, None, &mut |_| {}).is_err());
    assert!(!fixture.path("target").exists());
}

#[test]
fn target_content_changes_with_unchanged_root_identity_are_preserved() {
    let fixture = Fixture::new();
    fixture.write("source/a", b"first");
    fixture.write("source/z", b"last");
    fixture.write("target/z", b"last");
    let plan = fixture.plan();
    fixture.write("target/z", b"changed elsewhere");
    recheck(&plan, None, &mut |_| {}).unwrap();
    copy(&plan);
    assert_eq!(fs::read(fixture.path("target/a")).unwrap(), b"first");
    assert_eq!(fs::read(fixture.path("target/z")).unwrap(), b"last");
    assert_eq!(
        fs::read(preserved_destinations(&plan).unwrap()[0].1.join("z")).unwrap(),
        b"changed elsewhere"
    );
}

#[test]
fn cancellation_before_preflight_makes_no_filesystem_changes() {
    let fixture = Fixture::new();
    fixture.write("source/file", b"original");
    let plan = fixture.plan();
    let cancel = AtomicBool::new(true);
    assert!(matches!(
        copy_verified(&plan, Some(&cancel), &mut |_| {}),
        Err(StorageError::StorageMigrationCancelled)
    ));
    assert!(!fixture.path("target").exists());
    assert_eq!(fs::read(fixture.path("source/file")).unwrap(), b"original");
}

#[test]
fn cancellation_during_stream_copy_removes_only_its_private_temporary_file() {
    let fixture = Fixture::new();
    let bytes = vec![17; BUFFER_BYTES * 2];
    fixture.write("source/file", &bytes);
    fixture.write("target/.langame-migration-not-ours.part", b"unrelated");
    let plan = fixture.plan();
    let cancel = AtomicBool::new(false);
    let result = copy_verified(&plan, Some(&cancel), &mut |status| {
        if status.phase == "copying" && status.completed_bytes > 0 {
            cancel.store(true, Ordering::Release);
        }
    });
    assert!(matches!(
        result,
        Err(StorageError::StorageMigrationCancelled)
    ));
    assert_eq!(fs::read(fixture.path("source/file")).unwrap(), bytes);
    assert!(!fixture.path("target/file").exists());
    let children = fs::read_dir(fixture.path("target"))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(children.is_empty());
    assert_eq!(
        fs::read(
            preserved_destinations(&plan).unwrap()[0]
                .1
                .join(".langame-migration-not-ours.part")
        )
        .unwrap(),
        b"unrelated"
    );
}

#[test]
fn publication_never_overwrites_a_file_created_after_preflight() {
    let fixture = Fixture::new();
    fixture.write("source/file", vec![29; BUFFER_BYTES * 2]);
    let plan = fixture.plan();
    let mut raced = false;
    let result = copy_verified(&plan, None, &mut |status| {
        if status.phase == "copying" && status.completed_bytes > 0 && !raced {
            raced = true;
            fixture.write("target/file", b"another writer");
        }
    });
    assert!(result.is_err());
    assert!(raced);
    assert_eq!(
        fs::read(fixture.path("target/file")).unwrap(),
        b"another writer"
    );
    assert_eq!(
        fs::metadata(fixture.path("source/file")).unwrap().len(),
        (BUFFER_BYTES * 2) as u64
    );
    assert_eq!(fs::read_dir(fixture.path("target")).unwrap().count(), 1);
}

#[test]
fn same_content_file_created_after_preflight_is_reused_safely() {
    let fixture = Fixture::new();
    let bytes = vec![31; BUFFER_BYTES * 2];
    fixture.write("source/file", &bytes);
    let plan = fixture.plan();
    let mut raced = false;
    copy_verified(&plan, None, &mut |status| {
        if status.phase == "copying" && status.completed_bytes > 0 && !raced {
            raced = true;
            fixture.write("target/file", &bytes);
        }
    })
    .unwrap();
    assert!(raced);
    assert_eq!(fs::read(fixture.path("target/file")).unwrap(), bytes);
    assert_eq!(fs::read_dir(fixture.path("target")).unwrap().count(), 1);
    assert!(fixture.path("source/file").exists());
}

#[test]
fn changed_source_and_destination_are_retained_during_cleanup() {
    let fixture = Fixture::new();
    fixture.write("source/changed-source", b"source before");
    fixture.write("source/changed-target", b"target before");
    fixture.write("source/unchanged", b"safe");
    let plan = fixture.plan();
    copy(&plan);
    fixture.write("source/changed-source", b"source after");
    fixture.write("target/changed-target", b"target after");
    assert!(verify_source_and_destination(&plan, &DestinationHashes::new()).is_err());
    let warnings = cleanup_sources(&plan, &DestinationHashes::new());
    assert!(warnings.len() >= 2);
    assert_eq!(
        fs::read(fixture.path("source/changed-source")).unwrap(),
        b"source after"
    );
    assert_eq!(
        fs::read(fixture.path("source/changed-target")).unwrap(),
        b"target before"
    );
    assert_eq!(
        fs::read(fixture.path("target/changed-target")).unwrap(),
        b"target after"
    );
    assert!(!fixture.path("source/unchanged").exists());
}

#[test]
fn committed_metadata_hash_override_preserves_source_verification() {
    let fixture = Fixture::new();
    fixture.write("source/config.json", br#"{"root":"old"}"#);
    let plan = fixture.plan();
    copy(&plan);
    fixture.write("target/config.json", br#"{"root":"new"}"#);
    assert!(verify_source_and_destination(&plan, &DestinationHashes::new()).is_err());
    let target = fixture.path("target/config.json");
    let hashes = BTreeMap::from([(target.clone(), digest(&target))]);
    verify_source_and_destination(&plan, &hashes).unwrap();
    assert!(cleanup_sources(&plan, &hashes).is_empty());
    assert_eq!(fs::read(target).unwrap(), br#"{"root":"new"}"#);
}

#[test]
fn cleanup_rejects_unknown_hash_overrides_without_deleting_sources() {
    let fixture = Fixture::new();
    fixture.write("source/file", b"original");
    let plan = fixture.plan();
    copy(&plan);
    let hashes = BTreeMap::from([(
        fixture.path("target/not-planned"),
        digest(&fixture.path("target/file")),
    )]);
    assert!(!cleanup_sources(&plan, &hashes).is_empty());
    assert!(fixture.path("source/file").exists());
}

#[test]
fn an_empty_source_directory_is_not_removed_if_its_destination_disappeared() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.path("source/empty")).unwrap();
    let plan = fixture.plan();
    copy(&plan);
    fs::remove_dir(fixture.path("target/empty")).unwrap();
    assert!(!cleanup_sources(&plan, &DestinationHashes::new()).is_empty());
    assert!(fixture.path("source/empty").is_dir());
}

#[cfg(windows)]
#[test]
fn windows_case_insensitive_component_boundaries_are_used_for_mapping_and_overlap() {
    let fixture = Fixture::new();
    fixture.write("source/file", b"original");
    let roots = fixture.plan().roots;
    let upper = PathBuf::from(fixture.path("SOURCE/FILE").to_string_lossy().to_uppercase());
    assert_eq!(
        PathKey(remap_path(&upper, &roots).unwrap()),
        PathKey(fixture.path("target/file"))
    );
    assert!(!contains(
        &fixture.path("source"),
        &fixture.path("source-other/file")
    ));
    let roots = [RootMove {
        source: fixture.path("source"),
        target: fixture.path("SOURCE/nested"),
    }];
    assert!(plan(&roots, &[], None, &mut |_| {}).is_err());
}

#[cfg(unix)]
#[test]
fn symbolic_source_entries_and_target_ancestors_are_rejected() {
    let fixture = Fixture::new();
    fixture.write("outside/file", b"outside");
    fs::create_dir_all(fixture.path("source")).unwrap();
    std::os::unix::fs::symlink(fixture.path("outside/file"), fixture.path("source/link")).unwrap();
    assert!(plan(&fixture.roots(), &[], None, &mut |_| {}).is_err());
    fs::remove_file(fixture.path("source/link")).unwrap();
    fixture.write("source/file", b"source");
    std::os::unix::fs::symlink(fixture.path("outside"), fixture.path("target")).unwrap();
    assert!(plan(&fixture.roots(), &[], None, &mut |_| {}).is_err());
    fs::remove_file(fixture.path("target")).unwrap();
    assert_eq!(fs::read(fixture.path("outside/file")).unwrap(), b"outside");
}

#[cfg(windows)]
#[test]
fn source_junctions_and_destination_junction_roots_are_rejected_without_touching_external_data() {
    let fixture = Fixture::new();
    let outside = Fixture::new();
    outside.write("sentinel", b"external data");
    fixture.write("source/file", b"original");
    for relative in ["source/link", "target"] {
        let link = fixture.path(relative);
        let output = std::process::Command::new("cmd")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside.0)
            .output()
            .unwrap();
        assert!(output.status.success(), "junction fixture: {output:?}");
        let result = plan(&fixture.roots(), &[], None, &mut |_| {});
        fs::remove_dir(&link).unwrap();
        assert!(result.is_err());
        assert_eq!(
            fs::read(outside.path("sentinel")).unwrap(),
            b"external data"
        );
        assert_eq!(fs::read(fixture.path("source/file")).unwrap(), b"original");
    }
}

#[test]
fn traversal_depth_is_bounded_before_destination_creation() {
    let fixture = Fixture::new();
    let mut path = fixture.path("source");
    for _ in 0..=MAX_DEPTH {
        path.push("a");
    }
    fs::create_dir_all(path).unwrap();
    assert!(plan(&fixture.roots(), &[], None, &mut |_| {}).is_err());
    assert!(!fixture.path("target").exists());
}

#[test]
fn absent_source_roots_still_require_plain_created_destination_roots() {
    let fixture = Fixture::new();
    let plan = fixture.plan();
    assert!(plan.files.is_empty());
    assert!(plan.directories.is_empty());
    copy(&plan);
    verify_source_and_destination(&plan, &DestinationHashes::new()).unwrap();
    fs::remove_dir(fixture.path("target")).unwrap();
    assert!(verify_source_and_destination(&plan, &DestinationHashes::new()).is_err());
}

#[cfg(windows)]
#[test]
fn copies_between_workspace_and_temp_volumes_when_they_are_distinct() {
    let (source_parent, target_parent, require_distinct_volumes) = match (
        std::env::var_os("LANGAME_MIGRATION_SOURCE_TEST_ROOT"),
        std::env::var_os("LANGAME_MIGRATION_DESTINATION_TEST_ROOT"),
    ) {
        (Some(source), Some(target)) => (PathBuf::from(source), PathBuf::from(target), true),
        (None, None) => (
            std::env::temp_dir(),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
            false,
        ),
        _ => panic!("cross-volume test roots must be configured together"),
    };
    // A source snapshot need not have an in-repository target directory.
    // Explicit test roots exercise separate volumes even when TEMP is relocated.
    let source_fixture = Fixture::under(&source_parent);
    let target_parent = fs::canonicalize(target_parent).unwrap();
    let source_volume = native::open(&source_fixture.0, true, false)
        .unwrap()
        .identity()
        .unwrap();
    let target_volume = native::open(&target_parent, true, false)
        .unwrap()
        .identity()
        .unwrap();
    if source_volume.same_volume(&target_volume) {
        assert!(
            !require_distinct_volumes,
            "explicit cross-volume test roots must be on separate volumes"
        );
        return;
    }
    let target_fixture = Fixture::under(&target_parent);
    source_fixture.write("source/file", b"cross-volume content");
    let roots = [RootMove {
        source: source_fixture.path("source"),
        target: target_fixture.path("target"),
    }];
    let plan = plan(&roots, &[], None, &mut |_| {}).unwrap();
    copy(&plan);
    verify_source_and_destination(&plan, &DestinationHashes::new()).unwrap();
    assert_eq!(
        fs::read(target_fixture.path("target/file")).unwrap(),
        b"cross-volume content"
    );
    assert!(source_fixture.path("source/file").exists());
    assert!(cleanup_sources(&plan, &DestinationHashes::new()).is_empty());
}
