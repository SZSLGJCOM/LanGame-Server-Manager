use super::*;

const TARGET: &str = "Steam_76561198000000001";
const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<adminTools><users/><blacklist>
<blacklisted platform="Steam" userid="76561198000000001" name="Alice &amp; Bob" unbandate="2036-02-29 23:59:58" reason="LanGame"/>
<blacklisted platform="Steam" userid="76561198000000009" name="Native only" unbandate="9999-12-31 00:00:00" reason="Not imported"/>
</blacklist><whitelist/><commands/></adminTools>"#;

fn receipts(target: &str) -> Vec<SevenDaysBanReceipt> {
    vec![SevenDaysBanReceipt {
        canonical_target: target.to_string(),
        unban_date: "2036-02-29 23:59:58".to_string(),
        reason: "LanGame".to_string(),
    }]
}

#[tokio::test]
async fn confirmed_native_ban_persists_without_replacing_native_or_pending_settings() {
    let (root, paths, created) =
        provision_real_module_instance("sevendaystodie", "Native ban readback").await;
    let config_path = Path::new(&created.config_file_path);
    let details = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let native_path = Path::new(&details.saves_path).join("serveradmin.xml");
    let mut before: Value = serde_json::from_slice(&fs::read(config_path).unwrap()).unwrap();
    before["unknown_document_metadata"] = json!({"keep":true});
    before["settings"]["server_name"] = json!("Pending server name");
    before["settings"]["admin_users"] = json!([
        {"platform":"Steam", "userid":"76561198000000001", "permission_level":0},
        {"platform":"Steam", "userid":"76561198000000002", "permission_level":5}
    ]);
    before["settings"]["blacklist_entries"] = json!([
        {"platform":"Steam", "userid":"76561198000000003", "name":"Other", "unbandate":"9999-12-31", "reason":"Other reason"}
    ]);
    let mut initial_config = vec![0xef, 0xbb, 0xbf];
    initial_config.extend(serde_json::to_vec_pretty(&before).unwrap());
    fs::write(config_path, initial_config).unwrap();
    fs::create_dir_all(native_path.parent().unwrap()).unwrap();
    fs::write(&native_path, XML).unwrap();
    let run = record_started_test_instance(&paths, &created.summary.id, 62001, "fixture.log")
        .await
        .unwrap();
    let result = record_seven_days_bans_from_native(
        &paths,
        &created.summary.id,
        run.run_id,
        TARGET,
        &receipts(TARGET),
    )
    .await
    .unwrap();
    let settings: Value = serde_json::from_str(&result.settings_json).unwrap();
    assert_eq!(settings["blacklist_entries"].as_array().unwrap().len(), 2);
    assert_eq!(
        settings["blacklist_entries"][1]["unbandate"],
        "2036-02-29 23:59:58"
    );
    assert_eq!(settings["blacklist_entries"][1]["name"], "Alice & Bob");
    assert_eq!(settings["blacklist_entries"][1]["reason"], "LanGame");
    assert_eq!(
        settings["blacklist_entries"][0],
        before["settings"]["blacklist_entries"][0]
    );
    assert_eq!(
        settings["admin_users"],
        json!([before["settings"]["admin_users"][1]])
    );
    assert_eq!(fs::read(&native_path).unwrap(), XML.as_bytes());
    let after: Value = serde_json::from_slice(&fs::read(config_path).unwrap()).unwrap();
    let mut expected = before.clone();
    expected["settings"]["blacklist_entries"] = settings["blacklist_entries"].clone();
    expected["settings"]["admin_users"] = settings["admin_users"].clone();
    assert_eq!(after, expected);
    let reopened = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&reopened.settings_json).unwrap(),
        settings
    );
    cleanup_root(&root);
}

#[tokio::test]
async fn native_ban_missing_target_stale_run_and_atomic_failure_leave_both_files_unchanged() {
    let (root, paths, created) =
        provision_real_module_instance("sevendaystodie", "Native ban failures").await;
    let config_path = Path::new(&created.config_file_path);
    let details = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let native_path = Path::new(&details.saves_path).join("serveradmin.xml");
    fs::create_dir_all(native_path.parent().unwrap()).unwrap();
    fs::write(&native_path, XML).unwrap();
    let before = fs::read(config_path).unwrap();
    let run = record_started_test_instance(&paths, &created.summary.id, 62002, "fixture.log")
        .await
        .unwrap();
    assert!(
        record_seven_days_bans_from_native(
            &paths,
            &created.summary.id,
            run.run_id + 1,
            TARGET,
            &receipts(TARGET)
        )
        .await
        .is_err()
    );
    assert!(
        record_seven_days_bans_from_native(
            &paths,
            &created.summary.id,
            run.run_id,
            "EOS_76561198000000001",
            &receipts("EOS_76561198000000001")
        )
        .await
        .is_err()
    );
    assert_eq!(fs::read(config_path).unwrap(), before);
    crate::atomic_file::fail_next_atomic_write_for_test(config_path);
    assert!(
        record_seven_days_bans_from_native(
            &paths,
            &created.summary.id,
            run.run_id,
            TARGET,
            &receipts(TARGET)
        )
        .await
        .is_err()
    );
    assert_eq!(fs::read(config_path).unwrap(), before);
    assert_eq!(fs::read(&native_path).unwrap(), XML.as_bytes());
    // Failure releases the settings lease and a subsequent verified write succeeds.
    record_seven_days_bans_from_native(
        &paths,
        &created.summary.id,
        run.run_id,
        TARGET,
        &receipts(TARGET),
    )
    .await
    .unwrap();
    cleanup_root(&root);
}

#[tokio::test]
async fn short_eos_native_expiry_survives_save_and_reopen() {
    let (root, paths, created) =
        provision_real_module_instance("sevendaystodie", "Native EOS ban").await;
    let details = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let native_path = Path::new(&details.saves_path).join("serveradmin.xml");
    let source = XML.replace(
        "platform=\"Steam\" userid=\"76561198000000001\"",
        "platform=\"EOS\" userid=\"abcdef12\"",
    );
    fs::create_dir_all(native_path.parent().unwrap()).unwrap();
    fs::write(&native_path, &source).unwrap();
    let run = record_started_test_instance(&paths, &created.summary.id, 62003, "fixture.log")
        .await
        .unwrap();
    record_seven_days_bans_from_native(
        &paths,
        &created.summary.id,
        run.run_id,
        "EOS_abcdef12",
        &receipts("EOS_abcdef12"),
    )
    .await
    .unwrap();
    let reopened = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let settings: Value = serde_json::from_str(&reopened.settings_json).unwrap();
    assert_eq!(settings["blacklist_entries"][0]["userid"], "abcdef12");
    assert_eq!(
        settings["blacklist_entries"][0]["unbandate"],
        "2036-02-29 23:59:58"
    );
    assert_eq!(fs::read_to_string(native_path).unwrap(), source);
    cleanup_root(&root);
}

#[tokio::test]
async fn stale_native_ban_cannot_satisfy_the_current_command_receipt() {
    let (root, paths, created) =
        provision_real_module_instance("sevendaystodie", "Stale native ban").await;
    let details = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let native_path = Path::new(&details.saves_path).join("serveradmin.xml");
    let config_path = Path::new(&created.config_file_path);
    fs::create_dir_all(native_path.parent().unwrap()).unwrap();
    let run = record_started_test_instance(&paths, &created.summary.id, 62004, "fixture.log")
        .await
        .unwrap();
    let baseline = fs::read(config_path).unwrap();
    for stale in [
        XML.replace("2036-02-29 23:59:58", "2020-01-01 00:00:00"),
        XML.replace("reason=\"LanGame\"", "reason=\"Old ban\""),
    ] {
        fs::write(&native_path, &stale).unwrap();
        let error = record_seven_days_bans_from_native(
            &paths,
            &created.summary.id,
            run.run_id,
            TARGET,
            &receipts(TARGET),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("does not match this command"));
        assert_eq!(fs::read(config_path).unwrap(), baseline);
        assert_eq!(fs::read_to_string(&native_path).unwrap(), stale);
    }
    cleanup_root(&root);
}

#[tokio::test]
async fn confirmed_family_owner_is_recorded_atomically_without_importing_other_native_bans() {
    let (root, paths, created) =
        provision_real_module_instance("sevendaystodie", "Native family ban").await;
    let details = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let native_path = Path::new(&details.saves_path).join("serveradmin.xml");
    let config_path = Path::new(&created.config_file_path);
    let mut document: Value = serde_json::from_slice(&fs::read(config_path).unwrap()).unwrap();
    document["settings"]["admin_users"] = json!([
        {"platform":"Steam", "userid":"76561198000000001", "permission_level":0},
        {"platform":"Steam", "userid":"76561198000000002", "permission_level":0},
        {"platform":"Steam", "userid":"76561198000000003", "permission_level":5}
    ]);
    document["settings"]["server_name"] = json!("Pending family settings");
    fs::write(config_path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    let baseline = fs::read(config_path).unwrap();
    fs::create_dir_all(native_path.parent().unwrap()).unwrap();
    fs::write(&native_path, XML).unwrap();
    let run = record_started_test_instance(&paths, &created.summary.id, 62005, "fixture.log")
        .await
        .unwrap();
    let mut confirmed = receipts(TARGET);
    confirmed.extend(receipts("Steam_76561198000000002"));
    // A missing owner rejects the whole write, including the already confirmed root.
    assert!(
        record_seven_days_bans_from_native(
            &paths,
            &created.summary.id,
            run.run_id,
            TARGET,
            &confirmed
        )
        .await
        .is_err()
    );
    assert_eq!(fs::read(config_path).unwrap(), baseline);
    let owner = r#"<blacklisted platform="Steam" userid="76561198000000002" name="Owner" unbandate="2036-02-29 23:59:58" reason="LanGame"/>"#;
    let source = XML.replace("</blacklist>", &format!("{owner}</blacklist>"));
    fs::write(&native_path, &source).unwrap();
    record_seven_days_bans_from_native(&paths, &created.summary.id, run.run_id, TARGET, &confirmed)
        .await
        .unwrap();
    let reopened = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let settings: Value = serde_json::from_str(&reopened.settings_json).unwrap();
    assert_eq!(settings["blacklist_entries"].as_array().unwrap().len(), 2);
    assert_eq!(
        settings["blacklist_entries"][0]["userid"],
        "76561198000000001"
    );
    assert_eq!(
        settings["blacklist_entries"][1]["userid"],
        "76561198000000002"
    );
    assert_eq!(
        settings["admin_users"],
        json!([document["settings"]["admin_users"][2]])
    );
    assert_eq!(settings["server_name"], "Pending family settings");
    assert_eq!(fs::read_to_string(&native_path).unwrap(), source);

    // End only the synthetic database run, then exercise the real stopped-instance
    // materialization path that previously discarded the native family-owner ban.
    mark_instance_process_stopped(&paths, &created.summary.id, run.run_id, Some(0), false)
        .await
        .unwrap();
    materialize_instance_configuration(&paths, &created.summary.id)
        .await
        .unwrap();
    let materialized = fs::read_to_string(&native_path).unwrap();
    let rendered =
        fs::read_to_string(config_path.parent().unwrap().join("serveradmin.xml")).unwrap();
    assert_eq!(materialized, rendered);
    assert_ne!(materialized, source);
    for userid in ["76561198000000001", "76561198000000002"] {
        let identity_attribute = format!("userid=\"{userid}\"");
        let line = materialized
            .lines()
            .find(|line| {
                line.trim_start().starts_with("<blacklisted ") && line.contains(&identity_attribute)
            })
            .unwrap_or_else(|| panic!("materialization dropped banned account {userid}"));
        assert!(line.contains("platform=\"Steam\""));
        assert!(line.contains("unbandate=\"2036-02-29 23:59:58\""));
        assert!(line.contains("reason=\"LanGame\""));
        assert!(!materialized.lines().any(|line| {
            line.trim_start().starts_with("<user ") && line.contains(&identity_attribute)
        }));
    }
    cleanup_root(&root);
}
