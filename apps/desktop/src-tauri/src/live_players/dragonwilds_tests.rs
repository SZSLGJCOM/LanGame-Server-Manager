use app_core::{InstanceDetails, RuntimeLivePlayerIssueCode, RuntimePlayerIdentityKind};
use std::path::{Path, PathBuf};

use super::response::parse;
use super::{BridgeError, Game, collect, valid_nonce};

const NONCE: &str = "1234567890abcdef1234567890abcdef";

#[test]
fn dragonwilds_recorded_empty_and_constructed_populated_fixtures_parse() {
    for body in [
        include_bytes!("../../test-data/live-players/runescapedragonwilds/empty.json").as_slice(),
        include_bytes!("../../test-data/live-players/runescapedragonwilds/normal.json").as_slice(),
    ] {
        let value: serde_json::Value = serde_json::from_slice(body).unwrap();
        let nonce = value["request_id"].as_str().unwrap();
        let observed_at = value["timestamp"].as_u64().unwrap() * 1000;
        let snapshot = parse(Game::Dragonwilds, "dragon", nonce, observed_at, body).unwrap();
        assert!(snapshot.public_snapshot.complete);
        assert_eq!(snapshot.public_snapshot.snapshot_id, nonce);
        assert_eq!(
            snapshot.public_snapshot.entries.len(),
            value["current_players"].as_u64().unwrap() as usize
        );
        assert!(snapshot.private_action_bindings.is_empty());
        assert!(snapshot.public_snapshot.entries.iter().all(|entry| {
            entry.player_key.starts_with("runescapedragonwilds:")
                && entry.available_action_ids.is_empty()
        }));
    }
}

fn response() -> serde_json::Value {
    // Synthetic connected-player fixture. No populated real-server claim.
    serde_json::json!({"protocol":1, "request_id":NONCE,
        "boot_id":"abcdef1234567890abcdef1234567890", "timestamp":100,
        "complete":true,"source":"net_driver_client_connections", "current_players":2,
        "max_players":4,"players":[{"name":"玩家一","session_id":"11"},
            {"name":"玩家一","session_id":"12"}]})
}

#[test]
fn file_ipc_nonces_match_the_extensions_lowercase_hex_contract() {
    assert!(valid_nonce(NONCE));
    for nonce in [
        NONCE.to_ascii_uppercase(),
        "x".repeat(32),
        "a".repeat(31),
        "a".repeat(33),
    ] {
        assert!(!valid_nonce(&nonce));
        for game in [Game::Windrose, Game::Dragonwilds] {
            let mut value = response();
            value["request_id"] = serde_json::json!(nonce);
            assert!(
                parse(
                    game,
                    "instance",
                    &nonce,
                    100_000,
                    &serde_json::to_vec(&value).unwrap()
                )
                .is_err()
            );
            value["request_id"] = serde_json::json!(NONCE);
            value["boot_id"] = serde_json::json!(nonce);
            assert!(
                parse(
                    game,
                    "instance",
                    NONCE,
                    100_000,
                    &serde_json::to_vec(&value).unwrap()
                )
                .is_err()
            );
        }
    }
}

#[test]
fn dragonwilds_requires_its_own_verified_image_layout() {
    for image in [
        "C:/games/dragonwilds/RSDragonwildsServer.exe",
        "C:/games/dragonwilds/RSDragonwilds/Binaries/Win64/RSDragonwildsServer-Win64-Shipping.exe",
    ] {
        assert_eq!(
            Game::Dragonwilds.install_root(Path::new(image)).unwrap(),
            PathBuf::from("C:/games/dragonwilds")
        );
        assert_eq!(
            Game::Windrose.install_root(Path::new(image)),
            Err(BridgeError::Process)
        );
    }
    for image in [
        "C:/games/windrose/WindroseServer.exe",
        "C:/games/windrose/R5/Binaries/Win64/WindroseServer-Win64-Shipping.exe",
        "C:/games/dragonwilds/RSDragonwildsServer-Win64-Shipping.exe",
        "C:/games/dragonwilds/R5/Binaries/Win64/RSDragonwildsServer-Win64-Shipping.exe",
        "C:/Windows/cmd.exe",
    ] {
        assert_eq!(
            Game::Dragonwilds.install_root(Path::new(image)),
            Err(BridgeError::Process)
        );
    }
}

#[test]
fn dragonwilds_and_windrose_share_schema_but_never_player_keys() {
    let body = serde_json::to_vec(&response()).unwrap();
    let dragon = parse(Game::Dragonwilds, "dragon", NONCE, 100_000, &body).unwrap();
    let windrose = parse(Game::Windrose, "windrose", NONCE, 100_000, &body).unwrap();
    assert_eq!(dragon.public_snapshot.current_players, Some(2));
    assert_eq!(dragon.public_snapshot.max_players, Some(4));
    for (dragon, windrose) in dragon
        .public_snapshot
        .entries
        .iter()
        .zip(windrose.public_snapshot.entries.iter())
    {
        assert_ne!(dragon.player_key, windrose.player_key);
        assert!(dragon.player_key.starts_with("runescapedragonwilds:"));
        assert_eq!(dragon.display_name, "玩家一");
        assert!(dragon.available_action_ids.is_empty());
        assert_eq!(
            dragon.identifiers[0].kind,
            RuntimePlayerIdentityKind::SessionId
        );
        assert!(!dragon.identifiers[0].stable);
    }
    assert!(dragon.private_action_bindings.is_empty());
}

#[test]
fn dragonwilds_rejects_cached_partial_or_untrusted_roster_sources() {
    for (key, value) in [
        (
            "request_id",
            serde_json::json!("ffffffffffffffffffffffffffffffff"),
        ),
        ("source", serde_json::json!("game_state_player_array")),
        ("source", serde_json::json!("join_leave_log")),
        ("complete", serde_json::json!(false)),
        ("timestamp", serde_json::json!(99)),
        ("current_players", serde_json::json!(0)),
        ("max_players", serde_json::json!(1)),
        ("error", serde_json::json!("current_world_unavailable")),
    ] {
        let mut value_to_parse = response();
        value_to_parse[key] = value;
        let failure = parse(
            Game::Dragonwilds,
            "dragon",
            NONCE,
            100_000,
            &serde_json::to_vec(&value_to_parse).unwrap(),
        )
        .unwrap_err();
        assert_eq!(
            failure.issue.unwrap().code,
            RuntimeLivePlayerIssueCode::ProtocolIncomplete
        );
        assert!(!failure.complete);
    }
}

fn details(module_id: &str) -> InstanceDetails {
    serde_json::from_value(serde_json::json!({
        "summary":{"id":"instance","name":"Synthetic player-query fixture","module_id":module_id,
            "status":"Running","bind_ip":"127.0.0.1","port_count":0,"autostart":false},
        "config_file_path":"","saves_path":"","auto_backup_on_stop":false,
        "backup_retention_count":1,"settings_json":"{}","ports":[],"active_run":null,
    }))
    .unwrap()
}

#[tokio::test]
async fn cross_game_instance_context_and_missing_run_never_reach_file_exchange() {
    for (game, module, id) in [
        (Game::Dragonwilds, "windrose", "instance"),
        (Game::Windrose, "runescapedragonwilds", "instance"),
        (Game::Dragonwilds, "runescapedragonwilds", "wrong-instance"),
        (Game::Dragonwilds, "runescapedragonwilds", "instance"),
    ] {
        let failure = collect(game, &details(module), id, NONCE, 100_000)
            .await
            .unwrap_err();
        assert_eq!(
            failure.issue.unwrap().code,
            RuntimeLivePlayerIssueCode::ProcessUnavailable
        );
        assert!(failure.entries.is_empty());
    }
}
