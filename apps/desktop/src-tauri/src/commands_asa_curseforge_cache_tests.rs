use super::*;
use serde_json::json;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("langame-asa-cache-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("ModsUserData/83374")).unwrap();
        let fixture = Self { root };
        for (path, body) in [
            ("Example.uplugin", b"{\"FileVersion\":3}".as_slice()),
            (
                "Content/Paks/WindowsServer/Example.pak",
                b"pak abc".as_slice(),
            ),
            (
                "Content/Paks/WindowsServer/Example.ucas",
                b"ucas abc".as_slice(),
            ),
            (
                "Content/Paks/WindowsServer/Example.utoc",
                b"utoc abc".as_slice(),
            ),
        ] {
            let target = fixture.module().join(path);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, body).unwrap();
        }
        fixture.write_library(json!({"installedMods":[{
            "unmanaged":false,"status":"Normal","pathOnDisk":"83374/941450_8828067",
            "details":{"iD":941450,"gameId":83374,"mainFileId":8828067},
            "installedFile":{"iD":8828067,"gameId":83374,"modId":941450,"fileSizeOnDisk":40,
                "modules":[{"name":"Example","fingerprint":1349230725}]},
            "latestUpdatedFile":{"iD":8828067}
        }]}));
        fixture
    }
    fn module(&self) -> PathBuf {
        self.root.join("Mods/83374/941450_8828067/Example")
    }
    fn manifest(&self) -> PathBuf {
        self.root.join("ModsUserData/83374/library.json")
    }
    fn write_library(&self, value: Value) {
        fs::write(self.manifest(), serde_json::to_vec(&value).unwrap()).unwrap();
    }
    fn change_record(&self, change: impl FnOnce(&mut Value)) {
        let mut library: Value =
            serde_json::from_slice(&fs::read(self.manifest()).unwrap()).unwrap();
        change(&mut library["installedMods"][0]);
        self.write_library(library);
    }
    fn inspect(&self) -> Result<Evidence, String> {
        inspect(&self.root, &["941450".into()], &|| false)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(
            self.root.is_absolute() && self.root.parent() == Some(std::env::temp_dir().as_path())
        );
        assert!(
            self.root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("langame-asa-cache-")
        );
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn murmur_matches_independent_seed_one_vectors_and_stream_boundaries() {
    for (bytes, expected) in [
        (b"".as_slice(), 1540447798),
        (b"a".as_slice(), 626045324),
        (b"abc".as_slice(), 1621425345),
        (b"abcd".as_slice(), 3376380438),
        (b"hello".as_slice(), 2788266382),
        (b"Hello World".as_slice(), 1756117720),
        (b"123456789".as_slice(), 2788329418),
    ] {
        assert_eq!(murmur_bytes(bytes), expected);
        for chunk in 1..=5 {
            let mut hash =
                Murmur::new(bytes.iter().filter(|byte| !whitespace(**byte)).count() as u64);
            for part in bytes.chunks(chunk) {
                hash.update(part);
            }
            assert_eq!(hash.finish(), expected);
        }
    }
    assert_eq!(murmur_bytes(b"a b\tc\r\n"), 1621425345);
}

#[test]
fn published_module_fingerprint_produces_an_exact_sha_snapshot_without_writing_sdk_state() {
    let fixture = Fixture::new();
    let before = fs::read(fixture.manifest()).unwrap();
    let evidence = fixture.inspect().unwrap();
    assert_eq!(evidence.mods[0].file_id, 8828067);
    assert_eq!(evidence.mods[0].files.len(), 4);
    assert_eq!(evidence.mods[0].module_fingerprints["Example"], 1349230725);
    assert_eq!(evidence.mods[0].files["Example/Example.uplugin"].bytes, 17);
    assert_eq!(
        evidence.mods[0].files["Example/Example.uplugin"]
            .sha256
            .len(),
        64
    );
    verify_current(&evidence, &|| false).unwrap();
    assert_eq!(fs::read(fixture.manifest()).unwrap(), before);
}

#[test]
fn outdated_complete_cache_is_usable_without_claiming_the_latest_revision() {
    let fixture = Fixture::new();
    fixture.change_record(|record| {
        record["status"] = json!("OutOfDate");
        record["details"]["mainFileId"] = json!(9999999);
        record["latestUpdatedFile"]["iD"] = json!(9999999);
    });
    let evidence = fixture.inspect().unwrap();
    assert_eq!(evidence.mods[0].file_id, 8828067);
    fixture.change_record(|record| record["status"] = json!("Normal"));
    verify_current(&evidence, &|| false).unwrap();
}

#[test]
fn matching_published_extra_files_are_allowed_and_recorded() {
    let fixture = Fixture::new();
    fs::write(fixture.module().join("author-metadata.txt"), b"valid extra").unwrap();
    fixture.change_record(|record| {
        record["installedFile"]["modules"][0]["fingerprint"] = json!(2704613470_u32);
        record["installedFile"]["fileSizeOnDisk"] = json!(51);
    });
    let evidence = fixture.inspect().unwrap();
    assert_eq!(evidence.mods[0].files.len(), 5);
    verify_current(&evidence, &|| false).unwrap();
}

#[test]
fn sdk_ignored_dynamic_file_does_not_change_published_size_or_fingerprint_but_is_snapshotted() {
    let fixture = Fixture::new();
    fs::write(
        fixture.module().join("AssetRegistry.bin"),
        b"native generated content",
    )
    .unwrap();
    let evidence = fixture.inspect().unwrap();
    assert_eq!(evidence.mods[0].files.len(), 5);
    fs::write(
        fixture.module().join("AssetRegistry.bin"),
        b"changed dynamic content",
    )
    .unwrap();
    assert!(verify_current(&evidence, &|| false).is_err());
}

#[test]
fn same_size_binary_corruption_is_rejected_by_published_module_fingerprint() {
    let fixture = Fixture::new();
    fs::write(
        fixture
            .module()
            .join("Content/Paks/WindowsServer/Example.pak"),
        b"pak abd",
    )
    .unwrap();
    assert!(fixture.inspect().unwrap_err().contains("fingerprint"));
}

#[test]
fn sha_snapshot_rejects_same_size_changes_that_sdk_whitespace_normalization_cannot_detect() {
    let fixture = Fixture::new();
    let evidence = fixture.inspect().unwrap();
    fs::write(
        fixture
            .module()
            .join("Content/Paks/WindowsServer/Example.pak"),
        b"pak\tabc",
    )
    .unwrap();
    // This is an SDK limitation, not a second trusted baseline. The original
    // SHA snapshot is what makes the later byte mutation detectable.
    assert_eq!(murmur_bytes(b"pak abc"), murmur_bytes(b"pak\tabc"));
    assert!(
        verify_current(&evidence, &|| false)
            .unwrap_err()
            .contains("exact content changed")
    );
}

#[test]
fn missing_partial_undeclared_and_new_empty_directories_cannot_pass_a_snapshot() {
    for action in 0..4 {
        let fixture = Fixture::new();
        let evidence = fixture.inspect().unwrap();
        match action {
            0 => fs::remove_file(
                fixture
                    .module()
                    .join("Content/Paks/WindowsServer/Example.ucas"),
            )
            .unwrap(),
            1 => fs::write(
                fixture
                    .module()
                    .join("Content/Paks/WindowsServer/Example.ucas"),
                b"short",
            )
            .unwrap(),
            2 => fs::write(fixture.module().join("download.partial"), b"incomplete").unwrap(),
            _ => fs::create_dir(fixture.module().join("new-empty-directory")).unwrap(),
        }
        assert!(
            verify_current(&evidence, &|| false).is_err(),
            "action {action}"
        );
    }
}

#[test]
fn unsafe_incomplete_and_unverifiable_records_are_rejected() {
    let mutations = [
        ("status", json!("Pending")),
        ("status", json!("Invalid")),
        ("status", json!("Uninstalled")),
        ("status", json!("Modified")),
        ("unmanaged", json!(true)),
        ("pathOnDisk", json!("83374/../941450_8828067")),
        ("pathOnDisk", json!("83374/941450_9999999")),
    ];
    for (field, value) in mutations {
        let fixture = Fixture::new();
        fixture.change_record(|record| record[field] = value);
        assert!(fixture.inspect().is_err(), "{field}");
    }
    for action in 0..7 {
        let fixture = Fixture::new();
        fixture.change_record(|record| match action {
            0 => record["details"]["gameId"] = json!(1),
            1 => record["installedFile"]["gameId"] = json!(1),
            2 => record["installedFile"]["modId"] = json!(1),
            3 => record["installedFile"]["fileSizeOnDisk"] = json!(0),
            4 => record["installedFile"]["modules"] = json!([]),
            5 => record["installedFile"]["modules"][0]["fingerprint"] = json!(0),
            _ => record["installedFile"]["modules"][0]["name"] = json!("../outside"),
        });
        assert!(fixture.inspect().is_err(), "action {action}");
    }
}

#[test]
fn selected_ids_and_library_records_are_unambiguous_and_bounded() {
    let fixture = Fixture::new();
    for ids in [
        vec![],
        vec!["0941450".into()],
        vec!["941450".into(), "941450".into()],
        vec!["not-a-mod".into()],
    ] {
        assert!(inspect(&fixture.root, &ids, &|| false).is_err());
    }
    let mut library: Value =
        serde_json::from_slice(&fs::read(fixture.manifest()).unwrap()).unwrap();
    let duplicate = library["installedMods"][0].clone();
    library["installedMods"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    fixture.write_library(library);
    assert!(fixture.inspect().unwrap_err().contains("duplicate"));
    fs::write(fixture.manifest(), b"not JSON").unwrap();
    assert!(fixture.inspect().is_err());
    fs::OpenOptions::new()
        .write(true)
        .open(fixture.manifest())
        .unwrap()
        .set_len(MAX_LIBRARY_BYTES + 1)
        .unwrap();
    assert!(fixture.inspect().unwrap_err().contains("size budget"));
}

#[test]
fn sdk_utf8_bom_is_read_without_rewriting_the_library() {
    let fixture = Fixture::new();
    let mut bytes = b"\xEF\xBB\xBF".to_vec();
    bytes.extend(fs::read(fixture.manifest()).unwrap());
    fs::write(fixture.manifest(), &bytes).unwrap();
    fixture.inspect().unwrap();
    assert_eq!(fs::read(fixture.manifest()).unwrap(), bytes);
}

#[test]
fn descriptor_and_server_content_are_required_even_if_a_fingerprint_were_supplied() {
    let mut files = BTreeMap::from([(
        "Example.uplugin".into(),
        FileEvidence {
            bytes: 1,
            sha256: "a".into(),
        },
    )]);
    assert!(require_server_payload("Example", &files).is_err());
    files.insert(
        "Content/Paks/WindowsServer/Example.utoc".into(),
        FileEvidence {
            bytes: 1,
            sha256: "b".into(),
        },
    );
    assert!(require_server_payload("Example", &files).is_err());
    files.insert(
        "Content/Paks/WindowsServer/Example.ucas".into(),
        FileEvidence {
            bytes: 1,
            sha256: "c".into(),
        },
    );
    require_server_payload("Example", &files).unwrap();
    files.insert(
        "Content/Paks/WindowsServer/empty-author-note.txt".into(),
        FileEvidence {
            bytes: 0,
            sha256: "e".into(),
        },
    );
    require_server_payload("Example", &files).unwrap();
    files.insert(
        "Content/Paks/WindowsServer/Example_s1.ucas".into(),
        FileEvidence {
            bytes: 1,
            sha256: "d".into(),
        },
    );
    require_server_payload("Example", &files).unwrap();
    files.remove("Example.uplugin");
    assert!(require_server_payload("Example", &files).is_err());
}

#[test]
fn cancellation_and_deadline_are_bounded_and_do_not_change_sdk_records() {
    let fixture = Fixture::new();
    let before = fs::read(fixture.manifest()).unwrap();
    assert!(
        inspect(&fixture.root, &["941450".into()], &|| true)
            .unwrap_err()
            .contains("cancelled")
    );
    let budget = Budget {
        entries: 0,
        bytes: 0,
        deadline: Instant::now(),
        cancelled: &|| false,
    };
    assert!(budget.check().unwrap_err().contains("time budget"));
    assert_eq!(fs::read(fixture.manifest()).unwrap(), before);
}

#[test]
fn cache_walk_enforces_entry_and_byte_budgets_without_allocating_large_content() {
    let fixture = Fixture::new();
    for (entries, bytes) in [(MAX_ENTRIES, 0), (0, MAX_CONTENT_BYTES)] {
        let mut budget = Budget {
            entries,
            bytes,
            deadline: Instant::now() + Duration::from_secs(30),
            cancelled: &|| false,
        };
        let result = read_tree(
            &fixture.module(),
            &mut BTreeMap::new(),
            &mut BTreeSet::new(),
            &mut Vec::new(),
            &mut 0,
            &mut budget,
        );
        assert!(result.unwrap_err().contains("budget"));
    }
}

#[test]
fn directory_junction_or_symlink_cannot_supply_cache_content() {
    let fixture = Fixture::new();
    fixture
        .inspect()
        .expect("ordinary cache must be valid before adding the link");
    let outside = fixture.root.join("outside");
    fs::create_dir(&outside).unwrap();
    let link = fixture.module().join("linked");
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use std::os::windows::process::CommandExt;
        let quote_path = |path: &Path| {
            let path = path
                .to_str()
                .expect("fixture path must be Unicode")
                .replace('/', "\\");
            assert!(
                !path.contains('"'),
                "fixture path cannot contain a CMD quote"
            );
            format!("\"{path}\"")
        };
        let output = std::process::Command::new("cmd.exe")
            .args(["/D", "/V:OFF", "/C"])
            .raw_arg("mklink /J")
            .raw_arg(quote_path(&link))
            .raw_arg(quote_path(&outside))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_ne!(
            fs::symlink_metadata(&link).unwrap().file_attributes() & 0x400,
            0,
            "the fixture must create a real junction"
        );
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    let error = fixture
        .inspect()
        .expect_err("linked content must be rejected");
    assert!(
        error.contains("reparse") || error.contains("symbolic link"),
        "{error}"
    );
    #[cfg(windows)]
    fs::remove_dir(&link).unwrap();
    #[cfg(unix)]
    fs::remove_file(&link).unwrap();
}

/// Opt-in real-payload validation. The caller supplies an isolated, sanitized
/// copy containing only Mods and ModsUserData; no live native context is used.
#[test]
#[ignore = "requires an explicitly supplied sanitized ASA cache copy"]
fn sanitized_pamd_native_cache_matches_published_fingerprints_and_exact_content() {
    let root = PathBuf::from(
        std::env::var_os("LGSM_ASA_CACHE_ROOT")
            .expect("LGSM_ASA_CACHE_ROOT must name the sanitized cache copy"),
    );
    let evidence =
        inspect(&root, &["1715971".into(), "941450".into()], &|| false).unwrap_or_else(|_| {
            panic!("sanitized native cache must match its published module fingerprints")
        });
    let expected = [
        (
            "1715971",
            9067441,
            "DarkPuddlesStructures",
            3145213246_u32,
            2671615_u64,
        ),
        (
            "941450",
            8828067,
            "AwesomeAdminTools",
            1779172362_u32,
            2833017_u64,
        ),
    ];
    for (project_id, file_id, module, fingerprint, size) in expected {
        let item = evidence
            .mods
            .iter()
            .find(|item| item.project_id == project_id)
            .expect("required PAMD Mod evidence is missing");
        assert_eq!(item.file_id, file_id);
        assert_eq!(item.module_fingerprints.get(module), Some(&fingerprint));
        assert_eq!(item.files.len(), 6);
        assert_eq!(
            item.files.values().map(|file| file.bytes).sum::<u64>(),
            size
        );
    }
    verify_current(&evidence, &|| false)
        .unwrap_or_else(|_| panic!("the cache must retain its exact verified content"));
    let projection = evidence.mods.iter().map(|item| json!({
        "project_id": item.project_id,
        "installed_file_id": item.file_id,
        "files": item.files.len(),
        "bytes": item.files.values().map(|file| file.bytes).sum::<u64>(),
        "module_fingerprints": item.module_fingerprints.values().copied().collect::<Vec<_>>(),
        "verified": true
    })).collect::<Vec<_>>();
    println!(
        "ASA_SANITIZED_CACHE {}",
        serde_json::to_string(&projection).unwrap()
    );
}
