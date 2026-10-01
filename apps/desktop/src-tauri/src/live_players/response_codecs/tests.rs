use super::*;
use serde_json::Value;

const NOW: u64 = 1_800_000_000_000;
const RUST: &str = include_str!("../../../test-data/live-players/rust/normal.json");
const RUST_EMPTY: &str = include_str!("../../../test-data/live-players/rust/empty.json");
const ARK: &str = include_str!("../../../test-data/live-players/ark/normal.txt");
const ARK_EMPTY: &str = include_str!("../../../test-data/live-players/ark/empty.txt");
const ASA: &str = include_str!("../../../test-data/live-players/ark/ascended.txt");
const CONAN: &str = include_str!("../../../test-data/live-players/conan/normal.txt");
const CONAN_EMPTY: &str = include_str!("../../../test-data/live-players/conan/empty.txt");
const SQUAD: &str = include_str!("../../../test-data/live-players/squad/normal.txt");
const SQUAD_EMPTY: &str = include_str!("../../../test-data/live-players/squad/empty.txt");
const ZOMBOID: &str = include_str!("../../../test-data/live-players/zomboid/normal.txt");
const ZOMBOID_EMPTY: &str = include_str!("../../../test-data/live-players/zomboid/empty.txt");
const SEVEN_DAYS: &str = include_str!("../../../test-data/live-players/seven-days/normal.txt");
const SEVEN_DAYS_EMPTY: &str = include_str!("../../../test-data/live-players/seven-days/empty.txt");

fn decoded(codec: ModulePlayerListCodec, text: &str, actions: &[&str]) -> ParsedPlayerList {
    parse(
        codec,
        text,
        "request",
        NOW,
        &actions
            .iter()
            .map(|id| (*id).to_owned())
            .collect::<Vec<_>>(),
    )
    .expect("synthetic protocol fixture should decode")
}

#[test]
fn rust_projects_identity_and_telemetry_without_ip_address() {
    let list = decoded(
        ModulePlayerListCodec::RustPlayerList,
        RUST,
        &["kick_player", "ban_player"],
    );
    assert!(list.complete);
    assert_eq!(list.current_players, Some(2));
    assert_eq!(list.entries[0].display_name, "Builder, 北境");
    assert_eq!(list.entries[0].ping_ms, Some(42));
    assert_eq!(
        list.entries[0].session_started_at_unix_ms,
        Some(NOW - 125_500)
    );
    assert_eq!(list.entries[0].available_action_ids, ["kick_player"]);
    assert_eq!(
        list.bindings[&(list.entries[0].player_key.clone(), "kick_player".to_owned())],
        "76561190000000001"
    );
    assert!(
        !serde_json::to_string(&list.entries)
            .expect("serialize")
            .contains("192.0.2.1")
    );
}

#[test]
fn ark_retains_commas_and_distinguishes_platform_accounts() {
    let list = decoded(
        ModulePlayerListCodec::ArkListPlayers,
        ARK,
        &["kick_player", "ban_player"],
    );
    assert_eq!(list.entries[0].display_name, "Builder, 北境");
    assert_eq!(
        list.entries[0].identifiers[0].kind,
        RuntimePlayerIdentityKind::SteamId
    );
    assert_eq!(
        list.entries[1].identifiers[0].kind,
        RuntimePlayerIdentityKind::ArkAccountId
    );
    assert_eq!(list.bindings.len(), 4);
    let asa = decoded(ModulePlayerListCodec::ArkListPlayers, ASA, &["kick_player"]);
    assert_eq!(
        asa.entries[0].identifiers[0].kind,
        RuntimePlayerIdentityKind::EosId
    );
}

#[test]
fn conan_binds_game_user_ids_instead_of_platform_ids_or_character_names() {
    let list = decoded(
        ModulePlayerListCodec::ConanListPlayers,
        CONAN,
        &["kick_player", "ban_player"],
    );
    assert_eq!(list.entries[0].display_name, "Builder, 北境");
    assert_eq!(
        list.entries[0].identifiers[0].kind,
        RuntimePlayerIdentityKind::ConanUserId
    );
    assert_eq!(
        list.entries[0].identifiers[1].kind,
        RuntimePlayerIdentityKind::SteamId
    );
    assert_eq!(list.entries[1].identifiers.len(), 1);
    assert_eq!(
        list.bindings[&(list.entries[0].player_key.clone(), "kick_player".to_owned())],
        "0000000000000001"
    );
}

#[test]
fn squad_retains_spectators_and_binds_stable_platform_ids() {
    let list = decoded(
        ModulePlayerListCodec::SquadListPlayers,
        SQUAD,
        &["kick_player", "ban_player", "kick_player_id"],
    );
    assert_eq!(list.current_players, Some(2));
    assert_eq!(list.entries[0].identifiers.len(), 3);
    assert_eq!(list.entries[1].role, None);
    assert_eq!(
        list.bindings[&(list.entries[1].player_key.clone(), "kick_player".to_owned())],
        "00000000000000000000000000000002"
    );
    assert!(
        !list.entries[0]
            .available_action_ids
            .iter()
            .any(|id| id == "kick_player_id")
    );
    let unknown = SQUAD.replace("EOS: 00000000000000000000000000000002", "other: guest-2");
    let readonly = decoded(
        ModulePlayerListCodec::SquadListPlayers,
        &unknown,
        &["kick_player"],
    );
    assert!(readonly.entries[1].available_action_ids.is_empty());
}

#[test]
fn squad_disconnected_players_never_enter_the_online_roster() {
    let response =
        format!("{SQUAD}\nID: 9 | Online IDs: steam: 76561190000000009 | Name: Departed\n");
    let list = decoded(
        ModulePlayerListCodec::SquadListPlayers,
        &response,
        &["kick_player"],
    );
    assert_eq!(list.current_players, Some(2));
    assert!(
        !list
            .entries
            .iter()
            .any(|entry| entry.display_name == "Departed")
    );
    assert!(
        parse(
            ModulePlayerListCodec::SquadListPlayers,
            "----- Active Players -----\n",
            "request",
            NOW,
            &[]
        )
        .is_err()
    );
}

#[test]
fn zomboid_preserves_authoritative_usernames_and_checks_declared_count() {
    let list = decoded(
        ModulePlayerListCodec::ZomboidPlayers,
        ZOMBOID,
        &["kick_user", "ban_steamid", "add_to_whitelist"],
    );
    assert_eq!(list.entries[0].display_name, "Builder, 北境");
    assert_eq!(
        list.entries[0].identifiers[0].kind,
        RuntimePlayerIdentityKind::PlayerName
    );
    assert!(!list.entries[0].identifiers[0].stable);
    assert_eq!(
        list.entries[0].available_action_ids,
        ["kick_user", "add_to_whitelist"]
    );
    let spaced = decoded(
        ModulePlayerListCodec::ZomboidPlayers,
        "Players connected (1): \n- name with spaces \n",
        &["kick_user"],
    );
    assert_eq!(spaced.entries[0].identifiers[0].value, " name with spaces ");
    let indirect = decoded(
        ModulePlayerListCodec::ZomboidPlayers,
        "Players connected (1):  <LINE> -Somebody",
        &[],
    );
    assert_eq!(indirect.entries[0].display_name, "Somebody");
}

#[test]
fn only_explicit_empty_responses_produce_empty_lists() {
    for (codec, text) in [
        (ModulePlayerListCodec::RustPlayerList, RUST_EMPTY),
        (ModulePlayerListCodec::ArkListPlayers, ARK_EMPTY),
        (ModulePlayerListCodec::ConanListPlayers, CONAN_EMPTY),
        (ModulePlayerListCodec::SquadListPlayers, SQUAD_EMPTY),
        (ModulePlayerListCodec::ZomboidPlayers, ZOMBOID_EMPTY),
        (ModulePlayerListCodec::SevenDaysPlayers, SEVEN_DAYS_EMPTY),
    ] {
        let list = decoded(codec, text, &[]);
        assert_eq!(list.current_players, Some(0));
        assert!(list.complete);
        assert!(list.entries.is_empty());
        for malformed in ["", "\n", "Unknown command", "Authentication failed"] {
            assert!(parse(codec, malformed, "request", NOW, &[]).is_err());
        }
    }
}

#[test]
fn malformed_incomplete_and_duplicate_rows_never_become_authoritative() {
    for (codec, text) in [
        (ModulePlayerListCodec::RustPlayerList, "{}".to_owned()),
        (
            ModulePlayerListCodec::RustPlayerList,
            RUST.replace("76561190000000002", "76561190000000001"),
        ),
        (
            ModulePlayerListCodec::RustPlayerList,
            RUST.replace("\"Ping\":42", "\"Ping\":-1"),
        ),
        (
            ModulePlayerListCodec::ArkListPlayers,
            ARK.replace("1. Epic", "2. Epic"),
        ),
        (
            ModulePlayerListCodec::ArkListPlayers,
            ARK.replace("1234567890123456789", "not-an-id"),
        ),
        (
            ModulePlayerListCodec::ConanListPlayers,
            CONAN.replace("1 | Another", "2 | Another"),
        ),
        (
            ModulePlayerListCodec::ConanListPlayers,
            CONAN.replace("Builder, 北境", "Builder | 北境"),
        ),
        (
            ModulePlayerListCodec::SquadListPlayers,
            SQUAD.replace("Is Leader: True", "Is Leader: broken"),
        ),
        (
            ModulePlayerListCodec::SquadListPlayers,
            SQUAD.replace("ID: 4 |", "ID: 0 |"),
        ),
        (
            ModulePlayerListCodec::ZomboidPlayers,
            ZOMBOID.replace("(2)", "(3)"),
        ),
        (
            ModulePlayerListCodec::ZomboidPlayers,
            "Players connected (1):\nMissing prefix".to_owned(),
        ),
        (
            ModulePlayerListCodec::SevenDaysPlayers,
            SEVEN_DAYS.replace("Total of 2 in the game", ""),
        ),
        (
            ModulePlayerListCodec::SevenDaysPlayers,
            SEVEN_DAYS.replace("Total of 2", "Total of 1"),
        ),
        (
            ModulePlayerListCodec::SevenDaysPlayers,
            SEVEN_DAYS.replace("1. id=", "2. id="),
        ),
    ] {
        assert!(
            parse(codec, &text, "request", NOW, &[]).is_err(),
            "{codec:?} must reject the malformed response"
        );
    }
}

#[test]
fn display_limits_revoke_all_actions_and_keep_the_observed_total() {
    let rows: Vec<Value> = (0..257_u64).map(|index| serde_json::json!({
        "SteamID": (76561190000000000_u64 + index).to_string(), "DisplayName": format!("Player {index}")
    })).collect();
    let text = serde_json::to_string(&rows).expect("serialize synthetic rows");
    let list = decoded(
        ModulePlayerListCodec::RustPlayerList,
        &text,
        &["kick_player"],
    );
    assert!(list.truncated);
    assert!(!list.complete);
    assert_eq!(list.current_players, Some(257));
    assert_eq!(list.entries.len(), 256);
    assert!(list.bindings.is_empty());
    assert!(
        list.entries
            .iter()
            .all(|entry| entry.available_action_ids.is_empty())
    );
    assert!(
        parse(
            ModulePlayerListCodec::RustPlayerList,
            &" ".repeat(MAX_RESPONSE_BYTES + 1),
            "request",
            NOW,
            &[]
        )
        .is_err()
    );
    assert!(
        parse(
            ModulePlayerListCodec::RustPlayerList,
            "[\0]",
            "request",
            NOW,
            &[]
        )
        .is_err()
    );
}
