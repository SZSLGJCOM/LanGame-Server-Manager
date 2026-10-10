use super::*;
use base64::Engine;
use serde_json::json;

fn values(key: &str, value: &str) -> BTreeMap<String, String> {
    BTreeMap::from([(key.into(), value.into())])
}

#[test]
fn creation_uses_native_percentage_choices_and_non_sequential_purity_values() {
    for value in ["25", "50", "75", "100", "200", "500"] {
        assert!(
            validate_rules(
                &values("FG.GameMode.EnergyCostMultiplier", value),
                true,
                true
            )
            .is_ok()
        );
    }
    for value in ["0.25", "1", "125", "1000"] {
        assert!(
            validate_rules(
                &values("FG.GameMode.EnergyCostMultiplier", value),
                true,
                true
            )
            .is_err()
        );
    }
    assert!(
        validate_rules(
            &values("FG.GameMode.SpacePartsCostMultiplier", "10000"),
            true,
            true
        )
        .is_ok()
    );
    assert!(validate_rules(&values("FG.GameMode.NodePuritySettings", "5"), true, true).is_ok());
    assert!(validate_rules(&values("FG.GameMode.NodeRandomization", "5"), true, true).is_err());
}

#[test]
fn creation_settings_and_immediate_commands_cannot_be_written_to_existing_worlds() {
    for key in [
        "FG.GameMode.NodePuritySettings",
        "FG.GameRules.StartingTier",
        "FG.GameRules.GiveAllTiers",
        "FG.GameRules.UnlockAllResearchSchematics",
    ] {
        assert!(validate_rules(&values(key, "True"), false, false).is_err());
    }
    for key in [
        "FG.GameRules.GiveItems",
        "FG.GameRules.SetGamePhase",
        "FG.PlayerRules.KeepInventory",
        "FG.PlayerRules.CreatureHostilityMode",
    ] {
        assert!(validate_rules(&values(key, "0"), true, false).is_err());
    }
    assert_eq!(
        validate_rules(&values("FG.PlayerRules.FlightMode", "true"), false, false).unwrap()["FG.PlayerRules.FlightMode"],
        "True"
    );
}

#[test]
fn seed_bounds_and_canonical_integer_format_are_checked() {
    for value in ["0", "2147483647", "-2147483648"] {
        assert!(
            validate_rules(
                &values("FG.GameMode.NodeRandomizationSeed", value),
                true,
                true
            )
            .is_ok()
        );
    }
    for value in ["2147483648", "-2147483649", "00", "+1", "1.0"] {
        assert!(
            validate_rules(
                &values("FG.GameMode.NodeRandomizationSeed", value),
                true,
                true
            )
            .is_err()
        );
    }
}

#[test]
fn world_names_cannot_inject_native_map_url_options() {
    assert!(validate_session_name("中文世界 2").is_ok());
    for name in [
        "",
        " ",
        "world?admin=true",
        "../world",
        "world\nother",
        " world",
        "world:part",
    ] {
        assert!(validate_session_name(name).is_err(), "{name}");
    }
}

#[test]
fn revision_survives_autosaves_and_player_count_changes_but_binds_world_rules() {
    let mut snapshot = empty_snapshot(
        "fixture",
        SatisfactoryConnectionStatus::Ready,
        Some("Fixture".into()),
    )
    .unwrap();
    snapshot.active_session_name = "World A".into();
    snapshot.is_game_running = true;
    let baseline = revision(&snapshot).unwrap();
    snapshot.connected_players = 3;
    snapshot.sessions.push(SatisfactorySession {
        session_name: "World A".into(),
        saves: vec![SatisfactorySave {
            save_name: "autosave".into(),
            save_date_time: "2026.10.09-15.31.49".into(),
            play_duration_seconds: 400,
            is_creative_mode_enabled: false,
        }],
    });
    assert_eq!(revision(&snapshot).unwrap(), baseline);
    snapshot.active_session_name = "World B".into();
    assert_ne!(revision(&snapshot).unwrap(), baseline);
    snapshot.active_session_name = "World A".into();
    snapshot
        .advanced_game_settings
        .insert("FG.GameRules.NoPower".into(), "True".into());
    assert_ne!(revision(&snapshot).unwrap(), baseline);
}

#[test]
fn application_token_parser_requires_current_native_prefix_and_privilege() {
    let payload = base64::engine::general_purpose::STANDARD.encode(br#"{"pl":"APIToken"}"#);
    let token = format!("{payload}.{}", "1a".repeat(64));
    let line = format!("New Server API Authentication Token: {token}\n");
    assert_eq!(parse_api_token(&line).unwrap(), token);
    assert_eq!(
        application_token_fingerprint(&token).unwrap(),
        "1a".repeat(64)
    );
    assert!(application_token_fingerprint("not-an-application-token").is_err());
    assert!(parse_api_token(&format!("{line}{line}")).is_err());
    assert!(parse_api_token(&token).is_err());
    assert!(parse_api_token(&line.replace("Authentication Token", "Token")).is_err());
    let admin = base64::engine::general_purpose::STANDARD.encode(br#"{"pl":"Administrator"}"#);
    assert!(parse_api_token(&line.replace(&payload, &admin)).is_err());
    assert!(parse_api_token(&line.replace(&"1a".repeat(64), "short")).is_err());
}

#[test]
fn save_collection_rejects_duplicate_and_path_like_game_owned_names() {
    let header = json!({"saveName":"world_0", "saveDateTime":"2026.10.09-15.31.49", "playDurationSeconds":1, "isCreativeModeEnabled":false});
    let native = json!({"sessions":[{"sessionName":"World", "saveHeaders":[header.clone()]}],"currentSessionIndex":0});
    assert_eq!(sessions(native).unwrap()[0].saves[0].save_name, "world_0");
    let duplicate =
        json!({"sessions":[{"sessionName":"World", "saveHeaders":[header.clone(), header]}]});
    assert!(sessions(duplicate).is_err());
    assert!(sessions(json!({"sessions":[{"sessionName":"World", "saveHeaders":[{"saveName":"../private","saveDateTime":"", "playDurationSeconds":0,"isCreativeModeEnabled":false}]}]})).is_err());
}
