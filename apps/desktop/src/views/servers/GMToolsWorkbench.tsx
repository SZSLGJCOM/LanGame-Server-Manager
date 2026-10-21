import { ActivityNotice } from "../../components/ActivityNotice";
import { InlineConfirmAction } from "../../components/InlineConfirmAction";
import { ConfigurationActionHelp } from "../settings/ConfigurationFieldHelp";
import { useEffect, useId, useMemo, useRef, useState } from "react";
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
  getGmToolCatalog,
  getInitialGmToolValues,
  localizeArkCatalogCategory,
  localizeArkCatalogName,
  searchArkGmItemOptions,
  searchArkGmNumericItemOptions,
  type ArkGmItemOption,
  type GmToolDefinition,
  type GmToolField,
  type GmToolFieldOption
} from "./gm-tools";
import { GmExecutionNotice } from "./GmExecutionNotice";
import { gmToolConfigurationIssue, useGmToolExecution } from "./gm-tool-execution";
import { ArkCreatureSpawner } from "./ArkCreatureSpawner";
import { DST_GM_PANELS, DstWorldMapWorkbench } from "./DstWorldMapWorkbench";
import { GmToolsNavigation } from "./GmToolsNavigation";
import "./gm-tool-execution.css";

interface GMToolsWorkbenchProps {
  details: InstanceDetails;
}

const ARK_CATALOG_SEARCH_RESULT_LIMIT = 12;
const ARK_ITEM_TOTAL_COUNT = getArkGmItemOptions().length;
const ARK_NUMERIC_ITEM_TOTAL_COUNT = getArkGmNumericItemOptions().length;

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

function fieldListId(tool: GmToolDefinition, field: GmToolField): string {
  return `gm-${tool.id}-${field.key}-options`;
}

function fieldInputId(tool: GmToolDefinition, field: GmToolField): string {
  return `gm-${tool.id}-${field.key}-input`;
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


function shouldUseFieldDatalist(moduleId: string, field: GmToolField): boolean {
  return Boolean(field.options?.length) && !isArkCatalogField(moduleId, field);
}

function fieldClassName(moduleId: string, field: GmToolField): string {
  const isFullWidth = field.type === "textarea" || isArkCatalogField(moduleId, field);
  const classes = [
    "gmt-field",
    field.key === "blueprintPath" ? "gmt-field--blueprint-path" : "",
    field.key === "lines" ? "gmt-field--lines" : "",
    isFullWidth ? "gmt-field--full" : "",
    field.type === "checkbox" ? "gmt-field--checkbox" : "",
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
  return classes.join(" ");
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

export function GMToolsWorkbench(props: GMToolsWorkbenchProps) {
  const { locale, t } = useI18n();
  const navigationId = useId();
  const navigationToggle = useRef<HTMLButtonElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const [navigationOpen, setNavigationOpen] = useState(false);
  const moduleId = props.details.summary.module_id;
  const [arkCatalogSearchByField, setArkCatalogSearchByField] = useState<Record<string, string>>({});
  const dontStarve = moduleId === "dontstarve";
  const catalog = useMemo(() => getGmToolCatalog(moduleId), [moduleId]);
  const firstToolId = dontStarve ? DST_GM_PANELS[0] : catalog?.tools[0]?.id ?? "";
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
    setNavigationOpen(false);
  }, [firstToolId, moduleId, props.details.settings_json, props.details.summary.id]);

  const activeTool = catalog?.tools.find((tool) => tool.id === activeToolId) ?? catalog?.tools[0] ?? null;
  const dstPanel = DST_GM_PANELS.find(panel => panel === activeToolId) ?? DST_GM_PANELS[0];
  const toolValues = activeTool
    ? valuesByToolId[activeTool.id] ?? getInitialGmToolValues(activeTool)
    : {};
  const commandPreview = !dontStarve && activeTool && activeTool.id !== "ark_spawn_creature"
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
    <section className={`configuration-workspace gmt-workbench${dontStarve ? " gmt-workbench--map" : ""}`}>
      <div className="configuration-workspace__body" data-navigation-open={navigationOpen || undefined}
        onKeyDown={event => {
          if (event.key === "Escape" && !event.nativeEvent.isComposing && navigationOpen) {
            event.stopPropagation(); setNavigationOpen(false); navigationToggle.current?.focus();
          }
        }}>
        <button type="button" ref={navigationToggle} className="configuration-workspace__navigation-toggle"
          aria-expanded={navigationOpen} aria-controls={navigationId}
          onClick={() => setNavigationOpen(open => !open)}>
          <ShellIcon name="list" aria-hidden="true" />
          {t("servers.gmTools.actions", undefined, "GM actions")}
          <ShellIcon name="chevron-right" aria-hidden="true" />
        </button>
        <aside id={navigationId} className="configuration-workspace__sidebar gmt-tools-sidebar"
          aria-label={t("servers.gmTools.actions", undefined, "GM actions")}>
          <GmToolsNavigation moduleId={moduleId} tools={catalog.tools} selectedId={dontStarve ? dstPanel : activeTool?.id ?? ""}
            onSelect={id => {
              setActiveToolId(id); setFeedback(null); setNavigationOpen(false);
              content.current?.scrollTo({ top: 0 });
              if (navigationToggle.current?.getClientRects().length) navigationToggle.current.focus();
            }} />
        </aside>
        <div ref={content} className="configuration-workspace__main" data-configuration-scroll-owner>
          <div className="configuration-workspace__content">
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
        {dontStarve ? <DstWorldMapWorkbench details={props.details} panel={dstPanel} /> : activeTool?.id === "ark_spawn_creature" ? null : activeTool ? (
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
                                  return (
                  <div
                    key={field.key}
                    className={`${fieldClassName(moduleId, field)}`}
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

            <div className="gmt-actions-row">
              {feedback ? <ActivityNotice tone={feedbackTone} onDismiss={() => setFeedback(null)}>{feedback}</ActivityNotice> : null}
              <ConfigurationActionHelp reason={availabilityIssue || null}>
              {activeTool.id === "ark_destroy_wild_dinos" ? (
                <InlineConfirmAction
                  scopeKey={`gmt:${activeTool.id}`}
                  className="gmt-btn gmt-btn--danger"
                  disabled={Boolean(availabilityIssue) || execution.busy}
                  confirmation={selectLocaleText(locale, "将清除所有野生生物，无法撤销。确认执行？", "This removes every wild creature and cannot be undone. Continue?")}
                  onConfirm={() => sendActiveTool()}
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
                </InlineConfirmAction>
              ) : (
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
              )}
              </ConfigurationActionHelp>
            </div>
            {execution.result ? <GmExecutionNotice result={execution.result} /> : null}
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
        </div>
      </div>
    </section>
  );
}
