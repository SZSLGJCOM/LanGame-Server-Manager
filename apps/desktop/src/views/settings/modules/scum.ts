import type { TranslateFn } from "../../../i18n";
import { ScumJsonSettingsRenderer, validateScumJsonSettings } from "../ScumJsonSettingsRenderer";
import { ScumServerSettingsRenderer } from "../ScumServerSettingsRenderer";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import {
  buildScumNativePresentationFields,
  initializeScumSettings,
  SCUM_CORE_SECTION_IDS,
  SCUM_SECTION_FIELDS,
  scumRendererId,
  validateScumStructuredSettings,
  type ScumNativeSection
} from "../scum-server-settings-inventory";
import type { GuidedFieldCopy, GuidedSettingsField, GuidedSettingsSection } from "../settings-schema";

const STEAM64_PATTERN = /^\d{17}$/;
const NATIVE_SECTIONS = Object.keys(SCUM_SECTION_FIELDS) as ScumNativeSection[];

const SCUM_SECTIONS: GuidedSettingsSection[] = [
  {
    id: "server-settings",
    title: "Server Settings",
    description: "The exact native SCUM ServerSettings.ini v7 hierarchy."
  },
  ...NATIVE_SECTIONS.map((section, index) => ({
    id: section.toLowerCase(),
    parentId: "server-settings",
    order: index,
    title: section,
    description: `Native [${section}] parameters from the generated SCUM server inventory.`
  })),
  {
    id: "json-files",
    title: "Structured Files",
    description: "SCUM economy, raid-window, and scheduled-notification JSON surfaces."
  },
  {
    id: "economy",
    parentId: "json-files",
    title: "Economy & Traders",
    description: "Global economy values and ordered trader item overrides."
  },
  {
    id: "raid",
    parentId: "json-files",
    title: "Raid Times",
    description: "Global raid-protection day and time windows."
  },
  {
    id: "notifications",
    parentId: "json-files",
    title: "Notifications",
    description: "Scheduled native server messages."
  },
  {
    id: "access",
    title: "Join & Admin",
    description: "Native administrator roster ownership remains in Player Access."
  }
];

function readCatalogText(t: TranslateFn, key: string): string | undefined {
  const value = t(key, undefined, "");
  return value.trim() ? value : undefined;
}

function buildScumSections(t: TranslateFn): GuidedSettingsSection[] {
  return SCUM_SECTIONS.map((section) => ({
    ...section,
    title: t(`scum.settings.sections.${section.id}`, undefined, section.title),
    description: t(
      `scum.settings.sections.${section.id}Description`,
      undefined,
      section.description ?? ""
    )
  }));
}

function buildScumFieldCopy(key: string, t: TranslateFn): GuidedFieldCopy | undefined {
  const baseKey = `settings.schema.scum.${key}`;
  const title = readCatalogText(t, `${baseKey}.title`);
  const description = readCatalogText(t, `${baseKey}.description`);
  return title || description ? { title: title ?? key, description } : undefined;
}

function buildScumFieldGroups(
  sectionId: string,
  fields: GuidedSettingsField[],
  t: TranslateFn
): SettingsModuleFieldGroup[] {
  if (fields.length === 0) return [];
  const groupId = sectionId === "access" ? "player-access" : "launch-overrides";
  const fallbackTitle = sectionId === "access" ? "Player Access ownership" : "Launch overrides";
  const fallbackDescription = sectionId === "access"
    ? "Edit the SCUM administrator roster in the Players tab."
    : "Enter one complete process argument per line.";
  return [{
    id: groupId,
    title: t(`scum.settings.groups.${groupId}`, undefined, fallbackTitle),
    description: t(
      `scum.settings.groups.${groupId}Description`,
      undefined,
      fallbackDescription
    ),
    layoutClass: `scum-${sectionId}`,
    fields
  }];
}

function parseSteam64Ids(value: unknown): string[] {
  if (typeof value !== "string") return [];
  return value.replace(/\r\n?/g, "\n").split(/[\n,;]+/).map((entry) => entry.trim()).filter(Boolean);
}

const fieldPresentationOverrides = Object.fromEntries([
  ...NATIVE_SECTIONS.map((section) => [SCUM_SECTION_FIELDS[section], {
    state: "specialized" as const,
    owner: "configuration" as const,
    sectionId: section.toLowerCase(),
    rendererId: scumRendererId(section)
  }]),
  ["economy_override", {
    state: "specialized" as const,
    owner: "configuration" as const,
    sectionId: "economy",
    rendererId: "scum-economy"
  }],
  ["raid_times", {
    state: "specialized" as const,
    owner: "configuration" as const,
    sectionId: "raid",
    rendererId: "scum-raid-times"
  }],
  ["notifications", {
    state: "specialized" as const,
    owner: "configuration" as const,
    sectionId: "notifications",
    rendererId: "scum-notifications"
  }]
]);

const specializedRenderers = Object.fromEntries([
  ...SCUM_CORE_SECTION_IDS.map((sectionId) => [scumRendererId(sectionId), {
    kind: "module-addon" as const,
    sectionId,
    Renderer: ScumServerSettingsRenderer
  }]),
  ...NATIVE_SECTIONS.map((section) => [scumRendererId(section), {
    kind: "module-addon" as const,
    sectionId: section.toLowerCase(),
    fieldKey: SCUM_SECTION_FIELDS[section],
    Renderer: ScumServerSettingsRenderer
  }]),
  ["scum-economy", {
    kind: "module-addon" as const,
    sectionId: "economy",
    fieldKey: "economy_override",
    Renderer: ScumJsonSettingsRenderer
  }],
  ["scum-raid-times", {
    kind: "module-addon" as const,
    sectionId: "raid",
    fieldKey: "raid_times",
    Renderer: ScumJsonSettingsRenderer
  }],
  ["scum-notifications", {
    kind: "module-addon" as const,
    sectionId: "notifications",
    fieldKey: "notifications",
    Renderer: ScumJsonSettingsRenderer
  }]
]);

export const scumSettingsDefinition: SettingsModuleDefinition = {
  id: "scum",
  fieldPresentationOverrides,
  specializedRenderers,
  getSections: buildScumSections,
  getAdditionalPresentationFields: ({ t }) => buildScumNativePresentationFields(t),
  buildFieldGroups: (sectionId, fields, _locale, t) => buildScumFieldGroups(sectionId, fields, t),
  getFieldCopy: (key, t) => buildScumFieldCopy(key, t),
  initializeSettings: (settings) => initializeScumSettings(settings),
  getSettingsValidationIssues: (settings, { t }) => [
    ...validateScumStructuredSettings(settings, t),
    ...validateScumJsonSettings(settings, t)
  ],
  resolveFieldEditorVariant(key) {
    return key === "admin_steam_ids" ? "string-list" : undefined;
  },
  getFieldValidationMessage({ field, value, t }) {
    if (field.key !== "admin_steam_ids") return undefined;
    const invalid = parseSteam64Ids(value).filter((entry) => !STEAM64_PATTERN.test(entry));
    if (invalid.length === 0) return undefined;
    const preview = invalid.slice(0, 3).join(", ");
    return t(
      "scum.settings.validation.adminSteamIds",
      { preview },
      `Admin list must use Steam64 IDs. Fix: ${preview}`
    );
  }
};
