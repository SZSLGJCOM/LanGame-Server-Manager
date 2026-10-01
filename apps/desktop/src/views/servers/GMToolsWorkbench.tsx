import { ActivityNotice } from "../../components/ActivityNotice";
import { useEffect, useMemo, useState } from "react";
import { describeError } from "../../app-state";
import { sendInstanceGmCommand } from "../../api";
import { ShellIcon } from "../../components/ShellIcon";
import { selectLocaleText, useI18n, type TranslateFn } from "../../i18n";
import type { InstanceDetails } from "../../types";
import {
  instanceHasRunningProcess,
  runtimeProcessKeyIsRunning
} from "../../runtime-action-state";
import {
  buildGmToolCommand,
  getArkGmItemOptions,
  getArkGmNumericItemOptions,
  getDstPrefabOptions,
  getGmToolCatalog,
  getInitialGmToolValues,
  localizeArkCatalogCategory,
  localizeArkCatalogName,
  localizeDstPrefabCategory,
  localizeDstPrefabCategoryTerm,
  localizeDstPrefabName,
  searchArkGmItemOptions,
  searchArkGmNumericItemOptions,
  searchDstPrefabOptions,
  type ArkGmItemOption,
  type GmToolDefinition,
  type GmToolField,
  type GmToolFieldOption
} from "./gm-tools";
import { gmExecutionText, gmToolConfigurationIssue, useGmToolExecution } from "./gm-tool-execution";
import { ArkCreatureSpawner } from "./ArkCreatureSpawner";
import "./gm-tool-execution.css";

interface GMToolsWorkbenchProps {
  details: InstanceDetails;
}

const ARK_CATALOG_SEARCH_RESULT_LIMIT = 12;
const ARK_ITEM_TOTAL_COUNT = getArkGmItemOptions().length;
const ARK_NUMERIC_ITEM_TOTAL_COUNT = getArkGmNumericItemOptions().length;
const DST_PREFAB_SEARCH_RESULT_LIMIT = 12;
const DST_PREFAB_TOTAL_COUNT = getDstPrefabOptions().length;
const DST_PREFAB_CATEGORY_TERMS = [
  "\u88c5\u5907",
  "\u70f9\u996a",
  "\u98df\u7269",
  "\u6750\u6599",
  "\u5de5\u5177",
  "\u9b54\u6cd5",
  "\u9053\u5177",
  "\u79cd\u5b50",
  "\u52a8\u7269",
  "\u77ff\u77f3"
];

function localizedFieldPlaceholder(t: TranslateFn, field: GmToolField): string {
  const fallback = field.placeholder ?? "";
  if (!field.placeholderKey) {
    return fallback;
  }
  return t(field.placeholderKey, undefined, fallback);
}

function localizedFieldOption(t: TranslateFn, option: GmToolFieldOption): string {
  return option.labelKey ? t(option.labelKey, undefined, option.label) : option.label;
}

function groupTools(tools: GmToolDefinition[]): Array<{ category: string; tools: GmToolDefinition[] }> {
  const groups: Array<{ category: string; tools: GmToolDefinition[] }> = [];
  for (const tool of tools) {
    const existing = groups.find((group) => group.category === tool.category);
    if (existing) {
      existing.tools.push(tool);
    } else {
      groups.push({ category: tool.category, tools: [tool] });
    }
  }
  return groups;
}

function fieldListId(tool: GmToolDefinition, field: GmToolField): string {
  return `gm-${tool.id}-${field.key}-options`;
}

function fieldInputId(tool: GmToolDefinition, field: GmToolField): string {
  return `gm-${tool.id}-${field.key}-input`;
}

function copySegment(value: string): string {
  return value.replace(/[^A-Za-z0-9]+/g, "_").replace(/^_+|_+$/g, "").toLowerCase();
}

function localizedCategory(t: TranslateFn, category: string): string {
  return t(`servers.gmTools.categories.${copySegment(category)}`, undefined, category);
}

function localizedToolTitle(t: TranslateFn, tool: GmToolDefinition): string {
  return t(`servers.gmTools.tools.${tool.id}.title`, undefined, tool.title);
}

function localizedToolDescription(t: TranslateFn, tool: GmToolDefinition): string {
  return t(`servers.gmTools.tools.${tool.id}.description`, undefined, tool.description);
}

function localizedToolSubmitLabel(t: TranslateFn, tool: GmToolDefinition): string {
  return t(`servers.gmTools.tools.${tool.id}.submit`, undefined, tool.submitLabel);
}

function localizedFieldLabel(t: TranslateFn, tool: GmToolDefinition, field: GmToolField): string {
  return t(`servers.gmTools.fields.${tool.id}.${field.key}`, undefined, field.label);
}

function isArkModule(moduleId: string): boolean {
  const normalized = moduleId.trim().toLowerCase();
  return normalized === "arksurvivalascended" || normalized === "arksurvivalevolved";
}

function isArkItemCatalogField(moduleId: string, field: GmToolField): boolean {
  return isArkModule(moduleId) && (field.key === "blueprintPath" || field.key === "itemId");
}

function isArkCatalogField(moduleId: string, field: GmToolField): boolean {
  return isArkItemCatalogField(moduleId, field);
}

function isDstModule(moduleId: string): boolean {
  return moduleId.trim().toLowerCase() === "dontstarve";
}

function isDstPrefabField(moduleId: string, field: GmToolField): boolean {
  return isDstModule(moduleId) && field.key === "prefab";
}

function shouldDisableField(moduleId: string, tool: GmToolDefinition, field: GmToolField, values: Record<string, string>): boolean {
  return isDstModule(moduleId)
    && tool.id === "dst_give_item_to_player"
    && field.key === "playerIndex"
    && values.allPlayers === "true";
}

function shouldUseFieldDatalist(moduleId: string, field: GmToolField): boolean {
  return Boolean(field.options?.length) && !isDstPrefabField(moduleId, field) && !isArkCatalogField(moduleId, field);
}

function fieldClassName(moduleId: string, field: GmToolField): string {
  const isFullWidth = field.type === "textarea" || isDstPrefabField(moduleId, field) || isArkCatalogField(moduleId, field);
  const classes = [
    "gmt-field",
    field.key === "blueprintPath" ? "gmt-field--blueprint-path" : "",
    field.key === "lines" ? "gmt-field--lines" : "",
    isFullWidth ? "gmt-field--full" : "",
    field.type === "checkbox" ? "gmt-field--checkbox" : "",
    isDstPrefabField(moduleId, field) ? "gmt-field--dst-prefab" : "",
    isArkCatalogField(moduleId, field) ? "gmt-field--ark-catalog" : "",
    "settings-schema-field",
    isFullWidth ? "settings-schema-field--full" : ""
  ];
  return classes.filter(Boolean).join(" ");
}

function isFieldVisible(field: GmToolField, values: Record<string, string>): boolean {
  if (!field.visibleWhen) {
    return true;
  }
  return field.visibleWhen.values.includes(values[field.visibleWhen.field] ?? "");
}

function fieldGridClassName(moduleId: string, tool: GmToolDefinition): string {
  const classes = ["gmt-fields-grid", "settings-schema-grid"];
  if (isArkModule(moduleId) && tool.id === "ark_give_item_to_player") {
    classes.push("gmt-fields-grid--ark-give-item");
  }
  if (isDstModule(moduleId) && tool.id === "dst_give_item_to_player") {
    classes.push("gmt-fields-grid--dst-item-action");
  }
  return classes.join(" ");
}

function categoryIcon(category: string): "users" | "sun" | "zap" | "shield" | "database" {
  const lower = category.toLowerCase();
  if (lower === "players") return "users";
  if (lower === "world") return "sun";
  if (lower === "server") return "shield";
  if (lower === "items") return "database";
  return "zap";
}

type ArkCatalogKind = "blueprint" | "number";
type ArkCatalogOption = ArkGmItemOption;

interface ArkCatalogPickerAssistProps {
  kind: ArkCatalogKind;
  locale: string;
  onPick: (value: string) => void;
  onQueryChange: (query: string) => void;
  query: string;
  t: TranslateFn;
  value: string;
}

function ArkCatalogPickerAssist(props: ArkCatalogPickerAssistProps) {
  const [pageIndex, setPageIndex] = useState(0);
  const activeQuery = props.query.trim();
  const allResults = useMemo(
    () => searchArkCatalogOptions(props.kind, activeQuery),
    [activeQuery, props.kind]
  );
  const totalCount = arkCatalogTotalCount(props.kind);
  const pageCount = Math.max(1, Math.ceil(allResults.length / ARK_CATALOG_SEARCH_RESULT_LIMIT));
  const safePageIndex = Math.min(pageIndex, pageCount - 1);
  const pageStart = safePageIndex * ARK_CATALOG_SEARCH_RESULT_LIMIT;
  const results = allResults.slice(pageStart, pageStart + ARK_CATALOG_SEARCH_RESULT_LIMIT);
  const visibleStart = allResults.length > 0 ? pageStart + 1 : 0;
  const visibleEnd = pageStart + results.length;
  const canGoPrevious = safePageIndex > 0;
  const canGoNext = safePageIndex < pageCount - 1;
  const activeValue = props.value.trim();

  useEffect(() => {
    setPageIndex(0);
  }, [activeQuery, props.kind]);

  useEffect(() => {
    setPageIndex((current) => Math.min(current, pageCount - 1));
  }, [pageCount]);

  return (
    <div className="gmt-ark-catalog-assist">
      <div className="gmt-ark-catalog-toolbar">
        <input
          className="gmt-text-input gmt-ark-catalog-search"
          value={props.query}
          placeholder={arkCatalogSearchPlaceholder(props.kind, props.t)}
          onChange={(event) => props.onQueryChange(event.target.value)}
        />
        <div className="gmt-ark-catalog-status">
          {activeValue ? (
            <span className="gmt-ark-catalog-current" title={activeValue}>
              <span>{props.t("servers.gmTools.arkCatalogCurrent", undefined, "Current")}</span>
              <code>{activeValue}</code>
            </span>
          ) : null}
          <div className="gmt-ark-catalog-pager" aria-label={props.t("servers.gmTools.arkCatalogPager", undefined, "ARK catalog pages")}>
            <button
              type="button"
              className="gmt-ark-catalog-page-btn"
              disabled={!canGoPrevious}
              title={props.t("servers.gmTools.arkCatalogPreviousPage", undefined, "Previous page")}
              onClick={() => setPageIndex((current) => Math.max(0, current - 1))}
            >
              <ShellIcon name="chevron-left" className="gmt-ark-catalog-page-icon" />
            </button>
            <span className="gmt-ark-catalog-page-label">{safePageIndex + 1}/{pageCount}</span>
            <button
              type="button"
              className="gmt-ark-catalog-page-btn"
              disabled={!canGoNext}
              title={props.t("servers.gmTools.arkCatalogNextPage", undefined, "Next page")}
              onClick={() => setPageIndex((current) => Math.min(pageCount - 1, current + 1))}
            >
              <ShellIcon name="chevron-right" className="gmt-ark-catalog-page-icon" />
            </button>
          </div>
          <span className="gmt-ark-catalog-count">
            {props.t(
              "servers.gmTools.arkCatalogResultCount",
              { start: visibleStart, end: visibleEnd, matches: allResults.length, total: totalCount },
              allResults.length === totalCount
                ? `${visibleStart}-${visibleEnd} / ${totalCount}`
                : `${visibleStart}-${visibleEnd} / ${allResults.length} (${totalCount})`
            )}
          </span>
        </div>
      </div>

      {results.length > 0 ? (
        <div className="gmt-ark-catalog-results" role="listbox">
          {results.map((option) => {
            const optionValue = arkCatalogOptionValue(option, props.kind);
            const optionName = localizeArkCatalogName(option, props.locale);
            const optionCategory = localizeArkCatalogCategory(option, props.locale);
            return (
              <button
                key={`${optionValue}:${option.name}`}
                type="button"
                className={optionValue === activeValue ? "gmt-ark-catalog-option gmt-ark-catalog-option--active" : "gmt-ark-catalog-option"}
                title={`${optionName} / ${optionCategory} / ${arkCatalogOptionCode(option, props.kind)}`}
                aria-pressed={optionValue === activeValue}
                onClick={() => props.onPick(optionValue)}
              >
                <span className="gmt-ark-catalog-option-name">{optionName}</span>
                <span className="gmt-ark-catalog-option-meta">
                  <span>{optionCategory}</span>
                  <code>{arkCatalogOptionCode(option, props.kind)}</code>
                </span>
              </button>
            );
          })}
        </div>
      ) : (
        <div className="gmt-ark-catalog-empty">
          {props.t("servers.gmTools.arkCatalogNoResults", undefined, "No matching ARK entry")}
        </div>
      )}
    </div>
  );
}

function searchArkCatalogOptions(kind: ArkCatalogKind, query: string): ArkCatalogOption[] {
  if (kind === "number") {
    return searchArkGmNumericItemOptions(query, ARK_NUMERIC_ITEM_TOTAL_COUNT);
  }
  return searchArkGmItemOptions(query, ARK_ITEM_TOTAL_COUNT);
}

function arkCatalogTotalCount(kind: ArkCatalogKind): number {
  if (kind === "number") {
    return ARK_NUMERIC_ITEM_TOTAL_COUNT;
  }
  return ARK_ITEM_TOTAL_COUNT;
}

function arkCatalogOptionValue(option: ArkCatalogOption, kind: ArkCatalogKind): string {
  return kind === "number" ? option.itemId : option.blueprintPath;
}

function arkCatalogOptionCode(option: ArkCatalogOption, kind: ArkCatalogKind): string {
  return kind === "number" ? `#${option.itemId}` : option.gfiCode;
}

function arkCatalogSearchPlaceholder(kind: ArkCatalogKind, t: TranslateFn): string {
  if (kind === "number") {
    return t("servers.gmTools.arkItemNumberSearchPlaceholder", undefined, "Search numeric item ID");
  }
  return t("servers.gmTools.arkItemSearchPlaceholder", undefined, "Search item, GFI, or blueprint");
}

interface DstPrefabPickerAssistProps {
  locale: string;
  onPick: (prefab: string) => void;
  onQueryChange: (query: string) => void;
  query: string;
  t: TranslateFn;
  value: string;
}

function DstPrefabPickerAssist(props: DstPrefabPickerAssistProps) {
  const [pageIndex, setPageIndex] = useState(0);
  const activeQuery = props.query.trim();
  const allResults = useMemo(
    () => searchDstPrefabOptions(activeQuery, DST_PREFAB_TOTAL_COUNT),
    [activeQuery]
  );
  const pageCount = Math.max(1, Math.ceil(allResults.length / DST_PREFAB_SEARCH_RESULT_LIMIT));
  const safePageIndex = Math.min(pageIndex, pageCount - 1);
  const pageStart = safePageIndex * DST_PREFAB_SEARCH_RESULT_LIMIT;
  const results = allResults.slice(pageStart, pageStart + DST_PREFAB_SEARCH_RESULT_LIMIT);
  const visibleStart = allResults.length > 0 ? pageStart + 1 : 0;
  const visibleEnd = pageStart + results.length;
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
          <div className="gmt-dst-prefab-pager" aria-label={props.t("servers.gmTools.dstPrefabPager", undefined, "Item pages")}>
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
          <span className="gmt-dst-prefab-count">
            {props.t(
              "servers.gmTools.dstPrefabResultCount",
              {
                count: results.length,
                start: visibleStart,
                end: visibleEnd,
                matches: allResults.length,
                total: DST_PREFAB_TOTAL_COUNT
              },
              allResults.length === DST_PREFAB_TOTAL_COUNT
                ? `${visibleStart}-${visibleEnd} / ${DST_PREFAB_TOTAL_COUNT}`
                : `${visibleStart}-${visibleEnd} / ${allResults.length} (${DST_PREFAB_TOTAL_COUNT})`
            )}
          </span>
        </div>
      </div>

      <div className="gmt-dst-prefab-categories" aria-label={props.t("servers.gmTools.dstPrefabCategories", undefined, "DST item categories")}>
        {DST_PREFAB_CATEGORY_TERMS.map((term) => {
          const label = localizeDstPrefabCategoryTerm(term, props.locale);
          const active = props.query === term || props.query === label;
          return (
            <button
              key={term}
              type="button"
              className={active ? "gmt-dst-prefab-category gmt-dst-prefab-category--active" : "gmt-dst-prefab-category"}
              onClick={() => props.onQueryChange(label)}
            >
              {label}
            </button>
          );
        })}
      </div>

      {results.length > 0 ? (
        <div className="gmt-dst-prefab-results" role="listbox">
          {results.map((option) => {
            const optionName = localizeDstPrefabName(option, props.locale);
            const optionCategory = localizeDstPrefabCategory(option, props.locale);
            return (
              <button
                key={option.value}
                type="button"
                className={option.value === activeValue ? "gmt-dst-prefab-option gmt-dst-prefab-option--active" : "gmt-dst-prefab-option"}
                title={`${optionName} / ${optionCategory} / ${option.value}`}
                aria-pressed={option.value === activeValue}
                onClick={() => props.onPick(option.value)}
              >
                <span className="gmt-dst-prefab-option-name">{optionName}</span>
                <span className="gmt-dst-prefab-option-meta">
                  <span>{optionCategory}</span>
                  <code>{option.value}</code>
                </span>
              </button>
            );
          })}
        </div>
      ) : (
        <div className="gmt-dst-prefab-empty">
          {props.t("servers.gmTools.dstPrefabNoResults", undefined, "No matching item or creature")}
        </div>
      )}
    </div>
  );
}

export function GMToolsWorkbench(props: GMToolsWorkbenchProps) {
  const { locale, t } = useI18n();
  const moduleId = props.details.summary.module_id;
  const [arkCatalogSearchByField, setArkCatalogSearchByField] = useState<Record<string, string>>({});
  const [dstPrefabSearchByField, setDstPrefabSearchByField] = useState<Record<string, string>>({});
  const catalog = useMemo(() => getGmToolCatalog(moduleId), [moduleId]);
  const firstToolId = catalog?.tools[0]?.id ?? "";
  const [activeToolId, setActiveToolId] = useState(firstToolId);
  const [valuesByToolId, setValuesByToolId] = useState<Record<string, Record<string, string>>>({});
  const [feedback, setFeedback] = useState<string | null>(null);
  const [feedbackTone, setFeedbackTone] = useState<"error" | "warning" | "success" | "info">("info");
  const execution = useGmToolExecution(props.details.summary.id, sendInstanceGmCommand, describeError);
  const sendingToolId = execution.sendingToolId;

  useEffect(() => {
    setActiveToolId(firstToolId);
    setValuesByToolId({});
    setFeedback(null);
    setArkCatalogSearchByField({});
    setDstPrefabSearchByField({});
  }, [firstToolId, moduleId, props.details.settings_json, props.details.summary.id]);

  const activeTool = catalog?.tools.find((tool) => tool.id === activeToolId) ?? catalog?.tools[0] ?? null;
  const toolValues = activeTool
    ? valuesByToolId[activeTool.id] ?? getInitialGmToolValues(activeTool)
    : {};
  const commandPreview = activeTool && activeTool.id !== "ark_spawn_creature"
    ? buildGmToolCommand(moduleId, activeTool.id, toolValues, t)
    : null;
  const visibleActiveFields = activeTool
    ? activeTool.fields.filter((field) => isFieldVisible(field, toolValues))
    : [];
  const instanceRunning = instanceHasRunningProcess(props.details.summary, props.details.active_run);
  const commandTargetRunning = commandPreview?.processKey
    ? runtimeProcessKeyIsRunning(props.details.active_run, commandPreview.processKey)
    : instanceRunning;
  const availabilityIssue = commandPreview?.error
    || (commandPreview ? gmToolConfigurationIssue(props.details, commandPreview, locale) : null)
    || (!commandTargetRunning
    ? commandPreview?.processKey && instanceRunning
      ? selectLocaleText(locale, `目标分片或进程 ${commandPreview.processKey} 未运行，请先启动它。`,
        `Target shard or process ${commandPreview.processKey} is not running. Start it first.`)
      : t("servers.gmTools.startFirst", undefined, "Start the server before sending GM commands.")
    : null);
  const toolGroups = useMemo(() => groupTools(catalog?.tools ?? []), [catalog]);

  function updateField(tool: GmToolDefinition, field: GmToolField, value: string) {
    setValuesByToolId((current) => ({
      ...current,
      [tool.id]: {
        ...getInitialGmToolValues(tool),
        ...(current[tool.id] ?? {}),
        [field.key]: value
      }
    }));
    setFeedback(null);
  }

  async function sendActiveTool() {
    if (!activeTool || !commandPreview || execution.busy) {
      return;
    }
    if (availabilityIssue) {
      setFeedbackTone("warning");
      setFeedback(availabilityIssue);
      return;
    }
    if (!commandTargetRunning) {
      setFeedbackTone("warning");
      setFeedback(t("servers.gmTools.startFirst", undefined, "Start the server before sending GM commands."));
      return;
    }
    if (commandPreview.error) {
      setFeedbackTone("warning");
      setFeedback(commandPreview.error);
      return;
    }
    if (commandPreview.commands.length === 0) {
      setFeedbackTone("warning");
      setFeedback(t("servers.gmTools.noCommand", undefined, "This GM tool did not generate a command."));
      return;
    }

    setFeedback(null);
    await execution.run(activeTool.id, commandPreview);
  }

  if (!catalog) return null;

  return (
    <section className="gmt-workbench">
      <div className="gmt-body">
        <aside className="gmt-nav" aria-label={t("servers.gmTools.actions", undefined, "GM actions")}>
          {toolGroups.map((group) => (
            <section key={group.category} className="gmt-nav-group">
              <div className="gmt-nav-group-label">
                <ShellIcon name={categoryIcon(group.category)} className="gmt-nav-group-icon" />
                {localizedCategory(t, group.category)}
              </div>
              {group.tools.map((tool) => (
                <button
                  key={tool.id}
                  type="button"
                  className={tool.id === activeTool?.id ? "gmt-nav-btn gmt-nav-btn--active" : "gmt-nav-btn"}
                  onClick={() => {
                    setActiveToolId(tool.id);
                    setFeedback(null);
                  }}
                >
                  <ShellIcon
                    name={categoryIcon(tool.category)}
                    className="gmt-nav-btn-icon"
                  />
                  <span className="gmt-nav-btn-copy">
                    <span className="gmt-nav-btn-title">{localizedToolTitle(t, tool)}</span>
                    <span className="gmt-nav-btn-desc">{localizedToolDescription(t, tool)}</span>
                  </span>
                </button>
              ))}
            </section>
          ))}
        </aside>

        {isArkModule(moduleId) ? (
          <div hidden={activeTool?.id !== "ark_spawn_creature"}>
            <ArkCreatureSpawner
              instanceId={props.details.summary.id}
              moduleId={moduleId}
              status={props.details.summary.status}
              settingsJson={props.details.settings_json}
            />
          </div>
        ) : null}
        {activeTool?.id === "ark_spawn_creature" ? null : activeTool ? (
          <form
            className="gmt-form"
            onSubmit={(event) => {
              event.preventDefault();
              void sendActiveTool();
            }}
          >
            <div className="gmt-form-header">
              <div className="gmt-form-header-left">
                <h4 className="gmt-form-title">{localizedToolTitle(t, activeTool)}</h4>
                <p className="gmt-form-desc">{localizedToolDescription(t, activeTool)}</p>
              </div>
            </div>

            {visibleActiveFields.length > 0 ? (
              <div className={fieldGridClassName(moduleId, activeTool)}>
                {visibleActiveFields.map((field) => {
                  const inputId = fieldInputId(activeTool, field);
                  const datalistId = fieldListId(activeTool, field);
                  const fieldStateKey = `${activeTool.id}:${field.key}`;
                  const fieldOptions = field.options ?? [];
                  const useDatalist = shouldUseFieldDatalist(moduleId, field);
                  const fieldDisabled = shouldDisableField(moduleId, activeTool, field, toolValues);
                  return (
                  <div
                    key={field.key}
                    className={`${fieldClassName(moduleId, field)}${fieldDisabled ? " gmt-field--disabled" : ""}`}
                  >
                    {field.type === "checkbox" ? null : (
                      <label htmlFor={inputId} className="gmt-field-label detail-label">{localizedFieldLabel(t, activeTool, field)}</label>
                    )}
                    {field.type === "checkbox" ? (
                      <label className="gmt-checkbox-field" htmlFor={inputId}>
                        <input
                          id={inputId}
                          className="gmt-checkbox-input"
                          type="checkbox"
                          checked={(toolValues[field.key] ?? field.defaultValue ?? "false") === "true"}
                          onChange={(event) => updateField(activeTool, field, event.target.checked ? "true" : "false")}
                        />
                        <span>{localizedFieldLabel(t, activeTool, field)}</span>
                      </label>
                    ) : field.type === "select" ? (
                      <select
                        id={inputId}
                        className="gmt-select settings-schema-select"
                        value={toolValues[field.key] ?? field.defaultValue ?? ""}
                        disabled={fieldDisabled}
                        onChange={(event) => updateField(activeTool, field, event.target.value)}
                      >
                        {(field.options ?? []).map((option) => (
                          <option key={option.value} value={option.value}>
                            {localizedFieldOption(t, option)}
                          </option>
                        ))}
                      </select>
                    ) : field.type === "textarea" ? (
                      <textarea
                        id={inputId}
                        className="gmt-textarea settings-schema-textarea"
                        value={toolValues[field.key] ?? ""}
                        placeholder={localizedFieldPlaceholder(t, field)}
                        disabled={fieldDisabled}
                        onChange={(event) => updateField(activeTool, field, event.target.value)}
                      />
                    ) : (
                      <>
                        <input
                          id={inputId}
                          className="gmt-text-input settings-schema-input"
                          type={field.type === "number" ? "number" : "text"}
                          value={toolValues[field.key] ?? ""}
                          placeholder={localizedFieldPlaceholder(t, field)}
                          min={field.min}
                          max={field.max}
                          step={field.step}
                          disabled={fieldDisabled}
                          list={useDatalist ? datalistId : undefined}
                          onChange={(event) => updateField(activeTool, field, event.target.value)}
                        />
                        {useDatalist ? (
                          <datalist id={datalistId}>
                            {fieldOptions.map((option) => (
                              <option key={option.value} value={option.value}>
                                {localizedFieldOption(t, option)}
                              </option>
                            ))}
                          </datalist>
                        ) : null}
                        {isArkItemCatalogField(moduleId, field) ? (
                          <ArkCatalogPickerAssist
                            kind={field.key === "itemId" ? "number" : "blueprint"}
                            locale={locale}
                            query={arkCatalogSearchByField[fieldStateKey] ?? ""}
                            onQueryChange={(query) => {
                              setArkCatalogSearchByField((current) => ({ ...current, [fieldStateKey]: query }));
                            }}
                            onPick={(value) => updateField(activeTool, field, value)}
                            t={t}
                            value={toolValues[field.key] ?? ""}
                          />
                        ) : null}
                        {isDstPrefabField(moduleId, field) ? (
                          <DstPrefabPickerAssist
                            locale={locale}
                            query={dstPrefabSearchByField[fieldStateKey] ?? ""}
                            onQueryChange={(query) => {
                              setDstPrefabSearchByField((current) => ({ ...current, [fieldStateKey]: query }));
                            }}
                            onPick={(prefab) => {
                              updateField(activeTool, field, prefab);
                            }}
                            t={t}
                            value={toolValues[field.key] ?? ""}
                          />
                        ) : null}
                      </>
                    )}
                  </div>
                  );
                })}
              </div>
            ) : (
              <div className="gmt-no-fields-notice">
                <ShellIcon name="check-circle" className="gmt-no-fields-icon" />
                {t("servers.gmTools.noInputNeeded", undefined, "This action does not need extra input.")}
              </div>
            )}

            {availabilityIssue ? <p className="gmt-tool-availability" role="status">{availabilityIssue}</p> : null}
            <div className="gmt-actions-row">
              {feedback ? <ActivityNotice tone={feedbackTone} onDismiss={() => setFeedback(null)}>{feedback}</ActivityNotice> : null}
              <button
                type="submit"
                className="gmt-btn gmt-btn--primary"
                disabled={Boolean(availabilityIssue) || execution.busy}
              >
                <ShellIcon
                  name={sendingToolId === activeTool.id ? "loader" : "send"}
                  className={sendingToolId === activeTool.id ? "gmt-btn-icon gmt-btn-icon--spin" : "gmt-btn-icon"}
                />
                {execution.busy && !sendingToolId
                  ? selectLocaleText(locale, "等待上一条请求…", "Waiting for the earlier request…")
                  : sendingToolId === activeTool.id
                  ? t("servers.gmTools.sendingShort", undefined, "Sending...")
                  : localizedToolSubmitLabel(t, activeTool)}
              </button>
            </div>
            {execution.result ? (
              <section className="gmt-command-results" aria-label={selectLocaleText(locale, "工具执行记录", "Tool dispatch results")}>
                <h5>{selectLocaleText(locale, "工具执行记录", "Tool dispatch results")}</h5>
                <p role={execution.result.error ? "alert" : "status"}>{gmExecutionText(locale, execution.result)}</p>
                {execution.result.responses.length > 0 ? (
                  <ol>
                    {execution.result.responses.map((response, index) => (
                      <li key={`${response.submitted_at_unix_ms}:${index}`}>
                        <code>{response.command}</code>
                        <small>{response.write_confirmation_pending
                          ? selectLocaleText(locale, "等待写入确认", "Awaiting write confirmation")
                          : selectLocaleText(locale, "已发送", "Sent")}</small>
                        <pre>{response.response_text || selectLocaleText(locale,
                          "服务器没有返回响应文本，请查看运行日志或游戏内结果。",
                          "The server returned no response text. Check the runtime log or in-game result.")}</pre>
                      </li>
                    ))}
                  </ol>
                ) : null}
              </section>
            ) : null}
          </form>
        ) : (
          <div className="gmt-form gmt-empty-tool">
            <div className="gmt-no-fields-notice">
              <ShellIcon name="alert-circle" className="gmt-no-fields-icon" />
              {t("servers.gmTools.noToolSelected", undefined, "Select a server tool from the left.")}
            </div>
          </div>
        )}
      </div>
    </section>
  );
}
