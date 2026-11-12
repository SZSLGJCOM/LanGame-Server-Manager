use super::*;

// Independent depot fixture: one optional defaults directory and its abc file.
const OPTIONAL_DIRECTORY: &str = "d017f6713a0000000a2a0a0e64656661756c74732f782e62696e100318002a14a9993e364706816aba3e25717850c26c9cd0d89d0a0c0a0864656661756c74731840be12481f0e0000000807100920002803489987b4830f17b8811b00000000ab15c432";

fn verify_optional(fixture: &Fixture) -> io::Result<VerifiedSteamPackage> {
    verify_installed_steam_package_with_progress(
        &fixture.root,
        &fixture.steamcmd,
        7,
        "17",
        SteamPackageVerificationOptions {
            optional_path: Some(&|_| true),
            ..Default::default()
        },
        |_| {},
        |_, _, _| Ok(()),
    )
}

fn verify_available_defaults(fixture: &Fixture) -> io::Result<VerifiedSteamPackage> {
    verify_installed_steam_package_with_progress(
        &fixture.root,
        &fixture.steamcmd,
        7,
        "17",
        SteamPackageVerificationOptions {
            optional_path: Some(&|_| true),
            allow_missing_depots: true,
            ..Default::default()
        },
        |_| {},
        |_, _, _| Ok(()),
    )
}

#[test]
fn incremental_defaults_can_skip_missing_declared_shared_depot_cache() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("steamapps/appmanifest_7.acf"),
        Fixture::acf(r#""SharedDepots" { "8" "8" }"#, "17", "4"),
    )
    .unwrap();
    fs::write(fixture.root.join("steamapps/appmanifest_8.acf"), r#""AppState" { "appid" "8" "StateFlags" "4" "InstalledDepots" { "8" { "manifest" "10" } } }"#).unwrap();
    let error = fixture.verify().unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert!(error.to_string().contains("8_10.manifest"));
    assert_eq!(
        verify_optional(&fixture).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
    let available = verify_available_defaults(&fixture).unwrap();
    assert_eq!(available.files["x.bin"], ABC_SHA256);
    assert_eq!(available.files.len(), 3);
    assert!(available.files.contains_key("steamapps/appmanifest_7.acf"));
    assert!(available.files.contains_key("steamapps/appmanifest_8.acf"));

    // These bytes are official only in the missing shared depot. Its declaration
    // alone cannot certify them; the exact cached inventory must be available.
    fs::write(fixture.root.join("x.bin"), b"updated").unwrap();
    assert!(
        !verify_available_defaults(&fixture)
            .unwrap()
            .files
            .contains_key("x.bin")
    );
    fs::write(
        fixture.steamcmd.join("depotcache/8_10.manifest"),
        bytes(OVERLAP),
    )
    .unwrap();
    assert!(
        verify_available_defaults(&fixture)
            .unwrap()
            .files
            .contains_key("x.bin")
    );
}

#[test]
fn missing_depot_tolerance_keeps_acf_and_build_checks_strict() {
    let fixture = Fixture::new();
    let acf = fixture.root.join("steamapps/appmanifest_7.acf");
    for (build, flags, detail) in [("18", "4", "build differs"), ("17", "6", "incomplete")] {
        fs::write(&acf, Fixture::acf("", build, flags)).unwrap();
        let error = verify_available_defaults(&fixture).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains(detail));
    }
    fs::write(
        &acf,
        Fixture::acf(r#""SharedDepots" { "8" "8" }"#, "17", "4"),
    )
    .unwrap();
    assert_eq!(
        verify_available_defaults(&fixture).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
}

#[test]
fn missing_depot_tolerance_does_not_hide_corrupt_available_manifests() {
    let fixture = Fixture::new();
    let mut damaged = bytes(ORIGINAL);
    let filename = damaged
        .windows(5)
        .position(|part| part == b"x.bin")
        .unwrap();
    damaged[filename] = b'y';
    fs::write(fixture.steamcmd.join("depotcache/7_9.manifest"), damaged).unwrap();
    let error = verify_available_defaults(&fixture).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("CRC mismatch"));
}

#[test]
fn optional_defaults_require_official_bytes_and_report_only_measured_work() {
    let cases: [Option<&[u8]>; 4] = [None, Some(b"different size"), Some(b"xyz"), Some(b"abc")];
    for content in cases {
        let fixture = Fixture::new();
        let path = fixture.root.join("x.bin");
        match content {
            Some(bytes) => fs::write(&path, bytes).unwrap(),
            None => fs::remove_file(&path).unwrap(),
        }
        let mut reports = Vec::new();
        let mut observed = BTreeMap::new();
        let verified = verify_installed_steam_package_with_progress(
            &fixture.root,
            &fixture.steamcmd,
            7,
            "17",
            SteamPackageVerificationOptions {
                optional_path: Some(&|path| path == "x.bin"),
                ..Default::default()
            },
            |progress| reports.push(progress),
            |relative, _, digest| {
                observed.insert(relative.to_owned(), digest.to_owned());
                Ok(())
            },
        )
        .unwrap();
        let matched = content == Some(b"abc".as_slice());
        assert_eq!(verified.files.contains_key("x.bin"), matched);
        if matched {
            assert_eq!(verified.files["x.bin"], ABC_SHA256);
        }
        assert_eq!(observed, verified.files);
        assert_eq!(fs::read(&path).ok().as_deref(), content);
        let read_payload = content.is_some_and(|bytes| bytes.len() == 3);
        let end = reports.last().unwrap();
        assert_eq!(reports.first().unwrap().completed_bytes, 0);
        assert_eq!(
            end.total_bytes,
            Fixture::acf("", "17", "4").len() as u64 + if read_payload { 3 } else { 0 }
        );
        assert_eq!(end.completed_bytes, end.total_bytes);
        assert_eq!(end.total_files, if read_payload { 2 } else { 1 });
        assert_eq!(end.completed_files, end.total_files);
    }
}

#[test]
fn include_filter_takes_precedence_and_optional_acfs_remain_strict() {
    let fixture = Fixture::new();
    let verify = || {
        verify_installed_steam_package_with_progress(
            &fixture.root,
            &fixture.steamcmd,
            7,
            "17",
            SteamPackageVerificationOptions {
                include_path: Some(&|_| false),
                optional_path: Some(&|_| true),
                ..Default::default()
            },
            |_| {},
            |_, _, _| Ok(()),
        )
    };
    let verified = verify().unwrap();
    assert!(!verified.files.contains_key("x.bin"));
    assert_eq!(verified.files.len(), 1);
    assert!(verified.files.contains_key("steamapps/appmanifest_7.acf"));
    fs::write(
        fixture.root.join("steamapps/appmanifest_7.acf"),
        Fixture::acf("", "17", "1026"),
    )
    .unwrap();
    assert!(verify().unwrap_err().to_string().contains("incomplete"));
}

#[test]
fn optional_directories_are_added_only_for_admitted_files() {
    let fixture = Fixture::new();
    fs::write(
        fixture.steamcmd.join("depotcache/7_9.manifest"),
        bytes(OPTIONAL_DIRECTORY),
    )
    .unwrap();
    let missing = verify_optional(&fixture).unwrap();
    assert!(!missing.directories.contains("defaults"));
    fs::create_dir(fixture.root.join("defaults")).unwrap();
    let path = fixture.root.join("defaults/x.bin");
    fs::write(&path, b"abc").unwrap();
    let matched = verify_optional(&fixture).unwrap();
    assert_eq!(matched.files["defaults/x.bin"], ABC_SHA256);
    assert!(matched.directories.contains("defaults"));
    fs::write(&path, b"xyz").unwrap();
    let changed = verify_optional(&fixture).unwrap();
    assert!(!changed.files.contains_key("defaults/x.bin"));
    assert!(!changed.directories.contains("defaults"));
    assert_eq!(fs::read(&path).unwrap(), b"xyz");
}

#[test]
fn optional_defaults_accept_only_declared_shared_depot_alternatives() {
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
    assert!(
        verify_optional(&fixture)
            .unwrap()
            .files
            .contains_key("x.bin")
    );
    fs::write(fixture.root.join("x.bin"), b"unknown").unwrap();
    assert!(
        !verify_optional(&fixture)
            .unwrap()
            .files
            .contains_key("x.bin")
    );
    assert_eq!(fs::read(fixture.root.join("x.bin")).unwrap(), b"unknown");
}

#[test]
fn optional_file_changes_after_measurement_still_fail() {
    let fixture = Fixture::new();
    let mut changed = false;
    let mut observations = 0;
    let error = verify_installed_steam_package_with_progress(
        &fixture.root,
        &fixture.steamcmd,
        7,
        "17",
        SteamPackageVerificationOptions {
            optional_path: Some(&|_| true),
            ..Default::default()
        },
        |_| {
            if !changed {
                changed = true;
                fs::write(fixture.root.join("x.bin"), b"changed after measurement").unwrap();
            }
        },
        |_, _, _| {
            observations += 1;
            Ok(())
        },
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("size changed before verification")
    );
    assert_eq!(observations, 0);
}

#[test]
fn optional_files_propagate_cancellation_and_evidence_errors() {
    let fixture = Fixture::new();
    let signal = AtomicBool::new(false);
    let error = verify_installed_steam_package_with_progress(
        &fixture.root,
        &fixture.steamcmd,
        7,
        "17",
        SteamPackageVerificationOptions {
            cancellation: Some(&signal),
            optional_path: Some(&|_| true),
            allow_missing_depots: true,
            ..Default::default()
        },
        |_| signal.store(true, Ordering::Release),
        |_, _, _| panic!("cancelled verification cannot publish evidence"),
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    let error = verify_installed_steam_package_with_progress(
        &fixture.root,
        &fixture.steamcmd,
        7,
        "17",
        SteamPackageVerificationOptions {
            optional_path: Some(&|_| true),
            allow_missing_depots: true,
            ..Default::default()
        },
        |_| {},
        |_, _, _| Err(io::Error::other("evidence unavailable")),
    )
    .unwrap_err();
    assert!(error.to_string().contains("evidence unavailable"));
}

#[cfg(windows)]
#[test]
fn optional_files_do_not_suppress_sharing_errors() {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION;

    let fixture = Fixture::new();
    let handle = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(fixture.root.join("x.bin"))
        .unwrap();
    let result = verify_optional(&fixture);
    let incremental_result = verify_available_defaults(&fixture);
    drop(handle);
    assert_eq!(
        result.unwrap_err().raw_os_error(),
        Some(ERROR_SHARING_VIOLATION as i32)
    );
    assert_eq!(
        incremental_result.unwrap_err().raw_os_error(),
        Some(ERROR_SHARING_VIOLATION as i32)
    );
}

#[cfg(windows)]
#[test]
fn missing_optional_file_cannot_hide_a_linked_parent() {
    let fixture = Fixture::new();
    fs::write(
        fixture.steamcmd.join("depotcache/7_9.manifest"),
        bytes(OPTIONAL_DIRECTORY),
    )
    .unwrap();
    let external = fixture.base.join("external");
    fs::create_dir(&external).unwrap();
    let linked = fixture.root.join("defaults");
    let output = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&linked)
        .arg(&external)
        .output()
        .unwrap();
    assert!(output.status.success());
    let result = verify_optional(&fixture);
    fs::remove_dir(&linked).unwrap();
    assert!(result.unwrap_err().to_string().contains("linked"));
    assert!(external.is_dir());
}
