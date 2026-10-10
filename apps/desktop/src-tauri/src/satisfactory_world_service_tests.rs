use super::super::api::fixture::{Fixture, Step, step};
use super::*;
use std::collections::BTreeMap;

fn save(name: &str) -> Value {
    json!({"saveName":name,"saveDateTime":"2026.10.09-15.31.49","playDurationSeconds":1,"isCreativeModeEnabled":false})
}

fn sessions(names: &[(&str, &[&str])]) -> Value {
    json!({"sessions":names.iter().map(|(name, saves)| json!({"sessionName":name,"saveHeaders":saves.iter().map(|name| save(name)).collect::<Vec<_>>()})).collect::<Vec<_>>()})
}

fn snapshot_steps(collection: Value) -> Vec<Step> {
    vec![
        step(
            "QueryServerState",
            json!({"serverGameState":{"activeSessionName":"Old","autoLoadSessionName":"Old","isGameRunning":true,"numConnectedPlayers":0}}),
        ),
        step(
            "GetServerOptions",
            json!({"serverOptions":{},"pendingServerOptions":{}}),
        ),
        step(
            "GetAdvancedGameSettings",
            json!({"creativeModeEnabled":false,"advancedGameSettings":{"FG.GameRules.NoPower":"False"}}),
        ),
        step("EnumerateSessions", collection),
    ]
}

fn expected_revision() -> String {
    let mut snapshot = protocol::empty_snapshot(
        "fixture",
        SatisfactoryConnectionStatus::Ready,
        Some("Fixture".into()),
    )
    .unwrap();
    snapshot.active_session_name = "Old".into();
    snapshot.auto_load_session_name = "Old".into();
    snapshot.is_game_running = true;
    snapshot
        .advanced_game_settings
        .insert("FG.GameRules.NoPower".into(), "False".into());
    protocol::revision(&snapshot).unwrap()
}

fn before_and_save() -> Vec<Step> {
    let mut steps = snapshot_steps(sessions(&[("Old", &["old_save"])]));
    steps.push(Step {
        function: "SaveGame",
        status: 204,
        body: json!({}),
    });
    steps.push(step(
        "EnumerateSessions",
        sessions(&[("Old", &["old_save", "$saved"])]),
    ));
    steps
}

fn create_input() -> CreateSatisfactoryWorldInput {
    CreateSatisfactoryWorldInput {
        instance_id: "fixture".into(),
        expected_revision: expected_revision(),
        session_name: "New".into(),
        starting_location: String::new(),
        skip_onboarding: true,
        acknowledge_enable_advanced_settings: false,
        game_mode_settings: BTreeMap::new(),
        advanced_game_settings: BTreeMap::new(),
    }
}

#[tokio::test]
async fn verification_sends_native_fingerprint_and_keeps_the_complete_application_token() {
    use base64::Engine;
    let payload = base64::engine::general_purpose::STANDARD.encode(br#"{"pl":"APIToken"}"#);
    let fingerprint = "1a".repeat(64);
    let token = format!("{payload}.{fingerprint}");
    let api = Fixture::api(vec![Step {
        function: "VerifyAuthenticationToken",
        status: 204,
        body: json!({}),
    }]);
    api.test_credentials
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .insert("api-token".into(), token.clone());
    assert_eq!(
        stored_authorization(&api).await.unwrap(),
        Some(token.clone())
    );
    let fixture = api.fixture.as_ref().unwrap();
    fixture.assert_finished();
    let calls = fixture.calls.lock().unwrap();
    assert_eq!(calls[0].1["AuthenticationToken"], fingerprint);
    assert_ne!(calls[0].1["AuthenticationToken"], token);
    assert_eq!(calls[0].1["PrivilegeLevel"], "APIToken");
}

#[tokio::test]
async fn create_rechecks_new_session_name_after_saving_the_current_world() {
    let mut steps = before_and_save();
    steps.extend(snapshot_steps(sessions(&[
        ("Old", &["old_save", "$saved"]),
        ("New", &["new_save"]),
    ])));
    let api = Fixture::api(steps);
    assert!(
        create(&api, create_input())
            .await
            .unwrap_err()
            .contains("was created with that name")
    );
    let fixture = api.fixture.as_ref().unwrap();
    fixture.assert_finished();
    assert!(
        !fixture
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(function, _)| *function == "CreateNewGame")
    );
}

#[tokio::test]
async fn load_rechecks_selected_save_after_saving_the_current_world() {
    let mut steps = before_and_save();
    steps.extend(snapshot_steps(sessions(&[("Old", &["$saved"])])));
    let api = Fixture::api(steps);
    let input = LoadSatisfactorySaveInput {
        instance_id: "fixture".into(),
        expected_revision: expected_revision(),
        save_name: "old_save".into(),
    };
    assert!(
        load(&api, input)
            .await
            .unwrap_err()
            .contains("disappeared before loading")
    );
    let fixture = api.fixture.as_ref().unwrap();
    fixture.assert_finished();
    assert!(
        !fixture
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(function, _)| *function == "LoadGame")
    );
}

#[tokio::test]
async fn native_202_is_accepted_even_when_https_listener_closes_for_map_loading() {
    let mut steps = before_and_save();
    steps.extend(snapshot_steps(sessions(&[(
        "Old",
        &["old_save", "$saved"],
    )])));
    steps.push(Step {
        function: "CreateNewGame",
        status: 202,
        body: json!({}),
    });
    let api = Fixture::api(steps);
    let result = create(&api, create_input()).await.unwrap();
    assert!(result.accepted);
    let fixture = api.fixture.as_ref().unwrap();
    fixture.assert_finished();
    assert_eq!(fixture.checks.lock().unwrap().last(), Some(&true));
    let calls = fixture.calls.lock().unwrap();
    let request = &calls.last().unwrap().1["NewGameData"];
    assert_eq!(request["bSkipOnboarding"], true);
    assert_eq!(request["GameModeSettings"], json!({}));
}

#[tokio::test]
async fn a_successful_world_write_then_failed_udp_readback_has_unknown_outcome() {
    let mut steps = before_and_save();
    steps.extend(snapshot_steps(sessions(&[(
        "Old",
        &["old_save", "$saved"],
    )])));
    steps.push(Step {
        function: "ApplyAdvancedGameSettings",
        status: 204,
        body: json!({}),
    });
    steps.push(Step {
        function: "SaveGame",
        status: 204,
        body: json!({}),
    });
    steps.push(step(
        "EnumerateSessions",
        sessions(&[("Old", &["old_save", "$saved"])]),
    ));
    let api = Fixture::api(steps);
    api.fixture.as_ref().unwrap().names.lock().unwrap().extend([
        Ok("Fixture".into()),
        Ok("Fixture".into()),
        Err("UDP unavailable".into()),
    ]);
    let input = WriteSatisfactoryWorldRulesInput {
        instance_id: "fixture".into(),
        expected_revision: expected_revision(),
        acknowledge_enable_advanced_settings: true,
        advanced_game_settings: BTreeMap::from([("FG.GameRules.NoPower".into(), "True".into())]),
    };
    assert!(
        write_rules(&api, input)
            .await
            .unwrap_err()
            .contains("outcome could not be confirmed")
    );
    api.fixture.as_ref().unwrap().assert_finished();
}

#[tokio::test]
async fn successful_room_write_then_failed_udp_readback_has_unknown_outcome() {
    let mut steps = snapshot_steps(sessions(&[("Old", &["old_save"])]));
    steps.push(Step {
        function: "RenameServer",
        status: 204,
        body: json!({}),
    });
    let api = Fixture::api(steps);
    api.fixture
        .as_ref()
        .unwrap()
        .names
        .lock()
        .unwrap()
        .extend([Ok("Fixture".into()), Err("UDP unavailable".into())]);
    let input = WriteSatisfactoryRoomInput {
        instance_id: "fixture".into(),
        expected_revision: expected_revision(),
        server_name: Some("Renamed".into()),
        client_password: None,
        auto_load_session_name: None,
    };
    assert!(
        room(&api, input)
            .await
            .unwrap_err()
            .contains("outcome could not be confirmed")
    );
    api.fixture.as_ref().unwrap().assert_finished();
}
