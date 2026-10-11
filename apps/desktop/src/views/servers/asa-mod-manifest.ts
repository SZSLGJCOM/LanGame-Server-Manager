import { canonicalAsaModId } from "./mod-workbench-asa";

export interface ParsedAsaModManifest {
  ids: string[];
  invalidTokens: string[];
  duplicateCount: number;
  tooLarge: boolean;
}

/** ASA consumes numeric CurseForge project IDs; do not silently skip bad entries. */
export function parseAsaModManifest(text: string): ParsedAsaModManifest {
  const result: ParsedAsaModManifest = { ids: [], invalidTokens: [], duplicateCount: 0, tooLarge: false };
  if (text.length > 1_048_576 || new TextEncoder().encode(text).length > 1_048_576) {
    result.tooLarge = true;
    return result;
  }
  const seen = new Set<string>();
  for (const match of text.matchAll(/[^\s,;、，；]+/g)) {
    const token = match[0];
    const id = canonicalAsaModId(token);
    if (!id) result.invalidTokens.push(token);
    else if (seen.has(id)) result.duplicateCount += 1;
    else {
      if (seen.size === 8192) { result.tooLarge = true; break; }
      seen.add(id); result.ids.push(id);
    }
  }
  return result;
}
