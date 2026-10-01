import type { SettingsModuleDefinition } from "../module-types";
import { createSurvivalDedicatedSettingsDefinition } from "./survival-dedicated";

const RETURNTOMORIA_OPTION_LABELS: Record<string, string> = {
  campaign: "Campaign",
  sandbox: "Sandbox",
  story: "Story",
  solo: "Solo",
  normal: "Normal",
  hard: "Hard",
  custom: "Custom",
  verylow: "Very Low",
  low: "Low",
  default: "Default",
  high: "High",
  veryhigh: "Very High"
};

const RETURNTOMORIA_FIELD_GROUPS = [
  { id: "console", sectionId: "runtime", keys: ["console_enabled"] },
  {
    id: "network",
    sectionId: "network",
    title: "Connections",
    description: "Advertised endpoint and reconnect timing.",
    keys: ["advertise_address", "advertise_port", "initial_connection_retry_seconds", "after_disconnection_retry_seconds"]
  },
  {
    id: "world",
    sectionId: "world",
    title: "World creation",
    description: "Choose the world type, seed, and preset used when the dedicated server creates a world.",
    keys: ["world_seed", "game_mode", "difficulty_preset"]
  },
  {
    id: "custom-difficulty",
    sectionId: "world",
    title: "Custom difficulty",
    description: "These native values apply when the difficulty preset is custom.",
    keys: [
      "combat_difficulty",
      "enemy_aggression",
      "survival_difficulty",
      "mining_drops",
      "world_drops",
      "horde_frequency",
      "siege_frequency",
      "patrol_frequency"
    ]
  },
  {
    id: "dlc",
    sectionId: "world",
    title: "World DLC",
    description: "Enable DLC when creating a new world.",
    keys: ["optional_dlc_array"]
  },
  {
    id: "performance",
    sectionId: "advanced",
    title: "Performance",
    description: "Set the server frame-rate limit and loaded-area budget.",
    keys: ["server_fps", "loaded_area_limit"]
  },
  {
    id: "advanced",
    sectionId: "advanced",
    keys: ["extra_launch_args"]
  }
] as const;

const returntomoriaBaseSettingsDefinition = createSurvivalDedicatedSettingsDefinition(
  "returntomoria",
  ["access", "network", "runtime", "world", "advanced"],
  RETURNTOMORIA_FIELD_GROUPS
);

export const returntomoriaSettingsDefinition: SettingsModuleDefinition = {
  id: "returntomoria",
  getSections: returntomoriaBaseSettingsDefinition.getSections,
  buildFieldGroups: returntomoriaBaseSettingsDefinition.buildFieldGroups,
  getFieldCopy: returntomoriaBaseSettingsDefinition.getFieldCopy,
  getEnumOptionLabel: (fieldKey, value, _locale, t) => {
    const key = String(value).trim().toLowerCase();
    const fallback = RETURNTOMORIA_OPTION_LABELS[key];
    return fallback
      ? t(`settings.schema.returntomoria.${fieldKey}.option.${key}`, undefined, fallback)
      : undefined;
  }
};
