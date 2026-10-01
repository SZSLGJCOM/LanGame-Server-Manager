import type { ArkGroupSpec } from "./ark-definition-shared";

export const ARK_ASE_FOUNDATION_GROUPS: Record<string, ArkGroupSpec[]> = {
  operations: [
    {
      id: "runtime",
      titleKey: "ark.settings.groups.operations.runtime.title",
      fallbackTitle: "Save and runtime guardrails",
      layoutClass: "ark-operations-runtime",
      keys: [
        "auto_save_period_minutes",
        "auto_restart_interval_seconds"
      ]
    }
  ],
  transfer: [
    {
      id: "cluster",
      titleKey: "ark.settings.groups.transfer.cluster.title",
      fallbackTitle: "Cluster baseline",
      layoutClass: "ark-transfer-cluster",
      keys: ["no_transfer_from_filtering", "cluster_id", "cluster_directory", "minimum_dino_reupload_interval"]
    },
    {
      id: "downloads",
      titleKey: "ark.settings.groups.transfer.downloads.title",
      fallbackTitle: "Download guards",
      layoutClass: "ark-transfer-downloads",
      keys: [
        "no_tribute_downloads",
        "prevent_download_survivors",
        "prevent_download_items",
        "prevent_download_dinos"
      ]
    },
    {
      id: "uploads",
      titleKey: "ark.settings.groups.transfer.uploads.title",
      fallbackTitle: "Upload guards",
      layoutClass: "ark-transfer-uploads",
      keys: [
        "prevent_upload_survivors",
        "prevent_upload_items",
        "prevent_upload_dinos",
        "secure_send_ar_kpayload"
      ]
    },
    {
      id: "tribute",
      titleKey: "ark.settings.groups.transfer.tribute.title",
      fallbackTitle: "Tribute caps and expiry",
      layoutClass: "ark-transfer-tribute",
      keys: [
        "max_tribute_characters",
        "max_tribute_dinos",
        "max_tribute_items",
        "tribute_character_expiration_seconds",
        "tribute_dino_expiration_seconds",
        "tribute_item_expiration_seconds"
      ]
    },
    {
      id: "class-filters",
      titleKey: "ark.settings.groups.transfer.class-filters.title",
      fallbackTitle: "Class-name filters",
      layoutClass: "ark-transfer-class-filters",
      keys: ["prevent_transfer_for_class_names"]
    }
  ],
  join: [
    {
      id: "gate",
      titleKey: "ark.settings.groups.join.gate.title",
      fallbackTitle: "Entry posture",
      layoutClass: "ark-join-gate",
      keys: [
        "exclusive_join_enabled",
        "kick_idle_players_period",
        "enable_idle_player_kick",
        "battleye_enabled",
        "ban_list_url",
        "spectator_password",
        "enable_afkkick_player_count_percent",
        "crossplay",
        "epiconly"
      ]
    },
    {
      id: "exclusive",
      titleKey: "ark.settings.groups.join.exclusive.title",
      fallbackTitle: "Exclusive list",
      layoutClass: "ark-join-exclusive",
      keys: ["exclusive_join_list"]
    },
    {
      id: "priority",
      titleKey: "ark.settings.groups.join.priority.title",
      fallbackTitle: "Priority list",
      layoutClass: "ark-join-priority",
      keys: ["priority_join_list"]
    }
  ],
  admin: [
    {
      id: "credentials",
      titleKey: "ark.settings.groups.admin.credentials.title",
      fallbackTitle: "Operator credentials",
      layoutClass: "ark-admin-credentials",
      keys: ["admin_password", "admin_list_url", "allowed_cheaters_url", "update_allowed_cheaters_interval"]
    },
    {
      id: "accounts",
      titleKey: "ark.settings.groups.admin.accounts.title",
      fallbackTitle: "Account roster",
      layoutClass: "ark-admin-accounts",
      keys: ["admin_account_ids"]
    }
  ],
  moderation: [
    {
      id: "filters",
      titleKey: "ark.settings.groups.moderation.filters.title",
      fallbackTitle: "Filter switches",
      layoutClass: "ark-moderation-filters",
      keys: ["filter_chat", "filter_character_names", "filter_tribe_names"]
    },
    {
      id: "sources",
      titleKey: "ark.settings.groups.moderation.sources.title",
      fallbackTitle: "Remote word lists",
      layoutClass: "ark-moderation-sources",
      keys: ["bad_word_list_url", "bad_word_whitelist_url"]
    }
  ],
  gameplay: [
    {
      id: "camera-and-map",
      titleKey: "ark.settings.groups.gameplay.camera-and-map.title",
      fallbackTitle: "Camera and map",
      layoutClass: "ark-gameplay-camera",
      keys: [
        "server_pve",
        "allow_third_person_player",
        "server_crosshair",
        "show_map_player_location",
        "b_use_corpse_locator"
      ]
    },
    {
      id: "combat-and-carry",
      titleKey: "ark.settings.groups.gameplay.combat-and-carry.title",
      fallbackTitle: "Combat posture",
      layoutClass: "ark-gameplay-combat",
      keys: [
        "allow_flyer_carry_pve",
        "allow_flying_stamina_recovery",
        "prevent_spawn_animations",
        "b_disable_friendly_fire",
        "b_pv_edisable_friendly_fire",
        "b_allow_unclaim_dinos",
        "b_disable_dino_riding",
        "b_disable_dino_taming",
        "b_passive_defenses_damage_riderless_dinos",
        "prevent_dino_tame_class_names"
      ]
    },
    {
      id: "raid-rules",
      titleKey: "ark.settings.groups.gameplay.raid-rules.title",
      fallbackTitle: "Raid and hit feedback",
      layoutClass: "ark-gameplay-raid",
      keys: ["allow_multiple_attached_c4", "allow_raid_dino_feeding", "allow_hit_markers"]
    },
    {
      id: "voice-and-feed",
      titleKey: "ark.settings.groups.gameplay.voice-and-feed.title",
      fallbackTitle: "Voice and event feed",
      layoutClass: "ark-gameplay-voice",
      keys: ["global_voice_chat", "proximity_chat", "always_notify_player_left"]
    },
    {
      id: "visibility-and-gamma",
      titleKey: "ark.settings.groups.gameplay.visibility-and-gamma.title",
      fallbackTitle: "Visibility and gamma",
      layoutClass: "ark-gameplay-visibility",
      keys: ["disable_weather_fog", "enable_pvp_gamma", "disable_pve_gamma"]
    },
    {
      id: "appearance",
      titleKey: "ark.settings.groups.gameplay.appearance.title",
      fallbackTitle: "Appearance and cosmetics",
      layoutClass: "ark-gameplay-appearance",
      keys: [
        "hair_growth_speed_multiplier"
      ]
    },
    {
      id: "tribes",
      titleKey: "ark.settings.groups.gameplay.tribes.title",
      fallbackTitle: "Tribes and alliances",
      layoutClass: "ark-gameplay-tribes",
      keys: [
        "max_alliances_per_tribe",
        "max_number_of_players_in_tribe",
        "max_tribe_logs",
        "max_tribes_per_alliance",
        "tribe_slot_reuse_cooldown",
        "b_pv_eallow_tribe_war",
        "b_pv_eallow_tribe_war_cancel"
      ]
    },
    {
      id: "pve-schedule",
      titleKey: "ark.settings.groups.gameplay.pve-schedule.title",
      fallbackTitle: "PvE schedule",
      layoutClass: "ark-gameplay-pve-schedule",
      keys: [
        "auto_pv_estart_time_seconds",
        "auto_pv_estop_time_seconds",
        "b_auto_pv_etimer",
        "b_auto_pv_euse_system_time"
      ]
    },
    {
      id: "respawn",
      titleKey: "ark.settings.groups.gameplay.respawn.title",
      fallbackTitle: "PvP respawn rules",
      layoutClass: "ark-gameplay-respawn",
      keys: [
        "b_increase_pv_prespawn_interval",
        "increase_pv_prespawn_interval_base_amount",
        "increase_pv_prespawn_interval_check_period",
        "increase_pv_prespawn_interval_multiplier"
      ]
    },
    {
      id: "missions",
      titleKey: "ark.settings.groups.gameplay.missions.title",
      fallbackTitle: "Missions and world buffs",
      layoutClass: "ark-gameplay-missions",
      keys: [
        "b_disable_genesis_missions",
        "b_disable_world_buffs",
        "b_enable_world_buff_scaling",
        "world_buff_scaling_efficacy"
      ]
    },
    {
      id: "hexagon-store",
      titleKey: "ark.settings.groups.gameplay.hexagon-store.title",
      fallbackTitle: "Hexagon store rules",
      layoutClass: "ark-gameplay-hexagon-store",
      keys: [
        "b_disable_hexagon_store",
        "b_hex_store_allow_only_engram_trade_option"
      ]
    }
  ],
  building: [
    {
      id: "structure-rules",
      titleKey: "ark.settings.groups.building.structure-rules.title",
      fallbackTitle: "Structure rules",
      layoutClass: "ark-building-structures",
      keys: [
        "allow_cave_building_pve",
        "disable_structure_decay_pve",
        "override_structure_platform_prevention",
        "allow_integrated_splus_structures",
        "b_ignore_structures_prevention_volumes",
        "b_allow_platform_saddle_multi_floors",
        "b_flyer_platform_allow_unaligned_dino_basing",
        "b_genesis_use_structures_prevention_volumes",
        "structure_damage_repair_cooldown"
      ]
    },
    {
      id: "offline-protection",
      titleKey: "ark.settings.groups.building.offline-protection.title",
      fallbackTitle: "Offline protection",
      layoutClass: "ark-building-offline",
      keys: ["prevent_offline_pvp", "prevent_offline_pvp_interval"]
    },
    {
      id: "placement-and-locking",
      titleKey: "ark.settings.groups.building.placement-and-locking.title",
      fallbackTitle: "Placement and locking",
      layoutClass: "ark-building-locking",
      keys: [
        "enable_extra_structure_prevention_volumes",
        "pve_allow_structures_at_supply_drops",
        "allow_crate_spawns_on_top_of_structures",
        "disable_structure_placement_collision",
        "force_all_structure_locking"
      ]
    },
    {
      id: "dino-decay",
      titleKey: "ark.settings.groups.building.dino-decay.title",
      fallbackTitle: "Dino ownership decay",
      layoutClass: "ark-building-dino-decay",
      keys: [
        "disable_dino_decay_pve",
        "auto_destroy_decayed_dinos",
        "pve_structure_decay_period_multiplier",
        "pve_dino_decay_period_multiplier"
      ]
    },
    {
      id: "power",
      titleKey: "ark.settings.groups.building.power.title",
      fallbackTitle: "Fuel and power consumption",
      layoutClass: "ark-building-power",
      keys: [
        "fuel_consumption_interval_multiplier",
        "global_powered_battery_durability_decrease_per_second"
      ]
    }
  ],
  limits: [
    {
      id: "pickup-and-cap",
      titleKey: "ark.settings.groups.limits.pickup-and-cap.title",
      fallbackTitle: "Pickup and placement cap",
      layoutClass: "ark-limits-pickup",
      keys: [
        "structure_pickup_time_after_placement",
        "structure_pickup_hold_duration",
        "the_max_structures_in_range",
        "max_platform_saddle_structure_limit"
      ]
    },
    {
      id: "abandoned-cleanup",
      titleKey: "ark.settings.groups.limits.abandoned-cleanup.title",
      fallbackTitle: "Abandoned-structure cleanup",
      layoutClass: "ark-limits-cleanup",
      keys: [
        "enable_auto_destroy_structures",
        "auto_destroy_old_structures_multiplier",
        "only_auto_destroy_core_structures",
        "only_decay_unsnapped_core_structures",
        "fast_decay_unsnapped_core_structures",
        "fast_decay_interval"
      ]
    },
    {
      id: "turret-caps",
      titleKey: "ark.settings.groups.limits.turret-caps.title",
      fallbackTitle: "Turret caps",
      layoutClass: "ark-limits-turrets",
      keys: [
        "limit_turrets_in_range",
        "hard_limit_turrets_in_range",
        "limit_turrets_num",
        "limit_turrets_range"
      ]
    },
    {
      id: "tame-levels",
      titleKey: "ark.settings.groups.limits.tame-levels.title",
      fallbackTitle: "Tame level and unit limits",
      layoutClass: "ark-limits-tame-levels",
      keys: [
        "destroy_tames_over_level_clamp",
        "b_use_tame_limit_for_structures_only"
      ]
    },
    {
      id: "dropped-items",
      titleKey: "ark.settings.groups.limits.dropped-items.title",
      fallbackTitle: "Dropped item limits",
      layoutClass: "ark-limits-dropped-items",
      keys: [
        "limit_non_player_dropped_items_count",
        "limit_non_player_dropped_items_range"
      ]
    },
    {
      id: "integrity",
      titleKey: "ark.settings.groups.limits.integrity.title",
      fallbackTitle: "Item and creature duplication protection",
      layoutClass: "ark-limits-integrity",
      keys: [
        "use_item_dupe_check",
        "enable_victory_core_dupe_check"
      ]
    }
  ],
  world: [
    {
      id: "difficulty",
      titleKey: "ark.settings.groups.world.difficulty.title",
      fallbackTitle: "Difficulty",
      layoutClass: "ark-world-difficulty",
      keys: ["difficulty_offset", "override_official_difficulty", "active_event"]
    },
    {
      id: "day-night",
      titleKey: "ark.settings.groups.world.day-night.title",
      fallbackTitle: "Day and night",
      layoutClass: "ark-world-cycle",
      keys: ["day_cycle_speed_scale", "day_time_speed_scale", "night_time_speed_scale"]
    },
    {
      id: "environment",
      titleKey: "ark.settings.groups.world.environment.title",
      fallbackTitle: "Map environment and landmarks",
      layoutClass: "ark-world-environment",
      keys: [
        "base_temperature_multiplier"
      ]
    }
  ],
  rates: [
    {
      id: "progression",
      titleKey: "ark.settings.groups.rates.progression.title",
      fallbackTitle: "Progression",
      layoutClass: "ark-rates-progression",
      keys: [
        "xp_multiplier",
        "taming_speed_multiplier",
        "harvest_amount_multiplier",
        "craft_xpmultiplier",
        "generic_xpmultiplier",
        "harvest_xpmultiplier",
        "kill_xpmultiplier",
        "special_xpmultiplier",
        "harvest_resource_item_amount_class_multipliers",
        "dino_harvesting_damage_multiplier",
        "player_harvesting_damage_multiplier",
        "passive_tame_interval_multiplier"
      ]
    },
    {
      id: "survivor-pacing",
      titleKey: "ark.settings.groups.rates.survivor-pacing.title",
      fallbackTitle: "Survivor pacing",
      layoutClass: "ark-rates-survivor",
      keys: [
        "player_character_water_drain_multiplier",
        "player_character_food_drain_multiplier",
        "player_character_health_recovery_multiplier",
        "player_character_stamina_drain_multiplier",
        "dino_character_food_drain_multiplier",
        "wild_dino_character_food_drain_multiplier",
        "tamed_dino_character_food_drain_multiplier",
        "tamed_dino_torpor_drain_multiplier",
        "wild_dino_torpor_drain_multiplier"
      ]
    },
    {
      id: "hexagon-economy",
      titleKey: "ark.settings.groups.rates.hexagon-economy.title",
      fallbackTitle: "Hexagon rewards and costs",
      layoutClass: "ark-rates-hexagon-economy",
      keys: [
        "base_hexagon_reward_multiplier",
        "hexagon_cost_multiplier"
      ]
    }
  ],
};
