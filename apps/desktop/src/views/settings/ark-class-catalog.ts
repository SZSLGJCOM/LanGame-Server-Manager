import { searchArkGmCreatureOptions } from "../servers/ark-gm-creature-catalog";
import { searchArkGmItemOptions } from "../servers/ark-gm-item-catalog";

export type ArkCatalogKind = "creature" | "item";
export interface ArkClassSuggestion { value: string; label: string }

export function arkClassCatalogKind(property: string): ArkCatalogKind | undefined {
  if (["FromClassName", "ToClassName", "NPCClassString", "NPCsToSpawnStrings"].includes(property)) return "creature";
  if (["ItemClassString", "ItemClassStrings", "ResourceItemTypeString"].includes(property)) return "item";
  return undefined;
}

/** The source catalog has no edition support metadata; suggestions never assert compatibility. */
export function searchArkClasses(kind: ArkCatalogKind, query: string): ArkClassSuggestion[] {
  const candidates = kind === "creature"
    ? searchArkGmCreatureOptions(query, 30).map((entry) => ({ value: entry.value, label: entry.name }))
    : searchArkGmItemOptions(query, 30).flatMap((entry) => {
    const match = entry.blueprintPath.match(/\.([^.'\/]+)'?$/);
    return match ? [{ value: match[1].replace(/_C$/, "") + "_C", label: entry.name }] : [];
  });
  return [...new Map(candidates.map((entry) => [entry.value, entry])).values()];
}
