use super::*;
use std::collections::BTreeMap;

struct Fixture {
    base: PathBuf,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!("lgsm-inspection-{}", uuid::Uuid::new_v4()));
        let root = base.join("library");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("x.bin"), b"abc").unwrap();
        Self { base, root }
    }
    fn inventory(&self) -> Inventory {
        let mut expected = Inventory::default();
        expected.files = BTreeMap::from([(
            "x.bin".into(),
            Entry {
                size: 3,
                sha1: "a9993e364706816aba3e25717850c26c9cd0d89d".into(),
            },
        )]);
        expected
    }
    fn inspect(&self, expected: &Inventory, mode: Inspection) -> (bool, Vec<Value>) {
        let mut events = Vec::new();
        let result = inspect(&self.root, 7, expected, mode, false, |event| {
            events.push(event)
        });
        (result.is_ok(), events)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.base).unwrap();
    }
}

fn summary(events: &[Value]) -> &Value {
    &events
        .iter()
        .find(|event| event["event"] == "inspection_summary")
        .unwrap()["summary"]
}

#[test]
fn verification_reads_official_bytes_without_creating_any_metadata() {
    let fixture = Fixture::new();
    let root_before = fs::metadata(&fixture.root).unwrap().modified().unwrap();
    let (ok, events) = fixture.inspect(&fixture.inventory(), Inspection::Verify);
    assert!(ok);
    assert_eq!(summary(&events)["payload_verified"], true);
    assert_eq!(summary(&events)["clean_tree"], true);
    let file = events
        .iter()
        .find(|event| event["event"] == "inspection_file")
        .unwrap();
    assert_eq!(file["expected_size"], 3);
    assert_eq!(
        file["actual"]["actual_sha1"],
        "a9993e364706816aba3e25717850c26c9cd0d89d"
    );
    assert_eq!(fs::read(fixture.root.join("x.bin")).unwrap(), b"abc");
    assert_eq!(fs::read_dir(&fixture.root).unwrap().count(), 1);
    assert_eq!(
        fs::metadata(&fixture.root).unwrap().modified().unwrap(),
        root_before
    );
}

#[test]
fn metadata_inventory_never_claims_same_size_modified_bytes_are_verified() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("x.bin"), b"xyz").unwrap();
    let (ok, events) = fixture.inspect(&fixture.inventory(), Inspection::Inventory);
    assert!(ok);
    assert_eq!(summary(&events)["payload_verified"], false);
    let (ok, events) = fixture.inspect(&fixture.inventory(), Inspection::Verify);
    assert!(!ok);
    assert_eq!(summary(&events)["mismatched_files"], 1);
    assert_eq!(fs::read(fixture.root.join("x.bin")).unwrap(), b"xyz");
}

#[test]
fn missing_exact_manifest_never_labels_unrecognized_program_files_as_extras() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("missing-depot.dll"),
        b"official but no metadata",
    )
    .unwrap();
    let mut expected = fixture.inventory();
    expected
        .missing_manifests
        .push(fixture.base.join("7_9.manifest"));
    let (ok, events) = fixture.inspect(&expected, Inspection::Verify);
    assert!(!ok);
    assert_eq!(summary(&events)["manifest_complete"], false);
    assert_eq!(summary(&events)["extras"], 0);
    assert_eq!(summary(&events)["unclassified"], 1);
    assert!(
        events
            .iter()
            .any(|event| event["relative_path"] == "missing-depot.dll"
                && event["classification"] == "unclassified")
    );
    assert!(fixture.root.join("missing-depot.dll").is_file());
}

#[test]
fn complete_inventory_reports_unknown_data_without_removing_it() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.root.join("saves")).unwrap();
    fs::write(fixture.root.join("saves/world.dat"), b"development world").unwrap();
    let (ok, events) = fixture.inspect(&fixture.inventory(), Inspection::Verify);
    assert!(!ok);
    assert_eq!(summary(&events)["payload_verified"], true);
    assert_eq!(summary(&events)["clean_tree"], false);
    assert_eq!(summary(&events)["extras"], 2);
    assert_eq!(
        fs::read(fixture.root.join("saves/world.dat")).unwrap(),
        b"development world"
    );
}

#[test]
fn explicit_official_empty_directories_are_not_classified_as_extra_data() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.root.join("official-empty")).unwrap();
    let mut expected = fixture.inventory();
    expected.directories.insert("official-empty".into());
    let (ok, events) = fixture.inspect(&expected, Inspection::Verify);
    assert!(ok);
    assert_eq!(summary(&events)["extras"], 0);
    fs::remove_dir(fixture.root.join("official-empty")).unwrap();
    let (ok, events) = fixture.inspect(&expected, Inspection::Verify);
    assert!(!ok);
    assert_eq!(summary(&events)["missing_directories"], 1);
    assert_eq!(summary(&events)["payload_verified"], false);
}

#[test]
fn control_file_change_invalidates_the_observed_manifest_selection() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.root.join("steamapps")).unwrap();
    fs::write(fixture.root.join("steamapps/appmanifest_7.acf"), b"changed").unwrap();
    let mut expected = fixture.inventory();
    expected
        .acfs
        .insert("steamapps/appmanifest_7.acf".into(), "original".into());
    let (ok, events) = fixture.inspect(&expected, Inspection::Verify);
    assert!(!ok);
    assert!(
        !events
            .iter()
            .any(|event| event["event"] == "inspection_path")
    );
    assert_eq!(
        fs::read(fixture.root.join("steamapps/appmanifest_7.acf")).unwrap(),
        b"changed"
    );
}

#[test]
fn certification_retries_ignore_only_plain_baselines_and_never_trust_their_contents() {
    let fixture = Fixture::new();
    for name in BASELINE_METADATA {
        fs::write(fixture.root.join(name), b"not a trusted inventory").unwrap();
    }
    let verify = || {
        inspect(
            &fixture.root,
            7,
            &fixture.inventory(),
            Inspection::Verify,
            true,
            |_| {},
        )
    };
    assert!(verify().is_ok());
    fs::write(fixture.root.join("x.bin"), b"xyz").unwrap();
    assert!(verify().is_err());
    fs::write(fixture.root.join("x.bin"), b"abc").unwrap();
    fs::write(fixture.root.join("unknown.dll"), b"mod").unwrap();
    assert!(verify().is_err());
    assert_eq!(fs::read(fixture.root.join("unknown.dll")).unwrap(), b"mod");
    fs::remove_file(fixture.root.join("unknown.dll")).unwrap();
    fs::remove_file(fixture.root.join(BASELINE_METADATA[0])).unwrap();
    fs::create_dir(fixture.root.join(BASELINE_METADATA[0])).unwrap();
    assert!(verify().is_err());
}

#[test]
fn official_empty_control_directories_allow_only_the_current_app() {
    let fixture = Fixture::new();
    let mut expected = fixture.inventory();
    for directory in ["steamapps/temp/7", "steamapps/downloading/7"] {
        fs::create_dir_all(fixture.root.join(directory)).unwrap();
    }
    fs::write(
        fixture.root.join("steamapps/appmanifest_7.acf"),
        b"official",
    )
    .unwrap();
    expected
        .acfs
        .insert("steamapps/appmanifest_7.acf".into(), "official".into());
    let (ok, events) = fixture.inspect(&expected, Inspection::Verify);
    assert!(ok);
    assert_eq!(summary(&events)["clean_tree"], true);
    assert_eq!(summary(&events)["extras"], 0);
    for relative in [
        "steamapps/temp/7/unknown.bin",
        "steamapps/downloading/7/unknown.bin",
        "steamapps/temp/unknown.bin",
        "steamapps/downloading/unknown.bin",
    ] {
        let path = fixture.root.join(relative);
        fs::write(&path, b"unverified payload").unwrap();
        let (ok, events) = fixture.inspect(&expected, Inspection::Verify);
        assert!(!ok, "accepted {relative}");
        assert_eq!(summary(&events)["extras"], 1);
        assert_eq!(fs::read(&path).unwrap(), b"unverified payload");
        fs::remove_file(path).unwrap();
    }
    for relative in [
        "steamapps/temp/8",
        "steamapps/downloading/8",
        "steamapps/temp/7/unknown",
    ] {
        let path = fixture.root.join(relative);
        fs::create_dir(&path).unwrap();
        let (ok, events) = fixture.inspect(&expected, Inspection::Verify);
        assert!(!ok, "accepted {relative}");
        assert_eq!(summary(&events)["extras"], 1);
        assert!(path.is_dir());
        fs::remove_dir(path).unwrap();
    }
    let control = fixture.root.join("steamapps/temp/7");
    fs::remove_dir(&control).unwrap();
    fs::write(&control, b"a file is not a control directory").unwrap();
    assert!(!fixture.inspect(&expected, Inspection::Verify).0);
}

#[test]
fn shared_owner_control_directories_require_a_referenced_owner_acf() {
    let fixture = Fixture::new();
    let mut expected = fixture.inventory();
    for directory in ["steamapps/temp/228980", "steamapps/downloading/228980"] {
        fs::create_dir_all(fixture.root.join(directory)).unwrap();
    }
    for app_id in [7, 228980] {
        let relative = format!("steamapps/appmanifest_{app_id}.acf");
        fs::write(fixture.root.join(&relative), b"official").unwrap();
        expected.acfs.insert(relative, "official".into());
    }
    let (ok, events) = fixture.inspect(&expected, Inspection::Verify);
    assert!(ok);
    assert_eq!(summary(&events)["clean_tree"], true);
    let unknown = fixture
        .root
        .join("steamapps/downloading/228980/unknown.bin");
    fs::write(&unknown, b"unverified payload").unwrap();
    let (ok, events) = fixture.inspect(&expected, Inspection::Verify);
    assert!(!ok);
    assert_eq!(summary(&events)["extras"], 1);
    assert_eq!(fs::read(&unknown).unwrap(), b"unverified payload");
    fs::remove_file(unknown).unwrap();
    expected.acfs.remove("steamapps/appmanifest_228980.acf");
    let (ok, events) = fixture.inspect(&expected, Inspection::Verify);
    assert!(!ok);
    assert_eq!(summary(&events)["extras"], 3);
    assert_eq!(
        fs::read(fixture.root.join("steamapps/appmanifest_228980.acf")).unwrap(),
        b"official"
    );
    fs::remove_file(fixture.root.join("steamapps/appmanifest_228980.acf")).unwrap();
    let (ok, events) = fixture.inspect(&expected, Inspection::Verify);
    assert!(!ok);
    assert_eq!(summary(&events)["extras"], 2);
    assert!(fixture.root.join("steamapps/temp/228980").is_dir());
    assert!(fixture.root.join("steamapps/downloading/228980").is_dir());
}

fn overlapping_dlls(fixture: &Fixture) -> Inventory {
    let mut expected = Inventory::default();
    fs::remove_file(fixture.root.join("x.bin")).unwrap();
    for name in [
        "steamclient.dll",
        "steamclient64.dll",
        "tier0_s.dll",
        "tier0_s64.dll",
        "vstdlib_s.dll",
        "vstdlib_s64.dll",
    ] {
        let old = Entry {
            size: 3,
            sha1: "a9993e364706816aba3e25717850c26c9cd0d89d".into(),
        };
        let current = Entry {
            size: 7,
            sha1: "13a1891af75c642306a6b695377d16e4a91f0e1b".into(),
        };
        expected.files.insert(name.into(), old.clone());
        expected.overlapping_files.insert(
            name.into(),
            vec![
                DepotCandidate {
                    depot: 1004,
                    manifest: 9,
                    entry: old,
                },
                DepotCandidate {
                    depot: 2465201,
                    manifest: 10,
                    entry: current,
                },
            ],
        );
        fs::write(fixture.root.join(name), b"updated").unwrap();
    }
    expected
}

#[test]
fn six_overlapping_official_dlls_report_the_exact_matching_depot() {
    let fixture = Fixture::new();
    let expected = overlapping_dlls(&fixture);
    let (ok, events) = fixture.inspect(&expected, Inspection::Verify);
    assert!(ok);
    assert_eq!(summary(&events)["payload_verified"], true);
    assert_eq!(summary(&events)["overlapping_files"], 6);
    assert_eq!(summary(&events)["expected_bytes"], 42);
    assert!(
        events
            .iter()
            .find(|event| event["event"] == "inspection_inventory")
            .unwrap()["expected_bytes"]
            .is_null()
    );
    for file in events
        .iter()
        .filter(|event| event["event"] == "inspection_file")
    {
        assert_eq!(file["official_candidates"].as_array().unwrap().len(), 2);
        assert_eq!(file["matched_depots"].as_array().unwrap().len(), 1);
        assert_eq!(file["matched_depots"][0]["depot"], 2465201);
        assert_eq!(file["matched_depots"][0]["manifest"], 10);
        assert_eq!(
            file["expected_sha1"],
            "13a1891af75c642306a6b695377d16e4a91f0e1b"
        );
    }
    fs::write(fixture.root.join("steamclient.dll"), b"abc").unwrap();
    let (ok, events) = fixture.inspect(&expected, Inspection::Verify);
    assert!(ok);
    let file = events
        .iter()
        .find(|event| event["relative_path"] == "steamclient.dll")
        .unwrap();
    assert_eq!(file["matched_depots"][0]["depot"], 1004);
    assert_eq!(summary(&events)["expected_bytes"], 38);
}

#[test]
fn overlapping_metadata_and_missing_manifests_never_claim_verified_payload() {
    let fixture = Fixture::new();
    let mut expected = overlapping_dlls(&fixture);
    let (ok, events) = fixture.inspect(&expected, Inspection::Inventory);
    assert!(ok);
    assert_eq!(summary(&events)["payload_verified"], false);
    assert!(summary(&events)["expected_bytes"].is_null());
    for file in events
        .iter()
        .filter(|event| event["event"] == "inspection_file")
    {
        assert!(file["matched_depots"].as_array().unwrap().is_empty());
        assert!(file["expected_sha1"].is_null());
        assert!(file["actual"]["actual_sha1"].is_null());
    }
    expected
        .missing_manifests
        .push(fixture.base.join("99_100.manifest"));
    fs::write(
        fixture.root.join("unclassified.dll"),
        b"unclassified official or user file",
    )
    .unwrap();
    let (ok, events) = fixture.inspect(&expected, Inspection::Verify);
    assert!(!ok);
    assert_eq!(summary(&events)["payload_verified"], false);
    assert_eq!(summary(&events)["extras"], 0);
    assert_eq!(summary(&events)["unclassified"], 1);
    assert!(fixture.root.join("unclassified.dll").is_file());
}

#[test]
fn overlapping_depots_reject_unknown_bytes_sizes_and_directory_substitution() {
    let fixture = Fixture::new();
    let expected = overlapping_dlls(&fixture);
    let path = fixture.root.join("steamclient.dll");
    for bytes in [b"corrupt".as_slice(), b"xyz", b"unexpected length"] {
        fs::write(&path, bytes).unwrap();
        let (ok, events) = fixture.inspect(&expected, Inspection::Verify);
        assert!(!ok);
        assert_eq!(summary(&events)["mismatched_files"], 1);
        assert_eq!(summary(&events)["clean_tree"], false);
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    let (ok, events) = fixture.inspect(&expected, Inspection::Verify);
    assert!(!ok);
    assert_eq!(summary(&events)["unreadable_files"], 1);
    assert!(path.is_dir());
}
