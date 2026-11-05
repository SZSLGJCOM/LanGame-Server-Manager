import type { ManualModInventoryItem } from "../../types";

function trimNativeName(value: string): string {
  // Rust str::trim uses Unicode White_Space, which differs from JS trim for BOM and NEL.
  return value.replace(/^\p{White_Space}+|\p{White_Space}+$/gu, "");
}

export function readTerrariaEnabledModNames(raw: unknown): string[] {
  if (typeof raw !== "string") return [];
  // Match the server's enabled.json renderer: one case-sensitive name per line.
  return [...new Set(raw.split("\n").map(trimNativeName)
    .filter((name) => name && !name.startsWith("#") && !name.startsWith("--")))];
}

export function isTerrariaModName(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value === trimNativeName(value)
    && value.toLowerCase() !== "terraria" && !/[./\\\p{Cc}]/u.test(value)
    && !value.startsWith("#") && !value.startsWith("--");
}

export function readTerrariaWorkshopMetadata(ownedIds: readonly string[], items?: readonly ManualModInventoryItem[] | null) {
  const owned = new Set(ownedIds);
  const namesById = new Map<string, string | null>();
  const ownersByName = new Map<string, Set<string>>();
  for (const item of items ?? []) {
    const id = item.inferred_id;
    if (!id || !owned.has(id)) continue;
    const name = isTerrariaModName(item.tmodloader_mod_name) ? item.tmodloader_mod_name : null;
    if (!namesById.has(id)) namesById.set(id, name);
    else if (namesById.get(id) !== name) namesById.set(id, null);
    if (name !== null) {
      let owners = ownersByName.get(name);
      if (!owners) {
        owners = new Set();
        ownersByName.set(name, owners);
      }
      owners.add(id);
    }
  }
  const uniqueNames = new Set<string>();
  for (const name of namesById.values()) {
    if (name !== null && ownersByName.get(name)?.size === 1) uniqueNames.add(name);
  }
  return { namesById, ownersByName, uniqueNames };
}

export function editTerrariaEnabledNames(raw: string, additions: readonly string[], removals: ReadonlySet<string>): string {
  // Keep comments, spelling, and the original separators of surviving lines.
  const parts = raw.split(/(\r?\n)/);
  const kept: string[] = [];
  for (let index = 0; index < parts.length; index += 2) {
    if (removals.has(trimNativeName(parts[index]))) continue;
    if (kept.length) kept.push(parts[index - 1]);
    kept.push(parts[index]);
  }
  const retained = kept.join("");
  const active = new Set(readTerrariaEnabledModNames(retained));
  const append = [...new Set(additions)].filter((name) => !active.has(name));
  if (!append.length) return retained;
  const eol = raw.match(/\r?\n/)?.[0] ?? "\n";
  return retained + (retained && !retained.endsWith("\n") ? eol : "") + append.join(eol)
    + (retained.endsWith("\n") ? eol : "");
}
