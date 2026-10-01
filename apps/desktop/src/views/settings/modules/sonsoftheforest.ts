import type { TranslateFn } from "../../../i18n";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type { GuidedFieldCopy, GuidedSettingsField, GuidedSettingsSection } from "../settings-schema";

const SOTF_SECTIONS: GuidedSettingsSection[] = [
  {
    id: "room",
    title: "Server Identity",
    description: "Server name, slots, and gameplay mode posture."
  },
  {
    id: "network",
    title: "Network",
    description: "LAN posture and network accessibility test behavior."
  },
  {
    id: "world",
    title: "World & Custom Mode",
    description: "Save lifecycle, shared world rules, and Custom-mode tuning."
  },
  {
    id: "access",
    title: "Join & Admin",
    description: "Join password and owner Steam IDs."
  },
  {
    id: "advanced",
    title: "Advanced",
    description: "Logging and launch argument overrides."
  }
];

interface SotfGroupSpec {
  id: string;
  title: string;
  description: string;
  layoutClass: string;
  keys: string[];
}

const SOTF_GROUP_SPECS: Record<string, SotfGroupSpec[]> = {
  network: [
    {
      id: "network-posture",
      title: "Network Posture",
      description: "Network accessibility test behavior.",
      layoutClass: "sotf-network-posture",
      keys: ["skip_network_accessibility_test"]
    }
  ],
  world: [
    {
      id: "mode",
      title: "Mode",
      description: "Primary gameplay mode for this dedicated server.",
      layoutClass: "sotf-mode",
      keys: ["game_mode"]
    },
    {
      id: "save",
      title: "Save",
      description: "World creation or continuation and autosave cadence.",
      layoutClass: "sotf-save",
      keys: ["save_mode", "save_interval"]
    },
    {
      id: "gameplay",
      title: "World Rules",
      description: "Tree regrowth, structure damage, and day-cycle speed while the server is empty.",
      layoutClass: "sotf-gameplay",
      keys: ["tree_regrowth", "structure_damage", "idle_day_cycle_speed"]
    },
    {
      id: "customCombat",
      title: "Custom Combat & Wildlife",
      description: "Combat, enemies, animals, and PvP for newly created Custom-mode worlds.",
      layoutClass: "sotf-custom-combat",
      keys: [
        "custom_cheats",
        "custom_pvp_damage",
        "custom_enemy_spawns",
        "custom_enemy_health",
        "custom_enemy_damage",
        "custom_enemy_armour",
        "custom_enemy_aggression",
        "custom_animal_spawn_rate",
        "custom_enemy_search_parties"
      ]
    },
    {
      id: "customEnvironment",
      title: "Custom Environment",
      description: "Starting season, season length, day length, and precipitation.",
      layoutClass: "sotf-custom-environment",
      keys: [
        "custom_starting_season",
        "custom_season_length",
        "custom_day_length",
        "custom_precipitation_frequency"
      ]
    },
    {
      id: "customSurvival",
      title: "Custom Survival & Building",
      description: "Survival penalties, containers, construction, and player assists.",
      layoutClass: "sotf-custom-survival",
      keys: [
        "custom_consumable_effects",
        "custom_player_stats_damage",
        "custom_cold_penalties",
        "custom_stat_regeneration_penalty",
        "custom_reduced_food_in_containers",
        "custom_single_use_containers",
        "custom_building_resistance",
        "custom_creative_mode",
        "custom_players_immortal",
        "custom_force_place_full_load",
        "custom_no_cuttings_spawn",
        "custom_one_hit_tree_cutting"
      ]
    }
  ],
  access: [
    {
      id: "owners",
      title: "Owner Whitelist",
      description: "Steam64 IDs materialized to ownerswhitelist.txt for the dedicated user data root.",
      layoutClass: "sotf-owners",
      keys: ["owner_whitelist_steam_ids"]
    }
  ],
  advanced: [
    {
      id: "performance",
      title: "Performance",
      description: "Active and idle server frame-rate targets.",
      layoutClass: "sotf-performance",
      keys: ["idle_target_framerate", "active_target_framerate"]
    },
    {
      id: "logs",
      title: "Logging",
      description: "Log file output and timestamp behavior.",
      layoutClass: "sotf-logs",
      keys: ["log_files_enabled", "timestamp_log_filenames", "timestamp_log_entries"]
    },
    {
      id: "launch",
      title: "Launch Overrides",
      description: "One full launch argument per line appended to SonsOfTheForestDS startup.",
      layoutClass: "sotf-launch",
      keys: ["extra_launch_args"]
    }
  ]
};

function readCatalogText(t: TranslateFn, key: string): string | undefined {
  const value = t(key, undefined, "");
  return value.trim() ? value : undefined;
}

function buildSotfSections(t: TranslateFn): GuidedSettingsSection[] {
  return SOTF_SECTIONS.map((section) => ({
    ...section,
    title: t(`sonsoftheforest.settings.sections.${section.id}`, undefined, section.title),
    description: t(
      `sonsoftheforest.settings.sections.${section.id}Description`,
      undefined,
      section.description ?? ""
    )
  }));
}

function buildSotfFieldCopy(key: string, t: TranslateFn): GuidedFieldCopy | undefined {
  const baseKey = `settings.schema.sonsoftheforest.${key}`;
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

function buildSotfFieldGroups(
  sectionId: string,
  fields: GuidedSettingsField[],
  t: TranslateFn
): SettingsModuleFieldGroup[] {
  const fieldsByKey = new Map(fields.map((field) => [field.key, field]));
  const claimedKeys = new Set<string>();
  const groups: SettingsModuleFieldGroup[] = [];

  for (const spec of SOTF_GROUP_SPECS[sectionId] ?? []) {
    const groupFields = spec.keys
      .map((key) => fieldsByKey.get(key))
      .filter((field): field is GuidedSettingsField => Boolean(field));

    if (groupFields.length === 0) {
      continue;
    }

    for (const field of groupFields) {
      claimedKeys.add(field.key);
    }

    groups.push({
      id: spec.id,
      title: t(`sonsoftheforest.settings.groups.${spec.id}.title`, undefined, spec.title),
      description: t(
        `sonsoftheforest.settings.groups.${spec.id}.description`,
        undefined,
        spec.description
      ),
      layoutClass: spec.layoutClass,
      fields: groupFields
    });
  }

  const remainingFields = fields.filter((field) => !claimedKeys.has(field.key));
  if (remainingFields.length > 0) {
    groups.push({
      id: "additional",
      layoutClass: `${sectionId}-additional`,
      fields: remainingFields
    });
  }

  return groups.length > 0 ? groups : [{ id: "default", fields }];
}

function readNumber(value: unknown): number | null {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

export const sonsOfTheForestSettingsDefinition: SettingsModuleDefinition = {
  id: "sonsoftheforest",
  getSections: buildSotfSections,
  buildFieldGroups: (sectionId, fields, _locale, t) => buildSotfFieldGroups(sectionId, fields, t),
  getFieldCopy: (key, t) => buildSotfFieldCopy(key, t),
  getFieldValidationMessage({ field, value, t }) {
    if (field.key === "save_interval") {
      const interval = readNumber(value);
      if (interval !== null && interval < 60) {
        return t(
          "sonsoftheforest.settings.validation.saveInterval",
          undefined,
          "Save interval must be at least 60 seconds."
        );
      }
    }

    return undefined;
  }
};
