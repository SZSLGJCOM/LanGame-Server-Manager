use super::*;

struct Fixture {
    root: PathBuf,
    cache: PathBuf,
}

// Independent depot wire records for the standard SHA-1 vector "abc".
const DEPOT_10: &str = "d017f671230000000a210a05782e62696e100318002a14a9993e364706816aba3e25717850c26c9cd0d89dbe12481f0d000000080a10652000280348b8e1fe5317b8811b00000000ab15c432";
const DEPOT_11: &str = "d017f671230000000a210a05782e62696e100318002a14a9993e364706816aba3e25717850c26c9cd0d89dbe12481f0d000000080b10662000280348b8e1fe5317b8811b00000000ab15c432";

fn bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

impl Fixture {
    fn new() -> Self {
        let root = crate::tests::unique_test_root();
        fs::create_dir_all(root.join("steamapps")).unwrap();
        fs::write(
            root.join("steamapps/appmanifest_7.acf"),
            r#""AppState" { "appid" "7" "StateFlags" "4" "SharedDepots" { "10" "8" "11" "8" } }"#,
        )
        .unwrap();
        let cache = root.join("depotcache");
        fs::create_dir(&cache).unwrap();
        fs::write(cache.join("10_101.manifest"), bytes(DEPOT_10)).unwrap();
        fs::write(cache.join("11_102.manifest"), bytes(DEPOT_11)).unwrap();
        Self { root, cache }
    }

    fn owner(&self, flags: &str, depots: &str, extra: &str) {
        fs::write(
            self.root.join("steamapps/appmanifest_8.acf"),
            format!(
                r#""AppState" {{ "appid" "8" "StateFlags" "{flags}" "InstalledDepots" {{ {depots} }} {extra} }}"#
            ),
        )
        .unwrap();
    }

    fn inspect(&self) -> io::Result<Vec<u32>> {
        unfinished_dependencies(&self.root, &self.cache, 7, || Ok(()))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

const DEPOTS: &str = r#""10" { "manifest" "101" } "11" { "manifest" "102" }"#;

#[test]
fn complete_owner_with_missing_or_invalid_cache_needs_acquisition_repair_only() {
    let fixture = Fixture::new();
    fixture.owner("4", DEPOTS, "");
    let path = fixture.cache.join("10_101.manifest");
    let mut bad_crc = bytes(DEPOT_10);
    bad_crc[12] ^= 1;
    for (case, content) in [
        ("missing", None),
        ("CRC", Some(bad_crc)),
        ("identity", Some(bytes(DEPOT_11))),
    ] {
        fs::remove_file(&path).unwrap();
        if let Some(content) = content {
            fs::write(&path, content).unwrap();
        }
        assert_eq!(fixture.inspect().unwrap(), [8], "{case}");
        assert!(
            crate::probe_steam_app_manifest(&fixture.root, 7)
                .unwrap()
                .complete,
            "ordinary state probing must not depend on cached manifests: {case}",
        );
        fs::write(&path, bytes(DEPOT_10)).unwrap();
        assert!(fixture.inspect().unwrap().is_empty());
    }
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert_eq!(
        fixture.inspect().unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn shared_dependency_completion_requires_every_declared_depot_and_finished_progress() {
    let fixture = Fixture::new();
    assert_eq!(fixture.inspect().unwrap(), [8]);
    assert!(
        !crate::probe_steam_app_manifest(&fixture.root, 7)
            .unwrap()
            .complete
    );
    for (flags, depots, extra) in [
        ("2", DEPOTS, ""),
        ("4", r#""10" { "manifest" "101" }"#, ""),
        (
            "4",
            r#""10" { "manifest" "0" } "11" { "manifest" "102" }"#,
            "",
        ),
        (
            "4",
            DEPOTS,
            r#""BytesToDownload" "10" "BytesDownloaded" "9""#,
        ),
        ("4", DEPOTS, r#""BytesToStage" "10" "BytesStaged" "0""#),
    ] {
        fixture.owner(flags, depots, extra);
        assert_eq!(fixture.inspect().unwrap(), [8]);
    }
    fixture.owner(
        "4",
        DEPOTS,
        r#""BytesToDownload" "10" "BytesDownloaded" "10""#,
    );
    assert!(fixture.inspect().unwrap().is_empty());
    assert!(
        crate::probe_steam_app_manifest(&fixture.root, 7)
            .unwrap()
            .complete
    );
    fs::write(
        fixture.root.join("steamapps/appmanifest_7.acf"),
        r#""AppState" { "appid" "7" "StateFlags" "4" }"#,
    )
    .unwrap();
    assert!(fixture.inspect().unwrap().is_empty());
}

#[test]
fn malformed_shared_dependency_metadata_is_rejected_without_treating_it_as_pending() {
    let fixture = Fixture::new();
    for source in [
        r#""AppState" { "appid" "9" "StateFlags" "4" }"#,
        r#""AppState" { "appid" "8" "StateFlags" "invalid" }"#,
        r#""AppState" { "appid" "8" "StateFlags" "4" "InstalledDepots" "invalid" }"#,
        r#""AppState" { "appid" "8" "StateFlags" "4" "InstalledDepots" { "10" { "manifest" "invalid" } } }"#,
        r#""AppState" { "appid" "8" "StateFlags" "2" "BytesToDownload" "invalid" }"#,
        r#""AppState" { "appid" "8" "StateFlags" "2" "InstalledDepots" "invalid" }"#,
        r#""AppState" { "appid" "8" "StateFlags" "2" "InstalledDepots" { "10" { "manifest" "invalid" } } }"#,
    ] {
        fs::write(fixture.root.join("steamapps/appmanifest_8.acf"), source).unwrap();
        assert_eq!(
            fixture.inspect().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
    for source in [
        r#""AppState" { "appid" "7" "StateFlags" "4" "SharedDepots" { "10" "0" } }"#,
        r#""AppState" { "appid" "7" "StateFlags" "4" "SharedDepots" { "../escape" "8" } }"#,
        r#""AppState" { "appid" "6" "StateFlags" "4" }"#,
    ] {
        fs::write(fixture.root.join("steamapps/appmanifest_7.acf"), source).unwrap();
        assert_eq!(
            fixture.inspect().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
    fs::remove_file(fixture.root.join("steamapps/appmanifest_7.acf")).unwrap();
    assert_eq!(
        fixture.inspect().unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
}

#[test]
fn dependency_inspection_checks_cancellation_before_accessing_metadata() {
    let error =
        unfinished_dependencies(Path::new("missing"), Path::new("missing-cache"), 7, || {
            Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"))
        })
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
}

#[cfg(windows)]
#[tokio::test]
async fn shared_dependency_acf_cannot_be_read_through_a_junction() {
    let fixture = Fixture::new();
    let outside = crate::tests::unique_test_root();
    fs::create_dir_all(&outside).unwrap();
    fs::rename(
        fixture.root.join("steamapps/appmanifest_7.acf"),
        outside.join("appmanifest_7.acf"),
    )
    .unwrap();
    let sentinel = outside.join("appmanifest_8.acf");
    fs::write(&sentinel, b"external metadata must remain untouched").unwrap();
    fs::remove_dir(fixture.root.join("steamapps")).unwrap();
    let link = fixture.root.join("steamapps");
    let script = format!(
        "New-Item -ItemType Junction -Path {} -Target {} | Out-Null",
        ps_literal(&link),
        ps_literal(&outside),
    );
    let output = run_powershell(
        &script,
        Some(&fixture.root),
        InstallDeadline::new("Steam dependency junction fixture", Duration::from_secs(30)),
    )
    .await
    .unwrap();
    assert!(output.status.success());
    assert_eq!(
        fixture.inspect().unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(
        fs::read(sentinel).unwrap(),
        b"external metadata must remain untouched"
    );
    fs::remove_dir(link).unwrap();
    fs::remove_dir_all(outside).unwrap();
}
