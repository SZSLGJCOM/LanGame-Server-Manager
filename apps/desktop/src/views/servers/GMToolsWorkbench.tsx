import { ActivityNotice } from "../../components/ActivityNotice";
import { ConfigurationActionHelp } from "../settings/ConfigurationFieldHelp";
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
  getGmToolCatalog,
  getInitialGmToolValues,
  type GmCommandBuildResult,
  type GmToolDefinition,
  type GmToolField,
  type GmToolFieldOption
} from "./gm-tools";
import { GmExecutionNotice } from "./GmExecutionNotice";
import { gmToolConfigurationIssue, useGmToolExecution } from "./gm-tool-execution";
import { ArkCreatureSpawner } from "./ArkCreatureSpawner";
import { ArkItemSpawnPanel } from "./ArkItemSpawnPanel";
import { ArkWorldToolsPanel } from "./ArkWorldToolsPanel";
import { ArkWorldMapWorkbench, type ArkMapToolContext } from "./ArkWorldMapWorkbench";
import { DST_GM_PANELS, DstWorldMapWorkbench } from "./DstWorldMapWorkbench";
import { GmToolsWorkspace } from "./GmToolsWorkspace";
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
  const moduleId = props.details.summary.module_id;
  const dontStarve = moduleId === "dontstarve";
  const ark = isArkModule(moduleId);
  const catalog = useMemo(() => getGmToolCatalog(moduleId), [moduleId]);
  const firstToolId = dontStarve ? DST_GM_PANELS[0] : catalog?.tools[0]?.id ?? "";
  const [activeToolId, setActiveToolId] = useState(firstToolId);
  useEffect(() => setActiveToolId(firstToolId), [firstToolId, moduleId, props.details.settings_json, props.details.summary.id]);
  if (!catalog) return null;
  const selectedId = dontStarve ? DST_GM_PANELS.find(panel => panel === activeToolId) ?? DST_GM_PANELS[0]
    : catalog.tools.find(tool => tool.id === activeToolId)?.id ?? firstToolId;
  return <GmToolsWorkspace instanceId={props.details.summary.id} moduleId={moduleId} tools={catalog.tools}
    selectedId={selectedId} onSelect={setActiveToolId} map={ark || dontStarve}>
    {dontStarve ? <DstWorldMapWorkbench details={props.details} panel={DST_GM_PANELS.find(panel => panel === selectedId) ?? DST_GM_PANELS[0]} />
      : ark ? <ArkWorldMapWorkbench details={props.details}>
        {context => <GMToolsForm details={props.details} activeToolId={selectedId} arkContext={context} />}
      </ArkWorldMapWorkbench> : <GMToolsForm {...props} activeToolId={selectedId} />}
  </GmToolsWorkspace>;
}

function GMToolsForm(props: GMToolsWorkbenchProps & { activeToolId: string; arkContext?: ArkMapToolContext }) {
  const { locale, t } = useI18n();
  const moduleId = props.details.summary.module_id;
  const [arkCatalogSearchByField, setArkCatalogSearchByField] = useState<Record<string, string>>({});
  const catalog = useMemo(() => getGmToolCatalog(moduleId), [moduleId]);
  const [valuesByToolId, setValuesByToolId] = useState<Record<string, Record<string, string>>>({});
  const [feedback, setFeedback] = useState<string | null>(null);
  const [feedbackTone, setFeedbackTone] = useState<"error" | "warning" | "success" | "info">("info");
  const [itemTargetMode, setItemTargetMode] = useState<"player" | "manual">("player");
  const [resultScope, setResultScope] = useState<string | null>(null);
  const execution = useGmToolExecution(props.details.summary.id, props.arkContext?.send ?? sendInstanceGmCommand, describeError);
  const sendingToolId = execution.sendingToolId;

  useEffect(() => {
    setValuesByToolId({});
    setItemTargetMode("player");
    setFeedback(null);
    setArkCatalogSearchByField({});
  }, [moduleId, props.details.settings_json, props.details.summary.id]);
  useEffect(() => { setValuesByToolId({}); setItemTargetMode("player"); setFeedback(null); }, [props.arkContext?.scopeKey]);
  useEffect(() => setFeedback(null), [props.activeToolId]);

  const activeTool = catalog?.tools.find((tool) => tool.id === props.activeToolId) ?? catalog?.tools[0] ?? null;
  const savedToolValues = activeTool
    ? valuesByToolId[activeTool.id] ?? getInitialGmToolValues(activeTool)
    : {};
  const selectedItemPlayer = props.arkContext?.live ? props.arkContext.selectedPlayer : null;
  const useSelectedItemPlayer = props.arkContext && activeTool?.id === "ark_give_item_to_player" && itemTargetMode === "player";
  // Derive the target from the current snapshot so changing or clearing selection cannot reuse an old ID.
  const toolValues = useSelectedItemPlayer
    ? { ...savedToolValues, playerId: selectedItemPlayer ? String(selectedItemPlayer.playerId) : "" }
    : savedToolValues;
  const commandPreview = activeTool && activeTool.id !== "ark_spawn_creature"
    ? buildGmToolCommand(moduleId, activeTool.id, toolValues, t)
    : null;
  const visibleActiveFields = activeTool
    ? activeTool.fields.filter((field) => isFieldVisible(field, toolValues))
    : [];
  const instanceRunning = instanceHasRunningProcess(props.details.summary, props.details.active_run);
  const commandTargetRunning = props.arkContext ? props.arkContext.running && Boolean(props.arkContext.scope) : commandPreview?.processKey
    ? runtimeProcessKeyIsRunning(props.details.active_run, commandPreview.processKey)
    : instanceRunning;
  const availabilityIssue = (useSelectedItemPlayer && !selectedItemPlayer
    ? selectLocaleText(locale, "请在地图或名单中选择在线玩家，或展开手动目标输入玩家 ID。", "Select a live player on the map or roster, or expand the manual target to enter a player ID.") : null)
    || commandPreview?.error
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

  async function sendPreparedTool(toolId: string, preview: GmCommandBuildResult) {
    if (execution.busy) return;
    setFeedback(null);
    setResultScope(props.arkContext?.scopeKey ?? null);
    await execution.run(toolId, preview);
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

    await sendPreparedTool(activeTool.id, commandPreview);
  }

  if (!catalog) return null;

  return (
    <>
        {isArkModule(moduleId) ? (
          <div className="ark-creature-tool" hidden={activeTool?.id !== "ark_spawn_creature"}>
            <ArkCreatureSpawner
              instanceId={props.details.summary.id}
              instanceName={props.details.summary.name}
              moduleId={moduleId}
              status={props.details.summary.status}
              settingsJson={props.details.settings_json}
              processKey={props.arkContext?.scope?.processKey ?? "main"}
              runId={props.arkContext?.scope?.runId ?? null}
              runtimeRunning={props.arkContext?.running}
              extensionStatus={props.arkContext?.extension}
              selectedPlayer={props.arkContext?.selectedPlayer ?? null}
            />
          </div>
        ) : null}
        {activeTool?.id === "ark_spawn_creature" ? null
          : props.arkContext && activeTool?.id === "ark_give_item_to_player" ? <ArkItemSpawnPanel tool={activeTool} values={toolValues}
            query={arkCatalogSearchByField[toolValues.itemMode ?? "blueprint"] ?? ""}
            onQueryChange={query => setArkCatalogSearchByField(current => ({ ...current, [toolValues.itemMode ?? "blueprint"]: query }))}
            update={(field, value) => updateField(activeTool, field, value)} selectedPlayer={selectedItemPlayer}
            targetMode={itemTargetMode} onTargetModeChange={mode => { setItemTargetMode(mode); setFeedback(null); }}
            busy={execution.busy} sending={sendingToolId === activeTool.id} issue={availabilityIssue} onSubmit={() => void sendActiveTool()}
            feedback={feedback ? <ActivityNotice tone={feedbackTone} onDismiss={() => setFeedback(null)}>{feedback}</ActivityNotice> : null}
            result={execution.result?.toolId === activeTool.id && resultScope === props.arkContext.scopeKey ? <GmExecutionNotice result={execution.result} /> : null} />
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
        {props.arkContext ? <ArkWorldToolsPanel details={props.details} context={props.arkContext}
          busy={execution.busy} sendingToolId={sendingToolId} onSend={sendPreparedTool}
          result={execution.result && ["ark_set_time", "ark_destroy_wild_dinos"].includes(execution.result.toolId)
            && resultScope === props.arkContext.scopeKey ? <GmExecutionNotice result={execution.result} /> : null} /> : null}
    </>
  );
}
