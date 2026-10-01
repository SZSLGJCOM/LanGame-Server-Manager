import type { ArkGroupSpec } from "./ark-definition-shared";

export const ARK_ASE_RULE_GROUPS: Record<string, ArkGroupSpec[]> = {
  leveling: [
    {
      id: "rules",
      titleKey: "ark.settings.groups.leveling.rules.title",
      fallbackTitle: "Leveling and respec rules",
      layoutClass: "ark-leveling-rules",
      keys: [
        "b_allow_flyer_speed_leveling",
        "b_allow_unlimited_respecs"
      ]
    },
    {
      id: "player",
      titleKey: "ark.settings.groups.leveling.player.title",
      fallbackTitle: "Player stats",
      layoutClass: "ark-leveling-player",
      keys: [
        "per_level_stats_multiplier_player_integer",
        "player_base_stat_multipliers_attribute"
      ]
    },
    {
      id: "wild",
      titleKey: "ark.settings.groups.leveling.wild.title",
      fallbackTitle: "Wild dino per-level stats",
      layoutClass: "ark-leveling-wild",
      keys: [
        "per_level_stats_multiplier_dino_wild_integer"
      ]
    },
    {
      id: "tamed",
      titleKey: "ark.settings.groups.leveling.tamed.title",
      fallbackTitle: "Tamed dino stats",
      layoutClass: "ark-leveling-tamed",
      keys: [
        "per_level_stats_multiplier_dino_tamed_type_integer",
        "mutagen_level_boost_stat_id",
        "mutagen_level_boost_bred_stat_id"
      ]
    }
  ],
  balance: [
    {
      id: "world-economy",
      titleKey: "ark.settings.groups.balance.world-economy.title",
      fallbackTitle: "Respawn and stack economy",
      layoutClass: "ark-balance-economy",
      keys: [
        "resources_respawn_period_multiplier",
        "item_stack_size_multiplier",
        "dino_count_multiplier",
        "server_auto_force_respawn_wild_dinos_interval"
      ]
    },
    {
      id: "survival-and-flight",
      titleKey: "ark.settings.groups.balance.survival-and-flight.title",
      fallbackTitle: "Survival and flyers",
      layoutClass: "ark-balance-flyers",
      keys: ["prevent_diseases", "force_can_ride_fliers"]
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
        "max_fall_speed_multiplier",
        "prevent_offline_pv_pconnection_invincible_interval"
      ]
    },
    {
      id: "wild-combat",
      titleKey: "ark.settings.groups.balance.wild-combat.title",
      fallbackTitle: "Wild dino combat",
      layoutClass: "ark-balance-wild",
      keys: [
        "dino_damage_multiplier",
        "dino_resistance_multiplier",
        "dino_turret_damage_multiplier"
      ]
    },
    {
      id: "tamed-combat",
      titleKey: "ark.settings.groups.balance.tamed-combat.title",
      fallbackTitle: "Tamed dino combat",
      layoutClass: "ark-balance-tamed",
      keys: ["tamed_dino_damage_multiplier", "tamed_dino_resistance_multiplier"]
    },
    {
      id: "structure-combat",
      titleKey: "ark.settings.groups.balance.structure-combat.title",
      fallbackTitle: "Structure combat",
      layoutClass: "ark-balance-structures",
      keys: [
        "structure_damage_multiplier",
        "structure_resistance_multiplier",
        "pv_pzone_structure_damage_multiplier"
      ]
    },
    {
      id: "class-specific-combat",
      titleKey: "ark.settings.groups.balance.class-specific-combat.title",
      fallbackTitle: "Class-specific micro tuning",
      layoutClass: "ark-balance-class-specific",
      keys: [
        "dino_class_damage_multipliers",
        "dino_class_resistance_multipliers",
        "tamed_dino_class_damage_multipliers",
        "tamed_dino_class_resistance_multipliers"
      ]
    },
    {
      id: "dynamic-config",
      titleKey: "ark.settings.groups.balance.dynamic-config.title",
      fallbackTitle: "Live dynamic balance",
      layoutClass: "ark-balance-dynamic",
      keys: ["use_dynamic_config", "custom_dynamic_config_url"]
    },
    {
      id: "cryopod-rules",
      titleKey: "ark.settings.groups.balance.cryopod-rules.title",
      fallbackTitle: "Cryopod posture",
      layoutClass: "ark-balance-cryopod",
      keys: [
        "enable_cryopod_nerf",
        "cryopod_nerf_duration",
        "cryopod_nerf_damage_mult",
        "cryopod_nerf_incoming_damage_mult_percent",
        "enable_cryo_sickness_pve"
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
    },
    {
      id: "mutagen",
      titleKey: "ark.settings.groups.farming.mutagen.title",
      fallbackTitle: "Mutagen replenishment",
      layoutClass: "ark-farming-mutagen",
      keys: [
        "adjustable_mutagen_spawn_delay_multiplier"
      ]
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
        "prevent_breeding_for_class_names",
        "b_disable_dino_breeding"
      ]
    }
  ],
  experience: [
    {
      id: "curve",
      titleKey: "ark.settings.groups.experience.curve.title",
      fallbackTitle: "Experience curves",
      layoutClass: "ark-experience-curves",
      keys: [
        "level_experience_ramp_overrides",
        "override_max_experience_points_dino",
        "override_max_experience_points_player"
      ]
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
      keys: [
        "auto_unlock_all_engrams",
        "b_only_allow_specified_engrams"
      ]
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
    },
    {
      id: "indexed-overrides",
      titleKey: "ark.settings.groups.engrams.indexed-overrides.title",
      fallbackTitle: "Indexed engram overrides",
      layoutClass: "ark-engrams-indexed-overrides",
      keys: [
        "override_engram_entries"
      ]
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
    },
    {
      id: "integrity",
      titleKey: "ark.settings.groups.spawns.integrity.title",
      fallbackTitle: "Spawn protection",
      layoutClass: "ark-spawns-integrity",
      keys: [
        "use_secure_spawn_rules"
      ]
    }
  ],
  loot: [
    {
      id: "quality",
      titleKey: "ark.settings.groups.loot.quality.title",
      fallbackTitle: "Quality",
      layoutClass: "ark-loot-quality",
      keys: ["supply_crate_loot_quality_multiplier", "fishing_loot_quality_multiplier"]
    },
    {
      id: "switches",
      titleKey: "ark.settings.groups.loot.switches.title",
      fallbackTitle: "Drop switches",
      layoutClass: "ark-loot-switches",
      keys: ["disable_loot_crates", "random_supply_crate_points"]
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
        "global_item_decomposition_time_multiplier",
        "global_corpse_decomposition_time_multiplier",
        "use_corpse_life_span_multiplier"
      ]
    },
    {
      id: "spawn-equipment",
      titleKey: "ark.settings.groups.loot.spawn-equipment.title",
      fallbackTitle: "Spawn equipment",
      layoutClass: "ark-loot-spawn-equipment",
      keys: [
        "b_disable_default_map_item_sets"
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
        "allow_custom_recipes",
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
      id: "item-rules",
      titleKey: "ark.settings.groups.crafting.item-rules.title",
      fallbackTitle: "Item preservation and attributes",
      layoutClass: "ark-crafting-item-rules",
      keys: [
        "global_spoiling_time_multiplier",
        "item_stat_clamps_attribute"
      ]
    }
  ],
  mods: [
    {
      id: "ids",
      titleKey: "ark.settings.groups.mods.ids.title",
      fallbackTitle: "Steam Workshop ActiveMods",
      layoutClass: "ark-mods-ids",
      keys: ["active_mod_ids"]
    },
    {
      id: "auto-managed",
      titleKey: "arkse.settings.groups.mods.autoManaged.title",
      fallbackTitle: "Steam auto management",
      layoutClass: "ark-mods-auto-managed",
      keys: ["auto_managed_mods", "auto_managed_mod_ids"]
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
      id: "admin-chat",
      titleKey: "ark.settings.groups.logs.admin-chat.title",
      fallbackTitle: "Admin command echo",
      layoutClass: "ark-logs-admin",
      keys: ["notify_admin_commands_in_chat"]
    },
    {
      id: "tribe-destruction-log",
      titleKey: "ark.settings.groups.logs.tribe-destruction-log.title",
      fallbackTitle: "Tribe destruction log",
      layoutClass: "ark-logs-tribe-destruction",
      keys: ["tribe_log_destroyed_enemy_structures", "allow_hide_damage_source_from_logs"]
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
