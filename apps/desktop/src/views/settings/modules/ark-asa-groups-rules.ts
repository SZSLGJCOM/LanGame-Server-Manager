import type { ArkGroupSpec } from "./ark-definition-shared";

export const ARK_ASA_RULE_GROUPS: Record<string, ArkGroupSpec[]> = {
  balance: [
    {
      id: "world-economy",
      titleKey: "ark.settings.groups.balance.world-economy.title",
      fallbackTitle: "Respawn and stack economy",
      layoutClass: "ark-balance-economy",
      keys: [
        "resources_respawn_period_multiplier",
        "item_stack_size_multiplier"
      ]
    },
    {
      id: "survival-and-flight",
      titleKey: "ark.settings.groups.balance.survival-and-flight.title",
      fallbackTitle: "Survival and flyers",
      layoutClass: "ark-balance-flyers",
      keys: ["prevent_diseases"]
    },
    {
      id: "server-posture",
      titleKey: "ark.settings.groups.balance.server-posture.title",
      fallbackTitle: "Server posture",
      layoutClass: "ark-balance-posture",
      keys: [
        "use_singleplayer_settings",
        "show_creative_mode",
        "use_dino_level_up_animations",
        "prevent_mate_boost"
      ]
    },
    {
      id: "player-combat",
      titleKey: "ark.settings.groups.balance.player-combat.title",
      fallbackTitle: "Player combat",
      layoutClass: "ark-balance-player",
      keys: [
        "player_damage_multiplier",
        "player_resistance_multiplier",
        "max_fall_speed_multiplier"
      ]
    },
    {
      id: "wild-combat",
      titleKey: "ark.settings.groups.balance.wild-combat.title",
      fallbackTitle: "Wild dino combat",
      layoutClass: "ark-balance-wild",
      keys: ["dino_damage_multiplier", "dino_resistance_multiplier"]
    },
    {
      id: "structure-combat",
      titleKey: "ark.settings.groups.balance.structure-combat.title",
      fallbackTitle: "Structure combat",
      layoutClass: "ark-balance-structures",
      keys: ["structure_resistance_multiplier"]
    },
    {
      id: "dynamic-config",
      titleKey: "ark.settings.groups.balance.dynamic-config.title",
      fallbackTitle: "Live dynamic balance",
      layoutClass: "ark-balance-dynamic",
      keys: ["use_dynamic_config", "custom_live_tuning_url"]
    },
    {
      id: "cryopod-rules",
      titleKey: "ark.settings.groups.balance.cryopod-rules.title",
      fallbackTitle: "Cryopod posture",
      layoutClass: "ark-balance-cryopod",
      keys: [
        "disable_cryopod_enemy_check",
        "disable_cryopod_fridge_requirement",
        "allow_cryo_fridge_on_saddle"
      ]
    }
  ],
  leveling: [
    {
      id: "player",
      titleKey: "ark.settings.groups.leveling.player.title",
      fallbackTitle: "Player stats",
      layoutClass: "ark-leveling-player",
      keys: ["per_level_stats_multiplier_player_integer"]
    },
    {
      id: "wild",
      titleKey: "ark.settings.groups.leveling.wild.title",
      fallbackTitle: "Wild dino per-level stats",
      layoutClass: "ark-leveling-wild",
      keys: ["per_level_stats_multiplier_dino_wild_integer"]
    },
    {
      id: "tamed",
      titleKey: "ark.settings.groups.leveling.tamed.title",
      fallbackTitle: "Tamed dino stats",
      layoutClass: "ark-leveling-tamed",
      keys: ["per_level_stats_multiplier_dino_tamed_type_integer"]
    },
    {
      id: "rules",
      titleKey: "ark.settings.groups.leveling.rules.title",
      fallbackTitle: "Leveling and respec rules",
      layoutClass: "ark-leveling-rules",
      keys: [
        "b_allow_flyer_speed_leveling",
        "b_allow_speed_leveling",
        "b_allow_unlimited_respecs"
      ]
    }
  ],
  farming: [
    {
      id: "resource-blockers",
      titleKey: "ark.settings.groups.farming.resource-blockers.title",
      fallbackTitle: "Resource refresh blockers",
      layoutClass: "ark-farming-resources",
      keys: ["resource_no_replenish_radius_players", "resource_no_replenish_radius_structures"]
    },
    {
      id: "crops",
      titleKey: "ark.settings.groups.farming.crops.title",
      fallbackTitle: "Crop pacing",
      layoutClass: "ark-farming-crops",
      keys: ["crop_growth_speed_multiplier", "crop_decay_speed_multiplier"]
    },
    {
      id: "waste",
      titleKey: "ark.settings.groups.farming.waste.title",
      fallbackTitle: "Waste and fertilizer",
      layoutClass: "ark-farming-waste",
      keys: ["poop_interval_multiplier"]
    }
  ],
  breeding: [
    {
      id: "cadence",
      titleKey: "ark.settings.groups.breeding.cadence.title",
      fallbackTitle: "Cadence",
      layoutClass: "ark-breeding-cadence",
      keys: ["mating_interval_multiplier", "lay_egg_interval_multiplier", "mating_speed_multiplier"]
    },
    {
      id: "growth",
      titleKey: "ark.settings.groups.breeding.growth.title",
      fallbackTitle: "Growth",
      layoutClass: "ark-breeding-growth",
      keys: [
        "egg_hatch_speed_multiplier",
        "baby_mature_speed_multiplier",
        "baby_food_consumption_speed_multiplier"
      ]
    },
    {
      id: "imprint",
      titleKey: "ark.settings.groups.breeding.imprint.title",
      fallbackTitle: "Imprint",
      layoutClass: "ark-breeding-imprint",
      keys: [
        "baby_cuddle_interval_multiplier",
        "baby_cuddle_grace_period_multiplier",
        "baby_cuddle_lose_imprint_quality_speed_multiplier",
        "baby_imprinting_stat_scale_multiplier",
        "baby_imprint_amount_multiplier",
        "allow_anyone_baby_imprint_cuddle",
        "disable_imprint_dino_buff"
      ]
    },
    {
      id: "rules",
      titleKey: "ark.settings.groups.breeding.rules.title",
      fallbackTitle: "Breeding eligibility",
      layoutClass: "ark-breeding-rules",
      keys: [
        "prevent_breeding_for_class_names"
      ]
    }
  ],
  experience: [
    {
      id: "curve",
      titleKey: "ark.settings.groups.experience.curve.title",
      fallbackTitle: "Experience curves",
      layoutClass: "ark-experience-curves",
      keys: ["level_experience_ramp_overrides", "override_max_experience_points_player", "override_max_experience_points_dino"]
    },
    {
      id: "engrampoints",
      titleKey: "ark.settings.groups.experience.engrampoints.title",
      fallbackTitle: "Engram points",
      layoutClass: "ark-experience-engrams",
      keys: ["override_player_level_engram_points"]
    }
  ],
  engrams: [
    {
      id: "posture",
      titleKey: "ark.settings.groups.engrams.posture.title",
      fallbackTitle: "Unlock posture",
      layoutClass: "ark-engrams-posture",
      keys: ["auto_unlock_all_engrams"]
    },
    {
      id: "auto",
      titleKey: "ark.settings.groups.engrams.auto.title",
      fallbackTitle: "Automatic unlocks",
      layoutClass: "ark-engrams-auto",
      keys: ["engram_entry_auto_unlocks"]
    },
    {
      id: "overrides",
      titleKey: "ark.settings.groups.engrams.overrides.title",
      fallbackTitle: "Named overrides",
      layoutClass: "ark-engrams-overrides",
      keys: ["override_named_engram_entries"]
    }
  ],
  spawns: [
    {
      id: "weights",
      titleKey: "ark.settings.groups.spawns.weights.title",
      fallbackTitle: "Replacement and weights",
      layoutClass: "ark-spawns-weights",
      keys: ["npc_replacements", "dino_spawn_weight_multipliers"]
    },
    {
      id: "additive",
      titleKey: "ark.settings.groups.spawns.additive.title",
      fallbackTitle: "Additive containers",
      layoutClass: "ark-spawns-add",
      keys: ["config_add_npc_spawn_entries_container"]
    },
    {
      id: "subtractive",
      titleKey: "ark.settings.groups.spawns.subtractive.title",
      fallbackTitle: "Subtractive containers",
      layoutClass: "ark-spawns-subtract",
      keys: ["config_subtract_npc_spawn_entries_container"]
    },
    {
      id: "override",
      titleKey: "ark.settings.groups.spawns.override.title",
      fallbackTitle: "Override containers",
      layoutClass: "ark-spawns-override",
      keys: ["config_override_npc_spawn_entries_container"]
    }
  ],
  loot: [
    {
      id: "quality",
      titleKey: "ark.settings.groups.loot.quality.title",
      fallbackTitle: "Quality",
      layoutClass: "ark-loot-quality",
      keys: ["supply_crate_loot_quality_multiplier"]
    },
    {
      id: "switches",
      titleKey: "ark.settings.groups.loot.switches.title",
      fallbackTitle: "Drop switches",
      layoutClass: "ark-loot-switches",
      keys: ["random_supply_crate_points"]
    },
    {
      id: "overrides",
      titleKey: "ark.settings.groups.loot.overrides.title",
      fallbackTitle: "Crate overrides",
      layoutClass: "ark-loot-overrides",
      keys: [
        "config_override_supply_crate_items",
        "exclude_item_indices"
      ]
    },
    {
      id: "lifetime",
      titleKey: "ark.settings.groups.loot.lifetime.title",
      fallbackTitle: "Corpse and dropped item lifetime",
      layoutClass: "ark-loot-lifetime",
      keys: [
        "global_item_decomposition_time_multiplier"
      ]
    }
  ],
  crafting: [
    {
      id: "recipes",
      titleKey: "ark.settings.groups.crafting.recipes.title",
      fallbackTitle: "Recipe posture",
      layoutClass: "ark-crafting-recipes",
      keys: [
        "custom_recipe_effectiveness_multiplier",
        "custom_recipe_skill_multiplier",
        "crafting_skill_bonus_multiplier"
      ]
    },
    {
      id: "overrides",
      titleKey: "ark.settings.groups.crafting.overrides.title",
      fallbackTitle: "Cost and stack overrides",
      layoutClass: "ark-crafting-overrides",
      keys: ["config_override_item_crafting_costs", "config_override_item_max_quantity"]
    },
    {
      id: "wireless",
      titleKey: "ark.settings.groups.crafting.wireless.title",
      fallbackTitle: "Wireless crafting",
      layoutClass: "ark-crafting-wireless",
      keys: [
        "b_disable_wireless_crafting",
        "b_disable_wireless_crafting_for_dinos",
        "b_disable_wireless_crafting_for_players",
        "b_disable_wireless_crafting_for_structures",
        "wireless_crafting_range_override"
      ]
    },
    {
      id: "item-rules",
      titleKey: "ark.settings.groups.crafting.item-rules.title",
      fallbackTitle: "Item preservation and attributes",
      layoutClass: "ark-crafting-item-rules",
      keys: [
        "global_spoiling_time_multiplier"
      ]
    }
  ],
  mods: [
    {
      id: "ids",
      titleKey: "ark.settings.groups.mods.ids.title",
      fallbackTitle: "Mod ID list",
      layoutClass: "ark-mods-ids",
      keys: ["mod_ids_csv"]
    },
    {
      id: "passive",
      titleKey: "arksa.settings.groups.mods.passive.title",
      fallbackTitle: "Passive mods",
      layoutClass: "ark-mods-passive",
      keys: ["passive_mod_ids_csv"]
    }
  ],
  logs: [
    {
      id: "game-log",
      titleKey: "ark.settings.groups.logs.game-log.title",
      fallbackTitle: "Game log posture",
      layoutClass: "ark-logs-game",
      keys: ["server_game_log", "server_game_log_include_tribe_logs"]
    },
    {
      id: "tribe-destruction-log",
      titleKey: "ark.settings.groups.logs.tribe-destruction-log.title",
      fallbackTitle: "Tribe destruction log",
      layoutClass: "ark-logs-tribe-destruction",
      keys: ["allow_hide_damage_source_from_logs"]
    }
  ],
  advanced: [
    {
      id: "raw-overrides",
      titleKey: "ark.settings.groups.advanced.raw-overrides.title",
      fallbackTitle: "Raw file and launch overrides",
      layoutClass: "ark-advanced-raw",
      keys: ["custom_launch_flags", "game_user_settings_extra", "game_ini_extra"]
    }
  ]
};
