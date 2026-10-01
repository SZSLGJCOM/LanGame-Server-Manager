import type { ManualModInventoryItem } from "../../types";

export interface SteamWorkshopEnablementPlanInput {
  downloadedWorkshopItemIds: readonly string[];
  inventoryBefore: readonly ManualModInventoryItem[];
  inventoryAfter: readonly ManualModInventoryItem[];
}

export interface SteamWorkshopEnablementPlan {
  inferredIds: string[];
  unresolvedWorkshopItemIds: string[];
}

interface EligibleInventoryItem {
  inferredId: string;
  identity: string;
  workshopIdTokens: ReadonlySet<string>;
}

export function buildSteamWorkshopEnablementPlan(
  input: SteamWorkshopEnablementPlanInput
): SteamWorkshopEnablementPlan {
  const downloadedItemIds = stableUnique(
    input.downloadedWorkshopItemIds
      .map((itemId) => itemId.trim())
      .filter((itemId) => /^\d+$/.test(itemId)),
    (itemId) => itemId
  );
  if (downloadedItemIds.length === 0) {
    return { inferredIds: [], unresolvedWorkshopItemIds: [] };
  }

  const inventoryAfter = eligibleInventory(input.inventoryAfter);
  const previousIdentities = new Set(
    eligibleInventory(input.inventoryBefore).map((entry) => entry.identity)
  );
  const inferredIds: string[] = [];
  const assignedIdentities = new Set<string>();
  const unresolvedWorkshopItemIds: string[] = [];

  for (const workshopItemId of downloadedItemIds) {
    const matches = inventoryAfter.filter((entry) => entry.workshopIdTokens.has(workshopItemId));
    if (matches.length === 0) {
      unresolvedWorkshopItemIds.push(workshopItemId);
      continue;
    }
    for (const entry of matches) {
      assignedIdentities.add(entry.identity);
      inferredIds.push(entry.inferredId);
    }
  }

  if (unresolvedWorkshopItemIds.length === 1) {
    const remainingInventoryDelta = inventoryAfter.filter(
      (entry) => !previousIdentities.has(entry.identity) && !assignedIdentities.has(entry.identity)
    );
    if (remainingInventoryDelta.length > 0) {
      for (const entry of remainingInventoryDelta) {
        assignedIdentities.add(entry.identity);
        inferredIds.push(entry.inferredId);
      }
      unresolvedWorkshopItemIds.length = 0;
    }
  }

  return {
    inferredIds: stableUnique(inferredIds, (inferredId) => inferredId.toLowerCase()),
    unresolvedWorkshopItemIds
  };
}

function eligibleInventory(items: readonly ManualModInventoryItem[]): EligibleInventoryItem[] {
  const eligible: EligibleInventoryItem[] = [];

  for (const item of items) {
    const inferredId = item.inferred_id?.trim();
    if (!inferredId) {
      continue;
    }

    eligible.push({
      inferredId,
      identity: `${inventoryPathIdentity(item)}\u0000${inferredId.toLowerCase()}`,
      workshopIdTokens: new Set(
        [item.name, item.path]
          .flatMap((value) => value.match(/\d+/g) ?? [])
      )
    });
  }

  return eligible;
}

function inventoryPathIdentity(item: ManualModInventoryItem): string {
  const normalizedPath = item.path
    .trim()
    .replace(/\\/g, "/")
    .replace(/\/+$/, "");

  if (normalizedPath) {
    const isWindowsPath = /^[a-z]:\//i.test(normalizedPath) || normalizedPath.startsWith("//");
    return `path:${isWindowsPath ? normalizedPath.toLowerCase() : normalizedPath}`;
  }

  return `item:${item.item_type}:${item.name.trim()}`;
}

function stableUnique<T>(values: readonly T[], keyOf: (value: T) => string): T[] {
  const seen = new Set<string>();
  const unique: T[] = [];

  for (const value of values) {
    const key = keyOf(value);
    if (seen.has(key)) {
      continue;
    }
    seen.add(key);
    unique.push(value);
  }

  return unique;
}
