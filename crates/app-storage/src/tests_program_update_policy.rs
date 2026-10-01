use super::*;
use app_core::InstanceProgramUpdatePolicy;

#[tokio::test]
async fn program_update_policy_persists_and_rejects_invalid_or_stale_saves() {
    let fixture = NativeSettingsFixture::new("returntomoria").await;
    let initial = fixture.details().await;
    assert_eq!(
        InstanceProgramUpdatePolicy::from_settings_json(&initial.settings_json).unwrap(),
        InstanceProgramUpdatePolicy::Automatic
    );
    let mut baseline = initial.clone();
    for policy in ["pinned", "automatic", "pinned"] {
        let mut draft: Value = serde_json::from_str(&baseline.settings_json).unwrap();
        draft["program_update"] = json!({"policy": policy});
        baseline = update_instance_if_current(
            &fixture.paths,
            fixture.input(&baseline, draft),
            &baseline.settings_json,
        )
        .await
        .unwrap();
        let persisted = fixture.details().await;
        let disk: Value =
            serde_json::from_slice(&fs::read(&baseline.config_file_path).unwrap()).unwrap();
        assert_eq!(disk["settings"]["program_update"]["policy"], policy);
        assert_eq!(
            serde_json::from_str::<Value>(&persisted.settings_json).unwrap()["program_update"]["policy"],
            policy
        );
    }

    let saved_bytes = fs::read(&baseline.config_file_path).unwrap();
    for invalid in [json!(null), json!("pinned"), json!({"policy": "latest"})] {
        let mut draft: Value = serde_json::from_str(&baseline.settings_json).unwrap();
        draft["program_update"] = invalid;
        let error = update_instance_if_current(
            &fixture.paths,
            fixture.input(&baseline, draft),
            &baseline.settings_json,
        )
        .await
        .expect_err("invalid policy must not change persisted settings");
        assert!(
            matches!(error, StorageError::InvalidModuleSetting { field, .. }
            if field == "program_update")
        );
        assert_eq!(fs::read(&baseline.config_file_path).unwrap(), saved_bytes);
    }

    let error = update_instance_if_current(
        &fixture.paths,
        fixture.input(
            &initial,
            serde_json::from_str(&initial.settings_json).unwrap(),
        ),
        &initial.settings_json,
    )
    .await
    .expect_err("a stale settings screen must not clear the saved pinned policy");
    assert!(matches!(
        error,
        StorageError::InstanceSettingsPreconditionFailed { .. }
    ));
    assert_eq!(fs::read(&baseline.config_file_path).unwrap(), saved_bytes);
}

#[tokio::test]
async fn program_update_policy_changes_require_idle_instance_but_unchanged_policy_can_save() {
    let fixture = NativeSettingsFixture::new("returntomoria").await;
    for status in ["starting", "running", "stopping"] {
        fixture.status(status).await;
        let baseline = fixture.details().await;
        let saved_bytes = fs::read(&baseline.config_file_path).unwrap();
        let mut draft: Value = serde_json::from_str(&baseline.settings_json).unwrap();
        draft["program_update"] = json!({"policy": "pinned"});
        let error = update_instance_if_current(
            &fixture.paths,
            fixture.input(&baseline, draft.clone()),
            &baseline.settings_json,
        )
        .await
        .expect_err("an active instance must not change update policy");
        assert!(
            matches!(error, StorageError::InvalidModuleSetting { field, .. }
            if field == "program_update.policy")
        );
        assert_eq!(fs::read(&baseline.config_file_path).unwrap(), saved_bytes);

        // Explicitly saving the existing default is not a policy change and
        // must not restrict the existing live configuration-save behavior.
        draft["program_update"] = json!({"policy": "automatic"});
        update_instance_if_current(
            &fixture.paths,
            fixture.input(&baseline, draft),
            &baseline.settings_json,
        )
        .await
        .unwrap();
    }
}
