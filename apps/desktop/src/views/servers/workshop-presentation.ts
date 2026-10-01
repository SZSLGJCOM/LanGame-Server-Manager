import type { SteamWorkshopLookupItem } from "../../types";

interface WorkshopPresentation {
  title: SteamWorkshopLookupItem["title"];
  description: SteamWorkshopLookupItem["description"];
  description_excerpt: SteamWorkshopLookupItem["description_excerpt"];
  localization_warning: SteamWorkshopLookupItem["localization_warning"];
  childTitles: Record<string, string>;
}

export type WorkshopPresentationCache = Record<string, WorkshopPresentation>;
const MAX_PRESENTATIONS = 64;

export function rememberWorkshopPresentation(cache: WorkshopPresentationCache, item: SteamWorkshopLookupItem): WorkshopPresentationCache {
  const next = { ...cache };
  delete next[item.id];
  next[item.id] = {
    title: item.title, description: item.description, description_excerpt: item.description_excerpt,
    localization_warning: item.localization_warning ?? null,
    childTitles: Object.fromEntries(item.children.flatMap((child) => typeof child.title === "string" ? [[child.id, child.title]] : []))
  };
  const otherIds = Object.keys(next).filter((id) => id !== item.id);
  for (const id of otherIds.slice(0, Math.max(0, otherIds.length + 1 - MAX_PRESENTATIONS))) delete next[id];
  return next;
}

/** Steam's batch metadata is not localized. It verifies identity, never replaces localized display text. */
export function composeWorkshopLookups(
  summaries: SteamWorkshopLookupItem[],
  metadata: Record<string, SteamWorkshopLookupItem>,
  presentations: WorkshopPresentationCache
): Record<string, SteamWorkshopLookupItem> {
  const result = { ...metadata };
  for (const summary of summaries) {
    result[summary.id] = metadata[summary.id] ? { ...metadata[summary.id],
      title: summary.title, description: summary.description, description_excerpt: summary.description_excerpt
    } : summary;
  }
  for (const [id, text] of Object.entries(presentations)) {
    const item = result[id];
    if (!item) continue;
    result[id] = { ...item, title: text.title, description: text.description, description_excerpt: text.description_excerpt,
      localization_warning: text.localization_warning,
      children: item.children.map((child) => text.childTitles[child.id] === undefined ? child : { ...child, title: text.childTitles[child.id] }) };
  }
  return result;
}
