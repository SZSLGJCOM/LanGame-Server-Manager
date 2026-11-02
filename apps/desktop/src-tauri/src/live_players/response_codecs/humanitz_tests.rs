use super::{info_player_count, response_is_complete};
use crate::live_players::response_codecs::parse;
use app_core::ModulePlayerListCodec;

const INFO: &str = include_str!("../../../test-data/live-players/humanitz/normal.txt");
const EMPTY: &str = include_str!("../../../test-data/live-players/humanitz/empty.txt");

#[test]
fn info_counts_duplicate_names_without_treating_them_as_identities() {
    let text = INFO
        .trim_end_matches('\n')
        .replace("Another Survivor", "Builder, 北境");
    assert_eq!(info_player_count(&text).unwrap(), Some(2));
    assert!(
        parse(
            ModulePlayerListCodec::HumanitzPlayers,
            &text,
            "info",
            1,
            &[]
        )
        .is_err()
    );
    let list = parse(
        ModulePlayerListCodec::HumanitzPlayers,
        &serde_json::json!({"info": EMPTY, "players": ""}).to_string(),
        "empty",
        1,
        &[],
    )
    .unwrap();
    assert_eq!(list.current_players, Some(0));
    assert!(list.entries.is_empty());
}

#[test]
fn complete_frames_require_the_declared_count_without_a_final_newline() {
    assert!(response_is_complete(INFO).unwrap());
    assert!(response_is_complete(&INFO.replace('\n', "\r\n")).unwrap());
    assert!(response_is_complete(EMPTY).unwrap());
    assert!(response_is_complete(INFO.trim_end_matches('\n')).unwrap());
    let prefix = INFO.split("Another Survivor").next().unwrap();
    assert!(!response_is_complete(prefix).unwrap());
    assert!(response_is_complete(&format!("{prefix}Another Survivor")).unwrap());
}

#[test]
fn native_zero_player_marker_completes_without_a_final_newline() {
    let text = format!("{}No players connected", EMPTY.replace('\n', "\r\n"));
    assert!(response_is_complete(&text).unwrap());
    let list = parse(
        ModulePlayerListCodec::HumanitzPlayers,
        &serde_json::json!({"info": text, "players": ""}).to_string(),
        "empty",
        1,
        &[],
    )
    .unwrap();
    assert_eq!(list.current_players, Some(0));
    assert!(list.complete && list.entries.is_empty());
    assert!(response_is_complete(text.trim_end_matches("connected")).is_err());
    for invalid in [
        format!("{text}\r\nNo players connected"),
        format!("{text}\r\nUnexpected player\r\n"),
        text.replace("0 connected.", "2 connected."),
    ] {
        assert!(!matches!(info_player_count(&invalid), Ok(Some(0))));
    }
    let named_player = text.replace("0 connected.", "1 connected.");
    // This marker is special only at count zero; it can also be a display name.
    assert!(response_is_complete(&named_player).unwrap());
}

#[test]
fn short_unknown_repeated_and_oversized_counts_never_become_empty() {
    for text in [
        String::from("Unknown command\n"),
        String::from("Players:\n"),
        INFO.replace("2 connected.", "3 connected."),
        INFO.replace("2 connected.", "1 connected."),
        INFO.replace("2 connected.", "4097 connected."),
        INFO.replace("2 connected.", "+2 connected."),
        INFO.replace("2 connected.", "2 connected.\n2 connected."),
        INFO.replace("FPS: 60\n", ""),
        INFO.replace("FPS: 60\n", "FPS: 60\nFPS: 60\n"),
        INFO.replace("Another Survivor", "broken\tname"),
        INFO.replace("Another Survivor", &"x".repeat(257)),
    ] {
        assert!(!matches!(info_player_count(&text), Ok(Some(_))));
    }
}

const NET_ID: &str = "76561190000000000_+_|0123456789abcdef0123456789abcdef";

fn capture(info: &str, rows: &str) -> String {
    serde_json::json!({"info": info, "players": rows}).to_string()
}

#[test]
fn native_players_capture_binds_the_complete_identity_to_the_declared_kick() {
    let info: String = serde_json::from_str(include_str!(
        "../../../test-data/live-players/humanitz/native-one-player.json"
    ))
    .unwrap();
    let rows: String = serde_json::from_str(include_str!(
        "../../../test-data/live-players/humanitz/native-players.json"
    ))
    .unwrap();
    let list = parse(
        ModulePlayerListCodec::HumanitzPlayers,
        &capture(&info, &rows),
        "native",
        1,
        &["kick_player".into(), "ban_player".into()],
    )
    .unwrap();
    assert!(list.complete && !list.truncated);
    assert_eq!(list.current_players, Some(1));
    let entry = &list.entries[0];
    assert_eq!(entry.display_name, "Survivor北境");
    assert_eq!(
        entry.identifiers[0].kind,
        app_core::RuntimePlayerIdentityKind::HumanitzNetId
    );
    assert!(entry.identifiers[0].stable);
    assert_eq!(entry.identifiers[0].value, NET_ID);
    assert_eq!(entry.available_action_ids, ["kick_player"]);
    assert_eq!(
        list.bindings
            .get(&(entry.player_key.clone(), "kick_player".into()))
            .unwrap(),
        NET_ID
    );
    let readonly = parse(
        ModulePlayerListCodec::HumanitzPlayers,
        &capture(&info, &rows),
        "readonly",
        1,
        &[],
    )
    .unwrap();
    assert!(readonly.entries[0].available_action_ids.is_empty());
    assert!(readonly.bindings.is_empty());
}

#[test]
fn same_names_do_not_merge_and_duplicate_or_malformed_identities_fail_closed() {
    let row = format!("Builder (北境) ({NET_ID}) Lv:0 Clan:Clan (one) DPassed:0");
    let other = row.replace(NET_ID, "|ABCDEF0123456789ABCDEF0123456789");
    let list = parse(
        ModulePlayerListCodec::HumanitzPlayers,
        &capture(INFO, &format!("{row}\r\n{other}")),
        "duplicates",
        1,
        &["kick_player".into()],
    )
    .unwrap();
    assert_eq!(list.entries.len(), 2);
    assert_eq!(list.entries[0].display_name, list.entries[1].display_name);
    assert_ne!(
        list.entries[0].identifiers[0].value,
        list.entries[1].identifiers[0].value
    );
    for rows in [
        format!("{row}\r\n{row}"),
        row.clone(),
        format!("{row}\r\n{}", other.replace(" DPassed:0", "")),
        format!(
            "{row}\r\n{}",
            other.replace("|ABCDEF0123456789ABCDEF0123456789", "76561190000000000")
        ),
        format!("{row}\r\n{}", other.replace("Lv:0", "Lv:-1")),
        format!("{row}\r\n{other}\r\nUnexpected player"),
    ] {
        assert!(
            parse(
                ModulePlayerListCodec::HumanitzPlayers,
                &capture(INFO, &rows),
                "invalid",
                1,
                &["kick_player".into()]
            )
            .is_err()
        );
    }
    assert!(
        parse(
            ModulePlayerListCodec::HumanitzPlayers,
            &capture(EMPTY, &row),
            "invalid-empty",
            1,
            &["kick_player".into()]
        )
        .is_err()
    );
}
