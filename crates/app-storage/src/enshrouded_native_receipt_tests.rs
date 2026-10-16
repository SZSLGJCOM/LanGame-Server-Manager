use super::*;

const TARGET: u64 = 9_007_199_254_740_993;

fn native_path(fixture: &NativeSettingsFixture) -> PathBuf {
    instance_private_runtime_root(&fixture.created).join("enshrouded_server.json")
}

fn managed_path(fixture: &NativeSettingsFixture) -> PathBuf {
    Path::new(&fixture.created.config_file_path)
        .parent()
        .unwrap()
        .join("enshrouded_server.json")
}

fn document(path: &Path) -> Value {
    let bytes = fs::read(path).unwrap();
    serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes)).unwrap()
}

fn write_document(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn ban(id: u64) -> Value {
    json!({"accountId":id,"displayName":"synthetic account","characterName":"synthetic character",
        "banDate":{"value":123,"opaqueDate":"keep"},"opaqueRecord":{"keep":[1,2,3]}})
}

fn write_native(fixture: &NativeSettingsFixture, records: Value) {
    let path = native_path(fixture);
    let mut value = document(&path);
    value.as_object_mut().unwrap().remove("bans");
    value["bannedAccounts"] = records;
    value["opaqueNative"] = json!({"keep":true});
    write_document(&path, &value);
}

fn set_stored_ids(fixture: &NativeSettingsFixture, value: &str) -> Value {
    let path = Path::new(&fixture.created.config_file_path);
    let mut stored = document(path);
    stored["opaqueDocument"] = json!({"keep":true});
    stored["settings"]["server_name"] = json!("Pending next-start name");
    stored["settings"]["banned_player_ids"] = json!(value);
    stored["settings"]["opaqueSetting"] = json!({"keep":[1,true,"value"]});
    write_document(path, &stored);
    stored
}

async fn start_run(fixture: &NativeSettingsFixture) -> ActiveInstanceRun {
    record_started_test_instance(
        &fixture.paths,
        &fixture.created.summary.id,
        62050,
        "synthetic-native-receipt.log",
    )
    .await
    .unwrap()
}

async fn receipt(
    fixture: &NativeSettingsFixture,
    run_id: i64,
    banned: bool,
) -> Result<InstanceDetails, StorageError> {
    record_enshrouded_ban_from_native(
        &fixture.paths,
        &fixture.created.summary.id,
        run_id,
        TARGET,
        banned,
    )
    .await
}

#[tokio::test]
async fn enshrouded_native_receipt_ban_changes_only_requested_mirror_and_preserves_native() {
    let fixture = NativeSettingsFixture::new("enshrouded").await;
    let mut expected = set_stored_ids(&fixture, "17");
    let config_path = Path::new(&fixture.created.config_file_path);
    let initial_text = serde_json::to_string_pretty(&expected)
        .unwrap()
        .replace('\n', "\r\n");
    let mut initial_bytes = vec![0xef, 0xbb, 0xbf];
    initial_bytes.extend_from_slice(initial_text.as_bytes());
    initial_bytes.extend_from_slice(b"\r\n");
    fs::write(config_path, initial_bytes).unwrap();
    write_native(&fixture, json!([ban(TARGET), ban(u64::MAX)]));
    let native_before = fs::read(native_path(&fixture)).unwrap();
    let managed_before = fs::read(managed_path(&fixture)).unwrap();
    let run = start_run(&fixture).await;
    let result = receipt(&fixture, run.run_id, true).await.unwrap();
    let projected = format!("17\n{TARGET}");
    expected["settings"]["banned_player_ids"] = json!(projected);
    assert_eq!(document(config_path), expected);
    assert_eq!(
        serde_json::from_str::<Value>(&result.settings_json).unwrap(),
        expected["settings"]
    );
    assert_eq!(fs::read(native_path(&fixture)).unwrap(), native_before);
    assert_eq!(fs::read(managed_path(&fixture)).unwrap(), managed_before);
    let after = fs::read(config_path).unwrap();
    assert!(after.starts_with(&[0xef, 0xbb, 0xbf]));
    assert!(after.ends_with(b"\r\n"));
    assert!(after.windows(2).any(|bytes| bytes == b"\r\n"));
    let before_retry = after;
    receipt(&fixture, run.run_id, true).await.unwrap();
    assert_eq!(fs::read(config_path).unwrap(), before_retry);
}

#[tokio::test]
async fn enshrouded_native_receipt_unban_removes_only_target_and_cannot_revive_from_managed() {
    let fixture = NativeSettingsFixture::new("enshrouded").await;
    let mut expected = set_stored_ids(&fixture, &format!("17\n{TARGET}"));
    write_native(&fixture, json!([ban(u64::MAX)]));
    let path = managed_path(&fixture);
    let mut managed = document(&path);
    managed["bannedAccounts"] = json!([ban(TARGET), ban(88)]);
    managed["bans"] = json!([ban(TARGET)]);
    managed["opaqueConfig"] = json!({"keep":[true,{"nested":1}]});
    write_document(&path, &managed);
    let native_before = fs::read(native_path(&fixture)).unwrap();
    let run = start_run(&fixture).await;
    receipt(&fixture, run.run_id, false).await.unwrap();
    expected["settings"]["banned_player_ids"] = json!("17");
    assert_eq!(
        document(Path::new(&fixture.created.config_file_path)),
        expected
    );
    managed["bannedAccounts"] = json!([ban(88)]);
    managed["bans"] = json!([]);
    assert_eq!(document(&path), managed);
    assert_eq!(fs::read(native_path(&fixture)).unwrap(), native_before);
    let reopened = fixture.details().await;
    let settings: Value = serde_json::from_str(&reopened.settings_json).unwrap();
    assert!(
        !settings["banned_player_ids"]
            .as_str()
            .unwrap()
            .lines()
            .any(|id| id == TARGET.to_string())
    );
    mark_instance_process_stopped(
        &fixture.paths,
        &fixture.created.summary.id,
        run.run_id,
        Some(0),
        false,
    )
    .await
    .unwrap();
    materialize_instance_configuration_for_start(&fixture.paths, &fixture.created.summary.id)
        .await
        .unwrap();
    let native_after_start = document(&native_path(&fixture));
    assert!(
        !native_after_start["bannedAccounts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["accountId"].as_u64() == Some(TARGET))
    );
    assert!(
        native_after_start["bannedAccounts"]
            .as_array()
            .unwrap()
            .contains(&ban(88))
    );
    assert!(
        native_after_start["bannedAccounts"]
            .as_array()
            .unwrap()
            .contains(&ban(u64::MAX))
    );
}

#[tokio::test]
async fn enshrouded_native_receipt_requires_current_complete_canonical_native_evidence() {
    let fixture = NativeSettingsFixture::new("enshrouded").await;
    set_stored_ids(&fixture, &format!("17\n{TARGET}"));
    let run = start_run(&fixture).await;
    let config_before = fs::read(&fixture.created.config_file_path).unwrap();
    let managed_before = fs::read(managed_path(&fixture)).unwrap();
    for (value, banned) in [
        (json!({}), false),
        (json!({"bans":[ban(TARGET)]}), false),
        (json!({"bannedAccounts":[ban(TARGET)]}), false),
        (json!({"bannedAccounts":[]}), true),
        (json!({"bannedAccounts":[],"bans":[ban(TARGET)]}), false),
        (
            json!({"bannedAccounts":[{"accountId":TARGET,"displayName":false}]}),
            true,
        ),
        (
            json!({"bannedAccounts":[{"accountId":TARGET.to_string()}]}),
            true,
        ),
    ] {
        write_document(&native_path(&fixture), &value);
        let native_before = fs::read(native_path(&fixture)).unwrap();
        assert!(receipt(&fixture, run.run_id, banned).await.is_err());
        assert_eq!(
            fs::read(&fixture.created.config_file_path).unwrap(),
            config_before
        );
        assert_eq!(fs::read(managed_path(&fixture)).unwrap(), managed_before);
        assert_eq!(fs::read(native_path(&fixture)).unwrap(), native_before);
    }
    fs::write(native_path(&fixture), b"{incomplete native save").unwrap();
    assert!(receipt(&fixture, run.run_id, false).await.is_err());
    assert_eq!(
        fs::read(&fixture.created.config_file_path).unwrap(),
        config_before
    );
    fs::remove_file(native_path(&fixture)).unwrap();
    assert!(receipt(&fixture, run.run_id, false).await.is_err());
    fs::write(native_path(&fixture), vec![b' '; 256 * 1024 + 1]).unwrap();
    assert!(receipt(&fixture, run.run_id, false).await.is_err());
    // Optional native metadata remains optional, matching the authoritative codec.
    write_document(
        &native_path(&fixture),
        &json!({"bannedAccounts":[{"accountId":TARGET}]}),
    );
    receipt(&fixture, run.run_id, true).await.unwrap();
}

#[tokio::test]
async fn enshrouded_native_receipt_second_file_failure_compensates_managed_unban() {
    let fixture = NativeSettingsFixture::new("enshrouded").await;
    set_stored_ids(&fixture, &format!("17\n{TARGET}"));
    write_native(&fixture, json!([]));
    let managed = managed_path(&fixture);
    let mut value = document(&managed);
    value["bannedAccounts"] = json!([ban(TARGET), ban(88)]);
    write_document(&managed, &value);
    let config_before = fs::read(&fixture.created.config_file_path).unwrap();
    let managed_before = fs::read(&managed).unwrap();
    let native_before = fs::read(native_path(&fixture)).unwrap();
    let run = start_run(&fixture).await;
    for failed_path in [&managed, Path::new(&fixture.created.config_file_path)] {
        crate::atomic_file::fail_next_atomic_write_for_test(failed_path);
        assert!(receipt(&fixture, run.run_id, false).await.is_err());
        assert_eq!(
            fs::read(&fixture.created.config_file_path).unwrap(),
            config_before
        );
        assert_eq!(fs::read(&managed).unwrap(), managed_before);
        assert_eq!(fs::read(native_path(&fixture)).unwrap(), native_before);
    }
    receipt(&fixture, run.run_id, false).await.unwrap();
}

#[tokio::test]
async fn enshrouded_native_receipt_preparation_changes_reject_without_overwriting_them() {
    for changed in ["runtime", "managed", "settings", "run"] {
        let fixture = NativeSettingsFixture::new("enshrouded").await;
        set_stored_ids(&fixture, &format!("17\n{TARGET}"));
        write_native(&fixture, json!([]));
        let managed = managed_path(&fixture);
        let mut value = document(&managed);
        value["bannedAccounts"] = json!([ban(TARGET), ban(88)]);
        write_document(&managed, &value);
        let run = start_run(&fixture).await;
        let mut gate =
            test_gate::register(&fixture.paths.database_path, Point::NativeSettingsReady);
        let paths = fixture.paths.clone();
        let id = fixture.created.summary.id.clone();
        let worker = tokio::spawn(async move {
            record_enshrouded_ban_from_native(&paths, &id, run.run_id, TARGET, false).await
        });
        gate.reached().await;
        match changed {
            "runtime" => write_native(&fixture, json!([ban(TARGET)])),
            "managed" => {
                value["opaqueExternal"] = json!(true);
                write_document(&managed, &value);
            }
            "settings" => {
                set_stored_ids(&fixture, "17\n29");
            }
            "run" => {
                mark_instance_process_stopped(
                    &fixture.paths,
                    &fixture.created.summary.id,
                    run.run_id,
                    Some(0),
                    false,
                )
                .await
                .unwrap();
                start_run(&fixture).await;
            }
            _ => unreachable!(),
        }
        let config_before = fs::read(&fixture.created.config_file_path).unwrap();
        let managed_before = fs::read(&managed).unwrap();
        let native_before = fs::read(native_path(&fixture)).unwrap();
        gate.resume();
        assert!(worker.await.unwrap().is_err(), "accepted changed {changed}");
        assert_eq!(
            fs::read(&fixture.created.config_file_path).unwrap(),
            config_before
        );
        assert_eq!(fs::read(&managed).unwrap(), managed_before);
        assert_eq!(fs::read(native_path(&fixture)).unwrap(), native_before);
    }
}

#[tokio::test]
async fn enshrouded_native_receipt_rejects_stale_run_wrong_module_and_invalid_identity() {
    let fixture = NativeSettingsFixture::new("enshrouded").await;
    write_native(&fixture, json!([ban(TARGET)]));
    let run = start_run(&fixture).await;
    assert!(receipt(&fixture, run.run_id + 1, true).await.is_err());
    assert!(
        record_enshrouded_ban_from_native(
            &fixture.paths,
            &fixture.created.summary.id,
            run.run_id,
            0,
            true
        )
        .await
        .is_err()
    );
    mark_instance_process_stopped(
        &fixture.paths,
        &fixture.created.summary.id,
        run.run_id,
        Some(0),
        false,
    )
    .await
    .unwrap();
    assert!(receipt(&fixture, run.run_id, true).await.is_err());
    let wrong = NativeSettingsFixture::new("returntomoria").await;
    let wrong_run = start_run(&wrong).await;
    assert!(receipt(&wrong, wrong_run.run_id, true).await.is_err());
}
