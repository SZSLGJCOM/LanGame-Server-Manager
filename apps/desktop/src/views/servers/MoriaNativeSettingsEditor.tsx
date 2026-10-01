import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { ActivityNotice } from "../../components/ActivityNotice";
import type { LocaleCode, TranslateFn } from "../../i18n";
import { instanceHasRunningProcess } from "../../runtime-action-state";
import type { InstanceDetails, ModuleDetails, SaveInstanceSettingsOptions, UpdateInstanceInput } from "../../types";
import { ConfigurationField } from "../settings/ConfigurationField";
import { buildConfigurationFieldCopy } from "../settings/GuidedSettingsForm";
import { useInstanceSettingsSaveCoordinator } from "../settings/InstanceSettingsSaveContext";
import { normalizeConfigurationSaveError } from "../settings/configuration-save-error";
import { parseGuidedSettingsSchema, parseSettingsObject, readGuidedFieldValue,
  serializeSettingsObject, validateGuidedSettingsObject, writeGuidedFieldValue } from "../settings/guided-settings";

interface MoriaNativeSettingsEditorProps {
  details: InstanceDetails;
  moduleDetails: ModuleDetails | null;
  kind: "permissions" | "world-upgrade";
  locale: LocaleCode;
  t: TranslateFn;
  readOnly?: boolean;
  onSaveSettings?: (input: UpdateInstanceInput, options?: SaveInstanceSettingsOptions) => Promise<InstanceDetails | undefined>;
}

export function MoriaNativeSettingsEditor(props: MoriaNativeSettingsEditorProps) {
  return <MoriaFieldEditor key={`${props.details.summary.id}:${props.kind}`} {...props} />;
}

function MoriaFieldEditor(props: MoriaNativeSettingsEditorProps) {
  const { t, details } = props;
  const coordinator = useInstanceSettingsSaveCoordinator();
  const saveOwner = `returntomoria:${props.kind}`;
  const fieldKey = props.kind === "permissions" ? "permissions_lines" : "upgrade_optional_dlc_array";
  const schema = useMemo(() => {
    const parsed = parseGuidedSettingsSchema(props.moduleDetails?.summary.id === "returntomoria" ? props.moduleDetails : null,
      props.locale, t, { surface: props.kind === "permissions" ? "player_access" : "maintenance" });
    return { ...parsed, fields: parsed.fields.filter((field) => field.key === fieldKey) };
  }, [props.moduleDetails, props.locale, props.kind, fieldKey, t]);
  const field = schema.fields[0];
  const [draft, setDraft] = useState<{ baseline: unknown; value: unknown } | null>(null);
  const [confirmed, setConfirmed] = useState<{ observed: string; saved: string } | null>(null);
  const [status, setStatus] = useState<"idle" | "saving" | "saved" | "failed">("idle");
  const [error, setError] = useState<string | null>(null);
  const saving = useRef(false);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  // Preserve a confirmed write until the parent finishes refreshing instance details.
  const latestJson = confirmed?.observed === details.settings_json ? confirmed.saved : details.settings_json;
  const parsedSettings = parseSettingsObject(latestJson, t);
  const latest = parsedSettings.value ?? {};
  const savedValue = field ? readGuidedFieldValue(field, latest) : undefined;
  const value = draft ? draft.value : savedValue;
  const settings = field && draft ? writeGuidedFieldValue(latest, field, draft.value) : latest;
  const issues = props.readOnly ? [] : validateGuidedSettingsObject(schema, settings, undefined, t);
  const schemaError = parsedSettings.error ?? schema.parseError
    ?? (!field ? t("returntomoria.workspace.schemaUnavailable") : null);
  useEffect(() => {
    if (!props.readOnly && !schemaError) {
      // Reopening acknowledges this editor's settled failure, without forgetting any pending write.
      void coordinator.runOperation(details.summary.id, async () => undefined, saveOwner);
    }
  }, [coordinator, details.summary.id, saveOwner, props.readOnly, Boolean(schemaError)]);
  const dirty = draft !== null && JSON.stringify(draft.value) !== JSON.stringify(savedValue);
  const conflict = dirty && JSON.stringify(draft.baseline) !== JSON.stringify(savedValue);
  const stopped = details.summary.status === "Stopped" && !details.active_run
    && !instanceHasRunningProcess(details.summary, details.active_run);
  const disabled = Boolean(props.readOnly) || !stopped || status === "saving" || !props.onSaveSettings || Boolean(schemaError);

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (disabled || saving.current || !dirty || !field || !props.onSaveSettings || issues.length) return;
    saving.current = true;
    setError(null);
    setStatus("saving");
    const settingsJson = serializeSettingsObject(settings);
    const onSave = props.onSaveSettings;
    let savedJson = settingsJson;
    try {
      await coordinator.runOperation(details.summary.id, async () => {
        if (conflict) throw new Error(t("returntomoria.workspace.conflict"));
        const saved = await onSave({ id: details.summary.id, settings_json: settingsJson,
          bind_ip: details.summary.bind_ip, ports: details.ports,
          auto_backup_on_stop: details.auto_backup_on_stop, backup_retention_count: details.backup_retention_count
        }, { expectedSettingsJson: latestJson, silent: true, throwOnError: true });
        if (!saved || saved.summary.id !== details.summary.id || !parseSettingsObject(saved.settings_json, t).value) {
          throw new Error(t("returntomoria.workspace.saveFailed"));
        }
        savedJson = saved.settings_json;
      }, saveOwner);
      if (!mounted.current) return;
      setConfirmed({ observed: details.settings_json, saved: savedJson });
      setDraft(null);
      setStatus("saved");
    } catch (cause) {
      if (!mounted.current) return;
      setError(normalizeConfigurationSaveError(cause).message);
      setStatus("failed");
    } finally {
      saving.current = false;
    }
  }

  return <section className="server-workbench-surface server-maintenance-card"
    aria-label={t(props.kind === "permissions" ? "returntomoria.workspace.permissionsTitle" : "returntomoria.workspace.worldUpgradeTitle")}>
    <form className="server-save-policy-editor" onSubmit={(event) => void save(event)} noValidate>
      <h3 className="server-workbench-section-label">{t(props.kind === "permissions"
        ? "returntomoria.workspace.permissionsTitle" : "returntomoria.workspace.worldUpgradeTitle")}</h3>
      {schemaError && <ActivityNotice tone="error">{schemaError}</ActivityNotice>}
      {props.kind === "world-upgrade" && <p className="form-note" role="note">{t("returntomoria.workspace.upgradeWarning")}</p>}
      {field && <ConfigurationField field={field} copy={buildConfigurationFieldCopy(t)}
        className="settings-schema-field--full" disabled={disabled} readOnly={props.readOnly}
        idPrefix={`returntomoria-${props.kind}`} settings={settings}
        value={props.readOnly ? latest[fieldKey] : value} t={t}
        validationMessage={issues.find((issue) => issue.fieldKey === fieldKey)?.message}
        onPatch={(patch) => {
          if (disabled || saving.current || !Object.prototype.hasOwnProperty.call(patch, fieldKey)) return;
          setDraft((current) => ({ baseline: current ? current.baseline : savedValue, value: patch[fieldKey] }));
          setError(null);
          setStatus("idle");
        }} />}
      <p className="form-note">{t("returntomoria.workspace.requiresStop")}</p>
      <ActivityNotice tone={status === "failed" ? "error" : status === "saving" ? "info" : "success"}>
        {error ?? (status === "saved" ? t("returntomoria.workspace.saved")
          : status === "saving" ? t("settings.configuration.save.saving") : "")}
      </ActivityNotice>
      {!props.readOnly && <div className="server-backup-policy-footer">
        {(draft || status === "failed") && <button type="button" className="ghost-button" disabled={status === "saving"}
          onClick={() => {
            if (saving.current) return;
            setDraft(null); setError(null); setStatus("idle");
            if (!schemaError) void coordinator.runOperation(details.summary.id, async () => undefined, saveOwner);
          }}>
          {t("servers.savePolicy.reload")}
        </button>}
        <button type="submit" className="secondary-button" disabled={disabled || !dirty || issues.length > 0}>
          {t("returntomoria.workspace.save")}
        </button>
      </div>}
    </form>
  </section>;
}
