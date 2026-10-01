import type { SteamWorkshopLookupChild, SteamWorkshopLookupItem } from "../../types";
import { parseWorkshopIdList } from "../settings/guided-setting-values";
import type { SettingsObject } from "../settings/settings-schema";
import { isClientOnlyDstWorkshopItem } from "./mod-workbench-model";

export interface ManagedWorkshopCollection {
  id: string;
  title: string;
  member_ids: string[];
}

export type WorkshopCollectionInstallIssue =
  | "missing" | "unresolved" | "wrong-game" | "unsupported" | "client-only"
  | "incomplete" | "cycle" | "empty-collection";

export class WorkshopCollectionInstallError extends Error {
  readonly code = "workshop-collection-install";

  constructor(readonly reason: WorkshopCollectionInstallIssue, readonly item_id: string) {
    super(JSON.stringify({ code: "workshop-collection-install", reason, item_id,
      message: `Workshop collection member ${item_id}: ${reason}.` }));
    this.name = "WorkshopCollectionInstallError";
  }
}

export function managedCollectionMemberIds(
  collection: ManagedWorkshopCollection,
  lookup?: SteamWorkshopLookupItem
): string[] {
  if (collection.member_ids.length) return collection.member_ids;
  return lookup?.children.filter((child) => child.item_kind === "item" && child.status === "resolved" &&
    child.consumer_app_id === lookup.consumer_app_id && !isClientOnlyDstWorkshopItem(child, lookup.consumer_app_id ?? null))
    .map((child) => child.id) ?? [];
}

const COLLECTIONS_KEY = "steam_workshop_collections";
const MAX_COLLECTIONS = 128;
const MAX_MEMBERS = 8192;
const MAX_TOTAL_MEMBERS = 65536;
const MAX_TITLE_CHARACTERS = 512;
const MAX_COLLECTION_DEPTH = 32;
const MAX_WORKSHOP_ID = 18446744073709551615n;

function isWorkshopId(value: unknown): value is string {
  return typeof value === "string" && /^[1-9]\d{5,19}$/.test(value) && BigInt(value) <= MAX_WORKSHOP_ID;
}

function invalidCollection(): Error {
  return new Error("Workshop collection records are invalid or exceed their limits.");
}

function decodeCollection(value: unknown): ManagedWorkshopCollection | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const record = value as Record<string, unknown>;
  if (Object.keys(record).some((key) => !["id", "title", "member_ids"].includes(key)) ||
      !isWorkshopId(record.id) || typeof record.title !== "string" ||
      record.title.length > MAX_TITLE_CHARACTERS * 2 || [...record.title].length > MAX_TITLE_CHARACTERS ||
      !Array.isArray(record.member_ids) || record.member_ids.length > MAX_MEMBERS ||
      !record.member_ids.every(isWorkshopId)) return null;
  return { id: record.id, title: record.title, member_ids: [...new Set(record.member_ids)] };
}

function decodeCollections(value: unknown, strict: boolean): ManagedWorkshopCollection[] {
  if (value === undefined) return [];
  if (!Array.isArray(value) || value.length > MAX_COLLECTIONS) {
    if (strict) throw invalidCollection();
    return [];
  }
  const records = new Map<string, ManagedWorkshopCollection>();
  let totalMembers = 0;
  for (const entry of value) {
    const record = decodeCollection(entry);
    if (!record) {
      if (strict) throw invalidCollection();
      continue;
    }
    if (records.has(record.id)) continue;
    if (totalMembers + record.member_ids.length > MAX_TOTAL_MEMBERS) {
      if (strict) throw invalidCollection();
      continue;
    }
    records.set(record.id, record);
    totalMembers += record.member_ids.length;
  }
  return [...records.values()];
}

export function readManagedWorkshopCollections(settings: SettingsObject, moduleId: string): ManagedWorkshopCollection[] {
  const records = decodeCollections(settings[COLLECTIONS_KEY], false);
  if (moduleId !== "dontstarve") return records;
  const known = new Set(records.map((record) => record.id));
  // The native DST list is explicit ownership, but contains no historical member snapshot.
  for (const id of parseWorkshopIdList(settings.shared_workshop_collection_ids)) {
    if (records.length === MAX_COLLECTIONS) break;
    if (isWorkshopId(id) && !known.has(id)) {
      records.push({ id, title: id, member_ids: [] });
      known.add(id);
    }
  }
  return records;
}

export function collectInstalledWorkshopCollections(
  rootIds: string[],
  lookupMap: Record<string, SteamWorkshopLookupItem>,
  expectedAppId: number | null
): ManagedWorkshopCollection[] {
  if (!Number.isSafeInteger(expectedAppId) || Number(expectedAppId) <= 0 || rootIds.length > MAX_TOTAL_MEMBERS) {
    throw invalidCollection();
  }
  const records: ManagedWorkshopCollection[] = [];
  let totalMembers = 0;

  function verify(item: SteamWorkshopLookupChild | undefined, id: string): asserts item is SteamWorkshopLookupChild {
    if (!isWorkshopId(id) || !item || item.id !== id || item.status === "not_found") {
      throw new WorkshopCollectionInstallError("missing", id);
    }
    if (item.status === "unsupported") throw new WorkshopCollectionInstallError("unsupported", id);
    if (item.status !== "resolved" || typeof item.consumer_app_id !== "number") {
      throw new WorkshopCollectionInstallError("unresolved", id);
    }
    if (item.consumer_app_id !== expectedAppId) throw new WorkshopCollectionInstallError("wrong-game", id);
    if (!["item", "collection"].includes(item.item_kind)) throw new WorkshopCollectionInstallError("unsupported", id);
  }

  for (const rootId of new Set(rootIds)) {
    const root = lookupMap[rootId];
    verify(root, rootId);
    if (isClientOnlyDstWorkshopItem(root, expectedAppId)) throw new WorkshopCollectionInstallError("client-only", rootId);
    if (root.item_kind !== "collection") continue;
    if (records.length === MAX_COLLECTIONS) throw invalidCollection();
    const members = new Set<string>();
    const active = new Set<string>();
    const visited = new Set<string>();

    function visit(id: string, summary?: SteamWorkshopLookupChild, depth = 0): void {
      // Full lookup results supersede child summaries, including a failed or changed type.
      const item = lookupMap[id] ?? summary;
      verify(item, id);
      if (item.item_kind === "collection" && isClientOnlyDstWorkshopItem(item, expectedAppId)) {
        throw new WorkshopCollectionInstallError("client-only", id);
      }
      if (active.has(id)) throw new WorkshopCollectionInstallError("cycle", id);
      if (visited.has(id)) return;
      if (visited.size === MAX_TOTAL_MEMBERS) throw invalidCollection();
      visited.add(id);
      if (item.item_kind === "item") {
        if (isClientOnlyDstWorkshopItem(item, expectedAppId)) return;
        members.add(id);
        if (members.size > MAX_MEMBERS) throw invalidCollection();
        return;
      }
      const collection = lookupMap[id];
      if (depth >= MAX_COLLECTION_DEPTH || !collection || !Array.isArray(collection.children) ||
          collection.children.length === 0 || collection.children.length > MAX_MEMBERS ||
          !Number.isSafeInteger(collection.child_count) || collection.child_count !== collection.children.length) {
        throw new WorkshopCollectionInstallError("incomplete", id);
      }
      active.add(id);
      for (const child of collection.children) visit(child.id, child, depth + 1);
      active.delete(id);
    }

    visit(rootId);
    if (members.size === 0) throw new WorkshopCollectionInstallError("empty-collection", rootId);
    totalMembers += members.size;
    if (totalMembers > MAX_TOTAL_MEMBERS) throw invalidCollection();
    const title = typeof root.title === "string" && root.title.trim() ? root.title : rootId;
    records.push({ id: rootId, title: [...title].slice(0, MAX_TITLE_CHARACTERS).join(""), member_ids: [...members] });
  }
  return records;
}

export function mergeManagedWorkshopCollections(
  settings: SettingsObject,
  records: ManagedWorkshopCollection[]
): SettingsObject {
  const existing = decodeCollections(settings[COLLECTIONS_KEY], true);
  const incoming = decodeCollections(records, true);
  const merged = new Map(existing.map((record) => [record.id, record]));
  for (const record of incoming) merged.set(record.id, record);
  const next = [...merged.values()];
  if (next.length > MAX_COLLECTIONS || next.reduce((count, record) => count + record.member_ids.length, 0) > MAX_TOTAL_MEMBERS) {
    throw invalidCollection();
  }
  if (JSON.stringify(existing) === JSON.stringify(next) &&
      JSON.stringify(settings[COLLECTIONS_KEY] ?? []) === JSON.stringify(next)) return settings;
  return { ...settings, [COLLECTIONS_KEY]: next };
}

export function removeManagedWorkshopCollection(settings: SettingsObject, moduleId: string, id: string): SettingsObject {
  if (!isWorkshopId(id)) throw invalidCollection();
  const existing = decodeCollections(settings[COLLECTIONS_KEY], true);
  const next: SettingsObject = { ...settings };
  if (existing.some((record) => record.id === id)) {
    next[COLLECTIONS_KEY] = existing.filter((record) => record.id !== id);
  }
  if (moduleId === "dontstarve") {
    const nativeIds = parseWorkshopIdList(settings.shared_workshop_collection_ids);
    if (nativeIds.includes(id)) next.shared_workshop_collection_ids = nativeIds.filter((entry) => entry !== id).join("\n");
  }
  // Deliberately leave member enablement, per-Mod options and files untouched.
  return next;
}
