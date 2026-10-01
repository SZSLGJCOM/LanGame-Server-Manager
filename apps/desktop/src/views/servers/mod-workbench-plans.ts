import type { ProjectZomboidWorkshopModsSnapshot, SteamWorkshopLookupItem } from "../../types";
import type { SettingsObject } from "../settings/settings-schema";
import { parseWorkshopIdList } from "../settings/guided-setting-values";
import { DST_RAW_MOD_WARNING, hasDstRawModOverrides } from "./mod-workbench-dst-policy";
import { DONTSTARVE_SHARDS, getDontStarveLayoutShards } from "../settings/modules/dontstarve-shards";
import { buildProjectZomboidWorkshopRemovalPlan } from "./mod-workbench-workshop-controls";
import {
  buildConfigurableEntries,
  buildConfiguredEntries,
  isCanonicalWorkshopModId,
  mergeProjectZomboidMapList,
  mergeTextList,
  mergeWorkshopList,
  isUnsupportedWorkshopItem,
  isIncompleteWorkshopCollection,
  parseDelimitedEntries,
  parseLineOrSemicolonEntries,
  readDstRemovedWorkshopModIds,
  readDisabledWorkshopModIds,
  removeTextListValues,
  removeWorkshopListIds,
  serverWorkshopCollectionChildren,
  steamItemBelongsToApp,
  uniqueEntries
} from "./mod-workbench-model";

export interface ModSettingsApplyPlan {
  canApply: boolean;
  actionKey: string;
  actionFallback: string;
  summaryKey: string;
  summaryFallback: string;
  params: Record<string, string | number | boolean>;
  nextSettings: SettingsObject | null;
  addedIds: string[];
}

export type WorkshopInstallPurpose = "enable" | "prepare";

export interface ModSettingsRemovePlan {
  canRemove: boolean;
  summaryKey: string;
  summaryFallback: string;
  params: Record<string, string | number | boolean>;
  nextSettings: SettingsObject | null;
  removedIds: string[];
}

function emptyRemovePlan(summaryKey: string, summaryFallback: string): ModSettingsRemovePlan {
  return {
    canRemove: false,
    summaryKey,
    summaryFallback,
    params: {},
    nextSettings: null,
    removedIds: []
  };
}

function selectedWorkshopBuckets(
  selectedIds: string[],
  lookupMap: Record<string, SteamWorkshopLookupItem>,
  expectedAppId: number | null,
  removalCollectionIds?: ReadonlySet<string>
) {
  const forRemoval = removalCollectionIds !== undefined;
  const itemIds: string[] = [];
  const collectionIds: string[] = [];
  const blockedIds: string[] = [];

  for (const id of selectedIds) {
    const item = lookupMap[id];
    if (!forRemoval && (isUnsupportedWorkshopItem(item, expectedAppId) || !steamItemBelongsToApp(item, expectedAppId))) {
      blockedIds.push(id);
      continue;
    }
    if (item?.item_kind === "collection" && (!forRemoval || removalCollectionIds.has(id))) {
      if (!forRemoval) {
        const children = serverWorkshopCollectionChildren(item, lookupMap, expectedAppId);
        if (!children?.length) {
          blockedIds.push(id);
          continue;
        }
        itemIds.push(...children.map((child) => child.id));
      } else {
        itemIds.push(id,
          ...item.children
            .filter((child) => child.status === "resolved" && child.item_kind === "item" &&
              (typeof expectedAppId !== "number" || typeof child.consumer_app_id !== "number" || child.consumer_app_id === expectedAppId))
            .map((child) => child.id)
        );
      }
      collectionIds.push(id);
      continue;
    }
    itemIds.push(id);
  }

  return {
    itemIds: uniqueEntries(itemIds),
    collectionIds: uniqueEntries(collectionIds),
    blockedIds: uniqueEntries(blockedIds)
  };
}

function emptyApplyPlan(summaryKey: string, summaryFallback: string): ModSettingsApplyPlan {
  return {
    canApply: false,
    actionKey: "servers.mods.applyAction.addToInstance",
    actionFallback: "Add to instance",
    summaryKey,
    summaryFallback,
    params: {},
    nextSettings: null,
    addedIds: []
  };
}

export function buildDstModEnablementPlan(
  settings: SettingsObject,
  selectedIds: readonly string[],
  enabled: boolean,
  ownedMemberIds: readonly string[] = []
): SettingsObject | null {
  if (!selectedIds.length || hasDstRawModOverrides(settings) || !selectedIds.every(isCanonicalWorkshopModId)) return null;
  const ids = uniqueEntries([...selectedIds]);
  const ownedIds = new Set(buildConfigurableEntries("dontstarve",
    buildConfiguredEntries("dontstarve", settings, undefined, ownedMemberIds)).flatMap((entry) => entry.ids));
  if (ids.some((id) => !ownedIds.has(id))) return null;

  // Both views control the same instance-owned Mods. A batch is one settings
  // update, and disabling retains ownership and each shard's saved options.
  const nextSettings: SettingsObject = { ...settings };
  nextSettings.shared_workshop_mod_ids = mergeWorkshopList(settings, "shared_workshop_mod_ids", ids).value;
  for (const shard of getDontStarveLayoutShards(settings)) {
    const key = `${shard}_enabled_workshop_mod_ids`;
    nextSettings[key] = enabled ? mergeWorkshopList(settings, key, ids).value : removeWorkshopListIds(settings, key, ids).value;
  }
  const restored = new Set(ids);
  const remainingRemovedIds = readDstRemovedWorkshopModIds(settings).filter((id) => !restored.has(id));
  if (remainingRemovedIds.length) nextSettings.dst_removed_workshop_mod_ids = remainingRemovedIds;
  else delete nextSettings.dst_removed_workshop_mod_ids;
  return nextSettings;
}

export function buildModSettingsApplyPlan(
  moduleId: string,
  settings: SettingsObject,
  selectedIds: string[],
  lookupMap: Record<string, SteamWorkshopLookupItem>,
  expectedAppId: number | null,
  purpose: WorkshopInstallPurpose = "enable"
): ModSettingsApplyPlan {
  if (purpose === "prepare") {
    return emptyApplyPlan("servers.mods.prepareOnly", "Download Mod files without changing their enablement or options.");
  }
  if (selectedIds.length === 0) {
    return emptyApplyPlan("servers.mods.applySummary.empty", "Select mods from the shelf first.");
  }
  if (moduleId === "dontstarve" && hasDstRawModOverrides(settings)) {
    return emptyApplyPlan("dst.settings.modStatus.rawOverrideWarning", DST_RAW_MOD_WARNING);
  }
  if (selectedIds.some((id) => isIncompleteWorkshopCollection(lookupMap[id]))) {
    return emptyApplyPlan("servers.mods.incompleteCollection", "This collection contains nested or unresolved entries. Add the individual Mods instead.");
  }

  const buckets = selectedWorkshopBuckets(selectedIds, lookupMap, expectedAppId);
  const nextSettings: SettingsObject = { ...settings };
  const addedIds: string[] = [];

  switch (moduleId) {
    case "dontstarve": {
      const sharedMods = mergeWorkshopList(nextSettings, "shared_workshop_mod_ids", buckets.itemIds);
      nextSettings.shared_workshop_mod_ids = sharedMods.value;
      // Managed collections use their reviewed server members. A native collection
      // request would expand them again and download the skipped client-only Mods.
      // Preserve explicit native requests already present in the user's settings.
      addedIds.push(...sharedMods.addedIds);
      for (const shard of getDontStarveLayoutShards(settings)) {
        const key = `${shard}_enabled_workshop_mod_ids`;
        const enabled = mergeWorkshopList(nextSettings, key, buckets.itemIds);
        nextSettings[key] = enabled.value;
        addedIds.push(...enabled.addedIds);
      }
      const restoredIds = new Set(buckets.itemIds);
      const removedIds = readDstRemovedWorkshopModIds(settings);
      const remainingRemovedIds = removedIds.filter((id) => !restoredIds.has(id));
      if (remainingRemovedIds.length) nextSettings.dst_removed_workshop_mod_ids = remainingRemovedIds;
      else delete nextSettings.dst_removed_workshop_mod_ids;
      addedIds.push(...removedIds.filter((id) => restoredIds.has(id)));
      break;
    }
    case "projectzomboid": {
      const workshopItems = mergeWorkshopList(nextSettings, "workshop_items", buckets.itemIds);
      nextSettings.workshop_items = workshopItems.value;
      addedIds.push(...workshopItems.addedIds);
      break;
    }
    case "unturned": {
      const fileIds = mergeWorkshopList(nextSettings, "workshop_file_ids", buckets.itemIds);
      nextSettings.workshop_file_ids = fileIds.value;
      addedIds.push(...fileIds.addedIds);
      break;
    }
    case "arksurvivalevolved": {
      const activeMods = mergeTextList(nextSettings, "active_mod_ids", buckets.itemIds, parseDelimitedEntries);
      const managedMods = mergeTextList(nextSettings, "auto_managed_mod_ids", buckets.itemIds, parseDelimitedEntries);
      nextSettings.active_mod_ids = activeMods.value;
      nextSettings.auto_managed_mod_ids = managedMods.value;
      if (buckets.itemIds.length > 0) {
        if (nextSettings.auto_managed_mods !== true) addedIds.push(...buckets.itemIds);
        nextSettings.auto_managed_mods = true;
      }
      addedIds.push(...activeMods.addedValues, ...managedMods.addedValues);
      break;
    }
    case "barotrauma":
    case "conanexiles":
    case "soulmask": {
      const modWorkshopIds = mergeWorkshopList(nextSettings, "mod_workshop_ids", buckets.itemIds);
      nextSettings.mod_workshop_ids = modWorkshopIds.value;
      addedIds.push(...modWorkshopIds.addedIds);
      const selected = new Set(buckets.itemIds);
      const disabled = readDisabledWorkshopModIds(settings);
      const remaining = disabled.filter((id) => !selected.has(id));
      if (remaining.length) nextSettings.steam_workshop_disabled_mod_ids = remaining;
      else delete nextSettings.steam_workshop_disabled_mod_ids;
      addedIds.push(...disabled.filter((id) => selected.has(id)));
      break;
    }
    case "terraria": {
      const workshopItems = mergeWorkshopList(nextSettings, "tmodloader_workshop_item_ids", buckets.itemIds);
      nextSettings.tmodloader_workshop_item_ids = workshopItems.value;
      addedIds.push(...workshopItems.addedIds);
      if (typeof nextSettings.server_runtime === "undefined" || String(nextSettings.server_runtime).trim() === "") {
        nextSettings.server_runtime = "tmodloader";
      }
      break;
    }
    default:
      return emptyApplyPlan(
        "servers.mods.applySummary.unsupported",
        "This module does not own a safe settings write path for selected Workshop items yet."
      );
  }

  const uniqueAddedIds = uniqueEntries(addedIds);
  if (uniqueAddedIds.length === 0) {
    return emptyApplyPlan("servers.mods.applySummary.alreadyApplied", "The selected items are already in this instance.");
  }

  const planByModule: Record<string, Pick<ModSettingsApplyPlan, "actionKey" | "actionFallback" | "summaryKey" | "summaryFallback">> = {
    dontstarve: {
      actionKey: "servers.mods.applyAction.addEnable",
      actionFallback: "Add and enable",
      summaryKey: "servers.mods.applySummary.dontstarve",
      summaryFallback: "Will add downloads and enable compatible Workshop items for this DST instance."
    },
    projectzomboid: {
      actionKey: "servers.mods.applyAction.addWorkshopIds",
      actionFallback: "Add Workshop IDs",
      summaryKey: "servers.mods.applySummary.projectzomboid",
      summaryFallback: "Will add WorkshopItems. Download and scan afterward to confirm internal Mod IDs and maps."
    },
    unturned: {
      actionKey: "servers.mods.applyAction.addFileIds",
      actionFallback: "Add file IDs",
      summaryKey: "servers.mods.applySummary.unturned",
      summaryFallback: "Will add Workshop file IDs for this Unturned ServerID."
    },
    arksurvivalevolved: {
      actionKey: "servers.mods.applyAction.addEnable",
      actionFallback: "Add and enable",
      summaryKey: "servers.mods.applySummary.arksurvivalevolved",
      summaryFallback: "Will write Steam Workshop IDs to ActiveMods for this ARK: Survival Evolved instance."
    },
    barotrauma: {
      actionKey: "servers.mods.applyAction.addWorkshopIds",
      actionFallback: "Add Workshop IDs",
      summaryKey: "servers.mods.applySummary.barotrauma",
      summaryFallback: "Will add Workshop IDs to Barotrauma content package materialization."
    },
    conanexiles: {
      actionKey: "servers.mods.applyAction.addWorkshopIds",
      actionFallback: "Add Workshop IDs",
      summaryKey: "servers.mods.applySummary.conanexiles",
      summaryFallback: "Will add Workshop IDs to Conan Exiles modlist materialization."
    },
    soulmask: {
      actionKey: "servers.mods.applyAction.addWorkshopIds",
      actionFallback: "Add Workshop IDs",
      summaryKey: "servers.mods.applySummary.soulmask",
      summaryFallback: "Will add Workshop IDs for Soulmask -mod startup."
    },
    terraria: {
      actionKey: "servers.mods.applyAction.addWorkshopIds",
      actionFallback: "Add Workshop IDs",
      summaryKey: "servers.mods.applySummary.terraria",
      summaryFallback: "Will write tModLoader Workshop IDs to Mods/install.txt for this instance."
    }
  };

  return {
    canApply: true,
    ...planByModule[moduleId],
    params: { count: uniqueAddedIds.length, blocked: buckets.blockedIds.length },
    nextSettings,
    addedIds: uniqueAddedIds
  };
}

export function collectProjectZomboidLocalIds(
  snapshot: ProjectZomboidWorkshopModsSnapshot | null,
  workshopItemIds: string[]
) {
  const targetIds = new Set(workshopItemIds);
  const modIds: string[] = [];
  const mapIds: string[] = [];

  for (const item of snapshot?.items ?? []) {
    if (!targetIds.has(item.workshop_item_id)) {
      continue;
    }
    for (const mod of item.mods) {
      if (mod.mod_id) {
        modIds.push(mod.mod_id);
      }
      mapIds.push(...mod.map_ids);
    }
  }

  return {
    modIds: uniqueEntries(modIds),
    mapIds: uniqueEntries(mapIds)
  };
}

export function buildProjectZomboidEnablePlan(
  settings: SettingsObject,
  items: ProjectZomboidWorkshopModsSnapshot["items"]
): ModSettingsApplyPlan {
  const workshopIds = items.map((item) => item.workshop_item_id);
  const localIds = collectProjectZomboidLocalIds(
    { workshop_root: "", workshop_root_exists: true, items },
    workshopIds
  );
  const nextSettings: SettingsObject = { ...settings };
  const addedIds: string[] = [];

  const workshopItems = mergeWorkshopList(nextSettings, "workshop_items", workshopIds);
  nextSettings.workshop_items = workshopItems.value;
  addedIds.push(...workshopItems.addedIds);

  if (localIds.modIds.length > 0) {
    const mods = mergeTextList(nextSettings, "mods", localIds.modIds, parseLineOrSemicolonEntries);
    nextSettings.mods = mods.value;
    addedIds.push(...mods.addedValues);
  }

  if (localIds.mapIds.length > 0) {
    const maps = mergeProjectZomboidMapList(nextSettings, "map_name", localIds.mapIds);
    nextSettings.map_name = maps.value;
    addedIds.push(...maps.addedValues);
  }

  const uniqueAddedIds = uniqueEntries(addedIds);
  if (uniqueAddedIds.length === 0) {
    return emptyApplyPlan("servers.mods.applySummary.alreadyApplied", "The selected items are already in this instance.");
  }

  return {
    canApply: true,
    actionKey: "servers.mods.applyAction.enableLocal",
    actionFallback: "Enable",
    summaryKey: "servers.mods.applySummary.projectzomboidLocal",
    summaryFallback: "Project Zomboid Workshop item was enabled from the local scan.",
    params: { count: uniqueAddedIds.length },
    nextSettings,
    addedIds: uniqueAddedIds
  };
}

export function buildModSettingsRemovePlan(
  moduleId: string,
  settings: SettingsObject,
  selectedIds: string[],
  lookupMap: Record<string, SteamWorkshopLookupItem>,
  expectedAppId: number | null,
  pzSnapshot: ProjectZomboidWorkshopModsSnapshot | null,
  ownedMemberIds: readonly string[] = []
): ModSettingsRemovePlan {
  if (selectedIds.length === 0) {
    return emptyRemovePlan("servers.mods.removeSummary.empty", "Select a configured mod first.");
  }
  if (moduleId === "dontstarve" && hasDstRawModOverrides(settings)) {
    return emptyRemovePlan("dst.settings.modStatus.rawOverrideWarning", DST_RAW_MOD_WARNING);
  }

  // Removing existing configuration must remain possible when online metadata
  // becomes private, changes game/type, or no longer describes a server Mod.
  // Only persisted collection ownership authorizes expanding the removal to
  // children. A newly reclassified ordinary item cannot remove independent Mods.
  const collectionIds = moduleId === "dontstarve" ? parseWorkshopIdList(settings.shared_workshop_collection_ids) : [];
  const buckets = selectedWorkshopBuckets(selectedIds, lookupMap, expectedAppId, new Set(collectionIds));
  const nextSettings: SettingsObject = { ...settings };
  const removedIds: string[] = [];

  switch (moduleId) {
    case "dontstarve": {
      const ownedIds = new Set(buildConfigurableEntries("dontstarve",
        buildConfiguredEntries("dontstarve", settings, undefined, ownedMemberIds)).flatMap((entry) => entry.ids));
      const removedMemberIds = buckets.itemIds.filter((id) => isCanonicalWorkshopModId(id) && ownedIds.has(id));
      const markers = uniqueEntries([...readDstRemovedWorkshopModIds(settings), ...removedMemberIds]);
      if (markers.length > 8192) {
        return emptyRemovePlan("servers.mods.removeSummary.ownershipLimit", "Too many removed Mods are recorded. Restore a removed Mod before removing another.");
      }
      for (const key of ["shared_workshop_mod_ids", ...DONTSTARVE_SHARDS.map((shard) => `${shard}_enabled_workshop_mod_ids`)]) {
        const result = removeWorkshopListIds(nextSettings, key, buckets.itemIds);
        nextSettings[key] = result.value;
        removedIds.push(...result.removedIds);
      }
      const collections = removeWorkshopListIds(
        nextSettings,
        "shared_workshop_collection_ids",
        uniqueEntries([...buckets.collectionIds, ...selectedIds])
      );
      nextSettings.shared_workshop_collection_ids = collections.value;
      removedIds.push(...collections.removedIds);
      // Retain options and source snapshots for a later repair. The marker
      // prevents those retained settings from recreating a removed My Mods row.
      if (removedMemberIds.length) nextSettings.dst_removed_workshop_mod_ids = markers;
      removedIds.push(...removedMemberIds);
      break;
    }
    case "projectzomboid": {
      const configured = new Set(parseWorkshopIdList(settings.workshop_items));
      const selected = buckets.itemIds.filter((id) => configured.has(id));
      if (selected.length) {
        Object.assign(nextSettings, buildProjectZomboidWorkshopRemovalPlan(settings, selected, pzSnapshot).nextSettings);
        removedIds.push(...selected);
      }
      break;
    }
    case "unturned": {
      const fileIds = removeWorkshopListIds(nextSettings, "workshop_file_ids", buckets.itemIds);
      nextSettings.workshop_file_ids = fileIds.value;
      removedIds.push(...fileIds.removedIds);
      break;
    }
    case "arksurvivalevolved": {
      const activeMods = removeTextListValues(nextSettings, "active_mod_ids", buckets.itemIds, parseDelimitedEntries);
      const managedMods = removeTextListValues(nextSettings, "auto_managed_mod_ids", buckets.itemIds, parseDelimitedEntries);
      nextSettings.active_mod_ids = activeMods.value;
      nextSettings.auto_managed_mod_ids = managedMods.value;
      removedIds.push(...activeMods.removedValues, ...managedMods.removedValues);
      break;
    }
    case "barotrauma":
    case "conanexiles":
    case "soulmask": {
      const modWorkshopIds = removeWorkshopListIds(nextSettings, "mod_workshop_ids", buckets.itemIds);
      nextSettings.mod_workshop_ids = modWorkshopIds.value;
      removedIds.push(...modWorkshopIds.removedIds);
      const selected = new Set(buckets.itemIds);
      const disabled = readDisabledWorkshopModIds(settings);
      const remaining = disabled.filter((id) => !selected.has(id));
      if (remaining.length) nextSettings.steam_workshop_disabled_mod_ids = remaining;
      else delete nextSettings.steam_workshop_disabled_mod_ids;
      removedIds.push(...disabled.filter((id) => selected.has(id)));
      break;
    }
    case "terraria": {
      const workshopItems = removeWorkshopListIds(nextSettings, "tmodloader_workshop_item_ids", buckets.itemIds);
      nextSettings.tmodloader_workshop_item_ids = workshopItems.value;
      removedIds.push(...workshopItems.removedIds);
      break;
    }
    default:
      return emptyRemovePlan(
        "servers.mods.removeSummary.unsupported",
        "This module does not own a safe mod removal path yet."
      );
  }

  const uniqueRemovedIds = uniqueEntries(removedIds);
  if (uniqueRemovedIds.length === 0) {
    return emptyRemovePlan("servers.mods.removeSummary.notConfigured", "This mod is not configured in the instance.");
  }

  return {
    canRemove: true,
    summaryKey: "servers.mods.removeSummary.success",
    summaryFallback: "Removed {count} mod entry(s) from this instance.",
    params: { count: uniqueRemovedIds.length },
    nextSettings,
    removedIds: uniqueRemovedIds
  };
}
