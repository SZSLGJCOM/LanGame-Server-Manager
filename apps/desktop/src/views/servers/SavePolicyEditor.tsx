import { ActivityNotice } from "../../components/ActivityNotice";
import { useEffect, useId, useMemo, useRef, useState, type FormEvent } from "react";
import type { LocaleCode, TranslateFn } from "../../i18n";
import type { InstanceDetails, ModuleDetails, SaveInstanceSettingsOptions, UpdateInstanceInput } from "../../types";
import { normalizeConfigurationSaveError } from "../settings/configuration-save-error";
import { useInstanceSettingsSaveCoordinator } from "../settings/InstanceSettingsSaveContext";
import { parseGuidedSettingsSchema, parseSettingsObject, serializeSettingsObject,
  readGuidedFieldValue, validateGuidedSettingsObject, writeGuidedFieldValue } from "../settings/guided-settings";
import { resolveSettingsModuleDefinition } from "../settings/module-registry";
import { nativeSavePolicyKeys } from "../settings/save-policy";
import type { GuidedSettingsField, SettingsObject } from "../settings/settings-schema";
import { changedSettingKeys, mergeSettingPatch } from "./mod-settings-patch";
import { NativeSavePolicyFields } from "./NativeSavePolicyFields";

interface SavePolicyEditorProps {
  details: InstanceDetails;
  readOnly?: boolean;
  moduleDetails: ModuleDetails | null;
  locale: LocaleCode;
  t: TranslateFn;
  onSaveSettings?: (input: UpdateInstanceInput, options?: SaveInstanceSettingsOptions) => Promise<InstanceDetails | undefined>;
}

interface PolicyScope {
  instanceId: string;
  saving: boolean;
}

interface PolicyDraft {
  scope: PolicyScope;
  autoBackupOnStop: boolean;
  retention: string;
  savedAutoBackupOnStop: boolean;
  savedRetention: number;
  settings: SettingsObject;
  savedSettings: SettingsObject;
  observedSettingsJson: string;
  observedAutoBackupOnStop: boolean;
  observedRetention: number;
  confirmedSettingsJson: string | null;
  status: "idle" | "saving" | "saved" | "failed";
  error: string | null;
}

function initialDraft(details: InstanceDetails, scope: PolicyScope): PolicyDraft {
  return {
    scope,
    autoBackupOnStop: details.auto_backup_on_stop,
    retention: String(details.backup_retention_count),
    savedAutoBackupOnStop: details.auto_backup_on_stop,
    savedRetention: details.backup_retention_count,
    settings: parseSettingsObject(details.settings_json).value ?? {},
    savedSettings: parseSettingsObject(details.settings_json).value ?? {},
    observedSettingsJson: details.settings_json,
    observedAutoBackupOnStop: details.auto_backup_on_stop,
    observedRetention: details.backup_retention_count,
    confirmedSettingsJson: null,
    status: "idle",
    error: null
  };
}

function retentionValue(value: string): number | null {
  const normalized = value.trim();
  const parsed = Number(normalized);
  // The storage contract uses u32; avoid truncating decimals or overflowing IPC.
  return /^\d+$/.test(normalized) && Number.isInteger(parsed) && parsed >= 1 && parsed <= 4_294_967_295
    ? parsed
    : null;
}

function isDirty(draft: PolicyDraft, keys: readonly string[]): boolean {
  return draft.autoBackupOnStop !== draft.savedAutoBackupOnStop
    || retentionValue(draft.retention) !== draft.savedRetention
    || changedSettingKeys(draft.savedSettings, draft.settings, keys).length > 0;
}

export function SavePolicyEditor(props: SavePolicyEditorProps) {
  const coordinator = useInstanceSettingsSaveCoordinator();
  const moduleId = props.details.summary.module_id;
  const keys = useMemo(() => nativeSavePolicyKeys(moduleId), [moduleId]);
  const moduleReady = props.moduleDetails?.summary.id === moduleId && Boolean(props.moduleDetails?.schema_json);
  const schema = useMemo(() => {
    const parsed = parseGuidedSettingsSchema(moduleReady ? props.moduleDetails : null,
      props.locale, props.t, { surface: "maintenance" });
    return { ...parsed, fields: parsed.fields.filter((field) => keys.includes(field.key)).map((field) => ({ ...field,
      title: props.t(`servers.savePolicy.field.${field.key}`, undefined, field.title)
    })) };
  }, [moduleReady, props.moduleDetails, props.locale, props.t, keys]);
  const scope = useMemo<PolicyScope>(
    () => ({ instanceId: props.details.summary.id, saving: false }),
    [props.details.summary.id]
  );
  const currentScope = useRef<PolicyScope | null>(scope);
  currentScope.current = scope;
  const currentDetails = useRef(props.details);
  currentDetails.current = props.details;
  const [draft, setDraft] = useState(() => initialDraft(props.details, scope));
  const validationId = useId();
  const activeDraft = draft.scope === scope ? draft : initialDraft(props.details, scope);
  const retention = retentionValue(activeDraft.retention);
  const dirtyKeys = changedSettingKeys(activeDraft.savedSettings, activeDraft.settings, keys);
  // A successful write remains authoritative while the panel refresh is delayed or fails.
  const latestSettingsJson = activeDraft.confirmedSettingsJson !== null
    && activeDraft.observedSettingsJson === props.details.settings_json
    ? activeDraft.confirmedSettingsJson : props.details.settings_json;
  const latest = parseSettingsObject(latestSettingsJson, props.t);
  const settings = mergeSettingPatch(latest.value ?? {}, activeDraft.settings, keys);
  const moduleDefinition = resolveSettingsModuleDefinition(moduleId);
  const validationSettings = { ...settings };
  if (!props.readOnly) for (const field of schema.fields) validationSettings[field.key] = readGuidedFieldValue(field, settings);
  const issues = props.readOnly ? [] : [
    ...validateGuidedSettingsObject(schema, settings, undefined, props.t),
    ...schema.fields.flatMap((field) => {
      const message = moduleDefinition?.getFieldValidationMessage?.({ field,
        value: validationSettings[field.key], settings: validationSettings, locale: props.locale, t: props.t });
      return message ? [{ fieldKey: field.key, reason: "module", message }] : [];
    }),
    ...(moduleDefinition?.getSettingsValidationIssues?.(validationSettings, { locale: props.locale, t: props.t })
      ?? []).filter((issue) => keys.includes(issue.fieldKey))
  ];
  const policyError = latest.error ?? schema.parseError
    ?? (!props.readOnly && keys.length > 0 && (!moduleReady || keys.some((key) => !schema.fields.some((field) => field.key === key)))
      ? props.t("servers.savePolicy.schemaUnavailable") : null);
  const valid = !policyError && issues.length === 0 && retention !== null;
  const dirty = isDirty(activeDraft, keys);
  const saving = activeDraft.status === "saving";

  useEffect(() => {
    if (!props.readOnly && !policyError) {
      // Reopening loads persisted policy; pending writes retain their start barrier.
      void coordinator.runOperation(scope.instanceId, async () => undefined, "save-policy");
    }
  }, [coordinator, scope, props.readOnly, Boolean(policyError)]);

  useEffect(() => {
    currentScope.current = scope;
    return () => {
      if (currentScope.current === scope) currentScope.current = null;
    };
  }, [scope]);

  useEffect(() => {
    setDraft((current) => {
      if (current.scope !== scope) return initialDraft(props.details, scope);
      if (scope.saving || (current.confirmedSettingsJson !== null
        && current.observedSettingsJson === props.details.settings_json
        && current.observedAutoBackupOnStop === props.details.auto_backup_on_stop
        && current.observedRetention === props.details.backup_retention_count)) return current;
      const confirmedSettingsJson = current.observedSettingsJson === props.details.settings_json
        ? current.confirmedSettingsJson : null;
      const refreshedDetails = confirmedSettingsJson === null ? props.details
        : { ...props.details, settings_json: confirmedSettingsJson };
      if (isDirty(current, keys)) {
        const latestSettings = parseSettingsObject(refreshedDetails.settings_json).value;
        if (!latestSettings) return current;
        const editedKeys = new Set(changedSettingKeys(current.savedSettings, current.settings, keys));
        const untouchedKeys = keys.filter((key) => !editedKeys.has(key));
        const keepToggle = current.autoBackupOnStop !== current.savedAutoBackupOnStop;
        const keepRetention = retentionValue(current.retention) !== current.savedRetention;
        return { ...current,
          observedSettingsJson: props.details.settings_json,
          confirmedSettingsJson,
          observedAutoBackupOnStop: props.details.auto_backup_on_stop,
          observedRetention: props.details.backup_retention_count,
          settings: mergeSettingPatch(current.settings, latestSettings, untouchedKeys),
          savedSettings: mergeSettingPatch(current.savedSettings, latestSettings, untouchedKeys),
          autoBackupOnStop: keepToggle ? current.autoBackupOnStop : props.details.auto_backup_on_stop,
          savedAutoBackupOnStop: keepToggle ? current.savedAutoBackupOnStop : props.details.auto_backup_on_stop,
          retention: keepRetention ? current.retention : String(props.details.backup_retention_count),
          savedRetention: keepRetention ? current.savedRetention : props.details.backup_retention_count
        };
      }
      if (current.savedAutoBackupOnStop === props.details.auto_backup_on_stop
        && current.savedRetention === props.details.backup_retention_count
        && changedSettingKeys(current.savedSettings, parseSettingsObject(refreshedDetails.settings_json).value ?? {}, keys).length === 0) return current;
      return { ...initialDraft(refreshedDetails, scope),
        observedSettingsJson: props.details.settings_json, confirmedSettingsJson };
    });
  }, [scope, keys, props.details.settings_json, props.details.auto_backup_on_stop, props.details.backup_retention_count]);

  function editNative(field: GuidedSettingsField, value: unknown) {
    if (props.readOnly || scope.saving) return;
    setDraft((current) => {
      if (currentScope.current !== scope) return current;
      const currentDraft = current.scope === scope ? current : initialDraft(props.details, scope);
      return { ...currentDraft, settings: writeGuidedFieldValue(currentDraft.settings, field, value), status: "idle", error: null };
    });
  }

  function edit(next: Partial<Pick<PolicyDraft, "autoBackupOnStop" | "retention">>) {
    if (props.readOnly || scope.saving) return;
    setDraft((current) => {
      if (currentScope.current !== scope) return current;
      const currentDraft = current.scope === scope ? current : initialDraft(props.details, scope);
      return { ...currentDraft, ...next, status: "idle", error: null };
    });
  }

  async function savePolicy(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const onSaveSettings = props.onSaveSettings;
    if (props.readOnly || !onSaveSettings || scope.saving || !dirty || !valid || retention === null || currentScope.current !== scope) return;
    scope.saving = true;
    const observedBeforeSave = props.details;
    setDraft({ ...activeDraft, status: "saving", error: null });
    try {
      await coordinator.runOperation(scope.instanceId, async () => {
        // Only owned, edited fields are merged; a simultaneous edit to the same native setting requires reloading.
        const changedElsewhere = changedSettingKeys(activeDraft.savedSettings, latest.value ?? {}, dirtyKeys);
        const submittedSettings = mergeSettingPatch(latest.value ?? {}, activeDraft.settings, dirtyKeys);
        if (changedSettingKeys(submittedSettings, latest.value ?? {}, changedElsewhere).length > 0) {
          throw new Error(props.t("servers.savePolicy.conflict"));
        }
        const autoBackupOnStop = activeDraft.autoBackupOnStop !== activeDraft.savedAutoBackupOnStop
          || activeDraft.observedAutoBackupOnStop === props.details.auto_backup_on_stop
          ? activeDraft.autoBackupOnStop : props.details.auto_backup_on_stop;
        const savedRetention = retention !== activeDraft.savedRetention
          || activeDraft.observedRetention === props.details.backup_retention_count ? retention : props.details.backup_retention_count;
        const submittedJson = dirtyKeys.length ? serializeSettingsObject(submittedSettings) : latestSettingsJson;
        await onSaveSettings({
          id: scope.instanceId,
          bind_ip: props.details.summary.bind_ip,
          auto_backup_on_stop: autoBackupOnStop,
          backup_retention_count: savedRetention,
          settings_json: submittedJson,
          ports: props.details.ports
        }, { expectedSettingsJson: latestSettingsJson, silent: true, throwOnError: true });
        if (currentScope.current !== scope) return;
        const observedSettingsJson = currentDetails.current.settings_json;
        const observed = parseSettingsObject(observedSettingsJson).value;
        const refreshed = observed !== null && observedSettingsJson !== observedBeforeSave.settings_json
          && changedSettingKeys(submittedSettings, observed, dirtyKeys).length === 0;
        const confirmedSettingsJson = refreshed ? observedSettingsJson : submittedJson;
        const confirmedSettings = refreshed ? observed : submittedSettings;
        const confirmedAutoBackup = currentDetails.current.auto_backup_on_stop !== observedBeforeSave.auto_backup_on_stop
          ? currentDetails.current.auto_backup_on_stop : autoBackupOnStop;
        const confirmedRetention = currentDetails.current.backup_retention_count !== observedBeforeSave.backup_retention_count
          ? currentDetails.current.backup_retention_count : savedRetention;
        setDraft({
          ...activeDraft,
          autoBackupOnStop: confirmedAutoBackup,
          retention: String(confirmedRetention),
          savedAutoBackupOnStop: confirmedAutoBackup,
          savedRetention: confirmedRetention,
          settings: confirmedSettings,
          savedSettings: confirmedSettings,
          observedSettingsJson,
          observedAutoBackupOnStop: currentDetails.current.auto_backup_on_stop,
          observedRetention: currentDetails.current.backup_retention_count,
          confirmedSettingsJson,
          status: "saved",
          error: null
        });
      }, "save-policy");
    } catch (error) {
      if (currentScope.current !== scope) return;
      setDraft({ ...activeDraft, status: "failed", error: normalizeConfigurationSaveError(error).message });
    } finally {
      scope.saving = false;
    }
  }

  return (
    <form className="server-save-policy-editor" onSubmit={(event) => void savePolicy(event)} noValidate>
      {policyError && <ActivityNotice tone="error">{policyError}</ActivityNotice>}
      <div className="server-native-save-policies">
        <NativeSavePolicyFields instanceId={scope.instanceId} moduleId={moduleId} fields={schema.fields} settings={settings}
          issues={props.readOnly ? [] : issues} readOnly={props.readOnly} disabled={saving || Boolean(props.readOnly)} t={props.t} onChange={editNative} />
      </div>
      {schema.fields.length > 0 && <p className="form-note">{props.t(moduleId === "unturned"
        ? "servers.savePolicy.managedEffective" : "servers.savePolicy.nativeEffective")}</p>}
      <fieldset className="server-save-policy-group server-managed-backup-policy" disabled={saving || Boolean(props.readOnly)} aria-label={props.t("servers.savePolicy.stopBackups")}>
      <div className="server-backup-policy-editor">
      <label className="settings-toggle-card server-backup-policy-toggle">
        <span className="settings-toggle-copy">
          <span className="settings-field-title">{props.t("servers.savePolicy.stopBackups")}</span>
        </span>
        <input type="checkbox" checked={activeDraft.autoBackupOnStop} disabled={saving || Boolean(props.readOnly)}
          onChange={(event) => edit({ autoBackupOnStop: event.target.checked })} />
      </label>
      <label className="server-backup-policy-field">
        <span className="detail-label">{props.t("settings.backups.retention")}</span>
        <input className="settings-schema-input" type="number" min={1} max={4_294_967_295} step={1}
          value={activeDraft.retention} disabled={saving || Boolean(props.readOnly)} aria-invalid={!props.readOnly && retention === null}
          aria-describedby={!props.readOnly && retention === null ? validationId : undefined}
          onChange={(event) => edit({ retention: event.target.value })} />
      </label>
      {!props.readOnly && retention === null && <div id={validationId} className="server-backup-policy-validation" role="alert">
        {props.t("servers.backups.retentionInvalid", undefined, "Enter a whole number from 1 to 4294967295.")}
      </div>}
      </div>
      </fieldset>
      <div className="server-backup-policy-footer">
        <ActivityNotice tone={activeDraft.status === "failed" ? "error" : saving ? "info" : "success"}>
          {saving ? props.t("servers.backups.policySaving", undefined, "Saving policy…")
            : activeDraft.status === "saved" ? props.t("servers.backups.policySaved", undefined, "Policy saved")
              : activeDraft.status === "failed" ? props.t("servers.backups.policyError", { message: activeDraft.error ?? "" },
                `Unable to save policy: ${activeDraft.error ?? ""}`) : ""}
        </ActivityNotice>
        {(dirty || activeDraft.status === "failed") && <button className="ghost-button" type="button" disabled={saving || Boolean(props.readOnly)}
          onClick={() => {
            if (scope.saving || props.readOnly || currentScope.current !== scope) return;
            setDraft(initialDraft(props.details, scope));
            if (!policyError) void coordinator.runOperation(scope.instanceId, async () => undefined, "save-policy");
          }}>{props.t("servers.savePolicy.reload")}</button>}
        <button className="secondary-button" type="submit" disabled={props.readOnly || !dirty || saving || !valid}>
          {props.t("servers.backups.policySave", undefined, "Save policy")}
        </button>
      </div>
    </form>
  );
}
