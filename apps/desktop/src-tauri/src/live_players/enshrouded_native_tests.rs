use super::*;
use serde_json::json;

fn context_fixture() -> Context {
    Context {
        pid: 42,
        identity: ProcessIdentity {
            creation_time: 7,
            image_path: "enshrouded_server.exe".into(),
        },
        root: PathBuf::new(),
    }
}
fn packet_fixture() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../test-data/live-players/enshrouded-native/normal.json"
    ))
    .unwrap()
}
fn parse(value: &serde_json::Value, tick: u64) -> Result<response::Packet, String> {
    response::parse(
        &context_fixture(),
        &serde_json::to_vec(value).unwrap(),
        tick,
    )
}

#[test]
fn native_identity_is_exact_and_binding_is_private() {
    let packet = parse(&packet_fixture(), 1001).unwrap();
    let projected = response::project(packet, "instance", "snapshot", 5000).unwrap();
    let entry = &projected.public_snapshot.entries[0];
    assert_eq!(
        entry.identifiers[0].kind,
        app_core::RuntimePlayerIdentityKind::EnshroudedAccountId
    );
    assert_eq!(entry.identifiers[0].value, "7105925790711809417");
    assert_eq!(entry.available_action_ids, ["kick_player", "ban_player"]);
    let target = projected
        .private_action_bindings
        .get(&(entry.player_key.clone(), "ban_player".into()))
        .unwrap();
    assert_eq!(Target::parse(target).unwrap().encode(), *target);
    assert!(
        !serde_json::to_string(&projected.public_snapshot)
            .unwrap()
            .contains("0123456789abcdef0123456789abcdef")
    );
}

#[test]
fn invalid_or_stale_identity_never_authorizes_actions() {
    for (key, value) in [
        ("pid", json!(43)),
        ("creation_time", json!("8")),
        ("run_id", json!("bad")),
        ("sequence", json!("0")),
        ("sequence", json!("18446744073709551616")),
        ("complete", json!("true")),
        ("issue", json!("publication_unverified")),
        ("unknown", json!(1)),
    ] {
        let mut packet = packet_fixture();
        packet[key] = value;
        assert!(parse(&packet, 1001).is_err(), "{key}");
    }
    assert!(parse(&packet_fixture(), 999).is_err());
    assert!(parse(&packet_fixture(), 3001).is_err());
    for (key, value) in [
        ("session_handle", json!(80)),
        ("session_handle", json!(0)),
        ("generation", json!("0")),
        ("account_id", json!("76561197960265729")),
        ("account_id", json!(7105925790711809417_u64)),
        ("platform", json!(0)),
        ("platform_id", json!("0")),
        ("name", json!("bad\nname")),
        ("extra", json!(1)),
    ] {
        let mut packet = packet_fixture();
        packet["players"][0][key] = value;
        assert!(parse(&packet, 1001).is_err(), "{key}");
    }
    let mut duplicate = packet_fixture();
    let row = duplicate["players"][0].clone();
    duplicate["players"].as_array_mut().unwrap().push(row);
    assert!(parse(&duplicate, 1001).is_err());
    let mut duplicate_slot = packet_fixture();
    let mut other = duplicate_slot["players"][0].clone();
    other["session_handle"] = json!(129);
    other["generation"] = json!("2");
    other["platform_id"] = json!("76561200107749375");
    other["account_id"] = json!("10863749286127979488");
    duplicate_slot["players"]
        .as_array_mut()
        .unwrap()
        .push(other);
    assert!(parse(&duplicate_slot, 1001).is_err());
}

#[test]
fn protected_and_incomplete_players_have_no_action_authority() {
    let mut protected = packet_fixture();
    protected["players"][0]["moderation_immune"] = json!(true);
    let projected = response::project(
        parse(&protected, 1001).unwrap(),
        "instance",
        "snapshot",
        5000,
    )
    .unwrap();
    assert!(projected.private_action_bindings.is_empty());
    assert!(
        projected.public_snapshot.entries[0]
            .available_action_ids
            .is_empty()
    );
    let mut incomplete = packet_fixture();
    incomplete["complete"] = json!(false);
    incomplete["issue"] = json!("identity_missing");
    assert!(
        response::project(
            parse(&incomplete, 1001).unwrap(),
            "instance",
            "snapshot",
            5000
        )
        .is_err()
    );
    let empty = serde_json::from_str(include_str!(
        "../../test-data/live-players/enshrouded-native/empty.json"
    ))
    .unwrap();
    assert_eq!(
        response::project(parse(&empty, 1001).unwrap(), "instance", "snapshot", 5000)
            .unwrap()
            .public_snapshot
            .current_players,
        Some(0)
    );
}

#[test]
fn request_matches_native_ascii_protocol_without_numeric_coercion() {
    let target = Target {
        run_id: "0123456789abcdef0123456789abcdef".into(),
        sequence: 100,
        tick: 1000,
        handle: 65,
        generation: 2,
        account: u64::MAX,
    };
    let request =
        transport::encode_request("ban", "abcdef0123456789abcdef0123456789", &target, 5000);
    assert_eq!(
        request,
        "LGSM_ENSHROUDED_CONTROL_1\nrequest_id=abcdef0123456789abcdef0123456789\nrun_id=0123456789abcdef0123456789abcdef\naction=ban\nsnapshot_sequence=100\nsession_handle=65\ngeneration=2\naccount_id=18446744073709551615\nexpires_at_tick=5000\n"
    );
    for token in [
        "",
        "1",
        "0123456789abcdef0123456789abcdef:0:1000:65:2:1",
        "0123456789abcdef0123456789abcdef:1:1000:80:2:1",
        "0123456789abcdef0123456789abcdef:1:1000:65:0:1",
        "0123456789abcdef0123456789abcdef:1:1000:65:2:0",
        "0123456789abcdef0123456789abcdef:1:1000:65:2:01",
        "0123456789abcdef0123456789abcdef:1:1000:65:2:18446744073709551616",
    ] {
        assert!(Target::parse(token).is_err());
    }
}

#[test]
fn absent_native_array_or_ambiguous_records_are_not_unban_confirmation() {
    let root = std::env::temp_dir().join(format!(
        "lgsm-enshrouded-bans-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("enshrouded_server.json");
    for document in [
        json!({}),
        json!({"bans":[]}),
        json!({"bannedAccounts":[{"accountId":"1"}]}),
        json!({"bannedAccounts":[{"accountId":1},{"accountId":1}]}),
    ] {
        std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
        assert!(transport::native_banned(&root, 1).is_err());
    }
    std::fs::write(
        &path,
        b"{\"bannedAccounts\":[{\"accountId\":18446744073709551615,\"unknown\":\"preserve\"}]}",
    )
    .unwrap();
    assert!(transport::native_banned(&root, u64::MAX).unwrap());
    assert!(!transport::native_banned(&root, 1).unwrap());
    std::fs::remove_dir_all(&root).unwrap();
}
