use super::*;

const NORMAL: &str = include_str!("../../../test-data/live-players/seven-days/normal.txt");
const EOS: &str = "EOS_00000000000000000000000000000001";
const STEAM: &str = "Steam_76561190000000002";

fn decode(text: &str, actions: &[&str]) -> ParsedPlayerList {
    parse(
        ModulePlayerListCodec::SevenDaysPlayers,
        text,
        "request",
        1_800_000_000_000,
        &actions
            .iter()
            .map(|action| (*action).to_owned())
            .collect::<Vec<_>>(),
    )
    .expect("synthetic listplayers fixture should decode")
}

#[test]
fn seven_days_uses_internal_account_identity_and_never_entity_id_for_actions() {
    let list = decode(NORMAL, &["kick_player", "ban_player", "unban_player"]);
    assert!(list.complete);
    assert_eq!(list.current_players, Some(2));
    assert_eq!(list.entries[0].display_name, "Builder, 北境");
    assert_eq!(list.entries[0].ping_ms, Some(42));
    for (entry, kind, value, target) in [
        (
            &list.entries[0],
            RuntimePlayerIdentityKind::EosId,
            &EOS[4..],
            EOS,
        ),
        (
            &list.entries[1],
            RuntimePlayerIdentityKind::SteamId,
            &STEAM[6..],
            STEAM,
        ),
    ] {
        assert_eq!(entry.identifiers.len(), 2);
        assert_eq!(entry.identifiers[0].kind, kind);
        assert_eq!(entry.identifiers[0].value, value);
        assert!(entry.identifiers[0].stable);
        assert_eq!(
            entry.identifiers[1].kind,
            RuntimePlayerIdentityKind::SessionId
        );
        assert!(!entry.identifiers[1].stable);
        assert_eq!(entry.available_action_ids, ["kick_player", "ban_player"]);
        for action in ["kick_player", "ban_player"] {
            assert_eq!(
                list.bindings[&(entry.player_key.clone(), action.to_owned())],
                target
            );
        }
    }
    assert_eq!(list.entries[0].attributes[0].key, "platform_id");
    assert_eq!(
        list.entries[0].attributes[0].value,
        "Steam_76561190000000001"
    );
    let projected = serde_json::to_string(&list.entries).expect("serialize entries");
    for private_field in ["192.0.2.1", "2001:db8::2", "pos=", "rot="] {
        assert!(!projected.contains(private_field));
    }
    let subset = decode(NORMAL, &["kick_player"]);
    assert_eq!(subset.bindings.len(), 2);
    assert_eq!(subset.entries[0].available_action_ids, ["kick_player"]);
}

#[test]
fn seven_days_duplicate_names_and_fake_fields_do_not_change_bound_accounts() {
    let name = "同名玩家, crossid=EOS_deadbeef, ip=127.0.0.1";
    let response = NORMAL
        .replace("Builder, 北境", name)
        .replace("Another Survivor", name);
    let list = decode(&response, &["kick_player"]);
    assert_eq!(list.entries[0].display_name, name);
    assert_eq!(list.entries[1].display_name, name);
    assert_ne!(list.entries[0].player_key, list.entries[1].player_key);
    for (entry, target) in [(&list.entries[0], EOS), (&list.entries[1], STEAM)] {
        assert_eq!(
            list.bindings[&(entry.player_key.clone(), "kick_player".to_owned())],
            target
        );
    }
}

#[test]
fn seven_days_unknown_and_unsupported_internal_accounts_remain_read_only() {
    for response in [
        NORMAL.replace(EOS, "XBL_1234567890"),
        NORMAL
            .replace(EOS, "<unknown>")
            .replace("Steam_76561190000000001", "<unknown>"),
    ] {
        let list = decode(&response, &["kick_player", "ban_player"]);
        let entry = &list.entries[0];
        assert_eq!(entry.identifiers.len(), 1);
        assert_eq!(
            entry.identifiers[0].kind,
            RuntimePlayerIdentityKind::SessionId
        );
        assert!(!entry.identifiers[0].stable);
        assert!(entry.available_action_ids.is_empty());
        assert_eq!(list.bindings.len(), 2);
        assert!(
            !list
                .bindings
                .keys()
                .any(|(key, _)| key == &entry.player_key)
        );
    }
}

#[test]
fn seven_days_accepts_native_short_eos_ids_and_unknown_network_address() {
    let response = NORMAL
        .replace(EOS, "EOS_deadBEEF")
        .replace("192.0.2.1", "<unknown>");
    let list = decode(&response, &["kick_player"]);
    assert_eq!(list.entries[0].identifiers[0].value, "deadBEEF");
    assert_eq!(
        list.bindings[&(list.entries[0].player_key.clone(), "kick_player".to_owned())],
        "EOS_deadBEEF"
    );
    let missing_address = NORMAL
        .replace("192.0.2.1", "")
        .replace("ping=42", "ping=-1");
    let list = decode(&missing_address, &["kick_player"]);
    assert_eq!(list.entries[0].ping_ms, None);
    assert_eq!(list.entries[0].available_action_ids, ["kick_player"]);
}

#[test]
fn seven_days_rejects_incomplete_fields_ambiguous_accounts_and_old_session_only_rows() {
    for response in [
        NORMAL.replace(EOS, "EOS_nothexadecimal"),
        NORMAL.replace(EOS, "EOS_1234567"),
        NORMAL.replace(EOS, "EOS_"),
        NORMAL.replace(EOS, "missingnamespace"),
        NORMAL.replace(STEAM, "Steam_12345"),
        NORMAL.replace(", crossid=<unknown>", ""),
        NORMAL.replace("crossid=<unknown>", &format!("crossid={EOS}")),
        NORMAL.replace("id=183", "id=171"),
        NORMAL.replace("remote=True", "remote=unknown"),
        NORMAL.replace("health=100", "health=unknown"),
        NORMAL.replace("pos=(1.0, 2.0, 3.0)", "pos=(NaN, 2.0, 3.0)"),
        NORMAL.replace("pos=(1.0, 2.0, 3.0)", "pos=(1.0, 2.0)"),
        NORMAL.replace("ping=42", "ping=<unknown>"),
        NORMAL.replace("ip=192.0.2.1", "ip=invalid"),
        format!("{NORMAL}\nunexpected trailing output"),
        "1. id=171, Builder, 北境\nTotal of 1 in the game\n".to_owned(),
    ] {
        assert!(
            parse(
                ModulePlayerListCodec::SevenDaysPlayers,
                &response,
                "request",
                0,
                &[]
            )
            .is_err(),
            "must reject malformed response: {response}"
        );
    }
}
