import type { SettingsObject } from "../settings/settings-schema";
import { parseEvolvedModIds } from "./ark-evolved-mod-ids";
import { buildAsaModPlan, canonicalAsaModId, hasAsaRawModFlags, readAsaModMembership, type AsaModAction } from "./mod-workbench-asa";
import { parseAsaModIni, readAsaModConfigurations, type AsaModIniFile } from "./asa-mod-configuration";

export const ARK_MOD_MAP_OVERRIDES = "ark_mod_map_overrides";
export interface ArkModScope { key: string; name: string; map: string; enabled: boolean }
export interface ArkModOverride {
  active?: boolean; passive?: boolean; disabled_active?: boolean; disabled_passive?: boolean; removed?: boolean;
  configuration?: { game_ini?: string; game_user_settings_ini?: string } | null;
}
export interface ArkModMapOverride { mods: Record<string, ArkModOverride>; active_order?: string[]; passive_order?: string[] }
export type ArkModMapOverrides = Record<string, ArkModMapOverride>;
export type ArkModChange = { modId: string } & (
  | { kind: "membership"; maps: string[] }
  | { kind: "inherit-membership" }
  | { kind: "configuration"; maps: string[] | null; file: AsaModIniFile; text: string }
  | { kind: "inherit-configuration"; maps: string[] }
);
export class ArkModScopeError extends Error {
  constructor(readonly code: "shape" | "scope" | "changed" | "raw-flags") { super(`ark-mod-scope-${code}`); }
}
const record = (value: unknown): value is SettingsObject => value !== null && typeof value === "object" && !Array.isArray(value);
const unique = (ids: readonly string[]) => [...new Set(ids)];
const membershipKeys = ["active", "passive", "disabled_active", "disabled_passive", "removed"] as const;
export function arkModFields(moduleId: string): readonly string[] {
  return moduleId === "arksurvivalascended" ? ["mod_ids_csv", "passive_mod_ids_csv", "asa_mod_configurations"]
    : moduleId === "arksurvivalevolved" ? ["active_mod_ids"] : [];
}
export function readArkModScopes(settings: Readonly<SettingsObject>, fallbackName: string): ArkModScope[] {
  const additional = Array.isArray(settings.additional_maps) ? settings.additional_maps : [];
  return [{ key: "main", name: String(settings.server_name || fallbackName), map: String(settings.map_name ?? ""), enabled: true },
    ...additional.flatMap((map) => record(map) && typeof map.id === "string" && typeof map.name === "string"
      && typeof map.map_name === "string" && typeof map.enabled === "boolean"
      ? [{ key: `map-${map.id}`, name: map.name, map: map.map_name, enabled: map.enabled }] : [])];
}
export function readArkModMapOverrides(settings: Readonly<SettingsObject>, moduleId: string): ArkModMapOverrides {
  const value = settings[ARK_MOD_MAP_OVERRIDES];
  if (value === undefined) return {};
  if (!record(value) || Object.keys(value).length > 16) throw new ArkModScopeError("shape");
  const maps = new Set(readArkModScopes(settings, "").map((map) => map.key)), asa = moduleId === "arksurvivalascended";
  const validId = (id: unknown) => typeof id === "string" && canonicalAsaModId(id) === id && (asa || id.length >= 6);
  for (const [map, entry] of Object.entries(value)) {
    if (!maps.has(map) || !record(entry) || Object.keys(entry).some((key) => !["mods", "active_order", ...(asa ? ["passive_order"] : [])].includes(key))
      || !record(entry.mods) || Object.keys(entry.mods).length > 65536) throw new ArkModScopeError("shape");
    for (const key of ["active_order", "passive_order"]) if (entry[key] !== undefined && (!Array.isArray(entry[key])
      || entry[key].length > 65536 || new Set(entry[key]).size !== entry[key].length || entry[key].some((id) => !validId(id)))) throw new ArkModScopeError("shape");
    for (const [id, override] of Object.entries(entry.mods)) {
      if (!validId(id) || !record(override) || Object.keys(override).some((key) => !(asa ? [...membershipKeys, "configuration"] : ["active"]).includes(key))) throw new ArkModScopeError("shape");
      for (const key of membershipKeys) if (override[key] !== undefined && typeof override[key] !== "boolean") throw new ArkModScopeError("shape");
      if (override.configuration !== undefined && override.configuration !== null) readAsaModConfigurations({ asa_mod_configurations: { [id]: override.configuration } });
    }
  }
  return value as unknown as ArkModMapOverrides;
}
const markerIds = (settings: Readonly<SettingsObject>, key: string): string[] => Array.isArray(settings[key])
  ? settings[key].filter((id): id is string => typeof id === "string") : [];
function orderedMembership(base: readonly string[], mods: Record<string, ArkModOverride>, key: "active" | "passive", order?: string[]): string[] {
  const enabled = new Set(base);
  for (const [id, override] of Object.entries(mods)) {
    if (override[key] === true) enabled.add(id); else if (override[key] === false) enabled.delete(id);
  }
  return unique([...(order ?? []), ...base, ...Object.keys(mods).sort()]).filter((id) => enabled.has(id));
}
export function projectArkModSettings(settings: Readonly<SettingsObject>, moduleId: string, map: string | null): SettingsObject {
  if (!arkModFields(moduleId).length || map === null) return { ...settings };
  const overrides = readArkModMapOverrides(settings, moduleId);
  if (!readArkModScopes(settings, "").some((entry) => entry.key === map)) throw new ArkModScopeError("scope");
  const entry = overrides[map];
  if (!entry) return { ...settings };
  const result = { ...settings }, asa = moduleId === "arksurvivalascended", shared = readAsaModMembership(settings);
  const active = asa ? shared.active : parseEvolvedModIds(settings.active_mod_ids);
  result[asa ? "mod_ids_csv" : "active_mod_ids"] = orderedMembership(active, entry.mods, "active", entry.active_order).join("\n");
  if (!asa) return result;
  result.passive_mod_ids_csv = orderedMembership(shared.passive, entry.mods, "passive", entry.passive_order).join("\n");
  for (const [key, field] of [["disabled_active", "curseforge_disabled_mod_ids"], ["disabled_passive", "curseforge_disabled_passive_mod_ids"], ["removed", "curseforge_removed_mod_ids"]] as const) {
    const ids = new Set(markerIds(settings, field));
    for (const [id, override] of Object.entries(entry.mods)) {
      if (override[key] === true) ids.add(id); else if (override[key] === false) ids.delete(id);
    }
    result[field] = [...ids];
  }
  const configurations = readAsaModConfigurations(settings);
  for (const [id, override] of Object.entries(entry.mods)) {
    if (override.configuration === null) delete configurations[id];
    else if (override.configuration !== undefined && Object.keys(override.configuration).length > 0) configurations[id] = {
      game_ini: override.configuration.game_ini ?? configurations[id]?.game_ini ?? "",
      game_user_settings_ini: override.configuration.game_user_settings_ini ?? configurations[id]?.game_user_settings_ini ?? "",
    };
  }
  result.asa_mod_configurations = configurations;
  return result;
}
/** This projection is for the instance list only. Never persist its union as shared membership. */
export function arkModUnionSettings(settings: Readonly<SettingsObject>, moduleId: string): SettingsObject {
  if (!arkModFields(moduleId).length) return { ...settings };
  const maps = readArkModScopes(settings, "").map((map) => projectArkModSettings(settings, moduleId, map.key));
  if (moduleId === "arksurvivalevolved") return { ...settings,
    active_mod_ids: unique([...parseEvolvedModIds(settings.active_mod_ids), ...maps.flatMap((map) => parseEvolvedModIds(map.active_mod_ids))]).join("\n"),
    auto_managed_mod_ids: unique([...parseEvolvedModIds(settings.auto_managed_mod_ids), ...maps.flatMap((map) => parseEvolvedModIds(map.active_mod_ids)),
      ...Object.values(readArkModMapOverrides(settings, moduleId)).flatMap((map) => Object.entries(map.mods).filter(([, mod]) => mod.active !== undefined).map(([id]) => id))]).join("\n") };
  const states = [readAsaModMembership(settings), ...maps.map(readAsaModMembership)], owned = new Set(states.flatMap((state) => [...state.owned]));
  return { ...settings, mod_ids_csv: unique(states.flatMap((state) => state.active)).join("\n"),
    passive_mod_ids_csv: unique(states.flatMap((state) => state.passive)).join("\n"),
    curseforge_disabled_mod_ids: unique(states.flatMap((state) => state.disabledActive)),
    curseforge_disabled_passive_mod_ids: unique(states.flatMap((state) => state.disabledPassive)),
    curseforge_removed_mod_ids: unique(states.flatMap((state) => [...state.removed])).filter((id) => !owned.has(id)) };
}
export function arkModEnabledMaps(settings: Readonly<SettingsObject>, moduleId: string, modId: string): string[] {
  return readArkModScopes(settings, "").filter((map) => {
    const effective = projectArkModSettings(settings, moduleId, map.key);
    return moduleId === "arksurvivalascended" ? readAsaModMembership(effective).enabled.has(modId) : parseEvolvedModIds(effective.active_mod_ids).includes(modId);
  }).map((map) => map.key);
}
export function arkModEnabledCounts(settings: Readonly<SettingsObject>, moduleId: string): { counts: Map<string, number>; total: number } {
  const maps = readArkModScopes(settings, ""), counts = new Map<string, number>();
  for (const map of maps) {
    const effective = projectArkModSettings(settings, moduleId, map.key);
    const ids = moduleId === "arksurvivalascended" ? [...readAsaModMembership(effective).enabled] : parseEvolvedModIds(effective.active_mod_ids);
    for (const id of ids) counts.set(id, (counts.get(id) ?? 0) + 1);
  }
  return { counts, total: maps.length };
}
function storeOverrides(settings: Readonly<SettingsObject>, overrides: ArkModMapOverrides): SettingsObject {
  for (const [map, entry] of Object.entries(overrides)) {
    for (const [id, mod] of Object.entries(entry.mods)) if (!Object.keys(mod).length) delete entry.mods[id];
    if (!Object.keys(entry.mods).length && entry.active_order === undefined && entry.passive_order === undefined) delete overrides[map];
  }
  const next = { ...settings };
  if (Object.keys(overrides).length) next[ARK_MOD_MAP_OVERRIDES] = overrides; else delete next[ARK_MOD_MAP_OVERRIDES];
  return next;
}
export function applyArkModChange(settings: Readonly<SettingsObject>, moduleId: string, change: ArkModChange): SettingsObject {
  if (canonicalAsaModId(change.modId) !== change.modId || (moduleId === "arksurvivalevolved" && change.modId.length < 6)) throw new ArkModScopeError("shape");
  if (moduleId !== "arksurvivalascended" && (change.kind === "configuration" || change.kind === "inherit-configuration")) throw new ArkModScopeError("shape");
  if (moduleId === "arksurvivalascended" && hasAsaRawModFlags(settings)) throw new ArkModScopeError("raw-flags");
  const maps = readArkModScopes(settings, "").map((map) => map.key), overrides = structuredClone(readArkModMapOverrides(settings, moduleId));
  const targets = change.kind === "membership" || change.kind === "inherit-membership" ? maps : change.maps;
  if (targets?.some((map) => !maps.includes(map)) || (change.kind === "membership" && change.maps.some((map) => !maps.includes(map)))) throw new ArkModScopeError("scope");
  if (change.kind === "configuration" && change.maps === null) {
    const configurations = readAsaModConfigurations(settings), config = configurations[change.modId] ?? { game_ini: "", game_user_settings_ini: "" };
    configurations[change.modId] = { ...config, [change.file]: change.text };
    return { ...settings, asa_mod_configurations: configurations };
  }
  for (const map of targets ?? []) {
    const entry = overrides[map] ??= { mods: {} }, mod = entry.mods[change.modId] ??= {};
    if (change.kind === "inherit-membership") for (const key of membershipKeys) delete mod[key];
    else if (change.kind === "membership") {
      const effective = projectArkModSettings(settings, moduleId, map), state = readAsaModMembership(effective), shared = readAsaModMembership(settings);
      const selected = change.maps.includes(map), passive = state.passive.includes(change.modId) || state.disabledPassive.includes(change.modId)
        || (!state.active.includes(change.modId) && !state.disabledActive.includes(change.modId) && shared.passive.includes(change.modId));
      const active = moduleId !== "arksurvivalascended" || state.active.includes(change.modId) || state.disabledActive.includes(change.modId) || !passive;
      mod.active = selected && active;
      if (moduleId === "arksurvivalascended") Object.assign(mod, { passive: selected && passive,
        disabled_active: !selected && active, disabled_passive: !selected && passive, removed: false });
    } else if (change.kind === "inherit-configuration") delete mod.configuration;
    else if (change.kind === "configuration") mod.configuration = { ...(mod.configuration === null ? { game_ini: "", game_user_settings_ini: "" } : mod.configuration), [change.file]: change.text };
  }
  return storeOverrides(settings, overrides);
}
export function arkModHasMembershipOverrides(settings: Readonly<SettingsObject>, moduleId: string, modId: string): boolean {
  return Object.values(readArkModMapOverrides(settings, moduleId)).some((map) => membershipKeys.some((key) => map.mods[modId]?.[key] !== undefined));
}
export function applyArkSharedModAction(settings: Readonly<SettingsObject>, moduleId: string, ids: string[], action: AsaModAction, inventory: string[] = []): SettingsObject {
  const unionOwned = moduleId === "arksurvivalascended" ? readAsaModMembership(arkModUnionSettings(settings, moduleId)).owned : new Set<string>();
  const sharedBase = action === "remove" ? { ...settings, curseforge_removed_mod_ids: markerIds(settings, "curseforge_removed_mod_ids")
    .filter((id) => !ids.includes(id) || !unionOwned.has(id)) } : settings;
  let next = moduleId === "arksurvivalascended" ? buildAsaModPlan(sharedBase, ids, action,
    unique([...inventory, ...(action === "remove" ? ids.filter((id) => unionOwned.has(id)) : [])])) : { ...settings };
  if (action !== "remove") return next;
  const overrides = structuredClone(readArkModMapOverrides(settings, moduleId));
  for (const entry of Object.values(overrides)) for (const id of ids) {
    const mod = entry.mods[id];
    if (mod) for (const key of membershipKeys) delete mod[key];
    if (entry.active_order) entry.active_order = entry.active_order.filter((value) => value !== id);
    if (entry.passive_order) entry.passive_order = entry.passive_order.filter((value) => value !== id);
  }
  if (moduleId === "arksurvivalevolved") {
    next.active_mod_ids = parseEvolvedModIds(settings.active_mod_ids).filter((id) => !ids.includes(id)).join("\n");
    next.auto_managed_mod_ids = parseEvolvedModIds(settings.auto_managed_mod_ids).filter((id) => !ids.includes(id)).join("\n");
  }
  return storeOverrides(next, overrides);
}
function stable(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(stable).join(",")}]`;
  if (record(value)) return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${stable(value[key])}`).join(",")}}`;
  return JSON.stringify(value) ?? "null";
}
export function assertArkModChangeBase(base: Readonly<SettingsObject>, current: Readonly<SettingsObject>, moduleId: string, change: ArkModChange) {
  const targets = change.kind === "membership" || change.kind === "inherit-membership" ? readArkModScopes(base, "").map((map) => map.key) : change.maps;
  const signature = (settings: Readonly<SettingsObject>, map: string | null) => {
    const effective = projectArkModSettings(settings, moduleId, map), id = change.modId;
    const local = map === null ? undefined : readArkModMapOverrides(settings, moduleId)[map]?.mods[id];
    if (change.kind === "configuration") return [readAsaModConfigurations(effective)[id]?.[change.file] ?? "",
      local?.configuration === null ? "cleared" : local?.configuration?.[change.file] === undefined ? "inherited" : ["override", local.configuration[change.file]]];
    if (change.kind === "inherit-configuration") return [readAsaModConfigurations(effective)[id] ?? {}, local?.configuration];
    const state = readAsaModMembership(effective);
    return [moduleId === "arksurvivalascended" ? [state.active.includes(id), state.passive.includes(id), state.disabledActive.includes(id), state.disabledPassive.includes(id), state.removed.has(id)]
      : parseEvolvedModIds(effective.active_mod_ids).includes(id), membershipKeys.map((key) => local?.[key] === undefined ? "inherited" : local[key])];
  };
  if (base.custom_launch_flags !== current.custom_launch_flags || stable(targets === null ? signature(base, null) : targets.map((map) => signature(base, map)))
    !== stable(targets === null ? signature(current, null) : targets.map((map) => signature(current, map)))) throw new ArkModScopeError("changed");
  if ((change.kind === "membership" || change.kind === "inherit-membership") && stable(readArkModScopes(base, "").map((map) => map.key)) !== stable(readArkModScopes(current, "").map((map) => map.key))) throw new ArkModScopeError("changed");
}
export function arkModCompatibilityDiffers(settings: Readonly<SettingsObject>, moduleId: string): boolean {
  const signatures = readArkModScopes(settings, "").filter((map) => map.enabled).map((map) => {
    const effective = projectArkModSettings(settings, moduleId, map.key), state = readAsaModMembership(effective);
    const ids = moduleId === "arksurvivalascended" ? [state.active, state.passive] : [parseEvolvedModIds(effective.active_mod_ids)];
    const configurations = moduleId === "arksurvivalascended" ? Object.fromEntries(Object.entries(readAsaModConfigurations(effective)).filter(([id]) => state.enabled.has(id))
      .map(([id, config]) => [id, Object.fromEntries(Object.entries(config).map(([file, text]) => [file, parseAsaModIni(text).entries.map(({ section, key, value }) => [section, key, value])]))])) : {};
    return stable([ids, configurations]);
  });
  return signatures.some((signature) => signature !== signatures[0]);
}
