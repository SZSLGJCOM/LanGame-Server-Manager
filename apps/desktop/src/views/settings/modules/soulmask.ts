import type { TranslateFn } from "../../../i18n";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type { GuidedFieldCopy, GuidedSettingsField, GuidedSettingsSection } from "../settings-schema";
import { SOULMASK_GROUP_SPECS } from "./soulmask-groups";

const SOULMASK_SECTIONS: GuidedSettingsSection[] = [
  {
    id: "room",
    title: "Server Identity",
    description: "Name, player cap, and optional PvE/PvP mode argument."
  },
  {
    id: "access",
    title: "Join & Admin",
    description: "Join and admin passwords passed into launch args."
  },
  {
    id: "world",
    title: "World Rules & Saves",
    description: "PvE/PvP mode and save or backup intervals."
  },
  {
    id: "xishu_general",
    title: "General Gameplay",
    description: "Core toggles, time, followers, permissions, and miscellaneous world behavior."
  },
  {
    id: "xishu_progression",
    title: "Progression",
    description: "Experience, leveling, attributes, training, and skill progression."
  },
  {
    id: "xishu_yields",
    title: "Yields & Crafting",
    description: "Harvest, drops, crops, animal production, crafting speed, and equipment drop correction."
  },
  {
    id: "xishu_building",
    title: "Building",
    description: "Decay, repair, portals, campfires, construction limits, and building interaction rules."
  },
  {
    id: "xishu_resources",
    title: "Resource Respawn",
    description: "Vegetation and resource respawn radii."
  },
  {
    id: "xishu_combat",
    title: "Combat",
    description: "Damage, recovery, quality, PvP damage, boss, dungeon, and tenacity tuning."
  },
  {
    id: "xishu_survival",
    title: "Survival Upkeep",
    description: "Durability, food, water, breath, fuel, spoilage, and repair consumption."
  },
  {
    id: "xishu_invasions",
    title: "Invasions",
    description: "Heat, random invasions, invasion wave size, timing, and rewards."
  },
  {
    id: "xishu_pvp_schedule",
    title: "PvP Schedule & Level Caps",
    description: "Regional PvP windows and open-server awareness level caps."
  },
  {
    id: "xishu_ai_followers",
    title: "AI & Followers",
    description: "AI difficulty and active follower counts."
  },
  {
    id: "xishu_battlefield",
    title: "Battlefield Windows",
    description: "Regional battlefield start and end windows."
  },
  {
    id: "xishu_events",
    title: "Server Events",
    description: "Global event region, timing, trigger probability, and open-day gates."
  },
  {
    id: "advanced",
    title: "Advanced",
    description: "Extra launch arguments appended to the Soulmask server command."
  },
];


function readCatalogText(t: TranslateFn, key: string): string | undefined {
  const value = t(key, undefined, "");
  return value.trim() ? value : undefined;
}

function buildSoulmaskSections(t: TranslateFn): GuidedSettingsSection[] {
  return SOULMASK_SECTIONS.map((section) => ({
    ...section,
    title: t(`soulmask.settings.sections.${section.id}`, undefined, section.title),
    description: t(`soulmask.settings.sections.${section.id}Description`, undefined, section.description ?? "")
  }));
}

function buildSoulmaskFieldCopy(key: string, t: TranslateFn): GuidedFieldCopy | undefined {
  const baseKey = `settings.schema.soulmask.${key}`;
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

function buildSoulmaskFieldGroups(
  sectionId: string,
  fields: GuidedSettingsField[],
  t: TranslateFn
): SettingsModuleFieldGroup[] {
  const fieldsByKey = new Map(fields.map((field) => [field.key, field]));
  const claimedKeys = new Set<string>();
  const groups: SettingsModuleFieldGroup[] = [];

  for (const spec of SOULMASK_GROUP_SPECS[sectionId] ?? []) {
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
      title: t(`soulmask.settings.groups.${spec.id}.title`, undefined, spec.title),
      description: t(`soulmask.settings.groups.${spec.id}.description`, undefined, spec.description),
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

export const soulmaskSettingsDefinition: SettingsModuleDefinition = {
  id: "soulmask",
  isFieldDisabled: (field) => field.key === "xishu_xi_shu_wei_ling",
  getSections: buildSoulmaskSections,
  buildFieldGroups: (sectionId, fields, _locale, t) => buildSoulmaskFieldGroups(sectionId, fields, t),
  getFieldCopy: (key, t) => buildSoulmaskFieldCopy(key, t)
};
