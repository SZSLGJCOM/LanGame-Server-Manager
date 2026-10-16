use super::*;
use serde_json::{Value, json};

const NONCE: &str = "1234567890abcdef1234567890abcdef";
const BOOT: &str = "234567890abcdef1234567890abcdef1";
const ACCOUNT: &str = "34567890abcdef1234567890abcdef12";
const SESSION_TOKEN: &str = "4567890abcdef1234567890abcdef123";
const OBSERVED: u64 = 100_123;

fn fixture() -> Value {
    json!({
        "protocol": 1,
        "request_id": NONCE,
        "boot_id": BOOT,
        "timestamp": 100,
        "complete": true,
        "source": "dragonwilds_native_control",
        "current_players": 1,
        "players": [{
            "name": "LanGame涵鸽",
            "session_id": "7",
            "account_id": ACCOUNT,
            "session_token": SESSION_TOKEN,
            "admin": false,
            "actions": ["kick_player", "ban_player"]
        }],
        "error": null
    })
}

fn parse_value(value: &Value) -> Result<CachedLivePlayerSnapshot, String> {
    parse(
        "dragon-instance",
        NONCE,
        OBSERVED,
        &serde_json::to_vec(value).unwrap(),
    )
}

#[test]
fn dragonwilds_control_response_maps_exact_identity_and_private_actions() {
    let parsed = parse_value(&fixture()).unwrap();
    let snapshot = &parsed.public_snapshot;
    let entry = &snapshot.entries[0];
    assert_eq!(snapshot.snapshot_id, NONCE);
    assert_eq!(snapshot.instance_id, "dragon-instance");
    assert_eq!(snapshot.status, RuntimeLivePlayerStatus::Ready);
    assert_eq!(snapshot.source, Some(ModulePlayerListSource::FileIpc));
    assert_eq!(snapshot.observed_at_unix_ms, Some(OBSERVED));
    assert_eq!(snapshot.expires_at_unix_ms, Some(OBSERVED + 15_000));
    assert_eq!(snapshot.current_players, Some(1));
    assert_eq!(snapshot.max_players, None);
    assert!(snapshot.complete && !snapshot.stale && !snapshot.truncated);
    assert!(snapshot.issue.is_none());
    assert_eq!(parsed.collected_at, OBSERVED);
    assert_eq!(
        entry.player_key,
        format!("dragonwilds:{BOOT}:{SESSION_TOKEN}")
    );
    assert_eq!(entry.display_name, "LanGame涵鸽");
    assert_eq!(entry.identifiers.len(), 2);
    assert_eq!(entry.identifiers[0].kind, RuntimePlayerIdentityKind::EosId);
    assert_eq!(entry.identifiers[0].value, ACCOUNT);
    assert!(entry.identifiers[0].stable);
    assert_eq!(
        entry.identifiers[1].kind,
        RuntimePlayerIdentityKind::SessionId
    );
    assert_eq!(entry.identifiers[1].value, "7");
    assert!(!entry.identifiers[1].stable);
    assert_eq!(entry.available_action_ids, ["kick_player", "ban_player"]);
    assert_eq!(entry.role, None);
    assert!(entry.attributes.is_empty());
    let target = format!("{BOOT}:7:{SESSION_TOKEN}:{ACCOUNT}");
    for action in ["kick_player", "ban_player"] {
        assert_eq!(
            parsed
                .private_action_bindings
                .get(&(entry.player_key.clone(), action.into())),
            Some(&target)
        );
    }
    assert_eq!(parsed.private_action_bindings.len(), 2);
    assert!(!serde_json::to_string(snapshot).unwrap().contains(&target));
}

#[test]
fn dragonwilds_control_response_accepts_complete_empty_roster() {
    let mut value = fixture();
    value["players"] = json!([]);
    value["current_players"] = json!(0);
    let parsed = parse_value(&value).unwrap();
    assert_eq!(parsed.public_snapshot.current_players, Some(0));
    assert!(parsed.public_snapshot.complete);
    assert!(parsed.public_snapshot.entries.is_empty());
    assert!(parsed.private_action_bindings.is_empty());
}

#[test]
fn dragonwilds_control_response_never_infers_or_expands_advertised_capabilities() {
    for actions in [json!([]), json!(["kick_player"]), json!(["ban_player"])] {
        for admin in [false, true] {
            let mut value = fixture();
            value["players"][0]["actions"] = actions.clone();
            value["players"][0]["admin"] = json!(admin);
            let parsed = parse_value(&value).unwrap();
            let entry = &parsed.public_snapshot.entries[0];
            let expected: Vec<String> = serde_json::from_value(actions.clone()).unwrap();
            assert_eq!(entry.available_action_ids, expected);
            assert_eq!(parsed.private_action_bindings.len(), expected.len());
            for action in ["kick_player", "ban_player", "unban_player"] {
                assert_eq!(
                    parsed
                        .private_action_bindings
                        .contains_key(&(entry.player_key.clone(), action.into())),
                    expected.iter().any(|candidate| candidate == action)
                );
            }
            assert_eq!(entry.role.as_deref(), admin.then_some("admin"));
            assert!(entry.attributes.is_empty());
        }
    }
}

#[test]
fn dragonwilds_control_response_rejects_old_nonce_and_invalid_boot_format() {
    let bytes = serde_json::to_vec(&fixture()).unwrap();
    assert!(parse("dragon-instance", "", OBSERVED, &bytes).is_err());
    assert!(
        parse(
            "dragon-instance",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            OBSERVED,
            &bytes,
        )
        .is_err()
    );
    for boot in [
        "",
        "bad",
        "234567890abcdef1234567890abcdef12",
        "234567890abcdef1234567890abcdefg",
    ] {
        let mut value = fixture();
        value["boot_id"] = json!(boot);
        assert!(parse_value(&value).is_err(), "{boot}");
    }
    let mut uppercase_boot = fixture();
    uppercase_boot["boot_id"] = json!(BOOT.to_uppercase());
    assert!(parse_value(&uppercase_boot).is_err());
    // Parsing alone cannot authenticate a boot. The validated boot is carried
    // into the private target and must be matched by the executing native bridge.
    let mut next_boot = fixture();
    next_boot["boot_id"] = json!("abcdef1234567890abcdef1234567890");
    let parsed = parse_value(&next_boot).unwrap();
    assert_ne!(
        parsed.public_snapshot.entries[0].player_key,
        parse_value(&fixture()).unwrap().public_snapshot.entries[0].player_key
    );
    assert!(
        parsed
            .private_action_bindings
            .values()
            .all(|target| target.starts_with("abcdef1234567890abcdef1234567890:"))
    );
}

#[test]
fn dragonwilds_control_response_enforces_time_and_complete_current_packet() {
    for timestamp in [100, 106] {
        let mut value = fixture();
        value["timestamp"] = json!(timestamp);
        assert!(parse_value(&value).is_ok());
    }
    for (key, value) in [
        ("protocol", json!(0)),
        ("protocol", json!(2)),
        ("protocol", json!("1")),
        ("request_id", json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")),
        ("timestamp", json!(99)),
        ("timestamp", json!(107)),
        ("timestamp", json!(-1)),
        ("complete", json!(false)),
        ("complete", json!("true")),
        ("source", json!("net_driver_client_connections")),
        ("current_players", json!(0)),
        ("current_players", json!(2)),
        ("current_players", json!(257)),
        ("current_players", json!(-1)),
        ("error", json!("")),
        ("error", json!("native_unavailable")),
        ("extra", json!(true)),
    ] {
        let mut packet = fixture();
        packet[key] = value;
        assert!(parse_value(&packet).is_err(), "{key}: {}", packet[key]);
    }
}

#[test]
fn dragonwilds_control_response_rejects_unbounded_or_noncanonical_player_fields() {
    for (key, value) in [
        ("name", json!("")),
        ("name", json!(" \t ")),
        ("name", json!("bad\nname")),
        ("name", json!("bad\u{7f}name")),
        ("name", json!("界".repeat(171))),
        ("session_id", json!("")),
        ("session_id", json!("07")),
        ("session_id", json!("+7")),
        ("session_id", json!("-1")),
        ("session_id", json!("2147483648")),
        ("session_id", json!("4294967296")),
        ("session_id", json!(7)),
        ("account_id", json!("")),
        ("account_id", json!("1234567890abcdef1234567890abcde")),
        ("account_id", json!("34567890abcdef1234567890abcdeFG1")),
        ("session_token", json!("bad")),
        ("admin", json!("false")),
        ("actions", json!(["kick_player", "kick_player"])),
        ("actions", json!(["ban_player", "ban_player"])),
        ("actions", json!(["unban_player"])),
        ("actions", json!(["kick"])),
        ("actions", json!(null)),
        ("actions", json!([1])),
        ("extra", json!(true)),
    ] {
        let mut packet = fixture();
        packet["players"][0][key] = value;
        assert!(
            parse_value(&packet).is_err(),
            "{key}: {}",
            packet["players"][0][key]
        );
    }
    for session in ["0", "2147483647"] {
        let mut value = fixture();
        value["players"][0]["session_id"] = json!(session);
        assert!(parse_value(&value).is_ok());
    }
    let mut exact_name_limit = fixture();
    exact_name_limit["players"][0]["name"] = json!("x".repeat(512));
    assert!(parse_value(&exact_name_limit).is_ok());
    exact_name_limit["players"][0]["name"] = json!("x".repeat(513));
    assert!(parse_value(&exact_name_limit).is_err());
}

fn second_player() -> Value {
    json!({
        "name": "LanGame涵鸽",
        "session_id": "8",
        "account_id": "567890abcdef1234567890abcdef1234",
        "session_token": "67890abcdef1234567890abcdef12345",
        "admin": true,
        "actions": ["kick_player"]
    })
}

#[test]
fn dragonwilds_control_response_accepts_same_name_but_rejects_duplicate_identities() {
    let mut valid = fixture();
    valid["players"]
        .as_array_mut()
        .unwrap()
        .push(second_player());
    valid["current_players"] = json!(2);
    let parsed = parse_value(&valid).unwrap();
    assert_eq!(parsed.public_snapshot.entries.len(), 2);
    assert_eq!(
        parsed.public_snapshot.entries[0].display_name,
        parsed.public_snapshot.entries[1].display_name
    );
    assert_ne!(
        parsed.public_snapshot.entries[0].player_key,
        parsed.public_snapshot.entries[1].player_key
    );
    assert_eq!(parsed.private_action_bindings.len(), 3);
    for identity in ["session_id", "account_id", "session_token"] {
        let mut duplicate = valid.clone();
        duplicate["players"][1][identity] = duplicate["players"][0][identity].clone();
        assert!(parse_value(&duplicate).is_err(), "{identity}");
    }
}

#[test]
fn dragonwilds_control_response_rejects_missing_duplicate_or_invalid_json_fields() {
    for field in [
        "protocol",
        "request_id",
        "boot_id",
        "timestamp",
        "complete",
        "source",
        "current_players",
        "players",
    ] {
        let mut value = fixture();
        value.as_object_mut().unwrap().remove(field);
        assert!(parse_value(&value).is_err(), "{field}");
    }
    for field in [
        "name",
        "session_id",
        "account_id",
        "session_token",
        "admin",
        "actions",
    ] {
        let mut value = fixture();
        value["players"][0].as_object_mut().unwrap().remove(field);
        assert!(parse_value(&value).is_err(), "{field}");
    }
    let text = serde_json::to_string(&fixture()).unwrap();
    let duplicate = text.replacen("\"protocol\":1", "\"protocol\":1,\"protocol\":1", 1);
    assert!(parse("dragon-instance", NONCE, OBSERVED, duplicate.as_bytes()).is_err());
    let duplicate_player = text.replacen("\"admin\":false", "\"admin\":false,\"admin\":false", 1);
    assert!(
        parse(
            "dragon-instance",
            NONCE,
            OBSERVED,
            duplicate_player.as_bytes()
        )
        .is_err()
    );
    let mut invalid_utf8 = text.into_bytes();
    invalid_utf8.push(0xff);
    assert!(parse("dragon-instance", NONCE, OBSERVED, &invalid_utf8).is_err());
    assert!(
        parse(
            "dragon-instance",
            NONCE,
            OBSERVED,
            &vec![b' '; MAX_RESPONSE_BYTES + 1],
        )
        .is_err()
    );
}

#[test]
fn dragonwilds_control_response_enforces_player_limit_without_truncation() {
    let mut value = fixture();
    let players = (0..MAX_PLAYERS)
        .map(|index| {
            json!({
                "name": format!("Player {index}"),
                "session_id": index.to_string(),
                "account_id": format!("{:032x}", index + 1),
                "session_token": format!("{:032x}", index + 1),
                "admin": false,
                "actions": []
            })
        })
        .collect::<Vec<_>>();
    value["players"] = json!(players);
    value["current_players"] = json!(MAX_PLAYERS);
    let parsed = parse_value(&value).unwrap();
    assert_eq!(parsed.public_snapshot.entries.len(), MAX_PLAYERS);
    assert!(parsed.private_action_bindings.is_empty());
    value["players"]
        .as_array_mut()
        .unwrap()
        .push(second_player());
    value["current_players"] = json!(MAX_PLAYERS + 1);
    assert!(parse_value(&value).is_err());
}
