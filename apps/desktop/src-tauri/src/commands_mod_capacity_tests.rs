use super::*;

#[test]
fn mod_capacity_accepts_large_payloads_and_exact_headroom_boundary() {
    let payload = 5 * 1024 * 1024 * 1024;
    assert!(check_capacity(payload + CAPACITY_RESERVE, payload).is_ok());
    assert!(check_capacity(CAPACITY_RESERVE, 0).is_ok());
    assert!(check_capacity(payload + CAPACITY_RESERVE - 1, payload).is_err());
    let error = check_capacity(CAPACITY_RESERVE - 1, 0).unwrap_err();
    assert!(error.contains("requires"));
    assert!(error.contains("available"));
    assert!(error.contains("512 MiB"));
}

#[test]
fn mod_capacity_rejects_overflow_instead_of_wrapping_required_space() {
    assert!(check_capacity(u64::MAX, u64::MAX).is_err());
    assert!(check_capacity(u64::MAX, u64::MAX - CAPACITY_RESERVE).is_ok());
    assert!(check_capacity(u64::MAX, u64::MAX - CAPACITY_RESERVE + 1).is_err());
}

#[test]
fn mod_space_budget_consumes_its_snapshot_and_rechecks_before_sixteen_mib() {
    let mut budget = ModSpaceBudget {
        path: PathBuf::from("unused-fixture"),
        available: CAPACITY_RESERVE + RECHECK_BYTES,
        written_since_probe: 0,
    };
    for _ in 0..255 {
        budget
            .reserve_with(COPY_BUFFER_BYTES, |_| panic!("premature capacity probe"))
            .unwrap();
    }
    assert_eq!(budget.written_since_probe, RECHECK_BYTES - 64 * 1024);
    let error = budget
        .reserve_with(COPY_BUFFER_BYTES, |_| Ok(CAPACITY_RESERVE))
        .unwrap_err();
    assert!(error.contains("available"));
    assert_eq!(budget.available, CAPACITY_RESERVE);
    assert_eq!(budget.written_since_probe, 0);
}

#[test]
fn mod_space_budget_checks_an_entire_large_chunk_and_does_not_consume_rejected_writes() {
    let mut budget = ModSpaceBudget {
        path: PathBuf::from("unused-fixture"),
        available: CAPACITY_RESERVE + 1,
        written_since_probe: 0,
    };
    assert!(
        budget
            .reserve_with(2, |_| panic!("unexpected probe"))
            .is_err()
    );
    assert_eq!(budget.available, CAPACITY_RESERVE + 1);
    budget
        .reserve_with(RECHECK_BYTES as usize + 1, |_| {
            Ok(CAPACITY_RESERVE + RECHECK_BYTES + 1)
        })
        .unwrap();
    assert_eq!(budget.available, CAPACITY_RESERVE);
    assert!(
        budget
            .reserve_with(1, |_| Err("probe failed".into()))
            .is_err()
    );
}

#[cfg(windows)]
struct Fixture(PathBuf);

#[cfg(windows)]
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-mod-capacity-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}

#[cfg(windows)]
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(windows)]
#[test]
fn mod_capacity_probes_the_real_nearest_directory_for_new_and_existing_files() {
    let fixture = Fixture::new();
    let directory = fixture.0.join("容量检查");
    fs::create_dir(&directory).unwrap();
    let existing = directory.join("existing.bin");
    fs::write(&existing, b"fixture").unwrap();
    let new = directory.join("missing/nested/package.zip");
    let canonical = fs::canonicalize(&directory).unwrap();
    assert_eq!(nearest_plain_directory(&new).unwrap(), canonical);
    assert_eq!(nearest_plain_directory(&existing).unwrap(), canonical);
    let available = available_bytes(&new).unwrap();
    assert!(available > 0);
    assert!(require_mod_capacity(&new, u64::MAX).is_err());
    assert!(!directory.join("missing").exists());
    if available >= CAPACITY_RESERVE {
        ModSpaceBudget::new(&new).unwrap().before_write(0).unwrap();
    }
}

#[cfg(windows)]
#[test]
fn mod_capacity_does_not_skip_an_existing_file_in_the_directory_chain() {
    let fixture = Fixture::new();
    let file = fixture.0.join("not-a-directory");
    fs::write(&file, b"fixture").unwrap();
    assert!(nearest_plain_directory(&file.join("nested/output.bin")).is_err());
}

#[cfg(windows)]
#[test]
fn mod_payload_copy_overwrites_truncates_and_preserves_source_permissions() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source.bin");
    let destination = fixture.0.join("destination.bin");
    let bytes = vec![0x5a; COPY_BUFFER_BYTES * 2 + 17];
    fs::write(&source, &bytes).unwrap();
    fs::write(&destination, vec![0x33; bytes.len() * 2]).unwrap();
    let original_permissions = fs::metadata(&source).unwrap().permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    fs::set_permissions(&source, permissions.clone()).unwrap();
    let result = copy_mod_payload(&source, &destination);
    // Restore only this fixture's attributes before assertions and cleanup.
    fs::set_permissions(&source, original_permissions.clone()).unwrap();
    let destination_readonly = fs::metadata(&destination).unwrap().permissions().readonly();
    fs::set_permissions(&destination, original_permissions).unwrap();
    assert_eq!(result.unwrap(), bytes.len() as u64);
    assert!(destination_readonly);
    assert_eq!(fs::read(&destination).unwrap(), bytes);
    assert_eq!(fs::read(&source).unwrap(), bytes);
}

#[cfg(windows)]
#[test]
fn mod_payload_copy_rejects_source_alias_without_truncating_source() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source.bin");
    fs::write(&source, b"source must survive").unwrap();
    assert!(copy_mod_payload(&source, &source).is_err());
    let alias = fixture.0.join("source-alias.bin");
    fs::hard_link(&source, &alias).unwrap();
    assert!(copy_mod_payload(&source, &alias).is_err());
    assert_eq!(fs::read(&source).unwrap(), b"source must survive");
}

#[cfg(not(windows))]
#[test]
fn mod_capacity_explicitly_rejects_unsupported_hosts() {
    assert!(
        require_mod_capacity(Path::new("."), 0)
            .unwrap_err()
            .contains("Windows")
    );
    assert!(ModSpaceBudget::new(Path::new(".")).is_err());
}
