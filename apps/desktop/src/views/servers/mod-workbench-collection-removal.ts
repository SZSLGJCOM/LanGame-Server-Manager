import type { ManualModInventoryResult, ProjectZomboidWorkshopModsSnapshot } from "../../types";
import { parseWorkshopIdList } from "../settings/guided-setting-values";
import type { SettingsObject } from "../settings/settings-schema";
import { hasDstRawModOverrides } from "./mod-workbench-dst-policy";
import { buildModSettingsRemovePlan } from "./mod-workbench-plans";
import { buildProjectZomboidWorkshopRemovalPlan, WorkshopControlError } from "./mod-workbench-workshop-controls";
import { buildPalworldWorkshopPlan, readRemovedWorkshopIds, WorkshopInventoryControlError } from "./mod-workbench-workshop-inventory";
import {
  mergeManagedWorkshopCollections, readManagedWorkshopCollections, removeManagedWorkshopCollection,
  type ManagedWorkshopCollection
} from "./mod-workbench-collections";

export type CollectionRemovalErrorCode =
  | "collection-not-found" | "invalid-records" | "missing-member-snapshot" | "unknown-collection-members"
  | "invalid-selection" | "shared-member" | "raw-overrides" | "local-metadata-missing" | "unsupported-module";

export class CollectionRemovalError extends Error {
  constructor(public readonly code: CollectionRemovalErrorCode, public readonly ids: readonly string[] = []) {
    super(`Workshop collection removal: ${code}${ids.length ? ` (${ids.join(", ")})` : ""}`);
    this.name = "CollectionRemovalError";
  }
}

export interface WorkshopCollectionRemovalInput {
  settings: SettingsObject;
  moduleId: string;
  collectionId: string;
}

export interface WorkshopCollectionRemovalPreview {
  collection: ManagedWorkshopCollection;
  memberIds: string[];
  removableMemberIds: string[];
  protectedMembers: Array<{ id: string; collectionIds: string[] }>;
  unknownCollectionIds: string[];
  memberRemovalBlock: CollectionRemovalErrorCode | null;
}

export interface WorkshopCollectionRemovalPlanInput extends WorkshopCollectionRemovalInput {
  selectedMemberIds: readonly string[];
  /** Complete, freshly read metadata for selected and configured Workshop IDs. */
  pzSnapshot?: ProjectZomboidWorkshopModsSnapshot | null;
  /** Complete inventory of this instance, not a filtered selection or shared cache. */
  manualInventory?: ManualModInventoryResult | null;
}

export interface WorkshopCollectionRemovalPlan {
  nextSettings: SettingsObject;
  removedMemberIds: string[];
  fileRemovalIds: string[];
  retainedLocalIds: string[];
}

export function buildWorkshopCollectionRemovalPreview(input: WorkshopCollectionRemovalInput): WorkshopCollectionRemovalPreview {
  try {
    // Do not derive destructive ownership from a partly decoded invalid record list.
    mergeManagedWorkshopCollections(input.settings, []);
  } catch {
    throw new CollectionRemovalError("invalid-records");
  }
  const records = readManagedWorkshopCollections(input.settings, input.moduleId);
  const collection = records.find((entry) => entry.id === input.collectionId);
  if (!collection) throw new CollectionRemovalError("collection-not-found", [input.collectionId]);
  const others = records.filter((entry) => entry.id !== collection.id);
  const unknownCollectionIds = others.filter((entry) => entry.member_ids.length === 0).map((entry) => entry.id);
  const references = new Map<string, string[]>();
  for (const other of others) {
    for (const id of other.member_ids) references.set(id, [...(references.get(id) ?? []), other.id]);
  }
  const memberIds = [...collection.member_ids];
  const protectedMembers = memberIds.filter((id) => references.has(id))
    .map((id) => ({ id, collectionIds: references.get(id)! }));
  const memberRemovalBlock: CollectionRemovalErrorCode | null = memberIds.length === 0 ? "missing-member-snapshot"
    : unknownCollectionIds.length ? "unknown-collection-members"
    : input.moduleId === "dontstarve" && hasDstRawModOverrides(input.settings) ? "raw-overrides" : null;
  return {
    collection, memberIds, protectedMembers, unknownCollectionIds, memberRemovalBlock,
    removableMemberIds: memberRemovalBlock ? [] : memberIds.filter((id) => !references.has(id))
  };
}

export function buildWorkshopCollectionRemovalPlan(input: WorkshopCollectionRemovalPlanInput): WorkshopCollectionRemovalPlan {
  const preview = buildWorkshopCollectionRemovalPreview(input);
  const ids = [...new Set(input.selectedMemberIds)];
  const known = new Set(preview.memberIds);
  if (ids.some((id) => !known.has(id))) throw new CollectionRemovalError("invalid-selection", ids.filter((id) => !known.has(id)));
  if (ids.length && preview.memberRemovalBlock) throw new CollectionRemovalError(preview.memberRemovalBlock, preview.unknownCollectionIds);
  const shared = new Set(preview.protectedMembers.map((entry) => entry.id));
  if (ids.some((id) => shared.has(id))) throw new CollectionRemovalError("shared-member", ids.filter((id) => shared.has(id)));
  let next = removeManagedWorkshopCollection(input.settings, input.moduleId, input.collectionId);
  let retainedLocalIds: string[] = [];
  try {
    if (ids.length) {
      if (input.moduleId === "projectzomboid") {
        const configured = new Set(parseWorkshopIdList(input.settings.workshop_items));
        const owned = ids.filter((id) => configured.has(id));
        if (owned.length) {
          const plan = buildProjectZomboidWorkshopRemovalPlan(next, owned, input.pzSnapshot ?? null);
          next = plan.nextSettings;
          retainedLocalIds = plan.retainedLocalIds;
        }
      } else if (input.moduleId === "palworld") {
        const removed = new Set(readRemovedWorkshopIds(input.settings));
        const owned = ids.filter((id) => !removed.has(id));
        if (owned.length) {
          const plan = buildPalworldWorkshopPlan(input.settings, input.manualInventory ?? null, owned, "remove");
          next = { ...plan.nextSettings, steam_workshop_collections: next.steam_workshop_collections };
          retainedLocalIds = plan.retainedLocalIds;
        }
      } else if (input.moduleId !== "squad") {
        if (!["dontstarve", "unturned", "arksurvivalevolved", "barotrauma", "conanexiles", "soulmask", "terraria"].includes(input.moduleId)) {
          throw new CollectionRemovalError("unsupported-module");
        }
        if (input.moduleId === "terraria" && typeof input.settings.tmodloader_enabled_mod_names === "string" &&
            input.settings.tmodloader_enabled_mod_names.split(/\r?\n/).some((line) => line.trim() && !/^(#|--)/.test(line.trim()))) {
          // Inventory exposes Workshop directories, not the internal names stored in enabled.json.
          throw new CollectionRemovalError("local-metadata-missing", ids);
        }
        const sourceOnly = next;
        const removal = buildModSettingsRemovePlan(input.moduleId, next, ids, {}, null, null, preview.memberIds);
        if (removal.summaryKey === "servers.mods.removeSummary.ownershipLimit") throw new CollectionRemovalError("invalid-records");
        next = removal.nextSettings ?? next;
        if (input.moduleId === "dontstarve") {
          // Member removal must not unlink another native collection whose ID
          // happens to match one of the target's saved members.
          if (Object.prototype.hasOwnProperty.call(sourceOnly, "shared_workshop_collection_ids")) {
            next.shared_workshop_collection_ids = sourceOnly.shared_workshop_collection_ids;
          } else delete next.shared_workshop_collection_ids;
        }
      }
    }
  } catch (error) {
    if (!(error instanceof WorkshopControlError) && !(error instanceof WorkshopInventoryControlError)) throw error;
    const code = error.code === "ownership-limit" || error.code === "invalid-settings" ? "invalid-records"
      : error.code === "not-owned" ? input.moduleId === "palworld" ? "local-metadata-missing" : "invalid-selection" : error.code;
    throw new CollectionRemovalError(code, error.ids);
  }
  return { nextSettings: next, removedMemberIds: ids, fileRemovalIds: input.moduleId === "squad" ? ids : [], retainedLocalIds };
}
