use super::*;
use serde_json::json;

fn schema() -> Map<String, Value> {
    let schema: Value =
        serde_json::from_str(include_str!("../../../modules/enshrouded/schema.json")).unwrap();
    schema["properties"]["banned_player_ids"]
        .as_object()
        .unwrap()
        .clone()
}

#[test]
fn enshrouded_uint64_mutations_preserve_exact_strings_and_existing_17_digit_values() {
    let schema = schema();
    let codec = parse_codec("enshrouded", "banned_player_ids", &schema).unwrap();
    assert_eq!(codec, PlayerAccessCodec::Uint64);
    for value in [
        "0",
        "00123",
        "9007199254740993",
        "76561198000000001",
        "18446744073709551615",
    ] {
        let entry = normalize_entry(
            "enshrouded",
            "banned_player_ids",
            codec,
            &schema,
            PlayerAccessMutationOperation::Add,
            &json!(value),
        )
        .unwrap();
        assert_eq!(entry.stored, json!(value));
        assert_eq!(entry.canonical_identity, value);
        let original = json!("76561198000000002,18446744073709551614");
        let added = patch_field_value(
            "enshrouded",
            "banned_player_ids",
            codec,
            &schema,
            PlayerAccessMutationOperation::Add,
            &original,
            &entry,
        )
        .unwrap();
        assert_eq!(
            added.value,
            json!(format!("76561198000000002\n18446744073709551614\n{value}"))
        );
        let removed = patch_field_value(
            "enshrouded",
            "banned_player_ids",
            codec,
            &schema,
            PlayerAccessMutationOperation::Remove,
            &added.value,
            &entry,
        )
        .unwrap();
        assert_eq!(
            removed.value,
            json!("76561198000000002\n18446744073709551614")
        );
    }
}

#[test]
fn enshrouded_uint64_rejects_invalid_mutations_and_whole_rosters_atomically() {
    let schema = schema();
    for value in [
        "",
        "-1",
        "+1",
        "1.0",
        "1e3",
        "0xff",
        "12 3",
        "１２３",
        "18446744073709551616",
    ] {
        assert!(
            normalize_entry(
                "enshrouded",
                "banned_player_ids",
                PlayerAccessCodec::Uint64,
                &schema,
                PlayerAccessMutationOperation::Add,
                &json!(value),
            )
            .is_err(),
            "{value}"
        );
        if value.is_empty() {
            continue;
        }
        let mut settings = json!({
            "banned_player_ids": format!("76561198000000001,{value},18446744073709551615"),
            "server_name": "keep",
        })
        .as_object()
        .unwrap()
        .clone();
        let original = settings.clone();
        assert!(
            normalize_module_player_access_settings_strict("enshrouded", &mut settings).is_err()
        );
        assert_eq!(settings, original);
    }
    assert!(
        normalize_entry(
            "enshrouded",
            "banned_player_ids",
            PlayerAccessCodec::Uint64,
            &schema,
            PlayerAccessMutationOperation::Add,
            &json!(9007199254740993_u64),
        )
        .is_err()
    );
}

#[test]
fn enshrouded_uint64_full_settings_preserve_lists_without_platform_conversion() {
    for value in [
        "",
        "00123,76561198000000001\n18446744073709551615,\n",
        "# comment\n0",
    ] {
        let mut settings = json!({"banned_player_ids": value})
            .as_object()
            .unwrap()
            .clone();
        let original = settings.clone();
        assert!(
            !normalize_module_player_access_settings_strict("enshrouded", &mut settings).unwrap()
        );
        assert_eq!(settings, original);
    }
    let mut settings = json!({"banned_player_ids": 123})
        .as_object()
        .unwrap()
        .clone();
    assert!(normalize_module_player_access_settings_strict("enshrouded", &mut settings).is_err());
}

#[test]
fn enshrouded_uint64_schema_diagnostics_validate_exact_boundaries() {
    use crate::settings_validation::{
        SettingsValidationPhase, collect_settings_schema_diagnostics,
    };
    let schema = json!({"type": "object", "properties": {"banned_player_ids": schema()}});
    for (value, expected) in [
        ("9007199254740993,18446744073709551615", 0),
        ("18446744073709551616", 1),
        ("123,invalid,456", 1),
    ] {
        let settings = json!({"banned_player_ids": value});
        let diagnostics = collect_settings_schema_diagnostics(
            &schema,
            settings.as_object().unwrap(),
            SettingsValidationPhase::Complete,
        );
        assert_eq!(diagnostics.len(), expected, "{value}: {diagnostics:?}");
    }
}
