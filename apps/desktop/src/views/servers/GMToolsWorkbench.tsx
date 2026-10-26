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
  getGmToolCatalog,
  getInitialGmToolValues,
  type GmToolDefinition,
  type GmToolField,
  type GmToolFieldOption
} from "./gm-tools";
import { GmExecutionNotice } from "./GmExecutionNotice";
import { gmToolConfigurationIssue, useGmToolExecution } from "./gm-tool-execution";
import { ArkCreatureSpawner } from "./ArkCreatureSpawner";
import { ArkItemSpawnPanel } from "./ArkItemSpawnPanel";
import { ArkWorldMapWorkbench, type ArkMapToolContext } from "./ArkWorldMapWorkbench";
import { DST_GM_PANELS, DstWorldMapWorkbench } from "./DstWorldMapWorkbench";
import { GmToolsNavigation } from "./GmToolsNavigation";
import "./gm-tool-execution.css";

interface GMToolsWorkbenchProps {
  details: InstanceDetails;
}

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

function fieldClassName(field: GmToolField): string {
  const isFullWidth = field.type === "textarea";
  const classes = [
    "gmt-field",
    field.key === "blueprintPath" ? "gmt-field--blueprint-path" : "",
    field.key === "lines" ? "gmt-field--lines" : "",
    isFullWidth ? "gmt-field--full" : "",
    field.type === "checkbox" ? "gmt-field--checkbox" : "",
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


export function GMToolsWorkbench(props: GMToolsWorkbenchProps) {
  return isArkModule(props.details.summary.module_id)
    ? <section className="gmt-workbench gmt-ark-map-workbench"><ArkWorldMapWorkbench details={props.details}>
      {context => <GMToolsForm details={props.details} arkContext={context} />}
    </ArkWorldMapWorkbench></section>
    : <GMToolsForm {...props} />;
}

function GMToolsForm(props: GMToolsWorkbenchProps & { arkContext?: ArkMapToolContext }) {
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
  const [playerFromMap, setPlayerFromMap] = useState<number | null>(null);
  const [resultScope, setResultScope] = useState<string | null>(null);
  const execution = useGmToolExecution(props.details.summary.id, props.arkContext?.send ?? sendInstanceGmCommand, describeError);
  const sendingToolId = execution.sendingToolId;

  useEffect(() => {
    setActiveToolId(firstToolId);
    setValuesByToolId({});
    setFeedback(null);
    setArkCatalogSearchByField({});
    setNavigationOpen(false);
  }, [firstToolId, moduleId, props.details.settings_json, props.details.summary.id]);
  useEffect(() => { setValuesByToolId({}); setPlayerFromMap(null); setFeedback(null); }, [props.arkContext?.scopeKey]);

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
  const commandTargetRunning = props.arkContext ? props.arkContext.running && Boolean(props.arkContext.scope) : commandPreview?.processKey
    ? runtimeProcessKeyIsRunning(props.details.active_run, commandPreview.processKey)
    : instanceRunning;
  const availabilityIssue = (visibleActiveFields.some(field => field.key === "playerId") && playerFromMap !== null
    && (!props.arkContext?.live || props.arkContext.selectedPlayer?.playerId !== playerFromMap)
    ? selectLocaleText(locale, "所选玩家位置已过期或已离开，请重新选取或手动输入玩家 ID。", "The selected player is stale or has left. Select again or enter a player ID manually.") : null)
    || commandPreview?.error
    || (commandPreview ? gmToolConfigurationIssue(props.details, commandPreview, locale) : null)
    || (!commandTargetRunning
    ? commandPreview?.processKey && instanceRunning
      ? selectLocaleText(locale, `目标分片或进程 ${commandPreview.processKey} 未运行，请先启动它。`,
        `Target shard or process ${commandPreview.processKey} is not running. Start it first.`)
      : t("servers.gmTools.startFirst", undefined, "Start the server before sending GM commands.")
    : null);

  function updateField(tool: GmToolDefinition, field: GmToolField, value: string) {
    if (field.key === "playerId") setPlayerFromMap(null);
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
    setResultScope(props.arkContext?.scopeKey ?? null);
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
        {props.arkContext ? <label className="ark-map-tool-picker">
          <select className="gmt-select" aria-label={t("servers.gmTools.actions", undefined, "GM actions")} value={activeTool?.id ?? ""} onChange={event => { setActiveToolId(event.target.value); setFeedback(null); }}>
            {catalog.tools.map(tool => <option key={tool.id} value={tool.id}>{localizedToolTitle(t, tool)}</option>)}
          </select></label> : null}
        {isArkModule(moduleId) ? (
          <div className="ark-creature-tool" hidden={activeTool?.id !== "ark_spawn_creature"}>
            <ArkCreatureSpawner
              instanceId={props.details.summary.id}
              moduleId={moduleId}
              status={props.details.summary.status}
              settingsJson={props.details.settings_json}
              processKey={props.arkContext?.scope?.processKey ?? "main"}
              runId={props.arkContext?.scope?.runId ?? null}
              runtimeRunning={props.arkContext?.running}
              selectedPlayer={props.arkContext?.selectedPlayer ?? null}
            />
          </div>
        ) : null}
        {dontStarve ? <DstWorldMapWorkbench details={props.details} panel={dstPanel} /> : activeTool?.id === "ark_spawn_creature" ? null
          : props.arkContext && activeTool?.id === "ark_give_item_to_player" ? <ArkItemSpawnPanel tool={activeTool} values={toolValues}
            query={arkCatalogSearchByField[toolValues.itemMode ?? "blueprint"] ?? ""}
            onQueryChange={query => setArkCatalogSearchByField(current => ({ ...current, [toolValues.itemMode ?? "blueprint"]: query }))}
            update={(field, value) => updateField(activeTool, field, value)} selectedPlayer={props.arkContext.selectedPlayer}
            onUsePlayer={() => { const player = props.arkContext?.selectedPlayer, field = activeTool.fields.find(value => value.key === "playerId");
              if (player && field) { updateField(activeTool, field, String(player.playerId)); setPlayerFromMap(player.playerId); } }}
            busy={execution.busy} sending={sendingToolId === activeTool.id} issue={availabilityIssue} onSubmit={() => void sendActiveTool()}
            feedback={feedback ? <ActivityNotice tone={feedbackTone} onDismiss={() => setFeedback(null)}>{feedback}</ActivityNotice> : null}
            result={execution.result && resultScope === props.arkContext.scopeKey ? <GmExecutionNotice result={execution.result} /> : null} />
          : activeTool ? (
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

            {props.arkContext && visibleActiveFields.some(field => field.key === "playerId") ? <button type="button"
              className="secondary-button ark-map-use-player" disabled={!props.arkContext.selectedPlayer}
              onClick={() => { const player = props.arkContext?.selectedPlayer, field = activeTool.fields.find(field => field.key === "playerId");
                if (player && field) { updateField(activeTool, field, String(player.playerId)); setPlayerFromMap(player.playerId); } }}>
              {props.arkContext.selectedPlayer ? selectLocaleText(locale, `使用所选玩家 · ${props.arkContext.selectedPlayer.name || props.arkContext.selectedPlayer.playerId}`,
                `Use selected player · ${props.arkContext.selectedPlayer.name || props.arkContext.selectedPlayer.playerId}`)
                : selectLocaleText(locale, "在地图或名单中选择在线玩家", "Select a live player on the map or roster")}</button> : null}

            {visibleActiveFields.length > 0 ? (
              <div className="gmt-fields-grid settings-schema-grid">
                {visibleActiveFields.map((field) => {
                  const inputId = fieldInputId(activeTool, field);
                  const datalistId = fieldListId(activeTool, field);
                  const fieldOptions = field.options ?? [];
                  const useDatalist = Boolean(field.options?.length);
                  return (
                  <div
                    key={field.key}
                    className={fieldClassName(field)}
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
            {execution.result && (!props.arkContext || resultScope === props.arkContext.scopeKey) ? <GmExecutionNotice result={execution.result} /> : null}
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
