import type { ArkGroupSpec } from "./ark-definition-shared";

export const ARK_ASA_FOUNDATION_GROUPS: Record<string, ArkGroupSpec[]> = {
  network: [
    {
      id: "protection",
      titleKey: "ark.settings.groups.network.protection.title",
      fallbackTitle: "Connection protection",
      layoutClass: "ark-network-protection",
      keys: [
        "enable_steel_shield"
      ]
    }
  ],
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
      keys: ["no_transfer_from_filtering", "cluster_id", "cluster_directory"]
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
      keys: ["prevent_upload_survivors", "prevent_upload_items", "prevent_upload_dinos"]
    },
    {
      id: "tribute",
      titleKey: "ark.settings.groups.transfer.tribute.title",
      fallbackTitle: "Tribute caps and expiry",
      layoutClass: "ark-transfer-tribute",
      keys: [
        "max_tribute_dinos",
        "max_tribute_items"
      ]
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
        "server_platform"
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
    },
    {
      id: "teleport",
      titleKey: "ark.settings.groups.admin.teleport.title",
      fallbackTitle: "Administrator teleport locations",
      layoutClass: "ark-admin-teleport",
      keys: [
        "cheat_teleport_locations"
      ]
    }
  ],
  moderation: [
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
        "show_floating_damage_text",
        "server_hardcore",
        "server_force_no_hud",
        "allow_third_person_player",
        "server_crosshair",
        "show_map_player_location",
        "use_astraeos_traversal_buff",
        "b_disable_photo_mode",
        "photo_mode_range_limit"
      ]
    },
    {
      id: "combat-and-carry",
      titleKey: "ark.settings.groups.gameplay.combat-and-carry.title",
      fallbackTitle: "Combat posture",
      layoutClass: "ark-gameplay-combat",
      keys: [
        "allow_flyer_carry_pve",
        "prevent_spawn_animations",
        "b_disable_friendly_fire",
        "b_pv_edisable_friendly_fire"
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
      keys: ["global_voice_chat", "proximity_chat"]
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
        "hair_growth_speed_multiplier",
        "disable_custom_cosmetics"
      ]
    },
    {
      id: "tribes",
      titleKey: "ark.settings.groups.gameplay.tribes.title",
      fallbackTitle: "Tribes and alliances",
      layoutClass: "ark-gameplay-tribes",
      keys: [
        "tribe_tower_bonus_multiplier"
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
        "prevent_template_on_saddle",
        "needs_power_to_activate_aquatic_compartments",
        "b_ignore_structures_prevention_volumes"
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
        "pve_dino_decay_period_multiplier"
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
        "the_max_structures_in_range"
      ]
    },
    {
      id: "abandoned-cleanup",
      titleKey: "ark.settings.groups.limits.abandoned-cleanup.title",
      fallbackTitle: "Abandoned-structure cleanup",
      layoutClass: "ark-limits-cleanup",
      keys: [
        "enable_auto_destroy_structures"
      ]
    },
    {
      id: "tame-soft-limit",
      titleKey: "ark.settings.groups.limits.tame-soft-limit.title",
      fallbackTitle: "Soft tame cap",
      layoutClass: "ark-limits-tames",
      keys: [
        "destroy_tames_over_soft_tame_limit",
        "max_tamed_dinos_soft_tame_limit",
        "max_tamed_dinos_soft_tame_limit_countdown_for_deletion_duration",
        "disable_burrow_decay_timers"
      ]
    },
    {
      id: "vessels",
      titleKey: "ark.settings.groups.limits.vessels.title",
      fallbackTitle: "Anchored vessels",
      layoutClass: "ark-limits-vessels",
      keys: ["max_anchored_vessels_in_range", "anchored_vessel_check_radius"]
    },
    {
      id: "tame-levels",
      titleKey: "ark.settings.groups.limits.tame-levels.title",
      fallbackTitle: "Tame level and unit limits",
      layoutClass: "ark-limits-tame-levels",
      keys: [
        "destroy_tames_over_level_clamp"
      ]
    },
    {
      id: "generators",
      titleKey: "ark.settings.groups.limits.generators.title",
      fallbackTitle: "Generator limits",
      layoutClass: "ark-limits-generators",
      keys: [
        "limit_generators_num",
        "limit_generators_range"
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
        "valguero_memorial_entries"
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
        "harvest_resource_item_amount_class_multipliers"
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
        "wild_dino_character_food_drain_multiplier"
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
