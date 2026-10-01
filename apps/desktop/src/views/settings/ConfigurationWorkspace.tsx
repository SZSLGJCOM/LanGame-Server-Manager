import { ActivityNotice } from "../../components/ActivityNotice";
import { useLayoutEffect, useMemo, useRef, useState } from "react";
import { ShellIcon } from "../../components/ShellIcon";
import { getLocalizedModuleDisplayName } from "../../store-media";
import { useI18n } from "../../i18n";
import type {
  BindAddressCandidate,
  InstanceDetails,
  InstanceRuntimeOverview,
  LaunchPlan,
  ModuleDetails,
  SaveInstanceSettingsOptions,
  UpdateInstanceInput
} from "../../types";
import { ConfigurationSearchNavigation } from "./ConfigurationSearchNavigation";
import { ConfigurationSaveStatus } from "./ConfigurationSaveStatus";
import { ConfigurationLoadError } from "./ConfigurationLoadError";
import { GuidedSettingsForm } from "./GuidedSettingsForm";
import { InstanceConnectionSettingsPanel } from "./InstanceConnectionSettingsPanel";
import {
  buildConfigurationWorkspaceModel,
  resolveConfigurationSectionId
} from "./configuration-workspace-model";
import {
  findConfigurationSectionNode,
  focusConfigurationControl,
  mergeConfigurationPatch,
  resolveConfigurationFieldNavigation
} from "./configuration-workspace-state";
import {
  parseGuidedSettingsSchema,
  parseSettingsObject,
  readGuidedFieldValue,
  serializeSettingsObject,
  validateGuidedSettingsObject,
  writeGuidedFieldValue
} from "./guided-settings";
import {
  listConfigurationSpecializedRenderers,
  resolveSettingsModuleDefinition
} from "./module-registry";
import type {
  GuidedFieldChange,
  GuidedSettingsSchema,
  GuidedSettingsValidationIssue,
  SettingsObject
} from "./settings-schema";
import { useAutoSaveInstanceSettings } from "./useAutoSaveInstanceSettings";
import { useInstancePortRegistration } from "./useInstancePortRegistration";
import { useModuleConfigurationIcons } from "./useModuleConfigurationIcons";
import type { InstanceArchiveDetails } from "../../storage-management-types";
import { buildSavedConfigurationSchema } from "./saved-configuration-schema";

export {
  focusConfigurationControl,
  mergeConfigurationPatch,
  resolveConfigurationFieldNavigation
} from "./configuration-workspace-state";

export interface ConfigurationWorkspaceProps {
  details: InstanceDetails;
  moduleDetails: ModuleDetails | null;
  moduleDetailsError: string | null;
  onRetryModuleDetails: () => void;
  bindAddressCandidates: BindAddressCandidate[];
  runtime: InstanceRuntimeOverview | null;
  launchPlan: LaunchPlan | null;
  launchPlanError: string | null;
  archive?: InstanceArchiveDetails;
  onSave?(
    input: UpdateInstanceInput,
    options?: SaveInstanceSettingsOptions
  ): Promise<InstanceDetails | undefined>;
}

interface ModelResult {
  model: ReturnType<typeof buildConfigurationWorkspaceModel> | null;
  error: string | null;
}

function buildModel(schema: GuidedSettingsSchema): ModelResult {
  try {
    return { model: buildConfigurationWorkspaceModel(schema), error: null };
  } catch (error) {
    return { model: null, error: String((error as Error).message || error) };
  }
}

function uniqueIssues(issues: GuidedSettingsValidationIssue[]): GuidedSettingsValidationIssue[] {
  const seen = new Set<string>();
  return issues.filter((issue) => {
    const signature = `${issue.fieldKey}\u0000${issue.message}`;
    if (seen.has(signature)) return false;
    seen.add(signature);
    return true;
  });
}

export function ConfigurationWorkspace(props: ConfigurationWorkspaceProps) {
  const { locale, t } = useI18n();
  const readOnly = Boolean(props.archive);
  const [navigationOpen, setNavigationOpen] = useState(false);
  const navigationToggle = useRef<HTMLButtonElement>(null);
  const moduleId = props.details.summary.module_id;
  const configurationIcons = useModuleConfigurationIcons(
    props.moduleDetails?.summary.id === moduleId ? props.moduleDetails : null,
    props.details.summary.id
  );
  const moduleDefinition = useMemo(() => resolveSettingsModuleDefinition(moduleId), [moduleId]);
  const localizedModuleDetails = useMemo(() => {
    if (!props.moduleDetails) return null;
    const name = getLocalizedModuleDisplayName(
      props.moduleDetails.summary.id,
      locale,
      props.moduleDetails.summary.name
    );
    return name === props.moduleDetails.summary.name ? props.moduleDetails : {
      ...props.moduleDetails,
      summary: { ...props.moduleDetails.summary, name }
    };
  }, [locale, props.moduleDetails]);
  const moduleContext = useMemo(() => ({ locale, t }), [locale, t]);
  const [bindIp, setBindIp] = useState(props.details.summary.bind_ip);
  const [settingsJson, setSettingsJson] = useState(() => {
    const parsed = parseSettingsObject(props.details.settings_json, t);
    if (readOnly || !parsed.value || !moduleDefinition?.initializeSettings) {
      return props.details.settings_json;
    }
    return serializeSettingsObject(moduleDefinition.initializeSettings(parsed.value, moduleContext));
  });
  const [requestedSectionId, setRequestedSectionId] = useState<string | null>(null);
  const [requestedFocusId, setRequestedFocusId] = useState<string | null>(null);
  const [networkValidationBlocked, setNetworkValidationBlocked] = useState(false);
  const { ports, defaultPorts, setPorts } = useInstancePortRegistration(
    props.details,
    localizedModuleDetails
  );

  const moduleSchema = useMemo(
    () => parseGuidedSettingsSchema(localizedModuleDetails, locale, t, { fieldIcons: configurationIcons.icons }),
    [configurationIcons.icons, locale, localizedModuleDetails, t]
  );
  const settingsParseResult = useMemo(
    () => parseSettingsObject(settingsJson, t),
    [settingsJson, t]
  );
  const schema = useMemo(() => readOnly
    ? buildSavedConfigurationSchema(moduleSchema, settingsParseResult.value ?? {}, t) : moduleSchema,
  [moduleSchema, readOnly, settingsParseResult.value, t]);
  const defaultContext = useMemo(() => ({
    instanceId: props.details.summary.id,
    instanceName: props.details.summary.name
  }), [props.details.summary.id, props.details.summary.name]);
  const modelResult = useMemo(() => buildModel(schema), [schema]);
  const model = modelResult.model;
  const validationSchema = useMemo(() => ({
    ...schema,
    fields: schema.fields.filter((field) =>
      !moduleDefinition?.isFieldDisabled?.(field, settingsParseResult.value ?? {})
    )
  }), [moduleDefinition, schema, settingsParseResult.value]);
  const schemaIssues = useMemo(() => {
    if (readOnly || !settingsParseResult.value) return [];
    const activeFields = new Set(validationSchema.fields.map((field) => field.key));
    return validateGuidedSettingsObject(schema, settingsParseResult.value, defaultContext, t)
      .filter((issue) => activeFields.has(issue.fieldKey) ||
        ["maxLength", "number", "managedDirective"].includes(issue.reason));
  }, [defaultContext, schema, validationSchema, settingsParseResult.value, t, readOnly]);
  const moduleIssues = useMemo(() => {
    if (readOnly || !settingsParseResult.value) return [];
    const fieldIssues = validationSchema.fields.flatMap((field) => {
      const message = moduleDefinition?.getFieldValidationMessage?.({
        field,
        value: readGuidedFieldValue(field, settingsParseResult.value!, defaultContext),
        settings: settingsParseResult.value!,
        locale,
        t
      });
      return message ? [{ fieldKey: field.key, reason: "module", message }] : [];
    });
    return uniqueIssues([
      ...fieldIssues,
      ...(moduleDefinition?.getSettingsValidationIssues?.(settingsParseResult.value, moduleContext) ?? [])
    ]);
  }, [defaultContext, locale, moduleContext, moduleDefinition, validationSchema.fields, settingsParseResult.value, t, readOnly]);
  const validationIssues = useMemo(
    () => uniqueIssues([...schemaIssues, ...moduleIssues]),
    [moduleIssues, schemaIssues]
  );
  const loading = localizedModuleDetails === null && !props.moduleDetailsError;
  const moduleMismatch = Boolean(
    localizedModuleDetails && localizedModuleDetails.summary.id !== moduleId
  );
  const workspaceError = props.moduleDetailsError ?? schema.parseError ?? settingsParseResult.error ?? modelResult.error;
  const editorDisabled = readOnly || loading || moduleMismatch || Boolean(workspaceError);
  const validationBlocked = validationIssues.length > 0 || networkValidationBlocked;
  const saveBlocked = editorDisabled || validationBlocked;
  const { status: saveStatus, retry: retrySave } = useAutoSaveInstanceSettings({
    enabled: !readOnly,
    details: props.details,
    bindIp,
    autoBackupOnStop: props.details.auto_backup_on_stop,
    backupRetentionCount: String(props.details.backup_retention_count),
    settingsJson,
    ports,
    ready: !loading && !moduleMismatch && !props.moduleDetailsError &&
      !schema.parseError && !modelResult.error,
    disabled: saveBlocked,
    onSave: props.onSave
  });

  const selectedSectionId = model
    ? resolveConfigurationSectionId(model, requestedSectionId)
    : null;
  const activeNode = model
    ? findConfigurationSectionNode(model.roots, selectedSectionId)
    : null;
  const activeGuidedFields = selectedSectionId
    ? schema.fields.filter((field) => field.sectionId === selectedSectionId)
    : [];
  const showIconNotice = moduleId === "dontstarve" &&
    ["mastergen", "mastersettings", "cavesgen", "cavessettings"].includes(selectedSectionId ?? "") &&
    Boolean(configurationIcons.error || configurationIcons.missing);
  const idPrefix = props.archive ? `configuration-archive-${props.archive.archive_id}` : `configuration-${moduleId}`;
  const rawJson = readOnly ? <details className="configuration-workspace__raw"
    open={Boolean(workspaceError || moduleMismatch) || undefined}>
    <summary>{t("servers.archives.configuration.rawJson", undefined, "Original configuration JSON")}</summary>
    <textarea className="settings-schema-input settings-schema-textarea configuration-workspace__raw-json"
      readOnly spellCheck={false} value={props.details.settings_json}
      aria-label={t("servers.archives.configuration.rawJson", undefined, "Original configuration JSON")} />
  </details> : null;

  useLayoutEffect(() => {
    if (requestedFocusId && focusConfigurationControl(requestedFocusId)) {
      setRequestedFocusId(null);
    }
  }, [requestedFocusId, selectedSectionId]);

  function commitSettings(reducer: (settings: SettingsObject) => SettingsObject) {
    if (readOnly) return;
    setSettingsJson((currentJson) => {
      const parsed = parseSettingsObject(currentJson, t);
      return parsed.value ? serializeSettingsObject(reducer(parsed.value)) : currentJson;
    });
  }

  function applyPatch(current: SettingsObject, patch: SettingsObject): SettingsObject {
    return moduleDefinition?.applySettingsPatch?.(current, patch, moduleContext)
      ?? mergeConfigurationPatch(current, patch);
  }

  function handleFieldChange(field: Parameters<typeof writeGuidedFieldValue>[1], value: unknown) {
    commitSettings((current) => {
      const written = writeGuidedFieldValue(current, field, value);
      return applyPatch(current, { [field.key]: written[field.key] });
    });
  }

  function handleBatchChange(changes: GuidedFieldChange[]) {
    commitSettings((current) => {
      let written = current;
      const patch: SettingsObject = {};
      for (const change of changes) {
        written = writeGuidedFieldValue(written, change.field, change.value);
        patch[change.field.key] = written[change.field.key];
      }
      return applyPatch(current, patch);
    });
  }

  function navigateValidationField(fieldKey: string) {
    if (!model) return;
    const target = resolveConfigurationFieldNavigation(model, fieldKey, idPrefix, readOnly);
    if (!target) return;
    setRequestedSectionId(target.sectionId);
    setRequestedFocusId(target.inputId);
  }

  const saveFailureMessage = saveStatus.state === "failed"
    ? t("settings.configuration.save.failed", { message: saveStatus.message }, "Save failed: {message}")
    : saveStatus.state === "conflict"
      ? t("settings.configuration.save.conflict", { message: saveStatus.message }, "Save conflict: {message}")
      : null;

  if (loading) {
    return <div className="configuration-workspace__local-state" data-configuration-state="loading" role={readOnly ? undefined : "status"}>
      {readOnly ? <p role="status">{t("settings.configuration.workspace.loading", undefined, "Loading game server configuration...")}</p>
        : t("settings.configuration.workspace.loading", undefined, "Loading game server configuration...")}
      {rawJson}
    </div>;
  }
  if (moduleMismatch) {
    return <div className="configuration-workspace__local-state" data-configuration-state="disabled">
      {t("settings.configuration.workspace.mismatch", undefined, "Configuration is waiting for the selected game module.")}
      {rawJson}
    </div>;
  }
  if (workspaceError) {
    return <ConfigurationLoadError error={workspaceError}
      onRetry={props.moduleDetailsError ? props.onRetryModuleDetails : undefined}
      repair={!readOnly && settingsParseResult.error ? { value: settingsJson, onChange: setSettingsJson } : undefined}>
      {rawJson}
    </ConfigurationLoadError>;
  }
  if (!localizedModuleDetails || !model || !selectedSectionId || !activeNode) {
    return <div className="configuration-workspace__local-state" data-configuration-state="empty">
      {t("settings.configuration.workspace.empty", undefined, "This game has no configurable server parameters.")}
      {rawJson}
    </div>;
  }

  const specializedRenderers = readOnly ? [] : listConfigurationSpecializedRenderers(
    moduleDefinition,
    selectedSectionId
  );
  const WorkspaceToolbar = readOnly ? undefined : moduleDefinition?.workspaceToolbar;
  const renderSpecializedRenderers = (placement: "before-fields" | "after-fields") => (
    settingsParseResult.value ? (
      <>
        {specializedRenderers.map((registration) => (
          (registration.placement ?? "after-fields") === placement ? (
            <div key={registration.id}
              id={registration.fieldKey
                ? `${idPrefix}-${registration.fieldKey.replace(/[^a-z0-9]+/gi, "-").toLowerCase()}-input`
                : undefined}
              className="configuration-workspace__addon" tabIndex={registration.fieldKey ? -1 : undefined}>
              <registration.Renderer sectionId={selectedSectionId}
                fieldKey={registration.fieldKey} details={props.details}
                moduleDetails={localizedModuleDetails} settings={settingsParseResult.value!}
                disabled={editorDisabled}
                onNavigateField={navigateValidationField}
                onPatch={(patch) => commitSettings((current) => applyPatch(current, patch))} />
            </div>
          ) : null
        ))}
      </>
    ) : null
  );

  return (
    <section className={`configuration-workspace${WorkspaceToolbar ? " configuration-workspace--toolbar" : ""}`}>
      {WorkspaceToolbar && settingsParseResult.value ? <WorkspaceToolbar
        sectionId={selectedSectionId} details={props.details} moduleDetails={localizedModuleDetails}
        settings={settingsParseResult.value} schema={schema} disabled={editorDisabled}
        onPatch={(patch) => commitSettings((current) => applyPatch(current, patch))} /> : null}
      <div className="configuration-workspace__body" data-navigation-open={navigationOpen || undefined}
        onKeyDown={(event) => {
          if (event.key === "Escape" && !event.nativeEvent.isComposing && navigationOpen) {
            event.stopPropagation();
            setNavigationOpen(false);
            navigationToggle.current?.focus();
          }
        }}>
        <button type="button" ref={navigationToggle} className="configuration-workspace__navigation-toggle"
          aria-expanded={navigationOpen} aria-controls={`${idPrefix}-navigation`}
          onClick={() => setNavigationOpen((open) => !open)}>
          <ShellIcon name="list" aria-hidden="true" />
          {t("settings.configuration.workspace.sections")}
          <ShellIcon name="chevron-right" aria-hidden="true" />
        </button>
        <aside id={`${idPrefix}-navigation`} className="configuration-workspace__sidebar">
          <ConfigurationSearchNavigation model={model}
            selectedSectionId={selectedSectionId}
            onSelectSection={(sectionId) => {
              setRequestedSectionId(sectionId);
              setNavigationOpen(false);
              if (navigationToggle.current?.getClientRects().length) navigationToggle.current.focus();
            }}
            onSelectField={(fieldKey) => {
              setNavigationOpen(false);
              navigateValidationField(fieldKey);
            }} />
        </aside>
        <main className="configuration-workspace__main" data-configuration-scroll-owner>
          <div className="configuration-workspace__content">
            {showIconNotice ? <ActivityNotice tone="warning" action={configurationIcons.retryAvailable ?
              <button type="button" className="secondary-button" disabled={configurationIcons.loading} onClick={configurationIcons.retry}>
                {t("settings.configuration.icons.retry", undefined, "Reload icons")}
              </button> : undefined}>
              {configurationIcons.missing
                ? t("settings.configuration.icons.missing", undefined,
                  "No local game icons were found. Install or update the Don't Starve Together server, then reload the icons.")
                : t("settings.configuration.icons.unavailable", undefined,
                  "Setting icons are temporarily unavailable. All settings remain usable.")}
            </ActivityNotice> : null}
            {saveFailureMessage ? <ActivityNotice tone="error" action={
              <button type="button" className="secondary-button" onClick={retrySave}>
                {t("settings.configuration.save.retry", undefined, "Retry save")}
              </button>}>
              {saveFailureMessage}
            </ActivityNotice> : null}
            {validationIssues.length > 0 ? <div className="configuration-workspace__validation" role="alert">
              <strong>{t("settings.configuration.workspace.validation", undefined, "Resolve these settings before saving")}</strong>
              <div className="configuration-workspace__validation-list">
                {validationIssues.map((issue, index) => <button type="button"
                  className="configuration-workspace__validation-button"
                  key={`${issue.fieldKey}-${index}`}
                  data-configuration-validation-field={issue.fieldKey}
                  onClick={() => navigateValidationField(issue.fieldKey)}>{issue.message}</button>)}
              </div>
            </div> : null}
            {renderSpecializedRenderers("before-fields")}
            {activeGuidedFields.length > 0 ? (
              <GuidedSettingsForm schema={schema} settings={settingsParseResult.value ?? {}}
                moduleDetails={localizedModuleDetails} selectedSectionId={selectedSectionId}
                disabled={editorDisabled} readOnly={readOnly} idPrefix={idPrefix}
                defaultContext={defaultContext} validationIssues={validationIssues}
                onChange={handleFieldChange} onBatchChange={handleBatchChange} />
            ) : null}

            {activeNode.builtInEditor === "instance-network" ? (
              <InstanceConnectionSettingsPanel details={props.details} moduleDetails={localizedModuleDetails}
                bindAddressCandidates={props.bindAddressCandidates} bindIp={bindIp} ports={ports}
                defaultPorts={readOnly ? [] : defaultPorts} disabled={editorDisabled}
                onBindIpChange={(value) => { if (!readOnly) setBindIp(value); }}
                onPortsChange={(value) => { if (!readOnly) setPorts(value); }}
                onValidationBlockedChange={setNetworkValidationBlocked} />
            ) : null}
            {renderSpecializedRenderers("after-fields")}
            {props.launchPlanError ? <ActivityNotice tone="error">{props.launchPlanError}</ActivityNotice> : null}
            {rawJson}
          </div>
        </main>
      </div>
      {!readOnly ? <ConfigurationSaveStatus status={saveStatus} validationBlocked={validationBlocked} t={t} /> : null}
    </section>
  );
}
