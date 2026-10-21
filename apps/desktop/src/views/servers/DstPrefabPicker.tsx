import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { ShellIcon, type ShellIconName } from "../../components/ShellIcon";
import type { TranslateFn } from "../../i18n";
import "./dst-prefab-picker.css";
import type { DstGmItemOption } from "./dst-gm-item-catalog";
import { DST_PREFAB_ID_PATTERN } from "./gm-tool-values";
import { getDstPrefabOptions, localizeDstPrefabCategory, localizeDstPrefabCategoryTerm, localizeDstPrefabName, searchDstPrefabOptions } from "./gm-tools";

// Before the first measurement; afterwards the page holds exactly the rows the results area fits.
const DST_PREFAB_INITIAL_PAGE_SIZE = 6;
const DST_PREFAB_ROW_HEIGHT = 26;
const DST_PREFAB_TOTAL_COUNT = getDstPrefabOptions().length;
// Category toggles reuse the map's GM tool grid and button (icon over label); each term is also the search query it applies.
const DST_PREFAB_CATEGORIES: { term: string; icon: ShellIconName }[] = [
  { term: "\u88c5\u5907", icon: "shield" },
  { term: "\u70f9\u996a", icon: "zap" },
  { term: "\u98df\u7269", icon: "star" },
  { term: "\u6750\u6599", icon: "database" },
  { term: "\u5de5\u5177", icon: "settings" },
  { term: "\u9b54\u6cd5", icon: "eye" },
  { term: "\u9053\u5177", icon: "tag" },
  { term: "\u79cd\u5b50", icon: "circle-dot" },
  { term: "\u52a8\u7269", icon: "users" },
  { term: "\u77ff\u77f3", icon: "hard-drive" }
];

/** Paginated DST item and creature chooser; results stay open after picking so several items can be compared. */
interface DstPrefabPickerProps {
  locale: string;
  onPick: (prefab: string) => void;
  onQueryChange: (query: string) => void;
  query: string;
  t: TranslateFn;
  value: string;
}

type DstPrefabEntry = { custom: true; value: string } | { custom: false; value: string; option: DstGmItemOption };

/** Rows (times grid columns) that fit the results container; it is sized by the rail, never by its content. */
function useDstPrefabPageSize() {
  const container = useRef<HTMLDivElement>(null);
  const [pageSize, setPageSize] = useState(DST_PREFAB_INITIAL_PAGE_SIZE);
  useLayoutEffect(() => {
    const node = container.current;
    if (!node) return;
    const measure = () => {
      const style = getComputedStyle(node), gap = parseFloat(style.rowGap) || 0;
      const columns = Math.max(1, style.gridTemplateColumns.split(" ").filter(Boolean).length);
      const rows = Math.max(1, Math.floor((node.clientHeight + gap) / (DST_PREFAB_ROW_HEIGHT + gap)));
      setPageSize(current => current === rows * columns ? current : rows * columns);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  return { container, pageSize };
}

export function DstPrefabPicker(props: DstPrefabPickerProps) {
  const [pageIndex, setPageIndex] = useState(0);
  const { container, pageSize } = useDstPrefabPageSize();
  const activeQuery = props.query.trim();
  const allResults = useMemo((): DstPrefabEntry[] => {
    const typed = activeQuery.toLowerCase();
    const found = searchDstPrefabOptions(activeQuery, DST_PREFAB_TOTAL_COUNT);
    const exact = getDstPrefabOptions().find(option => option.value === typed);
    const options = (exact ? [exact, ...found.filter(option => option !== exact)] : found).map(option => ({ custom: false as const, value: option.value, option }));
    // A well-formed id outside the catalog (mods, new content) is offered first; the server confirms it exists.
    return !exact && DST_PREFAB_ID_PATTERN.test(typed) ? [{ custom: true, value: typed }, ...options] : options;
  }, [activeQuery]);
  const pageCount = Math.max(1, Math.ceil(allResults.length / pageSize));
  const safePageIndex = Math.min(pageIndex, pageCount - 1);
  const pageStart = safePageIndex * pageSize;
  const results = allResults.slice(pageStart, pageStart + pageSize);
  const canGoPrevious = safePageIndex > 0;
  const canGoNext = safePageIndex < pageCount - 1;
  const activeValue = props.value.trim();

  useEffect(() => {
    setPageIndex(0);
  }, [activeQuery]);

  useEffect(() => {
    setPageIndex((current) => Math.min(current, pageCount - 1));
  }, [pageCount]);

  return (
    <div className="gmt-dst-prefab-assist">
      <div className="gmt-dst-prefab-toolbar">
        <input
          className="gmt-text-input gmt-dst-prefab-search"
          value={props.query}
          placeholder={props.t("servers.gmTools.dstPrefabSearchPlaceholder", undefined, "Search item, creature, or category")}
          onChange={(event) => props.onQueryChange(event.target.value)}
        />
        <div className="gmt-dst-prefab-status">
          {activeValue ? (
            <span className="gmt-dst-prefab-current" title={activeValue}>
              <span>{props.t("servers.gmTools.dstPrefabCurrent", undefined, "Current")}</span>
              <code>{activeValue}</code>
            </span>
          ) : null}
          <div className="gmt-dst-prefab-pager" title={`${allResults.length} / ${DST_PREFAB_TOTAL_COUNT}`} aria-label={props.t("servers.gmTools.dstPrefabPager", undefined, "Item pages")}>
            <button
              type="button"
              className="gmt-dst-prefab-page-btn"
              disabled={!canGoPrevious}
              title={props.t("servers.gmTools.dstPrefabPreviousPage", undefined, "Previous page")}
              onClick={() => setPageIndex((current) => Math.max(0, current - 1))}
            >
              <ShellIcon name="chevron-left" className="gmt-dst-prefab-page-icon" />
            </button>
            <span className="gmt-dst-prefab-page-label">{safePageIndex + 1}/{pageCount}</span>
            <button
              type="button"
              className="gmt-dst-prefab-page-btn"
              disabled={!canGoNext}
              title={props.t("servers.gmTools.dstPrefabNextPage", undefined, "Next page")}
              onClick={() => setPageIndex((current) => Math.min(pageCount - 1, current + 1))}
            >
              <ShellIcon name="chevron-right" className="gmt-dst-prefab-page-icon" />
            </button>
          </div>
        </div>
      </div>


      <div ref={container} className="gmt-dst-prefab-results" role="listbox" data-page-size={pageSize}>
        {results.map((entry) => {
          const useCode = props.t("servers.gmTools.dstPrefabUseCode", undefined, "Use code");
          const name = entry.custom ? `${useCode} ${entry.value}` : localizeDstPrefabName(entry.option, props.locale);
          const title = entry.custom ? name : `${name} / ${localizeDstPrefabCategory(entry.option, props.locale)} / ${entry.value}`;
          const active = entry.value === activeValue;
          return (
            <button
              key={`${entry.custom ? "custom" : "option"}:${entry.value}`}
              type="button"
              className={`gmt-dst-prefab-option${active ? " gmt-dst-prefab-option--active" : ""}${entry.custom ? " gmt-dst-prefab-option--custom" : ""}`}
              title={title}
              aria-pressed={active}
              onClick={() => props.onPick(entry.value)}
            >
              <span className="gmt-dst-prefab-option-name">{entry.custom ? useCode : name}</span>
              <code>{entry.value}</code>
            </button>
          );
        })}
        {results.length ? null : <div className="gmt-dst-prefab-empty">{props.t("servers.gmTools.dstPrefabNoResults", undefined, "No matching item or creature")}</div>}
      </div>
    </div>
  );
}

/** Category toggles in the pinned GM grid; each applies its term as the picker's search query. */
export function DstPrefabCategories(props: { locale: string; query: string; onQueryChange: (query: string) => void; t: TranslateFn }) {
  const label = props.t("servers.gmTools.dstPrefabCategories", undefined, "DST item categories");
  return (
    <div className="dst-gm-tool-grid gmt-dst-prefab-categories" role="group" aria-label={label}>
      {DST_PREFAB_CATEGORIES.map(({ term, icon }) => {
        const name = localizeDstPrefabCategoryTerm(term, props.locale);
        const active = props.query === term || props.query === name;
        return (
          <button key={term} type="button" className="secondary-button dst-gm-tool" aria-pressed={active} title={name}
            onClick={() => props.onQueryChange(active ? "" : name)}>
            <ShellIcon name={icon} /><span>{name}</span>
          </button>
        );
      })}
    </div>
  );
}
