import type { TranslateFn } from "../../../i18n";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type { GuidedFieldCopy, GuidedSettingsField, GuidedSettingsSection } from "../settings-schema";
import { satisfactoryNativePresentation } from "./satisfactory-native-presentation";

const SATISFACTORY_SECTIONS: GuidedSettingsSection[] = [
  {
    id: "room",
    title: "Server Runtime",
    description: "Player capacity and server runtime settings."
  },
  {
    id: "network",
    title: "Network",
    description: "Network quality, connection limits, and local API access."
  },
  {
    id: "world",
    title: "World Rules",
    description: "World simulation and seasonal event rules."
  },
  { id: "world_generation", parentId: "world", title: "New World", description: "Create a separate world with native generation rules." },
  { id: "creative_rules", parentId: "world", title: "Creative Mode", description: "World rules and defaults for new players." },
  {
    id: "advanced",
    title: "Advanced",
    description: "Simulation tick rate, privacy, logs, and native overrides."
  }
];

interface SatisfactoryFieldGroupSpec {
  id: string;
  title: string;
  description: string;
  layoutClass: string;
  keys: string[];
}

const SATISFACTORY_GROUP_SPECS: Record<string, SatisfactoryFieldGroupSpec[]> = {
  network: [
    {
      id: "local-api",
      title: "Local API",
      description:
        "Only enable insecure local API access when this Windows host needs trusted local automation or diagnostics.",
      layoutClass: "satisfactory-local-api",
      keys: [
        "allow_insecure_local_api",
        "external_reliable_port",
        "disable_packet_routing"
      ]
    },
    {
      id: "engine-networking",
      title: "Engine networking",
      description: "Network quality, client connection timeouts, and transfer-rate limits.",
      layoutClass: "satisfactory-engine-networking",
      keys: ["network_quality", "net_connection_timeout_seconds", "net_initial_connect_timeout_seconds", "net_max_client_rate", "net_max_internet_client_rate"]
    }
  ],
  world: [
    {
      id: "simulation",
      title: "World simulation",
      description: "Pause policy and weather presets for the world.",
      layoutClass: "satisfactory-simulation",
      keys: ["auto_pause_when_empty", "weather_preset"]
    },
    {
      id: "events",
      title: "Seasonal events",
      description: "Enable or disable seasonal game events.",
      layoutClass: "satisfactory-events",
      keys: ["disable_seasonal_events"]
    }
  ],
  advanced: [
    {
      id: "privacy",
      title: "Gameplay data privacy",
      description: "Control gameplay data sent to Coffee Stain Studios.",
      layoutClass: "satisfactory-privacy",
      keys: ["send_gameplay_data"]
    },
    {
      id: "engine-runtime",
      title: "Engine runtime and logs",
      description: "Simulation tick rate, crash reporting, and server-query logs.",
      layoutClass: "satisfactory-engine-runtime",
      keys: [
        "net_server_max_tick_rate",
        "disable_crash_reporting",
        "server_query_log_level"
      ]
    },
    {
      id: "advanced-overrides",
      title: "Advanced overrides",
      description: "Use raw INI append lanes and launch flags for unsupported dedicated-server fields.",
      layoutClass: "satisfactory-advanced-overrides",
      keys: ["engine_ini_extra", "game_ini_extra", "custom_launch_flags"]
    }
  ]
};

function readCatalogText(t: TranslateFn, key: string): string | undefined {
  const value = t(key, undefined, "");
  return value.trim() ? value : undefined;
}

function buildSchemaEnumOptionKey(value: unknown): string {
  const raw = String(value).trim();
  const prefix = raw.startsWith("-") ? "minus_" : "";
  const normalized = raw
    .replace(/^-+/, "")
    .replace(/[^A-Za-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "")
    .toLowerCase();

  return `${prefix}${normalized || "empty"}`;
}

function buildSatisfactorySections(t: TranslateFn): GuidedSettingsSection[] {
  return SATISFACTORY_SECTIONS.map((section) => ({
    ...section,
    title: t(`satisfactory.settings.sections.${section.id}`, undefined, section.title),
    description: t(
      `satisfactory.settings.sections.${section.id}Description`,
      undefined,
      section.description ?? ""
    )
  }));
}

function buildSatisfactoryFieldCopy(key: string, t: TranslateFn): GuidedFieldCopy | undefined {
  const baseKey = `settings.schema.satisfactory.${key}`;
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

function getSatisfactoryEnumOptionLabel(fieldKey: string, value: unknown, t: TranslateFn): string | undefined {
  if (typeof value !== "string" && typeof value !== "number") {
    return undefined;
  }

  return readCatalogText(t, `settings.schema.satisfactory.${fieldKey}.option.${buildSchemaEnumOptionKey(value)}`);
}

function buildSatisfactoryFieldGroups(
  sectionId: string,
  fields: GuidedSettingsField[],
  t: TranslateFn
): SettingsModuleFieldGroup[] {
  const fieldsByKey = new Map(fields.map((field) => [field.key, field]));
  const claimedKeys = new Set<string>();
  const groups: SettingsModuleFieldGroup[] = [];

  for (const spec of SATISFACTORY_GROUP_SPECS[sectionId] ?? []) {
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
      title: t(`satisfactory.settings.groups.${spec.id}.title`, undefined, spec.title),
      description: t(`satisfactory.settings.groups.${spec.id}.description`, undefined, spec.description),
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

function readNumber(value: unknown): number | null {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

export const satisfactorySettingsDefinition: SettingsModuleDefinition = {
  id: "satisfactory",
  ...satisfactoryNativePresentation,
  getSections: buildSatisfactorySections,
  buildFieldGroups: (sectionId, fields, _locale, t) =>
    buildSatisfactoryFieldGroups(sectionId, fields, t),
  getFieldCopy: (key, t) => buildSatisfactoryFieldCopy(key, t),
  getEnumOptionLabel: (fieldKey, value, _locale, t) => getSatisfactoryEnumOptionLabel(fieldKey, value, t),
  getFieldValidationMessage({ field, value, settings, t }) {
    if (field.key === "net_max_internet_client_rate") {
      const internetRate = readNumber(value);
      const maxRate = readNumber(settings.net_max_client_rate);
      if (internetRate !== null && maxRate !== null && internetRate > maxRate) {
        return t(
          "satisfactory.settings.validation.internetRateHigherThanMax",
          undefined,
          "Max internet client rate should not exceed the overall max client rate."
        );
      }
    }

    return undefined;
  }
};
