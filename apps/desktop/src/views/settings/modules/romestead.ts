import type { SettingsModuleDefinition } from "../module-types";
import { RomesteadSleepThresholdField } from "../RomesteadSleepThresholdField";
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
  { id: "performance", sectionId: "performance", keys: ["sleep_threshold_ms"] },
  { id: "advanced", sectionId: "advanced", keys: ["extra_launch_args"] }
] as const;

const romesteadBaseSettingsDefinition = createSurvivalDedicatedSettingsDefinition(
  "romestead",
  ["room", "world", "access", "performance", "advanced"],
  ROMESTEAD_FIELD_GROUPS
);

export const romesteadSettingsDefinition: SettingsModuleDefinition = {
  id: "romestead",
  fieldPresentationOverrides: {
    sleep_threshold_ms: {
      state: "specialized",
      owner: "configuration",
      sectionId: "performance",
      rendererId: "romestead-cpu-sleep"
    }
  },
  specializedRenderers: {
    "romestead-cpu-sleep": {
      kind: "module-addon",
      sectionId: "performance",
      keepMounted: true,
      Renderer: RomesteadSleepThresholdField
    }
  },
  getSections: romesteadBaseSettingsDefinition.getSections,
  buildFieldGroups: romesteadBaseSettingsDefinition.buildFieldGroups,
  getFieldCopy: romesteadBaseSettingsDefinition.getFieldCopy,
  getEnumOptionLabel(fieldKey, value, _locale, t) {
    if (fieldKey !== "auto_create_world_size") return undefined;
    return typeof value === "number" && Number.isInteger(value) && value >= 0 && value <= 2
      ? t(`settings.schema.romestead.auto_create_world_size.option.${value}`)
      : undefined;
  },
  getFieldValidationMessage({ field, value, t }) {
    if (field.key !== "sleep_threshold_ms") {
      return undefined;
    }
    const numeric = typeof value === "number" || typeof value === "string" ? Number(value) : NaN;
    if (numeric === -1 || (Number.isFinite(numeric) && numeric >= 1 && numeric <= 16.5)) {
      return undefined;
    }
    return t(
      "romestead.settings.validation.sleepThreshold",
      undefined,
      "Enter a threshold from 1 to 16.5 milliseconds, or turn off CPU sleep."
    );
  }
};
