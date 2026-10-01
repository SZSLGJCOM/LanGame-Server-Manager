import type { ProjectZomboidWorkshopModsSnapshot } from "../../types";
import { parseWorkshopIdList } from "../settings/guided-setting-values";
import type { SettingsObject } from "../settings/settings-schema";
import {
  isCanonicalWorkshopModId, mergeProjectZomboidMapList, mergeTextList, mergeWorkshopList,
  parseLineOrSemicolonEntries, readDisabledWorkshopModIds, removeTextListValues, removeWorkshopListIds
} from "./mod-workbench-model";

export type WorkshopControlErrorCode = "unsupported-module" | "invalid-selection" | "not-owned"
  | "local-metadata-missing" | "invalid-settings" | "ownership-limit";

export class WorkshopControlError extends Error {
  constructor(readonly code: WorkshopControlErrorCode, readonly ids: readonly string[] = []) {
    super(`Workshop Mod controls: ${code}${ids.length ? ` (${ids.join(", ")})` : ""}`);
    this.name = "WorkshopControlError";
  }
}

export interface WorkshopControlState {
  owned: boolean;
  enabled: boolean;
  partiallyEnabled: boolean;
  canToggle: boolean;
}

export interface WorkshopModControlPlan {
  nextSettings: SettingsObject;
  retainedLocalIds: string[];
}

const DISABLED_FIELD = "steam_workshop_disabled_mod_ids";
const DISABLED_MODULES = new Set(["barotrauma", "conanexiles", "soulmask"]);
const MAX_DISABLED_IDS = 8192;

export function supportsWorkshopModEnablement(moduleId: string): boolean {
  return moduleId === "projectzomboid" || moduleId === "arksurvivalevolved" || DISABLED_MODULES.has(moduleId);
}

function readWorkshopOwnership(moduleId: string, settings: SettingsObject): { ids: string[]; active: Set<string> } {
  if (moduleId === "projectzomboid") return { ids: parseWorkshopIdList(settings.workshop_items), active: new Set() };
  if (moduleId === "arksurvivalevolved") {
    const active = new Set(parseWorkshopIdList(settings.active_mod_ids));
    return { ids: [...new Set([...active, ...parseWorkshopIdList(settings.auto_managed_mod_ids)])], active };
  }
  if (DISABLED_MODULES.has(moduleId)) {
    const active = new Set(parseWorkshopIdList(settings.mod_workshop_ids));
    return { ids: [...new Set([...active, ...readDisabledWorkshopModIds(settings)])], active };
  }
  return { ids: [], active: new Set() };
}

export function readOwnedWorkshopModIds(moduleId: string, settings: SettingsObject): string[] {
  return readWorkshopOwnership(moduleId, settings).ids;
}

type PzLocalIds = { mods: string[]; maps: string[] };

function readPzItemLocalIds(item: ProjectZomboidWorkshopModsSnapshot["items"][number]): PzLocalIds {
  if (item.status !== "installed" || !item.mods.length ||
    item.mods.some((mod) => mod.status !== "loaded" || (!mod.mod_id?.trim() && !mod.map_ids.some((id) => id.trim())))) {
    throw new WorkshopControlError("local-metadata-missing", [item.workshop_item_id]);
  }
  const values = {
    mods: item.mods.flatMap((mod) => mod.mod_id?.trim() ? [mod.mod_id.trim()] : []),
    maps: item.mods.flatMap((mod) => mod.map_ids.map((id) => id.trim()).filter(Boolean))
  };
  if (!values.mods.length && !values.maps.some((id) => id.toLowerCase() !== "muldraugh, ky")) {
    throw new WorkshopControlError("local-metadata-missing", [item.workshop_item_id]);
  }
  return values;
}

function readPzLocalIds(
  ids: readonly string[], snapshot?: ProjectZomboidWorkshopModsSnapshot | null
): Map<string, PzLocalIds> {
  if (!snapshot?.workshop_root_exists) throw new WorkshopControlError("local-metadata-missing", ids);
  const required = new Set(ids);
  const local = new Map<string, PzLocalIds>();
  for (const item of snapshot.items) {
    if (!required.has(item.workshop_item_id)) continue;
    if (local.has(item.workshop_item_id)) {
      throw new WorkshopControlError("local-metadata-missing", [item.workshop_item_id]);
    }
    local.set(item.workshop_item_id, readPzItemLocalIds(item));
  }
  const missing = [...required].filter((id) => !local.has(id));
  if (missing.length) throw new WorkshopControlError("local-metadata-missing", missing);
  return local;
}

export function readWorkshopControlStates(
  moduleId: string, settings: SettingsObject, pzSnapshot?: ProjectZomboidWorkshopModsSnapshot | null
): Map<string, WorkshopControlState> {
  const { ids, active } = readWorkshopOwnership(moduleId, settings);
  const states = new Map<string, WorkshopControlState>();
  for (const id of ids) {
    states.set(id, { owned: true, enabled: active.has(id), partiallyEnabled: false, canToggle: moduleId !== "projectzomboid" });
  }
  if (moduleId !== "projectzomboid" || !pzSnapshot?.workshop_root_exists) return states;
  const mods = new Set(parseLineOrSemicolonEntries(settings.mods).map((value) => value.toLowerCase()));
  const maps = new Set(parseLineOrSemicolonEntries(settings.map_name).map((value) => value.toLowerCase()));
  const visited = new Set<string>();
  for (const item of pzSnapshot.items) {
    const state = states.get(item.workshop_item_id);
    if (!state) continue;
    try {
      if (visited.has(item.workshop_item_id)) throw new WorkshopControlError("local-metadata-missing", [item.workshop_item_id]);
      visited.add(item.workshop_item_id);
      const local = readPzItemLocalIds(item);
      const enabledParts = [...local.mods.map((value) => mods.has(value.toLowerCase())),
        ...local.maps.filter((value) => value.toLowerCase() !== "muldraugh, ky").map((value) => maps.has(value.toLowerCase()))];
      state.enabled = enabledParts.every(Boolean);
      state.partiallyEnabled = !state.enabled && enabledParts.some(Boolean);
      state.canToggle = true;
    } catch (error) {
      if (!(error instanceof WorkshopControlError)) throw error;
      state.enabled = false;
      state.partiallyEnabled = false;
      state.canToggle = false;
    }
  }
  return states;
}

export function readWorkshopControlState(
  moduleId: string, settings: SettingsObject, id: string, pzSnapshot?: ProjectZomboidWorkshopModsSnapshot | null
): WorkshopControlState {
  return readWorkshopControlStates(moduleId, settings, pzSnapshot).get(id)
    ?? { owned: false, enabled: false, partiallyEnabled: false, canToggle: false };
}

function checkedSelection(moduleId: string, settings: SettingsObject, ids: readonly string[], requireOwnership: boolean): string[] {
  if (!supportsWorkshopModEnablement(moduleId)) throw new WorkshopControlError("unsupported-module");
  if (!ids.length || ids.length > MAX_DISABLED_IDS || !ids.every(isCanonicalWorkshopModId)) throw new WorkshopControlError("invalid-selection", ids);
  if (DISABLED_MODULES.has(moduleId)) {
    const disabled = settings[DISABLED_FIELD];
    if (disabled !== undefined && (!Array.isArray(disabled) || disabled.length > MAX_DISABLED_IDS ||
      !disabled.every(isCanonicalWorkshopModId) || new Set(disabled).size !== disabled.length)) {
      throw new WorkshopControlError("invalid-settings");
    }
  }
  const selected = [...new Set(ids)];
  const owned = new Set(readOwnedWorkshopModIds(moduleId, settings));
  const missing = requireOwnership ? selected.filter((id) => !owned.has(id)) : [];
  if (missing.length) throw new WorkshopControlError("not-owned", missing);
  return selected;
}

function writeDisabled(next: SettingsObject, ids: readonly string[]): void {
  const unique = [...new Set(ids)];
  if (unique.length > MAX_DISABLED_IDS) throw new WorkshopControlError("ownership-limit");
  if (unique.length) next[DISABLED_FIELD] = unique;
  else delete next[DISABLED_FIELD];
}

function disablePzLocalIds(settings: SettingsObject, ids: string[], local: Map<string, PzLocalIds>): WorkshopModControlPlan {
  const nextSettings = { ...settings };
  const targets = new Set(ids);
  const remaining = parseWorkshopIdList(settings.workshop_items).filter((id) => !targets.has(id));
  const retainedLocalIds: string[] = [];
  for (const [key, kind] of [["mods", "mods"], ["map_name", "maps"]] as const) {
    const needed = new Set(remaining.flatMap((id) => local.get(id)![kind]).map((value) => value.toLowerCase()));
    if (kind === "maps") needed.add("muldraugh, ky");
    const targeted = [...new Set(ids.flatMap((id) => local.get(id)![kind]))];
    retainedLocalIds.push(...targeted.filter((value) => needed.has(value.toLowerCase())));
    const removal = removeTextListValues(settings, key, targeted.filter((value) => !needed.has(value.toLowerCase())), parseLineOrSemicolonEntries);
    if (removal.removedValues.length) nextSettings[key] = removal.value;
  }
  return { nextSettings, retainedLocalIds: [...new Set(retainedLocalIds)] };
}

export function buildWorkshopModEnablementPlan(
  moduleId: string, settings: SettingsObject, ids: readonly string[], enabled: boolean,
  pzSnapshot?: ProjectZomboidWorkshopModsSnapshot | null
): WorkshopModControlPlan {
  const selected = checkedSelection(moduleId, settings, ids, true);
  const nextSettings = { ...settings };
  if (moduleId === "projectzomboid") {
    const local = readPzLocalIds([...new Set([...parseWorkshopIdList(settings.workshop_items), ...selected])], pzSnapshot);
    if (!enabled) return disablePzLocalIds(settings, selected, local);
    const mods = selected.flatMap((id) => local.get(id)!.mods);
    const maps = selected.flatMap((id) => local.get(id)!.maps).filter((id) => id.toLowerCase() !== "muldraugh, ky");
    if (mods.length) nextSettings.mods = mergeTextList(settings, "mods", mods, parseLineOrSemicolonEntries).value;
    if (maps.length) nextSettings.map_name = mergeProjectZomboidMapList(settings, "map_name", maps).value;
  } else if (moduleId === "arksurvivalevolved") {
    nextSettings.auto_managed_mod_ids = mergeWorkshopList(settings, "auto_managed_mod_ids", selected).value;
    nextSettings.active_mod_ids = enabled ? mergeWorkshopList(settings, "active_mod_ids", selected).value
      : removeWorkshopListIds(settings, "active_mod_ids", selected).value;
  } else {
    nextSettings.mod_workshop_ids = enabled ? mergeWorkshopList(settings, "mod_workshop_ids", selected).value
      : removeWorkshopListIds(settings, "mod_workshop_ids", selected).value;
    const targets = new Set(selected);
    const disabled = readDisabledWorkshopModIds(settings).filter((id) => !targets.has(id));
    writeDisabled(nextSettings, enabled ? disabled : [...disabled, ...selected]);
  }
  return { nextSettings, retainedLocalIds: [] };
}

export function buildProjectZomboidWorkshopRemovalPlan(
  settings: SettingsObject, ids: readonly string[], pzSnapshot: ProjectZomboidWorkshopModsSnapshot | null
): WorkshopModControlPlan {
  // Whole-collection removal may include already absent saved members, so the
  // fresh complete local snapshot supplies the mapping, not inferred ownership.
  const selected = checkedSelection("projectzomboid", settings, ids, false);
  const local = readPzLocalIds([...new Set([...parseWorkshopIdList(settings.workshop_items), ...selected])], pzSnapshot);
  const plan = disablePzLocalIds(settings, selected, local);
  const removal = removeWorkshopListIds(settings, "workshop_items", selected);
  if (removal.removedIds.length) plan.nextSettings.workshop_items = removal.value;
  return plan;
}

export function buildWorkshopDownloadOwnershipPlan(moduleId: string, settings: SettingsObject, ids: readonly string[]): SettingsObject {
  const selected = checkedSelection(moduleId, settings, ids, false);
  const next = { ...settings };
  if (moduleId === "projectzomboid") next.workshop_items = mergeWorkshopList(settings, "workshop_items", selected).value;
  else if (moduleId === "arksurvivalevolved") {
    next.auto_managed_mod_ids = mergeWorkshopList(settings, "auto_managed_mod_ids", selected).value;
    next.auto_managed_mods = true;
  } else {
    const enabled = new Set(parseWorkshopIdList(settings.mod_workshop_ids));
    writeDisabled(next, [...readDisabledWorkshopModIds(settings), ...selected].filter((id) => !enabled.has(id)));
  }
  return next;
}
