use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

const MAX_INPUT_BYTES: usize = 4096;
const MAX_MODS: usize = 16;

pub(super) fn input_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect()
}

#[derive(Debug, Serialize)]
pub(super) struct CustomModInputEvidence {
    mod_ids: Vec<String>,
    input_sha256: String,
}

#[derive(Debug)]
pub(super) struct NativeModInput {
    pub(super) mod_ids: Vec<String>,
    pub(super) custom_evidence: Option<CustomModInputEvidence>,
}

// Test inputs only. Workshop discovery, author configuration, world generation,
// native required-prefab checks and map/player/save checks retain their normal
// implementations. An override must never silently redefine a named fixture.
pub(super) fn resolve(scenario: &str, custom_json: Option<&str>) -> Result<NativeModInput, String> {
    if scenario == "custom_mods" {
        let raw = custom_json.ok_or("custom_mods requires LANGAME_DST_SMOKE_CUSTOM_MODS_JSON")?;
        if raw.len() > MAX_INPUT_BYTES {
            return Err("Native custom Mod input exceeds 4096 UTF-8 bytes".into());
        }
        let ids: Vec<String> = serde_json::from_str(raw).map_err(|error| {
            format!("Native custom Mod input must be a JSON string array: {error}")
        })?;
        if ids.is_empty() || ids.len() > MAX_MODS {
            return Err("Native custom Mod input must contain 1..16 Workshop IDs".into());
        }
        let mut unique = HashSet::new();
        for id in &ids {
            let numeric = id.parse::<u64>().ok();
            if id.is_empty()
                || id.len() > 20
                || !id.bytes().all(|byte| byte.is_ascii_digit())
                || !numeric.is_some_and(|value| value != 0 && value.to_string() == *id)
                || !unique.insert(id.as_str())
            {
                return Err(
                    "Native custom Mod IDs must be unique canonical nonzero u64 decimals".into(),
                );
            }
        }
        return Ok(NativeModInput {
            custom_evidence: Some(CustomModInputEvidence {
                mod_ids: ids.clone(),
                // Bind the exact supplied bytes, including whitespace, rather
                // than replacing the caller's input with normalized JSON.
                input_sha256: input_sha256(raw.as_bytes()),
            }),
            mod_ids: ids,
        });
    }
    if custom_json.is_some() {
        return Err("Custom Mod input is accepted only by the custom_mods scenario".into());
    }
    let ids: &[&str] = match scenario {
        "vanilla"
        | "shadowy_depths"
        | "events"
        | "invalid_mod_lua"
        | "invalid_caves_mod_lua"
        | "invalid_world_lua"
        | "invalid_world_preset" => &[],
        "uncompromising" => &["2039181790"],
        "myth_words" => &["1991746508", "1699194522"],
        "legion" => &["1392778117"],
        "legend_and_sea" => &["2979177306"],
        "creeps_deeps" => &["2085762931"],
        "combined" | "combined_recovery" | "configured_mods" => &["2039181790", "1392778117"],
        "tropical" | "tropical_ocean" => &["1505270912"],
        "cherry_forest" => &["1289779251"],
        "tropical_adventures" => &["2986194136"],
        "tropical_cherry" => &["1505270912", "1289779251"],
        "adventures_cherry" => &["2986194136", "1289779251"],
        "island_adventures" => &["3435352667", "1467214795"],
        _ => return Err(format!("Unknown native smoke scenario: {scenario}")),
    };
    Ok(NativeModInput {
        mod_ids: ids.iter().map(|id| (*id).to_owned()).collect(),
        custom_evidence: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_ids_preserve_authoritative_order_without_a_named_mod_branch() {
        let input = resolve("custom_mods", Some(r#"["9","3","18446744073709551615"]"#)).unwrap();
        assert_eq!(input.mod_ids, ["9", "3", "18446744073709551615"]);
        assert!(input.custom_evidence.is_some());
    }

    #[test]
    fn all_existing_native_scenario_ids_and_order_are_preserved() {
        let cases: &[(&str, &[&str])] = &[
            ("vanilla", &[]),
            ("shadowy_depths", &[]),
            ("events", &[]),
            ("invalid_mod_lua", &[]),
            ("invalid_caves_mod_lua", &[]),
            ("invalid_world_lua", &[]),
            ("invalid_world_preset", &[]),
            ("uncompromising", &["2039181790"]),
            ("myth_words", &["1991746508", "1699194522"]),
            ("legion", &["1392778117"]),
            ("legend_and_sea", &["2979177306"]),
            ("creeps_deeps", &["2085762931"]),
            ("combined", &["2039181790", "1392778117"]),
            ("combined_recovery", &["2039181790", "1392778117"]),
            ("configured_mods", &["2039181790", "1392778117"]),
            ("tropical", &["1505270912"]),
            ("tropical_ocean", &["1505270912"]),
            ("cherry_forest", &["1289779251"]),
            ("tropical_adventures", &["2986194136"]),
            ("tropical_cherry", &["1505270912", "1289779251"]),
            ("adventures_cherry", &["2986194136", "1289779251"]),
            ("island_adventures", &["3435352667", "1467214795"]),
        ];
        for (scenario, expected) in cases {
            let input = resolve(scenario, None).unwrap();
            assert_eq!(input.mod_ids, *expected, "{scenario}");
            assert!(input.custom_evidence.is_none(), "{scenario}");
        }
    }

    #[test]
    fn overrides_cannot_redefine_builtin_or_unknown_scenarios() {
        for scenario in [
            "vanilla",
            "myth_words",
            "configured_mods",
            "island_adventures",
            "unknown",
        ] {
            for raw in [r#"["9"]"#, "", "null"] {
                assert!(resolve(scenario, Some(raw)).is_err(), "{scenario}: {raw}");
            }
        }
        assert!(resolve("unknown", None).is_err());
    }

    #[test]
    fn custom_input_requires_a_nonempty_json_string_array_and_complete_document() {
        assert!(resolve("custom_mods", None).is_err());
        for raw in [
            "",
            " ",
            "null",
            "{}",
            r#"{"modIds":["9"]}"#,
            "[]",
            "[9]",
            "[null]",
            r#"["9",false]"#,
            r#"["9"] []"#,
            r#"["9",]"#,
        ] {
            assert!(resolve("custom_mods", Some(raw)).is_err(), "{raw}");
        }
    }

    #[test]
    fn workshop_ids_reject_aliases_paths_unicode_zero_and_overflow() {
        for id in [
            "",
            "0",
            "00",
            "01",
            "+1",
            "-1",
            " 1",
            "1 ",
            "1.0",
            "1e3",
            "workshop-1",
            "../1",
            "1/2",
            "１",
            "١",
            "18446744073709551616",
            "\u{0}",
            "1\n",
        ] {
            let raw = serde_json::to_string(&[id]).unwrap();
            assert!(resolve("custom_mods", Some(&raw)).is_err(), "{id:?}");
        }
    }

    #[test]
    fn duplicate_ids_are_rejected_and_the_mod_count_limit_is_exact() {
        assert!(resolve("custom_mods", Some(r#"["3","7","3"]"#)).is_err());
        let mut ids: Vec<_> = (1..=MAX_MODS).map(|id| id.to_string()).collect();
        let raw = serde_json::to_string(&ids).unwrap();
        assert_eq!(resolve("custom_mods", Some(&raw)).unwrap().mod_ids, ids);
        ids.push((MAX_MODS + 1).to_string());
        let raw = serde_json::to_string(&ids).unwrap();
        assert!(resolve("custom_mods", Some(&raw)).is_err());
    }

    #[test]
    fn json_byte_limit_accepts_the_boundary_and_rejects_one_more_byte() {
        let raw = format!("{}[\"1\"]", " ".repeat(MAX_INPUT_BYTES - 5));
        assert_eq!(raw.len(), MAX_INPUT_BYTES);
        assert_eq!(resolve("custom_mods", Some(&raw)).unwrap().mod_ids, ["1"]);
        assert!(resolve("custom_mods", Some(&(raw + " "))).is_err());
        assert!(resolve("custom_mods", Some(&" ".repeat(MAX_INPUT_BYTES + 1))).is_err());
    }

    #[test]
    fn native_evidence_binds_the_exact_input_bytes_and_selected_order() {
        let compact = resolve("custom_mods", Some(r#"["9","3"]"#)).unwrap();
        let spaced = resolve("custom_mods", Some("[ \"9\", \"3\" ]")).unwrap();
        assert_eq!(compact.mod_ids, spaced.mod_ids);
        let compact = serde_json::to_value(compact.custom_evidence.unwrap()).unwrap();
        let spaced = serde_json::to_value(spaced.custom_evidence.unwrap()).unwrap();
        assert_eq!(compact["mod_ids"], serde_json::json!(["9", "3"]));
        assert_eq!(
            compact["input_sha256"],
            "AE35442F9181175A4FF49B641F3C32E2FF98E0DBF02F7F186180C3B55AE4DB6D"
        );
        assert_ne!(compact["input_sha256"], spaced["input_sha256"]);
    }
}
