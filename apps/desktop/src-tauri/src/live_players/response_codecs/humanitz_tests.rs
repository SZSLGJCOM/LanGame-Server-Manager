use super::response_is_complete;
use crate::live_players::response_codecs::parse;
use app_core::ModulePlayerListCodec;

const INFO: &str = include_str!("../../../test-data/live-players/humanitz/normal.txt");
const EMPTY: &str = include_str!("../../../test-data/live-players/humanitz/empty.txt");

#[test]
fn counted_names_are_read_only_and_duplicate_names_remain_distinct() {
    let text = INFO.replace("Another Survivor", "Builder, 北境");
    let list = parse(
        ModulePlayerListCodec::HumanitzPlayers,
        &text,
        "synthetic-request",
        1,
        &["kick_player".into(), "ban_player".into()],
    )
    .unwrap();
    assert_eq!(list.current_players, Some(2));
    assert!(list.complete);
    assert!(list.bindings.is_empty());
    assert_ne!(list.entries[0].player_key, list.entries[1].player_key);
    for entry in list.entries {
        assert_eq!(entry.display_name, "Builder, 北境");
        assert!(entry.identifiers.is_empty());
        assert!(entry.available_action_ids.is_empty());
    }
    let list = parse(
        ModulePlayerListCodec::HumanitzPlayers,
        EMPTY,
        "empty",
        1,
        &[],
    )
    .unwrap();
    assert_eq!(list.current_players, Some(0));
    assert!(list.entries.is_empty());
}

#[test]
fn completion_requires_count_and_terminated_names() {
    assert!(response_is_complete(INFO).unwrap());
    assert!(response_is_complete(&INFO.replace('\n', "\r\n")).unwrap());
    assert!(response_is_complete(EMPTY).unwrap());
    assert!(!response_is_complete(INFO.trim_end_matches('\n')).unwrap());
    let prefix = INFO.split("Another Survivor").next().unwrap();
    assert!(!response_is_complete(prefix).unwrap());
    assert!(!response_is_complete(&format!("{prefix}Another Surv")).unwrap());
}

#[test]
fn native_zero_player_marker_completes_without_a_final_newline() {
    let text = format!("{}No players connected", EMPTY.replace('\n', "\r\n"));
    assert!(response_is_complete(&text).unwrap());
    let list = parse(
        ModulePlayerListCodec::HumanitzPlayers,
        &text,
        "empty",
        1,
        &[],
    )
    .unwrap();
    assert_eq!(list.current_players, Some(0));
    assert!(list.complete && list.entries.is_empty());
    assert!(!response_is_complete(text.trim_end_matches("connected")).unwrap());
    for invalid in [
        format!("{text}\r\nNo players connected"),
        format!("{text}\r\nUnexpected player\r\n"),
        text.replace("0 connected.", "2 connected."),
    ] {
        assert!(
            parse(
                ModulePlayerListCodec::HumanitzPlayers,
                &invalid,
                "bad",
                1,
                &[]
            )
            .is_err()
        );
    }
    let named_player = text.replace("0 connected.", "1 connected.");
    assert!(!response_is_complete(&named_player).unwrap());
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
        assert!(parse(ModulePlayerListCodec::HumanitzPlayers, &text, "bad", 1, &[]).is_err());
    }
}
