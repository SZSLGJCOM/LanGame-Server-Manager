import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type { GuidedFieldCopy, GuidedSettingsField } from "../settings-schema";
import type { TranslateFn } from "../../../i18n";
import { DontStarveCavesNotice, DontStarvePresetNotice } from "./dontstarve-preset-notice";
import { buildDontStarveWorldFieldGroups } from "./dontstarve-world-groups";
import {
  applyDontStarveWorldSettingsPatch,
  getDontStarveWorldScriptMode,
  initializeDontStarveWorldSettings
} from "./dontstarve-world-lua";
import {
  applyDontStarveOperationalPatch,
  getDontStarveSettingsValidationIssues,
  isCustomRawValue
} from "./dontstarve-validation";
import { DONTSTARVE_SHARDS, isDontStarveShardActive } from "./dontstarve-shards";

const DST_WORKSHOP_LIST_FIELDS = new Set([
  "shared_workshop_mod_ids",
  "shared_workshop_collection_ids",
  ...DONTSTARVE_SHARDS.map((shard) => `${shard}_enabled_workshop_mod_ids`)
]);

const DST_ENUM_OPTION_COPY: Record<string, { key: string; fallback: string }> = {
  SURVIVAL_TOGETHER: { key: "dst.settings.presets.survival", fallback: "Survival" },
  RELAXED: { key: "dst.settings.presets.relaxed", fallback: "Relaxed" },
  ENDLESS: { key: "dst.settings.presets.endless", fallback: "Endless" },
  WILDERNESS: { key: "dst.settings.presets.wilderness", fallback: "Wilderness" },
  LIGHTS_OUT: { key: "dst.settings.presets.lightsOut", fallback: "Lights Out" },
  DST_CAVE: { key: "dst.settings.presets.caves", fallback: "Caves" },
  "supplies.always": { key: "dst.settings.option.supplies.always", fallback: "Always" },
  "supplies.day5": { key: "dst.settings.option.supplies.day5", fallback: "After day 5" },
  "supplies.day10": { key: "dst.settings.option.supplies.day10", fallback: "After day 10 (default)" },
  "supplies.day15": { key: "dst.settings.option.supplies.day15", fallback: "After day 15" },
  "supplies.day20": { key: "dst.settings.option.supplies.day20", fallback: "After day 20" },
  "supplies.never": { key: "dst.settings.option.supplies.never", fallback: "Never" },
  "death.changeSurvivor": { key: "dst.settings.option.death.changeSurvivor", fallback: "Choose another survivor" },
  "death.ghost": { key: "dst.settings.option.death.ghost", fallback: "Become a ghost" },
  "reset.disabled": { key: "dst.settings.option.reset.disabled", fallback: "Keep the world" },
  "reset.slow": { key: "dst.settings.option.reset.slow", fallback: "Long countdown" },
  "reset.default": { key: "dst.settings.option.reset.default", fallback: "Default countdown" },
  "reset.fast": { key: "dst.settings.option.reset.fast", fallback: "Short countdown" },
  "reset.instant": { key: "dst.settings.option.reset.instant", fallback: "Reset immediately" },
  "damage.less": { key: "dst.settings.option.damage.less", fallback: "Less damage" },
  "damage.default": { key: "dst.settings.option.damage.default", fallback: "Default damage" },
  "damage.more": { key: "dst.settings.option.damage.more", fallback: "More damage" },
  "spawn.portal": { key: "dst.settings.option.spawn.portal", fallback: "Florid Postern" },
  "spawn.random": { key: "dst.settings.option.spawn.random", fallback: "Random location" },
  "rifts.progression": { key: "dst.settings.option.rifts.progression", fallback: "Follow game progression" },
  "rifts.always": { key: "dst.settings.option.rifts.always", fallback: "No progression requirement" },
  caves: { key: "dst.settings.option.caves", fallback: "Caves" },
  cave_default: { key: "dst.settings.option.caveBiomes", fallback: "Cave biomes" },
  disabled: { key: "dst.settings.option.disabled", fallback: "Disabled" },
  always: { key: "dst.settings.option.always", fallback: "Always" },
  autumn: { key: "dst.settings.option.autumn", fallback: "Autumn" },
  "autumn|spring": { key: "dst.settings.option.autumnOrSpring", fallback: "Autumn or Spring" },
  "autumn|winter|spring|summer": { key: "dst.settings.option.anySeason", fallback: "Any Season" },
  classic: { key: "dst.settings.option.classic", fallback: "Classic" },
  competitive: { key: "dst.settings.option.competitive", fallback: "Competitive" },
  cooperative: { key: "dst.settings.option.cooperative", fallback: "Cooperative" },
  crow_carnival: { key: "dst.settings.option.crowCarnival", fallback: "Crow Carnival" },
  darkness: { key: "dst.settings.option.darkness", fallback: "Darkness" },
  default: { key: "dst.settings.option.default", fallback: "Default" },
  enabled: { key: "dst.settings.option.enabled", fallback: "Enabled" },
  endless: { key: "dst.settings.option.endless", fallback: "Endless" },
  fast: { key: "dst.settings.option.fast", fallback: "Fast" },
  few: { key: "dst.settings.option.few", fallback: "Few" },
  hallowed_nights: { key: "dst.settings.option.hallowedNights", fallback: "Hallowed Nights" },
  "highly random": { key: "dst.settings.option.highlyRandom", fallback: "Highly Random" },
  huge: { key: "dst.settings.option.huge", fallback: "Huge" },
  insane: { key: "dst.settings.option.insane", fallback: "Insane" },
  least: { key: "dst.settings.option.least", fallback: "Least" },
  longday: { key: "dst.settings.option.longDay", fallback: "Long Day" },
  longdusk: { key: "dst.settings.option.longDusk", fallback: "Long Dusk" },
  longnight: { key: "dst.settings.option.longNight", fallback: "Long Night" },
  longseason: { key: "dst.settings.option.longSeason", fallback: "Long" },
  madness: { key: "dst.settings.option.madness", fallback: "Madness" },
  many: { key: "dst.settings.option.many", fallback: "Many" },
  max: { key: "dst.settings.option.max", fallback: "Max" },
  medium: { key: "dst.settings.option.medium", fallback: "Medium" },
  most: { key: "dst.settings.option.most", fallback: "Most" },
  mostly: { key: "dst.settings.option.mostly", fallback: "Mostly" },
  never: { key: "dst.settings.option.never", fallback: "Never" },
  noday: { key: "dst.settings.option.noDay", fallback: "No Day" },
  nodusk: { key: "dst.settings.option.noDusk", fallback: "No Dusk" },
  none: { key: "dst.settings.option.none", fallback: "None" },
  nonight: { key: "dst.settings.option.noNight", fallback: "No Night" },
  nonlethal: { key: "dst.settings.option.nonLethal", fallback: "Nonlethal" },
  noseason: { key: "dst.settings.option.noSeason", fallback: "Disabled" },
  ocean_always: { key: "dst.settings.option.oceanAlways", fallback: "Always" },
  ocean_default: { key: "dst.settings.option.oceanDefault", fallback: "Default" },
  ocean_insane: { key: "dst.settings.option.oceanInsane", fallback: "Insane" },
  ocean_mostly: { key: "dst.settings.option.oceanMostly", fallback: "Mostly" },
  ocean_never: { key: "dst.settings.option.oceanNever", fallback: "Never" },
  ocean_often: { key: "dst.settings.option.oceanOften", fallback: "Often" },
  ocean_rare: { key: "dst.settings.option.oceanRare", fallback: "Rare" },
  ocean_uncommon: { key: "dst.settings.option.oceanUncommon", fallback: "Uncommon" },
  often: { key: "dst.settings.option.often", fallback: "Often" },
  onlyday: { key: "dst.settings.option.onlyDay", fallback: "Only Day" },
  onlydusk: { key: "dst.settings.option.onlyDusk", fallback: "Only Dusk" },
  onlynight: { key: "dst.settings.option.onlyNight", fallback: "Only Night" },
  plus: { key: "dst.settings.option.plus", fallback: "Plus" },
  random: { key: "dst.settings.option.random", fallback: "Random" },
  rare: { key: "dst.settings.option.rare", fallback: "Rare" },
  shortseason: { key: "dst.settings.option.shortSeason", fallback: "Short" },
  slow: { key: "dst.settings.option.slow", fallback: "Slow" },
  small: { key: "dst.settings.option.small", fallback: "Small" },
  social: { key: "dst.settings.option.social", fallback: "Social" },
  spring: { key: "dst.settings.option.spring", fallback: "Spring" },
  summer: { key: "dst.settings.option.summer", fallback: "Summer" },
  survival: { key: "dst.settings.option.survival", fallback: "Survival" },
  uncommon: { key: "dst.settings.option.uncommon", fallback: "Uncommon" },
  veryfast: { key: "dst.settings.option.veryFast", fallback: "Very Fast" },
  verylongseason: { key: "dst.settings.option.veryLongSeason", fallback: "Very Long" },
  veryshortseason: { key: "dst.settings.option.veryShortSeason", fallback: "Very Short" },
  veryslow: { key: "dst.settings.option.verySlow", fallback: "Very Slow" },
  wilderness: { key: "dst.settings.option.wilderness", fallback: "Wilderness" },
  winter: { key: "dst.settings.option.winter", fallback: "Winter" },
  "winter|summer": { key: "dst.settings.option.winterOrSummer", fallback: "Winter or Summer" },
  winters_feast: { key: "dst.settings.option.wintersFeast", fallback: "Winter's Feast" }
};

const DST_ENABLED_ENUM_KEYS = { none: "disabled", always: "enabled" };
const DST_RIFT_ENUM_KEYS = { never: "disabled", default: "rifts.progression", always: "rifts.always" };

// Several native values have different meanings across world settings.
const DST_FIELD_ENUM_KEYS: Record<string, Record<string, string>> = {
  world_extrastartingitems: {
    "0": "supplies.always", "5": "supplies.day5", default: "supplies.day10",
    "15": "supplies.day15", "20": "supplies.day20", none: "supplies.never"
  },
  world_ghostenabled: { none: "death.changeSurvivor", always: "death.ghost" },
  world_ghostsanitydrain: DST_ENABLED_ENUM_KEYS,
  world_portalresurection: DST_ENABLED_ENUM_KEYS,
  master_wanderingtrader_enabled: DST_ENABLED_ENUM_KEYS,
  world_basicresource_regrowth: DST_ENABLED_ENUM_KEYS,
  caves_acidrain_enabled: DST_ENABLED_ENUM_KEYS,
  world_resettime: {
    none: "reset.disabled", slow: "reset.slow", default: "reset.default",
    fast: "reset.fast", always: "reset.instant"
  },
  world_lessdamagetaken: { always: "damage.less", none: "damage.default", more: "damage.more" },
  world_spawnmode: { fixed: "spawn.portal", scatter: "spawn.random" },
  master_rifts_enabled: DST_RIFT_ENUM_KEYS,
  caves_rifts_enabled: DST_RIFT_ENUM_KEYS
};

const GROUP_COPY: Record<string, { title: string; description: string }> = {
  presets: { title: "Preset", description: "Choose the base rules or terrain for this shard." },
  credentials: { title: "Server authentication", description: "Klei server authentication token." },
  "join-access": { title: "Admission policy", description: "Friends-only access, reserved seats, and Lua file permissions." },
  "steam-group": { title: "Steam group access", description: "Group membership and administrator permission rules." },
  network: { title: "Connections", description: "Offline connectivity, shard binding, and data collection." },
  shards: { title: "Shard coordination", description: "Choose the standard or Island Adventures layout for the next server start." },
  rules: { title: "Cluster rules", description: "Game mode, PvP, and voting shared by every shard." },
  runtime: { title: "Cluster operation", description: "Pause behavior, tick rate, automatic saves, and snapshot retention." },
  other: { title: "Other settings", description: "" },
  downloads: { title: "Mod downloads", description: "Choose Workshop mods and collections to download for this instance." },
  mastermods: { title: "Master Mods", description: "Master enablement and configuration_options." },
  cavesmods: { title: "Caves Mods", description: "Caves enablement and configuration_options." },
  islandsmods: { title: "Islands Mods", description: "Islands enablement and configuration_options." },
  volcanomods: { title: "Volcano Mods", description: "Volcano enablement and configuration_options." },
  rawworld: { title: "Raw world files", description: "Expert full-file and extra Lua overrides." },
  rawmods: { title: "Raw Mod files", description: "Expert full-file modoverrides.lua." },
  launch: { title: "Launch options", description: "Official dedicated-server command-line switches." }
};

function groupId(sectionId: string, key: string): string {
  if (key.endsWith("_preset")) return "presets";
  if (sectionId === "access") return key === "cluster_token" ? "credentials"
    : key.startsWith("steam_group_") ? "steam-group" : "join-access";
  if (sectionId === "network") return "network";
  if (sectionId === "cluster-rules") return "rules";
  if (sectionId === "cluster-runtime") return "runtime";
  if (sectionId === "cluster-shard-coordination") return "shards";
  if (sectionId === "mods") {
    if (key.startsWith("shared_")) return "downloads";
    return `${key.split("_")[0]}mods`;
  }
  if (sectionId === "advanced") {
    if (key.includes("world")) return "rawworld";
    return key.includes("modoverrides") ? "rawmods" : "launch";
  }

  return "other";
}

function buildDstFieldGroups(
  sectionId: string,
  fields: GuidedSettingsField[],
  locale: string,
  t: TranslateFn
): SettingsModuleFieldGroup[] {
  const worldGroups = buildDontStarveWorldFieldGroups(sectionId, fields, locale, t);
  if (worldGroups) return worldGroups;
  if (sectionId === "room") return [{ id: "room", fields }];
  const grouped = new Map<string, GuidedSettingsField[]>();
  for (const field of fields) {
    const id = groupId(sectionId, field.key);
    grouped.set(id, [...(grouped.get(id) ?? []), field]);
  }
  return Array.from(grouped, ([id, groupFields]) => {
    const copy = GROUP_COPY[id];
    return {
      id,
      title: t(`dst.settings.fieldGroups.${sectionId}.${id}.title`, undefined, copy.title),
      description: t(`dst.settings.fieldGroups.${sectionId}.${id}.description`, undefined, copy.description),
      layoutClass: `dst-${sectionId}-${id}`,
      fields: groupFields
    };
  });
}

function readCatalogText(t: TranslateFn, key: string): string | undefined {
  const translated = t(key, undefined, "");
  return translated.trim().length > 0 ? translated : undefined;
}

function buildDstFieldCopy(key: string, t: TranslateFn): GuidedFieldCopy | undefined {
  const schemaBaseKey = `settings.schema.dontstarve.${key}`;
  const title = readCatalogText(t, `${schemaBaseKey}.title`);
  const description = readCatalogText(t, `${schemaBaseKey}.description`);
  return title || description ? { title: title ?? key, description } : undefined;
}

function translateDstEnumOption(fieldKey: string, value: unknown, t: TranslateFn): string | undefined {
  if (typeof value !== "string") return undefined;
  if (fieldKey === "shard_layout") {
    return value === "island_adventures"
      ? t("dst.settings.layout.islandAdventures", undefined, "Island Adventures · four shards")
      : value === "standard" ? t("dst.settings.layout.standard", undefined, "Standard · Master and optional Caves") : undefined;
  }
  const entry = DST_ENUM_OPTION_COPY[DST_FIELD_ENUM_KEYS[fieldKey]?.[value] ?? value];
  return entry ? t(entry.key, undefined, entry.fallback) : undefined;
}

export const dontStarveSettingsDefinition: SettingsModuleDefinition = {
  id: "dontstarve",
  specializedRenderers: {
    "dst-mastergen-preset-notice": {
      kind: "module-addon",
      placement: "before-fields",
      sectionId: "mastergen",
      Renderer: DontStarvePresetNotice
    },
    "dst-mastersettings-preset-notice": {
      kind: "module-addon",
      placement: "before-fields",
      sectionId: "mastersettings",
      Renderer: DontStarvePresetNotice
    },
    "dst-cavesgen-preset-notice": {
      kind: "module-addon",
      placement: "before-fields",
      sectionId: "cavesgen",
      Renderer: DontStarvePresetNotice
    },
    "dst-cavessettings-preset-notice": {
      kind: "module-addon",
      placement: "before-fields",
      sectionId: "cavessettings",
      Renderer: DontStarvePresetNotice
    },
    "dst-caves-advanced-notice": {
      kind: "module-addon",
      placement: "before-fields",
      sectionId: "advanced",
      Renderer: DontStarveCavesNotice
    }
  },
  getSections: (t) => [
    { id: "room", title: t("dst.settings.sections.room", undefined, "Room Settings"),
      description: t("dst.settings.sections.roomDescription", undefined,
        "Room name, visibility, join gate, admins, whitelist, and blocklist controls.") },
    { id: "cluster", title: t("dst.settings.sections.cluster", undefined, "Cluster"), order: 10,
      description: t("dst.settings.sections.clusterDescription", undefined,
        "Set shared rules and choose the standard or Island Adventures shard layout.") },
    { id: "cluster-rules", parentId: "cluster", title: t("dst.settings.sections.clusterRules", undefined, "Cluster rules"), order: 10,
      description: t("dst.settings.sections.clusterRulesDescription", undefined,
        "Set the game mode, player-versus-player damage, and voting rules shared by every shard.") },
    { id: "cluster-shard-coordination", parentId: "cluster", title: t("dst.settings.sections.clusterShardCoordination", undefined, "Shard coordination"), order: 20,
      description: t("dst.settings.sections.clusterShardCoordinationDescription", undefined,
        "Choose Master with optional Caves, or Island Adventures with Master, Caves, Islands and Volcano. Layout changes apply on the next start and preserve saved shard settings.") },
    { id: "cluster-runtime", parentId: "runtime", title: t("dst.settings.sections.clusterRuntime", undefined, "Cluster operation"), order: 10,
      description: t("dst.settings.sections.clusterRuntimeDescription", undefined,
        "Configure pausing when empty, tick rate, and the interval and retention count for server log backups.") },
    { id: "surface", title: t("dst.settings.sections.surface", undefined, "Surface / Master"), order: 20,
      description: t("dst.settings.sections.surfaceDescription", undefined,
        "Configure new-world generation and the environment and survival rules for the surface shard.") },
    { id: "mastergen", parentId: "surface", title: t("dst.settings.sections.mastergen", undefined, "World generation"), order: 10,
      description: t("dst.settings.sections.mastergenDescription", undefined,
        "Match the original overworld generation flow for map layout, resource density, creature spread, and import setup.") },
    { id: "mastersettings", parentId: "surface", title: t("dst.settings.sections.mastersettings", undefined, "World settings"), order: 20,
      description: t("dst.settings.sections.mastersettingsDescription", undefined,
        "Match the original overworld settings flow for events, seasons, survivor rules, hostile pressure, giants, and regrowth.") },
    { id: "caves", title: t("dst.settings.sections.caves", undefined, "Caves"), order: 30,
      description: t("dst.settings.sections.cavesDescription", undefined,
        "Configure new-world generation and the environment and survival rules for the Caves shard.") },
    { id: "cavesgen", parentId: "caves", title: t("dst.settings.sections.cavesgen", undefined, "World generation"), order: 10,
      description: t("dst.settings.sections.cavesgenDescription", undefined,
        "Configure cave map layout, resource density, and creature spread using the original world-generation options.") },
    { id: "cavessettings", parentId: "caves", title: t("dst.settings.sections.cavessettings", undefined, "World settings"), order: 20,
      description: t("dst.settings.sections.cavessettingsDescription", undefined,
        "Match the world-settings side of the original Caves tab for day type, earthquakes, hostile pressure, giants, and regrowth.") },
    { id: "mods", title: t("dst.settings.sections.mods", undefined, "Server mods"), order: 40,
      description: t("dst.settings.sections.modsDescription", undefined,
        "Manage this instance's downloads, shard enable lists, and per-Mod options.") },
    { id: "advanced", parentId: "runtime", title: t("dst.settings.sections.advanced", undefined, "Advanced native overrides"), order: 50,
      description: t("dst.settings.sections.advancedDescription", undefined,
        "Edit world and Mod Lua files for every shard, including Island Adventures Islands and Volcano.") }
  ],
  buildFieldGroups: (sectionId, fields, locale, t) => buildDstFieldGroups(sectionId, fields, locale, t),
  getFieldCopy: (key, t) => buildDstFieldCopy(key, t),
  getEnumOptionLabel: (key, value, _locale, t) => translateDstEnumOption(key, value, t),
  resolveFieldEditorVariant: (key) => DST_WORKSHOP_LIST_FIELDS.has(key) ? "workshop-id-list" : undefined,
  initializeSettings: (settings) => initializeDontStarveWorldSettings(settings),
  applySettingsPatch: (settings, patch) => applyDontStarveWorldSettingsPatch(
    settings,
    applyDontStarveOperationalPatch(settings, patch)
  ),
  isFieldDisabled: (field, settings) => {
    const fieldShard = DONTSTARVE_SHARDS.find((shard) => field.key.startsWith(`${shard}_`));
    if (fieldShard && !isDontStarveShardActive(settings, fieldShard)) return true;
    if (field.key === "enable_caves" && settings.shard_layout === "island_adventures") return true;
    if (field.key.endsWith("_world_overrides_extra") &&
      isCustomRawValue(settings, field.key.replace("_world_overrides_extra", "_worldgenoverride_lua"))) return true;
    const shard = field.sectionId === "mastergen" || field.sectionId === "mastersettings" ? "master"
      : field.sectionId === "cavesgen" || field.sectionId === "cavessettings" ? "caves" : null;
    return shard !== null && getDontStarveWorldScriptMode(settings, shard);
  },
  getSettingsValidationIssues: (settings, context) =>
    getDontStarveSettingsValidationIssues(settings, context.t)
};
