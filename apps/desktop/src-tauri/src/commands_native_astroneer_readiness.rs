use app_core::InstanceDetails;
use serde_json::Value;

// Called only after the current managed process owns the console TCP endpoint.
// Matching native state proves active-world selection, not a disk save or join.
pub(super) async fn active_world_selected(instance: &InstanceDetails) -> bool {
    match super::astroneer_console::world_state(instance).await {
        Ok((games, statistics)) => selected_world_matches(&games, &statistics),
        Err(_) => false,
    }
}

fn selected_world_matches(games: &Value, statistics: &Value) -> bool {
    let Some(active) = games.get("activeSaveName").and_then(Value::as_str) else {
        return false;
    };
    games.get("gameList").is_some_and(Value::is_array)
        && !active.trim().is_empty()
        && active.chars().count() <= 256
        && !active.chars().any(char::is_control)
        && statistics.get("saveGameName").and_then(Value::as_str) == Some(active)
}

#[test]
fn astroneer_world_readiness_rejects_empty_mismatched_and_incomplete_state() {
    for (games, statistics) in [
        (
            serde_json::json!({"activeSaveName":"", "gameList":[]}),
            serde_json::json!({"saveGameName":""}),
        ),
        (
            serde_json::json!({"activeSaveName":"SAVE_1", "gameList":[]}),
            serde_json::json!({"saveGameName":""}),
        ),
        (
            serde_json::json!({"activeSaveName":"SAVE_1", "gameList":[]}),
            serde_json::json!({"saveGameName":"SAVE_2"}),
        ),
        (
            serde_json::json!({"activeSaveName":"SAVE_1"}),
            serde_json::json!({"saveGameName":"SAVE_1"}),
        ),
        (
            serde_json::json!({"activeSaveName":true, "gameList":[]}),
            serde_json::json!({"saveGameName":true}),
        ),
        (
            serde_json::json!({"activeSaveName":"\n", "gameList":[]}),
            serde_json::json!({"saveGameName":"\n"}),
        ),
    ] {
        assert!(!selected_world_matches(&games, &statistics));
    }
    assert!(selected_world_matches(
        &serde_json::json!({"activeSaveName":"SAVE_1", "gameList":[]}),
        &serde_json::json!({"saveGameName":"SAVE_1"}),
    ));
}
