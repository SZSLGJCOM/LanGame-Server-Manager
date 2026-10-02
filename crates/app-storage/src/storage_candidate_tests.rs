use super::*;

const MARKER: &str = ".langame-server-manager.json";
const CLAIM: &str = ".langame-server-manager.initializing";

#[test]
fn an_active_initializer_cannot_be_adopted_by_a_second_initializer() {
    let fixture = Fixture::new();
    let root = fixture.candidate("selected");
    let guard = super::super::candidate::prepare(&root).unwrap();
    let marker = fs::read(root.join(MARKER)).unwrap();
    assert!(super::super::candidate::prepare(&root).is_err());
    assert_eq!(fs::read(root.join(MARKER)).unwrap(), marker);
    drop(guard);
    assert!(!root.join(CLAIM).exists());
    let reused = super::super::candidate::prepare(&root).unwrap();
    assert_eq!(fs::read(root.join(MARKER)).unwrap(), marker);
    drop(reused);
}

#[test]
fn an_existing_initialization_claim_is_neither_opened_nor_removed() {
    let fixture = Fixture::new();
    let root = fixture.candidate("selected");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join(CLAIM), b"another initializer").unwrap();
    let before = fs::metadata(&root).unwrap().modified().unwrap();
    assert!(super::super::candidate::prepare(&root).is_err());
    assert_eq!(fs::read(root.join(CLAIM)).unwrap(), b"another initializer");
    assert_eq!(fs::metadata(&root).unwrap().modified().unwrap(), before);
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
}

#[test]
fn marker_publication_failure_cannot_make_an_unmarked_partial_layout_usable() {
    let fixture = Fixture::new();
    let root = fixture.candidate("selected");
    crate::atomic_file::fail_next_atomic_write_for_test(&root.join(MARKER));
    assert!(super::super::candidate::prepare(&root).is_err());
    assert!(!root.join(MARKER).exists());
    assert!(!root.join(CLAIM).exists());
    assert!(!fixture.pointer().exists());
    let before = fs::metadata(&root).unwrap().modified().unwrap();
    let entries = fs::read_dir(&root).unwrap().count();
    assert!(super::super::candidate::prepare(&root).is_err());
    assert_eq!(fs::read_dir(&root).unwrap().count(), entries);
    assert_eq!(fs::metadata(&root).unwrap().modified().unwrap(), before);
}

#[test]
fn failed_location_publication_does_not_overwrite_the_unregistered_database_on_retry() {
    let fixture = Fixture::new();
    let root = fixture.candidate("selected");
    crate::atomic_file::fail_next_atomic_write_for_test(&fixture.pointer());
    assert!(matches!(
        resolve_paths(&fixture.user, || Ok(vec![root.clone()])),
        Err(StorageError::WriteConfig { .. })
    ));
    assert!(!fixture.pointer().exists());
    assert!(root.join(MARKER).is_file());
    let private = fs::read_dir(root.join("app-data/ServerManager"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let database = fs::read(private.join("db/lgs.db")).unwrap();
    let settings = fs::read(private.join("settings.json")).unwrap();
    let next = resolve_paths(&fixture.user, || Ok(vec![root.clone()])).unwrap();
    assert_ne!(next.app_data_root, private);
    assert!(fs::read(private.join("db/lgs.db")).unwrap() == database);
    assert_eq!(fs::read(private.join("settings.json")).unwrap(), settings);
    assert!(next.database_path.is_file());
    assert!(fixture.pointer().is_file());
}

#[cfg(windows)]
#[test]
fn a_candidate_guard_prevents_replacement_of_the_root_marker_and_business_roots() {
    let fixture = Fixture::new();
    let root = fixture.candidate("selected");
    let guard = super::super::candidate::prepare(&root).unwrap();
    assert!(fs::write(root.join(MARKER), b"changed").is_err());
    for path in [
        root.clone(),
        root.join("instances"),
        root.join("app-data/ServerManager"),
    ] {
        assert!(fs::rename(&path, path.with_extension("replaced")).is_err());
    }
    drop(guard);
    assert!(!root.join(CLAIM).exists());
}
