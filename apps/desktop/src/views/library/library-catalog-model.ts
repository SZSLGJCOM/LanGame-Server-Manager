import type { ModuleSummary } from "../../types";

export type LibraryCatalogPageDirection = "left" | "right";

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

export interface LibraryCatalogWheelNavigationInput {
  accumulator: number;
  rawDelta: number;
  deltaScale: number;
  nowMs: number;
  lastStepAtMs: number | null;
  threshold?: number;
  minStepIntervalMs?: number;
  maxSteps?: number;
}

export interface LibraryCatalogWheelNavigationResult {
  accumulator: number;
  lastStepAtMs: number | null;
  offset: number;
}

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

export function resolveLibraryCatalogFocusId(
  visibleModules: ModuleSummary[],
  rememberedFocusId: string | null | undefined,
  selectedModuleId: string | null | undefined
) {
  if (rememberedFocusId && visibleModules.some((module) => module.id === rememberedFocusId)) {
    return rememberedFocusId;
  }

  if (selectedModuleId && visibleModules.some((module) => module.id === selectedModuleId)) {
    return selectedModuleId;
  }

  return visibleModules[0]?.id ?? null;
}

function clampLibraryCatalogIndex(index: number, visibleModules: ModuleSummary[]) {
  return Math.min(Math.max(index, 0), Math.max(visibleModules.length - 1, 0));
}

function resolveLibraryCatalogFocusIndex(visibleModules: ModuleSummary[], currentFocusId: string | null | undefined) {
  if (visibleModules.length === 0) {
    return -1;
  }

  const currentIndex = currentFocusId
    ? visibleModules.findIndex((module) => module.id === currentFocusId)
    : -1;

  return currentIndex >= 0 ? currentIndex : 0;
}

export function resolveLibraryCatalogFocusByOffset(
  visibleModules: ModuleSummary[],
  currentFocusId: string | null | undefined,
  offset: number
) {
  if (visibleModules.length === 0) {
    return null;
  }

  const currentIndex = resolveLibraryCatalogFocusIndex(visibleModules, currentFocusId);
  const nextIndex = clampLibraryCatalogIndex(currentIndex + Math.trunc(offset), visibleModules);
  return visibleModules[nextIndex]?.id ?? null;
}

export function resolveLibraryCatalogPageFocusId(
  visibleModules: ModuleSummary[],
  currentFocusId: string | null | undefined,
  direction: LibraryCatalogPageDirection,
  visibleCardCount: number
) {
  if (visibleModules.length === 0) {
    return null;
  }

  const currentIndex = resolveLibraryCatalogFocusIndex(visibleModules, currentFocusId);
  const pageStep = Math.max(1, Math.trunc(visibleCardCount) - 1);
  const signedStep = direction === "left" ? -pageStep : pageStep;
  const nextIndex = clampLibraryCatalogIndex(currentIndex + signedStep, visibleModules);
  return visibleModules[nextIndex]?.id ?? null;
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

export function resolveLibraryCatalogWheelNavigation(
  input: LibraryCatalogWheelNavigationInput
): LibraryCatalogWheelNavigationResult {
  const threshold = Math.max(1, input.threshold ?? 72);
  const minStepIntervalMs = Math.max(0, input.minStepIntervalMs ?? 90);
  const maxSteps = Math.max(1, Math.trunc(input.maxSteps ?? 3));
  const accumulator = input.accumulator + input.rawDelta * input.deltaScale;
  const isThrottled = input.lastStepAtMs !== null && input.nowMs - input.lastStepAtMs < minStepIntervalMs;

  if (Math.abs(accumulator) < threshold || isThrottled) {
    return {
      accumulator,
      lastStepAtMs: input.lastStepAtMs,
      offset: 0
    };
  }

  const direction = accumulator > 0 ? 1 : -1;
  const steps = Math.min(maxSteps, Math.max(1, Math.floor(Math.abs(accumulator) / threshold)));

  return {
    accumulator: 0,
    lastStepAtMs: input.nowMs,
    offset: direction * steps
  };
}
