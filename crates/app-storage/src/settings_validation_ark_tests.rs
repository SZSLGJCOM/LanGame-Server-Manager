use super::*;
use serde_json::json;
use std::sync::OnceLock;

fn descriptors() -> &'static Vec<ModuleDescriptor> {
    static DESCRIPTORS: OnceLock<Vec<ModuleDescriptor>> = OnceLock::new();
    DESCRIPTORS.get_or_init(|| {
        app_modules::discover_modules(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules"),
        )
        .unwrap()
        .into_iter()
        .filter(|descriptor| descriptor.summary.id.starts_with("arksurvival"))
        .collect()
    })
}

fn valid_base(descriptor: &ModuleDescriptor) -> Map<String, Value> {
    let settings = crate::templates::collect_schema_defaults_from_schema_json(
        descriptor.schema_json.as_deref(),
        crate::templates::SchemaDefaultContext {
            instance_id: Some("ark-rules-test"),
            instance_name: Some("ARK rules test"),
        },
    )
    .unwrap();
    validate_settings_against_schema(
        Some(descriptor),
        &settings,
        SettingsValidationPhase::Creation,
    )
    .unwrap();
    settings
}

#[test]
fn ark_rules_reject_invalid_native_syntax_and_numeric_domains() {
    for descriptor in descriptors() {
        for (field, raw) in [
            ("per_level_stats_multiplier_player_integer", "[-1]=2"),
            ("per_level_stats_multiplier_player_integer", "[0]=NaN"),
            ("per_level_stats_multiplier_player_integer", "[0]=-1"),
            ("per_level_stats_multiplier_player_integer", "[0]=2\n0=3"),
            (
                "per_level_stats_multiplier_player_integer",
                "OtherStat[0]=2",
            ),
            (
                "level_experience_ramp_overrides",
                "(ExperiencePointsForLevel[0]=0,ExperiencePointsForLevel[1]=5,ExperiencePointsForLevel[2]=4)",
            ),
            (
                "level_experience_ramp_overrides",
                "(ExperiencePointsForLevel[-1]=2)",
            ),
            (
                "level_experience_ramp_overrides",
                "(ExperiencePointsForLevel[0]=0,ExperiencePointsForLevel[0]=1)",
            ),
            ("override_player_level_engram_points", "1.5"),
            (
                "config_override_item_crafting_costs",
                "(ItemClassString=\"Mod_C\",BaseCraftingResourceRequirements=((ResourceItemTypeString=\"Wood_C\",BaseResourceRequirement=2))",
            ),
            (
                "config_override_item_crafting_costs",
                "(ItemClassString=\"Mod_C\",BaseCraftingResourceRequirements=((BaseResourceRequirement=-2)))",
            ),
            (
                "config_override_supply_crate_items",
                "(SupplyCrateClassString=\"Mod_C\",ItemSets=((MinNumItems=1.5)))",
            ),
            (
                "config_override_supply_crate_items",
                "(SupplyCrateClassString=\"Mod_C\",ItemSets=((ItemsWeights=(1,NaN))))",
            ),
            (
                "dino_spawn_weight_multipliers",
                "(DinoNameTag=Rex,SpawnLimitPercentage=2)",
            ),
        ] {
            let mut settings = valid_base(descriptor);
            settings.insert(field.to_owned(), Value::String(raw.to_owned()));
            let error = validate_settings_against_schema(
                Some(descriptor),
                &settings,
                SettingsValidationPhase::Creation,
            )
            .expect_err(&format!(
                "{} must reject {field}={raw}",
                descriptor.summary.id
            ));
            assert!(
                matches!(error, StorageError::InvalidModuleSetting { field: ref actual, .. } if actual == field),
                "{error}"
            );
        }
    }
}

#[test]
fn ark_rules_accept_mod_extensions_repeated_ramps_and_all_indexed_forms() {
    let settings = json!({
        "per_level_stats_multiplier_player_integer":"PerLevelStatsMultiplier_Player[0]=0\n[12]=1.5\n99=2",
        "per_level_stats_multiplier_dino_tamed_type_integer":"[0]=1\n_Add[0]=0.2\n_Affinity[0]=0.3",
        "level_experience_ramp_overrides":"(ExperiencePointsForLevel[1]=5,ExperiencePointsForLevel[0]=0)\nLevelExperienceRampOverrides=(ExperiencePointsForLevel[0]=0,ExperiencePointsForLevel[1]=7)\n(ExperiencePointsForLevel[0]=1)",
        "override_player_level_engram_points":"0\nOverridePlayerLevelEngramPoints=12",
        "config_override_item_crafting_costs":"ConfigOverrideItemCraftingCosts=(ItemClassString=\"/Mod/New, \\\"Special\\\"_C\",BaseCraftingResourceRequirements=((ResourceItemTypeString=\"ModWood_C\",BaseResourceRequirement=2.5)),ModSetting=(Unknown=-5))",
        "config_override_item_max_quantity":"(ItemClassString=\"Mod_C\",Quantity=(MaxItemQuantity=500,bIgnoreMultiplier=True))",
        "config_override_supply_crate_items":"(SupplyCrateClassString=\"Mod_C\",ItemSets=((ItemsWeights=(1,2.5),MinNumItems=1,ModText=custom)),Future=(Weight=-5,Future=(EntryWeight=-2)))",
        "game_ini_extra":"[Unknown.Mod]\nCustomNativeKey=(Arbitrary=\"legitimate\")\nCustomNativeKey=Second\n",
    });
    for descriptor in descriptors() {
        let mut complete = valid_base(descriptor);
        complete.extend(settings.as_object().unwrap().clone());
        validate_settings_against_schema(
            Some(descriptor),
            &complete,
            SettingsValidationPhase::Creation,
        )
        .unwrap();
    }
}
