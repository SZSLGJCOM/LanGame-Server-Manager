import { ActivityNotice } from "../../components/ActivityNotice";
import { useEffect, useId, useRef, useState } from "react";
import type { TranslateFn } from "../../i18n";
import type { InstanceDetails, SaveInstanceSettingsOptions, UpdateInstanceInput } from "../../types";
import { useConfigurationFieldHelp } from "../settings/ConfigurationFieldHelp";
import { parseSettingsObject, serializeSettingsObject } from "../settings/guided-settings";
import type { SettingsObject } from "../settings/settings-schema";
import { useAutoSaveInstanceSettings } from "../settings/useAutoSaveInstanceSettings";
import { MAINTENANCE_TYPING_DELAY_MS, MaintenanceAutoSaveFooter } from "./MaintenanceAutoSaveFooter";
import { mergeRecoveryPolicy, parseRecoveryDraft, readRecoveryPolicy, recoveryDraft,
  type RecoveryDraft, type RecoveryPolicy } from "./runtime-recovery-model";

interface Props {
  details: InstanceDetails;
  readOnly?: boolean;
  savedCrashRestartLimit?: number | null;
  t: TranslateFn;
  onSaveSettings?(input: UpdateInstanceInput, options?: SaveInstanceSettingsOptions): Promise<InstanceDetails | undefined>;
}
/** Only a touched policy overrides the persisted record; `base` is the persisted policy the edit started from. */
interface RecoveryEdits {
  instanceId: string;
  values: RecoveryDraft | null;
  base: SettingsObject;
  delayMs: number;
}
function noEdits(instanceId: string): RecoveryEdits {
  return { instanceId, values: null, base: {}, delayMs: MAINTENANCE_TYPING_DELAY_MS };
}
const policyKey = (policy: RecoveryPolicy) => JSON.stringify(policy);

function retainedRecoverySource(settings: SettingsObject, savedCrashRestartLimit?: number | null) {
  const native = settings.runtime_restart && typeof settings.runtime_restart === "object" && !Array.isArray(settings.runtime_restart)
    ? settings.runtime_restart as SettingsObject : {};
  return { enabled: native.enabled, maxRestarts: native.max_restarts === undefined ? savedCrashRestartLimit : native.max_restarts,
    backoffMs: native.backoff_ms, onlyNonzeroExit: native.only_nonzero_exit };
}

function retainedBoolean(value: unknown): boolean | undefined {
  if (typeof value === "boolean") return value;
  if (typeof value !== "string") return undefined;
  const text = value.trim().toLowerCase();
  return ["true", "1", "yes", "on"].includes(text) ? true
    : ["false", "0", "no", "off"].includes(text) ? false : undefined;
}

function retainedValueText(value: unknown): string {
  return value === undefined ? "" : typeof value === "string" && value !== "" ? value : JSON.stringify(value) ?? "";
}

function retainedRecoveryValues(settings: SettingsObject, savedCrashRestartLimit?: number | null): RecoveryDraft {
  const source = retainedRecoverySource(settings, savedCrashRestartLimit);
  const backoff = source.backoffMs;
  const numericBackoff = typeof backoff === "number" && Number.isFinite(backoff)
    || typeof backoff === "string" && /^\d+$/.test(backoff.trim());
  return { enabled: retainedBoolean(source.enabled) ?? false, maxRestarts: retainedValueText(source.maxRestarts),
    waitSeconds: numericBackoff ? String(Number(backoff) / 1000) : retainedValueText(backoff),
    onlyNonzeroExit: retainedBoolean(source.onlyNonzeroExit) ?? false };
}

export function RuntimeRecoveryEditor(props: Props) {
  const instanceId = props.details.summary.id;
  const currentInstanceId = useRef(instanceId);
  currentInstanceId.current = instanceId;
  const [edits, setEdits] = useState(() => noEdits(instanceId));
  // Policies this editor handed to auto-save; their refreshed echo rebases the edit instead of looking external.
  const ownPolicies = useRef(new Set<string>());
  const validationId = useId();
  const exitHelp = useConfigurationFieldHelp(useId(), props.t("servers.recovery.exitBehavior"),
    undefined, undefined, "instructions");
  const active = edits.instanceId === instanceId ? edits : noEdits(instanceId);
  const latest = parseSettingsObject(props.details.settings_json, props.t);
  const latestSettings = latest.value ?? {};
  const persistedPolicy = readRecoveryPolicy(latestSettings);
  const editing = !props.readOnly && active.values !== null;
  const values = props.readOnly ? retainedRecoveryValues(latestSettings, props.savedCrashRestartLimit)
    : active.values ?? recoveryDraft(persistedPolicy);
  const parsed = parseRecoveryDraft(values);
  let merged: SettingsObject | null = null;
  let conflict = false;
  // The refreshed echo of this editor's own earlier write is a new base, not an external change.
  const base = ownPolicies.current.has(policyKey(persistedPolicy)) ? latestSettings : active.base;
  if (editing && parsed.value && latest.value) {
    // A policy edited here and also changed elsewhere is never overwritten silently.
    try { merged = mergeRecoveryPolicy(latest.value, base, parsed.value); } catch { conflict = true; }
  }
  const changed = Boolean(merged && parsed.value && policyKey(parsed.value) !== policyKey(persistedPolicy));
  if (changed && parsed.value) ownPolicies.current.add(policyKey(parsed.value));
  // Unchanged policies keep the persisted text, so a refresh never looks like a new draft.
  const settingsJson = changed && merged ? serializeSettingsObject(merged) : props.details.settings_json;
  const valid = !latest.error && !conflict && parsed.value !== null;
  const save = useAutoSaveInstanceSettings({
    enabled: !props.readOnly,
    details: props.details,
    bindIp: props.details.summary.bind_ip,
    autoBackupOnStop: props.details.auto_backup_on_stop,
    backupRetentionCount: String(props.details.backup_retention_count),
    settingsJson,
    ports: props.details.ports,
    ready: !latest.error,
    disabled: !valid,
    delayMs: active.delayMs,
    adoptExternalWrites: true,
    onSave: props.onSaveSettings
  });
  const retained = props.readOnly ? retainedRecoverySource(latestSettings, props.savedCrashRestartLimit) : null;
  const enabledIsRaw = Boolean(retained && retainedBoolean(retained.enabled) === undefined);
  const exitIsRaw = Boolean(retained && retainedBoolean(retained.onlyNonzeroExit) === undefined);

  useEffect(() => {
    // A persisted policy that caught up with the edit ends it; the echo of an earlier own write becomes its base.
    setEdits((current) => {
      if (current.instanceId !== instanceId) {
        ownPolicies.current.clear();
        return noEdits(instanceId);
      }
      const persisted = parseSettingsObject(props.details.settings_json).value;
      if (!current.values || !persisted) return current;
      const persistedKey = policyKey(readRecoveryPolicy(persisted));
      const draft = parseRecoveryDraft(current.values).value;
      if (draft && policyKey(draft) === persistedKey) {
        ownPolicies.current.clear();
        return noEdits(instanceId);
      }
      if (ownPolicies.current.has(persistedKey) && policyKey(readRecoveryPolicy(current.base)) !== persistedKey) {
        return { ...current, base: persisted };
      }
      return current;
    });
  }, [instanceId, props.details.settings_json]);

  function edit(patch: Partial<RecoveryDraft>, delayMs: number) {
    if (props.readOnly) return;
    const owner = instanceId;
    const persisted = latestSettings;
    setEdits((current) => {
      // Events retained from a previous instance's controls must not edit the new instance.
      if (currentInstanceId.current !== owner) return current;
      const scoped = current.instanceId === owner ? current : noEdits(owner);
      const base = scoped.values ? scoped.base : persisted;
      const start = scoped.values ?? recoveryDraft(readRecoveryPolicy(persisted));
      return { instanceId: owner, values: { ...start, ...patch }, base, delayMs };
    });
  }

  const blocked = !props.readOnly && !latest.error && !valid;
  const typing = MAINTENANCE_TYPING_DELAY_MS;
  return <form className="server-save-policy-editor server-runtime-recovery-editor" onSubmit={(event) => event.preventDefault()} noValidate>
    {latest.error && <ActivityNotice tone="error">{latest.error}</ActivityNotice>}
    <label className="settings-toggle-card server-backup-policy-toggle">
      <span className="settings-toggle-copy"><span className="settings-field-title">{props.t("servers.recovery.enabled")}</span>
        {enabledIsRaw ? <span className="form-note">{retained?.enabled === undefined
          ? props.t("servers.archives.configuration.notSaved") : retainedValueText(retained.enabled)}</span> : null}
      </span>
      <input name="enabled" type="checkbox" checked={values.enabled} disabled={Boolean(props.readOnly)}
        hidden={enabledIsRaw}
        onChange={(event) => edit({ enabled: event.target.checked }, 0)} />
    </label>
    <div className="server-recovery-options">
    <p className="form-note">{props.t("servers.recovery.description")}</p>
    <div className="server-backup-policy-editor">
      <label className="server-backup-policy-field"><span className="detail-label">{props.t("servers.recovery.maxRestarts")}</span>
        <input className="settings-schema-input" name="maxRestarts" type={props.readOnly ? "text" : "number"} min={1} max={10} step={1}
          value={values.maxRestarts} placeholder={props.readOnly ? props.t("servers.archives.configuration.notSaved") : undefined}
          readOnly={props.readOnly} disabled={Boolean(props.readOnly)} aria-invalid={!props.readOnly && parsed.error === "count"}
          aria-describedby={!props.readOnly && parsed.error === "count" ? validationId : undefined}
          onChange={(event) => edit({ maxRestarts: event.target.value }, typing)} />
      </label>
      <label className="server-backup-policy-field"><span className="detail-label">{props.t("servers.recovery.waitSeconds")}</span>
        <input className="settings-schema-input" name="waitSeconds" type={props.readOnly ? "text" : "number"} min={0} max={300} step={0.001}
          value={values.waitSeconds} placeholder={props.readOnly ? props.t("servers.archives.configuration.notSaved") : undefined}
          readOnly={props.readOnly} disabled={Boolean(props.readOnly)} aria-invalid={!props.readOnly && parsed.error === "wait"}
          aria-describedby={!props.readOnly && parsed.error === "wait" ? validationId : undefined}
          onChange={(event) => edit({ waitSeconds: event.target.value }, typing)} />
      </label>
    </div>
    {!props.readOnly && parsed.error && <p id={validationId} className="server-backup-policy-validation" role="alert">
      {props.t(parsed.error === "count" ? "servers.recovery.invalidCount" : "servers.recovery.invalidWait")}</p>}
    <label className="settings-toggle-card server-backup-policy-toggle"
      ref={exitHelp.anchorRef} {...exitHelp.interactionProps}>
      {exitHelp.helpNode}
      <span className="settings-toggle-copy"><span className="settings-field-title">{props.t("servers.recovery.onlyNonzero")}</span>
        {exitIsRaw ? <span className="form-note">{retained?.onlyNonzeroExit === undefined
          ? props.t("servers.archives.configuration.notSaved") : retainedValueText(retained.onlyNonzeroExit)}</span> : null}
      </span>
      <input name="onlyNonzeroExit" type="checkbox" checked={values.onlyNonzeroExit} disabled={Boolean(props.readOnly)}
        hidden={exitIsRaw}
        aria-describedby={exitHelp.descriptionId}
        onChange={(event) => edit({ onlyNonzeroExit: event.target.checked }, 0)} />
    </label>
    </div>
    {!props.readOnly && <MaintenanceAutoSaveFooter status={save.status} blocked={blocked}
      conflictMessage={conflict ? props.t("servers.recovery.conflict") : null} edited={editing} t={props.t}
      onRetry={save.retry} onDiscard={() => { ownPolicies.current.clear(); setEdits(noEdits(instanceId)); }} />}
  </form>;
}
