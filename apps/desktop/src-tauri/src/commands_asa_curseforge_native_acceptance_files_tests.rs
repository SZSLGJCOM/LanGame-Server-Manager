use super::*;

#[test]
fn native_acceptance_projects_and_seeds_the_actual_sdk_id_field_shape() {
    let root =
        std::env::temp_dir().join(format!("asa-cf-library-fixture-{}", uuid::Uuid::new_v4()));
    let source = root.join("source");
    let destination = root.join("destination");
    let payload = source.join("Mods/83374/1715971_9067441");
    let user_data = source.join("ModsUserData/83374");
    fs::create_dir_all(&payload).unwrap();
    fs::create_dir_all(source.join("Mods/83374/999_1")).unwrap();
    fs::create_dir_all(&user_data).unwrap();
    fs::create_dir(&destination).unwrap();
    fs::write(payload.join("package.bin"), b"fixture cached payload").unwrap();
    fs::write(
        source.join("Mods/83374/999_1/unselected.bin"),
        b"unselected fixture",
    )
    .unwrap();
    let manifest = user_data.join("library.json");
    let bytes = serde_json::to_vec(&json!({"installedMods":[{
        "iD":"fixture-installed-mod-guid", "users":[], "status":"Normal",
        "pathOnDisk":"83374/1715971_9067441",
        "details":{"iD":1715971,"gameId":83374,"mainFileId":9067441},
        "installedFile":{"iD":9067441},"latestUpdatedFile":{"iD":9067441}
    }]}))
    .unwrap();
    fs::write(&manifest, &bytes).unwrap();
    check_source_user_data(&source.join("ModsUserData")).unwrap();
    let projection = library_projection(&source, &[1715971]).unwrap();
    assert_eq!(
        projection,
        vec![json!({"project_id":1715971,
        "installed_file_id":9067441,"latest_updated_file_id":9067441,
        "main_file_id":9067441,"status":"Normal","path_on_disk":"83374/1715971_9067441"})]
    );
    let bytes = [b"\xEF\xBB\xBF".as_slice(), bytes.as_slice()].concat();
    fs::write(&manifest, &bytes).unwrap();
    assert_eq!(library_projection(&source, &[1715971]).unwrap(), projection);
    check_source_user_data(&source.join("ModsUserData")).unwrap();
    seed_cache(&source, &destination, &[1715971]).unwrap();
    assert_eq!(
        library_projection(&destination, &[1715971]).unwrap(),
        projection
    );
    assert_eq!(fs::read(&manifest).unwrap(), bytes);
    assert!(fs::read(&manifest).unwrap().starts_with(b"\xEF\xBB\xBF"));
    assert_eq!(
        fs::read(destination.join("Mods/83374/1715971_9067441/package.bin")).unwrap(),
        b"fixture cached payload"
    );
    assert!(!destination.join("Mods/83374/999_1").exists());
    assert!(!destination.join("ModsUserData/83374/local").exists());
    fs::write(user_data.join("shared_info.json"), b"{}").unwrap();
    assert!(check_source_user_data(&source.join("ModsUserData")).is_err());
    fs::remove_file(user_data.join("shared_info.json")).unwrap();
    fs::create_dir(user_data.join("local")).unwrap();
    fs::write(user_data.join("local/user_info.json"), b"{}").unwrap();
    assert!(check_source_user_data(&source.join("ModsUserData")).is_err());
    plain_path(&root).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_acceptance_evidence_redacts_api_error_details_and_uses_current_records() {
    let root = std::env::temp_dir().join(format!("asa-cf-report-fixture-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let path = root.join("native.log");
    let line =
        |ms: &str, detail: &str| format!("[2026.10.17-10.00.00:{ms}][  0]LogCFCore: {detail}\n");
    let cutoff = app_storage::ark_native_log_message(&line("100", "cutoff"))
        .unwrap()
        .0;
    fs::write(
        &path,
        [
            line("099", "Mod valid: Old (1715971)"),
            line("110", "Warning: Mod valid: Warning (941450)"),
            line("111", "Error: Mod valid: Error (941450)"),
            line("120", "Mod valid: Current (941450)"),
            line(
                "130",
                "Error querying server mods: https://example.invalid/mods?signature=fixture",
            ),
        ]
        .concat(),
    )
    .unwrap();
    let (selected, valid, terminal) = native_evidence(&path, cutoff).unwrap();
    assert_eq!(valid, BTreeSet::from([941450]));
    assert!(terminal);
    let output = serde_json::to_string(&selected).unwrap();
    assert!(!output.contains("example.invalid"));
    assert!(!output.contains("signature"));
    assert!(!output.contains("Old (1715971)"));
    assert!(output.contains("error_querying_server_mods"));
    assert!(
        !safe_error("query failed https://example.invalid?signature=fixture")
            .contains("example.invalid")
    );
    plain_path(&root).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_acceptance_terminal_evidence_requires_the_documented_complete_record() {
    for malformed in [
        "Missing installed mods - (",
        "Missing installed mods - (2/not-a-count)",
        "Missing installed mods - (2/3) trailing",
        "Detected an unavailable mod: Name (2147483647)",
        "Mod: Name (941450) doesn't contain it's main file (bad)!",
    ] {
        assert_eq!(terminal_marker(malformed), None, "{malformed}");
    }
    assert_eq!(
        terminal_marker("Missing installed mods - (2/3)"),
        Some("missing_installed_mods")
    );
    assert_eq!(
        terminal_marker(
            "Detected an unavailable mod: Name (2147483647)! Ensuring mod is deleted..."
        ),
        Some("unavailable_mod")
    );
    assert_eq!(
        terminal_marker("Mod: Name (941450) doesn't contain it's main file (8828067)!"),
        Some("missing_main_file")
    );
}

#[test]
fn native_acceptance_recognizes_only_the_current_exact_asa_mod_install_failure() {
    let root = std::env::temp_dir().join(format!("asa-mod-error-fixture-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let path = root.join("native.log");
    let line = |ms: &str, message: &str| format!("[2026.10.17-16.14.39:{ms}][ 17]{message}\n");
    let error = "ASAMods: Error: Not all mods were installed. Check the log for CFCore errors.";
    let cutoff = app_storage::ark_native_log_message(&line("300", "cutoff"))
        .unwrap()
        .0;
    let current_valid = [
        line("301", "LogCFCore: Mod valid: First (1715971)"),
        line("302", "LogCFCore: Mod valid: Second (941450)"),
    ]
    .concat();
    fs::write(&path, format!("{current_valid}{}If you have any Custom Cosmetics https://example.invalid/private\nMods not installed: 2147483647\n", line("303", error))).unwrap();
    let (selected, valid, terminal) = native_evidence(&path, cutoff).unwrap();
    assert_eq!(valid, BTreeSet::from([1715971, 941450]));
    assert!(terminal);
    let asa = selected
        .iter()
        .find(|record| record["category"] == "ASAMods")
        .unwrap();
    assert_eq!(
        asa,
        &json!({"timestamp_ms":cutoff + 3, "category":"ASAMods",
        "level":"Error", "marker":"missing_requested_mods"})
    );
    let output = serde_json::to_string(&selected).unwrap();
    assert!(!output.contains("example.invalid"));
    assert!(!output.contains("Custom Cosmetics"));
    assert!(!output.contains("Mods not installed"));
    for (ms, rejected) in [
        ("299", error),
        (
            "303",
            "ASAMods: Warning: Not all mods were installed. Check the log for CFCore errors.",
        ),
        ("303", "ASAMods: Error: Not all mods were installed."),
        (
            "303",
            "ASAMods: Error: Not all mods were installed. Check the log for CFCore errors. trailing",
        ),
        (
            "303",
            "LogOther: ASAMods: Error: Not all mods were installed. Check the log for CFCore errors.",
        ),
    ] {
        fs::write(&path, format!("{current_valid}{}", line(ms, rejected))).unwrap();
        let (selected, valid, terminal) = native_evidence(&path, cutoff).unwrap();
        assert!(!terminal, "{ms} {rejected}");
        assert_eq!(valid, BTreeSet::from([1715971, 941450]));
        assert!(
            selected
                .iter()
                .all(|record| record["category"] != "ASAMods")
        );
    }
    plain_path(&root).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_acceptance_requires_current_main_file_but_allows_stale_warm_latest_field() {
    let cache = vec![
        json!({"status":"Normal","installed_file_id":9067441,"main_file_id":9067441,"latest_updated_file_id":1}),
        json!({"status":"Normal","installed_file_id":8828067,"main_file_id":8828067,"latest_updated_file_id":2}),
    ];
    assert!(require_current_installed(&cache).is_ok());
    let mut stale = cache;
    stale[0]["installed_file_id"] = json!(1);
    assert!(require_current_installed(&stale).is_err());
}
