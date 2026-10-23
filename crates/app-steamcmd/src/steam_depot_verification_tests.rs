use super::*;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;

// Independent wire fixtures and standard SHA1 vectors retained from the
// catalog's reader tests. These do not use the parser to construct expectations.
const ORIGINAL: &str = "d017f671230000000a210a05782e62696e100318002a14a9993e364706816aba3e25717850c26c9cd0d89dbe12481f0d000000080710092000280348b8e1fe5317b8811b00000000ab15c432";
const OVERLAP: &str = "d017f671230000000a210a05782e62696e100718002a1413a1891af75c642306a6b695377d16e4a91f0e1bbe12481f0e0000000808100a2000280748b8a084f10f17b8811b00000000ab15c432";
const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    base: PathBuf,
    root: PathBuf,
    steamcmd: PathBuf,
}

fn bytes(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

impl Fixture {
    fn new() -> Self {
        let base = loop {
            let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("lg-depot-{}-{sequence}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => break path,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create {}: {error}", path.display()),
            }
        };
        let root = base.join("game");
        let steamcmd = base.join("steamcmd");
        fs::create_dir_all(root.join("steamapps")).unwrap();
        fs::create_dir_all(steamcmd.join("depotcache")).unwrap();
        fs::write(root.join("x.bin"), b"abc").unwrap();
        fs::write(
            root.join("steamapps/appmanifest_7.acf"),
            Self::acf("", "17", "4"),
        )
        .unwrap();
        fs::write(steamcmd.join("depotcache/7_9.manifest"), bytes(ORIGINAL)).unwrap();
        Self {
            base,
            root,
            steamcmd,
        }
    }

    fn acf(extra: &str, build: &str, flags: &str) -> String {
        format!(
            r#""AppState" {{ "appid" "7" "StateFlags" "{flags}" "buildid" "{build}" "TargetBuildID" "{build}" "InstalledDepots" {{ "7" {{ "manifest" "9" }} }} {extra} }}"#
        )
    }

    fn verify(&self) -> io::Result<VerifiedSteamPackage> {
        verify_installed_steam_package(&self.root, &self.steamcmd, 7, "17", None)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.base.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(&self.base).unwrap();
    }
}

#[test]
fn exact_package_preserves_unknown_files_without_certifying_them() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("operator.cfg"), b"keep user data").unwrap();
    fs::write(
        fixture.root.join(".langame-program-acquisition.json"),
        b"pending ownership",
    )
    .unwrap();
    let result = fixture.verify().unwrap();
    assert_eq!(
        result.files.get("x.bin").map(String::as_str),
        Some(ABC_SHA256)
    );
    assert_eq!(result.files.len(), 2);
    assert!(result.files.contains_key("steamapps/appmanifest_7.acf"));
    assert!(result.directories.contains("steamapps"));
    assert!(!result.files.contains_key("operator.cfg"));
    assert!(
        !result
            .files
            .contains_key(".langame-program-acquisition.json")
    );
    assert_eq!(
        fs::read(fixture.root.join("operator.cfg")).unwrap(),
        b"keep user data"
    );
    assert_eq!(
        fs::read(fixture.root.join(".langame-program-acquisition.json")).unwrap(),
        b"pending ownership"
    );
}

#[test]
fn missing_modified_and_wrong_size_payloads_cannot_be_certified() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("x.bin"), b"xyz").unwrap();
    assert!(
        fixture
            .verify()
            .unwrap_err()
            .to_string()
            .contains("SHA1 mismatch")
    );
    fs::write(fixture.root.join("x.bin"), b"different size").unwrap();
    assert!(
        fixture
            .verify()
            .unwrap_err()
            .to_string()
            .contains("size mismatch")
    );
    fs::remove_file(fixture.root.join("x.bin")).unwrap();
    assert_eq!(
        fixture.verify().unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
}

#[test]
fn missing_exact_depot_and_incomplete_or_changed_builds_are_rejected() {
    let fixture = Fixture::new();
    let acf = fixture.root.join("steamapps/appmanifest_7.acf");
    fs::write(&acf, Fixture::acf("", "18", "4")).unwrap();
    assert!(
        fixture
            .verify()
            .unwrap_err()
            .to_string()
            .contains("build differs")
    );
    fs::write(&acf, Fixture::acf("", "17", "1026")).unwrap();
    assert!(
        fixture
            .verify()
            .unwrap_err()
            .to_string()
            .contains("incomplete")
    );
    fs::write(&acf, Fixture::acf("", "17", "4")).unwrap();
    fs::remove_file(fixture.steamcmd.join("depotcache/7_9.manifest")).unwrap();
    let error = fixture.verify().unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert!(error.to_string().contains("7_9.manifest"));
}

#[test]
fn absent_or_zero_target_build_is_complete_but_another_target_is_rejected() {
    let fixture = Fixture::new();
    let path = fixture.root.join("steamapps/appmanifest_7.acf");
    for target in ["", "\"TargetBuildID\" \"0\""] {
        fs::write(
            &path,
            Fixture::acf("", "17", "4").replace("\"TargetBuildID\" \"17\"", target),
        )
        .unwrap();
        assert!(fixture.verify().is_ok());
    }
    for target in ["18", "invalid"] {
        fs::write(
            &path,
            Fixture::acf("", "17", "4").replace(
                "\"TargetBuildID\" \"17\"",
                &format!("\"TargetBuildID\" \"{target}\""),
            ),
        )
        .unwrap();
        assert!(fixture.verify().is_err());
    }
}

#[test]
fn shared_depot_overlap_accepts_only_declared_official_alternatives() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("steamapps/appmanifest_7.acf"),
        Fixture::acf(r#""SharedDepots" { "8" "8" }"#, "17", "4"),
    )
    .unwrap();
    fs::write(fixture.root.join("steamapps/appmanifest_8.acf"), r#""AppState" { "appid" "8" "StateFlags" "4" "InstalledDepots" { "8" { "manifest" "10" } } }"#).unwrap();
    fs::write(
        fixture.steamcmd.join("depotcache/8_10.manifest"),
        bytes(OVERLAP),
    )
    .unwrap();
    fs::write(fixture.root.join("x.bin"), b"updated").unwrap();
    let updated = fixture.verify().unwrap();
    assert_eq!(updated.files.len(), 3);
    assert!(updated.files.contains_key("steamapps/appmanifest_8.acf"));
    fs::write(fixture.root.join("x.bin"), b"abc").unwrap();
    assert_eq!(fixture.verify().unwrap().files["x.bin"], ABC_SHA256);
    fs::write(fixture.root.join("x.bin"), b"unknown").unwrap();
    assert!(fixture.verify().is_err());
}

#[test]
fn cancelled_verification_returns_no_package() {
    let fixture = Fixture::new();
    let signal = AtomicBool::new(true);
    let error =
        verify_installed_steam_package(&fixture.root, &fixture.steamcmd, 7, "17", Some(&signal))
            .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    let expected = Entry {
        size: 3,
        sha1: "a9993e364706816aba3e25717850c26c9cd0d89d".into(),
    };
    let mut on_progress = |_| {};
    let mut reporter = VerificationReporter::new(3, 1, &mut on_progress);
    let mut file = open_read(&fixture.root.join("x.bin")).unwrap();
    assert_eq!(
        verify_file(
            &mut file,
            &fixture.root.join("x.bin"),
            &[&expected],
            Some(&signal),
            &mut reporter
        )
        .unwrap_err()
        .kind(),
        io::ErrorKind::Interrupted
    );
}

#[test]
fn measured_progress_counts_only_the_exact_official_files() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("operator-data.bin"), vec![0; 4096]).unwrap();
    let mut reports = Vec::new();
    let mut observed = BTreeMap::new();
    let result = verify_installed_steam_package_with_progress(
        &fixture.root,
        &fixture.steamcmd,
        7,
        "17",
        SteamPackageVerificationOptions::default(),
        |progress| reports.push(progress),
        |relative, file, digest| {
            assert_eq!(
                file.metadata()?.len(),
                fs::metadata(fixture.root.join(relative))?.len()
            );
            observed.insert(relative.to_owned(), digest.to_owned());
            #[cfg(windows)]
            {
                assert!(
                    fs::OpenOptions::new()
                        .write(true)
                        .open(fixture.root.join(relative))
                        .is_err()
                );
                assert!(
                    fs::rename(
                        fixture.root.join(relative),
                        fixture.root.join("replaced.bin")
                    )
                    .is_err()
                );
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(observed, result.files);
    let start = reports.first().unwrap();
    assert_eq!(start.completed_bytes, 0);
    assert_eq!(start.completed_files, 0);
    let end = reports.last().unwrap();
    assert_eq!(end.stage, InstallVerificationStage::SteamFiles);
    assert_eq!(end.total_files, 2);
    assert_eq!(end.completed_files, 2);
    assert_eq!(
        end.total_bytes,
        3 + Fixture::acf("", "17", "4").len() as u64
    );
    assert_eq!(end.completed_bytes, end.total_bytes);
}

#[test]
fn failed_official_hash_cannot_produce_evidence_or_completion() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("x.bin"), b"xyz").unwrap();
    let mut reports = Vec::new();
    let mut observations = 0;
    assert!(
        verify_installed_steam_package_with_progress(
            &fixture.root,
            &fixture.steamcmd,
            7,
            "17",
            SteamPackageVerificationOptions::default(),
            |progress| reports.push(progress),
            |_, _, _| {
                observations += 1;
                Ok(())
            },
        )
        .is_err()
    );
    assert_eq!(observations, 0);
    assert!(
        reports
            .iter()
            .all(|progress| progress.completed_files < progress.total_files)
    );
}

#[test]
fn evidence_callback_errors_and_progress_cancellation_stop_publication() {
    let fixture = Fixture::new();
    let error = verify_installed_steam_package_with_progress(
        &fixture.root,
        &fixture.steamcmd,
        7,
        "17",
        SteamPackageVerificationOptions::default(),
        |_| {},
        |_, _, _| Err(io::Error::other("evidence unavailable")),
    )
    .unwrap_err();
    assert!(error.to_string().contains("evidence unavailable"));
    let signal = AtomicBool::new(false);
    let mut observed = false;
    let error = verify_installed_steam_package_with_progress(
        &fixture.root,
        &fixture.steamcmd,
        7,
        "17",
        SteamPackageVerificationOptions {
            cancellation: Some(&signal),
            ..Default::default()
        },
        |_| signal.store(true, Ordering::Release),
        |_, _, _| {
            observed = true;
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    assert!(!observed);
}

#[cfg(windows)]
#[test]
fn verification_handles_deny_writers_and_replacements_until_the_sample_finishes() {
    let fixture = Fixture::new();
    let path = fixture.root.join("x.bin");
    let handle = open_read(&path).unwrap();
    assert!(fs::OpenOptions::new().write(true).open(&path).is_err());
    assert!(fs::rename(&path, fixture.root.join("replaced.bin")).is_err());
    drop(handle);
    fs::write(&path, b"xyz").unwrap();
    assert!(fixture.verify().is_err());
}

#[cfg(windows)]
#[test]
fn linked_official_parent_is_rejected_without_following_it() {
    let fixture = Fixture::new();
    let linked = fixture.base.join("link");
    let output = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&linked)
        .arg(&fixture.root)
        .output()
        .unwrap();
    assert!(output.status.success());
    let result = verify_installed_steam_package(&linked, &fixture.steamcmd, 7, "17", None);
    fs::remove_dir(&linked).unwrap();
    assert!(result.is_err());
}
