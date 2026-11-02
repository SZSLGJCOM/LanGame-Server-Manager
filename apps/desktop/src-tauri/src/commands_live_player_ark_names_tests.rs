use super::*;
use crate::live_players::response_codecs;

const REQUEST: &str = "0123456789abcdef0123456789abcdef";
const STEAM: &str = "76561198000000001";
const EOS: &str = "0123456789abcdef0123456789abcdef";

fn native_list(text: &str) -> response_codecs::ParsedPlayerList {
    response_codecs::parse(
        app_core::ModulePlayerListCodec::ArkListPlayers,
        text,
        REQUEST,
        1000,
        &["kick_player".into(), "ban_player".into()],
    )
    .unwrap()
}

fn player(id: u32, account: &str, account_name: Option<&str>) -> protocol::Player {
    protocol::Player {
        player_id: id,
        name: "Different character name".into(),
        account_name: account_name.map(str::to_owned),
        account_id: account.into(),
        team: 1000,
        x: None,
        y: None,
        z: None,
    }
}

#[test]
fn escaped_unicode_restores_only_the_matching_account_name_and_preserves_actions() {
    for (edition, module, account) in [
        ("ase", "arksurvivalevolved", STEAM),
        ("asa", "arksurvivalascended", EOS),
    ] {
        // These are synthetic native-wire responses: stock ListPlayers has
        // already lost the Unicode; the extension keeps it as ASCII escapes.
        let mut native = native_list(&format!("0. ??, {account}\n1. Alice, 76561198000000002"));
        let bindings = native.bindings.clone();
        let identifiers = serde_json::to_value(&native.entries[0].identifiers).unwrap();
        let actions = native.entries[0].available_action_ids.clone();
        let key = native.entries[0].player_key.clone();
        let response = format!(
            r#"LGSM_ARK_TOOLS {{"version":1,"edition":"{edition}","action":"players","requestId":"{REQUEST}","ok":true,"mapName":"TheIsland","projection":null,"players":[{{"playerId":123,"name":"Character","accountId":"{account}","accountName":"\u4e2d\u6587, \ud83d\ude80","team":1000,"x":null,"y":null,"z":null}}]}}"#
        );
        assert!(response.is_ascii());
        let snapshot = protocol::snapshot(&response, module, REQUEST, "TheIsland").unwrap();
        assert!(apply_names(
            &mut native.entries,
            &extension_names(&snapshot.players).unwrap()
        ));
        assert_eq!(native.entries[0].display_name, "中文, 🚀");
        assert_eq!(native.entries[1].display_name, "Alice");
        assert_eq!(native.entries[0].player_key, key);
        assert_eq!(
            serde_json::to_value(&native.entries[0].identifiers).unwrap(),
            identifiers
        );
        assert_eq!(native.entries[0].available_action_ids, actions);
        assert_eq!(native.bindings, bindings);
        assert_eq!(native.current_players, Some(2));
        assert!(native.complete && !native.truncated);
    }
}

#[test]
fn missing_names_never_use_character_names_or_match_by_row_order() {
    for account_name in [None, Some(""), Some(" "), Some("\u{fffd}")] {
        let mut native = native_list(&format!("0. ??, {STEAM}\n1. ??, {EOS}"));
        let names = [
            player(123, STEAM, account_name),
            player(456, "76561198000000003", Some("Other account")),
        ];
        assert!(!apply_names(
            &mut native.entries,
            &extension_names(&names).unwrap()
        ));
        assert!(
            native
                .entries
                .iter()
                .all(|entry| entry.display_name == "??")
        );
        assert_eq!(native.current_players, Some(2));
        assert_eq!(native.bindings.len(), 4);
    }
}

#[test]
fn ambiguous_extension_accounts_are_rejected_before_any_name_changes() {
    let native = native_list(&format!("0. ??, {STEAM}"));
    let names = [
        player(123, STEAM, Some("First")),
        player(456, STEAM, Some("Second")),
    ];
    assert!(extension_names(&names).is_err());
    assert_eq!(native.entries[0].display_name, "??");
}

#[test]
fn replacement_characters_are_repaired_but_valid_names_are_not_overwritten() {
    let mut native = native_list(&format!("0. \u{fffd}, {STEAM}\n1. 中文原名, {EOS}"));
    let names = [
        player(1, STEAM, Some("玩家?")),
        player(2, EOS, Some("Different")),
    ];
    assert!(apply_names(
        &mut native.entries,
        &extension_names(&names).unwrap()
    ));
    assert_eq!(native.entries[0].display_name, "玩家?");
    assert_eq!(native.entries[1].display_name, "中文原名");
    assert!(!needs_unicode_name(&native.entries[1]));
}
