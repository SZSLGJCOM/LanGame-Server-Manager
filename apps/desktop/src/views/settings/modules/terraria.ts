import type { TranslateFn } from "../../../i18n";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type { GuidedFieldCopy, GuidedSettingsField, GuidedSettingsSection } from "../settings-schema";
import { terrariaReadBoolean, terrariaReadString } from "./terraria-helpers";

const WORLD_FILE_PATTERN = /^[^\\/]+\.wld$/i;

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

function buildTerrariaFieldCopy(key: string, t: TranslateFn): GuidedFieldCopy | undefined {
  const baseKey = `settings.schema.terraria.${key}`;
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

function getTerrariaEnumOptionLabel(fieldKey: string, value: unknown, t: TranslateFn): string | undefined {
  if (typeof value !== "string" && typeof value !== "number") {
    return undefined;
  }

  return readCatalogText(t, `settings.schema.terraria.${fieldKey}.option.${buildSchemaEnumOptionKey(value)}`);
}

function groupFields(
  fieldsByKey: Map<string, GuidedSettingsField>,
  id: string,
  title: string,
  description: string,
  layoutClass: string,
  keys: string[]
): SettingsModuleFieldGroup | null {
  const fields = keys
    .map((key) => fieldsByKey.get(key))
    .filter((field): field is GuidedSettingsField => Boolean(field));

  if (fields.length === 0) {
    return null;
  }

  return { id, title, description, layoutClass, fields };
}

function buildTerrariaSections(t: TranslateFn): GuidedSettingsSection[] {
  return [
    {
      id: "room",
      title: t("terraria.settings.sections.room", undefined, "Room Identity"),
      description: t(
        "terraria.settings.sections.roomDescription",
        undefined,
        "World name, save file, join message, and slot cap for the server your friends will actually see."
      )
    },
    {
      id: "world",
      title: t("terraria.settings.sections.world", undefined, "World Creation"),
      description: t(
        "terraria.settings.sections.worldDescription",
        undefined,
        "Fresh-world generation, difficulty, seeds, and Terraria's own rolling world copies."
      )
    },
    {
      id: "access",
      title: t("terraria.settings.sections.access", undefined, "Join & Trust"),
      description: t(
        "terraria.settings.sections.accessDescription",
        undefined,
        "Password gate, instance-local ban list, cheat protection, and optional Steam friend-lobby exposure."
      )
    },
    {
      id: "host",
      title: t("terraria.settings.sections.host", undefined, "Host Runtime"),
      description: t(
        "terraria.settings.sections.hostDescription",
        undefined,
        "Server runtime, NPC sync, liquid simulation and process priority."
      )
    },
    {
      id: "mods",
      title: t("terraria.settings.sections.mods", undefined, "tModLoader Mods"),
      description: t(
        "terraria.settings.sections.modsDescription",
        undefined,
        "Workshop item IDs and tModLoader internal names used by the instance-local modpack files."
      )
    },
    {
      id: "journey",
      parentId: "access",
      title: t("terraria.settings.sections.journey", undefined, "Journey Powers"),
      description: t(
        "terraria.settings.sections.journeyDescription",
        undefined,
        "Only matters for Journey worlds. Decide whether powers stay locked, host-only, or open to everyone."
      )
    }
  ];
}

function buildTerrariaFieldGroups(
  sectionId: string,
  fields: GuidedSettingsField[],
  _locale: string,
  t: TranslateFn
): SettingsModuleFieldGroup[] {
  const fieldsByKey = new Map(fields.map((field) => [field.key, field]));
  const claimedKeys = new Set<string>();

  const groupSpecs: Record<
    string,
    Array<{
      id: string;
      title: string;
      description: string;
      layoutClass: string;
      keys: string[];
    }>
  > = {
    world: [
      {
        id: "generation",
        title: t("terraria.settings.groups.generation", undefined, "Fresh-world recipe"),
        description: t(
          "terraria.settings.groups.generationDescription",
          undefined,
          "These settings only matter when Terraria has to create a new world file for this instance."
        ),
        layoutClass: "terraria-generation",
        keys: ["world_size", "difficulty", "seed", "special_seed"]
      },
      {
        id: "rollback",
        title: t("terraria.settings.groups.rollback", undefined, "Built-in world copies"),
        description: t(
          "terraria.settings.groups.rollbackDescription",
          undefined,
          "Terraria can keep rolling world copies next to the save file. This is separate from LanGame's managed backups."
        ),
        layoutClass: "terraria-rollback",
        keys: ["worldrollbackstokeep"]
      },
      {
        id: "announcements",
        title: t("terraria.settings.groups.announcements", undefined, "Announcement boxes"),
        description: t(
          "terraria.settings.groups.announcementsDescription",
          undefined,
          "Control whether wired announcement boxes broadcast messages and how far those messages travel."
        ),
        layoutClass: "terraria-announcements",
        keys: ["disableannouncementbox", "announcementboxrange"]
      }
    ],
    access: [
      {
        id: "join-gate",
        title: t("terraria.settings.groups.joinGate", undefined, "Anti-cheat protection"),
        description: t(
          "terraria.settings.groups.joinGateDescription",
          undefined,
          "Enable native cheat protection for connected players."
        ),
        layoutClass: "terraria-join-gate",
        keys: ["secure", "banlist_entries"]
      }
    ],
    network: [
      {
        id: "connections",
        title: t("terraria.settings.groups.connections", undefined, "Connection services"),
        description: t("terraria.settings.groups.connectionsDescription", undefined, "Steam connectivity and automatic port forwarding."),
        layoutClass: "terraria-connections",
        keys: ["steam", "upnp"]
      }
    ],
    host: [
      {
        id: "runtime-mode",
        title: t("terraria.settings.groups.runtimeMode", undefined, "Server runtime"),
        description: t(
          "terraria.settings.groups.runtimeModeDescription",
          undefined,
          "Choose the server runtime before changing mod files."
        ),
        layoutClass: "terraria-runtime-mode",
        keys: ["server_runtime", "tmodloader_runtime_dir"]
      },
      {
        id: "simulation",
        title: t("terraria.settings.groups.simulation", undefined, "Simulation pressure"),
        description: t(
          "terraria.settings.groups.simulationDescription",
          undefined,
          "NPC streaming, slow liquids, and process priority all belong to the host-operations layer."
        ),
        layoutClass: "terraria-simulation",
        keys: ["npcstream", "slowliquids", "priority"]
      }
    ],
    mods: [
      {
        id: "tmodloader-workshop",
        title: t("terraria.settings.groups.tmodloaderWorkshop", undefined, "Workshop modpack"),
        description: t(
          "terraria.settings.groups.tmodloaderWorkshopDescription",
          undefined,
          "Steam Workshop IDs populate install.txt; enabled.json still needs tModLoader internal mod names."
        ),
        layoutClass: "terraria-tmodloader-workshop",
        keys: ["tmodloader_workshop_item_ids", "tmodloader_enabled_mod_names"]
      }
    ],
    journey: [
      {
        id: "time",
        title: t("terraria.settings.groups.journeyTime", undefined, "Time controls"),
        description: t(
          "terraria.settings.groups.journeyTimeDescription",
          undefined,
          "Decide who can freeze time or jump directly to dawn, noon, dusk, midnight, and custom time speeds."
        ),
        layoutClass: "terraria-journey-time",
        keys: [
          "journeypermission_time_setfrozen",
          "journeypermission_time_setdawn",
          "journeypermission_time_setnoon",
          "journeypermission_time_setdusk",
          "journeypermission_time_setmidnight",
          "journeypermission_time_setspeed"
        ]
      },
      {
        id: "combat-world",
        title: t("terraria.settings.groups.journeyWorld", undefined, "Difficulty & world powers"),
        description: t(
          "terraria.settings.groups.journeyWorldDescription",
          undefined,
          "These controls decide who can alter difficulty, spawn pressure, biome spread, and broad world-state powers."
        ),
        layoutClass: "terraria-journey-world",
        keys: [
          "journeypermission_godmode",
          "journeypermission_setdifficulty",
          "journeypermission_setspawnrate",
          "journeypermission_increaseplacementrange",
          "journeypermission_biomespread_setfrozen"
        ]
      },
      {
        id: "weather",
        title: t("terraria.settings.groups.journeyWeather", undefined, "Wind & rain"),
        description: t(
          "terraria.settings.groups.journeyWeatherDescription",
          undefined,
          "Control who can change weather strength or freeze wind and rain states in Journey mode."
        ),
        layoutClass: "terraria-journey-weather",
        keys: [
          "journeypermission_wind_setstrength",
          "journeypermission_wind_setfrozen",
          "journeypermission_rain_setstrength",
          "journeypermission_rain_setfrozen"
        ]
      }
    ]
  };

  const groups = (groupSpecs[sectionId] ?? [])
    .map((group) => {
      const built = groupFields(
        fieldsByKey,
        group.id,
        group.title,
        group.description,
        group.layoutClass,
        group.keys
      );
      if (!built) {
        return null;
      }

      for (const field of built.fields) {
        claimedKeys.add(field.key);
      }

      return built;
    })
    .filter((group): group is SettingsModuleFieldGroup => Boolean(group));

  const remainingFields = fields.filter((field) => !claimedKeys.has(field.key));
  if (remainingFields.length > 0) {
    groups.push({
      id: "additional",
      title: t("terraria.settings.groups.additional", undefined, "Additional fields"),
      description: t(
        "terraria.settings.groups.additionalDescription",
        undefined,
        "These settings are still part of the same section but do not fit the main Terraria workflow groups."
      ),
      layoutClass: `${sectionId}-additional`,
      fields: remainingFields
    });
  }

  return groups;
}

export const terrariaSettingsDefinition: SettingsModuleDefinition = {
  id: "terraria",
  getSections: buildTerrariaSections,
  buildFieldGroups(sectionId, fields, locale, t) {
    return buildTerrariaFieldGroups(sectionId, fields, locale, t);
  },
  getFieldCopy: (key, t) => buildTerrariaFieldCopy(key, t),
  getEnumOptionLabel: (fieldKey, value, _locale, t) => getTerrariaEnumOptionLabel(fieldKey, value, t),
  resolveFieldEditorVariant(key) {
    if (key === "tmodloader_workshop_item_ids") {
      return "workshop-id-list";
    }
    if (key === "banlist_entries") {
      return "string-list";
    }
    if (key === "tmodloader_enabled_mod_names") {
      return "string-list";
    }
    return undefined;
  },
  getFieldValidationMessage({ field, value, settings, t }) {
    const plainSeed = terrariaReadString(settings.seed).trim();
    const specialSeed = terrariaReadString(settings.special_seed).trim().toLowerCase();

    if (field.key === "world_file") {
      const text = terrariaReadString(value).trim();
      if (text.length === 0 || WORLD_FILE_PATTERN.test(text)) {
        return undefined;
      }

      return t(
        "terraria.settings.validation.worldFile",
        undefined,
        "Use a simple .wld file name without any folder segments."
      );
    }

    if (field.key === "max_players") {
      const parsed = Number(value);
      if (!Number.isFinite(parsed) || parsed < 1 || parsed > 255) {
        return t(
          "terraria.settings.validation.maxPlayersRange",
          undefined,
          "Max players must stay between 1 and 255."
        );
      }
    }

    if (field.key === "worldrollbackstokeep") {
      const parsed = Number(value);
      if (!Number.isFinite(parsed) || parsed < 0) {
        return t(
          "terraria.settings.validation.rollbackNonNegative",
          undefined,
          "Rolling world backups cannot be less than 0."
        );
      }
    }

    if ((field.key === "seed" || field.key === "special_seed") && plainSeed.length > 0 && specialSeed !== "" && specialSeed !== "none") {
      return t(
        "terraria.settings.validation.specialSeedOverrides",
        undefined,
        "A special seed is enabled, so plain seed text will not control fresh-world generation."
      );
    }

    if (field.key === "lobby") {
      const lobbyValue = terrariaReadString(value).trim().toLowerCase();
      if ((lobbyValue === "friends" || lobbyValue === "private") && !terrariaReadBoolean(settings.steam)) {
        return t(
          "terraria.settings.validation.lobbyRequiresSteam",
          undefined,
          "Enable Steam Support before exposing a Steam lobby."
        );
      }
    }

    if (field.key === "npcstream") {
      const parsed = Number(value);
      if (!Number.isFinite(parsed) || parsed < 0) {
        return t(
          "terraria.settings.validation.npcstreamNonNegative",
          undefined,
          "NPC stream radius must be 0 or greater."
        );
      }
    }

    if (field.key === "announcementboxrange") {
      const parsed = Number(value);
      if (!Number.isFinite(parsed) || parsed < -1) {
        return t(
          "terraria.settings.validation.announcementRange",
          undefined,
          "Announcement box range must be -1 or greater."
        );
      }

      if (terrariaReadBoolean(settings.disableannouncementbox)) {
        return t(
          "terraria.settings.validation.announcementMuted",
          undefined,
          "Announcement boxes are muted, so this range will be ignored until they are enabled again."
        );
      }
    }

    return undefined;
  }
};
