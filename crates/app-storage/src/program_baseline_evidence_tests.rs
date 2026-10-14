use super::*;
use crate::instance_creation_io::test_gate::count_hash_reads;
use crate::private_runtime_refresh::{PackageTree, hash_open_file};
use app_core::InstallVerificationStage;

fn capture_evidence(root: &Path, package: &PackageTree) -> VerifiedProgramFiles {
    capture_evidence_with(root, package, VerifiedProgramFiles::capture_verified_file)
}

fn capture_evidence_with(
    root: &Path,
    package: &PackageTree,
    capture: fn(&mut VerifiedProgramFiles, &str, &fs::File, &str) -> Result<(), StorageError>,
) -> VerifiedProgramFiles {
    let mut verified = VerifiedProgramFiles::default();
    for (relative, expected) in &package.files {
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1);
        }
        let path = root.join(relative);
        let mut file = options.open(&path).unwrap();
        let digest = hash_open_file(&mut file, &path, None).unwrap();
        assert_eq!(&digest, expected);
        capture(&mut verified, relative, &file, &digest).unwrap();
    }
    verified
}

fn original_package(fixture: &Fixture, root: &Path) -> PackageTree {
    fixture.record(root);
    require_initial_package_tree(root, "fixture", None).unwrap()
}

#[cfg(windows)]
#[test]
fn large_file_admission_rehashes_and_clears_old_evidence_below_the_threshold() {
    let fixture = Fixture::new();
    let root = fixture.source();
    let below_threshold = VerifiedProgramFiles::MIN_LARGE_FILE_BYTES as usize - 1;
    put(
        &root,
        "assets/below-threshold.bin",
        &vec![37; below_threshold],
    );
    let package = original_package(&fixture, &root);
    let evidence = capture_evidence_with(&root, &package, |verified, relative, file, digest| {
        verified.capture_verified_file(relative, file, digest)?;
        verified.capture_large_verified_file(relative, file, digest)
    });
    let cancellation = Arc::new(AtomicBool::new(false));
    let reads = count_hash_reads(&cancellation);
    record_verified_library_program_baseline(
        &root,
        &fixture.descriptor,
        ProgramBaselineInventory {
            files: package.files,
            directories: package.directories,
            verified_files: evidence,
        },
        Some(&cancellation),
    )
    .unwrap();
    assert_eq!(
        reads.chunks(),
        18,
        "the 4 MiB-minus-one file and both small server files must be rehashed"
    );
}

#[cfg(windows)]
#[test]
fn large_file_admission_reuses_content_at_and_above_the_threshold() {
    for extra in [0, 1] {
        let fixture = Fixture::new();
        let root = fixture.source();
        let size = VerifiedProgramFiles::MIN_LARGE_FILE_BYTES as usize + extra;
        put(&root, "assets/large.bin", &vec![43; size]);
        let package = original_package(&fixture, &root);
        let evidence = capture_evidence_with(
            &root,
            &package,
            VerifiedProgramFiles::capture_large_verified_file,
        );
        let cancellation = Arc::new(AtomicBool::new(false));
        let reads = count_hash_reads(&cancellation);
        record_verified_library_program_baseline(
            &root,
            &fixture.descriptor,
            ProgramBaselineInventory {
                files: package.files,
                directories: package.directories,
                verified_files: evidence,
            },
            Some(&cancellation),
        )
        .unwrap();
        assert_eq!(
            reads.chunks(),
            2,
            "large content must be reused while both small server files are rehashed"
        );
    }
}

#[test]
fn large_file_admission_still_rejects_invalid_small_file_inputs() {
    let fixture = Fixture::new();
    let root = fixture.source();
    let file = fs::File::open(root.join("server.bin")).unwrap();
    let mut evidence = VerifiedProgramFiles::default();
    assert!(
        evidence
            .capture_large_verified_file("server.bin", &file, "invalid digest")
            .is_err()
    );
    assert!(
        evidence
            .capture_large_verified_file("../server.bin", &file, &"0".repeat(64))
            .is_err()
    );
}

#[cfg(windows)]
#[test]
fn current_installation_evidence_avoids_a_second_content_read() {
    let fixture = Fixture::new();
    let root = fixture.source();
    put(&root, "assets/large.bin", &vec![31; 512 * 1024 + 1]);
    let package = original_package(&fixture, &root);
    let evidence = capture_evidence(&root, &package);
    let cancellation = Arc::new(AtomicBool::new(false));
    let reads = count_hash_reads(&cancellation);
    record_verified_library_program_baseline(
        &root,
        &fixture.descriptor,
        ProgramBaselineInventory {
            files: package.files,
            directories: package.directories,
            verified_files: evidence,
        },
        Some(&cancellation),
    )
    .unwrap();
    assert_eq!(
        reads.chunks(),
        0,
        "unchanged verified handles must be reused"
    );
}

#[test]
fn missing_current_evidence_reads_every_official_file() {
    let fixture = Fixture::new();
    let root = fixture.source();
    let package = original_package(&fixture, &root);
    let cancellation = Arc::new(AtomicBool::new(false));
    let reads = count_hash_reads(&cancellation);
    record_verified_library_program_baseline(
        &root,
        &fixture.descriptor,
        ProgramBaselineInventory {
            files: package.files,
            directories: package.directories,
            verified_files: VerifiedProgramFiles::default(),
        },
        Some(&cancellation),
    )
    .unwrap();
    assert_eq!(
        reads.chunks(),
        2,
        "historical cache cannot replace current evidence"
    );
}

#[test]
fn current_evidence_cannot_authorize_a_different_expected_digest() {
    let fixture = Fixture::new();
    let root = fixture.source();
    let mut package = original_package(&fixture, &root);
    let evidence = capture_evidence(&root, &package);
    let before = fs::read(root.join(CLEAN_PACKAGE)).unwrap();
    package.files.insert("server.bin".into(), "0".repeat(64));
    let cancellation = Arc::new(AtomicBool::new(false));
    let reads = count_hash_reads(&cancellation);
    assert!(matches!(
        record_verified_library_program_baseline(
            &root,
            &fixture.descriptor,
            ProgramBaselineInventory {
                files: package.files,
                directories: package.directories,
                verified_files: evidence,
            },
            Some(&cancellation),
        ),
        Err(StorageError::CleanLibraryProgramRequired { .. })
    ));
    #[cfg(windows)]
    assert_eq!(
        reads.chunks(),
        1,
        "evidence must remain bound to its SHA-256"
    );
    #[cfg(not(windows))]
    assert_eq!(
        reads.chunks(),
        2,
        "unsupported evidence must recheck every file"
    );
    assert_eq!(fs::read(root.join(CLEAN_PACKAGE)).unwrap(), before);
}

#[cfg(windows)]
#[test]
fn replaced_file_is_rehashed_even_when_its_bytes_and_mtime_match() {
    let fixture = Fixture::new();
    let root = fixture.source();
    let package = original_package(&fixture, &root);
    let evidence = capture_evidence(&root, &package);
    let path = root.join("server.bin");
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let replacement = root.join("replacement.bin");
    fs::copy(&path, &replacement).unwrap();
    fs::remove_file(&path).unwrap();
    fs::rename(replacement, &path).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    let cancellation = Arc::new(AtomicBool::new(false));
    let reads = count_hash_reads(&cancellation);
    record_verified_library_program_baseline(
        &root,
        &fixture.descriptor,
        ProgramBaselineInventory {
            files: package.files,
            directories: package.directories,
            verified_files: evidence,
        },
        Some(&cancellation),
    )
    .unwrap();
    assert_eq!(
        reads.chunks(),
        1,
        "file identity changes must recheck content"
    );
}

#[cfg(windows)]
#[test]
fn same_size_rewrite_with_restored_mtime_cannot_reuse_installation_evidence() {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::*;
    let fixture = Fixture::new();
    let root = fixture.source();
    let path = root.join("server.bin");
    {
        let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
        let baseline_time = FILE_BASIC_INFO {
            ChangeTime: 132_000_000_000_000_000,
            ..Default::default()
        };
        assert_ne!(
            unsafe {
                SetFileInformationByHandle(
                    file.as_raw_handle(),
                    FileBasicInfo,
                    (&baseline_time as *const FILE_BASIC_INFO).cast(),
                    std::mem::size_of_val(&baseline_time) as u32,
                )
            },
            0
        );
    }
    let package = original_package(&fixture, &root);
    let evidence = capture_evidence(&root, &package);
    let before = fs::read(root.join(CLEAN_PACKAGE)).unwrap();
    let metadata = fs::metadata(&path).unwrap();
    fs::write(&path, vec![b'x'; metadata.len() as usize]).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(metadata.modified().unwrap()))
        .unwrap();
    let cancellation = Arc::new(AtomicBool::new(false));
    let reads = count_hash_reads(&cancellation);
    assert!(matches!(
        record_verified_library_program_baseline(
            &root,
            &fixture.descriptor,
            ProgramBaselineInventory {
                files: package.files,
                directories: package.directories,
                verified_files: evidence,
            },
            Some(&cancellation),
        ),
        Err(StorageError::CleanLibraryProgramRequired { .. })
    ));
    assert_eq!(
        reads.chunks(),
        1,
        "ChangeTime must invalidate restored mtime"
    );
    assert_eq!(fs::read(root.join(CLEAN_PACKAGE)).unwrap(), before);
}

#[test]
fn evidence_for_extra_files_does_not_expand_the_official_allowlist() {
    let fixture = Fixture::new();
    let root = fixture.source();
    let package = original_package(&fixture, &root);
    put(&root, "operator.dll", b"operator addition");
    let mut extended = package.clone();
    let path = root.join("operator.dll");
    let mut file = fs::File::open(&path).unwrap();
    extended.files.insert(
        "operator.dll".into(),
        hash_open_file(&mut file, &path, None).unwrap(),
    );
    let evidence = capture_evidence(&root, &extended);
    record_verified_library_program_baseline(
        &root,
        &fixture.descriptor,
        ProgramBaselineInventory {
            files: package.files,
            directories: package.directories,
            verified_files: evidence,
        },
        None,
    )
    .unwrap();
    assert!(
        !read_manifest(&root)
            .unwrap()
            .unwrap()
            .files
            .contains_key("operator.dll")
    );
    assert!(path.is_file());
}

#[test]
fn cancelled_evidence_publication_preserves_existing_baseline() {
    let fixture = Fixture::new();
    let root = fixture.source();
    let package = original_package(&fixture, &root);
    let evidence = capture_evidence(&root, &package);
    let before = fs::read(root.join(CLEAN_PACKAGE)).unwrap();
    let cancellation = AtomicBool::new(true);
    assert!(matches!(
        record_verified_library_program_baseline(
            &root,
            &fixture.descriptor,
            ProgramBaselineInventory {
                files: package.files,
                directories: package.directories,
                verified_files: evidence,
            },
            Some(&cancellation),
        ),
        Err(StorageError::InstanceCreationCancelled)
    ));
    assert_eq!(fs::read(root.join(CLEAN_PACKAGE)).unwrap(), before);
}

#[cfg(not(windows))]
#[test]
fn unsupported_filesystem_evidence_falls_back_to_content_verification() {
    let fixture = Fixture::new();
    let root = fixture.source();
    let package = original_package(&fixture, &root);
    let evidence = capture_evidence(&root, &package);
    let cancellation = Arc::new(AtomicBool::new(false));
    let reads = count_hash_reads(&cancellation);
    record_verified_library_program_baseline(
        &root,
        &fixture.descriptor,
        ProgramBaselineInventory {
            files: package.files,
            directories: package.directories,
            verified_files: evidence,
        },
        Some(&cancellation),
    )
    .unwrap();
    assert_eq!(reads.chunks(), 2);
}

#[test]
fn package_verification_progress_reports_actual_file_work_and_baseline_publication() {
    let fixture = Fixture::new();
    let root = fixture.source();
    let package = original_package(&fixture, &root);
    let expected_bytes: u64 = package
        .files
        .keys()
        .map(|relative| fs::metadata(root.join(relative)).unwrap().len())
        .sum();
    let expected_files = package.files.len() as u64;
    let mut updates = Vec::new();
    record_verified_library_program_baseline_with_progress(
        &root,
        &fixture.descriptor,
        ProgramBaselineInventory {
            files: package.files,
            directories: package.directories,
            verified_files: VerifiedProgramFiles::default(),
        },
        None,
        &mut |progress| updates.push(progress),
    )
    .unwrap();
    let package_updates: Vec<_> = updates
        .iter()
        .filter(|progress| progress.stage == InstallVerificationStage::PackageFiles)
        .collect();
    let first = package_updates.first().unwrap();
    let last = package_updates.last().unwrap();
    assert_eq!((first.completed_bytes, first.completed_files), (0, 0));
    assert_eq!(
        (last.total_bytes, last.completed_bytes),
        (expected_bytes, expected_bytes)
    );
    assert_eq!(
        (last.total_files, last.completed_files),
        (expected_files, expected_files)
    );
    assert!(package_updates.windows(2).all(|pair| {
        pair[0].completed_bytes <= pair[1].completed_bytes
            && pair[0].completed_files <= pair[1].completed_files
    }));
    let final_update = updates.last().unwrap();
    assert_eq!(
        final_update.stage,
        InstallVerificationStage::RecordingBaseline
    );
    assert_eq!(
        (final_update.total_files, final_update.completed_files),
        (2, 2)
    );
}

#[test]
fn cancellation_from_progress_stops_before_baseline_publication() {
    let fixture = Fixture::new();
    let root = fixture.source();
    let package = original_package(&fixture, &root);
    let evidence = capture_evidence(&root, &package);
    let before = fs::read(root.join(CLEAN_PACKAGE)).unwrap();
    let cancellation = AtomicBool::new(false);
    assert!(matches!(
        record_verified_library_program_baseline_with_progress(
            &root,
            &fixture.descriptor,
            ProgramBaselineInventory {
                files: package.files,
                directories: package.directories,
                verified_files: evidence,
            },
            Some(&cancellation),
            &mut |progress| {
                if progress.stage == InstallVerificationStage::RecordingBaseline {
                    cancellation.store(true, Ordering::Release);
                }
            },
        ),
        Err(StorageError::InstanceCreationCancelled)
    ));
    assert_eq!(fs::read(root.join(CLEAN_PACKAGE)).unwrap(), before);
}

#[test]
fn retained_inventories_share_monotonic_progress_for_their_file_union() {
    let fixture = Fixture::new();
    let root = fixture.source();
    put(&root, "user.cfg", b"official defaults");
    fixture.record(&root);
    let mut updates = Vec::new();
    assert!(
        retain_verified_library_program_baseline_with_progress(
            &root,
            &fixture.descriptor,
            None,
            &mut |progress| updates.push(progress),
        )
        .unwrap()
    );
    let package_updates: Vec<_> = updates
        .iter()
        .filter(|progress| progress.stage == InstallVerificationStage::PackageFiles)
        .collect();
    assert!(package_updates.windows(2).all(|pair| {
        pair[0].completed_bytes <= pair[1].completed_bytes
            && pair[0].completed_files <= pair[1].completed_files
            && pair[0].total_bytes == pair[1].total_bytes
            && pair[0].total_files == pair[1].total_files
    }));
    let final_package = package_updates.last().unwrap();
    assert_eq!(final_package.total_files, 3);
    assert_eq!(final_package.completed_files, final_package.total_files);
    assert_eq!(final_package.completed_bytes, final_package.total_bytes);
    assert_eq!(
        updates.last().unwrap().stage,
        InstallVerificationStage::RecordingBaseline
    );
}
