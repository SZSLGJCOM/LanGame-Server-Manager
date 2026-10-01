import { memo, useEffect, useLayoutEffect, useMemo, useRef } from "react";
import type { CSSProperties, KeyboardEvent, KeyboardEventHandler, MouseEvent, RefObject } from "react";
import { ModuleCover } from "../../components/ModuleCover";
import { ShellIcon } from "../../components/ShellIcon";
import { useI18n } from "../../i18n";
import { getModuleArtTheme } from "../../module-art";
import { buildModuleMediaItems, getLocalizedModuleStoreData } from "../../store-media";
import type { ModuleSummary } from "../../types";
import type { LibraryFocusTarget } from "./library-navigation-model";
import { canConfirmLibraryCatalogPointerSelection, libraryCatalogOptionId } from "./library-navigation-model";
import { formatProgramLocations } from "./library-shared";

const COVER_DECODE_RADIUS = 2;

interface LibraryContextMenuState {
  target: LibraryFocusTarget;
  x: number;
  y: number;
}

interface LibraryCatalogPageProps {
  modules: ModuleSummary[];
  search: string;
  activeTarget: LibraryFocusTarget | null;
  railRef: RefObject<HTMLDivElement | null>;
  contextMenu: LibraryContextMenuState | null;
  onSearchChange: (value: string) => void;
  onSelect: (target: LibraryFocusTarget) => void;
  onConfirm: (target?: LibraryFocusTarget) => void;
  onContext: (target: LibraryFocusTarget, point?: { x: number; y: number }) => void;
  onCloseContext: (restoreFocus?: boolean) => void;
  onRailKeyDown: KeyboardEventHandler<HTMLDivElement>;
  onPage: (direction: "left" | "right") => void;
  onRailWheel: (event: globalThis.WheelEvent) => void;
  onRailScroll: (scrollLeft: number) => void;
  onPreviewModuleChange: (moduleId: string | null) => void;
  onCatalogReady: () => void;
}

interface LibraryCatalogTileModel {
  module: ModuleSummary;
  title: string;
  subtitle: string;
  previewVideoUrl: string | null;
  previewVideoPoster: string | null;
}

interface LibraryCatalogTileProps extends LibraryCatalogTileModel {
  active: boolean;
  eager: boolean;
  onSelect: (target: LibraryFocusTarget) => void;
  onConfirm: (target: LibraryFocusTarget) => void;
  onContext: (target: LibraryFocusTarget, point: { x: number; y: number }) => void;
  onPreview: (moduleId: string | null) => void;
}

const LibraryCatalogTile = memo(function LibraryCatalogTile(props: LibraryCatalogTileProps) {
  const { t } = useI18n();
  const target = { moduleId: props.module.id } satisfies LibraryFocusTarget;
  const status = formatProgramLocations(props.module, t);
  const activeSinceRef = useRef<number | null>(props.active ? Date.now() : null);

  useEffect(() => {
    activeSinceRef.current = props.active ? Date.now() : null;
  }, [props.active]);

  function handleContextMenu(event: MouseEvent<HTMLElement>) {
    event.preventDefault();
    props.onContext(target, { x: event.clientX, y: event.clientY });
  }

  function handleDoubleClick(event: MouseEvent<HTMLElement>) {
    event.preventDefault();
    event.stopPropagation();
    if (!canConfirmLibraryCatalogPointerSelection(props.active, activeSinceRef.current, Date.now())) {
      return;
    }
    props.onConfirm(target);
  }

  return (
    <article
      id={libraryCatalogOptionId(target)}
      data-module-id={props.module.id}
      className={props.active ? "library-catalog-tile is-active" : "library-catalog-tile"}
      role="option"
      aria-selected={props.active}
      aria-label={`${props.title}, ${status}`}
      onClick={() => props.onSelect(target)}
      onDoubleClick={handleDoubleClick}
      onContextMenu={handleContextMenu}
      onMouseEnter={() => props.onPreview(props.module.id)}
      onMouseLeave={() => props.onPreview(null)}
    >
      <span className="library-catalog-hitarea" aria-hidden="true" />
      <ModuleCover
        moduleId={props.module.id}
        moduleName={props.title}
        subtitle={props.subtitle}
        showOverlay={false}
        imageLoading={props.eager ? "eager" : "lazy"}
        previewVideoUrl={props.previewVideoUrl}
        previewVideoPoster={props.previewVideoPoster}
        previewVideoActive={props.active}
      />
    </article>
  );
});

function buildTileModels(modules: ModuleSummary[], locale: ReturnType<typeof useI18n>["locale"], t: ReturnType<typeof useI18n>["t"]) {
  return modules.map((module): LibraryCatalogTileModel => {
    const catalogStore = getLocalizedModuleStoreData(module.id, locale);
    const title = catalogStore?.storeName ?? module.name;
    const previewMedia = buildModuleMediaItems(module.id, catalogStore, locale, t)
      .find((item) => item.kind === "trailer" && item.streamUrl);
    const subtitle = module.steam_app_id
      ? t("library.catalog.steamAppSubtitle", { appId: module.steam_app_id })
      : catalogStore?.storeSource === "official"
        ? t("library.catalog.officialSourceSubtitle")
        : module.id;
    return {
      module,
      title,
      subtitle,
      previewVideoUrl: previewMedia?.streamUrl ?? null,
      previewVideoPoster: previewMedia?.imageSrc ?? null
    };
  });
}

function resolveEagerIds(modules: ModuleSummary[], activeId: string | null) {
  const activeIndex = modules.findIndex((module) => module.id === activeId);
  if (activeIndex < 0) {
    return new Set(modules.slice(0, COVER_DECODE_RADIUS + 1).map((module) => module.id));
  }
  return new Set(
    modules
      .slice(Math.max(0, activeIndex - COVER_DECODE_RADIUS), activeIndex + COVER_DECODE_RADIUS + 1)
      .map((module) => module.id)
  );
}

export function LibraryCatalogPage(props: LibraryCatalogPageProps) {
  const { locale, t } = useI18n();
  const searchRef = useRef<HTMLInputElement>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const activeSearch = props.search.trim();
  const tiles = useMemo(() => buildTileModels(props.modules, locale, t), [locale, props.modules, t]);
  const eagerIds = useMemo(
    () => resolveEagerIds(props.modules, props.activeTarget?.moduleId ?? null),
    [props.activeTarget?.moduleId, props.modules]
  );
  const focusedModule = props.modules.find((module) => module.id === props.activeTarget?.moduleId) ?? null;
  const focusedStoreEntry = getLocalizedModuleStoreData(focusedModule?.id, locale);
  const focusedEnglishStoreEntry = getLocalizedModuleStoreData(focusedModule?.id, "en-US");
  const focusedTitle = focusedStoreEntry?.storeName ?? focusedModule?.name ?? t("library.catalog.titleFallback");
  const focusedSubtitle = focusedEnglishStoreEntry?.storeName && focusedEnglishStoreEntry.storeName !== focusedTitle
    ? focusedEnglishStoreEntry.storeName
    : tiles.find((tile) => tile.module.id === focusedModule?.id)?.subtitle ?? "";
  const focusedInstalled = String(focusedModule?.install_state ?? "").toLowerCase() === "installed";
  const focusedStatus = focusedModule ? formatProgramLocations(focusedModule, t) : "";
  const focusedTheme = getModuleArtTheme(focusedModule?.id ?? "library", focusedTitle);
  const focusStyle = {
    "--library-accent": focusedTheme.accent,
    "--library-accent-strong": focusedTheme.accentStrong
  } as CSSProperties;
  const activeIndex = focusedModule ? props.modules.findIndex((module) => module.id === focusedModule.id) : -1;
  const railClassName = activeSearch && props.modules.length > 0 && props.modules.length <= 4
    ? "library-catalog-rail is-filtered"
    : "library-catalog-rail";

  useLayoutEffect(() => {
    props.onCatalogReady();
  }, [props.modules.length, props.onCatalogReady]);

  useEffect(() => {
    const rail = props.railRef.current;
    if (!rail) {
      return;
    }
    const handleWheel = (event: globalThis.WheelEvent) => props.onRailWheel(event);
    rail.addEventListener("wheel", handleWheel, { passive: false });
    return () => rail.removeEventListener("wheel", handleWheel);
  }, [props.modules.length, props.onRailWheel, props.railRef]);

  useEffect(() => {
    if (!props.contextMenu) {
      return;
    }
    menuRef.current?.querySelector<HTMLElement>("button")?.focus({ preventScroll: true });
    const closeFromOutside = (event: PointerEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) {
        props.onCloseContext(false);
      }
    };
    window.addEventListener("pointerdown", closeFromOutside);
    return () => window.removeEventListener("pointerdown", closeFromOutside);
  }, [props.contextMenu, props.onCloseContext]);

  function handleContextMenuKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      props.onCloseContext(true);
    }
  }

  function clearSearch() {
    props.onSearchChange("");
    searchRef.current?.focus();
  }

  return (
    <div className="page-grid workspace-page library-home-page library-home-page--cinematic">
      <section
        className="panel-card library-home-stage library-home-stage--ambient library-home-stage--cinematic"
        style={focusStyle}
      >
        <div className="library-catalog-toolbar">
          <div className="library-catalog-search-shell" role="search">
            <ShellIcon name="search" className="library-catalog-search-icon" />
            <input
              ref={searchRef}
              type="search"
              className="library-catalog-search-input"
              value={props.search}
              onChange={(event) => props.onSearchChange(event.currentTarget.value)}
              placeholder={t("library.catalog.searchPlaceholder", undefined, "Search game / AppID")}
              aria-label={t("library.catalog.searchLabel", undefined, "Search game library")}
              autoComplete="off"
              spellCheck={false}
            />
            {activeSearch ? (
              <button
                type="button"
                className="library-catalog-search-clear"
                onClick={clearSearch}
                aria-label={t("library.catalog.searchClear", undefined, "Clear library search")}
              >
                <ShellIcon name="x" className="library-catalog-search-clear-icon" />
              </button>
            ) : null}
          </div>
        </div>

        <div className="library-catalog-rail-shell">
          <button
            type="button"
            className="library-catalog-rail-nav is-left"
            onClick={() => props.onPage("left")}
            disabled={activeIndex <= 0}
            aria-label={t("library.catalog.previousScreen")}
          >
            <ShellIcon name="chevron-left" className="library-catalog-rail-nav-icon" />
          </button>
          {tiles.length ? (
            <div
              ref={props.railRef}
              className={railClassName}
              role="listbox"
              tabIndex={0}
              aria-label={t("library.catalog.allGamesLabel", undefined, "All games")}
              aria-activedescendant={props.activeTarget ? libraryCatalogOptionId(props.activeTarget) : undefined}
              onKeyDown={props.onRailKeyDown}
              onScroll={(event) => props.onRailScroll(event.currentTarget.scrollLeft)}
            >
              {tiles.map((tile) => (
                <LibraryCatalogTile
                  key={tile.module.id}
                  {...tile}
                  active={props.activeTarget?.moduleId === tile.module.id}
                  eager={eagerIds.has(tile.module.id)}
                  onSelect={props.onSelect}
                  onConfirm={props.onConfirm}
                  onContext={props.onContext}
                  onPreview={props.onPreviewModuleChange}
                />
              ))}
            </div>
          ) : (
            <div className="empty-state library-catalog-empty" role="status">
              <ShellIcon name="search" className="library-catalog-empty-icon" />
              <strong>{t("library.catalog.empty")}</strong>
              <span>{t("library.catalog.emptyHint")}</span>
              {activeSearch ? <button type="button" className="secondary-button" onClick={clearSearch}>{t("library.catalog.searchClear")}</button> : null}
            </div>
          )}
          <button
            type="button"
            className="library-catalog-rail-nav is-right"
            onClick={() => props.onPage("right")}
            disabled={activeIndex < 0 || activeIndex >= props.modules.length - 1}
            aria-label={t("library.catalog.nextScreen")}
          >
            <ShellIcon name="chevron-right" className="library-catalog-rail-nav-icon" />
          </button>
        </div>

        {focusedModule ? (
          <div className="library-catalog-focus-copy" aria-live="polite">
            <div key={focusedModule.id} className="library-catalog-focus-plaque">
              <div className={focusedInstalled ? "library-catalog-focus-status is-installed" : "library-catalog-focus-status"}>
                <ShellIcon
                  name={focusedInstalled ? "check-circle" : "package"}
                  className="library-catalog-focus-status-icon"
                />
                <span>{focusedStatus}</span>
              </div>
              <div className="library-catalog-focus-title">{focusedTitle}</div>
              {focusedSubtitle ? <div className="library-catalog-focus-subtitle">{focusedSubtitle}</div> : null}
            </div>
            <button
              type="button"
              className="primary-button library-catalog-open-details"
              onClick={() => props.onConfirm({ moduleId: focusedModule.id })}
            >
              {t("library.catalog.viewDetails")}
              <ShellIcon name="chevron-right" className="shell-small-icon" />
            </button>
          </div>
        ) : null}

        {props.contextMenu ? (
          <div
            ref={menuRef}
            className="library-catalog-context-menu"
            role="menu"
            style={{ left: props.contextMenu.x, top: props.contextMenu.y }}
            onKeyDown={handleContextMenuKeyDown}
          >
            <button type="button" role="menuitem" onClick={() => props.onConfirm(props.contextMenu!.target)}>
              {t("library.catalog.viewDetails", undefined, "View details")}
            </button>
          </div>
        ) : null}
      </section>
    </div>
  );
}
