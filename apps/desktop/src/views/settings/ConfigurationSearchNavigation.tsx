import { useMemo, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { configurationNavigationRoots, searchConfigurationItems } from "./configuration-workspace-model";
import { ConfigurationSectionNavigation } from "./ConfigurationSectionNavigation";
import type { ConfigurationWorkspaceModel } from "./settings-schema";

interface ConfigurationSearchNavigationProps {
  model: ConfigurationWorkspaceModel;
  selectedSectionId: string;
  onSelectSection(sectionId: string): void;
  onSelectField(fieldKey: string): void;
}

export function ConfigurationSearchNavigation(props: ConfigurationSearchNavigationProps) {
  const { locale, t } = useI18n();
  const [query, setQuery] = useState("");
  const searchInput = useRef<HTMLInputElement>(null);
  const navigationRoots = useMemo(() => configurationNavigationRoots(props.model.roots), [props.model]);
  const results = useMemo(() => searchConfigurationItems(props.model, query, locale)
    .filter((item) => item.owner === "configuration" &&
      (item.state === "editable" || item.state === "specialized")), [locale, props.model, query]);
  const searching = query.trim().length > 0;
  const searchLabel = t("settings.configuration.workspace.search", undefined, "Search configuration");

  function selectField(fieldKey: string) {
    setQuery("");
    props.onSelectField(fieldKey);
  }

  return <>
    <div className="configuration-search">
      <input ref={searchInput} type="search" value={query} aria-label={searchLabel}
        placeholder={searchLabel} autoComplete="off" spellCheck={false}
        onChange={(event) => setQuery(event.target.value)}
        onKeyDown={(event) => {
          if (event.nativeEvent.isComposing) return;
          if (event.key === "Escape") {
            event.preventDefault();
            setQuery("");
          } else if (event.key === "Enter" && results[0]) {
            event.preventDefault();
            selectField(results[0].fieldKey);
          }
        }} />
      {searching ? <button type="button" className="configuration-search__clear"
        aria-label={t("settings.configuration.workspace.clearSearch", undefined, "Clear search")}
        onClick={() => { setQuery(""); searchInput.current?.focus(); }}>×</button> : null}
    </div>
    {searching ? <nav className="configuration-search-results" aria-label={searchLabel}>
      <p className="configuration-search-results__count" role="status">
        {results.length > 0
          ? t("settings.configuration.workspace.resultCount", { count: results.length }, "Settings found: {count}")
          : t("settings.configuration.workspace.noResults", undefined, "No matching settings. Try a setting name or native key.")}
      </p>
      <ul>{results.map((result) => <li key={result.fieldKey}>
        <button type="button" onClick={() => selectField(result.fieldKey)}>
          <strong>{result.title}</strong>
          <span>{result.breadcrumb.join(" / ")}</span>
          {result.sourceKey ? <code>{result.sourceKey}</code> : null}
        </button>
      </li>)}</ul>
    </nav> : <ConfigurationSectionNavigation roots={navigationRoots}
      selectedSectionId={props.selectedSectionId}
      ariaLabel={t("settings.configuration.workspace.sections", undefined, "Configuration sections")}
      onSelectSection={props.onSelectSection} />}
  </>;
}
