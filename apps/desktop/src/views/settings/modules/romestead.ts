import type { SettingsModuleDefinition } from "../module-types";
import { createSurvivalDedicatedSettingsDefinition } from "./survival-dedicated";

const ROMESTEAD_FIELD_GROUPS = [
  {
    id: "world",
    sectionId: "world",
    keys: [
      "auto_create_and_load_world",
      "auto_create_world_size",
      "auto_create_world_seed"
    ]
  },
  { id: "admin", sectionId: "access", keys: ["enable_cheats"] },
  { id: "advanced", sectionId: "advanced", keys: ["sleep_threshold_ms", "extra_launch_args"] }
] as const;

const romesteadBaseSettingsDefinition = createSurvivalDedicatedSettingsDefinition(
  "romestead",
  ["room", "world", "access", "advanced"],
  ROMESTEAD_FIELD_GROUPS
);

export const romesteadSettingsDefinition: SettingsModuleDefinition = {
  id: "romestead",
  getSections: romesteadBaseSettingsDefinition.getSections,
  buildFieldGroups: romesteadBaseSettingsDefinition.buildFieldGroups,
  getFieldCopy: romesteadBaseSettingsDefinition.getFieldCopy,
  getFieldValidationMessage({ field, value, t }) {
    if (field.key !== "sleep_threshold_ms") {
      return undefined;
    }
    const numeric = Number(value);
    if (numeric === -1 || (Number.isFinite(numeric) && numeric >= 1 && numeric <= 16.5)) {
      return undefined;
    }
    return t(
      "romestead.settings.validation.sleepThreshold",
      undefined,
      "Use -1 to disable sleeping, or a value from 1.0 to 16.5 milliseconds."
    );
  }
};
