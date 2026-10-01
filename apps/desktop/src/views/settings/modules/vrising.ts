import type { TranslateFn } from "../../../i18n";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type { GuidedFieldCopy, GuidedSettingsField, GuidedSettingsSection } from "../settings-schema";
import {
  applyVRisingSettingsPatch,
  getVRisingRawSettingsIssue,
  initializeVRisingSettings
} from "./vrising-settings-hooks";
import {
  VRISING_CASTLE_HEART_LIMIT_KEYS,
  VRISING_EQUIPMENT_STAT_KEYS,
  VRISING_UNIT_STAT_KEYS,
  VRISING_VAMPIRE_STAT_KEYS
} from "./vrising-exact-groups";

const VRISING_SECTIONS: GuidedSettingsSection[] = [
  {
    id: "room",
    title: "Server Host",
    description: "Identity, browser visibility, and simulation basics for this V Rising world."
  },
  {
    id: "presets",
    title: "Presets & Bootstrap",
    description: "Optional official preset names plus starter loadout IDs for boosted or level-start servers."
  },
  {
    id: "rules",
    title: "Rules & PvP",
    description: "Core mode, difficulty, combat rules, loot, travel, relics, and clan behavior."
  },
  {
    id: "survival",
    title: "Survival & Progression",
    description: "Disconnect safety, inactivity cleanup, progression start, hazards, and durability rules."
  },
  {
    id: "rates",
    title: "Economy & Crafting",
    description: "Gathering, loot, traders, stack sizes, crafting, research, repair, and servant economy."
  },
  {
    id: "castle",
    title: "Castle & Siege",
    description: "Castle ownership limits, structure caps, upkeep, relocation, siege timing, and base-attack behavior."
  },
  {
    id: "schedule",
    title: "PvP Schedule",
    description: "Weekday and weekend windows for player-vs-player combat and castle vulnerability."
  },
  {
    id: "time",
    title: "Day & Blood Moon",
    description: "Day cycle pacing and blood moon cadence for the whole world."
  },
  {
    id: "events",
    title: "War Events",
    description: "Official WarEventGameSettings interval, active windows, and player-scaling modifiers."
  },
  {
    id: "raw",
    title: "Advanced JSON",
    description: "Direct ServerGameSettings.json access for unsupported gameplay fields."
  },
  {
    id: "access",
    title: "Access & Admin",
    description: "Passwords, admin access, and bans."
  },
  {
    id: "advanced",
    title: "Persistence & Debug",
    description: "Save compression, diagnostics, and optional server API behavior."
  }
];

function buildVRisingSections(t: TranslateFn): GuidedSettingsSection[] {
  return VRISING_SECTIONS.map((section) => ({
    ...section,
    title: t(`vrising.settings.sections.${section.id}`, undefined, section.title),
    description: t(`vrising.settings.sections.${section.id}Description`, undefined, section.description ?? "")
  }));
}

function readCatalogText(t: TranslateFn, key: string): string | undefined {
  const value = t(key, undefined, "");
  return value.trim() ? value : undefined;
}

function buildSchemaEnumOptionKey(value: unknown): string {
  const raw = String(value).trim();
  const prefix = raw.startsWith("-") ? "minus_" : "";
  const normalized = raw
    .replace(/^-+/, "")
    .replace(/[^A-Za-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "")
    .toLowerCase();

  return `${prefix}${normalized || "empty"}`;
}

function buildVRisingFieldCopy(key: string, t: TranslateFn): GuidedFieldCopy | undefined {
  const baseKey = `settings.schema.vrising.${key}`;
  const title = readCatalogText(t, `${baseKey}.title`);
  const description = readCatalogText(t, `${baseKey}.description`);

  if (!title && !description) {
    return undefined;
  }

  return {
    title: title ?? key,
    description
  };
}

function buildVRisingFieldGroups(
  sectionId: string,
  fields: GuidedSettingsField[],
  _locale: string,
  t: TranslateFn
): SettingsModuleFieldGroup[] {
  const fieldsByKey = new Map(fields.map((field) => [field.key, field]));
  const claimedKeys = new Set<string>();
  const sectionGroups: Record<
    string,
    Array<{ id: string; title: string; description: string; keys: string[] }>
  > = {
    room: [
      {
        id: "reconnect",
        title: t("vrising.settings.groups.reconnect", undefined, "Reconnect reservations"),
        description: t("vrising.settings.groups.reconnectDescription", undefined, "Keep room for players who temporarily disconnect."),
        keys: ["safe_reconnect_time", "safe_reconnect_slots"]
      }
    ],
    network: [
      {
        id: "connections",
        title: t("vrising.settings.groups.connections", undefined, "Network services"),
        description: t("vrising.settings.groups.connectionsDescription", undefined, "Server IP privacy and API access."),
        keys: ["hide_ip_address", "api_enabled", "lan_mode"]
      }
    ],
    access: [
      {
        id: "access",
        title: t("vrising.settings.groups.access", undefined, "Permissions and protection"),
        description: t(
          "vrising.settings.groups.accessDescription",
          undefined,
          "Administrator capacity, anti-cheat protection, and debug permissions."
        ),
        keys: ["max_admins", "secure_mode", "admin_only_debug_events", "admin_list", "ban_list"]
      }
    ],
    presets: [
      {
        id: "presets",
        title: t("vrising.settings.groups.presets", undefined, "Presets"),
        description: t(
          "vrising.settings.groups.presetsDescription",
          undefined,
          "Optional GameSetting preset + starter IDs consumed during world start."
        ),
        keys: ["game_settings_preset", "game_difficulty_preset", "starter_equipment_id", "starter_resources_id"]
      }
    ],
    rules: [
      {
        id: "rules",
        title: t("vrising.settings.groups.rules", undefined, "Rules & PvP"),
        description: t(
          "vrising.settings.groups.rulesDescription",
          undefined,
          "Difficulty, mode, PvP restrictions, loot access and castle interaction policy."
        ),
        keys: [
          "game_difficulty",
          "game_mode_type",
          "castle_damage_mode",
          "siege_weapon_health",
          "player_damage_mode",
          "castle_heart_damage_mode",
          "pvp_protection_mode",
          "death_container_permission",
          "relic_spawn_type",
          "can_loot_enemy_containers",
          "blood_bound_equipment",
          "teleport_bound_items",
          "bat_bound_items",
          "bat_bound_shards",
          "allow_global_chat",
          "all_waypoints_unlocked",
          "free_castle_claim",
          "free_castle_raid",
          "free_castle_destroy",
          "clan_size",
          "player_interaction_time_zone",
          ...VRISING_UNIT_STAT_KEYS
        ]
      }
    ],
    survival: [
      {
        id: "survival",
        title: t("vrising.settings.groups.survival", undefined, "Survival posture"),
        description: t(
          "vrising.settings.groups.survivalDescription",
          undefined,
          "Inactivity cleanup, progression start, disconnect cleanup, and hazard modifiers."
        ),
        keys: [
          "inactivity_kill_enabled",
          "inactivity_kill_time_min",
          "inactivity_kill_time_max",
          "inactivity_kill_safe_time_addition",
          "inactivity_kill_timer_max_item_level",
          "starting_progression_level",
          "weapon_slots",
          "disable_disconnected_dead_enabled",
          "disable_disconnected_dead_timer",
          "disconnected_sun_immunity_time",
          "soul_shard_durability_loss_rate",
          "journal_vblood_source_unit_max_distance",
          "pvp_vampire_respawn_modifier",
          "blood_drain_modifier",
          "durability_drain_modifier",
          "garlic_area_strength_modifier",
          "holy_area_strength_modifier",
          "silver_strength_modifier",
          "sun_damage_modifier",
          "death_durability_factor_loss",
          "death_durability_loss_factor_as_resources",
          ...VRISING_VAMPIRE_STAT_KEYS
        ]
      }
    ],
    rates: [
      {
        id: "rates",
        title: t("vrising.settings.groups.rates", undefined, "Economy tuning"),
        description: t(
          "vrising.settings.groups.ratesDescription",
          undefined,
          "Resource yield, loot, trader economy, crafting and repair rates."
        ),
        keys: [
          "inventory_stacks_modifier",
          "material_yield_modifier_global",
          "blood_essence_yield_modifier",
          "drop_table_modifier_general",
          "drop_table_modifier_missions",
          "drop_table_modifier_stygian_shards",
          "build_cost_modifier",
          "recipe_cost_modifier",
          "craft_rate_modifier",
          "research_cost_modifier",
          "refinement_cost_modifier",
          "refinement_rate_modifier",
          "research_time_modifier",
          "dismantle_resource_modifier",
          "servant_convert_rate_modifier",
          "repair_cost_modifier",
          "trader_stock_modifier",
          "trader_price_modifier",
          "trader_restock_timer_modifier",
          ...VRISING_EQUIPMENT_STAT_KEYS
        ]
      }
    ],
    castle: [
      {
        id: "castle",
        title: t("vrising.settings.groups.castle", undefined, "Castle controls"),
        description: t(
          "vrising.settings.groups.castleDescription",
          undefined,
          "Ownership limits, structural caps, relocation, timers, and combat warnings."
        ),
        keys: [
          "castle_minimum_distance_in_floors",
          "castle_limit",
          "nether_gate_limit",
          "throne_of_darkness_limit",
          "arena_station_limit",
          "routing_station_limit",
          "castle_decay_rate_modifier",
          "castle_blood_essence_drain_modifier",
          "castle_siege_timer",
          "castle_under_attack_timer",
          "castle_raid_timer",
          "castle_raid_protection_time",
          "castle_exposed_free_claim_timer",
          "castle_relocation_cooldown",
          "castle_relocation_enabled",
          "announce_siege_weapon_spawn",
          "show_siege_weapon_map_icon",
          "castle_tick_period",
          "safety_box_limit",
          "eye_structures_limit",
          "tomb_limit",
          "vermin_nest_limit",
          "prison_cell_limit",
          "castle_heart_limit_type",
          ...VRISING_CASTLE_HEART_LIMIT_KEYS
        ]
      }
    ],
    schedule: [
      {
        id: "schedule",
        title: t("vrising.settings.groups.schedule", undefined, "PvP schedule"),
        description: t(
          "vrising.settings.groups.scheduleDescription",
          undefined,
          "Weekday/weekend PvP and castle vulnerability windows."
        ),
        keys: [
          "vs_player_weekday_start_hour",
          "vs_player_weekday_start_minute",
          "vs_player_weekday_end_hour",
          "vs_player_weekday_end_minute",
          "vs_player_weekend_start_hour",
          "vs_player_weekend_start_minute",
          "vs_player_weekend_end_hour",
          "vs_player_weekend_end_minute",
          "vs_castle_weekday_start_hour",
          "vs_castle_weekday_start_minute",
          "vs_castle_weekday_end_hour",
          "vs_castle_weekday_end_minute",
          "vs_castle_weekend_start_hour",
          "vs_castle_weekend_start_minute",
          "vs_castle_weekend_end_hour",
          "vs_castle_weekend_end_minute"
        ]
      }
    ],
    time: [
      {
        id: "time",
        title: t("vrising.settings.groups.time", undefined, "Time cycle"),
        description: t(
          "vrising.settings.groups.timeDescription",
          undefined,
          "Day duration and blood moon pacing for the world cycle."
        ),
        keys: [
          "day_duration_in_seconds",
          "day_start_hour",
          "day_start_minute",
          "day_end_hour",
          "day_end_minute",
          "blood_moon_frequency_min",
          "blood_moon_frequency_max",
          "blood_moon_buff"
        ]
      }
    ],
    events: [
      {
        id: "war-events",
        title: t("vrising.settings.groups.warEvents", undefined, "War events"),
        description: t(
          "vrising.settings.groups.warEventsDescription",
          undefined,
          "Current V Rising WarEventGameSettings values from the installed dedicated server JSON."
        ),
        keys: [
          "war_event_interval",
          "war_event_major_duration",
          "war_event_minor_duration",
          "war_event_weekday_start_hour",
          "war_event_weekday_start_minute",
          "war_event_weekday_end_hour",
          "war_event_weekday_end_minute",
          "war_event_weekend_start_hour",
          "war_event_weekend_start_minute",
          "war_event_weekend_end_hour",
          "war_event_weekend_end_minute",
          "war_event_scaling_players_1_points_modifier",
          "war_event_scaling_players_1_drop_modifier",
          "war_event_scaling_players_2_points_modifier",
          "war_event_scaling_players_2_drop_modifier",
          "war_event_scaling_players_3_points_modifier",
          "war_event_scaling_players_3_drop_modifier",
          "war_event_scaling_players_4_points_modifier",
          "war_event_scaling_players_4_drop_modifier"
        ]
      }
    ],
    raw: [
      {
        id: "raw",
        title: t("vrising.settings.groups.raw", undefined, "ServerGameSettings.json"),
        description: t(
          "vrising.settings.groups.rawDescription",
          undefined,
          "Typed overrides are synchronized into this JSON payload."
        ),
        keys: ["server_game_settings_json"]
      }
    ],
    advanced: [
      {
        id: "advanced",
        title: t("vrising.settings.groups.advanced", undefined, "Advanced runtime"),
        description: t(
          "vrising.settings.groups.advancedDescription",
          undefined,
          "Server FPS, autosave, compression and debug events."
        ),
        keys: ["autosave_count", "autosave_interval_seconds", "autosave_smart_keep", "compress_save_files", "server_fps", "lower_fps_when_empty", "lower_fps_when_empty_value", "disable_debug_events"]
      }
    ]
  };

  const groups: SettingsModuleFieldGroup[] = (sectionGroups[sectionId] ?? []).map((spec) => {
    const groupFields = spec.keys
      .map((key) => fieldsByKey.get(key))
      .filter((field): field is GuidedSettingsField => Boolean(field));

    for (const field of groupFields) {
      claimedKeys.add(field.key);
    }

    return {
      id: spec.id,
      title: spec.title,
      description: spec.description,
      fields: groupFields
    };
  }).filter((group) => group.fields.length > 0);

  const remainingFields = fields.filter((field) => !claimedKeys.has(field.key));
  if (remainingFields.length > 0) {
    groups.push({
      id: "additional",
      fields: remainingFields
    });
  }

  return groups;
}

function getVRisingEnumOptionLabel(fieldKey: string, value: unknown, t: TranslateFn): string | undefined {
  return readCatalogText(t, `settings.schema.vrising.${fieldKey}.option.${buildSchemaEnumOptionKey(value)}`);
}

export const vrisingSettingsDefinition: SettingsModuleDefinition = {
  id: "vrising",
  getSections: buildVRisingSections,
  getFieldCopy: (key, t) => buildVRisingFieldCopy(key, t),
  buildFieldGroups: (sectionId, fields, _locale, t) => buildVRisingFieldGroups(sectionId, fields, _locale, t),
  getEnumOptionLabel: (fieldKey, value, _locale, t) => getVRisingEnumOptionLabel(fieldKey, value, t),
  initializeSettings: (settings, context) => initializeVRisingSettings(settings, context.t),
  applySettingsPatch: (settings, patch, context) => applyVRisingSettingsPatch(settings, patch, context.t),
  getSettingsValidationIssues: (settings, context) => {
    const issue = getVRisingRawSettingsIssue(settings, context.t);
    return issue ? [issue] : [];
  }
};
