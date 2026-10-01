import { Suspense, lazy, useCallback, useDeferredValue, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { KeyboardEvent, PointerEvent as ReactPointerEvent } from "react";
import { useMotionValue } from "motion/react";
import { useI18n } from "../i18n";
import { resolveModuleCoverSrc } from "../module-art";
import { buildModuleMediaItems, getLocalizedModuleStoreData } from "../store-media";
import type { BackgroundJob, CreateInstanceInput, InstanceSummary, ModuleDetails, ModuleSummary, SteamCmdStatus } from "../types";
import {
  filterLibraryCatalogModules,
  resolveLibraryCatalogAlignmentScrollLeft
} from "./library/library-catalog-model";
import {
  libraryCatalogOptionId,
  resolveLibraryHorizontalFocusId,
  resolveLibraryKeyboardAction,
  type LibraryDirection,
  type LibraryFocusTarget,
  type LibraryNavigationAction
} from "./library/library-navigation-model";
import { LibraryCatalogPage } from "./library/LibraryCatalogPage";
import { useLibraryGamepadNavigation } from "./library/useLibraryGamepadNavigation";
import { LibraryAtmosphere } from "./library/LibraryAtmosphere";
import { formatInstallState, installTone, suggestedName } from "./library/library-shared";
import { isActiveInstallationJob, installationJobPhase } from "../installation-job";

const LibraryDetailPage = lazy(() =>
  import("./library/LibraryDetailPage").then((module) => ({ default: module.LibraryDetailPage }))
);
const LibraryAtmosphereField = lazy(() =>
  import("./library/LibraryAtmosphereField").then((module) => ({ default: module.LibraryAtmosphereField }))
);

const CATALOG_FOCUS_PERSIST_DELAY_MS = 320;
const CATALOG_SCROLL_PERSIST_DELAY_MS = 360;
const CATALOG_RAIL_MOTION_MIN_MS = 140;
const CATALOG_RAIL_MOTION_MAX_MS = 210;
const CATALOG_ATMOSPHERE_PREVIEW_DELAY_MS = 140;

type LibraryPageMode = "catalog" | "detail";

interface LibraryContextMenuState {
  target: LibraryFocusTarget;
  x: number;
  y: number;
}

interface LibraryViewProps {
  mode: LibraryPageMode;
  modules: ModuleSummary[];
  instances?: InstanceSummary[];
  selectedModuleId: string | null;
  selectedModuleDetails: ModuleDetails | null;
  steamCmdStatus: SteamCmdStatus | null;
  steamCmdBusy: boolean;
  search: string;
  catalogFocusId: string | null;
  catalogScrollLeft: number;
  jobs: BackgroundJob[];
  onSearchChange: (value: string) => void;
  onCatalogFocusChange: (moduleId: string | null) => void;
  onCatalogScrollLeftChange: (scrollLeft: number) => void;
  onOpenModule: (moduleId: string) => void;
  onBackToCatalog: () => void;
  onInstall: (moduleId: string, validate: boolean) => void;
  onUninstall: (moduleId: string) => void | Promise<void>;
  onCreateServer: (input: CreateInstanceInput) => Promise<void>;
  creatingModuleIds: ReadonlySet<string>;
  creationStartedAtByModule?: ReadonlyMap<string, number>;
}

function joinLibraryMeta(parts: Array<string | null | undefined>) {
  return parts.filter((value): value is string => Boolean(value && value.trim())).join(" · ");
}

function hasVisibleApplicationDialog() {
  return Array.from(document.querySelectorAll<HTMLElement>('[role="dialog"]')).some(
    (dialog) => dialog.getAttribute("aria-hidden") !== "true" && dialog.getClientRects().length > 0
  );
}

function LibraryDetailLoadingState() {
  const { t } = useI18n();

  return (
    <div className="library-view-loading" role="status" aria-live="polite" aria-busy="true">
      <span className="library-view-loading-mark" aria-hidden="true" />
      <span className="library-view-loading-copy">{t("library.loadingDetail")}</span>
    </div>
  );
}

export function LibraryView(props: LibraryViewProps) {
  const { locale, t } = useI18n();
  const deferredSearch = useDeferredValue(props.search);
  const [instanceName, setInstanceName] = useState("");
  const [activeMediaKey, setActiveMediaKey] = useState<string | null>(null);
  const [catalogPreviewModuleId, setCatalogPreviewModuleId] = useState<string | null>(null);
  const [catalogAtmosphereModuleId, setCatalogAtmosphereModuleId] = useState<string | null>(
    () => props.catalogFocusId ?? props.selectedModuleId
  );
  const [activeTarget, setActiveTarget] = useState<LibraryFocusTarget | null>(() =>
    props.catalogFocusId ? { moduleId: props.catalogFocusId } : null
  );
  const [contextMenu, setContextMenu] = useState<LibraryContextMenuState | null>(null);
  const atmospherePointerX = useMotionValue(0);
  const atmospherePointerY = useMotionValue(0);
  const atmospherePointerActivity = useMotionValue(0);
  const atmosphereBackdropRef = useRef<HTMLDivElement | null>(null);
  const railRef = useRef<HTMLDivElement | null>(null);
  const catalogFocusCurrentRef = useRef<string | null>(props.catalogFocusId);
  const catalogFocusPersistTimerRef = useRef<number | null>(null);
  const catalogScrollPersistTimerRef = useRef<number | null>(null);
  const catalogFocusRestoreTimerRef = useRef<number | null>(null);
  const catalogRailAlignmentFrameRef = useRef<number | null>(null);
  const catalogRailMotionFrameRef = useRef<number | null>(null);
  const catalogScrollLeftRef = useRef(props.catalogScrollLeft);
  const returnFocusRequestedRef = useRef(false);
  const modeRef = useRef(props.mode);
  const effectiveTargetRef = useRef<LibraryFocusTarget | null>(activeTarget);
  const searchRef = useRef(props.search);
  const focusChangeCallbackRef = useRef(props.onCatalogFocusChange);
  const scrollChangeCallbackRef = useRef(props.onCatalogScrollLeftChange);

  const searchedModules = useMemo(
    () => filterLibraryCatalogModules(props.modules, deferredSearch, (moduleId) => getLocalizedModuleStoreData(moduleId, locale)),
    [deferredSearch, locale, props.modules]
  );
  const allModules = searchedModules;
  const allIds = useMemo(() => allModules.map((module) => module.id), [allModules]);

  const effectiveTarget = useMemo<LibraryFocusTarget | null>(() => {
    if (activeTarget && allIds.includes(activeTarget.moduleId)) {
      return activeTarget;
    }
    const requestedId = props.catalogFocusId ?? props.selectedModuleId;
    if (requestedId && allIds.includes(requestedId)) {
      return { moduleId: requestedId };
    }
    return allIds[0] ? { moduleId: allIds[0] } : null;
  }, [activeTarget, allIds, props.catalogFocusId, props.selectedModuleId]);

  effectiveTargetRef.current = effectiveTarget;
  catalogFocusCurrentRef.current = effectiveTarget?.moduleId ?? null;
  modeRef.current = props.mode;
  searchRef.current = props.search;
  focusChangeCallbackRef.current = props.onCatalogFocusChange;
  scrollChangeCallbackRef.current = props.onCatalogScrollLeftChange;

  const focusedCatalogModule = allModules.find((module) => module.id === effectiveTarget?.moduleId) ?? null;
  const detailsMatchSelection = props.selectedModuleDetails?.summary.id === props.selectedModuleId;
  const selectedModuleDetails = detailsMatchSelection ? props.selectedModuleDetails : null;
  const selected = selectedModuleDetails?.summary
    ?? props.modules.find((item) => item.id === props.selectedModuleId)
    ?? null;
  const selectedInstallJob = props.jobs.find((job) => job.target_id === selected?.id && isActiveInstallationJob(job));

  function queueCatalogFocusPersistence(moduleId: string | null) {
    if (catalogFocusPersistTimerRef.current !== null) {
      window.clearTimeout(catalogFocusPersistTimerRef.current);
    }
    catalogFocusPersistTimerRef.current = window.setTimeout(() => {
      catalogFocusPersistTimerRef.current = null;
      focusChangeCallbackRef.current(moduleId);
    }, CATALOG_FOCUS_PERSIST_DELAY_MS);
  }

  function flushCatalogFocusPersistence() {
    if (catalogFocusPersistTimerRef.current !== null) {
      window.clearTimeout(catalogFocusPersistTimerRef.current);
      catalogFocusPersistTimerRef.current = null;
    }
    focusChangeCallbackRef.current(catalogFocusCurrentRef.current);
  }

  function queueCatalogScrollPersistence() {
    if (catalogScrollPersistTimerRef.current !== null) {
      window.clearTimeout(catalogScrollPersistTimerRef.current);
    }
    catalogScrollPersistTimerRef.current = window.setTimeout(() => {
      catalogScrollPersistTimerRef.current = null;
      scrollChangeCallbackRef.current(catalogScrollLeftRef.current);
    }, CATALOG_SCROLL_PERSIST_DELAY_MS);
  }

  function flushCatalogScrollPersistence() {
    if (catalogScrollPersistTimerRef.current !== null) {
      window.clearTimeout(catalogScrollPersistTimerRef.current);
      catalogScrollPersistTimerRef.current = null;
    }
    scrollChangeCallbackRef.current(catalogScrollLeftRef.current);
  }

  const focusRail = useCallback(() => {
    railRef.current?.focus({ preventScroll: true });
  }, []);

  const restoreRailFocus = useCallback(() => {
    focusRail();
    if (catalogFocusRestoreTimerRef.current !== null) {
      window.clearTimeout(catalogFocusRestoreTimerRef.current);
    }
    catalogFocusRestoreTimerRef.current = window.setTimeout(() => {
      catalogFocusRestoreTimerRef.current = null;
      if (modeRef.current === "catalog") {
        focusRail();
      }
    }, 0);
  }, [focusRail]);

  const cancelRailMotion = useCallback(() => {
    if (catalogRailAlignmentFrameRef.current !== null) {
      window.cancelAnimationFrame(catalogRailAlignmentFrameRef.current);
      catalogRailAlignmentFrameRef.current = null;
    }
    if (catalogRailMotionFrameRef.current !== null) {
      window.cancelAnimationFrame(catalogRailMotionFrameRef.current);
      catalogRailMotionFrameRef.current = null;
    }
  }, []);

  const ensureTargetVisible = useCallback((target: LibraryFocusTarget) => {
    cancelRailMotion();
    catalogRailAlignmentFrameRef.current = window.requestAnimationFrame(() => {
      catalogRailAlignmentFrameRef.current = null;
      const rail = railRef.current;
      const tile = document.getElementById(libraryCatalogOptionId(target));
      if (!rail || !tile) {
        return;
      }

      const railRect = rail.getBoundingClientRect();
      const tileRect = tile.getBoundingClientRect();
      const targetScrollLeft = resolveLibraryCatalogAlignmentScrollLeft({
        railLeft: railRect.left,
        railRight: railRect.right,
        railScrollLeft: rail.scrollLeft,
        tileLeft: tileRect.left,
        tileRight: tileRect.right,
        maxScrollLeft: rail.scrollWidth - rail.clientWidth,
        safeInset: Math.min(96, rail.clientWidth * 0.13)
      });
      if (targetScrollLeft === null) {
        return;
      }

      const startScrollLeft = rail.scrollLeft;
      const distance = targetScrollLeft - startScrollLeft;
      if (Math.abs(distance) < 1 || window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
        rail.scrollLeft = targetScrollLeft;
        return;
      }

      const startedAt = performance.now();
      const duration = Math.min(
        CATALOG_RAIL_MOTION_MAX_MS,
        Math.max(CATALOG_RAIL_MOTION_MIN_MS, 120 + Math.abs(distance) * 0.16)
      );
      const advance = (now: number) => {
        const progress = Math.min((now - startedAt) / duration, 1);
        const easedProgress = 1 - Math.pow(1 - progress, 4);
        rail.scrollLeft = startScrollLeft + distance * easedProgress;
        if (progress < 1) {
          catalogRailMotionFrameRef.current = window.requestAnimationFrame(advance);
        } else {
          catalogRailMotionFrameRef.current = null;
          rail.scrollLeft = targetScrollLeft;
        }
      };
      catalogRailMotionFrameRef.current = window.requestAnimationFrame(advance);
    });
  }, [cancelRailMotion]);

  const selectTarget = useCallback((target: LibraryFocusTarget, restoreDomFocus = true) => {
    catalogFocusCurrentRef.current = target.moduleId;
    effectiveTargetRef.current = target;
    setActiveTarget(target);
    setContextMenu(null);
    queueCatalogFocusPersistence(target.moduleId);
    ensureTargetVisible(target);
    if (restoreDomFocus) {
      window.requestAnimationFrame(focusRail);
    }
  }, [ensureTargetVisible, focusRail]);

  const confirmTarget = useCallback((target = effectiveTargetRef.current) => {
    if (!target) {
      return;
    }
    catalogFocusCurrentRef.current = target.moduleId;
    effectiveTargetRef.current = target;
    setActiveTarget(target);
    setContextMenu(null);
    returnFocusRequestedRef.current = true;
    flushCatalogFocusPersistence();
    flushCatalogScrollPersistence();
    props.onOpenModule(target.moduleId);
  }, [props.onOpenModule]);

  const closeContextMenu = useCallback((restoreFocus = true) => {
    setContextMenu(null);
    const target = effectiveTargetRef.current;
    if (restoreFocus && target) {
      restoreRailFocus();
    }
  }, [restoreRailFocus]);

  const openContextMenu = useCallback((target: LibraryFocusTarget, point?: { x: number; y: number }) => {
    selectTarget(target, false);
    const optionRect = document.getElementById(libraryCatalogOptionId(target))?.getBoundingClientRect();
    const x = Math.min(Math.max(point?.x ?? optionRect?.left ?? 24, 12), Math.max(window.innerWidth - 212, 12));
    const y = Math.min(Math.max(point?.y ?? optionRect?.bottom ?? 24, 12), Math.max(window.innerHeight - 112, 12));
    setContextMenu({ target, x, y });
  }, [selectTarget]);

  function moveContextMenuFocus(direction: LibraryDirection) {
    const menu = document.querySelector<HTMLElement>(".library-catalog-context-menu");
    const buttons = Array.from(menu?.querySelectorAll<HTMLButtonElement>("button") ?? []);
    if (!buttons.length) {
      return;
    }
    const currentIndex = Math.max(buttons.indexOf(document.activeElement as HTMLButtonElement), 0);
    const step = direction === "left" || direction === "up" ? -1 : 1;
    buttons[(currentIndex + step + buttons.length) % buttons.length]?.focus({ preventScroll: true });
  }

  function confirmContextMenuAction() {
    const menu = document.querySelector<HTMLElement>(".library-catalog-context-menu");
    const activeButton = document.activeElement instanceof HTMLButtonElement && menu?.contains(document.activeElement)
      ? document.activeElement
      : menu?.querySelector<HTMLButtonElement>("button");
    activeButton?.click();
  }

  function moveSelection(direction: LibraryDirection) {
    if (direction !== "left" && direction !== "right") {
      return;
    }
    const nextId = resolveLibraryHorizontalFocusId(allIds, effectiveTargetRef.current?.moduleId ?? null, direction);
    if (nextId) {
      selectTarget({ moduleId: nextId });
    }
  }

  function pageSelection(direction: "previous" | "next" | "first" | "last") {
    if (!allIds.length) {
      return;
    }
    const currentId = effectiveTargetRef.current?.moduleId ?? allIds[0];
    if (direction === "first" || direction === "last") {
      selectTarget({ moduleId: direction === "first" ? allIds[0] : allIds[allIds.length - 1] });
      return;
    }
    const cardsPerPage = Math.max(1, Math.floor((railRef.current?.clientWidth ?? 320) / 320));
    const offset = Math.max(1, cardsPerPage - 1);
    const currentIndex = Math.max(allIds.indexOf(currentId), 0);
    const nextIndex = Math.min(Math.max(currentIndex + (direction === "previous" ? -offset : offset), 0), allIds.length - 1);
    const nextId = allIds[nextIndex];
    if (nextId) {
      selectTarget({ moduleId: nextId });
    }
  }

  function dispatchNavigationAction(action: LibraryNavigationAction) {
    if (hasVisibleApplicationDialog()) {
      return;
    }
    if (props.mode === "detail") {
      if (action.type === "back") {
        returnFocusRequestedRef.current = true;
        props.onBackToCatalog();
      }
      return;
    }
    if (contextMenu) {
      if (action.type === "back" || action.type === "context") {
        closeContextMenu(true);
      } else if (action.type === "move") {
        moveContextMenuFocus(action.direction);
      } else if (action.type === "confirm") {
        confirmContextMenuAction();
      }
      return;
    }
    if (action.type === "move") {
      moveSelection(action.direction);
      return;
    }
    if (action.type === "page") {
      pageSelection(action.direction === "left" ? "previous" : "next");
      return;
    }
    if (action.type === "confirm") {
      confirmTarget();
      return;
    }
    if (action.type === "context") {
      const target = effectiveTargetRef.current;
      if (target) {
        openContextMenu(target);
      }
      return;
    }
    if (contextMenu) {
      closeContextMenu();
    } else if (props.search.trim()) {
      props.onSearchChange("");
    }
  }

  function handleRailKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Home" || event.key === "End") {
      event.preventDefault();
      event.stopPropagation();
      pageSelection(event.key === "Home" ? "first" : "last");
      return;
    }
    const action = resolveLibraryKeyboardAction(event);
    if (!action) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    dispatchNavigationAction(action);
  }

  useLibraryGamepadNavigation({
    enabled: true,
    onAction: (action) => dispatchNavigationAction(action)
  });

  useEffect(() => {
    if (props.mode !== "catalog") {
      setCatalogPreviewModuleId(null);
      return;
    }
    const nextId = catalogPreviewModuleId ?? effectiveTarget?.moduleId ?? null;
    if (nextId === catalogAtmosphereModuleId) {
      return;
    }
    const timer = window.setTimeout(
      () => setCatalogAtmosphereModuleId(nextId),
      CATALOG_ATMOSPHERE_PREVIEW_DELAY_MS
    );
    return () => window.clearTimeout(timer);
  }, [catalogAtmosphereModuleId, catalogPreviewModuleId, effectiveTarget?.moduleId, props.mode]);

  useEffect(() => {
    if (effectiveTarget && activeTarget?.moduleId !== effectiveTarget.moduleId) {
      setActiveTarget(effectiveTarget);
      queueCatalogFocusPersistence(effectiveTarget.moduleId);
    }
  }, [activeTarget, effectiveTarget]);

  useEffect(() => {
    if (!props.catalogFocusId || catalogFocusCurrentRef.current === props.catalogFocusId) {
      return;
    }
    setActiveTarget({ moduleId: props.catalogFocusId });
  }, [props.catalogFocusId]);

  useEffect(() => {
    if (!selected) {
      setInstanceName("");
      setActiveMediaKey(null);
      return;
    }
    setInstanceName(suggestedName(selected.name));
    setActiveMediaKey(null);
  }, [selected?.id, selected?.name]);

  useEffect(() => {
    const handleGlobalBack = (event: globalThis.KeyboardEvent) => {
      if ((event.key !== "Escape" && !(event.altKey && event.key === "ArrowLeft")) || event.defaultPrevented || hasVisibleApplicationDialog()) {
        return;
      }
      if (props.mode === "catalog" && !contextMenu && !props.search.trim()) {
        return;
      }
      event.preventDefault();
      dispatchNavigationAction({ type: "back", source: "keyboard" });
    };
    window.addEventListener("keydown", handleGlobalBack);
    return () => window.removeEventListener("keydown", handleGlobalBack);
  }, [contextMenu, props.mode, props.search]);

  useEffect(() => {
    if (props.mode !== "catalog") {
      flushCatalogFocusPersistence();
      flushCatalogScrollPersistence();
    }
  }, [props.mode]);

  useEffect(() => () => {
    cancelRailMotion();
    if (catalogFocusRestoreTimerRef.current !== null) {
      window.clearTimeout(catalogFocusRestoreTimerRef.current);
    }
    flushCatalogFocusPersistence();
    flushCatalogScrollPersistence();
  }, [cancelRailMotion]);

  useLayoutEffect(() => {
    if (props.mode !== "catalog") {
      return;
    }
    const activeElement = document.activeElement;
    const shouldRestoreFocus = returnFocusRequestedRef.current
      || !activeElement
      || activeElement === document.body
      || !activeElement.isConnected;
    if (!shouldRestoreFocus) {
      return;
    }
    returnFocusRequestedRef.current = false;
    const target = effectiveTargetRef.current;
    if (target) {
      restoreRailFocus();
    }
  }, [props.mode, restoreRailFocus]);

  const handleCatalogReady = useCallback(() => {
    if (modeRef.current !== "catalog") {
      return;
    }
    if (railRef.current) {
      railRef.current.scrollLeft = searchRef.current.trim() ? 0 : catalogScrollLeftRef.current;
    }
    const target = effectiveTargetRef.current;
    if (target && returnFocusRequestedRef.current) {
      returnFocusRequestedRef.current = false;
      restoreRailFocus();
    }
  }, [restoreRailFocus]);

  const handleRailWheel = useCallback((event: globalThis.WheelEvent) => {
    const rail = railRef.current;
    if (!rail || event.ctrlKey) {
      return;
    }
    cancelRailMotion();
    if (Math.abs(event.deltaX) >= Math.abs(event.deltaY)) {
      return;
    }
    const scale = event.deltaMode === 1 ? 32 : event.deltaMode === 2 ? rail.clientWidth : 1;
    const delta = event.deltaY * scale;
    const maxScrollLeft = Math.max(rail.scrollWidth - rail.clientWidth, 0);
    const canContinue = delta < 0 ? rail.scrollLeft > 0.5 : rail.scrollLeft < maxScrollLeft - 0.5;
    if (!canContinue) {
      return;
    }
    event.preventDefault();
    rail.scrollLeft = Math.min(Math.max(rail.scrollLeft + delta, 0), maxScrollLeft);
  }, [cancelRailMotion]);

  const handleAtmospherePointerMove = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    const bounds = atmosphereBackdropRef.current?.getBoundingClientRect();
    if (!bounds || bounds.width <= 0 || bounds.height <= 0) {
      return;
    }
    const inside = event.clientX >= bounds.left
      && event.clientX <= bounds.right
      && event.clientY >= bounds.top
      && event.clientY <= bounds.bottom;
    if (!inside) {
      atmospherePointerActivity.set(0);
      return;
    }
    atmospherePointerX.set(Math.min(Math.max(((event.clientX - bounds.left) / bounds.width) * 2 - 1, -1), 1));
    atmospherePointerY.set(Math.min(Math.max(1 - ((event.clientY - bounds.top) / bounds.height) * 2, -1), 1));
    atmospherePointerActivity.set(1);
  }, [atmospherePointerActivity, atmospherePointerX, atmospherePointerY]);

  const handleAtmospherePointerLeave = useCallback(() => {
    atmospherePointerActivity.set(0);
  }, [atmospherePointerActivity]);

  const storeEntry = getLocalizedModuleStoreData(selected?.id, locale);
  const mediaItems = buildModuleMediaItems(selected?.id, storeEntry, locale, t);
  const activeMedia = mediaItems.find((item) => item.key === activeMediaKey) ?? mediaItems[0] ?? null;
  const installLabel = formatInstallState(selected?.install_state, t);
  const installStatusClass = installTone(selected?.install_state);
  const heroTitle = storeEntry?.storeName ?? selected?.name ?? t("library.catalog.titleFallback");
  const heroSubtitle = storeEntry?.shortDescription ?? selected?.description ?? t("library.detail.englishFallbackBody");
  const storyParagraphs = storeEntry?.aboutParagraphs.length
    ? storeEntry.aboutParagraphs
    : selected?.description
      ? [selected.description]
      : [t("library.detail.englishFallbackBody")];
  const categoryTags = storeEntry?.categories.slice(0, 8) ?? [];
  const genreTags = storeEntry?.genres.slice(0, 5) ?? [];
  const detailHeaderMeta = joinLibraryMeta([
    storeEntry?.releaseDate ?? null,
    selected?.supported_platforms.length ? selected.supported_platforms.join(" / ") : t("common.windows"),
    selected?.steam_app_id ? t("library.detail.steamAppMeta", { appId: selected.steam_app_id }) : null
  ]);
  const catalogAtmosphereModule = allModules.find((module) => module.id === catalogAtmosphereModuleId)
    ?? focusedCatalogModule;
  const catalogAtmosphereStoreEntry = getLocalizedModuleStoreData(catalogAtmosphereModule?.id, locale);
  const catalogAtmosphereMediaItems = buildModuleMediaItems(catalogAtmosphereModule?.id, catalogAtmosphereStoreEntry, locale, t);
  const catalogAtmosphereMedia = catalogAtmosphereMediaItems.find((item) => item.kind === "screenshot" && item.imageSrc)
    ?? catalogAtmosphereMediaItems.find((item) => item.imageSrc)
    ?? null;
  const detailAtmosphereMedia = activeMedia?.imageSrc
    ? activeMedia
    : mediaItems.find((item) => item.imageSrc) ?? null;
  const visualModule = props.mode === "detail" ? selected : catalogAtmosphereModule;
  const visualStoreEntry = props.mode === "detail" ? storeEntry : catalogAtmosphereStoreEntry;
  const visualMedia = props.mode === "detail" ? detailAtmosphereMedia : catalogAtmosphereMedia;
  const visualModuleName = visualStoreEntry?.storeName ?? visualModule?.name ?? "Library";
  const visualMediaKey = visualMedia?.key ?? "cover";
  const visualImageSrc = visualMedia?.imageSrc ?? (visualModule ? resolveModuleCoverSrc(visualModule.id) : null);

  return (
    <div
      className={`library-experience library-experience--${props.mode}`}
      onPointerMove={handleAtmospherePointerMove}
      onPointerLeave={handleAtmospherePointerLeave}
    >
      <div ref={atmosphereBackdropRef} className="library-experience-backdrop">
        {visualModule ? (
          <>
            <LibraryAtmosphere
              moduleId={visualModule.id}
              moduleName={visualModuleName}
              mediaKey={visualMediaKey}
              imageSrc={visualImageSrc}
              className="library-experience-atmosphere"
              variant={props.mode}
            />
            <Suspense fallback={null}>
              <LibraryAtmosphereField
                moduleId={visualModule.id}
                moduleName={visualModuleName}
                mediaKey={visualMediaKey}
                imageSrc={visualImageSrc}
                mode={props.mode}
                pointerX={atmospherePointerX}
                pointerY={atmospherePointerY}
                pointerActivity={atmospherePointerActivity}
              />
            </Suspense>
          </>
        ) : null}
      </div>

      <div className="library-experience-content">
        {props.mode !== "detail" ? (
          <LibraryCatalogPage
            modules={allModules}
            search={props.search}
            activeTarget={effectiveTarget}
            railRef={railRef}
            contextMenu={contextMenu}
            onSearchChange={props.onSearchChange}
            onSelect={selectTarget}
            onConfirm={confirmTarget}
            onContext={openContextMenu}
            onCloseContext={closeContextMenu}
            onRailKeyDown={handleRailKeyDown}
            onPage={(direction) => pageSelection(direction === "left" ? "previous" : "next")}
            onRailWheel={handleRailWheel}
            onRailScroll={(scrollLeft) => {
              catalogScrollLeftRef.current = scrollLeft;
              if (!props.search.trim()) {
                queueCatalogScrollPersistence();
              }
            }}
            onPreviewModuleChange={setCatalogPreviewModuleId}
            onCatalogReady={handleCatalogReady}
          />
        ) : (
          <Suspense fallback={<LibraryDetailLoadingState />}>
            <LibraryDetailPage
              selected={selected}
              selectedModuleDetails={selectedModuleDetails}
              steamCmdStatus={props.steamCmdStatus}
              steamCmdBusy={props.steamCmdBusy}
              storeEntry={storeEntry}
              mediaItems={mediaItems}
              activeMedia={activeMedia}
              heroTitle={heroTitle}
              heroSubtitle={heroSubtitle}
              detailHeaderMeta={detailHeaderMeta}
              storyParagraphs={storyParagraphs}
              categoryTags={categoryTags}
              genreTags={genreTags}
              installLabel={selectedInstallJob ? t(`installation.phase.${installationJobPhase(selectedInstallJob)}`) : installLabel}
              installBusy={Boolean(selectedInstallJob)}
              installStatusClass={installStatusClass}
              creating={selected !== null && props.creatingModuleIds.has(selected.id)}
              creationStartedAt={selected ? props.creationStartedAtByModule?.get(selected.id) : undefined}
              instanceName={instanceName}
              onBackToCatalog={() => {
                returnFocusRequestedRef.current = true;
                props.onBackToCatalog();
              }}
              onActiveMediaChange={setActiveMediaKey}
              onInstall={props.onInstall}
              onUninstall={props.onUninstall}
              onCreateServer={props.onCreateServer}
              onInstanceNameChange={setInstanceName}
            />
          </Suspense>
        )}
      </div>
    </div>
  );
}
