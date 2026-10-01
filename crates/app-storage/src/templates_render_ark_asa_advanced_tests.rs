use super::*;
use serde_json::json;

#[test]
fn asa_platform_template_override_is_optional_and_preserves_explicit_false() {
    let schema: Value = serde_json::from_str(include_str!(
        "../../../modules/arksurvivalascended/schema.json"
    ))
    .unwrap();
    assert!(
        schema["properties"]["prevent_template_on_saddle"]
            .get("default")
            .is_none()
    );
    for (settings, expected) in [
        (json!({}), ""),
        (
            json!({"prevent_template_on_saddle": true}),
            "PreventTemplateOnSaddle=true\n",
        ),
        (
            json!({"prevent_template_on_saddle": false}),
            "PreventTemplateOnSaddle=false\n",
        ),
    ] {
        assert_eq!(
            render_ark_native_ini_lines(
                settings.as_object().unwrap(),
                &Map::new(),
                ARK_ASA_PATCH_GUS_SERVER_SETTINGS,
            ),
            expected,
        );
    }
}
#[test]
fn asa_advanced_values_render_without_changing_repeated_rule_order() {
    let settings = json!({
        "supply_crate_loot_quality_multiplier": 2.5,
        "override_max_experience_points_player": 120000,
        "override_max_experience_points_dino": 85000,
        "auto_unlock_all_engrams": true,
        "override_player_level_engram_points": "8\r\n\r\nOverridePlayerLevelEngramPoints=12\n0",
        "npc_replacements": "(FromClassName=\"Raptor_Character_BP_C\",ToClassName=\"\")"
    });

    let output = render_ark_native_ini_lines(
        settings.as_object().unwrap(),
        &Map::new(),
        ARK_ASA_ADVANCED_GAME_INI,
    );

    assert_eq!(
        output,
        concat!(
            "SupplyCrateLootQualityMultiplier=2.5\n",
            "OverrideMaxExperiencePointsPlayer=120000\n",
            "OverrideMaxExperiencePointsDino=85000\n",
            "bAutoUnlockAllEngrams=true\n",
            "NPCReplacements=(FromClassName=\"Raptor_Character_BP_C\",ToClassName=\"\")\n",
            "OverridePlayerLevelEngramPoints=8\n",
            "OverridePlayerLevelEngramPoints=12\n",
            "OverridePlayerLevelEngramPoints=0\n",
        )
    );
}

#[test]
fn asa_stat_rules_keep_base_addition_affinity_and_full_native_keys() {
    let settings = json!({
        "per_level_stats_multiplier_dino_tamed_type_integer":
            "[0]=0.2\n_Add[0]=0.14\n_Affinity[0]=0.44\nPerLevelStatsMultiplier_DinoTamed[8]=0.17",
        "per_level_stats_multiplier_dino_wild_integer": "0=1.5\n[7]=2.0"
    });
    let output = render_ark_native_ini_lines(
        settings.as_object().unwrap(),
        &Map::new(),
        ARK_ASA_ADVANCED_GAME_INI,
    );

    assert_eq!(
        output,
        concat!(
            "PerLevelStatsMultiplier_DinoTamed[0]=0.2\n",
            "PerLevelStatsMultiplier_DinoTamed_Add[0]=0.14\n",
            "PerLevelStatsMultiplier_DinoTamed_Affinity[0]=0.44\n",
            "PerLevelStatsMultiplier_DinoTamed[8]=0.17\n",
            "PerLevelStatsMultiplier_DinoWild[0]=1.5\n",
            "PerLevelStatsMultiplier_DinoWild[7]=2.0\n",
        )
    );
}

#[test]
fn clearing_optional_asa_overrides_removes_native_lines() {
    let schema: Value = serde_json::from_str(include_str!(
        "../../../modules/arksurvivalascended/schema.json"
    ))
    .unwrap();
    let defaults = schema["properties"]
        .as_object()
        .unwrap()
        .iter()
        .filter_map(|(key, prop)| {
            prop.get("default")
                .map(|value| (key.clone(), value.clone()))
        })
        .collect::<Map<String, Value>>();
    let mut settings = Map::new();
    settings.insert("supply_crate_loot_quality_multiplier".into(), json!(2.5));
    assert!(
        render_ark_native_ini_lines(&settings, &defaults, ARK_ASA_ADVANCED_GAME_INI)
            .contains("SupplyCrateLootQualityMultiplier=2.5\n")
    );

    settings.remove("supply_crate_loot_quality_multiplier");
    let output = render_ark_native_ini_lines(&settings, &defaults, ARK_ASA_ADVANCED_GAME_INI);
    assert_eq!(output, "bAutoUnlockAllEngrams=false\n");

    settings.insert("supply_crate_loot_quality_multiplier".into(), Value::Null);
    assert_eq!(
        render_ark_native_ini_lines(&settings, &defaults, ARK_ASA_ADVANCED_GAME_INI),
        output
    );
}

#[test]
fn native_null_is_omitted_but_zero_false_and_empty_strings_remain_explicit() {
    let settings = json!({"optional": null, "zero": 0, "disabled": false, "empty": ""});
    let defaults = json!({"optional": 5});
    let definitions: &[ArkIniSetting] = &[
        ("Optional", "optional", false, false, false),
        ("Zero", "zero", false, false, false),
        ("Disabled", "disabled", false, false, false),
        ("Empty", "empty", false, false, false),
    ];
    assert_eq!(
        render_ark_native_ini_lines(
            settings.as_object().unwrap(),
            defaults.as_object().unwrap(),
            definitions,
        ),
        "Zero=0\nDisabled=false\nEmpty=\n"
    );
}
