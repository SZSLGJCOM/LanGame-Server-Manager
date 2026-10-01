import type { TranslateFn } from "../../../i18n";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type { GuidedFieldCopy, GuidedSettingsField, GuidedSettingsSection } from "../settings-schema";

const HUMANITZ_SECTIONS: GuidedSettingsSection[] = [
  {
    id: "access",
    title: "Access & Rosters",
    description: "Join credentials and native admin/allow/ban list files."
  },
  {
    id: "world",
    title: "World Rules",
    description: "Death rules, time, seasons, sleep, and survival toggles."
  },
  {
    id: "difficulty",
    title: "Threats & Events",
    description: "Zombie, hostile human, dog, airdrop, and event tuning."
  },
  {
    id: "loot",
    title: "Loot",
    description: "Loot rarity, respawn, pickup respawn, and cleanup timers."
  },
  {
    id: "spawns",
    title: "Respawns",
    description: "Zombie, hostile human, and animal respawn timers."
  },
  {
    id: "building",
    title: "Building",
    description: "Base health, dismantle, and decay settings."
  },
  {
    id: "vehicles",
    title: "Vehicles",
    description: "Vehicle decay, caps, respawn, and ambient damage behavior."
  },
  {
    id: "advanced",
    title: "Advanced",
    description: "Autosave, raw GameServerSettings lines, and launch argument overrides."
  }
];

interface HumanitzGroupSpec {
  id: string;
  title: string;
  description: string;
  layoutClass: string;
  keys: string[];
}

const HUMANITZ_GROUP_SPECS: Record<string, HumanitzGroupSpec[]> = {
  access: [
    {
      id: "join",
      title: "Administration",
      description: "Administrator credentials.",
      layoutClass: "humanitz-join",
      keys: ["admin_password"]
    },
    {
      id: "rosters",
      title: "Native roster files",
      description: "Player IDs written to the native admin, reserved-slot, and ban lists.",
      layoutClass: "humanitz-rosters",
      keys: [
        "reserved_slots",
        "admin_steam_ids",
        "reserved_player_steam_ids",
        "banned_player_steam_ids"
      ]
    },
    {
      id: "host-feedback",
      title: "Admission policies",
      description: "Global ban checks and family sharing eligibility.",
      layoutClass: "humanitz-host-feedback",
      keys: ["use_global_ban_list", "allow_family_sharing"]
    }
  ],
  world: [
    {
      id: "player-feedback",
      title: "Player notifications",
      description: "Visibility of player deaths and join or leave messages.",
      layoutClass: "humanitz-player-feedback",
      keys: ["no_death_feedback", "no_join_feedback"]
    },
    {
      id: "spawn-grids",
      title: "Spawn grids",
      description: "World grid sizes used for object and vehicle spawning.",
      layoutClass: "humanitz-spawn-grids",
      keys: ["map_segment_0", "map_segment_1", "map_segment_2"]
    },
    {
      id: "survival-rules",
      title: "Survival rules",
      description: "PvP, death handling, respawns, and survival rules.",
      layoutClass: "humanitz-survival-rules",
      keys: [
        "perma_death",
        "on_death_penalty",
        "xp_multiplier",
        "respawn_timer_seconds",
        "limited_spawns",
        "logout_timer_seconds",
        "territory_enabled",
        "weapon_break",
        "vital_drain",
        "multiplayer_sleep",
        "food_decay_multiplier",
        "sleep_deprivation",
        "freeze_time_when_empty",
        "pvp_enabled"
      ]
    },
    {
      id: "time-seasons",
      title: "Time and seasons",
      description: "Day/night length and season cycle controls.",
      layoutClass: "humanitz-time-seasons",
      keys: ["day_duration_minutes", "night_duration_minutes", "starting_season", "days_per_season"]
    },
    {
      id: "weather",
      title: "Weather weights",
      description: "Relative odds for each shipped weather type.",
      layoutClass: "humanitz-weather",
      keys: [
        "weather_clear_sky",
        "weather_cloudy",
        "weather_foggy",
        "weather_light_rain",
        "weather_rain",
        "weather_thunderstorm",
        "weather_light_snow",
        "weather_snow",
        "weather_blizzard"
      ]
    }
  ],
  difficulty: [
    {
      id: "threat-scaling",
      title: "Threat scaling",
      description: "Zombie and hostile human difficulty plus population multipliers.",
      layoutClass: "humanitz-threat-scaling",
      keys: [
        "zombie_difficulty_health",
        "zombie_difficulty_speed",
        "zombie_difficulty_damage",
        "human_health",
        "human_speed",
        "human_damage",
        "zombie_amount_multiplier",
        "human_amount_multiplier",
        "zombie_dog_multiplier",
        "animal_amount_multiplier"
      ]
    },
    {
      id: "events",
      title: "Dogs and events",
      description: "Dog spawns, recruitment, airdrops, and world event toggles.",
      layoutClass: "humanitz-events",
      keys: [
        "dog_enabled",
        "dog_count",
        "recruit_dog",
        "companion_health",
        "companion_damage",
        "air_drop_enabled",
        "air_drop_interval_days",
        "ai_event_frequency",
      ]
    }
  ],
  loot: [
    {
      id: "loot-rules",
      title: "Loot rules",
      description: "Loot rarity, respawn mode, respawn intervals, and world pickup cleanup.",
      layoutClass: "humanitz-loot-rules",
      keys: [
        "rarity_food",
        "rarity_drink",
        "rarity_melee",
        "rarity_ranged",
        "rarity_ammo",
        "rarity_armor",
        "rarity_resources",
        "rarity_other",
        "loot_respawn",
        "loot_respawn_minutes",
        "pickup_respawn_minutes",
        "pickup_cleanup_days"
      ]
    }
  ],
  spawns: [
    {
      id: "respawn-timers",
      title: "Respawn timers",
      description: "Native timers for enemy and animal respawns.",
      layoutClass: "humanitz-respawn-timers",
      keys: ["zombie_respawn_minutes", "human_respawn_minutes", "animal_respawn_minutes"]
    }
  ],
  building: [
    {
      id: "base-rules",
      title: "Base rules",
      description: "Building health, dismantle permissions, and base decay.",
      layoutClass: "humanitz-base-rules",
      keys: [
        "building_health_multiplier",
        "allow_dismantle",
        "allow_house_dismantle",
        "free_build",
        "no_build_zone",
        "spawn_point_decay_days",
        "building_decay_days",
        "fake_building_cleanup_minutes",
        "generator_fuel_multiplier"
      ]
    }
  ],
  vehicles: [
    {
      id: "vehicle-spawns",
      title: "Vehicle ownership and recycling",
      description: "Vehicle claim limits and unattended-car recycling.",
      layoutClass: "humanitz-vehicle-spawns",
      keys: [
        "max_owned_cars",
        "recycle_car_days",
      ]
    },
  ],
  network: [
    {
      id: "voice",
      title: "Voice chat",
      description: "In-game voice communication.",
      layoutClass: "humanitz-voice",
      keys: ["voip_enabled"]
    }
  ],
  advanced: [
    {
      id: "save",
      title: "Saving",
      description: "Autosave interval written to GameServerSettings.ini.",
      layoutClass: "humanitz-save",
      keys: ["save_interval_seconds"]
    },
    {
      id: "launch",
      title: "Raw overrides",
      description: "Version-specific GameServerSettings lines and launch arguments.",
      layoutClass: "humanitz-launch",
      keys: ["settings_extra", "extra_launch_args"]
    }
  ]
};

function readCatalogText(t: TranslateFn, key: string): string | undefined {
  const value = t(key, undefined, "");
  return value.trim() ? value : undefined;
}

function buildHumanitzSections(t: TranslateFn): GuidedSettingsSection[] {
  return HUMANITZ_SECTIONS.map((section) => ({
    ...section,
    title: t(`humanitz.settings.sections.${section.id}`, undefined, section.title),
    description: t(`humanitz.settings.sections.${section.id}Description`, undefined, section.description ?? "")
  }));
}

function buildHumanitzFieldCopy(key: string, t: TranslateFn): GuidedFieldCopy | undefined {
  const baseKey = `settings.schema.humanitz.${key}`;
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

function buildHumanitzFieldGroups(
  sectionId: string,
  fields: GuidedSettingsField[],
  t: TranslateFn
): SettingsModuleFieldGroup[] {
  const fieldsByKey = new Map(fields.map((field) => [field.key, field]));
  const claimedKeys = new Set<string>();
  const groups: SettingsModuleFieldGroup[] = [];

  for (const spec of HUMANITZ_GROUP_SPECS[sectionId] ?? []) {
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
      title: t(`humanitz.settings.groups.${spec.id}.title`, undefined, spec.title),
      description: t(`humanitz.settings.groups.${spec.id}.description`, undefined, spec.description),
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

export const humanitzSettingsDefinition: SettingsModuleDefinition = {
  id: "humanitz",
  getSections: buildHumanitzSections,
  buildFieldGroups: (sectionId, fields, _locale, t) => buildHumanitzFieldGroups(sectionId, fields, t),
  getFieldCopy: (key, t) => buildHumanitzFieldCopy(key, t),
  resolveFieldEditorVariant(key) {
    if (key === "admin_steam_ids" || key === "banned_player_steam_ids") {
      return "string-list";
    }

    return undefined;
  }
};
