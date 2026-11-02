use super::super::super::tests::Fixture;
use super::*;

fn fixture() -> Fixture {
    let fixture = Fixture::new();
    fs::write(
        fixture.bin().join(ASA.executable),
        b"ASA executable version one",
    )
    .unwrap();
    fixture
}

fn map(key: &[u8], value_size: usize) -> Vec<u8> {
    let mut bytes = (key.len() as u64).to_le_bytes().to_vec();
    bytes.extend_from_slice(key);
    bytes.extend(vec![0; value_size]);
    bytes
}

fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn stage(prepared: &mut Prepared) {
    fs::write(
        prepared.archive_path().unwrap(),
        archive(&[
            ("cached_offsets.cache", &map(b"Function", 8)),
            ("cached_bitfields.cache", &map(b"BitField", 32)),
            (
                "cached_key.cache",
                b"archive metadata must never select the executable",
            ),
        ]),
    )
    .unwrap();
    prepared.extract().unwrap();
}

#[test]
fn asa_symbols_enforce_upstream_executable_path_and_combined_cache_bounds() {
    assert!(executable_path_supported(Path::new(&"a".repeat(259))).is_ok());
    assert!(executable_path_supported(Path::new(&"a".repeat(260))).is_err());
    assert!(executable_path_supported(Path::new(&"𠀀".repeat(130))).is_err());
    assert!(cache_total_supported([MAX_MAP, MAX_ARCHIVE - MAX_MAP]));
    assert!(!cache_total_supported([MAX_MAP, MAX_ARCHIVE - MAX_MAP + 1]));
    assert!(!cache_total_supported([MAX_MAP, MAX_MAP]));
}

#[test]
fn asa_symbol_metadata_requires_executable_hash_and_safe_generation() {
    let hash = "a".repeat(64);
    assert_eq!(
        metadata(format!("  {}\r\n", hash.to_uppercase()).as_bytes())
            .unwrap()
            .executable_hash,
        hash
    );
    for directory in [
        "",
        &format!("generations/{hash}-123-456-789"),
        &format!("generations\\{hash}-1-2-3"),
    ] {
        let bytes = serde_json::to_vec(
            &serde_json::json!({"version":1,"executable_hash":hash,"cache_directory":directory}),
        )
        .unwrap();
        assert!(metadata(&bytes).is_some());
    }
    for directory in [
        "../escape",
        "C:/external",
        "generations/../escape",
        &format!("generations/{hash}-1-2-../escape"),
        &format!("generations/{hash}-1-2-3/child"),
    ] {
        let bytes = serde_json::to_vec(
            &serde_json::json!({"version":1,"executable_hash":hash,"cache_directory":directory}),
        )
        .unwrap();
        assert!(metadata(&bytes).is_none(), "{directory}");
    }
    assert!(metadata(br#"{"version":2,"executable_hash":"invalid"}"#).is_none());
}

#[test]
fn asa_symbol_maps_reject_empty_truncated_duplicate_and_wrong_layout() {
    let fixture = fixture();
    let path = fixture.bin().join("map-test.cache");
    for size in [8, 32] {
        let valid = map(b"Unique", size);
        fs::write(&path, &valid).unwrap();
        validate_map(&mut File::open(&path).unwrap(), size).unwrap();
        for invalid in [
            Vec::new(),
            valid[..valid.len() - 1].to_vec(),
            [valid.clone(), valid.clone()].concat(),
            vec![0; 8],
        ] {
            fs::write(&path, invalid).unwrap();
            assert!(validate_map(&mut File::open(&path).unwrap(), size).is_err());
        }
    }
    fs::write(&path, map(b"WrongValueWidth", 8)).unwrap();
    assert!(validate_map(&mut File::open(&path).unwrap(), 32).is_err());
    fixture.finish();
}

#[test]
fn asa_symbols_publish_current_executable_generation_and_preserve_previous_cache() {
    let fixture = fixture();
    let cache = fixture.bin().join("ArkApi/Cache");
    fs::create_dir_all(&cache).unwrap();
    let before = "b".repeat(64).into_bytes();
    fs::write(cache.join("cached_key.cache"), &before).unwrap();
    fs::write(
        cache.join("cached_offsets.cache"),
        b"previous offsets retained",
    )
    .unwrap();
    let mut prepared = Prepared::inspect(&fixture.0).unwrap();
    assert!(!prepared.ready());
    let expected_hash = prepared.executable_hash.clone();
    stage(&mut prepared);
    assert_eq!(fs::read(cache.join("cached_key.cache")).unwrap(), before);
    prepared.publish().unwrap();
    let selected = metadata(&fs::read(cache.join("cached_key.cache")).unwrap()).unwrap();
    assert_eq!(selected.executable_hash, expected_hash);
    assert!(generation(&selected.cache_directory));
    assert_eq!(
        fs::read(cache.join("cached_offsets.cache")).unwrap(),
        b"previous offsets retained"
    );
    let ready = Prepared::inspect(&fixture.0).unwrap();
    assert!(ready.ready());
    assert!(ready.staging.is_none());
    ready.publish().unwrap();
    assert!(!fs::read_dir(fixture.bin()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".langame-asa-symbols-")
    }));
    fixture.finish();
}

#[test]
fn asa_symbols_invalid_archive_never_replaces_existing_selection() {
    for entries in [
        vec![("cached_offsets.cache", map(b"Function", 8))],
        vec![
            ("cached_offsets.cache", map(b"Function", 8)),
            ("../escape", map(b"Bit", 32)),
        ],
        vec![
            ("cached_offsets.cache", map(b"Function", 8)),
            ("cached_bitfields.cache", Vec::new()),
        ],
        vec![
            ("cached_offsets.cache", map(b"Function", 8)),
            ("cached_bitfields.cache", map(b"Bit", 8)),
        ],
    ] {
        let fixture = fixture();
        let cache = fixture.bin().join("ArkApi/Cache");
        fs::create_dir_all(&cache).unwrap();
        let original = b"existing selection retained";
        fs::write(cache.join("cached_key.cache"), original).unwrap();
        let mut prepared = Prepared::inspect(&fixture.0).unwrap();
        let references: Vec<_> = entries
            .iter()
            .map(|(name, bytes)| (*name, bytes.as_slice()))
            .collect();
        fs::write(prepared.archive_path().unwrap(), archive(&references)).unwrap();
        assert!(prepared.extract().is_err());
        drop(prepared);
        assert_eq!(fs::read(cache.join("cached_key.cache")).unwrap(), original);
        fixture.finish();
    }
}

#[test]
fn asa_symbols_publication_preserves_concurrently_changed_selection() {
    let fixture = fixture();
    let mut prepared = Prepared::inspect(&fixture.0).unwrap();
    stage(&mut prepared);
    let cache = fixture.bin().join("ArkApi/Cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("cached_key.cache"), b"concurrent selection").unwrap();
    assert!(
        prepared
            .publish()
            .unwrap_err()
            .contains("changed concurrently")
    );
    assert_eq!(
        fs::read(cache.join("cached_key.cache")).unwrap(),
        b"concurrent selection"
    );
    assert_eq!(fs::read_dir(cache.join("generations")).unwrap().count(), 0);
    fixture.finish();
}

#[test]
fn asa_symbols_executable_change_requires_a_new_matching_cache() {
    let fixture = fixture();
    let mut prepared = Prepared::inspect(&fixture.0).unwrap();
    stage(&mut prepared);
    prepared.publish().unwrap();
    fs::write(
        fixture.bin().join(ASA.executable),
        b"ASA executable version two",
    )
    .unwrap();
    let changed = Prepared::inspect(&fixture.0).unwrap();
    assert!(!changed.ready());
    drop(changed);
    fixture.finish();
}
