import type { SettingsModuleDefinition } from "../module-types";
import { DragonwildsWorldSettingsPanel } from "../DragonwildsWorldSettingsPanel";
import { DragonwildsWorldSettingsProvider } from "../DragonwildsWorldSettingsContext";
import {
  DRAGONWILDS_WORLD_CATEGORIES, DRAGONWILDS_WORLD_SETTINGS, dragonwildsWorldSettingCopyKey,
  dragonwildsWorldSettingSection, isDragonwildsWorldSettingEditable
} from "../../../dragonwilds-world-settings";
import { createSurvivalDedicatedSettingsDefinition } from "./survival-dedicated";

const RUNESCAPE_DRAGONWILDS_FIELD_GROUPS = [
  { id: "admin", sectionId: "access", keys: ["owner_id", "admin_password"] },
  { id: "admission", sectionId: "access", keys: ["platform_policy"] },
  { id: "advanced", sectionId: "advanced", keys: ["extra_launch_args"] }
] as const;

const runescapeDragonwildsBaseSettingsDefinition = createSurvivalDedicatedSettingsDefinition(
  "runescapedragonwilds",
  ["room", "access", "world", "advanced"],
  RUNESCAPE_DRAGONWILDS_FIELD_GROUPS
);

export const runescapedragonwildsSettingsDefinition: SettingsModuleDefinition = {
  id: "runescapedragonwilds",
  workspaceProvider: DragonwildsWorldSettingsProvider,
  getSections: (t, locale) => [
    ...(runescapeDragonwildsBaseSettingsDefinition.getSections?.(t, locale) ?? [])
      .map((section) => section.id === "world" ? { ...section, order: 0 } : section),
    ...DRAGONWILDS_WORLD_CATEGORIES.map((category, index) => ({
      id: `world_${category}`, order: (index + 1) * 10,
      title: t(`runescapedragonwilds.settings.worldEditor.groups.${category}`),
      description: t(`runescapedragonwilds.settings.worldEditor.groups.${category}Description`)
    }))
  ],
  buildFieldGroups: runescapeDragonwildsBaseSettingsDefinition.buildFieldGroups,
  specializedRenderers: Object.fromEntries(["world", ...DRAGONWILDS_WORLD_CATEGORIES.map((category) => `world_${category}`)]
    .map((sectionId) => [`dragonwilds-${sectionId}`, {
      kind: "module-addon" as const, sectionId, saveMode: "native-settings" as const, Renderer: DragonwildsWorldSettingsPanel
    }])),
  getAdditionalPresentationFields: ({ t }) => [{
    key: "world_settings", title: t("runescapedragonwilds.settings.worldEditor.mode"),
    description: t("runescapedragonwilds.settings.sections.worldDescription"), sectionId: "world",
    presentation: {
      state: "specialized", owner: "configuration", sectionId: "world", rendererId: "dragonwilds-world",
      aliases: ["difficulty", "mode", "难度", "模式"]
    }
  }, ...DRAGONWILDS_WORLD_SETTINGS.filter((definition) => isDragonwildsWorldSettingEditable(definition)).map((definition) => {
    const sectionId = dragonwildsWorldSettingSection(definition.tag);
    const copy = `runescapedragonwilds.settings.worldEditor.fields.${dragonwildsWorldSettingCopyKey(definition.tag)}`;
    return {
      key: definition.tag, title: t(`${copy}.title`), description: t(`${copy}.description`), sectionId,
      sourceSurface: "save",
      presentation: { state: "specialized" as const, owner: "configuration" as const, sectionId,
        rendererId: `dragonwilds-${sectionId}`, aliases: [definition.tag] }
    };
  })],
  getFieldCopy: (key, t, locale) => key === "owner_id" || key === "platform_policy"
    ? {
        title: t(`settings.schema.runescapedragonwilds.${key}.title`),
        description: t(`settings.schema.runescapedragonwilds.${key}.description`)
      }
    : runescapeDragonwildsBaseSettingsDefinition.getFieldCopy?.(key, t, locale)
};
