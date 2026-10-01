use super::*;

#[tokio::test]
async fn compact_archive_reads_library_three_times_and_restores_exact_bytes() {
    let (fixture, library) = operations::with_program().await;
    let before = crate::test_file_snapshot::tree_snapshot(&fixture.instance_root).unwrap();
    let library_before = crate::test_file_snapshot::tree_snapshot(&library).unwrap();
    let reads = read_probe::register(&fixture.root);
    let archived = fixture.archive().await.unwrap();
    for (relative, bytes) in [
        ("server.exe", b"official program".len() as u64),
        ("assets/content.bin", b"official content".len() as u64),
    ] {
        assert_eq!(
            reads.bytes(&library.join(relative)),
            bytes * 3,
            "{relative}: planning, pre-move validation and final source validation each read once"
        );
    }
    drop(reads);
    let root = PathBuf::from(archived.archived_instance_root.as_ref().unwrap());
    assert!(!root.join("runtime/server.exe").exists());
    assert!(!root.join("runtime/assets/content.bin").exists());
    assert_eq!(
        fs::read(root.join("runtime/unknown.dll")).unwrap(),
        b"operator bytes"
    );
    let archive_before = crate::test_file_snapshot::tree_snapshot(&root).unwrap();
    let listed = list_instance_archives(&fixture.paths).await.unwrap();
    assert_eq!(listed.archives.len(), 1);
    assert_eq!(listed.archives[0].omitted_program_files, 2);
    assert!(listed.archives[0].can_restore);
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&root).unwrap(),
        archive_before
    );
    restore_instance_archive(&fixture.paths, &archived.archive_id)
        .await
        .unwrap();
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&fixture.instance_root).unwrap(),
        before
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&library).unwrap(),
        library_before
    );
}

#[tokio::test]
async fn external_archive_reads_owned_source_four_times_and_restores_exact_bytes() {
    let (fixture, library) = external_programs::exclusive_fixture(true).await;
    // External capture resolves short-name aliases before its reads are counted.
    let library = fs::canonicalize(library).unwrap();
    let before = crate::test_file_snapshot::tree_snapshot(&fixture.instance_root).unwrap();
    let library_before = crate::test_file_snapshot::tree_snapshot(&library).unwrap();
    let reads = read_probe::register(&fs::canonicalize(&fixture.root).unwrap());
    let archived = fixture.archive().await.unwrap();
    assert_eq!(
        reads.bytes(&library.join("native/world.dat")),
        b"world".len() as u64 * 4,
        "inventory, copy, cleanup preflight and final unlink validation each read once"
    );
    drop(reads);
    assert!(!library.join("native/world.dat").exists());
    assert_eq!(
        fs::read(library.join("server.exe")).unwrap(),
        b"official bytes"
    );
    assert_eq!(fs::read(library.join("Mods/custom.dll")).unwrap(), b"mod");
    assert_eq!(fs::read(library.join("custom.dll")).unwrap(), b"custom");
    assert_eq!(
        fs::read(library.join("unowned/foreign.dat")).unwrap(),
        b"foreign"
    );
    let root = PathBuf::from(archived.archived_instance_root.as_ref().unwrap());
    let archive_before = crate::test_file_snapshot::tree_snapshot(&root).unwrap();
    let listed = list_instance_archives(&fixture.paths).await.unwrap();
    assert_eq!(listed.archives.len(), 1);
    assert_eq!(listed.archives[0].program_storage, "reconstructable");
    assert!(listed.archives[0].can_restore);
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&root).unwrap(),
        archive_before
    );
    restore_instance_archive(&fixture.paths, &archived.archive_id)
        .await
        .unwrap();
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&fixture.instance_root).unwrap(),
        before
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&library).unwrap(),
        library_before
    );
    assert!(!fixture.instance_root.join(external::PAYLOAD).exists());
}
