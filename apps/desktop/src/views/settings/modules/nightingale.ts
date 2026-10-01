import type { SettingsModuleDefinition } from "../module-types";
import { createSurvivalDedicatedSettingsDefinition } from "./survival-dedicated";

const NIGHTINGALE_FIELD_GROUPS = [
  { id: "access", sectionId: "access", keys: ["admin_password", "enable_cheats"] },
  { id: "world", sectionId: "world", keys: ["starting_difficulty"] },
  { id: "network", sectionId: "network", keys: ["status_endpoint_enabled"] },
  { id: "advanced", sectionId: "advanced", keys: ["json_logging", "extra_launch_args"] }
] as const;

const nightingaleBaseSettingsDefinition = createSurvivalDedicatedSettingsDefinition(
  "nightingale",
  ["world", "network", "access", "advanced"],
  NIGHTINGALE_FIELD_GROUPS
);

export const nightingaleSettingsDefinition: SettingsModuleDefinition = {
  id: "nightingale",
  getSections: nightingaleBaseSettingsDefinition.getSections,
  buildFieldGroups: nightingaleBaseSettingsDefinition.buildFieldGroups,
  getFieldCopy: nightingaleBaseSettingsDefinition.getFieldCopy
};
