#![cfg(test)]
use super::*;
use serde_json::{Value, json};

const NONCE: &str = "11111111111111111111111111111111";
const SESSION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const STEAM: &str = "76561198000000001";
const PID: u32 = 1234;

fn player(session: &str, host: &str, platform: &str) -> Value {
    json!({"session_key": session, "name": "Same name", "host_id": host,
        "platform_id": platform, "admin": false})
}

fn packet(players: Vec<Value>) -> Value {
    json!({"protocol":1,"nonce":NONCE,"pid":PID,"op":"query","status":"ok",
        "capabilities":["query","reload","kick"],"players":players,
        "access":{"admin":[],"banned":[],"permitted":[]}})
}

fn fixture() -> Value {
    packet(vec![player(SESSION, STEAM, &format!("Steam_{STEAM}"))])
}

fn save_packet() -> Value {
    let mut value = packet(Vec::new());
    value["op"] = json!("save");
    value["capabilities"] = json!(["query", "reload", "kick", "save"]);
    value["world_save"] = json!({
        "previous_save_number": 7,
        "committed_save_number": 8,
        "evidence": "native_commit"
    });
    value
}

fn decode_save(value: Value) -> Result<ControlSnapshot, ResponseError> {
    parse(
        &serde_json::to_vec(&value).unwrap(),
        NONCE,
        PID,
        Operation::Save,
    )
}

#[test]
fn save_success_requires_a_correlated_native_commit_and_exact_counter_transition() {
    let response = decode_save(save_packet()).expect("confirmed save");
    let receipt = response.world_save.expect("native receipt");
    assert_eq!(receipt.previous_save_number, 7);
    assert_eq!(receipt.committed_save_number, 8);
    for replacement in [
        Value::Null,
        json!({"previous_save_number":7,"committed_save_number":7,"evidence":"native_commit"}),
        json!({"previous_save_number":7,"committed_save_number":9,"evidence":"native_commit"}),
        json!({"previous_save_number":7,"committed_save_number":8,"evidence":"thread_finished"}),
        json!({"previous_save_number":7,"committed_save_number":8,"evidence":"native_commit","path":"other"}),
    ] {
        let mut input = save_packet();
        input["world_save"] = replacement;
        assert!(decode_save(input).is_err());
    }
    let mut no_receipt = save_packet();
    no_receipt.as_object_mut().unwrap().remove("world_save");
    assert!(decode_save(no_receipt).is_err());
    let mut wrapped = save_packet();
    wrapped["world_save"]["previous_save_number"] = json!(u32::MAX);
    wrapped["world_save"]["committed_save_number"] = json!(0);
    assert!(decode_save(wrapped).is_ok());
}

#[test]
fn save_receipts_cannot_upgrade_a_failure_or_an_unrelated_operation() {
    let mut failure = save_packet();
    failure["status"] = json!("error");
    failure["error"] = json!("save_failed");
    assert!(matches!(
        decode_save(failure.clone()),
        Err(ResponseError::Invalid(_))
    ));
    failure.as_object_mut().unwrap().remove("world_save");
    assert_eq!(
        decode_save(failure).unwrap_err(),
        ResponseError::Rejected("save_failed".into())
    );
    let mut query = save_packet();
    query["op"] = json!("query");
    assert!(decode(query).is_err());
}

#[test]
fn save_native_diagnostics_remain_bounded_failures_without_a_commit_receipt() {
    let mut failure = save_packet();
    failure["status"] = json!("error");
    failure.as_object_mut().unwrap().remove("world_save");
    let prefix = "native_error type=System.InvalidOperationException at=ZNet.HardSaveBlock ";
    let detail = format!("{prefix}{}", "x".repeat(512 - prefix.len()));
    failure["error"] = json!(&detail);
    assert_eq!(
        decode_save(failure.clone()).unwrap_err(),
        ResponseError::Rejected(detail.clone())
    );
    for invalid in [format!("{detail}x"), format!("{prefix}\nprivate-detail")] {
        let mut input = failure.clone();
        input["error"] = json!(invalid);
        assert!(matches!(decode_save(input), Err(ResponseError::Invalid(_))));
    }
    failure["world_save"] = save_packet()["world_save"].clone();
    assert!(matches!(
        decode_save(failure),
        Err(ResponseError::Invalid(_))
    ));
}

fn decode(value: Value) -> Result<ControlSnapshot, ResponseError> {
    parse(
        &serde_json::to_vec(&value).unwrap(),
        NONCE,
        PID,
        Operation::Query,
    )
}

#[test]
fn steam_identity_and_native_access_are_validated_without_promoting_to_public_namespaces() {
    let mut input = fixture();
    input["players"][0]["admin"] = json!(true);
    input["access"]["admin"] = json!([format!("Steam_{STEAM}")]);
    input["access"]["banned"] = json!(["Character With Spaces"]);
    let response = decode(input).expect("validated native response");
    assert_eq!(response.players.len(), 1);
    let peer = &response.players[0];
    assert_eq!(peer.session_key, SESSION);
    assert_eq!(peer.host_id, STEAM);
    assert_eq!(peer.platform_id, format!("Steam_{STEAM}"));
    assert_eq!(peer.name, "Same name");
    assert!(peer.admin);
    assert!(response.capabilities.contains(&Operation::Kick));
    assert_eq!(response.access.banned, ["Character With Spaces"]);
}

#[test]
fn equal_names_are_distinct_native_peers_and_other_platforms_keep_exact_namespaces() {
    let peers = vec![
        player(SESSION, STEAM, &format!("Steam_{STEAM}")),
        player(
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "Xbox_123456",
            "Xbox_123456",
        ),
        player(
            "cccccccccccccccccccccccccccccccc",
            "PlayFab_abcdef",
            "PlayFab_abcdef",
        ),
    ];
    let response = decode(packet(peers)).expect("same names do not collide");
    assert_eq!(response.players.len(), 3);
    assert!(response.players.iter().all(|peer| peer.name == "Same name"));
    assert_eq!(response.players[1].platform_id, "Xbox_123456");
    assert_eq!(response.players[1].host_id, "Xbox_123456");
    assert_eq!(response.players[2].platform_id, "PlayFab_abcdef");
    assert_eq!(response.players[2].host_id, "PlayFab_abcdef");
}

#[test]
fn missing_or_unadvertised_capabilities_do_not_imply_operation_support() {
    let mut query_only = fixture();
    query_only["capabilities"] = json!(["query"]);
    let response = decode(query_only).expect("query-only capability");
    assert!(!response.capabilities.contains(&Operation::Kick));
    for capabilities in [
        json!([]),
        json!(["reload"]),
        json!(["query", "query"]),
        json!(["query", "ban"]),
    ] {
        let mut input = fixture();
        input["capabilities"] = capabilities;
        assert!(decode(input).is_err());
    }
}

#[test]
fn response_nonce_pid_operation_and_protocol_must_match_the_request() {
    for (field, value) in [
        ("nonce", json!("22222222222222222222222222222222")),
        ("pid", json!(PID + 1)),
        ("op", json!("reload")),
        ("protocol", json!(2)),
    ] {
        let mut input = fixture();
        input[field] = value;
        assert!(decode(input).is_err(), "{field}");
    }
    let body = serde_json::to_vec(&fixture()).unwrap();
    assert!(parse(&body, NONCE, 0, Operation::Query).is_err());
    assert!(
        parse(
            &body,
            "00000000000000000000000000000000",
            PID,
            Operation::Query
        )
        .is_err()
    );
    assert!(parse(&body, "not-a-nonce", PID, Operation::Query).is_err());
    assert!(parse(&body, NONCE, PID, Operation::Kick).is_err());
}

#[test]
fn duplicate_sessions_and_hosts_are_rejected_without_name_deduplication() {
    let first = player(SESSION, STEAM, &format!("Steam_{STEAM}"));
    let duplicate_session = player(SESSION, "other-host", "Xbox_123");
    assert!(decode(packet(vec![first.clone(), duplicate_session])).is_err());
    let duplicate_host = player("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", STEAM, "Xbox_123");
    assert!(decode(packet(vec![first, duplicate_host])).is_err());
}

#[test]
fn malformed_session_host_platform_and_names_do_not_produce_validated_peers() {
    for (field, value) in [
        ("session_key", "00000000000000000000000000000000"),
        ("session_key", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
        ("session_key", "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"),
        ("host_id", "host with spaces"),
        ("host_id", "host|other"),
        ("platform_id", "Xbox_"),
        ("platform_id", "Steam_76561198000000002"),
        ("platform_id", "Steam_0001"),
        ("platform_id", "NoPlatformDelimiter"),
        ("name", " "),
        ("name", "line\nname"),
    ] {
        let mut input = fixture();
        input["players"][0][field] = json!(value);
        assert!(decode(input).is_err(), "{field}: {value}");
    }
    for field in ["name", "host_id", "platform_id"] {
        let mut input = fixture();
        input["players"][0][field] = json!("x".repeat(257));
        assert!(decode(input).is_err(), "oversized {field}");
    }
}

#[test]
fn schema_rejects_duplicate_unknown_missing_fields_wrong_types_and_token_echoes() {
    let wire = serde_json::to_string(&fixture()).unwrap();
    for duplicate in [
        wire.replace("\"protocol\":1", "\"protocol\":1,\"protocol\":1"),
        wire.replace(
            "\"name\":\"Same name\"",
            "\"name\":\"Same name\",\"name\":\"Other\"",
        ),
        wire.replace("\"admin\":[]", "\"admin\":[],\"admin\":[]"),
        wire.replace(
            "\"protocol\":1",
            "\"protocol\":1,\"error\":null,\"error\":null",
        ),
    ] {
        assert!(parse(duplicate.as_bytes(), NONCE, PID, Operation::Query).is_err());
    }
    for field in ["nonce", "pid", "op", "players", "access", "capabilities"] {
        let mut input = fixture();
        input.as_object_mut().unwrap().remove(field);
        assert!(decode(input).is_err(), "missing {field}");
    }
    let mut unknown = fixture();
    unknown["token"] = json!("private-control-token");
    let error = decode(unknown).expect_err("responses cannot echo control credentials");
    assert!(!error.to_string().contains("private-control-token"));
    let mut nested = fixture();
    nested["players"][0]["steam_id"] = json!(STEAM);
    assert!(decode(nested).is_err());
    let mut nested_access = fixture();
    nested_access["access"]["moderators"] = json!([]);
    assert!(decode(nested_access).is_err());
    for (field, value) in [
        ("players", json!({})),
        ("pid", json!(-1)),
        ("capabilities", json!("kick")),
    ] {
        let mut input = fixture();
        input[field] = value;
        assert!(decode(input).is_err());
    }
    let mut input = fixture();
    input["players"][0]["admin"] = json!("true");
    assert!(decode(input).is_err());
}

#[test]
fn invalid_utf8_trailing_json_oversize_and_unannounced_count_are_rejected() {
    assert!(parse(b"\xff", NONCE, PID, Operation::Query).is_err());
    let mut trailing = serde_json::to_vec(&fixture()).unwrap();
    trailing.extend_from_slice(b"{}");
    assert!(parse(&trailing, NONCE, PID, Operation::Query).is_err());
    assert_eq!(
        parse(
            &vec![b' '; MAX_RESPONSE_BYTES + 1],
            NONCE,
            PID,
            Operation::Query
        )
        .unwrap_err(),
        ResponseError::TooLarge
    );
    let mut input = fixture();
    input["count"] = json!(99);
    assert!(decode(input).is_err());
    let empty = decode(packet(Vec::new())).expect("complete zero-peer native query");
    assert!(empty.players.is_empty());
}

#[test]
fn remote_failure_and_contradictory_success_never_return_a_successful_snapshot() {
    let mut error = fixture();
    error["status"] = json!("error");
    error["error"] = json!("kick_timeout");
    assert_eq!(
        decode(error).unwrap_err(),
        ResponseError::Rejected("kick_timeout".into())
    );
    let mut missing = fixture();
    missing["status"] = json!("error");
    assert!(decode(missing).is_err());
    let mut contradictory = fixture();
    contradictory["error"] = json!("failed");
    assert!(decode(contradictory).is_err());
    let mut too_long = fixture();
    too_long["status"] = json!("error");
    too_long["error"] = json!("x".repeat(513));
    assert!(decode(too_long).is_err());
}

#[test]
fn native_access_facts_preserve_historical_values_and_only_reject_resources_or_controls() {
    let mut input = fixture();
    input["access"]["admin"] = json!([" Historical bad admin ", "bad|id", "bad|id", ""]);
    input["access"]["banned"] =
        json!(["#comment", "{{target}}", "Case Sensitive", "case sensitive"]);
    let response = decode(input).expect("native history does not invalidate current peers");
    assert_eq!(response.players.len(), 1);
    assert_eq!(
        response.access.admin,
        [" Historical bad admin ", "bad|id", "bad|id", ""]
    );
    assert_eq!(
        response.access.banned,
        ["#comment", "{{target}}", "Case Sensitive", "case sensitive"]
    );
    for field in ["admin", "banned", "permitted"] {
        let mut controls = fixture();
        controls["access"][field] = json!(["bad\nname"]);
        assert!(decode(controls).is_err(), "{field} controls");
        let mut too_long = fixture();
        too_long["access"][field] = json!(["x".repeat(1025)]);
        assert!(decode(too_long).is_err(), "{field} limit");
    }
}

#[test]
fn response_array_limits_are_bounded_inside_the_byte_budget() {
    let players = (1..=MAX_PLAYERS + 1)
        .map(|index| {
            player(
                &format!("{index:032x}"),
                &format!("Xbox_{index}"),
                &format!("Xbox_{index}"),
            )
        })
        .collect();
    let body = serde_json::to_vec(&packet(players)).unwrap();
    assert!(body.len() <= MAX_RESPONSE_BYTES);
    assert_eq!(
        parse(&body, NONCE, PID, Operation::Query).unwrap_err(),
        ResponseError::Invalid("player limit")
    );
    let mut input = packet(Vec::new());
    input["access"]["banned"] = json!(
        (0..=MAX_ACCESS_ENTRIES)
            .map(|index| format!("name{index}"))
            .collect::<Vec<_>>()
    );
    let body = serde_json::to_vec(&input).unwrap();
    assert!(body.len() <= MAX_RESPONSE_BYTES);
    assert!(parse(&body, NONCE, PID, Operation::Query).is_err());
}

#[test]
fn reload_and_kick_responses_only_succeed_for_the_matching_expected_operation() {
    for operation in [Operation::Reload, Operation::Kick] {
        let mut input = packet(Vec::new());
        input["op"] = serde_json::to_value(operation).unwrap();
        let body = serde_json::to_vec(&input).unwrap();
        let response =
            parse(&body, NONCE, PID, operation).expect("matching acknowledged operation");
        assert!(response.players.is_empty());
        assert!(response.capabilities.contains(&operation));
        assert!(parse(&body, NONCE, PID, Operation::Query).is_err());
    }
}

#[test]
fn crossplay_steam_uses_its_native_full_host_without_reinterpreting_the_namespace() {
    let mut input = fixture();
    input["players"][0]["host_id"] = json!(format!("Steam_{STEAM}"));
    let response = decode(input).expect("native full Steam host from PlayFab socket");
    assert_eq!(response.players[0].host_id, format!("Steam_{STEAM}"));
    assert_eq!(response.players[0].platform_id, format!("Steam_{STEAM}"));
    let mut mismatched = fixture();
    mismatched["players"][0]["host_id"] = json!("Xbox_123");
    mismatched["players"][0]["platform_id"] = json!("Xbox_456");
    assert!(decode(mismatched).is_err());
}
