use std::path::Path;

use super::{BridgeError, Game, response};

#[test]
fn scum_requires_the_native_executable_layout() {
    let image = Path::new("C:/games/scum/SCUM/Binaries/Win64/SCUMServer.exe");
    assert_eq!(
        Game::Scum.install_root(image).unwrap(),
        Path::new("C:/games/scum")
    );
    for image in [
        "C:/games/scum/SCUMServer.exe",
        "C:/games/scum/R5/Binaries/Win64/SCUMServer.exe",
    ] {
        assert!(matches!(
            Game::Scum.install_root(Path::new(image)),
            Err(BridgeError::Process)
        ));
    }
}

#[test]
fn scum_current_roster_is_read_only_and_rejects_other_requests() {
    let nonce = "1234567890abcdef1234567890abcdef";
    let mut body = serde_json::json!({
        "protocol": 1, "request_id": nonce,
        "boot_id": "abcdef1234567890abcdef1234567890",
        "timestamp": 1788876000_u64, "complete": true,
        "source": "net_driver_client_connections", "current_players": 2,
        "players": [{"name":"同名玩家", "session_id":"1"}, {"name":"同名玩家", "session_id":"2"}]
    });
    let parsed = response::parse(
        Game::Scum,
        "scum-instance",
        nonce,
        1788876000000,
        &serde_json::to_vec(&body).unwrap(),
    )
    .unwrap();
    assert_eq!(parsed.public_snapshot.entries.len(), 2);
    assert!(parsed.private_action_bindings.is_empty());
    for entry in &parsed.public_snapshot.entries {
        assert!(entry.player_key.starts_with("scum:"));
        assert!(entry.available_action_ids.is_empty());
        assert!(!entry.identifiers[0].stable);
    }
    body["request_id"] = serde_json::json!("ffffffffffffffffffffffffffffffff");
    assert!(
        response::parse(
            Game::Scum,
            "scum-instance",
            nonce,
            1788876000000,
            &serde_json::to_vec(&body).unwrap()
        )
        .is_err()
    );
}
