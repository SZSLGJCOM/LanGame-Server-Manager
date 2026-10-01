use super::manifest_format::{fields, number};
use super::*;

// Independent synthetic protobuf bytes: file "x.bin"/"y.bin", contents "abc".
// SHA1 is the standard abc vector; CRC values were produced by Python zlib.
const OLD: &str = "d017f671230000000a210a05782e62696e100318002a14a9993e364706816aba3e25717850c26c9cd0d89dbe12481f0d000000080710092000280348b8e1fe5317b8811b00000000ab15c432";
const NEW: &str = "d017f671230000000a210a05792e62696e100318002a14a9993e364706816aba3e25717850c26c9cd0d89dbe12481f0d0000000807100a2000280348e9b0e12417b8811b00000000ab15c432";
// Independent depot 8 / manifest 10: x.bin contains "updated" (7 bytes).
const OVERLAP: &str = "d017f671230000000a210a05782e62696e100718002a1413a1891af75c642306a6b695377d16e4a91f0e1bbe12481f0e0000000808100a2000280748b8a084f10f17b8811b00000000ab15c432";
fn bytes(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|part| u8::from_str_radix(std::str::from_utf8(part).unwrap(), 16).unwrap())
        .collect()
}
fn acf(gid: u64) -> String {
    format!(
        r#""AppState" {{ "appid" "7" "StateFlags" "4" "InstalledDepots" {{ "7" {{ "manifest" "{gid}" }} }} }}"#
    )
}
struct Fixture {
    root: PathBuf,
    source: PathBuf,
    target: PathBuf,
    steamcmd: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lgsm-steam-seed-test-{}", uuid::Uuid::new_v4()));
        let source = root.join("source");
        let target = root.join("target");
        let steamcmd = root.join("steamcmd");
        fs::create_dir_all(source.join("steamapps")).unwrap();
        fs::create_dir_all(steamcmd.join("depotcache")).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(source.join("steamapps/appmanifest_7.acf"), acf(9)).unwrap();
        fs::write(source.join("x.bin"), b"abc").unwrap();
        fs::write(steamcmd.join("depotcache/7_9.manifest"), bytes(OLD)).unwrap();
        fs::write(target.join(ACQUISITION), serde_json::to_vec(&serde_json::json!({
            "version":1,"module_id":"fixture","target":target,"id":uuid::Uuid::new_v4().to_string()
        })).unwrap()).unwrap();
        Self {
            root,
            source,
            target,
            steamcmd,
        }
    }
    fn prepare(&self) -> SeedReceipt {
        prepare(&self.source, &self.target, &self.steamcmd, 7, "fixture").unwrap()
    }
    // Model only the external SteamCMD boundary, after it has validated payload.
    fn installed(&self) {
        fs::create_dir_all(self.target.join("steamapps")).unwrap();
        fs::write(self.target.join("steamapps/appmanifest_7.acf"), acf(9)).unwrap();
    }
    fn update(&self) {
        fs::create_dir_all(self.target.join("steamapps")).unwrap();
        fs::write(self.steamcmd.join("depotcache/7_10.manifest"), bytes(NEW)).unwrap();
        fs::write(self.target.join("steamapps/appmanifest_7.acf"), acf(10)).unwrap();
        fs::write(self.target.join("y.bin"), b"abc").unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn cached_manifest_checks_crc_identity_and_protobuf_boundaries() {
    let data = bytes(OLD);
    let parsed = manifest(&data, 7, 9).unwrap();
    assert_eq!(
        parsed["x.bin"],
        Entry {
            size: 3,
            sha1: "a9993e364706816aba3e25717850c26c9cd0d89d".into()
        }
    );
    assert!(manifest(&data, 7, 10).is_err());
    let mut damaged = data.clone();
    damaged[20] ^= 1;
    for bad in [&data[..4], &data[..data.len() - 1], damaged.as_slice()] {
        assert!(manifest(bad, 7, 9).is_err());
    }
    for bad in [vec![0], vec![0x0a, 0xff, 0x7f], vec![0xff; 10]] {
        assert!(fields(&bad).is_err());
    }
    assert!(number(&fields(&[8, 1, 8, 2]).unwrap(), 1).is_err());
}

#[test]
fn cache_paths_and_acf_identity_fail_closed() {
    for path in [
        "../save",
        "C:/save",
        "a:stream",
        "NUL.txt",
        "a//b",
        "a.",
        "a ",
        ".langame-clean-package.json",
    ] {
        assert!(relative(path).is_err(), "{path}");
    }
    assert!(parse_acf(acf(9).as_bytes(), 8).is_err());
    assert!(
        parse_acf(
            acf(9)
                .replace("\"StateFlags\" \"4\"", "\"StateFlags\" \"1026\"")
                .as_bytes(),
            7
        )
        .is_err()
    );
    assert!(parse_acf(b"\"AppState\" { \"appid\" \"7\" \"APPID\" \"7\" }", 7).is_err());
    let entry = Entry {
        size: 3,
        sha1: "a9993e364706816aba3e25717850c26c9cd0d89d".into(),
    };
    assert!(
        validate_entries(&BTreeMap::from([
            ("A/x".into(), entry.clone()),
            ("a/y".into(), entry)
        ]))
        .is_err()
    );
}

#[test]
fn partial_seed_skips_modified_and_unknown_files_without_touching_source() {
    let fixture = Fixture::new();
    fs::write(fixture.source.join("x.bin"), b"xyz").unwrap();
    fs::write(fixture.source.join("user.ini"), b"personal").unwrap();
    let receipt = fixture.prepare();
    assert_eq!(receipt.summary.copied_files, 0);
    assert_eq!(receipt.summary.skipped_files, 1);
    assert!(!fixture.target.join("x.bin").exists());
    assert!(!fixture.target.join("user.ini").exists());
    assert!(
        !fixture
            .target
            .join(format!(".langame-cache-copy-{}", receipt.id))
            .exists()
    );
    assert_eq!(fs::read(fixture.source.join("x.bin")).unwrap(), b"xyz");
    assert_eq!(
        fs::read(fixture.source.join("user.ini")).unwrap(),
        b"personal"
    );
    // Model the actual external installer restoring the official bytes.
    fs::write(fixture.target.join("x.bin"), b"abc").unwrap();
    fixture.installed();
    assert_eq!(
        finalize(&receipt, &fixture.steamcmd)
            .unwrap()
            .verified_files,
        1
    );
    assert!(fixture.target.join(ACQUISITION).exists());
    assert!(fixture.target.join(RECEIPT).exists());
}

#[test]
fn update_removes_only_seeded_files_retired_from_new_depots() {
    let fixture = Fixture::new();
    let receipt = fixture.prepare();
    assert_eq!(receipt.summary.copied_bytes, 3);
    fixture.update();
    let result = finalize(&receipt, &fixture.steamcmd).unwrap();
    assert_eq!((result.verified_files, result.removed_files), (1, 1));
    assert!(!fixture.target.join("x.bin").exists());
    assert_eq!(fs::read(fixture.source.join("x.bin")).unwrap(), b"abc");
    assert_eq!(fs::read(fixture.target.join("y.bin")).unwrap(), b"abc");
}

#[test]
fn changed_retired_or_unknown_files_block_baseline_and_preserve_receipt() {
    let fixture = Fixture::new();
    let receipt = fixture.prepare();
    fixture.update();
    fs::write(fixture.target.join("x.bin"), b"xyz").unwrap();
    assert!(finalize(&receipt, &fixture.steamcmd).is_err());
    assert_eq!(fs::read(fixture.target.join("x.bin")).unwrap(), b"xyz");
    assert!(fixture.target.join(RECEIPT).exists());
    fs::write(fixture.target.join("x.bin"), b"abc").unwrap();
    fs::write(fixture.target.join("unknown.dll"), b"personal").unwrap();
    assert!(finalize(&receipt, &fixture.steamcmd).is_err());
    assert_eq!(
        fs::read(fixture.target.join("unknown.dll")).unwrap(),
        b"personal"
    );
    assert!(fixture.target.join(RECEIPT).exists());
}

#[test]
fn resume_retains_updated_bytes_and_rejects_changed_acquisition() {
    let fixture = Fixture::new();
    let receipt = fixture.prepare();
    fs::write(fixture.target.join("x.bin"), b"new-version").unwrap();
    let resumed = fixture.prepare();
    assert_eq!(resumed.id, receipt.id);
    assert_eq!(
        fs::read(fixture.target.join("x.bin")).unwrap(),
        b"new-version"
    );
    let mut marker: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.target.join(ACQUISITION)).unwrap()).unwrap();
    marker["id"] = serde_json::json!(uuid::Uuid::new_v4().to_string());
    fs::write(
        fixture.target.join(ACQUISITION),
        serde_json::to_vec(&marker).unwrap(),
    )
    .unwrap();
    assert!(load(&fixture.target, "fixture", 7).is_err());
    assert!(finalize(&receipt, &fixture.steamcmd).is_err());
}

#[test]
fn missing_exact_depot_is_partial_only_and_shared_depots_cannot_be_omitted() {
    let fixture = Fixture::new();
    fs::rename(
        fixture.steamcmd.join("depotcache/7_9.manifest"),
        fixture.steamcmd.join("depotcache/7_10.manifest"),
    )
    .unwrap();
    let receipt = fixture.prepare();
    assert_eq!(receipt.summary.missing_depots, 1);
    assert_eq!(receipt.summary.copied_files, 0);
    fixture.installed();
    assert!(finalize(&receipt, &fixture.steamcmd).is_err());
    fs::write(fixture.steamcmd.join("depotcache/7_9.manifest"), bytes(OLD)).unwrap();
    fs::write(fixture.target.join("x.bin"), b"abc").unwrap();
    let shared = acf(9).replacen(
        "\"InstalledDepots\"",
        "\"SharedDepots\" { \"99\" \"100\" } \"InstalledDepots\"",
        1,
    );
    fs::write(fixture.target.join("steamapps/appmanifest_7.acf"), shared).unwrap();
    assert!(finalize(&receipt, &fixture.steamcmd).is_err());
    assert!(fixture.target.join(RECEIPT).exists());
}

#[test]
fn observation_retains_exact_depot_alternatives_while_seed_refuses_to_choose() {
    let fixture = Fixture::new();
    let mounted = acf(9).replace(
        "\"InstalledDepots\" {",
        "\"InstalledDepots\" { \"8\" { \"manifest\" \"10\" }",
    );
    fs::write(fixture.source.join("steamapps/appmanifest_7.acf"), mounted).unwrap();
    fs::write(
        fixture.steamcmd.join("depotcache/8_10.manifest"),
        bytes(OVERLAP),
    )
    .unwrap();
    let observed = inventory_for_inspection(&fixture.source, &fixture.steamcmd, 7, true).unwrap();
    let candidates = &observed.overlapping_files["x.bin"];
    assert_eq!(candidates.len(), 2);
    assert_eq!(
        (
            candidates[0].depot,
            candidates[0].manifest,
            candidates[0].entry.size
        ),
        (7, 9, 3)
    );
    assert_eq!(
        (
            candidates[1].depot,
            candidates[1].manifest,
            candidates[1].entry.size
        ),
        (8, 10, 7)
    );
    assert_eq!(
        candidates[1].entry.sha1,
        "13a1891af75c642306a6b695377d16e4a91f0e1b"
    );
    for payload in [b"abc".as_slice(), b"updated"] {
        fs::write(fixture.source.join("x.bin"), payload).unwrap();
        assert!(inventory(&fixture.source, &fixture.steamcmd, 7, true).is_err());
        assert!(
            prepare(
                &fixture.source,
                &fixture.target,
                &fixture.steamcmd,
                7,
                "fixture"
            )
            .is_err()
        );
        assert!(!fixture.target.join("x.bin").exists());
    }
    fs::remove_file(fixture.steamcmd.join("depotcache/8_10.manifest")).unwrap();
    let partial = inventory_for_inspection(&fixture.source, &fixture.steamcmd, 7, false).unwrap();
    assert_eq!(partial.missing_manifests.len(), 1);
    assert!(inventory_for_inspection(&fixture.source, &fixture.steamcmd, 7, true).is_err());
}

#[test]
fn candidate_size_budget_covers_cross_depot_maxima() {
    // Independent protobuf fixtures: each depot declares a valid 2 TiB total,
    // with x/y sizes 1.5/0.5 TiB and 0.5/1.5 TiB. Their path maxima total 3 TiB.
    let first = bytes(
        "d017f671500000000a260a05782e62696e1080808080803018002a14a9993e364706816aba3e25717850c26c9cd0d89d0a260a05792e62696e1080808080801018002a14a9993e364706816aba3e25717850c26c9cd0d89dbe12481f1300000008071009200028808080808040489ed8f6fc0e17b8811b00000000ab15c432",
    );
    let second = bytes(
        "d017f671500000000a260a05782e62696e1080808080801018002a14a9993e364706816aba3e25717850c26c9cd0d89d0a260a05792e62696e1080808080803018002a14a9993e364706816aba3e25717850c26c9cd0d89dbe12481f130000000808100a2000288080808080404898c98fc40917b8811b00000000ab15c432",
    );
    assert!(manifest(&first, 7, 9).is_ok());
    assert!(manifest(&second, 8, 10).is_ok());
    let fixture = Fixture::new();
    let mounted = acf(9).replace(
        "\"InstalledDepots\" {",
        "\"InstalledDepots\" { \"8\" { \"manifest\" \"10\" }",
    );
    fs::write(fixture.source.join("steamapps/appmanifest_7.acf"), mounted).unwrap();
    fs::write(fixture.steamcmd.join("depotcache/7_9.manifest"), first).unwrap();
    fs::write(fixture.steamcmd.join("depotcache/8_10.manifest"), second).unwrap();
    let error = inventory_for_inspection(&fixture.source, &fixture.steamcmd, 7, true)
        .err()
        .unwrap();
    assert!(
        error
            .to_string()
            .contains("candidate package exceeds byte bound"),
        "{error}"
    );
}

#[test]
fn zero_digest_requires_zero_length_and_no_chunks() {
    let empty = bytes(
        "d017f671230000000a210a05782e62696e100018002a140000000000000000000000000000000000000000be12481f0e000000080710092000280048be88eda90817b8811b00000000ab15c432",
    );
    let entry = manifest(&empty, 7, 9).unwrap().remove("x.bin").unwrap();
    assert_eq!(entry.size, 0);
    assert_eq!(entry.sha1, "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    for invalid in [
        "d017f671230000000a210a05782e62696e100318002a140000000000000000000000000000000000000000be12481f0e000000080710092000280348ede4db860b17b8811b00000000ab15c432",
        "d017f671250000000a230a05782e62696e100018002a1400000000000000000000000000000000000000003200be12481f0e000000080710092000280048afb6be850717b8811b00000000ab15c432",
    ] {
        assert!(manifest(&bytes(invalid), 7, 9).is_err());
    }
}

#[test]
fn finalization_is_repeated_until_baseline_and_registration_consume_receipt() {
    let fixture = Fixture::new();
    let receipt = fixture.prepare();
    fixture.installed();
    finalize(&receipt, &fixture.steamcmd).unwrap();
    assert!(fixture.target.join(RECEIPT).exists());
    assert!(finish(&receipt).is_err());
    fs::write(fixture.target.join("unknown.dll"), b"preserve").unwrap();
    let resumed = load(&fixture.target, "fixture", 7).unwrap().unwrap();
    assert!(finalize(&resumed, &fixture.steamcmd).is_err());
    assert_eq!(
        fs::read(fixture.target.join("unknown.dll")).unwrap(),
        b"preserve"
    );
}

#[test]
fn baseline_partial_commit_restores_exact_acquisition_before_reverification() {
    let fixture = Fixture::new();
    let receipt = fixture.prepare();
    fixture.installed();
    finalize(&receipt, &fixture.steamcmd).unwrap();
    let original = fs::read(fixture.target.join(ACQUISITION)).unwrap();
    fs::remove_file(fixture.target.join(ACQUISITION)).unwrap();
    fs::write(fixture.target.join(".langame-clean-package.json"), b"{}").unwrap();
    fs::write(fixture.target.join(".langame-initial-package.json"), b"{}").unwrap();
    let resumed = load(&fixture.target, "fixture", 7).unwrap().unwrap();
    assert_eq!(
        fs::read(fixture.target.join(ACQUISITION)).unwrap(),
        original
    );
    finalize(&resumed, &fixture.steamcmd).unwrap();
    assert!(fixture.target.join(RECEIPT).exists());
    fs::remove_file(fixture.target.join(ACQUISITION)).unwrap();
    finish(&resumed).unwrap();
    assert!(!fixture.target.join(RECEIPT).exists());
}

#[test]
fn fresh_and_resumed_seed_never_publish_source_installed_state() {
    let fixture = Fixture::new();
    let original = fs::read(fixture.source.join("steamapps/appmanifest_7.acf")).unwrap();
    let receipt = fixture.prepare();
    assert_eq!(receipt.summary.copied_bytes, 3);
    assert_eq!(receipt.acfs.len(), 1);
    assert!(!fixture.target.join("steamapps/appmanifest_7.acf").exists());
    assert!(finalize(&receipt, &fixture.steamcmd).is_err());
    let resumed = fixture.prepare();
    assert_eq!(receipt.id, resumed.id);
    assert!(!fixture.target.join("steamapps/appmanifest_7.acf").exists());
    assert_eq!(
        fs::read(fixture.source.join("steamapps/appmanifest_7.acf")).unwrap(),
        original
    );
}

#[test]
fn partial_seed_does_not_claim_uncopied_shared_depot_is_installed() {
    let fixture = Fixture::new();
    let shared = acf(9).replacen(
        "\"InstalledDepots\"",
        "\"SharedDepots\" { \"99\" \"100\" } \"InstalledDepots\"",
        1,
    );
    fs::write(fixture.source.join("steamapps/appmanifest_7.acf"), shared).unwrap();
    fs::write(
        fixture.source.join("steamapps/appmanifest_100.acf"),
        r#""AppState" { "appid" "100" "StateFlags" "4" "InstalledDepots" { "99" { "manifest" "11" } } }"#,
    ).unwrap();
    let receipt = fixture.prepare();
    assert_eq!(receipt.summary.copied_files, 1);
    assert_eq!(receipt.summary.missing_depots, 1);
    assert_eq!(receipt.acfs.len(), 2);
    for app_id in [7, 100] {
        assert!(
            !fixture
                .target
                .join(format!("steamapps/appmanifest_{app_id}.acf"))
                .exists()
        );
    }
}

#[test]
fn failed_steam_acf_is_preserved_and_operator_removal_is_not_undone_on_resume() {
    let fixture = Fixture::new();
    let receipt = fixture.prepare();
    fixture.installed();
    let path = fixture.target.join("steamapps/appmanifest_7.acf");
    let failed = acf(9).replace("\"StateFlags\" \"4\"", "\"StateFlags\" \"1062\"");
    fs::write(&path, &failed).unwrap();
    assert_eq!(fixture.prepare().id, receipt.id);
    assert_eq!(fs::read(&path).unwrap(), failed.as_bytes());
    // The operator separately preserves/removes this one failed control file.
    let preserved = fs::read(&path).unwrap();
    fs::remove_file(&path).unwrap();
    assert_eq!(fixture.prepare().id, receipt.id);
    assert!(!path.exists());
    assert_eq!(preserved, failed.as_bytes());
    assert_eq!(fs::read(fixture.target.join("x.bin")).unwrap(), b"abc");
    fixture.installed();
    assert_eq!(
        finalize(&receipt, &fixture.steamcmd)
            .unwrap()
            .verified_files,
        1
    );
}
