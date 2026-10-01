import type { SettingsModuleDefinition } from "../module-types";
import { createSurvivalDedicatedSettingsDefinition } from "./survival-dedicated";

const THEFOREST_FIELD_GROUPS = [
  { id: "access", sectionId: "access", keys: ["admin_password", "allow_cheats", "steam_account_token", "vac_enabled"] },
  {
    id: "world",
    sectionId: "world",
    keys: [
      "difficulty",
      "init_type",
      "autosave_interval_minutes",
      "vegan_mode",
      "vegetarian_mode",
      "reset_holes_on_load",
      "tree_regrowth",
      "building_destruction",
      "enemies_in_creative",
      "realistic_player_damage"
    ]
  },
  {
    id: "advanced",
    sectionId: "advanced",
    keys: ["show_logs", "idle_target_fps", "active_target_fps", "extra_launch_args"]
  }
] as const;

const theforestBaseSettingsDefinition = createSurvivalDedicatedSettingsDefinition(
  "theforest",
  ["room", "access", "world", "advanced"],
  THEFOREST_FIELD_GROUPS
);

export const theforestSettingsDefinition: SettingsModuleDefinition = {
  id: "theforest",
  getSections: theforestBaseSettingsDefinition.getSections,
  buildFieldGroups: theforestBaseSettingsDefinition.buildFieldGroups,
  getFieldCopy: theforestBaseSettingsDefinition.getFieldCopy
};
