import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { ActivityNotice } from "../../components/ActivityNotice";
import type { LocaleCode, TranslateFn } from "../../i18n";
import type { InstanceDetails, ModuleDetails, SaveInstanceSettingsOptions, UpdateInstanceInput } from "../../types";
import { ScumServerSettingsRenderer } from "../settings/ScumServerSettingsRenderer";
import { useInstanceSettingsSaveCoordinator } from "../settings/InstanceSettingsSaveContext";
import { normalizeConfigurationSaveError } from "../settings/configuration-save-error";
import { parseSettingsObject, serializeSettingsObject } from "../settings/guided-settings";
import { SCUM_NATIVE_SETTINGS, SCUM_WIPE_KEYS, isScumWipeKey, patchScumNativeValue,
  readScumNativeValue, validateScumStructuredSettings } from "../settings/scum-server-settings-inventory";

interface ScumWipeSettingsEditorProps {
  details: InstanceDetails;
  moduleDetails: ModuleDetails | null;
  locale: LocaleCode;
  t: TranslateFn;
  readOnly?: boolean;
  onSaveSettings?: (input: UpdateInstanceInput, options?: SaveInstanceSettingsOptions) => Promise<InstanceDetails | undefined>;
}

type WipeDraft = Record<string, { baseline: unknown; value: boolean }>;
const WIPE_SETTINGS = SCUM_NATIVE_SETTINGS.filter((setting) => isScumWipeKey(setting.key));
const SAVE_OWNER = "scum:wipe-switches";

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function hasWipeSchema(module: ModuleDetails | null): boolean {
  if (module?.summary.id !== "scum" || !module.schema_json) return false;
  try {
    const schema: unknown = JSON.parse(module.schema_json);
    if (!isRecord(schema) || !isRecord(schema.properties)) return false;
    const general = schema.properties.server_general;
    if (!isRecord(general) || !isRecord(general.properties)) return false;
    const properties = general.properties;
    return WIPE_SETTINGS.every((setting) => {
      const field = properties[setting.key];
      return isRecord(field) && field.type === "boolean" && field["x-lsgm-native-key"] === setting.nativeKey;
    });
  } catch {
    return false;
  }
}

export function ScumWipeSettingsEditor(props: ScumWipeSettingsEditorProps) {
  return <ScumWipeEditor key={props.details.summary.id} {...props} />;
}

function ScumWipeEditor(props: ScumWipeSettingsEditorProps) {
  const { details, t } = props;
  const coordinator = useInstanceSettingsSaveCoordinator();
  const [draft, setDraft] = useState<WipeDraft>({});
  const [confirmed, setConfirmed] = useState<{ observed: string; saved: string } | null>(null);
  const [status, setStatus] = useState<"idle" | "saving" | "saved" | "failed">("idle");
  const [error, setError] = useState<string | null>(null);
  const saving = useRef(false);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  const moduleReady = useMemo(() => hasWipeSchema(props.moduleDetails), [props.moduleDetails]);
  // A parent refresh may arrive after persistence confirms this editor's write.
  const latestJson = confirmed?.observed === details.settings_json ? confirmed.saved : details.settings_json;
  const parsed = parseSettingsObject(latestJson, t);
  const latest = parsed.value ?? {};
  const schemaError = parsed.error ?? (!moduleReady || !isRecord(latest.server_general)
    ? t("scum.maintenance.unavailable") : null);
  useEffect(() => {
    if (!props.readOnly && !schemaError) void coordinator.runOperation(details.summary.id, async () => undefined, SAVE_OWNER);
  }, [coordinator, details.summary.id, props.readOnly, Boolean(schemaError)]);
  const changed = WIPE_SETTINGS.filter((setting) => draft[setting.key]
    && draft[setting.key].value !== readScumNativeValue(latest, setting));
  const conflict = changed.some((setting) => draft[setting.key].baseline !== readScumNativeValue(latest, setting));
  const settings = changed.reduce((current, setting) => ({
    ...current, ...patchScumNativeValue(current, setting, draft[setting.key].value)
  }), latest);
  const issues = validateScumStructuredSettings(settings, t)
    .filter((issue) => SCUM_WIPE_KEYS.some((key) => issue.fieldKey === `server_general.${key}`));
  const stopped = details.summary.status === "Stopped" && details.active_run === null
    && details.summary.active_process_count === 0;
  const disabled = Boolean(props.readOnly) || !stopped || status === "saving" || !props.onSaveSettings || Boolean(schemaError);

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (disabled || saving.current || !changed.length || !props.onSaveSettings || issues.length) return;
    saving.current = true;
    setStatus("saving");
    setError(null);
    const onSave = props.onSaveSettings;
    try {
      let persistedJson = "";
      await coordinator.runOperation(details.summary.id, async () => {
        if (conflict) throw new Error(t("scum.maintenance.conflict"));
        const result = await onSave({ id: details.summary.id, settings_json: serializeSettingsObject(settings),
          bind_ip: details.summary.bind_ip, ports: details.ports,
          auto_backup_on_stop: details.auto_backup_on_stop, backup_retention_count: details.backup_retention_count
        }, { expectedSettingsJson: latestJson, silent: true, throwOnError: true });
        const persisted = result && parseSettingsObject(result.settings_json, t).value;
        if (!result || result.summary.id !== details.summary.id || !persisted
          || changed.some((setting) => readScumNativeValue(persisted, setting) !== draft[setting.key].value)) {
          throw new Error(t("scum.maintenance.saveFailed"));
        }
        persistedJson = result.settings_json;
      }, SAVE_OWNER);
      if (!mounted.current) return;
      setConfirmed({ observed: details.settings_json, saved: persistedJson });
      setDraft({});
      setStatus("saved");
    } catch (cause) {
      if (!mounted.current) return;
      setError(normalizeConfigurationSaveError(cause).message);
      setStatus("failed");
    } finally {
      saving.current = false;
    }
  }

  return <section className="server-workbench-surface server-maintenance-card" aria-label={t("scum.maintenance.title")}>
    <form className="server-save-policy-editor" onSubmit={(event) => void save(event)} noValidate>
      <h3 className="server-workbench-section-label">{t("scum.maintenance.title")}</h3>
      <p className="form-note" role="note">{t("scum.maintenance.warning")} {t("scum.maintenance.persistenceNotice")}</p>
      {schemaError && <ActivityNotice tone="error">{schemaError}</ActivityNotice>}
      {props.moduleDetails?.summary.id === "scum" && <ScumServerSettingsRenderer sectionId="maintenance"
        details={details} moduleDetails={props.moduleDetails} settings={settings} disabled={disabled}
        onPatch={(patch) => {
          if (disabled || saving.current) return;
          const values = patch.server_general;
          if (!values || typeof values !== "object" || Array.isArray(values)) return;
          const general = values as Record<string, unknown>;
          setDraft((current) => {
            const next = { ...current };
            for (const setting of WIPE_SETTINGS) {
              const value = general[setting.key];
              if (typeof value !== "boolean" || value === readScumNativeValue(settings, setting)) continue;
              next[setting.key] = { baseline: current[setting.key]?.baseline ?? readScumNativeValue(latest, setting), value };
            }
            return next;
          });
          setStatus("idle"); setError(null);
        }} />}
      <p className="form-note">{t("scum.maintenance.requiresStop")}</p>
      {issues.map((issue) => <ActivityNotice key={issue.fieldKey} tone="error">{issue.message}</ActivityNotice>)}
      <ActivityNotice tone={status === "failed" ? "error" : status === "saving" ? "info" : "success"}>
        {error ?? (status === "saved" ? t("scum.maintenance.saved") : status === "saving" ? t("settings.configuration.save.saving") : "")}
      </ActivityNotice>
      {!props.readOnly && <div className="server-backup-policy-footer">
        {(Object.keys(draft).length > 0 || status === "failed") && <button type="button" className="ghost-button" disabled={status === "saving"}
          onClick={() => {
            if (saving.current) return;
            setDraft({}); setError(null); setStatus("idle");
            if (!schemaError) void coordinator.runOperation(details.summary.id, async () => undefined, SAVE_OWNER);
          }}>{t("servers.savePolicy.reload")}</button>}
        <button type="submit" className="secondary-button" disabled={disabled || !changed.length || issues.length > 0}>
          {t("scum.maintenance.save")}
        </button>
      </div>}
    </form>
  </section>;
}
