import type { TranslateFn } from "../../../i18n";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type { GuidedSettingsField, GuidedSettingsSection } from "../settings-schema";
import { CONAN_DOCUMENTED_GROUPS, CONAN_EXACT_SECTION_KEYS } from "./conanexiles-exact-groups";
import {
  CONAN_REGION_LABEL_KEYS,
  buildConanFieldCopy,
  readConanBoolean,
  readConanNumber
} from "./conanexiles-support";

const CONAN_SECTIONS: Array<{ id: string; title: string; description: string }> = [
  {
    id: "communication",
    title: "Communication & Privacy",
    description: "Voice, chat channels, formatting, and event-log privacy."
  },
  {
    id: "access",
    title: "Access & Security",
    description: "Join password, admin credentials, and ownership policy."
  },
  {
    id: "world",
    title: "World Runtime",
    description: "Day-night pacing, storms, avatars, and world simulation cadence."
  },
  {
    id: "rates",
    title: "Progression Rates",
    description: "XP, harvesting, crafting, thrall conversion, and decay timing multipliers."
  },
  {
    id: "combat",
    title: "Combat & Damage",
    description: "Player, NPC, thrall, building, and friendly-fire damage posture."
  },
  {
    id: "network",
    title: "Network & Remote Console",
    description: "Connection limits and RCON access."
  },
  {
    id: "purge",
    title: "Purge Control",
    description: "Purge enablement, windows, thresholds, and purge timing controls."
  },
  {
    id: "pvp_schedule",
    title: "PvP & Avatar Windows",
    description: "Weekday raid, PvP, building-damage, and avatar summoning schedules."
  },
  {
    id: "followers",
    title: "Followers & Thralls",
    description: "Follower rescue, scouting, decay, population, and damage behavior."
  },
  {
    id: "building",
    title: "Building & Decay",
    description: "Land claim, stability, pickup, abandonment, decay, and validation rules."
  },
  {
    id: "survival",
    title: "Survival & Stamina",
    description: "Stamina, encumbrance, corpses, item repair, crafting time, and corruption."
  },
  {
    id: "transfer",
    title: "Character Transfers",
    description: "Character-transfer allowlists and server merge destinations."
  },
  {
    id: "mods",
    title: "Mods",
    description: "Steam Workshop items and Conan modlist load order."
  },
  {
    id: "advanced",
    title: "Advanced Overrides",
    description: "Raw ini escape hatches and custom launch flags for unsupported keys."
  }
];

function buildConanSections(t: TranslateFn): GuidedSettingsSection[] {
  return CONAN_SECTIONS.map((section) => ({
    id: section.id,
    title: t(`conan.settings.sections.${section.id}`, undefined, section.title),
    description: t(
      `conan.settings.sections.${section.id}Description`,
      undefined,
      section.description
    )
  }));
}

function buildConanFieldGroups(
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
    access: [
      {
        id: "access",
        title: t("conan.settings.groups.access", undefined, "Access controls"),
        description: t(
          "conan.settings.groups.accessDescription",
          undefined,
          "Administrator credentials, authentication, and admission policies."
        ),
        keys: ["admin_password", "battleye_enabled", "region_block_list", "allow_family_shared_account", "enable_login_queue"]
      },
    ],
    world: [
      {
        id: "world",
        title: t("conan.settings.groups.world", undefined, "World controls"),
        description: t(
          "conan.settings.groups.worldDescription",
          undefined,
          "Cycle and storm timing that set the basic room rhythm."
        ),
        keys: [
          "enable_sand_storm",
          "ambient_life_enabled",
          "fatalities_enabled",
          "day_cycle_speed_scale",
          "day_time_speed_scale",
          "night_time_speed_scale",
          "dawn_dusk_speed_scale",
          "use_client_catch_up_time",
          "client_catch_up_time",
          "avatars_disabled",
          "avatar_lifetime",
          "building_replication_distance",
          "undermesh_detection_enabled"
        ]
      }
    ],
    followers: [
      {
        id: "minions",
        title: t("conan.settings.groups.minions", undefined, "Thrall and pet limits"),
        description: t(
          "conan.settings.groups.minionsDescription",
          undefined,
          "Population caps, per-player allowance, and overpopulation cleanup."
        ),
        keys: [
          "use_minion_population_limit",
          "minion_population_base",
          "minion_population_per_player",
          "minion_overpopulation_allowed",
          "minion_overpopulation_cleanup_minutes"
        ]
      }
    ],
    rates: [
      {
        id: "rates",
        title: t("conan.settings.groups.rates", undefined, "Progression and economy"),
        description: t(
          "conan.settings.groups.ratesDescription",
          undefined,
          "XP, thrall conversion, harvest, and decay multipliers."
        ),
        keys: ["player_xp_rate_multiplier", "player_xp_time_multiplier", "player_xp_kill_multiplier", "player_xp_harvest_multiplier", "player_xp_craft_multiplier", "harvest_amount_multiplier", "item_spoil_rate_scale", "crafting_cost_multiplier", "thrall_conversion_multiplier", "fuel_burn_time_multiplier", "resource_respawn_speed_multiplier", "building_decay_time_multiplier", "craft_from_storage_radius", "build_from_storage_radius", "personal_craft_from_storage_radius"]
      }
    ],
    combat: [
      {
        id: "combat",
        title: t("conan.settings.groups.combat", undefined, "Combat multipliers"),
        description: t(
          "conan.settings.groups.combatDescription",
          undefined,
          "Direct, player, and NPC damage behavior used across all fights."
        ),
        keys: [
          "player_damage_multiplier",
          "player_damage_taken_multiplier",
          "npc_damage_multiplier",
          "npc_damage_taken_multiplier",
          "npc_mind_reading_mode",
          "player_knockback_multiplier",
          "npc_knockback_multiplier",
          "thrall_damage_to_players_multiplier",
          "friendly_fire_damage_multiplier",
          "building_damage_multiplier",
          "structure_health_multiplier",
          "structure_damage_taken_multiplier",
          "durability_multiplier"
        ]
      }
    ],
    network: [
      {
        id: "network",
        title: t("conan.settings.groups.network", undefined, "Remote console"),
        description: t("conan.settings.groups.networkDescription", undefined, "RCON access and request limits."),
        keys: ["rcon_enabled", "rcon_password", "rcon_max_karma"]
      }
    ],
    communication: [
      {
        id: "communication",
        title: t("conan.settings.sections.communication", undefined, "Communication & Privacy"),
        description: t("conan.settings.sections.communicationDescription", undefined, "Voice, chat channels, formatting, and event-log privacy."),
        keys: ["server_voice_chat", "chat_has_global", "chat_local_radius", "chat_max_message_length", "disable_chat_formatting", "event_log_pvp_causer_privacy", "event_log_pve_causer_privacy"]
      }
    ],
    pvp_schedule: [
      {
        id: "pvp",
        title: t("conan.settings.groups.pvp", undefined, "PvP control"),
        description: t("conan.settings.groups.pvpDescription", undefined, "PvP and building damage rules and windows."),
        keys: ["pvp_enabled", "restrict_pvp_time", "restrict_pvp_building_damage_time", "pvp_blitz_server"]
      }
    ],
    building: [
      {
        id: "ownership",
        title: t("conan.settings.groups.ownership", undefined, "Ownership and loot policy"),
        description: t("conan.settings.groups.ownershipDescription", undefined, "Player-owned structures and corpse-loot protection."),
        keys: ["no_ownership", "containers_ignore_ownership", "everybody_can_loot_corpse", "can_damage_player_owned_structures", "can_be_damaged"]
      }
    ],
    purge: [
      {
        id: "purge",
        title: t("conan.settings.groups.purge", undefined, "Purge behavior"),
        description: t(
          "conan.settings.groups.purgeDescription",
          undefined,
          "When purge is allowed and how hostile cleanup windows behave."
        ),
        keys: [
          "enable_purge",
          "restrict_purge_time",
          "purge_level",
          "purge_periodicity",
          "purge_restriction_weekday_start",
          "purge_restriction_weekday_end",
          "purge_restriction_weekend_start",
          "purge_restriction_weekend_end",
          "purge_preparation_time",
          "purge_duration",
          "min_purge_online_players",
          "allow_building_during_purge",
          "clan_purge_trigger"
        ]
      }
    ],
    mods: [
      {
        id: "mods",
        title: t("conan.settings.groups.mods", undefined, "Workshop load order"),
        description: t(
          "conan.settings.groups.modsDescription",
          undefined,
          "Conan reads enabled .pak filenames from ConanSandbox/Mods/modlist.txt in this order."
        ),
        keys: ["mod_workshop_ids"]
      }
    ],
    advanced: [
      {
        id: "advanced",
        title: t("conan.settings.groups.advanced", undefined, "Advanced overrides"),
        description: t(
          "conan.settings.groups.advancedDescription",
          undefined,
          "INI and launch-flag escape hatches for unsupported dedicated server toggles."
        ),
        keys: ["server_settings_extra", "game_ini_extra", "engine_ini_extra", "custom_launch_flags"]
      }
    ]
  };

  const specs = [...(sectionGroups[sectionId] ?? [])];
  for (const group of CONAN_DOCUMENTED_GROUPS) {
    if (group.section === sectionId) specs.push({
      id: group.id,
      title: t(`conan.settings.groups.${group.id}`, undefined, group.title),
      description: "",
      keys: [...group.keys]
    });
  }
  const exactKeys = CONAN_EXACT_SECTION_KEYS[sectionId] ?? [];
  if (exactKeys.length > 0) {
    specs.push({
      id: "exact-build",
      title: "",
      description: "",
      keys: [...exactKeys]
    });
  }

  const groups: SettingsModuleFieldGroup[] = specs.map((spec) => {
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

export const conanExilesSettingsDefinition: SettingsModuleDefinition = {
  id: "conanexiles",
  getSections: buildConanSections,
  buildFieldGroups: (sectionId, fields, _locale, t) => buildConanFieldGroups(sectionId, fields, _locale, t),
  getFieldCopy: (key, t, locale = "en-US") => buildConanFieldCopy(key, t, locale),
  getEnumOptionLabel: (fieldKey, value, _locale, t) => {
    if (fieldKey !== "excluded_regions" || typeof value !== "string") {
      return undefined;
    }

    const entry = CONAN_REGION_LABEL_KEYS[value];
    return entry ? t(entry.key, undefined, entry.fallback) : undefined;
  },
  getFieldValidationMessage: ({ field, value, settings, t }) => {
    if (field.key === "max_players") {
      const maxPlayers = readConanNumber(value);
      if (maxPlayers === null) {
        return undefined;
      }

      if (maxPlayers < 1 || maxPlayers > 70) {
        return t(
          "conan.settings.validation.maxPlayersRange",
          undefined,
          "Keep the Conan Exiles player cap between 1 and 70."
        );
      }

      return undefined;
    }

    if (field.key === "admin_password") {
      const password = typeof value === "string" ? value.trim() : "";
      if (!password) {
        return t(
          "conan.settings.validation.adminPasswordMissing",
          undefined,
          "Set an admin password before relying on in-game operator access."
        );
      }

      if (password === "change-me-admin") {
        return t(
          "conan.settings.validation.adminPasswordPlaceholder",
          undefined,
          "The admin password is still using the placeholder value. Replace it before daily use."
        );
      }
    }

    if (field.key === "rcon_password") {
      const rconEnabled = readConanBoolean(settings.rcon_enabled);
      if (!rconEnabled) {
        return undefined;
      }

      const password = typeof value === "string" ? value.trim() : "";
      if (!password) {
        return t(
          "conan.settings.validation.rconPasswordMissing",
          undefined,
          "RCON is enabled, so this instance needs a real remote console password."
        );
      }

      if (password === "change-me-rcon") {
        return t(
          "conan.settings.validation.rconPasswordPlaceholder",
          undefined,
          "The RCON password is still using the placeholder value. Replace it before exposing the host surface."
        );
      }
    }

    return undefined;
  }
};
