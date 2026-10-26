import { useLayoutEffect, useMemo, useRef, useState } from "react";
import { ShellIcon } from "../../components/ShellIcon";
import { selectLocaleText, useI18n } from "../../i18n";
import { getArkGmCreatureOptions, getArkGmItemOptions, getArkGmNumericItemOptions, localizeArkCatalogCategory,
  localizeArkCatalogName, searchArkGmCreatureOptions, searchArkGmItemOptions, searchArkGmNumericItemOptions } from "./gm-tools";
import "./ark-catalog-picker.css";

export type ArkCatalogKind = "blueprint" | "number" | "creature";
interface Props { kind: ArkCatalogKind; query: string; value: string; onQueryChange: (query: string) => void; onPick: (value: string) => void }
const ROW_HEIGHT = 26;

/** The map rail owns the available height; only the rows that fit are paged in. */
export function ArkCatalogPicker({ kind, query, value, onQueryChange, onPick }: Props) {
  const { locale } = useI18n();
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const container = useRef<HTMLDivElement>(null);
  const [pageSize, setPageSize] = useState(6);
  const [page, setPage] = useState({ query, kind, index: 0 });
  useLayoutEffect(() => {
    const node = container.current;
    if (!node) return;
    const measure = () => {
      const gap = parseFloat(getComputedStyle(node).rowGap) || 0;
      setPageSize(Math.max(1, Math.floor((node.clientHeight + gap) / (ROW_HEIGHT + gap))));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  const results = useMemo(() => {
    if (kind === "creature") return searchArkGmCreatureOptions(query, getArkGmCreatureOptions().length).map(option => ({
      key: option.value, value: option.value, name: localizeArkCatalogName(option, locale), code: option.value, category: localizeArkCatalogCategory(option, locale)
    }));
    const options = kind === "number" ? searchArkGmNumericItemOptions(query, getArkGmNumericItemOptions().length)
      : searchArkGmItemOptions(query, getArkGmItemOptions().length);
    return options.map(option => ({
      key: `${kind === "number" ? option.itemId : option.blueprintPath}:${option.name}`,
      value: kind === "number" ? option.itemId : option.blueprintPath,
      name: localizeArkCatalogName(option, locale),
      code: kind === "number" ? `#${option.itemId}` : option.gfiCode,
      category: localizeArkCatalogCategory(option, locale)
    }));
  }, [kind, query, locale]);
  const pageCount = Math.max(1, Math.ceil(results.length / pageSize));
  const index = Math.min(page.query === query && page.kind === kind ? page.index : 0, pageCount - 1);
  const selected = results.find(option => option.value === value);
  const go = (next: number) => setPage({ query, kind, index: next });
  const placeholder = kind === "creature" ? text("查找生物名称或类名", "Search creature names or classes")
    : kind === "number" ? text("查找物品名称或编号", "Search item names or numbers")
    : text("查找物品、GFI 或蓝图", "Search items, GFI, or blueprints");
  return <section className="gmt-ark-catalog-assist" aria-label={kind === "creature" ? text("生物清单", "Creature catalog") : text("物品清单", "Item catalog")}>
    <div className="gmt-ark-catalog-toolbar">
      <input type="search" className="gmt-text-input gmt-ark-catalog-search" aria-label={placeholder}
        placeholder={placeholder} value={query} onChange={event => onQueryChange(event.target.value)} />
      <div className="gmt-ark-catalog-status">
        <span className="gmt-ark-catalog-current" title={value}>{selected?.name || value || text("尚未选择", "Nothing selected")}</span>
        <div className="gmt-ark-catalog-pager" aria-label={text("清单分页", "Catalog pages")} title={text(`${results.length} 项`, `${results.length} entries`)}>
          <button type="button" className="gmt-ark-catalog-page-btn" disabled={index === 0} aria-label={text("上一页", "Previous page")} onClick={() => go(index - 1)}><ShellIcon name="chevron-left" /></button>
          <span className="gmt-ark-catalog-page-label">{index + 1}/{pageCount}</span>
          <button type="button" className="gmt-ark-catalog-page-btn" disabled={index + 1 === pageCount} aria-label={text("下一页", "Next page")} onClick={() => go(index + 1)}><ShellIcon name="chevron-right" /></button>
        </div>
      </div>
    </div>
    <div ref={container} className="gmt-ark-catalog-results" data-page-size={pageSize} role="group" aria-label={text("匹配条目", "Matching entries")}>
      {results.slice(index * pageSize, (index + 1) * pageSize).map(option => <button type="button" key={option.key}
        className={`gmt-ark-catalog-option${option.value === value ? " gmt-ark-catalog-option--active" : ""}`}
        title={`${option.name} / ${option.category} / ${option.code}`} aria-pressed={option.value === value} onClick={() => onPick(option.value)}>
        <span className="gmt-ark-catalog-option-name">{option.name}</span><code>{option.code}</code>
      </button>)}
      {!results.length ? <p className="gmt-ark-catalog-empty">{text("没有匹配条目", "No matching entries")}</p> : null}
    </div>
  </section>;
}
