use super::*;

fn read_market(fixture: &RimWorldFixture) -> Value {
    serde_json::from_slice(&fs::read(fixture.install.join("Configs/Actions/Market.json")).unwrap())
        .unwrap()
}

fn remove_recorded_archive_hash(root: &Path) {
    let receipt = root.join(".langame-clean-package.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
    manifest.as_object_mut().unwrap().remove("archive_sha256");
    fs::write(receipt, serde_json::to_vec(&manifest).unwrap()).unwrap();
}

#[test]
fn new_market_and_explicit_overrides_preserve_native_fields() {
    let fixture = RimWorldFixture::new();
    materialize_module_support_files(&fixture.context(&Map::new())).unwrap();
    assert_eq!(read_market(&fixture)["UseGlobalServer"], true);

    let path = fixture.install.join("Configs/Actions/Market.json");
    let native = br#"{"UseGlobalServer":false,"Future":{"kept":[3]},"IsEnabled":false}"#;
    fs::write(&path, native).unwrap();
    materialize_module_support_files(&fixture.context(&Map::new())).unwrap();
    assert_eq!(fs::read(&path).unwrap(), native);
    for mode in [true, false] {
        let settings =
            json!({"action_market_use_global_server":mode, "action_market_cooldown":700});
        materialize_module_support_files(&fixture.context(settings.as_object().unwrap())).unwrap();
        assert_eq!(
            read_market(&fixture),
            json!({
                "UseGlobalServer":mode, "Future":{"kept":[3]}, "IsEnabled":false, "Cooldown":700
            })
        );
    }

    fs::write(&path, br#"{"UseGlobalServer":"false"}"#).unwrap();
    assert!(materialize_module_support_files(&fixture.context(&Map::new())).is_err());
    assert_eq!(read_market(&fixture)["UseGlobalServer"], "false");
}

#[test]
fn earlier_action_file_or_imported_local_ledger_keeps_the_local_market() {
    for old_action in [true, false] {
        let fixture = RimWorldFixture::new();
        if old_action {
            fs::create_dir_all(fixture.install.join("Configs/Actions")).unwrap();
            fs::write(
                fixture.install.join("Configs/Actions/Market.json"),
                br#"{"MinimumPrice":90}"#,
            )
            .unwrap();
        } else {
            fs::create_dir_all(fixture.install.join("Assets")).unwrap();
            fs::write(
                fixture.install.join("Assets/Market.json"),
                br#"{"Entries":[]}"#,
            )
            .unwrap();
        }
        for _ in 0..2 {
            materialize_module_support_files(&fixture.context(&Map::new())).unwrap();
            let market = read_market(&fixture);
            assert_eq!(market["UseGlobalServer"], false);
            if old_action {
                assert_eq!(market["MinimumPrice"], 90);
            }
        }
    }
}

#[test]
fn an_unknown_image_cannot_withdraw_listings_even_with_a_package_receipt() {
    let fixture = RimWorldFixture::new();
    let modules = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules");
    let current = app_modules::discover_modules(&modules)
        .unwrap()
        .into_iter()
        .find(|module| module.summary.id == "rimworld")
        .unwrap();
    let mut old = current.clone();
    old.install
        .as_mut()
        .unwrap()
        .download_integrity_windows
        .as_mut()
        .unwrap()
        .sha256 = "0".repeat(64);
    for (name, descriptor, supported) in [("old", &old, false), ("current", &current, true)] {
        let root = fixture.root.join(name);
        fs::create_dir(&root).unwrap();
        fs::write(root.join("RTServer.exe"), name).unwrap();
        crate::record_library_program_baseline(&root, descriptor, true, None).unwrap();
        let check = |feature| {
            crate::templates::rimworld::validate_release_before_start(
                &root,
                Some(&current),
                feature,
            )
        };
        assert_eq!(check(Some("The cross-server market")).is_ok(), supported);
        assert_eq!(check(Some("Player save rollback")).is_ok(), supported);

        fs::create_dir(root.join("Assets")).unwrap();
        fs::write(
            root.join("Assets/Market.json"),
            br#"{"Entries":[{"Owner":"seller"}]}"#,
        )
        .unwrap();
        assert!(check(None).is_err());
        assert!(check(Some("The cross-server market")).is_err());
        fs::write(root.join("Assets/Market.json"), br#"{"Entries":[]}"#).unwrap();
        assert!(check(None).is_ok());
    }
}

#[test]
fn market_recovery_accepts_only_reviewed_original_images() {
    let evidence: Value = serde_json::from_str(include_str!(
        "../../../modules/rimworld/reference-fixtures/2026-11-04-local_market_recovery.json"
    ))
    .unwrap();
    let old = evidence["old_packages"].as_array().unwrap();
    assert_eq!(old.len(), 3);
    for package in old {
        assert_eq!(
            crate::templates::rimworld::old_market_archive(
                package["executable_sha256"].as_str().unwrap()
            ),
            package["archive_sha256"].as_str()
        );
    }
    let current = evidence["replacement_executable_sha256"].as_str().unwrap();
    assert!(crate::templates::rimworld::old_market_archive(current).is_none());
}

#[test]
#[ignore = "requires official RTServer.exe images in LANGAME_RIMWORLD_MARKET_PACKAGES/<tag>"]
fn official_rimworld_images_preserve_old_market_ledgers_on_start() {
    let packages = PathBuf::from(std::env::var_os("LANGAME_RIMWORLD_MARKET_PACKAGES").unwrap());
    let evidence: Value = serde_json::from_str(include_str!(
        "../../../modules/rimworld/reference-fixtures/2026-11-04-local_market_recovery.json"
    ))
    .unwrap();
    let modules = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules");
    let current = app_modules::discover_modules(&modules)
        .unwrap()
        .into_iter()
        .find(|module| module.summary.id == "rimworld")
        .unwrap();
    let fixture = RimWorldFixture::new();
    let active = br#"{"Entries":[{"Identifier":"probe","Owner":"seller"}]}"#;
    let replacements = packages.join("26.11.2.1/RTServer.exe");
    for package in evidence["old_packages"].as_array().unwrap() {
        let tag = package["tag"].as_str().unwrap();
        let root = fixture.root.join(tag);
        let file = root.join("RTServer.exe");
        let ledger = root.join("Assets/Market.json");
        fs::create_dir_all(ledger.parent().unwrap()).unwrap();
        fs::copy(packages.join(tag).join("RTServer.exe"), &file).unwrap();
        fs::write(&ledger, active).unwrap();
        let check = |feature| {
            crate::templates::rimworld::validate_release_before_start(
                &root,
                Some(&current),
                feature,
            )
        };
        assert!(check(None).is_ok(), "{tag} without receipts");
        assert!(
            crate::templates::rimworld::validate_release_before_start(&root, None, None).is_ok()
        );
        assert!(check(Some("Player save rollback")).is_err());
        let mut old = current.clone();
        old.install
            .as_mut()
            .unwrap()
            .download_integrity_windows
            .as_mut()
            .unwrap()
            .sha256 = package["archive_sha256"].as_str().unwrap().to_owned();
        crate::record_library_program_baseline(&root, &old, true, None).unwrap();
        assert!(check(None).is_ok(), "{tag} with matching receipts");
        remove_recorded_archive_hash(&root);
        assert!(
            check(None).is_ok(),
            "{tag} with an early receipt lacking the archive SHA"
        );
        for damaged in [
            br#"{"Entries":[null]}"#.as_slice(),
            br#"{"Entries":{}}"#,
            b"truncated",
        ] {
            fs::write(&ledger, damaged).unwrap();
            assert!(check(None).is_err());
            assert_eq!(fs::read(&ledger).unwrap(), damaged);
        }
        fs::write(&ledger, active).unwrap();
        {
            use std::io::Write;
            let mut changed = fs::OpenOptions::new().write(true).open(&file).unwrap();
            changed.write_all(b"!").unwrap();
        }
        assert!(
            check(None).is_err(),
            "modified image with the original length"
        );
        let other = if tag == "26.8.31.1" {
            "26.10.15.1"
        } else {
            "26.8.31.1"
        };
        fs::copy(packages.join(other).join("RTServer.exe"), &file).unwrap();
        assert!(check(None).is_err(), "mismatched old image and inventory");
        fs::copy(&replacements, &file).unwrap();
        assert!(
            check(None).is_err(),
            "new image manually installed over old receipt"
        );
        assert_eq!(fs::read(&ledger).unwrap(), active);
    }
    let root = &fixture.install;
    let file = root.join("RTServer.exe");
    fs::copy(replacements, &file).unwrap();
    fs::create_dir_all(root.join("Assets")).unwrap();
    let ledger = root.join("Assets/Market.json");
    fs::write(&ledger, active).unwrap();
    let check =
        || crate::templates::rimworld::validate_release_before_start(root, Some(&current), None);
    assert!(check().is_err(), "new image without receipts");
    crate::record_library_program_baseline(root, &current, true, None).unwrap();
    assert!(check().is_err(), "new image with current receipt");
    remove_recorded_archive_hash(root);
    assert!(
        check().is_err(),
        "new image with an early receipt lacking the archive SHA"
    );
    assert_eq!(fs::read(&ledger).unwrap(), active);
    fs::write(&ledger, br#"{"Entries":[]}"#).unwrap();
    assert!(check().is_ok());
}

#[tokio::test]
async fn missing_descriptor_keeps_market_and_feature_checks_on_the_prestart_path() {
    let fixture = RimWorldFixture::new();
    let settings = Map::new();
    let context = fixture.context(&settings);
    let lock = crate::instance_settings_lock::acquire_instance_settings_mutation_lock(
        &fixture.paths,
        context.instance_id,
    )
    .unwrap();
    let ledger = fixture.install.join("Assets/Market.json");
    fs::create_dir_all(ledger.parent().unwrap()).unwrap();
    let active = br#"{"Entries":[{"Owner":"seller"}]}"#;
    fs::write(&ledger, active).unwrap();
    assert!(
        crate::templates::apply_module_prestart_support(&context, &lock, None)
            .await
            .is_err()
    );
    assert_eq!(fs::read(&ledger).unwrap(), active);
    fs::write(&ledger, br#"{"Entries":[]}"#).unwrap();
    crate::templates::apply_module_prestart_support(&context, &lock, None)
        .await
        .unwrap();
    for key in [
        "action_market_use_global_server",
        "action_rollback_is_enabled",
    ] {
        let requested = Map::from_iter([(key.to_owned(), Value::Bool(true))]);
        let error = crate::templates::apply_module_prestart_support(
            &fixture.context(&requested),
            &lock,
            None,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("requires the verified Together"));
    }
}
