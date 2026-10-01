import type { TranslateFn } from "../../../i18n";
import { WindroseWorldNameField, WindroseWorldSettingsPanel } from "../WindroseWorldSettingsPanel";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type { GuidedFieldCopy, GuidedSettingsField, GuidedSettingsSection } from "../settings-schema";

const WINDROSE_SECTIONS: GuidedSettingsSection[] = [
  {
    id: "room",
    title: "Server Identity",
    description: "Primary server identity and player cap."
  },
  {
    id: "network",
    title: "Connection Routing",
    description: "Direct and P2P routing addresses exposed to clients."
  },
  {
    id: "world",
    title: "World Rules",
    description: "Presets, combat, exploration and co-op rules for the selected world."
  },
  {
    id: "advanced",
    title: "Runtime & Recovery",
    description: "Save recovery and isolated server process behavior."
  }
];

interface WindroseGroupSpec {
  id: string;
  title: string;
  description: string;
  layoutClass: string;
  keys: string[];
}

const WINDROSE_GROUP_SPECS: Record<string, WindroseGroupSpec[]> = {
  network: [
    {
      id: "routing",
      title: "Routing",
      description: "Direct-connect mode and routable endpoint addresses.",
      layoutClass: "windrose-routing",
      keys: [
        "direct_connect_enabled",
        "p2p_proxy_address",
        "direct_connection_proxy_address"
      ]
    }
  ],
  advanced: [
    {
      id: "recovery",
      title: "Recovery and processes",
      description: "Save recovery and isolated server process behavior.",
      layoutClass: "windrose-recovery",
      keys: [
        "auto_load_latest_backup_if_has_broken",
        "allow_multiple_server_instances"
      ]
    }
  ],
};

const WINDROSE_WORLD_PARAMETER_KEYS = new Set([
  "world_preset_type",
  "coop_quests",
  "easy_explore",
  "mob_health_multiplier",
  "mob_damage_multiplier",
  "ship_health_multiplier",
  "ship_damage_multiplier",
  "boarding_difficulty_multiplier",
  "coop_stats_correction_modifier",
  "coop_ship_stats_correction_modifier",
  "combat_difficulty"
]);

const WINDROSE_WORLD_DEFAULTS = {
  world_name: "The Archipelago",
  world_preset_type: "Medium",
  coop_quests: true,
  easy_explore: false,
  mob_health_multiplier: 1,
  mob_damage_multiplier: 1,
  ship_health_multiplier: 1,
  ship_damage_multiplier: 1,
  boarding_difficulty_multiplier: 1,
  coop_stats_correction_modifier: 1,
  coop_ship_stats_correction_modifier: 0,
  combat_difficulty: "Normal"
} as const;

function readCatalogText(t: TranslateFn, key: string): string | undefined {
  const value = t(key, undefined, "");
  return value.trim() ? value : undefined;
}

function buildWindroseSections(t: TranslateFn): GuidedSettingsSection[] {
  return WINDROSE_SECTIONS.map((section) => ({
    ...section,
    title: t(`windrose.settings.sections.${section.id}`, undefined, section.title),
    description: t(
      `windrose.settings.sections.${section.id}Description`,
      undefined,
      section.description ?? ""
    )
  }));
}

function buildWindroseFieldCopy(key: string, t: TranslateFn): GuidedFieldCopy | undefined {
  const baseKey = `settings.schema.windrose.${key}`;
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

function buildWindroseFieldGroups(
  sectionId: string,
  fields: GuidedSettingsField[],
  t: TranslateFn
): SettingsModuleFieldGroup[] {
  const fieldsByKey = new Map(fields.map((field) => [field.key, field]));
  const claimedKeys = new Set<string>();
  const groups: SettingsModuleFieldGroup[] = [];

  for (const spec of WINDROSE_GROUP_SPECS[sectionId] ?? []) {
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
      title: t(`windrose.settings.groups.${spec.id}.title`, undefined, spec.title),
      description: t(
        `windrose.settings.groups.${spec.id}.description`,
        undefined,
        spec.description
      ),
      layoutClass: spec.layoutClass,
      fields: groupFields
    });
  }

  const remainingFields = fields.filter((field) =>
    !claimedKeys.has(field.key) && !WINDROSE_WORLD_PARAMETER_KEYS.has(field.key)
  );
  if (remainingFields.length > 0) {
    groups.push({
      id: "additional",
      layoutClass: `${sectionId}-additional`,
      fields: remainingFields
    });
  }

  return groups.length > 0 ? groups : [{ id: "default", fields }];
}

export const windroseSettingsDefinition: SettingsModuleDefinition = {
  id: "windrose",
  fieldPresentationOverrides: {
    ...Object.fromEntries(
      [...WINDROSE_WORLD_PARAMETER_KEYS].map((key) => [key, {
        state: "specialized" as const,
        owner: "configuration" as const,
        sectionId: "world",
        rendererId: "windrose-world-settings"
      }])
    ),
    world_name: {
      state: "specialized",
      owner: "configuration",
      sectionId: "room",
      rendererId: "windrose-world-name"
    }
  },
  specializedRenderers: {
    "windrose-world-name": {
      kind: "module-addon",
      sectionId: "room",
      Renderer: WindroseWorldNameField
    },
    "windrose-world-settings": {
      kind: "module-addon",
      sectionId: "world",
      Renderer: WindroseWorldSettingsPanel
    }
  },
  getSections: buildWindroseSections,
  buildFieldGroups: (sectionId, fields, _locale, t) => buildWindroseFieldGroups(sectionId, fields, t),
  getFieldCopy: (key, t) => buildWindroseFieldCopy(key, t),
  initializeSettings: (settings) => ({ ...WINDROSE_WORLD_DEFAULTS, ...settings })
};
