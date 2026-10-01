import { ActivityNotice } from "../../components/ActivityNotice";
import { useEffect, useId, useMemo, useRef, useState, type FormEvent } from "react";
import type { TranslateFn } from "../../i18n";
import type { InstanceDetails, SaveInstanceSettingsOptions, UpdateInstanceInput } from "../../types";
import { normalizeConfigurationSaveError } from "../settings/configuration-save-error";
import { useConfigurationFieldHelp } from "../settings/ConfigurationFieldHelp";
import { parseSettingsObject, serializeSettingsObject } from "../settings/guided-settings";
import type { SettingsObject } from "../settings/settings-schema";
import { mergeRecoveryPolicy, parseRecoveryDraft, readRecoveryPolicy, recoveryDraft,
  recoveryDraftChanged, type RecoveryDraft } from "./runtime-recovery-model";

interface Props {
  details: InstanceDetails;
  readOnly?: boolean;
  savedCrashRestartLimit?: number | null;
  t: TranslateFn;
  onSaveSettings?(input: UpdateInstanceInput, options?: SaveInstanceSettingsOptions): Promise<InstanceDetails | undefined>;
}
interface Scope { instanceId: string; saving: boolean }
interface Draft {
  scope: Scope;
  values: RecoveryDraft;
  baseline: SettingsObject;
  observedJson: string;
  confirmedJson: string | null;
  status: "idle" | "saving" | "saved" | "failed";
  error: string | null;
}
function initial(scope: Scope, settingsJson: string, readOnly = false, savedCrashRestartLimit?: number | null): Draft {
  const baseline = parseSettingsObject(settingsJson).value ?? {};
  const values = readOnly ? retainedRecoveryValues(baseline, savedCrashRestartLimit)
    : recoveryDraft(readRecoveryPolicy(baseline));
  return { scope, baseline, values, observedJson: settingsJson,
    confirmedJson: null, status: "idle", error: null };
}

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
  const scope = useMemo<Scope>(() => ({ instanceId: props.details.summary.id, saving: false }), [props.details.summary.id]);
  const currentScope = useRef<Scope | null>(scope);
  currentScope.current = scope;
  const latestDetails = useRef(props.details);
  latestDetails.current = props.details;
  const [draft, setDraft] = useState<Draft | null>(null);
  const validationId = useId();
  const exitHelp = useConfigurationFieldHelp(useId(), props.t("servers.recovery.exitBehavior"),
    undefined, undefined, "instructions");
  const scoped = draft?.scope === scope ? draft : null;
  const latestJson = scoped?.confirmedJson && scoped.observedJson === props.details.settings_json
    ? scoped.confirmedJson : props.details.settings_json;
  const latest = parseSettingsObject(latestJson, props.t);
  const active = scoped && (scope.saving || recoveryDraftChanged(scoped.values, scoped.baseline)
    || scoped.observedJson === props.details.settings_json) ? scoped : initial(scope, latestJson, props.readOnly, props.savedCrashRestartLimit);
  const parsed = parseRecoveryDraft(active.values);
  const dirty = !props.readOnly && recoveryDraftChanged(active.values, active.baseline);
  const saving = active.status === "saving";
  const error = latest.error ?? active.error;
  const retained = props.readOnly ? retainedRecoverySource(active.baseline, props.savedCrashRestartLimit) : null;
  const enabledIsRaw = Boolean(retained && retainedBoolean(retained.enabled) === undefined);
  const exitIsRaw = Boolean(retained && retainedBoolean(retained.onlyNonzeroExit) === undefined);

  useEffect(() => {
    currentScope.current = scope;
    return () => { if (currentScope.current === scope) currentScope.current = null; };
  }, [scope]);

  function edit(patch: Partial<RecoveryDraft>) {
    if (props.readOnly || scope.saving || currentScope.current !== scope) return;
    setDraft((current) => {
      if (props.readOnly || scope.saving || currentScope.current !== scope) return current;
      const base = current?.scope === scope
        && (recoveryDraftChanged(current.values, current.baseline)
          || current.observedJson === latestDetails.current.settings_json) ? current : active;
      return { ...base, values: { ...base.values, ...patch }, status: "idle", error: null };
    });
  }
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (props.readOnly || !props.onSaveSettings || scope.saving || !dirty || !parsed.value || !latest.value || currentScope.current !== scope) return;
    scope.saving = true;
    setDraft({ ...active, status: "saving", error: null });
    try {
      const submitted = mergeRecoveryPolicy(latest.value, active.baseline, parsed.value);
      const submittedJson = serializeSettingsObject(submitted);
      await props.onSaveSettings({
        id: scope.instanceId, bind_ip: props.details.summary.bind_ip,
        auto_backup_on_stop: props.details.auto_backup_on_stop,
        backup_retention_count: props.details.backup_retention_count,
        settings_json: submittedJson, ports: props.details.ports
      }, { expectedSettingsJson: latestJson, silent: true, throwOnError: true });
      if (currentScope.current !== scope) return;
      const observedJson = latestDetails.current.settings_json;
      const observed = parseSettingsObject(observedJson).value;
      const refreshed = observed !== null && observedJson !== latestJson
        && JSON.stringify(readRecoveryPolicy(observed)) === JSON.stringify(parsed.value);
      setDraft({ ...active, baseline: refreshed ? observed : submitted, values: recoveryDraft(parsed.value),
        observedJson, confirmedJson: refreshed ? observedJson : submittedJson, status: "saved", error: null });
    } catch (error) {
      if (currentScope.current !== scope) return;
      const message = error instanceof Error && error.message === "runtime_recovery_conflict"
        ? props.t("servers.recovery.conflict") : normalizeConfigurationSaveError(error).message;
      setDraft({ ...active, status: "failed", error: message });
    } finally { scope.saving = false; }
  }

  return <form className="server-save-policy-editor server-runtime-recovery-editor" onSubmit={(event) => void save(event)} noValidate>
    {error && <ActivityNotice tone="error">{error}</ActivityNotice>}
    <label className="settings-toggle-card server-backup-policy-toggle">
      <span className="settings-toggle-copy"><span className="settings-field-title">{props.t("servers.recovery.enabled")}</span>
        {enabledIsRaw ? <span className="form-note">{retained?.enabled === undefined
          ? props.t("servers.archives.configuration.notSaved") : retainedValueText(retained.enabled)}</span> : null}
      </span>
      <input name="enabled" type="checkbox" checked={active.values.enabled} disabled={saving || Boolean(props.readOnly)}
        hidden={enabledIsRaw}
        onChange={(event) => edit({ enabled: event.target.checked })} />
    </label>
    <div className="server-recovery-options">
    <p className="form-note">{props.t("servers.recovery.description")}</p>
    <div className="server-backup-policy-editor">
      <label className="server-backup-policy-field"><span className="detail-label">{props.t("servers.recovery.maxRestarts")}</span>
        <input className="settings-schema-input" name="maxRestarts" type={props.readOnly ? "text" : "number"} min={1} max={10} step={1}
          value={active.values.maxRestarts} placeholder={props.readOnly ? props.t("servers.archives.configuration.notSaved") : undefined}
          readOnly={props.readOnly} disabled={saving || Boolean(props.readOnly)} aria-invalid={!props.readOnly && parsed.error === "count"}
          aria-describedby={!props.readOnly && parsed.error === "count" ? validationId : undefined}
          onChange={(event) => edit({ maxRestarts: event.target.value })} />
      </label>
      <label className="server-backup-policy-field"><span className="detail-label">{props.t("servers.recovery.waitSeconds")}</span>
        <input className="settings-schema-input" name="waitSeconds" type={props.readOnly ? "text" : "number"} min={0} max={300} step={0.001}
          value={active.values.waitSeconds} placeholder={props.readOnly ? props.t("servers.archives.configuration.notSaved") : undefined}
          readOnly={props.readOnly} disabled={saving || Boolean(props.readOnly)} aria-invalid={!props.readOnly && parsed.error === "wait"}
          aria-describedby={!props.readOnly && parsed.error === "wait" ? validationId : undefined}
          onChange={(event) => edit({ waitSeconds: event.target.value })} />
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
      <input name="onlyNonzeroExit" type="checkbox" checked={active.values.onlyNonzeroExit} disabled={saving || Boolean(props.readOnly)}
        hidden={exitIsRaw}
        aria-describedby={exitHelp.descriptionId}
        onChange={(event) => edit({ onlyNonzeroExit: event.target.checked })} />
    </label>
    </div>
    <div className="server-backup-policy-footer">
      <ActivityNotice tone={saving ? "info" : "success"}>
        {saving ? props.t("servers.backups.policySaving") : active.status === "saved" ? props.t("servers.backups.policySaved") : ""}
      </ActivityNotice>
      {(dirty || active.status === "failed") && <button type="button" className="ghost-button" disabled={saving || Boolean(props.readOnly)}
        onClick={() => setDraft(initial(scope, latestJson))}>{props.t("servers.savePolicy.reload")}</button>}
      <button type="submit" className="secondary-button" disabled={props.readOnly || saving || !dirty || !parsed.value || !latest.value}>
        {props.t("servers.backups.policySave")}</button>
    </div>
  </form>;
}
