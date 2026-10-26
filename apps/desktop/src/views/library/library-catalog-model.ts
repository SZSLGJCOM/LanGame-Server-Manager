import type { ModuleSummary } from "../../types";

export interface LibraryCatalogAlignmentInput {
  railLeft: number;
  railRight: number;
  railScrollLeft: number;
  tileLeft: number;
  tileRight: number;
  maxScrollLeft: number;
  safeInset?: number;
}

export interface LibraryCatalogStoreSearchEntry {
  storeName?: string | null;
  shortDescription?: string | null;
  genres?: string[] | null;
}

export type LibraryCatalogStoreResolver = (moduleId: string) => LibraryCatalogStoreSearchEntry | null | undefined;

export function normalizeLibraryCatalogSearch(value: string) {
  return value.trim().toLowerCase();
}

export function filterLibraryCatalogModules(
  modules: ModuleSummary[],
  search: string,
  resolveStoreEntry: LibraryCatalogStoreResolver
) {
  const normalizedSearch = normalizeLibraryCatalogSearch(search);
  if (!normalizedSearch) {
    return modules;
  }

  return modules.filter((module) => {
    const storeEntry = resolveStoreEntry(module.id);

    return [
      module.name,
      module.id,
      module.description ?? "",
      String(module.steam_app_id ?? ""),
      storeEntry?.storeName ?? "",
      storeEntry?.shortDescription ?? "",
      storeEntry?.genres?.join(" ") ?? ""
    ].some((value) => value.toLowerCase().includes(normalizedSearch));
  });
}

export function resolveLibraryCatalogAlignmentScrollLeft(input: LibraryCatalogAlignmentInput) {
  const safeInset = Math.max(0, input.safeInset ?? 72);
  const safeLeft = input.railLeft + safeInset;
  const safeRight = input.railRight - safeInset;

  if (input.tileLeft >= safeLeft && input.tileRight <= safeRight) {
    return null;
  }

  const targetScrollLeft =
    input.tileLeft < safeLeft
      ? input.railScrollLeft + input.tileLeft - safeLeft
      : input.railScrollLeft + input.tileRight - safeRight;

  return Math.min(Math.max(targetScrollLeft, 0), Math.max(input.maxScrollLeft, 0));
}
