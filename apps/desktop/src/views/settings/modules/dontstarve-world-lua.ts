import dontStarveSchema from "../../../../../../modules/dontstarve/schema.json";
import type { SettingsObject } from "../settings-schema";
import { luaChildTable, luaScalar, parseLuaDataTable, patchLuaDataTable } from "./dontstarve-lua-data";
import type { LuaScalar, LuaTable } from "./dontstarve-lua-data";

type Shard = "master" | "caves";
type SchemaProperty = {
  default?: unknown;
  type?: string;
  enum?: readonly unknown[];
  "x-lsgm-section"?: string;
  "x-lsgm-source-key"?: string;
};
type WorldField = { key: string; nativeKey: string; override: boolean; property: SchemaProperty };
type WorldSource = {
  raw: boolean;
  text: string;
  root: LuaTable | null;
  overrides: LuaTable | null;
  script: boolean;
};

const PROPERTIES: Record<string, SchemaProperty> = dontStarveSchema.properties;
const SHARDS: readonly Shard[] = ["master", "caves"];
const SOURCE_CACHE = new WeakMap<Readonly<SettingsObject>, Map<Shard, WorldSource>>();
const FIELDS = new Map<Shard, WorldField[]>(SHARDS.map((shard) => [shard,
  Object.entries(PROPERTIES).flatMap<WorldField>(([key, property]) => {
    if (![`${shard}gen`, `${shard}settings`].includes(property["x-lsgm-section"] ?? "")) return [];
    const source = property["x-lsgm-source-key"] ?? "";
    if (source.startsWith("overrides.")) {
      return [{ key, nativeKey: source.slice("overrides.".length), override: true, property }];
    }
    return ["settings_preset", "worldgen_preset"].includes(source)
      ? [{ key, nativeKey: source, override: false, property }]
      : [];
  })
]));

function textSetting(settings: Readonly<SettingsObject>, key: string): string {
  return typeof settings[key] === "string" ? settings[key] : "";
}

function activeRaw(settings: Readonly<SettingsObject>, shard: Shard): string | null {
  const key = `${shard}_worldgenoverride_lua`;
  const raw = textSetting(settings, key);
  const baseline = PROPERTIES[key].default;
  const normalized = raw.replace(/\r\n|\r/g, "\n").trim();
  return normalized && normalized !== (typeof baseline === "string" ? baseline.replace(/\r\n|\r/g, "\n").trim() : "") ? raw : null;
}

function scalar(value: unknown): value is LuaScalar {
  return value === null || typeof value === "string" || typeof value === "boolean" ||
    (typeof value === "number" && Number.isFinite(value));
}

function accepts(field: WorldField, value: LuaScalar): boolean {
  if (value === null) return true;
  if (!field.override && (typeof value !== "string" || value.length > 128 || !/^[A-Za-z0-9_][A-Za-z0-9_.-]*$/.test(value))) return false;
  const type = field.property.type;
  return (type === "integer" ? typeof value === "number" && Number.isInteger(value) : typeof value === type) &&
    (!field.property.enum || field.property.enum.includes(value));
}

function nativeValue(source: WorldSource, field: WorldField): LuaScalar | undefined {
  if (field.override) return luaScalar(source.overrides, field.nativeKey);
  if (!source.raw) return undefined;
  return luaScalar(source.root, field.nativeKey) ?? luaScalar(source.root, "preset");
}

function worldSource(settings: Readonly<SettingsObject>, shard: Shard): WorldSource {
  const cached = SOURCE_CACHE.get(settings)?.get(shard);
  if (cached) return cached;
  const raw = activeRaw(settings, shard);
  const text = raw ?? textSetting(settings, `${shard}_world_overrides_extra`);
  const root = parseLuaDataTable(text, raw === null);
  const overrides = raw !== null && root ? luaChildTable(root, "overrides") : root;
  const overrideValue = root?.entries.get("overrides")?.value;
  let script = !root || (raw !== null && (
    luaScalar(root, "override_enabled") !== true ||
    (overrideValue !== undefined && overrideValue.kind !== "table" && overrideValue.value !== null)
  ));
  const source = { raw: raw !== null, text, root, overrides, script };
  for (const field of FIELDS.get(shard) ?? []) {
    const owner = field.override ? overrides : raw !== null ? root : null;
    const node = owner?.entries.get(field.nativeKey)?.value;
    const fallback = !field.override && raw !== null ? root?.entries.get("preset")?.value : undefined;
    const value = nativeValue(source, field);
    const usesFallback = !node || (node.kind === "scalar" && node.value === null);
    if (node?.kind === "table" || (usesFallback && fallback?.kind === "table") ||
        (value !== undefined && !accepts(field, value))) script = true;
  }
  const result = { ...source, script };
  const cache = SOURCE_CACHE.get(settings) ?? new Map<Shard, WorldSource>();
  cache.set(shard, result);
  SOURCE_CACHE.set(settings, cache);
  return result;
}

function projectShard(settings: SettingsObject, shard: Shard): void {
  const source = worldSource(settings, shard);
  for (const field of FIELDS.get(shard) ?? []) {
    // Absent raw keys inherit the selected game preset. Schema defaults are a
    // form placeholder, never additional entries written into that raw table.
    if (source.raw) settings[field.key] = field.property.default;
    const value = nativeValue(source, field);
    // Nil extra entries are filled by the generated typed value, not a reset.
    if (!source.raw && value === null) continue;
    const owner = field.override ? source.overrides : source.raw ? source.root : null;
    if (owner?.entries.has(field.nativeKey) || value !== undefined) {
      settings[field.key] = value !== undefined && value !== null && accepts(field, value)
        ? value : field.property.default;
    }
  }
}

export function initializeDontStarveWorldSettings(settings: Readonly<SettingsObject>): SettingsObject {
  const initialized = { ...settings };
  for (const shard of SHARDS) projectShard(initialized, shard);
  return initialized;
}

export function getDontStarveWorldScriptMode(settings: Readonly<SettingsObject>, shard: Shard): boolean {
  return worldSource(settings, shard).script;
}

function nonDefaultPreset(settings: Readonly<SettingsObject>, shard: Shard): boolean {
  return (FIELDS.get(shard) ?? []).some((field) => !field.override &&
    (settings[field.key] ?? field.property.default) !== field.property.default);
}

function applyGuidedChanges(
  current: Readonly<SettingsObject>,
  next: SettingsObject,
  patch: Readonly<SettingsObject>,
  shard: Shard
): void {
  const rawKey = `${shard}_worldgenoverride_lua`;
  const extraKey = `${shard}_world_overrides_extra`;
  const source = worldSource(next, shard);
  // Direct source edits are authoritative in mixed patches, matching storage.
  if (source.script || (Object.prototype.hasOwnProperty.call(patch, rawKey) && current[rawKey] !== next[rawKey]) ||
      (!source.raw && Object.prototype.hasOwnProperty.call(patch, extraKey) && current[extraKey] !== next[extraKey])) return;
  const extraText = textSetting(next, extraKey);
  const extra = parseLuaDataTable(extraText, true);
  const extraChanges = new Map<string, LuaScalar>();
  const overrides = new Map<string, LuaScalar>();
  const topLevel = new Map<string, LuaScalar | Map<string, LuaScalar>>();
  for (const field of FIELDS.get(shard) ?? []) {
    const value = patch[field.key];
    if (!Object.prototype.hasOwnProperty.call(patch, field.key) || !scalar(value)) continue;
    if (field.override) {
      if (source.raw) overrides.set(field.nativeKey, value);
      if (extra?.entries.has(field.nativeKey) || (!source.raw && nonDefaultPreset(next, shard) && value === field.property.default)) {
        extraChanges.set(field.nativeKey, value);
      }
    } else if (source.raw) topLevel.set(field.nativeKey, value);
  }
  if (source.raw && source.root) {
    if (overrides.size) topLevel.set("overrides", overrides);
    if (topLevel.size) next[rawKey] = patchLuaDataTable(source.text, source.root, topLevel);
  }
  if (extra && extraChanges.size) next[extraKey] = patchLuaDataTable(extraText, extra, extraChanges);
}

export function applyDontStarveWorldSettingsPatch(
  current: Readonly<SettingsObject>,
  patch: Readonly<SettingsObject>
): SettingsObject {
  const initialized = initializeDontStarveWorldSettings(current);
  const next = { ...initialized, ...patch };
  for (const shard of SHARDS) applyGuidedChanges(initialized, next, patch, shard);
  return initializeDontStarveWorldSettings(next);
}
