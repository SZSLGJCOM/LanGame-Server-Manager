import type { SteamWorkshopInstallationSnapshot, SteamWorkshopLookupItem } from "../../types";
import { isClientOnlyDstWorkshopItem } from "./mod-workbench-model";

const MAX_MANIFEST_BYTES = 1024 * 1024;
const MAX_MANIFEST_ITEMS = 8192;
const LOOKUP_BATCH_SIZE = 64;
const LOOKUP_CONCURRENCY = 4;
const MAX_COLLECTION_DEPTH = 32;
const MAX_WORKSHOP_ID = 18446744073709551615n;

export interface ParsedWorkshopManifest {
  ids: string[];
  invalidTokens: string[];
  duplicateCount: number;
  tooLarge: boolean;
}

export type WorkshopManifestIssueReason =
  | "unresolved"
  | "wrong-game"
  | "unsupported"
  | "client-only"
  | "empty-collection";

export interface WorkshopManifestIssue {
  id: string;
  reason: WorkshopManifestIssueReason;
}

export interface WorkshopManifestReview {
  ids: string[];
  items: Record<string, SteamWorkshopLookupItem>;
  contentIds: string[];
  installedIds: string[];
  missingIds: string[];
  skippedClientOnlyIds: string[];
  issues: WorkshopManifestIssue[];
  searchedRoots: string[];
}

export interface WorkshopManifestDependencies {
  lookup: (ids: string[]) => Promise<SteamWorkshopLookupItem[]>;
  inspect: (ids: string[]) => Promise<SteamWorkshopInstallationSnapshot>;
  isCurrent: () => boolean;
}

function validWorkshopId(value: string): boolean {
  return /^[1-9]\d{5,19}$/.test(value) && BigInt(value) <= MAX_WORKSHOP_ID;
}

function tokenWorkshopId(token: string): string | null {
  const plainId = token.replace(/^workshop-/i, "");
  if (validWorkshopId(plainId)) return plainId;
  const urlText = /^(?:www\.)?steamcommunity\.com\//i.test(token) ? `https://${token}` : token;
  let url: URL;
  try {
    url = new URL(urlText);
  } catch {
    return null;
  }
  if (!["https:", "http:"].includes(url.protocol) ||
      !["steamcommunity.com", "www.steamcommunity.com"].includes(url.hostname.toLowerCase()) ||
      url.username || url.password || url.port ||
      !/^\/(?:sharedfiles|workshop)\/filedetails\/?$/.test(url.pathname)) {
    return null;
  }
  const ids = url.searchParams.getAll("id");
  return ids.length === 1 && validWorkshopId(ids[0]) ? ids[0] : null;
}

export function parseWorkshopManifest(text: string): ParsedWorkshopManifest {
  const result: ParsedWorkshopManifest = { ids: [], invalidTokens: [], duplicateCount: 0, tooLarge: false };
  if (text.length > MAX_MANIFEST_BYTES || new TextEncoder().encode(text).length > MAX_MANIFEST_BYTES) {
    result.tooLarge = true;
    return result;
  }
  const seen = new Set<string>();
  for (const match of text.matchAll(/[^\s,;，；]+/g)) {
    const token = match[0];
    const id = tokenWorkshopId(token);
    if (!id) {
      result.invalidTokens.push(token);
    } else if (seen.has(id)) {
      result.duplicateCount += 1;
    } else {
      if (seen.size === MAX_MANIFEST_ITEMS) {
        result.tooLarge = true;
        break;
      }
      seen.add(id);
      result.ids.push(id);
    }
  }
  return result;
}

function ensureCurrent(dependencies: WorkshopManifestDependencies): void {
  if (!dependencies.isCurrent()) {
    throw new DOMException("Workshop manifest review was superseded", "AbortError");
  }
}

async function lookupBatches(ids: string[], dependencies: WorkshopManifestDependencies): Promise<SteamWorkshopLookupItem[]> {
  const batchCount = Math.ceil(ids.length / LOOKUP_BATCH_SIZE);
  const results: SteamWorkshopLookupItem[][] = new Array(batchCount);
  let nextBatch = 0;
  let stopped = false;
  async function worker(): Promise<void> {
    try {
      while (!stopped && nextBatch < batchCount) {
        ensureCurrent(dependencies);
        const batchIndex = nextBatch++;
        const batch = ids.slice(batchIndex * LOOKUP_BATCH_SIZE, (batchIndex + 1) * LOOKUP_BATCH_SIZE);
        const items = await dependencies.lookup(batch);
        ensureCurrent(dependencies);
        const requested = new Set(batch);
        results[batchIndex] = items.filter((item) => requested.has(item.id));
      }
    } catch (error) {
      stopped = true;
      throw error;
    }
  }
  await Promise.all(Array.from({ length: Math.min(LOOKUP_CONCURRENCY, batchCount) }, worker));
  ensureCurrent(dependencies);
  return results.flat();
}

function itemIssue(item: SteamWorkshopLookupItem | undefined, appId: number): WorkshopManifestIssueReason | null {
  if (!item) return "unresolved";
  if (item.status === "unsupported") return "unsupported";
  if (item.status !== "resolved") return "unresolved";
  if (!["item", "collection"].includes(item.item_kind)) return "unsupported";
  if (typeof item.consumer_app_id !== "number") return "unresolved";
  if (item.consumer_app_id !== appId) return "wrong-game";
  if (isClientOnlyDstWorkshopItem(item, appId)) return "client-only";
  return null;
}

export async function inspectWorkshopManifest(
  ids: string[],
  appId: number,
  dependencies: WorkshopManifestDependencies
): Promise<WorkshopManifestReview> {
  if (!Number.isSafeInteger(appId) || appId <= 0) throw new RangeError("A valid Steam app ID is required");
  const manifestIds = [...new Set(ids)];
  const explicitIds = new Set(manifestIds);
  if (manifestIds.length > MAX_MANIFEST_ITEMS || manifestIds.some((id) => !validWorkshopId(id))) {
    throw new RangeError("Workshop manifest IDs are invalid or exceed the 8192-item limit");
  }
  ensureCurrent(dependencies);
  const items: Record<string, SteamWorkshopLookupItem> = {};
  const issues: WorkshopManifestIssue[] = [];
  const issueIds = new Set<string>();
  const content = new Set<string>();
  const skippedClientOnly = new Set<string>();
  const collectionIds: string[] = [];
  const parents = new Map<string, Set<string>>();
  const seen = new Set(manifestIds);
  let pending = manifestIds;
  let depth = 0;
  const addIssue = (id: string, reason: WorkshopManifestIssueReason) => {
    if (!issueIds.has(id)) {
      issueIds.add(id);
      issues.push({ id, reason });
    }
  };
  while (pending.length > 0) {
    if (depth++ === MAX_COLLECTION_DEPTH) throw new RangeError("Workshop collections exceed the 32-level inspection limit");
    for (const item of await lookupBatches(pending, dependencies)) items[item.id] = item;
    const next: string[] = [];
    for (const id of pending) {
      const item = items[id];
      const reason = itemIssue(item, appId);
      if (reason) {
        if (reason === "client-only" && item.item_kind === "item" && !explicitIds.has(id)) {
          skippedClientOnly.add(id);
        } else addIssue(id, reason);
        continue;
      }
      if (item.item_kind === "item") {
        content.add(id);
        continue;
      }
      collectionIds.push(id);
      if (item.children.length === 0) addIssue(id, "empty-collection");
      if (item.child_count > item.children.length) addIssue(id, "unresolved");
      if (item.children.length > MAX_MANIFEST_ITEMS) throw new RangeError("Workshop collection exceeds the 8192-item inspection limit");
      for (const child of item.children) {
        const childParents = parents.get(child.id) ?? new Set<string>();
        childParents.add(id);
        parents.set(child.id, childParents);
        if (!validWorkshopId(child.id)) {
          addIssue(child.id, "unresolved");
        } else if (!seen.has(child.id)) {
          if (seen.size === MAX_MANIFEST_ITEMS) throw new RangeError("Workshop collection expansion exceeds the 8192-item inspection limit");
          seen.add(child.id);
          next.push(child.id);
        }
      }
    }
    pending = next;
  }
  // Walk reverse edges once so a content-free cycle is rejected even when
  // another manifest entry has valid content. Existing child issues retain
  // their precise diagnosis instead of also labelling their parents empty.
  const accounted = new Set([...content, ...issueIds]);
  const accountedQueue = [...accounted];
  for (let index = 0; index < accountedQueue.length; index += 1) {
    for (const parent of parents.get(accountedQueue[index]) ?? []) {
      if (!accounted.has(parent)) {
        accounted.add(parent);
        accountedQueue.push(parent);
      }
    }
  }
  const clientOnlyBranches = new Set(skippedClientOnly);
  const clientQueue = [...clientOnlyBranches];
  for (let index = 0; index < clientQueue.length; index += 1) {
    for (const parent of parents.get(clientQueue[index]) ?? []) {
      if (!clientOnlyBranches.has(parent)) {
        clientOnlyBranches.add(parent);
        clientQueue.push(parent);
      }
    }
  }
  for (const id of collectionIds) {
    if (!accounted.has(id) && (explicitIds.has(id) || !clientOnlyBranches.has(id))) addIssue(id, "empty-collection");
  }
  // Fetch breadth-first for bounded concurrency, then restore manifest load
  // order by expanding each collection in the order Steam supplied its children.
  const contentIds: string[] = [];
  const visited = new Set<string>();
  const collections = new Set(collectionIds);
  const ordered = [...manifestIds].reverse();
  while (ordered.length > 0) {
    const id = ordered.pop();
    if (id === undefined || visited.has(id)) continue;
    visited.add(id);
    if (content.has(id)) contentIds.push(id);
    else if (collections.has(id)) {
      const children = items[id].children;
      for (let index = children.length - 1; index >= 0; index -= 1) ordered.push(children[index].id);
    }
  }
  ensureCurrent(dependencies);
  const snapshot = await dependencies.inspect(contentIds);
  ensureCurrent(dependencies);
  if (snapshot.consumer_app_id !== appId) throw new Error("Workshop inventory belongs to a different Steam app");
  const installed = new Set(snapshot.items.filter((item) => item.installed === true).map((item) => item.item_id));
  return {
    ids: manifestIds,
    items,
    contentIds,
    installedIds: contentIds.filter((id) => installed.has(id)),
    missingIds: contentIds.filter((id) => !installed.has(id)),
    skippedClientOnlyIds: [...skippedClientOnly],
    issues,
    searchedRoots: [...snapshot.searched_roots]
  };
}
