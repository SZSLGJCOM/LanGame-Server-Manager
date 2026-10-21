import { ActivityNotice } from "../../components/ActivityNotice";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import type { LocaleCode, TranslateFn } from "../../i18n";
import type { InstanceDetails, ModuleDetails, SaveInstanceSettingsOptions, UpdateInstanceInput } from "../../types";
import { parseGuidedSettingsSchema, parseSettingsObject, serializeSettingsObject,
  readGuidedFieldValue, validateGuidedSettingsObject, writeGuidedFieldValue } from "../settings/guided-settings";
import { resolveSettingsModuleDefinition } from "../settings/module-registry";
import { nativeSavePolicyKeys } from "../settings/save-policy";
import type { GuidedSettingsField, SettingsObject } from "../settings/settings-schema";
import { useAutoSaveInstanceSettings } from "../settings/useAutoSaveInstanceSettings";
import { changedSettingKeys, mergeSettingPatch } from "./mod-settings-patch";
import { NativeSavePolicyFields } from "./NativeSavePolicyFields";

/** Typed values wait for a pause, like the configuration workspace; discrete choices save at once. */
export const SAVE_POLICY_TYPING_DELAY_MS = 650;

/** "native" edits the game's own autosave fields; "backup" edits LanGame's stop backup and retention. */
export type SavePolicyScope = "native" | "backup";

interface SavePolicyEditorProps {
  /** Omitted edits both groups as one policy; Maintenance renders each scope in its own section. */
  scope?: SavePolicyScope;
  details: InstanceDetails;
  readOnly?: boolean;
  moduleDetails: ModuleDetails | null;
  locale: LocaleCode;
  t: TranslateFn;
  onSaveSettings?: (input: UpdateInstanceInput, options?: SaveInstanceSettingsOptions) => Promise<InstanceDetails | undefined>;
}

/** Only fields the user touched override the persisted record; everything else follows refreshes. */
interface PolicyEdits {
  instanceId: string;
  autoBackupOnStop?: boolean;
  retention?: string;
  /** Edited native values, and the persisted value each edit started from. */
  settings: SettingsObject;
  baseSettings: SettingsObject;
  editedKeys: readonly string[];
  delayMs: number;
}

function noEdits(instanceId: string): PolicyEdits {
  return { instanceId, settings: {}, baseSettings: {}, editedKeys: [], delayMs: SAVE_POLICY_TYPING_DELAY_MS };
}

function retentionValue(value: string): number | null {
  const normalized = value.trim();
  const parsed = Number(normalized);
  // The storage contract uses u32; avoid truncating decimals or overflowing IPC.
  return /^\d+$/.test(normalized) && Number.isInteger(parsed) && parsed >= 1 && parsed <= 4_294_967_295
    ? parsed
    : null;
}

function discreteField(field: GuidedSettingsField, value: unknown): boolean {
  return typeof value === "boolean" || field.control === "checkbox" || field.control === "select";
}

export function SavePolicyEditor(props: SavePolicyEditorProps) {
  const moduleId = props.details.summary.module_id;
  const instanceId = props.details.summary.id;
  const nativeScope = props.scope !== "backup";
  const backupScope = props.scope !== "native";
  // Each scope validates and saves only its own fields; the other scope's values are passed through unchanged.
  const keys = useMemo(() => nativeScope ? nativeSavePolicyKeys(moduleId) : [], [moduleId, nativeScope]);
  const moduleReady = props.moduleDetails?.summary.id === moduleId && Boolean(props.moduleDetails?.schema_json);
  const schema = useMemo(() => {
    const parsed = parseGuidedSettingsSchema(moduleReady ? props.moduleDetails : null,
      props.locale, props.t, { surface: "maintenance" });
    return { ...parsed, fields: parsed.fields.filter((field) => keys.includes(field.key)).map((field) => ({ ...field,
      title: props.t(`servers.savePolicy.field.${field.key}`, undefined, field.title)
    })) };
  }, [moduleReady, props.moduleDetails, props.locale, props.t, keys]);
  const currentInstanceId = useRef(instanceId);
  currentInstanceId.current = instanceId;
  const [edits, setEdits] = useState(() => noEdits(instanceId));
  // Native values this editor handed to auto-save, by key; their refreshed echo is a new base, not an external change.
  const ownValues = useRef(new Map<string, Set<string>>());
  const validationId = useId();
  const active = edits.instanceId === instanceId ? edits : noEdits(instanceId);
  const latest = parseSettingsObject(props.details.settings_json, props.t);
  const latestSettings = latest.value ?? {};
  const settings = mergeSettingPatch(latestSettings, active.settings, active.editedKeys);
  const autoBackupOnStop = active.autoBackupOnStop ?? props.details.auto_backup_on_stop;
  const retentionText = active.retention ?? String(props.details.backup_retention_count);
  const retention = retentionValue(retentionText);
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
  const ownEcho = (key: string) => {
    const persisted = JSON.stringify(latestSettings[key]);
    return persisted !== undefined && ownValues.current.get(key)?.has(persisted) === true;
  };
  // A native value edited here and also changed elsewhere is never overwritten silently.
  const conflict = active.editedKeys.some((key) => !ownEcho(key)
    && changedSettingKeys(active.baseSettings, latestSettings, [key]).length > 0
    && changedSettingKeys(active.settings, latestSettings, [key]).length > 0);
  const valid = !policyError && !conflict && issues.length === 0 && retention !== null;
  const nativeChangedKeys = changedSettingKeys(latestSettings, settings, active.editedKeys);
  const nativeChanged = nativeChangedKeys.length > 0;
  if (valid) for (const key of nativeChangedKeys) {
    const value = JSON.stringify(settings[key]);
    if (value === undefined) continue;
    const owned = ownValues.current.get(key) ?? new Set<string>();
    owned.add(value);
    ownValues.current.set(key, owned);
  }
  // Unchanged settings keep their persisted text, so a refresh never looks like a new draft.
  const settingsJson = nativeChanged ? serializeSettingsObject(settings) : props.details.settings_json;
  const save = useAutoSaveInstanceSettings({
    enabled: !props.readOnly,
    details: props.details,
    bindIp: props.details.summary.bind_ip,
    autoBackupOnStop,
    backupRetentionCount: retention === null ? String(props.details.backup_retention_count) : String(retention),
    settingsJson,
    ports: props.details.ports,
    ready: !latest.error && !schema.parseError && (keys.length === 0 || moduleReady),
    disabled: !valid,
    delayMs: active.delayMs,
    adoptExternalWrites: true,
    onSave: props.onSaveSettings
  });

  useEffect(() => {
    // Persisted values that caught up with an edit end that edit, so later refreshes are followed again.
    setEdits((current) => {
      if (current.instanceId !== instanceId) {
        ownValues.current.clear();
        return noEdits(instanceId);
      }
      const persisted = parseSettingsObject(props.details.settings_json).value;
      const editedKeys = persisted
        ? current.editedKeys.filter((key) => changedSettingKeys(current.settings, persisted, [key]).length > 0)
        : current.editedKeys;
      for (const key of current.editedKeys) if (!editedKeys.includes(key)) ownValues.current.delete(key);
      const keepToggle = current.autoBackupOnStop !== undefined && current.autoBackupOnStop !== props.details.auto_backup_on_stop;
      const keepRetention = current.retention !== undefined && retentionValue(current.retention) !== props.details.backup_retention_count;
      if (editedKeys.length === current.editedKeys.length && keepToggle === (current.autoBackupOnStop !== undefined)
        && keepRetention === (current.retention !== undefined)) return current;
      return { ...current, editedKeys,
        autoBackupOnStop: keepToggle ? current.autoBackupOnStop : undefined,
        retention: keepRetention ? current.retention : undefined };
    });
  }, [instanceId, props.details.settings_json, props.details.auto_backup_on_stop, props.details.backup_retention_count]);

  function update(change: (current: PolicyEdits) => PolicyEdits) {
    if (props.readOnly) return;
    const owner = instanceId;
    setEdits((current) => {
      // Events retained from a previous instance's controls must not edit the new instance.
      if (currentInstanceId.current !== owner) return current;
      return change(current.instanceId === owner ? current : noEdits(owner));
    });
  }

  function editNative(field: GuidedSettingsField, value: unknown) {
    update((current) => {
      const edited = current.editedKeys.includes(field.key);
      return { ...current,
        settings: writeGuidedFieldValue(current.settings, field, value),
        baseSettings: edited ? current.baseSettings : mergeSettingPatch(current.baseSettings, latestSettings, [field.key]),
        editedKeys: edited ? current.editedKeys : [...current.editedKeys, field.key],
        delayMs: discreteField(field, value) ? 0 : SAVE_POLICY_TYPING_DELAY_MS };
    });
  }

  const blocked = !props.readOnly && !policyError && !valid;
  const failed = save.status.state === "failed" || save.status.state === "conflict";
  const state = blocked ? "blocked" : save.status.state;
  const tone = state === "saved" ? "success" : failed ? "error" : blocked ? "warning" : "info";
  const statusText = conflict ? props.t("servers.savePolicy.conflict")
    : blocked ? props.t("settings.configuration.save.blocked", undefined, "Fix invalid settings to save")
      : state === "saving" ? props.t("servers.backups.policySaving", undefined, "Saving policy…")
        : state === "dirty" ? props.t("settings.configuration.save.pending", undefined, "Changes waiting to save")
          : failed && "message" in save.status ? props.t("servers.backups.policyError", { message: save.status.message },
            `Unable to save policy: ${save.status.message}`)
            : props.t("servers.backups.policySaved", undefined, "Policy saved");
  const edited = active.editedKeys.length > 0 || active.autoBackupOnStop !== undefined || active.retention !== undefined;

  return (
    <form className="server-save-policy-editor" onSubmit={(event) => event.preventDefault()} noValidate>
      {policyError && <ActivityNotice tone="error">{policyError}</ActivityNotice>}
      {nativeScope ? <div className="server-native-save-policies">
        <NativeSavePolicyFields instanceId={instanceId} moduleId={moduleId} fields={schema.fields} settings={settings}
          issues={props.readOnly ? [] : issues} readOnly={props.readOnly} disabled={Boolean(props.readOnly)} t={props.t} onChange={editNative} />
      </div> : null}
      {backupScope ? <fieldset className="server-save-policy-group server-managed-backup-policy" disabled={Boolean(props.readOnly)} aria-label={props.t("servers.savePolicy.stopBackups")}>
      <legend>{props.t("servers.maintenance.backupPolicy")}</legend>
      <div className="server-backup-policy-editor">
      <label className="settings-toggle-card server-backup-policy-toggle">
        <span className="settings-toggle-copy">
          <span className="settings-field-title">{props.t("servers.savePolicy.stopBackups")}</span>
        </span>
        <input type="checkbox" checked={autoBackupOnStop} disabled={Boolean(props.readOnly)}
          onChange={(event) => {
            const checked = event.target.checked;
            update((current) => ({ ...current, autoBackupOnStop: checked, delayMs: 0 }));
          }} />
      </label>
      <label className="server-backup-policy-field">
        <span className="detail-label">{props.t("settings.backups.retention")}</span>
        <input className="settings-schema-input" type="number" min={1} max={4_294_967_295} step={1}
          value={retentionText} disabled={Boolean(props.readOnly)} aria-invalid={!props.readOnly && retention === null}
          aria-describedby={!props.readOnly && retention === null ? validationId : undefined}
          onChange={(event) => {
            const value = event.target.value;
            update((current) => ({ ...current, retention: value, delayMs: SAVE_POLICY_TYPING_DELAY_MS }));
          }} />
      </label>
      {!props.readOnly && retention === null && <div id={validationId} className="server-backup-policy-validation" role="alert">
        {props.t("servers.backups.retentionInvalid", undefined, "Enter a whole number from 1 to 4294967295.")}
      </div>}
      </div>
      </fieldset> : null}
      {!props.readOnly && <div className="server-backup-policy-footer" data-save-state={state}>
        <ActivityNotice tone={tone}>{statusText}</ActivityNotice>
        {failed && !blocked && <button className="secondary-button" type="button" onClick={save.retry}>
          {props.t("settings.configuration.save.retry", undefined, "Retry save")}
        </button>}
        {edited && (failed || blocked) && <button className="ghost-button" type="button"
          onClick={() => { ownValues.current.clear(); update(() => noEdits(instanceId)); }}>{props.t("servers.savePolicy.reload")}</button>}
      </div>}
    </form>
  );
}
