use super::*;
use crate::commands::commands_mods::mod_inventory::read_inventory;

struct TestRoot(PathBuf);
impl TestRoot {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("langame-archive-formats-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/test_fixtures/mod_archives")
        .join(name)
}

fn extract(path: &Path, root: &Path) -> Result<DirectoryCopyStats, String> {
    fs::create_dir_all(root.join("staging")).unwrap();
    fs::create_dir_all(root.join("partials")).unwrap();
    extract_additional_mod_archive_to_directory(
        path,
        &root.join("staging"),
        &root.join("partials"),
        &mut ManualModStagingRegistry::default(),
        ManualModArchiveLayout::Preserve,
    )
}

fn seven_z(path: &Path, entries: &[(&str, &[u8])]) {
    let mut writer = sevenz_rust2::ArchiveWriter::new(fs::File::create(path).unwrap()).unwrap();
    for (name, bytes) in entries {
        writer
            .push_archive_entry(sevenz_rust2::ArchiveEntry::new_file(name), Some(*bytes))
            .unwrap();
    }
    writer.finish().unwrap();
}

// A valid stored RAR4 container, used only to create exact tiny boundary cases.
// Decoder interoperability is independently covered by upstream RAR4/RAR5 blobs.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

fn rar_header(out: &mut Vec<u8>, kind: u8, flags: u16, body: &[u8]) {
    let mut header = vec![kind];
    header.extend_from_slice(&flags.to_le_bytes());
    header.extend_from_slice(&u16::try_from(body.len() + 7).unwrap().to_le_bytes());
    header.extend_from_slice(body);
    out.extend_from_slice(&(crc32(&header) as u16).to_le_bytes());
    out.extend(header);
}

fn stored_rar(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = b"Rar!\x1a\x07\0".to_vec();
    rar_header(&mut out, 0x73, 0, &[0; 6]);
    for (name, data) in entries {
        let mut body = Vec::new();
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.push(3); // RAR4 Unix host; mode below is explicitly regular.
        body.extend_from_slice(&crc32(data).to_le_bytes());
        body.extend_from_slice(&0u32.to_le_bytes());
        body.extend_from_slice(&[20, 0x30]);
        body.extend_from_slice(&(name.len() as u16).to_le_bytes());
        body.extend_from_slice(&0o100_644u32.to_le_bytes());
        body.extend_from_slice(name.as_bytes());
        rar_header(&mut out, 0x74, 0x8000, &body);
        out.extend_from_slice(data);
    }
    rar_header(&mut out, 0x7b, 0, &[]);
    out
}

pub(in crate::commands) fn archive_fixture_bytes(extension: &str) -> Vec<u8> {
    let entries = [(
        "CameraTweaks/CameraTweaks.dll",
        b"test server mod bytes".as_slice(),
    )];
    if extension == "rar" {
        return stored_rar(&entries);
    }
    assert_eq!(extension, "7z");
    let mut writer = sevenz_rust2::ArchiveWriter::new(io::Cursor::new(Vec::new())).unwrap();
    for (name, bytes) in entries {
        writer
            .push_archive_entry(sevenz_rust2::ArchiveEntry::new_file(name), Some(bytes))
            .unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn target(root: &Path) -> ResolvedManualModTarget {
    ResolvedManualModTarget {
        instance_id: "archive-format-test".into(),
        module_id: "enshrouded".into(),
        source_label: "Nexus Mods".into(),
        target_label: "mods".into(),
        target_path: root.join("mods"),
        accepts: vec!["folder".into(), "zip".into(), "dll".into()],
        id_strategy: None,
    }
}

#[test]
fn mod_archive_formats_decode_upstream_7z_lzma_solid_and_ppmd() {
    for (name, file_count, bytes) in [
        ("lzma.7z", 2, 34),
        ("solid.7z", 2, 6101),
        ("ppmd.7z", 1, 11356),
    ] {
        let root = TestRoot::new();
        assert_eq!(
            inspect_additional_mod_archive(&fixture(name)).unwrap(),
            Some(AdditionalModArchiveFormat::SevenZ)
        );
        let result = extract(&fixture(name), &root.0).unwrap();
        assert_eq!(result.file_count, file_count, "{name}");
        assert_eq!(result.total_bytes, bytes, "{name}");
        assert_eq!(fs::read_dir(root.0.join("partials")).unwrap().count(), 0);
        if name == "lzma.7z" {
            assert_eq!(
                fs::read(root.0.join("staging/file1.txt")).unwrap().len(),
                17
            );
            assert_eq!(
                fs::read(root.0.join("staging/file2.txt")).unwrap().len(),
                17
            );
        }
    }
}

#[test]
fn mod_archive_formats_decode_upstream_rar4_rar5_and_unicode() {
    for (name, expected, contents) in [
        ("rar4.rar", "VERSION", b"unrar-0.4.0".as_slice()),
        (
            "rar5-unicode.rar",
            "unicodefilename❤️.txt",
            b"foobar\n".as_slice(),
        ),
        (
            "rar5-solid.rar",
            ".gitignore",
            b"target\nCargo.lock\n".as_slice(),
        ),
    ] {
        let root = TestRoot::new();
        assert_eq!(
            inspect_additional_mod_archive(&fixture(name)).unwrap(),
            Some(AdditionalModArchiveFormat::Rar)
        );
        let result = extract(&fixture(name), &root.0).unwrap();
        assert_eq!(result.file_count, 1);
        assert_eq!(
            fs::read(root.0.join("staging").join(expected)).unwrap(),
            contents,
            "{name}"
        );
        assert_eq!(fs::read_dir(root.0.join("partials")).unwrap().count(), 0);
    }
}

#[test]
fn mod_archive_formats_stage_both_formats_with_presentation_and_old_configuration() {
    for extension in ["rar", "7z"] {
        let root = TestRoot::new();
        let source = root.0.join(format!("package.{extension}"));
        let entries = [(
            "CameraTweaks/CameraTweaks.dll",
            b"test server mod bytes".as_slice(),
        )];
        if extension == "rar" {
            fs::write(&source, stored_rar(&entries)).unwrap();
        } else {
            seven_z(&source, &entries);
        }
        fs::create_dir(root.0.join("mods")).unwrap();
        fs::write(root.0.join("mods/operator.cfg"), b"keep my settings").unwrap();
        let icon = "https://staticdelivery.nexusmods.com/mods/5811/images/thumbnails/315/315-1790000000-123456789.png";
        let result = stage_website_mod_source(
            target(&root.0),
            source,
            ManualModImportPresentation {
                display_name: Some("Camera Tweaks".into()),
                icon_url: Some(icon.into()),
            },
            None,
        )
        .unwrap();
        assert_eq!(result.copied_file_count, 1, "{extension}");
        assert_eq!(
            fs::read(root.0.join("mods/CameraTweaks/CameraTweaks.dll")).unwrap(),
            entries[0].1
        );
        assert_eq!(
            fs::read(root.0.join("mods/operator.cfg")).unwrap(),
            b"keep my settings"
        );
        let inventory = read_inventory(target(&root.0)).unwrap();
        let installed = inventory
            .items
            .iter()
            .find(|item| item.display_name.as_deref() == Some("Camera Tweaks"))
            .unwrap();
        assert_eq!(installed.icon_url.as_deref(), Some(icon));
        assert!(
            !fs::read_dir(&root.0)
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".langame-mod-transaction-"))
        );
    }
}

#[test]
fn mod_archive_formats_content_identification_ignores_the_suggested_extension() {
    let root = TestRoot::new();
    for (fixture_name, expected) in [
        ("rar4.rar", AdditionalModArchiveFormat::Rar),
        ("solid.7z", AdditionalModArchiveFormat::SevenZ),
    ] {
        let path = root.0.join("Friendly mod v1.0");
        fs::copy(fixture(fixture_name), &path).unwrap();
        assert_eq!(
            inspect_additional_mod_archive(&path).unwrap(),
            Some(expected)
        );
        assert!(matches!(expected.extension(), "rar" | "7z"));
    }
    for data in [
        b"MZ not an archive".as_slice(),
        b"<html>sign in</html>",
        b"PK\x05\x06",
        b"",
    ] {
        let path = root.0.join("misleading.rar");
        fs::write(&path, data).unwrap();
        assert_eq!(inspect_additional_mod_archive(&path).unwrap(), None);
    }
}

#[test]
fn mod_archive_formats_reject_unsafe_paths_before_writing_any_entry() {
    for extension in ["rar", "7z"] {
        for bad in [
            "../outside.dll",
            "dir/../../outside.dll",
            "C:/outside.dll",
            "\\\\server\\share\\x",
            "safe.dll:stream",
            "CON.dll",
            "trailing. ",
        ] {
            let root = TestRoot::new();
            let source = root.0.join(format!("unsafe.{extension}"));
            let entries = [
                ("Good/plugin.dll", b"safe".as_slice()),
                (bad, b"bad".as_slice()),
            ];
            if extension == "rar" {
                fs::write(&source, stored_rar(&entries)).unwrap();
            } else {
                seven_z(&source, &entries);
            }
            assert!(extract(&source, &root.0).is_err(), "{extension}: {bad}");
            assert_eq!(
                fs::read_dir(root.0.join("staging")).unwrap().count(),
                0,
                "{extension}: {bad}"
            );
            assert_eq!(fs::read_dir(root.0.join("partials")).unwrap().count(), 0);
        }
    }
}

#[test]
fn mod_archive_formats_reject_aliases_and_file_directory_conflicts() {
    for names in [
        ["Mod/plugin.dll", "mod/PLUGIN.dll"],
        ["Mod", "Mod/plugin.dll"],
    ] {
        for extension in ["rar", "7z"] {
            let root = TestRoot::new();
            let path = root.0.join(format!("duplicate.{extension}"));
            let entries = [(names[0], b"a".as_slice()), (names[1], b"b".as_slice())];
            if extension == "rar" {
                fs::write(&path, stored_rar(&entries)).unwrap();
            } else {
                seven_z(&path, &entries);
            }
            assert!(extract(&path, &root.0).is_err());
            assert_eq!(fs::read_dir(root.0.join("staging")).unwrap().count(), 0);
        }
    }
}

#[test]
fn mod_archive_formats_reject_links_and_7z_deletion_markers() {
    for (mode, anti, directory) in [
        (0o120_777u32 << 16, false, false),
        (0x400, false, false),
        (0, true, false),
        (0o120_777 << 16, false, true),
    ] {
        let root = TestRoot::new();
        let path = root.0.join("link.7z");
        let mut writer =
            sevenz_rust2::ArchiveWriter::new(fs::File::create(&path).unwrap()).unwrap();
        let mut entry = if directory {
            sevenz_rust2::ArchiveEntry::new_directory("link")
        } else {
            sevenz_rust2::ArchiveEntry::new_file("link")
        };
        entry.has_windows_attributes = true;
        entry.windows_attributes = mode;
        entry.is_anti_item = anti;
        if anti {
            entry.has_stream = false;
        }
        writer.push_archive_entry(entry, None::<&[u8]>).unwrap();
        writer.finish().unwrap();
        assert!(
            extract(&path, &root.0).is_err(),
            "mode {mode:x}, anti {anti}, directory {directory}"
        );
        assert_eq!(fs::read_dir(root.0.join("staging")).unwrap().count(), 0);
    }
    let root = TestRoot::new();
    let path = root.0.join("link.rar");
    let mut bytes = stored_rar(&[("link", b"../outside")]);
    let file_header = 20usize; // signature + fixed RAR4 main header
    bytes[file_header + 28..file_header + 32].copy_from_slice(&0o120_777u32.to_le_bytes());
    let size = u16::from_le_bytes([bytes[file_header + 5], bytes[file_header + 6]]) as usize;
    let checksum = crc32(&bytes[file_header + 2..file_header + size]) as u16;
    bytes[file_header..file_header + 2].copy_from_slice(&checksum.to_le_bytes());
    fs::write(&path, bytes).unwrap();
    assert!(extract(&path, &root.0).is_err());
    assert_eq!(fs::read_dir(root.0.join("staging")).unwrap().count(), 0);
}

#[test]
fn mod_archive_formats_reject_encryption_and_truncated_containers() {
    for name in ["encrypted.7z", "encrypted.rar"] {
        let root = TestRoot::new();
        assert!(extract(&fixture(name), &root.0).is_err(), "{name}");
        assert_eq!(fs::read_dir(root.0.join("staging")).unwrap().count(), 0);
    }
    for name in ["lzma.7z", "rar4.rar", "rar5-solid.rar"] {
        let root = TestRoot::new();
        let bytes = fs::read(fixture(name)).unwrap();
        let path = root.0.join("truncated");
        fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();
        assert!(inspect_additional_mod_archive(&path).is_err(), "{name}");
    }
}

#[test]
fn mod_archive_formats_corrupt_payload_rolls_back_without_replacing_old_data() {
    let root = TestRoot::new();
    let path = root.0.join("bad.rar");
    let mut bytes = stored_rar(&[("NewMod/plugin.dll", b"corrupt-me")]);
    let offset = bytes
        .windows(10)
        .position(|bytes| bytes == b"corrupt-me")
        .unwrap();
    bytes[offset] ^= 1;
    fs::write(&path, bytes).unwrap();
    fs::create_dir(root.0.join("mods")).unwrap();
    fs::write(root.0.join("mods/operator.cfg"), b"untouched").unwrap();
    assert!(
        stage_website_mod_source(
            target(&root.0),
            path,
            ManualModImportPresentation {
                display_name: None,
                icon_url: None
            },
            None,
        )
        .is_err()
    );
    assert_eq!(
        fs::read(root.0.join("mods/operator.cfg")).unwrap(),
        b"untouched"
    );
    assert!(!root.0.join("mods/NewMod").exists());
    assert!(
        !fs::read_dir(&root.0)
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with(".langame-mod-transaction-"))
    );
}

#[test]
fn mod_archive_formats_metadata_limits_precede_rar_directory_allocation() {
    let root = TestRoot::new();
    let path = root.0.join("too-many.rar");
    let mut bytes = b"Rar!\x1a\x07\0".to_vec();
    for _ in 0..=MANUAL_MOD_ARCHIVE_MAX_ENTRY_COUNT {
        rar_header(&mut bytes, 0x74, 0, &[]);
    }
    fs::write(&path, bytes).unwrap();
    let error = inspect_additional_mod_archive(&path).unwrap_err();
    assert!(error.contains("too many entries"), "{error}");
    let mut bytes = [0; 32];
    bytes[..6].copy_from_slice(b"7z\xbc\xaf\x27\x1c");
    bytes[20..28]
        .copy_from_slice(&(MANUAL_MOD_ARCHIVE_MAX_CENTRAL_DIRECTORY_BYTES + 1).to_le_bytes());
    fs::write(&path, bytes).unwrap();
    assert!(
        inspect_additional_mod_archive(&path)
            .unwrap_err()
            .contains("metadata budget")
    );
}

#[test]
fn mod_archive_formats_bounded_writer_rejects_excess_and_discards_incomplete_partial() {
    let root = TestRoot::new();
    {
        let mut writer = StagedArchiveWriter::new(&root.0, 3).unwrap();
        assert!(writer.write_all(b"four").is_err());
    }
    assert_eq!(fs::read_dir(&root.0).unwrap().count(), 0);
    let mut writer = StagedArchiveWriter::new(&root.0, 3).unwrap();
    writer.write_all(b"ab").unwrap();
    assert!(writer.finish(&root.0.join("payload.dll")).is_err());
    assert_eq!(fs::read_dir(&root.0).unwrap().count(), 0);
}

#[test]
fn mod_archive_formats_rejects_inconsistent_stored_rar_lengths_before_decoding() {
    let root = TestRoot::new();
    let path = root.0.join("bomb.rar");
    let mut bytes = stored_rar(&[("bomb.dll", b"x")]);
    let file_header = 20usize;
    bytes[file_header + 11..file_header + 15].copy_from_slice(&100_000u32.to_le_bytes());
    let size = u16::from_le_bytes([bytes[file_header + 5], bytes[file_header + 6]]) as usize;
    let checksum = crc32(&bytes[file_header + 2..file_header + size]) as u16;
    bytes[file_header..file_header + 2].copy_from_slice(&checksum.to_le_bytes());
    fs::write(&path, bytes).unwrap();
    let error = extract(&path, &root.0).unwrap_err();
    assert!(
        error.contains("Stored entry has inconsistent lengths"),
        "{error}"
    );
    assert!(
        error.contains("compressed=1 bytes, uncompressed=100000 bytes"),
        "{error}"
    );
    assert_eq!(fs::read_dir(root.0.join("staging")).unwrap().count(), 0);
}

#[test]
fn mod_archive_formats_decode_bounded_high_compression_data() {
    let root = TestRoot::new();
    let path = root.0.join("map.7z");
    let data = vec![0; 1024 * 1024];
    seven_z(&path, &[("RWGIslands/Prefabs/bridge.tts", &data)]);
    let (_, reader) = open_archive(&path).unwrap().unwrap();
    if let ArchiveReader::SevenZ { archive, .. } = &reader {
        assert!(
            data.len() as u64
                > archive.pack_sizes().iter().sum::<u64>()
                    * MANUAL_MOD_ARCHIVE_MAX_COMPRESSION_RATIO
        );
    } else {
        panic!("expected the 7z reader");
    }
    drop(reader);
    let stats = extract(&path, &root.0).unwrap();
    assert_eq!(stats.total_bytes, data.len() as u64);
    assert_eq!(
        fs::read(root.0.join("staging/RWGIslands/Prefabs/bridge.tts")).unwrap(),
        data
    );
}

#[test]
fn mod_archive_formats_share_high_compression_budget_across_7z_blocks() {
    let root = TestRoot::new();
    let path = root.0.join("blocks.7z");
    let mut writer = sevenz_rust2::ArchiveWriter::new(fs::File::create(&path).unwrap()).unwrap();
    // The resource boundary needs large declared output, not an expensive encoder.
    let mut compression = sevenz_rust2::encoder_options::Lzma2Options::from_level(0);
    compression.set_dictionary_size(4096);
    writer.set_content_methods(vec![compression.into()]);
    for index in 0..3 {
        writer
            .push_archive_entry(
                sevenz_rust2::ArchiveEntry::new_file(&format!("bridge-{index}.tts")),
                Some(io::repeat(0).take(96 * 1024 * 1024)),
            )
            .unwrap();
    }
    writer.finish().unwrap();
    let (_, reader) = open_archive(&path).unwrap().unwrap();
    if let ArchiveReader::SevenZ { archive, .. } = &reader {
        assert_eq!(
            archive.blocks.len(),
            3,
            "each entry must have its own compression block"
        );
        for packed in archive.pack_sizes() {
            validate_manual_mod_archive_metrics(1, *packed, 96 * 1024 * 1024).unwrap();
        }
    } else {
        panic!("expected the 7z reader");
    }
    drop(reader);
    let error = extract(&path, &root.0).unwrap_err();
    assert!(error.contains("compression ratio"), "{error}");
    assert!(error.contains("uncompressed=301989888 bytes"), "{error}");
    assert_eq!(fs::read_dir(root.0.join("staging")).unwrap().count(), 0);
    assert_eq!(fs::read_dir(root.0.join("partials")).unwrap().count(), 0);
}
