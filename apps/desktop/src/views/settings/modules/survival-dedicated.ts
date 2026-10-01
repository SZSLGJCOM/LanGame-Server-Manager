import type { TranslateFn } from "../../../i18n";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type { GuidedSettingsField, GuidedSettingsSection } from "../settings-schema";

const SECTION_COPY: Record<string, { title: string; description: string }> = {
  room: {
    title: "Room Settings",
    description: "Identity, join password, player capacity, listing, and world selection."
  },
  network: {
    title: "Network",
    description: "Listen and public addresses, ports, and remote control connections."
  },
  access: {
    title: "Administration & Permissions",
    description: "Operator credentials, permission levels, and admission policies."
  },
  world: {
    title: "World",
    description: "World generation, difficulty, and gameplay rules."
  },
  admin: {
    title: "Admin",
    description: "Operator credentials and management permissions."
  },
  performance: {
    title: "Performance",
    description: "Game-native frame, tick, activity, and resource controls."
  },
  advanced: {
    title: "Advanced",
    description: "Low-level launch arguments and settings that should stay explicit."
  }
};

interface SurvivalDedicatedFieldGroupSpec {
  id: string;
  sectionId: string;
  title?: string;
  description?: string;
  keys: readonly string[];
}

function buildSections(moduleId: string, sectionIds: readonly string[], t: TranslateFn): GuidedSettingsSection[] {
  return sectionIds.map((sectionId) => {
    const copy = SECTION_COPY[sectionId] ?? {
      title: sectionId,
      description: ""
    };

    return {
      id: sectionId,
      title: t(`${moduleId}.settings.sections.${sectionId}`, undefined, copy.title),
      description: t(
        `${moduleId}.settings.sections.${sectionId}Description`,
        undefined,
        copy.description
      )
    };
  });
}

function buildSingleFieldGroup(
  moduleId: string,
  sectionId: string,
  fields: GuidedSettingsField[],
  t: TranslateFn
): SettingsModuleFieldGroup[] {
  if (fields.length === 0) {
    return [];
  }

  return [
    {
      id: sectionId,
      title: t(`${moduleId}.settings.groups.${sectionId}.title`, undefined, SECTION_COPY[sectionId]?.title),
      description: t(
        `${moduleId}.settings.groups.${sectionId}.description`,
        undefined,
        SECTION_COPY[sectionId]?.description ?? ""
      ),
      layoutClass: `${moduleId}-${sectionId}`,
      fields
    }
  ];
}

function buildConfiguredFieldGroups(
  moduleId: string,
  sectionId: string,
  fields: GuidedSettingsField[],
  groupSpecs: readonly SurvivalDedicatedFieldGroupSpec[],
  t: TranslateFn
): SettingsModuleFieldGroup[] {
  const sectionGroupSpecs = groupSpecs.filter((group) => group.sectionId === sectionId);
  if (sectionGroupSpecs.length === 0) {
    return buildSingleFieldGroup(moduleId, sectionId, fields, t);
  }

  const fieldByKey = new Map(fields.map((field) => [field.key, field]));
  const claimedKeys = new Set<string>();
  const groups: SettingsModuleFieldGroup[] = [];

  sectionGroupSpecs.forEach((groupSpec) => {
    const groupFields = groupSpec.keys
      .map((key) => {
        claimedKeys.add(key);
        return fieldByKey.get(key);
      })
      .filter((field): field is GuidedSettingsField => Boolean(field));

    if (groupFields.length === 0) {
      return;
    }

    const copy = SECTION_COPY[sectionId];
    groups.push({
      id: groupSpec.id,
      title: t(
        `${moduleId}.settings.groups.${groupSpec.id}.title`,
        undefined,
        groupSpec.title ?? copy?.title ?? groupSpec.id
      ),
      description: t(
        `${moduleId}.settings.groups.${groupSpec.id}.description`,
        undefined,
        groupSpec.description ?? copy?.description ?? ""
      ),
      layoutClass: `${moduleId}-${groupSpec.id}`,
      fields: groupFields
    });
  });

  const remainingFields = fields.filter((field) => !claimedKeys.has(field.key));
  if (remainingFields.length > 0) {
    groups.push(...buildSingleFieldGroup(moduleId, sectionId, remainingFields, t));
  }

  return groups;
}

export function createSurvivalDedicatedSettingsDefinition(
  id: string,
  sectionIds: readonly string[],
  groupSpecs: readonly SurvivalDedicatedFieldGroupSpec[] = []
): SettingsModuleDefinition {
  return {
    id,
    getSections: (t) => buildSections(id, sectionIds, t),
    buildFieldGroups: (sectionId, fields, _locale, t) =>
      buildConfiguredFieldGroups(id, sectionId, fields, groupSpecs, t),
    getFieldCopy: () => undefined
  };
}
