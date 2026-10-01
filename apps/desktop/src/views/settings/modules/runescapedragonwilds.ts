import type { SettingsModuleDefinition } from "../module-types";
import { createSurvivalDedicatedSettingsDefinition } from "./survival-dedicated";

const RUNESCAPE_DRAGONWILDS_FIELD_GROUPS = [
  { id: "admin", sectionId: "access", keys: ["owner_id", "admin_password"] },
  { id: "admission", sectionId: "access", keys: ["platform_policy"] },
  { id: "advanced", sectionId: "advanced", keys: ["extra_launch_args"] }
] as const;

const runescapeDragonwildsBaseSettingsDefinition = createSurvivalDedicatedSettingsDefinition(
  "runescapedragonwilds",
  ["room", "access", "advanced"],
  RUNESCAPE_DRAGONWILDS_FIELD_GROUPS
);

export const runescapedragonwildsSettingsDefinition: SettingsModuleDefinition = {
  id: "runescapedragonwilds",
  getSections: runescapeDragonwildsBaseSettingsDefinition.getSections,
  buildFieldGroups: runescapeDragonwildsBaseSettingsDefinition.buildFieldGroups,
  getFieldCopy: (key, t, locale) => key === "owner_id" || key === "platform_policy"
    ? {
        title: t(`settings.schema.runescapedragonwilds.${key}.title`),
        description: t(`settings.schema.runescapedragonwilds.${key}.description`)
      }
    : runescapeDragonwildsBaseSettingsDefinition.getFieldCopy?.(key, t, locale)
};
