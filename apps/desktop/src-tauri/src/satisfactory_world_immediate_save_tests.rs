use super::super::api::fixture::{Fixture, Step, step};
use super::*;

fn state(session: &str, running: bool) -> Step {
    step(
        "QueryServerState",
        json!({"serverGameState": {
            "activeSessionName": session,
            "autoLoadSessionName": session,
            "isGameRunning": running,
            "numConnectedPlayers": 2
        }}),
    )
}

fn collection(session: &str, names: &[&str]) -> Step {
    step(
        "EnumerateSessions",
        json!({"sessions": [{
            "sessionName": session,
            "saveHeaders": names.iter().map(|name| json!({
                "saveName": name,
                "saveDateTime": "2026.10.19-12.00.00",
                "playDurationSeconds": 12,
                "isCreativeModeEnabled": false
            })).collect::<Vec<_>>()
        }]}),
    )
}

fn before_save() -> Vec<Step> {
    vec![
        state("Factory", true),
        collection("Factory", &["user_named_save"]),
        state("Factory", true),
    ]
}

fn saved() -> Step {
    Step {
        function: "SaveGame",
        status: 204,
        body: json!({}),
    }
}

fn assert_save_requests(api: &Api, expected: usize) {
    let fixture = api.fixture.as_ref().unwrap();
    fixture.assert_finished();
    let calls = fixture.calls.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|(function, _)| *function == "SaveGame")
            .count(),
        expected
    );
    assert!(calls.iter().all(|(function, _)| matches!(
        *function,
        "VerifyAuthenticationToken" | "QueryServerState" | "EnumerateSessions" | "SaveGame"
    )));
}

#[tokio::test]
async fn immediate_save_requires_a_running_world_with_a_nonempty_session() {
    for (name, running) in [("Factory", false), ("", true), ("  ", true)] {
        let api = Fixture::api(vec![state(name, running)]);
        assert!(
            save_current(&api)
                .await
                .unwrap_err()
                .contains("Load a Satisfactory world")
        );
        assert_save_requests(&api, 0);
    }
}

#[tokio::test]
async fn immediate_save_requires_authorization_without_enabling_insecure_access() {
    let mut api = Fixture::api(vec![]);
    api.endpoint.insecure_local_access = false;
    assert!(save_current(&api).await.unwrap_err().contains("Authorize"));
    assert!(!api.endpoint.insecure_local_access);
    assert_save_requests(&api, 0);
}

#[tokio::test]
async fn immediate_save_rejects_an_invalid_stored_token_before_reading_or_saving() {
    use base64::Engine;
    let payload = base64::engine::general_purpose::STANDARD.encode(br#"{"pl":"APIToken"}"#);
    let api = Fixture::api(vec![Step {
        function: "VerifyAuthenticationToken",
        status: 403,
        body: json!({}),
    }]);
    api.test_credentials
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .insert("api-token".into(), format!("{payload}.{}", "1a".repeat(64)));
    assert!(save_current(&api).await.unwrap_err().contains("Authorize"));
    assert_save_requests(&api, 0);
}

#[tokio::test]
async fn immediate_save_confirms_its_unique_native_name_in_the_same_running_session() {
    let mut steps = before_save();
    steps.extend([
        saved(),
        collection("Factory", &["user_named_save", "$saved"]),
        state("Factory", true),
    ]);
    let api = Fixture::api(steps);
    let result = save_current(&api).await.unwrap();
    assert_save_requests(&api, 1);
    let calls = api.fixture.as_ref().unwrap().calls.lock().unwrap();
    let (_, request) = calls
        .iter()
        .find(|(function, _)| *function == "SaveGame")
        .unwrap();
    let name = request["SaveName"].as_str().unwrap();
    assert!(uuid::Uuid::parse_str(name.strip_prefix("LGSM_manual_save_").unwrap()).is_ok());
    assert_ne!(name, "user_named_save");
    assert_eq!(request.as_object().unwrap().len(), 1);
    assert!(result.contains(name));
    assert!(result.contains("Factory"));
}

#[tokio::test]
async fn immediate_save_requires_a_terminal_native_response_without_resending() {
    for status in [200, 202, 204] {
        let mut steps = before_save();
        steps.push(Step {
            function: "SaveGame",
            status,
            body: json!({}),
        });
        if status != 202 {
            steps.extend([
                collection("Factory", &["user_named_save", "$saved"]),
                state("Factory", true),
            ]);
        }
        let api = Fixture::api(steps);
        let result = save_current(&api).await;
        if status == 202 {
            let error = result.unwrap_err();
            assert!(error.contains("outcome could not be confirmed"));
            assert!(error.contains("was not resent"));
            assert_eq!(
                api.fixture
                    .as_ref()
                    .unwrap()
                    .calls
                    .lock()
                    .unwrap()
                    .last()
                    .unwrap()
                    .0,
                "SaveGame"
            );
        } else {
            assert!(result.is_ok());
        }
        assert_save_requests(&api, 1);
    }
}

#[test]
fn immediate_save_name_collision_is_rejected_across_sessions_and_case_variants() {
    let sessions = vec![SatisfactorySession {
        session_name: "Other".into(),
        saves: vec![SatisfactorySave {
            save_name: "LGSM_MANUAL_SAVE_EXISTING".into(),
            save_date_time: "2026.10.19-12.00.00".into(),
            play_duration_seconds: 12,
            is_creative_mode_enabled: false,
        }],
    }];
    assert!(require_unused_save_name(&sessions, "LGSM_manual_save_existing").is_err());
    assert!(require_unused_save_name(&sessions, "LGSM_manual_save_new").is_ok());
}

#[tokio::test]
async fn immediate_save_does_not_write_if_the_session_changed_during_preflight() {
    let api = Fixture::api(vec![
        state("Factory", true),
        collection("Factory", &["user_named_save"]),
        state("Other", true),
    ]);
    assert!(
        save_current(&api)
            .await
            .unwrap_err()
            .contains("No save was requested")
    );
    assert_save_requests(&api, 0);
}

#[tokio::test]
async fn immediate_save_does_not_retry_a_native_save_rejection() {
    let mut steps = before_save();
    steps.push(Step {
        function: "SaveGame",
        status: 200,
        body: json!({"errorCode":"save_game_failed", "errorMessage":"private native details"}),
    });
    let api = Fixture::api(steps);
    let error = save_current(&api).await.unwrap_err();
    assert!(error.contains("save_game_failed"));
    assert!(!error.contains("private native details"));
    assert!(!error.contains("outcome could not be confirmed"));
    assert_save_requests(&api, 1);
}

#[tokio::test]
async fn immediate_save_requires_its_exact_name_in_the_original_session() {
    for collection in [
        collection("Factory", &["user_named_save"]),
        collection("Other", &["$saved"]),
        step("EnumerateSessions", json!({"sessions": []})),
    ] {
        let mut steps = before_save();
        steps.extend([saved(), collection]);
        let api = Fixture::api(steps);
        assert!(
            save_current(&api)
                .await
                .unwrap_err()
                .contains("could not be verified")
        );
        assert_save_requests(&api, 1);
    }
}

#[tokio::test]
async fn immediate_save_reports_unknown_outcome_when_confirmation_is_unavailable() {
    for response in [
        Step {
            function: "EnumerateSessions",
            status: 503,
            body: json!({}),
        },
        step("EnumerateSessions", json!({"sessions": null})),
    ] {
        let mut steps = before_save();
        steps.extend([saved(), response]);
        let api = Fixture::api(steps);
        let error = save_current(&api).await.unwrap_err();
        assert!(error.contains("outcome could not be confirmed"));
        assert!(error.contains("was not resent"));
        assert_save_requests(&api, 1);
    }
}

#[tokio::test]
async fn immediate_save_requires_the_session_to_remain_active_after_native_confirmation() {
    for after in [state("Other", true), state("Factory", false)] {
        let mut steps = before_save();
        steps.extend([saved(), collection("Factory", &["$saved"]), after]);
        let api = Fixture::api(steps);
        assert!(
            save_current(&api)
                .await
                .unwrap_err()
                .contains("outcome could not be confirmed")
        );
        assert_save_requests(&api, 1);
    }
}

#[tokio::test]
async fn immediate_save_requires_complete_final_state_readback() {
    let mut steps = before_save();
    steps.extend([
        saved(),
        collection("Factory", &["$saved"]),
        step(
            "QueryServerState",
            json!({"serverGameState": {"isGameRunning": true}}),
        ),
    ]);
    let api = Fixture::api(steps);
    assert!(
        save_current(&api)
            .await
            .unwrap_err()
            .contains("outcome could not be confirmed")
    );
    assert_save_requests(&api, 1);
}

#[tokio::test]
async fn cancelled_immediate_save_waiter_retains_real_instance_and_storage_leases() {
    use crate::state::DesktopState;
    use std::{sync::Arc, time::Duration};

    let desktop = Arc::new(DesktopState::default());
    let operation = desktop
        .begin_storage_context_operation("immediate save lifecycle test")
        .unwrap();
    let mutation = desktop.acquire_instance_mutation("fixture").await;
    let mut steps = before_save();
    steps.extend([
        saved(),
        collection("Factory", &["$saved"]),
        state("Factory", true),
    ]);
    let api = Fixture::api(steps);
    let fixture = api.fixture.as_ref().unwrap();
    let calls = Arc::clone(&fixture.calls);
    let (started, release) = fixture.pause_save_reply();
    let waiter = tokio::spawn(async move {
        super::super::spawn_current_save(&operation, Some(mutation), api)
            .await
            .unwrap()
    });
    started.await.unwrap();
    waiter.abort();
    assert!(waiter.await.unwrap_err().is_cancelled());
    assert!(
        desktop
            .try_acquire_instance_mutation("fixture")
            .await
            .is_none()
    );
    assert!(desktop.begin_storage_context_transition().is_err());
    assert_eq!(calls.lock().unwrap().last().unwrap().0, "SaveGame");

    release.send(()).unwrap();
    let mutation = tokio::time::timeout(
        Duration::from_secs(5),
        desktop.acquire_instance_mutation("fixture"),
    )
    .await
    .unwrap();
    drop(mutation);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if desktop.begin_storage_context_transition().is_ok() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let calls = calls.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|(function, _)| *function == "SaveGame")
            .count(),
        1
    );
    assert_eq!(calls[calls.len() - 2].0, "EnumerateSessions");
    assert_eq!(calls.last().unwrap().0, "QueryServerState");
}
