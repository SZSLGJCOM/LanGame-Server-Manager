import { applyUnturnedSettingsPatch, validateUnturnedLobbyLinks } from "./unturned-native-settings";
import type { TranslateFn } from "../../../i18n";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type { GuidedFieldCopy, GuidedSettingsField, GuidedSettingsSection } from "../settings-schema";
import {
  UNTURNED_BROWSER_KEYS,
  UNTURNED_SERVER_KEYS,
  UNTURNED_ITEMS_KEYS,
  UNTURNED_VEHICLES_KEYS,
  UNTURNED_ZOMBIES_KEYS,
  UNTURNED_ANIMALS_KEYS,
  UNTURNED_BARRICADES_KEYS,
  UNTURNED_STRUCTURES_KEYS,
  UNTURNED_PLAYERS_KEYS,
  UNTURNED_OBJECTS_KEYS,
  UNTURNED_EVENTS_KEYS,
  UNTURNED_UNITYEVENTS_KEYS,
  UNTURNED_GAMEPLAY_KEYS,
} from "./unturned-native-groups";

const STEAM64_PATTERN = /^\d{17}$/;
const GSLT_PATTERN = /^[A-Za-z0-9]{16,64}$/;

const UNTURNED_SECTIONS: GuidedSettingsSection[] = [
  {
    id: "room",
    title: "Server Listing",
    description: "Name, welcome message, and slot cap that players see before and during join."
  },
  {
    id: "network",
    title: "Join Mode",
    description: "Connection addresses, FakeIP routing, and timeout and ping limits."
  },
  {
    id: "world",
    title: "Map & Rules",
    description: "Map, game mode, perspective, and PvE posture written into Commands.dat."
  },
  {
    id: "access",
    title: "Access & Trust",
    description: "Join password, owner Steam64 ID, whitelist, cheats, BattlEye, and admin visibility."
  },
  {
    id: "advanced",
    title: "Host & Workshop",
    description: "Current Config.txt controls and Workshop downloads tied to this ServerID folder."
  },
  { id: "items", title: "Items and loot", description: "World rules applied to every player." },
  { id: "vehicles", title: "Vehicles", description: "World rules applied to every player." },
  { id: "zombies", title: "Zombies", description: "World rules applied to every player." },
  { id: "animals", title: "Animals", description: "World rules applied to every player." },
  { id: "building", title: "Barricades", description: "World rules applied to every player." },
  { id: "survival", title: "Survival and progression", description: "World rules applied to every player." },
  { id: "objects", title: "World objects", description: "World rules applied to every player." },
  { id: "events", title: "Weather and arena", description: "World rules applied to every player." },
  { id: "gameplay", title: "Gameplay rules", description: "World rules applied to every player." },

];

interface UnturnedFieldGroupSpec {
  id: string;
  title: string;
  description: string;
  layoutClass: string;
  keys: string[];
}

const UNTURNED_GROUP_SPECS: Record<string, UnturnedFieldGroupSpec[]> = {
  gameplay: [
    { id: "native-gameplay", title: "Gameplay rules", description: "Native server rules; unset values retain current configuration.", layoutClass: "unturned-native-gameplay", keys: UNTURNED_GAMEPLAY_KEYS },
  ],
  events: [
    { id: "native-events", title: "Weather and arena", description: "Native server rules; unset values retain current configuration.", layoutClass: "unturned-native-events", keys: UNTURNED_EVENTS_KEYS },
  ],
  objects: [
    { id: "native-objects", title: "World objects", description: "Native server rules; unset values retain current configuration.", layoutClass: "unturned-native-objects", keys: UNTURNED_OBJECTS_KEYS },
  ],
  survival: [
    { id: "native-players", title: "Survival and progression", description: "Native server rules; unset values retain current configuration.", layoutClass: "unturned-native-players", keys: UNTURNED_PLAYERS_KEYS },
  ],
  building: [
    { id: "native-structures", title: "Structures", description: "Native server rules; unset values retain current configuration.", layoutClass: "unturned-native-structures", keys: UNTURNED_STRUCTURES_KEYS },
    { id: "native-barricades", title: "Barricades", description: "Native server rules; unset values retain current configuration.", layoutClass: "unturned-native-barricades", keys: UNTURNED_BARRICADES_KEYS },
  ],
  animals: [
    { id: "native-animals", title: "Animals", description: "Native server rules; unset values retain current configuration.", layoutClass: "unturned-native-animals", keys: UNTURNED_ANIMALS_KEYS },
  ],
  zombies: [
    { id: "native-zombies", title: "Zombies", description: "Native server rules; unset values retain current configuration.", layoutClass: "unturned-native-zombies", keys: UNTURNED_ZOMBIES_KEYS },
  ],
  vehicles: [
    { id: "native-vehicles", title: "Vehicles", description: "Native server rules; unset values retain current configuration.", layoutClass: "unturned-native-vehicles", keys: UNTURNED_VEHICLES_KEYS },
  ],
  items: [
    { id: "native-items", title: "Items and loot", description: "Native server rules; unset values retain current configuration.", layoutClass: "unturned-native-items", keys: UNTURNED_ITEMS_KEYS },
  ],
  room: [
    { id: "native-browser", title: "Server listing", description: "Native server rules; unset values retain current configuration.", layoutClass: "unturned-native-browser", keys: UNTURNED_BROWSER_KEYS },
  ],
  network: [
    { id: "native-server", title: "Connection protection", description: "Native server rules; unset values retain current configuration.", layoutClass: "unturned-native-server", keys: UNTURNED_SERVER_KEYS },
    {
      id: "internet",
      title: "Connection addresses",
      description: "Bookmark address and FakeIP routing for this server.",
      layoutClass: "unturned-internet",
      keys: ["bookmark_host", "use_fake_ip"]
    },
    {
      id: "network-quality",
      title: "Connection guardrails",
      description: "Timeout and ping caps decide when the server drops unstable clients.",
      layoutClass: "unturned-network-quality",
      keys: ["timeout_seconds", "max_ping"]
    }
  ],
  world: [
    {
      id: "map-rules",
      title: "Gameplay rules",
      description: "Difficulty, game mode, perspective and PvE rules.",
      layoutClass: "unturned-map-rules",
      keys: ["difficulty", "game_mode", "perspective", "pve", "prevent_level_skill_overrides"]
    }
  ],
  access: [
    { id: "native-protection", title: "Game integrity", description: "Global game-integrity policies.", layoutClass: "unturned-native-protection", keys: ["native_server_validate_econ_info_hash", "native_players_enable_terrain_color_kick"] },
    { id: "native-unityevents", title: "Content command permissions", description: "Native server rules; unset values retain current configuration.", layoutClass: "unturned-native-unityevents", keys: UNTURNED_UNITYEVENTS_KEYS },
    {
      id: "access-gate",
      title: "Permissions and protection",
      description: "Operator permissions, allow-list policy and anti-cheat protection.",
      layoutClass: "unturned-access-gate",
      keys: [
        "owner_steam_id",
        "game_server_login_token",
        "admin_steam_ids",
        "whitelist_enabled",
        "cheats",
        "battl_eye",
        "vac_secure",
        "hide_admins"
      ]
    }
  ],
  advanced: [
    {
      id: "configuration-output",
      title: "Native configuration behavior",
      description: "Level overrides, configuration logging, and generated Config.txt formatting.",
      layoutClass: "unturned-configuration-output",
      keys: [
        "no_level_config_overrides",
        "log_gameplay_config",
        "gameplay_config_no_generated_comments",
        "gameplay_config_no_empty_values"
      ]
    },
    {
      id: "workshop",
      title: "Workshop downloads",
      description: "Workshop File IDs materialize into WorkshopDownloadConfig.json for this ServerID.",
      layoutClass: "unturned-workshop",
      keys: [
        "workshop_file_ids",
        "workshop_ignore_children_file_ids",
        "workshop_query_cache_max_age_seconds",
        "workshop_max_query_retries",
        "workshop_use_cached_downloads",
        "workshop_monitor_updates"
      ]
    },
    {
      id: "launch-overrides",
      title: "Launch overrides",
      description: "Advanced launch flags for custom runtime behavior.",
      layoutClass: "unturned-launch-overrides",
      keys: ["custom_launch_flags"]
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

function buildUnturnedSections(t: TranslateFn): GuidedSettingsSection[] {
  return UNTURNED_SECTIONS.map((section) => ({
    ...section,
    title: t(`unturned.settings.sections.${section.id}`, undefined, section.title),
    description: t(
      `unturned.settings.sections.${section.id}Description`,
      undefined,
      section.description ?? ""
    )
  }));
}

function buildUnturnedFieldCopy(key: string, t: TranslateFn): GuidedFieldCopy | undefined {
  const baseKey = `settings.schema.unturned.${key}`;
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

function getUnturnedEnumOptionLabel(fieldKey: string, value: unknown, t: TranslateFn): string | undefined {
  if (typeof value !== "string" && typeof value !== "number") {
    return undefined;
  }

  return readCatalogText(t, `settings.schema.unturned.${fieldKey}.option.${buildSchemaEnumOptionKey(value)}`);
}

function buildUnturnedFieldGroups(
  sectionId: string,
  fields: GuidedSettingsField[],
  t: TranslateFn
): SettingsModuleFieldGroup[] {
  const fieldsByKey = new Map(fields.map((field) => [field.key, field]));
  const claimedKeys = new Set<string>();
  const groups: SettingsModuleFieldGroup[] = [];

  for (const spec of UNTURNED_GROUP_SPECS[sectionId] ?? []) {
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
      title: t(`unturned.settings.groups.${spec.id}.title`, undefined, spec.title),
      description: t(`unturned.settings.groups.${spec.id}.description`, undefined, spec.description),
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

function readText(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

function readBoolean(value: unknown): boolean {
  if (typeof value === "boolean") {
    return value;
  }

  if (typeof value === "string") {
    return value.trim().toLowerCase() === "true";
  }

  return Boolean(value);
}

function parseWorkshopIdList(value: unknown): string[] {
  if (typeof value !== "string") {
    return [];
  }

  return value
    .replace(/\r\n/g, "\n")
    .replace(/\r/g, "\n")
    .split(/[\n,;]+/)
    .map((entry) => entry.trim())
    .filter(Boolean);
}

function parseSteam64IdList(value: unknown): string[] {
  if (typeof value !== "string") {
    return [];
  }

  return value
    .replace(/\r\n/g, "\n")
    .replace(/\r/g, "\n")
    .split(/[\n,;]+/)
    .map((entry) => entry.trim())
    .filter(Boolean);
}

function buildWorkshopValidationMessage(value: unknown, t: TranslateFn): string | undefined {
  const invalidIds = parseWorkshopIdList(value).filter((entry) => !/^\d+$/.test(entry));
  if (invalidIds.length === 0) {
    return undefined;
  }

  const preview = invalidIds.slice(0, 3).join(", ");
  return t(
    "unturned.settings.validation.workshopIds",
    { preview },
    `Workshop File IDs must be numeric. Fix: ${preview}`
  );
}

export const unturnedSettingsDefinition: SettingsModuleDefinition = {
  id: "unturned",
  applySettingsPatch: applyUnturnedSettingsPatch,
  getSections: buildUnturnedSections,
  buildFieldGroups: (sectionId, fields, _locale, t) =>
    buildUnturnedFieldGroups(sectionId, fields, t),
  getFieldCopy: (key, t) => buildUnturnedFieldCopy(key, t),
  getEnumOptionLabel: (fieldKey, value, _locale, t) => getUnturnedEnumOptionLabel(fieldKey, value, t),
  resolveFieldEditorVariant(key) {
    if (key === "workshop_file_ids" || key === "workshop_ignore_children_file_ids") {
      return "workshop-id-list";
    }
    if (key === "admin_steam_ids") {
      return "string-list";
    }

    return undefined;
  },
  getFieldValidationMessage({ field, value, settings, t }) {
    if (field.key === "browser_links_json") {
      return validateUnturnedLobbyLinks(value, t);
    }
    if (field.key === "server_name" && readText(value).length === 0) {
      return t(
        "unturned.settings.validation.serverNameRequired",
        undefined,
        "Give this Unturned server a name before launch."
      );
    }

    if (field.key === "map" && readText(value).length === 0) {
      return t(
        "unturned.settings.validation.mapRequired",
        undefined,
        "Set the map name before launch."
      );
    }

    if (field.key === "owner_steam_id") {
      const ownerSteamId = readText(value);
      if (ownerSteamId.length > 0 && !STEAM64_PATTERN.test(ownerSteamId)) {
        return t(
          "unturned.settings.validation.ownerSteamId",
          undefined,
          "Owner must be a Steam64 ID."
        );
      }
    }

    if (field.key === "admin_steam_ids") {
      const invalidEntries = parseSteam64IdList(value).filter((entry) => !STEAM64_PATTERN.test(entry));
      if (invalidEntries.length > 0) {
        const preview = invalidEntries.slice(0, 3).join(", ");
        return t(
          "unturned.settings.validation.adminSteamIds",
          { preview },
          `Admin list must use Steam64 IDs. Fix: ${preview}`
        );
      }
    }

    if (field.key === "game_server_login_token") {
      const token = readText(value);
      if (token.length > 0 && !GSLT_PATTERN.test(token)) {
        return t(
          "unturned.settings.validation.gslt",
          undefined,
          "GSLT must be an alphanumeric token."
        );
      }
      if (readBoolean(settings.internet_server) && token.length === 0) {
        return t(
          "unturned.settings.validation.internetNeedsGslt",
          undefined,
          "Internet listing is enabled but no GSLT is set. Configure one before expecting public browser visibility."
        );
      }
    }

    if (field.key === "internet_server") {
      const internetEnabled = readBoolean(value);
      const token = readText(settings.game_server_login_token);
      if (internetEnabled && token.length === 0) {
        return t(
          "unturned.settings.validation.internetNeedsGslt",
          undefined,
          "Internet listing is enabled but no GSLT is set. Configure one before expecting public browser visibility."
        );
      }
    }

    if (field.key === "bookmark_host") {
      const bookmarkHost = readText(value);
      const fakeIpEnabled = readBoolean(settings.use_fake_ip);
      if (fakeIpEnabled && bookmarkHost.length === 0) {
        return t(
          "unturned.settings.validation.bookmarkHostNeeded",
          undefined,
          "When FakeIP mode is enabled, set Bookmark Host so clients can resolve this room consistently."
        );
      }
    }

    if (field.key === "use_fake_ip" && readBoolean(value)) {
      const bookmarkHost = readText(settings.bookmark_host);
      if (bookmarkHost.length === 0) {
        return t(
          "unturned.settings.validation.bookmarkHostNeeded",
          undefined,
          "When FakeIP mode is enabled, set Bookmark Host so clients can resolve this room consistently."
        );
      }
    }

    if (field.key === "workshop_file_ids" || field.key === "workshop_ignore_children_file_ids") {
      return buildWorkshopValidationMessage(value, t);
    }

    if (field.key === "cheats" && readBoolean(value)) {
      return t(
        "unturned.settings.validation.cheatsEnabled",
        undefined,
        "Cheats are enabled. Keep this off unless the server is intentionally admin-only or experimental."
      );
    }

    return undefined;
  }
};
