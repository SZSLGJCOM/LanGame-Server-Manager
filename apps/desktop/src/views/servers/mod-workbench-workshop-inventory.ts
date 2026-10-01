import type { ManualModInventoryItem, ManualModInventoryResult } from "../../types";
import type { SettingsObject } from "../settings/settings-schema";
import { isCanonicalWorkshopModId, mergeTextList, parseDelimitedEntries, removeTextListValues, uniqueEntries } from "./mod-workbench-model";

function inventoryRelativePath(item: ManualModInventoryItem, inventory: ManualModInventoryResult | null): string | null {
  const root = inventory?.target_path.replace(/\\/g, "/").replace(/\/+$/, "");
  const path = item.path.replace(/\\/g, "/");
  if (!root || item.file_count <= 0 || !path.toLowerCase().startsWith(`${root.toLowerCase()}/`)) return null;
  const relative = path.slice(root.length + 1);
  if (relative.split("/").some((segment) => segment === "." || segment === "..")) return null;
  return relative;
}

/** Only an item's path inside this instance can establish Workshop ownership. */
export function inventoryWorkshopIds(item: ManualModInventoryItem, inventory: ManualModInventoryResult | null): string[] {
  const relative = inventoryRelativePath(item, inventory);
  if (relative === null) return [];
  return uniqueEntries([item.name, relative].flatMap((value) => value.match(/\d+/g) ?? []).filter(isCanonicalWorkshopModId));
}

export function workshopInventoryItems(ids: readonly string[], inventory: ManualModInventoryResult | null): ManualModInventoryItem[] {
  const selected = new Set(ids);
  return (inventory?.items ?? []).filter((item) => inventoryWorkshopIds(item, inventory).some((id) => selected.has(id)));
}

export function readRemovedWorkshopIds(settings: SettingsObject): string[] {
  const value = settings.steam_workshop_removed_mod_ids;
  return Array.isArray(value) ? uniqueEntries(value.filter(isCanonicalWorkshopModId)) : [];
}

export function restoreRemovedWorkshopIds(settings: SettingsObject, ids: readonly string[]): SettingsObject {
  const restored = new Set(ids);
  const remaining = readRemovedWorkshopIds(settings).filter((id) => !restored.has(id));
  const next = { ...settings };
  if (remaining.length) next.steam_workshop_removed_mod_ids = remaining;
  else delete next.steam_workshop_removed_mod_ids;
  return next;
}

export function isRemovedWorkshopInventoryItem(item: ManualModInventoryItem, inventory: ManualModInventoryResult | null, settings: SettingsObject): boolean {
  const ids = inventoryWorkshopIds(item, inventory);
  const removed = new Set(readRemovedWorkshopIds(settings));
  return ids.length > 0 && ids.every((id) => removed.has(id));
}

export class WorkshopInventoryControlError extends Error {
  constructor(public readonly code: "local-metadata-missing" | "not-owned" | "ownership-limit", public readonly ids: readonly string[]) {
    super(`Workshop inventory control: ${code} (${ids.join(", ")})`);
  }
}

export function palworldWorkshopStates(settings: SettingsObject, inventory: ManualModInventoryResult | null) {
  const members = new Map<string, Array<string | null>>();
  for (const item of inventory?.items ?? []) {
    for (const id of inventoryWorkshopIds(item, inventory)) {
      const names = members.get(id) ?? [];
      names.push(item.inferred_id?.trim().toLowerCase() || null);
      members.set(id, names);
    }
  }
  const removed = new Set(readRemovedWorkshopIds(settings));
  const enabledNames = new Set(parseDelimitedEntries(settings.mod_package_names).map((name) => name.toLowerCase()));
  return new Map([...members].map(([id, names]) => {
    const owned = !removed.has(id);
    const canToggle = owned && names.every((name) => name !== null);
    const enabled = canToggle && names.every((name) => name !== null && enabledNames.has(name));
    return [id, { owned, enabled, canToggle,
      partiallyEnabled: owned && !enabled && names.some((name) => name !== null && enabledNames.has(name)) }] as const;
  }));
}

export function palworldWorkshopState(settings: SettingsObject, inventory: ManualModInventoryResult | null, id: string) {
  return palworldWorkshopStates(settings, inventory).get(id) ?? { owned: false, enabled: false, canToggle: false, partiallyEnabled: false };
}

/** Removing keeps payloads recoverable, while package enablement and instance membership change together. */
export function buildPalworldWorkshopPlan(settings: SettingsObject, inventory: ManualModInventoryResult | null,
  ids: readonly string[], action: "enable" | "disable" | "remove") {
  const selected = uniqueEntries([...ids]);
  if (!selected.length || !inventory || inventory.module_id !== "palworld" || !inventory.target_exists) {
    throw new WorkshopInventoryControlError("local-metadata-missing", selected);
  }
  if (action === "remove" && inventory.items.some((item) => item.file_count > 0 &&
    (inventoryRelativePath(item, inventory) === null || !item.inferred_id?.trim()))) {
    throw new WorkshopInventoryControlError("local-metadata-missing", selected);
  }
  const names: string[] = [];
  const states = palworldWorkshopStates(settings, inventory);
  for (const id of selected) {
    const state = states.get(id);
    if (!isCanonicalWorkshopModId(id) || !state?.owned) throw new WorkshopInventoryControlError("not-owned", [id]);
    if (!state.canToggle) throw new WorkshopInventoryControlError("local-metadata-missing", [id]);
    names.push(...workshopInventoryItems([id], inventory).map((item) => item.inferred_id!.trim()));
  }
  const next = { ...settings };
  let retainedLocalIds: string[] = [];
  if (action === "enable") {
    next.mod_package_names = mergeTextList(settings, "mod_package_names", names, parseDelimitedEntries).value;
  } else {
    // A different installed Mod can use the same PackageName. Do not unload it when removing a source.
    const removed = new Set(readRemovedWorkshopIds(settings));
    const retained = new Set(inventory.items.filter((item) => {
      if (isRemovedWorkshopInventoryItem(item, inventory, settings)) return false;
      const tokens = inventoryWorkshopIds(item, inventory);
      return !tokens.some((id) => selected.includes(id)) || tokens.some((id) => !selected.includes(id) && !removed.has(id));
    }).flatMap((item) => item.inferred_id?.trim() ? [item.inferred_id.trim().toLowerCase()] : []));
    retainedLocalIds = action === "remove" ? uniqueEntries(names.filter((name) => retained.has(name.toLowerCase()))) : [];
    const retainedKeys = new Set(retainedLocalIds.map((name) => name.toLowerCase()));
    next.mod_package_names = removeTextListValues(settings, "mod_package_names", names.filter((name) => !retainedKeys.has(name.toLowerCase())), parseDelimitedEntries).value;
  }
  if (action === "remove") {
    const removed = uniqueEntries([...readRemovedWorkshopIds(settings), ...selected]);
    if (removed.length > 8192) throw new WorkshopInventoryControlError("ownership-limit", selected);
    next.steam_workshop_removed_mod_ids = removed;
  }
  return { nextSettings: next, retainedLocalIds };
}
