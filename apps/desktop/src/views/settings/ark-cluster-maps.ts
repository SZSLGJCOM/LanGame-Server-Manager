import type { GuidedSettingsValidationIssue, SettingsObject } from "./settings-schema";
import type { TranslateFn } from "../../i18n";
import { ARK_WORKSPACE_EN } from "../../i18n/games/ark-workspace.en";
import { isArkModule } from "../../ark-clusters";
import type { ModulePortGroupDetails } from "../../types";
import { ASA_MEMBERSHIP_KEYS, ASA_RAW_MOD_WARNING, AsaModControlError, buildAsaModPlan, readAsaModMembership } from "../servers/mod-workbench-asa";

export const ARK_ADDITIONAL_MAP_LIMIT = 15;
export interface AdditionalArkMap {
  id: string;
  map_name: string;
  name: string;
  enabled: boolean;
}
export interface ArkMapSuggestion { value: string; label: string; modId?: string }
export interface ArkMapAvailabilityView {
  status: "loading" | "ready" | "failed";
  choices: readonly ArkMapSuggestion[];
  error?: Error | null;
  refresh: () => void;
}
export type ArkMapValidationReason = "shape" | "limit" | "id" | "duplicate" | "map" | "name";

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

export function readAdditionalArkMaps(settings: Readonly<SettingsObject>): AdditionalArkMap[] | null {
  const entries = settings.additional_maps;
  if (entries === undefined) return [];
  if (!Array.isArray(entries) || entries.some((entry) => !isRecord(entry)
    || Object.keys(entry).some((key) => !["id", "map_name", "name", "enabled"].includes(key))
    || typeof entry.id !== "string" || typeof entry.map_name !== "string"
    || typeof entry.name !== "string" || typeof entry.enabled !== "boolean")) return null;
  return entries.map((entry) => ({ id: entry.id, map_name: entry.map_name, name: entry.name, enabled: entry.enabled }));
}

export function validateAdditionalArkMaps(settings: Readonly<SettingsObject>): ArkMapValidationReason | null {
  const maps = readAdditionalArkMaps(settings);
  if (!maps) return "shape";
  if (maps.length > ARK_ADDITIONAL_MAP_LIMIT) return "limit";
  const ids = new Set<string>();
  for (const map of maps) {
    if (!/^[a-z0-9][a-z0-9-]{0,31}$/.test(map.id)) return "id";
    if (ids.has(map.id)) return "duplicate";
    ids.add(map.id);
    if (!/^[A-Za-z0-9_]{1,128}$/.test(map.map_name)) return "map";
    if (!map.name.trim()
      || /[\u0000-\u001f\u007f-\u009f?\u2028\u2029]/.test(map.name)) return "name";
  }
  return null;
}

export function arkMapsText(t: TranslateFn, key: string, values?: Record<string, string | number>): string {
  const catalogKey = `ark.maps.${key}`;
  return t(catalogKey, values, ARK_WORKSPACE_EN[catalogKey] ?? key);
}

export function getArkMapSettingsValidationIssues(settings: Readonly<SettingsObject>, t: TranslateFn): GuidedSettingsValidationIssue[] {
  const reason = validateAdditionalArkMaps(settings);
  return reason ? [{ fieldKey: "additional_maps", reason: "module", message: arkMapsText(t, `error.${reason}`) }] : [];
}

export function buildArkMapPortGroups(moduleId: string, settingsJson: string, base: ModulePortGroupDetails[]): ModulePortGroupDetails[] {
  if (!isArkModule(moduleId) || base.length === 0) return base;
  let settings: SettingsObject;
  try { settings = JSON.parse(settingsJson) as SettingsObject; } catch { return base; }
  if (!isRecord(settings) || validateAdditionalArkMaps(settings)) return base;
  return [...base, ...readAdditionalArkMaps(settings)!.flatMap((map) => base.map((group) => {
    const key = `map-${map.id}`;
    return { ...group, id: `${key}-${group.id}`, members: group.members.map((name) => `${key}-${name}`),
      member_offsets: group.member_offsets ? Object.fromEntries(Object.entries(group.member_offsets).map(([name, offset]) => [`${key}-${name}`, offset])) : undefined };
  }))];
}

export function readArkMapSuggestions(schemaJson: string | null | undefined, locale: string): ArkMapSuggestion[] {
  let schema: unknown;
  try { schema = JSON.parse(schemaJson ?? "{}"); } catch { return []; }
  if (!isRecord(schema) || !isRecord(schema.properties) || !isRecord(schema.properties.map_name)) return [];
  const suggestions = schema.properties.map_name["x-lsgm-suggestions"];
  if (!Array.isArray(suggestions)) return [];
  return suggestions.flatMap((suggestion) => {
    if (!isRecord(suggestion) || typeof suggestion.value !== "string" || !/^[A-Za-z0-9_]{1,128}$/.test(suggestion.value)) return [];
    const localized = locale === "zh-CN" ? suggestion["x-lsgm-label-zh-CN"] : suggestion.label;
    return [{ value: suggestion.value, label: typeof localized === "string" ? localized : suggestion.value }];
  });
}

/** Installed packages augment the edition's official catalog without inventing identifiers from display names. */
export function mergeArkMapSuggestions(official: readonly ArkMapSuggestion[], installed: readonly ArkMapSuggestion[]): ArkMapSuggestion[] {
  const choices = new Map(official.map((map) => [map.value, map]));
  for (const map of installed) {
    if (/^[A-Za-z0-9_]{1,128}$/.test(map.value) && !choices.has(map.value)) choices.set(map.value, map);
  }
  return [...choices.values()];
}

/** A deliberate map choice enables its discovered Mod in the same settings mutation. Other edits leave membership alone. */
export function buildArkMapSelectionPatch(settings: Readonly<SettingsObject>, mapName: string,
  choices: readonly ArkMapSuggestion[], mapPatch: Readonly<SettingsObject>): SettingsObject {
  const id = choices.find((choice) => choice.value === mapName)?.modId;
  if (!id) return { ...mapPatch };
  const added = buildAsaModPlan(settings, [id], "add", [id]);
  let next = added;
  if (readAsaModMembership(added).passive.includes(id)) {
    // The shared actions remove passive membership; keep the active list's existing order.
    next = buildAsaModPlan(buildAsaModPlan(added, [id], "disable", [id]), [id], "add", [id]);
    next.mod_ids_csv = added.mod_ids_csv;
  }
  const patch = { ...mapPatch };
  for (const key of ASA_MEMBERSHIP_KEYS) {
    if (JSON.stringify(next[key]) !== JSON.stringify(settings[key])) patch[key] = next[key];
  }
  return patch;
}

export function arkMapSelectionErrorMessage(cause: unknown, t: TranslateFn): string {
  return cause instanceof AsaModControlError && cause.code === "raw-mod-flags"
    ? t("servers.mods.asaRawFlagsWarning", undefined, ASA_RAW_MOD_WARNING)
    : arkMapsText(t, "enableMapFailed");
}

/** A new identity never reuses the directory of a removed map. */
export function newArkMapId(existingIds: readonly string[], generateId = () => crypto.randomUUID()): string {
  const existing = new Set(existingIds);
  for (let attempt = 0; attempt < 10; attempt += 1) {
    const id = `world-${generateId().replace(/-/g, "").slice(0, 16).toLowerCase()}`;
    if (/^[a-z0-9][a-z0-9-]{0,31}$/.test(id) && !existing.has(id)) return id;
  }
  throw new Error("Could not allocate a unique ARK map identity.");
}
