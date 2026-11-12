use super::*;
use std::io::Cursor;
use std::path::PathBuf;

// Exact rows from the official ASA build 26564816 manifest (SHA-256
// 0657750b175b6c87b1acca101eb39a9b6780d9c4ac35fae978dfb50b74e21655).
const SPIDER_ROW: &str = "ShooterGame/Content/PrimalEarth/Dinos/Spider-Small/SpiderS_Character_BP_Aberrant.uasset\t1601-01-01T00:00:00.000Z\n";
const MEGALANIA_ROW: &str = "ShooterGame/Content/PrimalEarth/Dinos/Megalania/Megalania_Character_BP_Aberrant.uasset\t1601-01-01T00:00:00.000Z\n";
const SPIDER_PATH: &str = "/Game/PrimalEarth/Dinos/Spider-Small/SpiderS_Character_BP_Aberrant.SpiderS_Character_BP_Aberrant_C";
const MEGALANIA_PATH: &str = "/Game/PrimalEarth/Dinos/Megalania/Megalania_Character_BP_Aberrant.Megalania_Character_BP_Aberrant_C";

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("lgsm-ark-paths-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).expect("create owned test directory");
        Self(path)
    }

    fn write(&self, bytes: impl AsRef<[u8]>) {
        fs::write(self.0.join(MANIFEST), bytes).expect("write test manifest");
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn official_unloaded_spider_and_megalania_resolve_to_complete_paths() {
    let root = Directory::new();
    root.write(format!("\u{feff}{SPIDER_ROW}{MEGALANIA_ROW}").replace('\n', "\r\n"));
    assert_eq!(
        resolve(&root.0, "SpiderS_Character_BP_Aberrant_C").unwrap(),
        SPIDER_PATH
    );
    assert_eq!(
        resolve(&root.0, "Megalania_Character_BP_Aberrant_C").unwrap(),
        MEGALANIA_PATH
    );
}

#[test]
fn resolution_preserves_the_asset_spelling_and_deduplicates_identical_rows() {
    let row = "ShooterGame/Content/PrimalEarth/Dinos/Megalodon/MEgaMegalodon_Character_BP.uasset\t1601-01-01T00:00:00.000Z\n";
    let (resolved, bytes) = scan(
        Cursor::new(format!("{row}{row}")),
        "MegaMegalodon_Character_BP",
        MAX_MANIFEST_BYTES,
    )
    .unwrap();
    assert_eq!(bytes, (row.len() * 2) as u64);
    assert_eq!(
        resolved.as_deref(),
        Some(
            "/Game/PrimalEarth/Dinos/Megalodon/MEgaMegalodon_Character_BP.MEgaMegalodon_Character_BP_C"
        )
    );
}

#[test]
fn missing_manifests_unknown_mods_and_explicit_paths_keep_existing_behavior() {
    let root = Directory::new();
    assert_eq!(resolve(&root.0, "MyModDino_C").unwrap(), "MyModDino_C");
    root.write(format!("{SPIDER_ROW}{MEGALANIA_ROW}"));
    assert_eq!(resolve(&root.0, "MyModDino_C").unwrap(), "MyModDino_C");
    root.write(b"corrupt manifest");
    let explicit = "/Game/Mods/MyMod/MyModDino.MyModDino_C";
    assert_eq!(resolve(&root.0, explicit).unwrap(), explicit);
}

#[test]
fn ambiguous_classes_and_malformed_trailing_rows_never_return_a_partial_match() {
    let root = Directory::new();
    root.write(format!("{SPIDER_ROW}ShooterGame/Content/Other/SpiderS_Character_BP_Aberrant.uasset\t1601-01-01T00:00:00.000Z\n"));
    assert!(resolve(&root.0, "SpiderS_Character_BP_Aberrant_C").is_err());
    root.write(format!("{SPIDER_ROW}invalid trailing row\n"));
    assert!(resolve(&root.0, "SpiderS_Character_BP_Aberrant_C").is_err());
    root.write([SPIDER_ROW.as_bytes(), &[0xff, b'\n']].concat());
    assert!(resolve(&root.0, "SpiderS_Character_BP_Aberrant_C").is_err());
}

#[test]
fn only_content_assets_match_and_unsafe_manifest_paths_are_refused() {
    let root = Directory::new();
    root.write("Engine/Content/Megalania_Character_BP_Aberrant.uasset\tdate\nShooterGame/Content/Megalania_Character_BP_Aberrant.uexp\tdate\n");
    assert_eq!(
        resolve(&root.0, "Megalania_Character_BP_Aberrant_C").unwrap(),
        "Megalania_Character_BP_Aberrant_C"
    );
    for path in [
        "/ShooterGame/Content/Megalania_Character_BP_Aberrant.uasset",
        "C:/ShooterGame/Content/Megalania_Character_BP_Aberrant.uasset",
        "ShooterGame\\Content\\Megalania_Character_BP_Aberrant.uasset",
        "ShooterGame/Content/../Megalania_Character_BP_Aberrant.uasset",
        "ShooterGame/Content//Megalania_Character_BP_Aberrant.uasset",
        "ShooterGame/Content/bad;command/Megalania_Character_BP_Aberrant.uasset",
    ] {
        root.write(format!("{path}\tdate\n"));
        assert!(
            resolve(&root.0, "Megalania_Character_BP_Aberrant_C").is_err(),
            "{path}"
        );
    }
}

#[test]
fn empty_non_file_oversized_and_unbounded_line_manifests_are_refused() {
    let root = Directory::new();
    fs::create_dir(root.0.join(MANIFEST)).unwrap();
    assert!(resolve(&root.0, "Megalania_Character_BP_Aberrant_C").is_err());
    fs::remove_dir(root.0.join(MANIFEST)).unwrap();
    root.write([]);
    assert!(resolve(&root.0, "Megalania_Character_BP_Aberrant_C").is_err());
    root.write(vec![b'a'; MAX_LINE_BYTES + 1]);
    assert!(resolve(&root.0, "Megalania_Character_BP_Aberrant_C").is_err());
    root.write(MEGALANIA_ROW);
    assert!(
        scan(
            Cursor::new(MEGALANIA_ROW),
            "Megalania_Character_BP_Aberrant",
            (MEGALANIA_ROW.len() - 1) as u64
        )
        .is_err()
    );
    OpenOptions::new()
        .write(true)
        .open(root.0.join(MANIFEST))
        .unwrap()
        .set_len(MAX_MANIFEST_BYTES + 1)
        .unwrap();
    assert!(resolve(&root.0, "Megalania_Character_BP_Aberrant_C").is_err());
}

#[test]
fn a_read_error_after_a_matching_row_is_not_treated_as_success() {
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("injected failure"))
        }
    }
    let reader = Cursor::new(SPIDER_ROW).chain(Broken);
    assert!(scan(reader, "SpiderS_Character_BP_Aberrant", MAX_MANIFEST_BYTES).is_err());
}

#[test]
#[ignore = "read-only: requires LANGAME_ARK_UFS_MANIFEST_ROOT pointing to a real official ASA package; does not load or spawn game assets"]
fn native_manifest_resolves_current_asa_creatures() -> Result<(), String> {
    let root = std::env::var_os("LANGAME_ARK_UFS_MANIFEST_ROOT")
        .map(PathBuf::from)
        .ok_or("set LANGAME_ARK_UFS_MANIFEST_ROOT explicitly")?;
    if !root.is_absolute() || !root.is_dir() {
        return Err("manifest root must be an existing absolute directory".into());
    }
    let started = std::time::Instant::now();
    if resolve(&root, "SpiderS_Character_BP_Aberrant_C")? != SPIDER_PATH
        || resolve(&root, "Megalania_Character_BP_Aberrant_C")? != MEGALANIA_PATH
    {
        return Err(
            "official manifest creature paths did not match the verified package assets".into(),
        );
    }
    println!(
        "ARK_MANIFEST_READBACK resolved=2 read_only=true assets_loaded=false elapsed_ms={}",
        started.elapsed().as_millis()
    );
    Ok(())
}
