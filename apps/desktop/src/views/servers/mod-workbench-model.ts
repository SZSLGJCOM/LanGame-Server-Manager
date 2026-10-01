import type { ModuleDetails, SteamWorkshopLookupChild, SteamWorkshopLookupItem } from "../../types";
import { parseWorkshopIdList } from "../settings/guided-setting-values";
import type { SettingsObject } from "../settings/settings-schema";
import type { ModProviderKind } from "./mod-workbench-capability";
import { readAsaModMembership } from "./mod-workbench-asa";
import { DONTSTARVE_SHARDS } from "../settings/modules/dontstarve-shards";
import { readDstRawModDeclarations } from "./mod-workbench-dst-raw";

export type ModEntryKind = "steam-item" | "steam-collection" | "game-mod" | "map" | "native" | "manual";
export type SteamWorkshopBrowseSort = "relevance" | "trend" | "popular" | "recent" | "subscribers";

export interface ModSourceEntry {
  key: string;
  label: string;
  fieldLabel: string;
  kind: ModEntryKind;
  ids: string[];
  values: string[];
}

export interface ModEnabledRow {
  key: string;
  entry: ModSourceEntry;
  value: string;
  id: string;
}

export function workflowProviderFromModuleDetails(moduleDetails?: ModuleDetails | null): ModProviderKind {
  if (moduleDetails?.workshop?.provider === "steam") {
    return "steam";
  }
  switch (moduleDetails?.mods?.source?.provider?.trim().toLowerCase()) {
    case "curseforge":
      return "curseforge";
    case "nexus":
      return "nexus";
    case "modrinth":
      return "modrinth";
    case "thunderstore":
      return "thunderstore";
    default:
      return "manual";
  }
}

export function parseDelimitedEntries(value: unknown): string[] {
  if (typeof value !== "string" || value.trim().length === 0) {
    return [];
  }

  const seen = new Set<string>();
  const entries: string[] = [];
  for (const entry of value
    .replace(/\r\n/g, "\n")
    .replace(/\r/g, "\n")
    .split(/[\n;,]+/)
    .map((item) => item.trim())
    .filter(Boolean)) {
    const key = entry.toLowerCase();
    if (!seen.has(key)) {
      seen.add(key);
      entries.push(entry);
    }
  }
  return entries;
}

export function parseLineOrSemicolonEntries(value: unknown): string[] {
  if (typeof value !== "string" || value.trim().length === 0) {
    return [];
  }

  const seen = new Set<string>();
  const entries: string[] = [];
  for (const entry of value
    .replace(/\r\n/g, "\n")
    .replace(/\r/g, "\n")
    .split(/[\n;]+/)
    .map((item) => item.trim())
    .filter(Boolean)) {
    const key = entry.toLowerCase();
    if (!seen.has(key)) {
      seen.add(key);
      entries.push(entry);
    }
  }
  return entries;
}

export function uniqueEntries(values: string[]): string[] {
  const seen = new Set<string>();
  const entries: string[] = [];
  for (const value of values.map((entry) => entry.trim()).filter(Boolean)) {
    if (!seen.has(value)) {
      seen.add(value);
      entries.push(value);
    }
  }
  return entries;
}

export function haveSameWorkshopItemReferences(
  current: SteamWorkshopLookupItem[],
  next: SteamWorkshopLookupItem[]
): boolean {
  return current.length === next.length && current.every((item, index) => item === next[index]);
}

function buildEntry(
  key: string,
  label: string,
  fieldLabel: string,
  kind: ModEntryKind,
  values: string[],
  ids: string[] = values
): ModSourceEntry | null {
  const cleanValues = uniqueEntries(values);
  const cleanIds = uniqueEntries(ids);
  if (cleanValues.length === 0 && cleanIds.length === 0) {
    return null;
  }
  return { key, label, fieldLabel, kind, values: cleanValues, ids: cleanIds };
}

export function buildEnabledRows(entries: ModSourceEntry[]): ModEnabledRow[] {
  const rows: ModEnabledRow[] = [];
  const seen = new Map<string, number>();
  for (const entry of entries) {
    const values = entry.values.length > 0 ? entry.values : entry.ids;
    values.forEach((value, index) => {
      const id = entry.ids[index] ?? value;
      const baseKey = `${entry.key}:${id || value}`;
      const duplicateIndex = seen.get(baseKey) ?? 0;
      seen.set(baseKey, duplicateIndex + 1);
      rows.push({
        key: duplicateIndex === 0 ? baseKey : `${baseKey}:${duplicateIndex}`,
        entry,
        value,
        id
      });
    });
  }
  return rows;
}

export function buildConfigurableEntries(moduleId: string, entries: ModSourceEntry[]): ModSourceEntry[] {
  if (moduleId === "projectzomboid") {
    return entries.filter((entry) => entry.key !== "pz-maps");
  }
  if (moduleId !== "dontstarve") {
    return entries;
  }
  return entries.filter((entry) => entry.key === "dst-enabled" || entry.key === "dst-disabled");
}

function enabledEntryValues(entry: ModSourceEntry): string[] {
  return entry.values.length > 0 ? entry.values : entry.ids;
}

export function canReorderEnabledRow(row: ModEnabledRow): boolean {
  return row.entry.key !== "dst-enabled" && !row.entry.key.endsWith("-disabled") && enabledEntryValues(row.entry).length > 1;
}

export function reorderEnabledEntryValues(
  entry: ModSourceEntry,
  sourceValue: string,
  targetValue: string
): string[] | null {
  const values = enabledEntryValues(entry);
  const sourceIndex = values.findIndex((value) => value === sourceValue);
  const targetIndex = values.findIndex((value) => value === targetValue);
  if (sourceIndex < 0 || targetIndex < 0 || sourceIndex === targetIndex) {
    return null;
  }

  const nextValues = [...values];
  const [movedValue] = nextValues.splice(sourceIndex, 1);
  nextValues.splice(targetIndex, 0, movedValue);
  return nextValues;
}

export function isCanonicalWorkshopModId(id: unknown): id is string {
  return typeof id === "string" && /^[1-9]\d{5,19}$/.test(id) && BigInt(id) <= 18446744073709551615n;
}

export function readDstRemovedWorkshopModIds(settings: SettingsObject): string[] {
  const value = settings.dst_removed_workshop_mod_ids;
  return Array.isArray(value) ? uniqueEntries(value.filter(isCanonicalWorkshopModId)) : [];
}

export function readDisabledWorkshopModIds(settings: SettingsObject): string[] {
  const value = settings.steam_workshop_disabled_mod_ids;
  return Array.isArray(value) && value.length <= 8192 ? uniqueEntries(value.filter(isCanonicalWorkshopModId)) : [];
}

export function buildConfiguredEntries(
  moduleId: string,
  settings: SettingsObject,
  moduleDetails?: ModuleDetails | null,
  /** Member IDs saved in this instance's collection records, never remote lookup children. */
  ownedMemberIds: readonly string[] = []
): ModSourceEntry[] {
  const entries: Array<ModSourceEntry | null> = [];
  const enablement = moduleDetails?.mods?.enablement;
  if (enablement) {
    if (moduleId === "arksurvivalascended") {
      const state = readAsaModMembership(settings);
      return [
        buildEntry(`${moduleId}-${enablement.setting_key}`, enablement.setting_label, enablement.setting_key, "game-mod", state.active),
        buildEntry(`${moduleId}-passive`, enablement.setting_label, "passive_mod_ids_csv", "game-mod",
          state.passive.filter((id) => !state.active.includes(id))),
        buildEntry(`${moduleId}-disabled`, enablement.setting_label, enablement.setting_key, "game-mod", state.disabled)
      ].filter((entry): entry is ModSourceEntry => Boolean(entry));
    }
    const enablementKind: ModEntryKind =
      enablement.id_strategy === "palworld_package_name"
        ? "game-mod"
        : moduleDetails?.workshop?.provider === "steam"
          ? "steam-item"
          : "game-mod";
    entries.push(buildEntry(
      `${moduleId}-${enablement.setting_key}`,
      enablement.setting_label,
      enablement.setting_key,
      enablementKind,
      parseDelimitedEntries(settings[enablement.setting_key])
    ));
    if (["arksurvivalevolved", "barotrauma", "conanexiles", "soulmask"].includes(moduleId)) {
      const enabled = new Set(parseWorkshopIdList(settings[enablement.setting_key]));
      const owned = moduleId === "arksurvivalevolved"
        ? parseWorkshopIdList(settings.auto_managed_mod_ids) : readDisabledWorkshopModIds(settings);
      entries.push(buildEntry(`${moduleId}-disabled`, enablement.setting_label, enablement.setting_key,
        "steam-item", owned.filter((id) => !enabled.has(id))));
    }
    return entries.filter((entry): entry is ModSourceEntry => Boolean(entry));
  }

  switch (moduleId) {
    case "dontstarve": {
      const rawDeclarations = DONTSTARVE_SHARDS.flatMap((shard) =>
        readDstRawModDeclarations(settings[`${shard}_modoverrides_lua`]));
      const enabledIds = uniqueEntries([...DONTSTARVE_SHARDS.flatMap((shard) =>
        parseWorkshopIdList(settings[`${shard}_enabled_workshop_mod_ids`])),
      ...rawDeclarations.filter((mod) => mod.enabled).map((mod) => mod.id)]);
      const removedIds = new Set(readDstRemovedWorkshopModIds(settings));
      const configurationIds = DONTSTARVE_SHARDS.map((shard) => settings[`${shard}_mod_configuration_options`])
        .flatMap((value) => value && typeof value === "object" && !Array.isArray(value)
          ? parseWorkshopIdList(Object.keys(value).join("\n")) : []).filter((id) => !removedIds.has(id));
      // Ownership comes from this instance's settings, never machine-wide caches.
      const enabledIdSet = new Set(enabledIds);
      const disabledIds = uniqueEntries([
        ...parseWorkshopIdList(settings.shared_workshop_mod_ids), ...configurationIds, ...rawDeclarations.map((mod) => mod.id),
        ...ownedMemberIds.filter((id) => isCanonicalWorkshopModId(id) && !removedIds.has(id))
      ]).filter((id) => !enabledIdSet.has(id));
      entries.push(
        buildEntry("dst-shared-mods", "Instance downloads", "shared_workshop_mod_ids", "steam-item", parseWorkshopIdList(settings.shared_workshop_mod_ids)),
        buildEntry("dst-shared-collections", "Instance collections", "shared_workshop_collection_ids", "steam-collection", parseWorkshopIdList(settings.shared_workshop_collection_ids)),
        buildEntry(
          "dst-enabled",
          "Mod",
          "modoverrides.lua",
          "game-mod",
          enabledIds
        ),
        buildEntry("dst-disabled", "Mod", "modoverrides.lua", "game-mod", disabledIds)
      );
      break;
    }
    case "projectzomboid":
      entries.push(
        buildEntry("pz-workshop", "WorkshopItems", "workshop_items", "steam-item", parseWorkshopIdList(settings.workshop_items)),
        buildEntry("pz-mods", "Enabled Mod IDs", "mods", "game-mod", parseLineOrSemicolonEntries(settings.mods), []),
        buildEntry("pz-maps", "Map order", "map_name", "map", parseLineOrSemicolonEntries(settings.map_name), [])
      );
      break;
    case "unturned":
      entries.push(buildEntry("unturned-workshop", "Workshop File IDs", "workshop_file_ids", "steam-item", parseWorkshopIdList(settings.workshop_file_ids)));
      break;
  }
  return entries.filter((entry): entry is ModSourceEntry => Boolean(entry));
}

export function buildSteamLookupIds(entries: ModSourceEntry[], draftIds: string[]): string[] {
  return uniqueEntries([
    ...entries
      .filter((entry) => entry.kind === "steam-item" || entry.kind === "steam-collection" ||
        entry.key === "dst-enabled" || entry.key === "dst-disabled")
      .flatMap((entry) => entry.ids),
    ...draftIds
  ]);
}

export function steamItemBelongsToApp(
  item: SteamWorkshopLookupItem | undefined,
  expectedAppId: number | null
): boolean {
  if (!item || typeof expectedAppId !== "number") {
    return true;
  }
  return typeof item.consumer_app_id !== "number" || item.consumer_app_id === expectedAppId;
}

export function buildDownloadableSteamIds(
  ids: string[],
  lookupMap: Record<string, SteamWorkshopLookupItem>,
  expectedAppId: number | null
): string[] {
  const downloadable: string[] = [];
  for (const id of ids) {
    const item = lookupMap[id];
    if (!item || item.status !== "resolved" || isUnsupportedWorkshopItem(item, expectedAppId) || isIncompleteWorkshopCollection(item) || !steamItemBelongsToApp(item, expectedAppId)) continue;
    if (item.item_kind === "collection") {
      downloadable.push(...(serverWorkshopCollectionChildren(item, lookupMap, expectedAppId) ?? []).map((child) => child.id));
      continue;
    }
    downloadable.push(id);
  }
  return uniqueEntries(downloadable);
}

export function canToggleModEnabledRow(
  row: ModEnabledRow,
  moduleId: string,
  isManualEnablementModule: boolean
): boolean {
  // Only these workflows retain an instance-owned entry after disabling it.
  return isManualEnablementModule || (moduleId === "dontstarve" &&
    (row.entry.key === "dst-enabled" || row.entry.key === "dst-disabled")) ||
    (moduleId === "projectzomboid" && row.entry.key === "pz-workshop") ||
    (["arksurvivalevolved", "barotrauma", "conanexiles", "soulmask"].includes(moduleId) &&
      (row.entry.key === `${moduleId}-disabled` || row.entry.key === `${moduleId}-${row.entry.fieldLabel}`));
}

export function isIncompleteWorkshopCollection(item: SteamWorkshopLookupItem | undefined): boolean {
  return item?.item_kind === "collection" && (
    item.child_count > item.children.length ||
    item.children.some((child) => child.item_kind !== "item" || child.status !== "resolved")
  );
}

export function isClientOnlyDstWorkshopItem(item: { tags?: readonly string[] } | undefined, appId: number | null): boolean {
  return appId === 322330 && Boolean(item?.tags?.some((tag) => tag.toLowerCase() === "client_only_mod"));
}

export function serverWorkshopCollectionChildren(
  item: SteamWorkshopLookupItem,
  lookupMap: Record<string, SteamWorkshopLookupItem>,
  appId: number | null
): SteamWorkshopLookupChild[] | null {
  const children = item.children.map((child) => lookupMap[child.id] ?? child);
  // A client-only tag cannot excuse unavailable metadata, another game or a guide.
  if (children.some((child, index) => child.id !== item.children[index].id || child.status !== "resolved" || child.item_kind !== "item" ||
      (typeof appId === "number" && child.consumer_app_id !== appId))) return null;
  return children.filter((child) => !isClientOnlyDstWorkshopItem(child, appId));
}

export function isUnsupportedWorkshopItem(item: SteamWorkshopLookupItem | undefined, appId: number | null): boolean {
  return Boolean(item && (item.status === "unsupported" ||
    (item.status !== "unverified" && (!["item", "collection"].includes(item.item_kind) || isIncompleteWorkshopCollection(item))) || isClientOnlyDstWorkshopItem(item, appId)));
}

export function expandWorkshopItemIds(items: SteamWorkshopLookupItem[]): string[] {
  return uniqueEntries(items.flatMap((item) =>
    item.item_kind === "collection" && item.children.length > 0
      ? item.children.filter((child) => child.status === "resolved" && child.item_kind === "item").map((child) => child.id)
      : [item.id]
  ));
}

export function mergeWorkshopList(
  settings: SettingsObject,
  key: string,
  ids: string[]
): { value: string; addedIds: string[] } {
  const current = parseWorkshopIdList(settings[key]);
  const currentSet = new Set(current);
  const next = uniqueEntries([...current, ...ids]);
  return {
    value: next.join("\n"),
    addedIds: next.filter((id) => !currentSet.has(id))
  };
}

export function mergeTextList(
  settings: SettingsObject,
  key: string,
  values: string[],
  parser: (value: unknown) => string[] = parseDelimitedEntries
): { value: string; addedValues: string[] } {
  const current = parser(settings[key]);
  const currentSet = new Set(current.map((entry) => entry.toLowerCase()));
  const next = uniqueEntries([...current, ...values]);
  return {
    value: next.join("\n"),
    addedValues: next.filter((entry) => !currentSet.has(entry.toLowerCase()))
  };
}

export function mergeProjectZomboidMapList(
  settings: SettingsObject,
  key: string,
  mapIds: string[]
): { value: string; addedValues: string[] } {
  const current = parseLineOrSemicolonEntries(settings[key]);
  const currentSet = new Set(current.map((entry) => entry.toLowerCase()));
  const addedValues = uniqueEntries(mapIds).filter((entry) => !currentSet.has(entry.toLowerCase()));
  return {
    value: uniqueEntries([...addedValues, ...current]).join("\n"),
    addedValues
  };
}

export function removeTextListValues(
  settings: SettingsObject,
  key: string,
  values: string[],
  parser: (value: unknown) => string[] = parseDelimitedEntries
): { value: string; removedValues: string[] } {
  const removeSet = new Set(values.map((entry) => entry.trim().toLowerCase()).filter(Boolean));
  const current = parser(settings[key]);
  const removedValues: string[] = [];
  const next = current.filter((entry) => {
    if (removeSet.has(entry.toLowerCase())) {
      removedValues.push(entry);
      return false;
    }
    return true;
  });
  return {
    value: next.join("\n"),
    removedValues
  };
}

export function removeWorkshopListIds(
  settings: SettingsObject,
  key: string,
  ids: string[]
): { value: string; removedIds: string[] } {
  const removeSet = new Set(ids);
  const current = parseWorkshopIdList(settings[key]);
  const removedIds: string[] = [];
  const next = current.filter((id) => {
    if (removeSet.has(id)) {
      removedIds.push(id);
      return false;
    }
    return true;
  });
  return {
    value: next.join("\n"),
    removedIds
  };
}

export function formatEntryKind(kind: ModEntryKind): string {
  switch (kind) {
    case "steam-item":
      return "Workshop item";
    case "steam-collection":
      return "Collection";
    case "game-mod":
      return "Enabled mod";
    case "map":
      return "Map";
    case "native":
      return "Game setting";
    case "manual":
      return "Manual";
  }
}

export function formatBrowseSortLabel(sort: SteamWorkshopBrowseSort): string {
  switch (sort) {
    case "relevance":
      return "Relevance";
    case "trend":
      return "Trending";
    case "popular":
      return "Top rated";
    case "recent":
      return "Newest";
    case "subscribers":
      return "Most subscribed";
  }
}

export function formatCompactCount(locale: string, value: number | null | undefined): string {
  return typeof value === "number" && Number.isFinite(value)
    ? new Intl.NumberFormat(locale, { notation: "compact", maximumFractionDigits: 1 }).format(value)
    : "";
}

export function instanceBlocksModChanges(details: { active_run?: unknown; summary: { status: string } }): boolean {
  return Boolean(details.active_run) || ["starting", "running", "stopping"].includes(
    details.summary.status.trim().toLowerCase()
  );
}
