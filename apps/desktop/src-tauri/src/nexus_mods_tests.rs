use super::*;

fn account(premium: bool) -> api::NexusAccount {
    serde_json::from_value(json!({"user_id": 42, "name": "Fixture User", "is_premium": premium}))
        .unwrap()
}

fn file(name: &str, bytes: u64) -> api::NexusFile {
    serde_json::from_value(
        json!({"file_id": 88, "name": "Server Mod", "file_name": name,
        "version": "1.0", "size_in_bytes": bytes, "category_id": 1, "category_name": "MAIN"}),
    )
    .unwrap()
}

#[test]
fn nexus_references_bind_ids_and_official_pages_to_the_declared_game() {
    for reference in [
        "12",
        "https://www.nexusmods.com/abioticfactor/mods/12",
        "https://nexusmods.com/games/abioticfactor/mods/12?tab=files",
        "nxm://abioticfactor/mods/12/files/88",
    ] {
        let parsed = parse_reference(reference, "abioticfactor").unwrap();
        assert_eq!(parsed.mod_id, "12");
        assert_eq!(parsed.domain, "abioticfactor");
    }
    for reference in [
        "https://evil.example/abioticfactor/mods/12",
        "http://www.nexusmods.com/abioticfactor/mods/12",
        "https://www.nexusmods.com.evil.example/abioticfactor/mods/12",
        "https://user@www.nexusmods.com/abioticfactor/mods/12",
        "https://www.nexusmods.com/enshrouded/mods/12",
        "nxm://enshrouded/mods/12/files/88",
        "https://www.nexusmods.com/abioticfactor/mods/12?file_id=88&file_id=89",
        "0",
        "18446744073709551616",
    ] {
        assert!(parse_reference(reference, "abioticfactor").is_err());
    }
}

#[test]
fn nexus_file_links_preselect_only_the_explicit_file_and_never_infer_latest() {
    let parsed = parse_reference(
        "https://www.nexusmods.com/abioticfactor/mods/12?tab=files&file_id=88",
        "abioticfactor",
    )
    .unwrap();
    assert_eq!(parsed.file_id.as_deref(), Some("88"));
    assert!(!parsed.is_nxm);
    let mut other_file = file("Other.zip", 10);
    other_file.file_id = 89;
    assert!(
        file_choice(&other_file, &["zip".into()], &parsed)
            .unwrap()
            .downloadable
    );
    assert!(
        parse_reference("12", "abioticfactor")
            .unwrap()
            .file_id
            .is_none()
    );
    assert!(
        parse_reference(
            "https://www.nexusmods.com/abioticfactor/mods/12/files/88?file_id=89",
            "abioticfactor"
        )
        .is_err()
    );
}

#[test]
fn nexus_authorized_free_links_are_file_account_and_expiry_bound() {
    let parsed = parse_reference(
        "nxm://abioticfactor/mods/12/files/88?key=fixture-download-secret&expires=200&user_id=42",
        "abioticfactor",
    )
    .unwrap();
    parsed.validate_authorization(&account(false), 199).unwrap();
    assert!(
        parsed
            .validate_authorization(&account(false), 200)
            .err()
            .unwrap()
            .contains("nexus_download_expired")
    );
    let mut other_account = account(true);
    other_account.user_id = 43;
    assert!(
        parsed
            .validate_authorization(&other_account, 199)
            .err()
            .unwrap()
            .contains("nexus_auth_invalid")
    );
    let choice = file_choice(&file("Server.zip", 5), &["zip".into()], &parsed).unwrap();
    assert!(choice.downloadable);
    let mut other_file = file("Server.zip", 5);
    other_file.file_id = 89;
    assert!(
        !file_choice(&other_file, &["zip".into()], &parsed)
            .unwrap()
            .downloadable
    );
    let keyless = parse_reference("nxm://abioticfactor/mods/12/files/88", "abioticfactor").unwrap();
    assert!(
        !file_choice(&other_file, &["zip".into()], &keyless)
            .unwrap()
            .downloadable
    );
}

#[test]
fn nexus_partial_duplicate_or_secret_website_authorization_is_rejected_without_echo() {
    for reference in [
        "nxm://abioticfactor/mods/12/files/88?key=fixture-download-secret&expires=200",
        "nxm://abioticfactor/mods/12/files/88?key=fixture-download-secret&key=second&expires=200&user_id=42",
        "https://www.nexusmods.com/abioticfactor/mods/12?key=fixture-download-secret",
    ] {
        let error = parse_reference(reference, "abioticfactor").err().unwrap();
        assert!(!error.contains("fixture-download-secret"));
        assert!(!error.contains("nxm://"));
    }
}

#[test]
fn nexus_review_filters_archives_oversize_and_unsafe_filenames() {
    let reference = parse_reference("12", "abioticfactor").unwrap();
    for filename in [
        "Mod.rar",
        "Mod.7z",
        "../Mod.zip",
        "C:Mod.zip",
        "NUL.zip",
        "Mod.zip ",
        "Mod.exe",
    ] {
        assert!(
            !file_choice(&file(filename, 10), &["zip".into()], &reference,)
                .unwrap()
                .downloadable
        );
    }
    let oversized = file("Mod.zip", MANUAL_MOD_SITE_DOWNLOAD_MAX_BYTES + 1);
    let choice = file_choice(&oversized, &["zip".into()], &reference).unwrap();
    assert!(!choice.downloadable);
    assert!(choice.unavailable_reason.unwrap().contains("256 MiB"));
    assert!(
        file_choice(&file("Mod.zip", 10), &["zip".into()], &reference,)
            .unwrap()
            .downloadable
    );
    assert!(
        file_choice(&file("Mod.PAK", 10), &["pak".into()], &reference,)
            .unwrap()
            .downloadable
    );
    for category in [None, Some(6), Some(7), Some(999)] {
        let mut unavailable = file("Mod.zip", 10);
        unavailable.category_id = category;
        assert!(
            !file_choice(&unavailable, &["zip".into()], &reference)
                .unwrap()
                .downloadable
        );
    }
    let mut historical = file("Mod.zip", 10);
    historical.category_id = Some(4);
    assert!(
        file_choice(&historical, &["zip".into()], &reference)
            .unwrap()
            .downloadable
    );
}

#[test]
fn nexus_review_serialization_contains_no_download_authorization() {
    let reference = parse_reference(
        "nxm://abioticfactor/mods/12/files/88?key=fixture-download-secret&expires=200&user_id=42",
        "abioticfactor",
    )
    .unwrap();
    let review = NexusModReview {
        reference_index: 0,
        game_domain: reference.domain.clone(),
        mod_id: reference.mod_id.clone(),
        mod_name: "Mod".into(),
        page_url: reference.page_url(),
        files: vec![file_choice(&file("Server.zip", 10), &["zip".into()], &reference).unwrap()],
        selected_file_id: reference.file_id.clone(),
    };
    let encoded = serde_json::to_string(&review).unwrap();
    assert!(!encoded.contains("fixture-download-secret"));
    assert!(!encoded.contains("expires"));
    assert!(!encoded.contains("user_id"));
}

#[test]
fn nexus_source_requires_an_explicit_nexus_game_domain() {
    let mut source: ModuleModSourceSpec = serde_json::from_value(json!({"provider": "nexus", "label": "Nexus", "url": "https://www.nexusmods.com/7daystodie"})).unwrap();
    assert_eq!(source_domain(&source).unwrap(), "7daystodie");
    source.url = "https://www.nexusmods.com/".into();
    assert!(source_domain(&source).is_err());
}
