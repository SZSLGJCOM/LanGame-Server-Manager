import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { ActivityNotice } from "../../components/ActivityNotice";
import type { TranslateFn } from "../../i18n";
import type { InstanceDetails, ModuleDetails, SaveInstanceSettingsOptions, UpdateInstanceInput } from "../../types";
import { useInstanceSettingsSaveCoordinator } from "../settings/InstanceSettingsSaveContext";
import { normalizeConfigurationSaveError } from "../settings/configuration-save-error";
import { parseSettingsObject, serializeSettingsObject } from "../settings/guided-settings";
import { mergeProgramUpdatePolicy, readProgramUpdatePolicy, supportsProgramUpdates, type ProgramUpdatePolicy } from "./program-update-policy";

interface Props {
  details: InstanceDetails;
  moduleDetails: ModuleDetails | null;
  t: TranslateFn;
  readOnly?: boolean;
  onSaveSettings?(input: UpdateInstanceInput, options?: SaveInstanceSettingsOptions): Promise<InstanceDetails | undefined>;
}

const SAVE_OWNER = "program:update-policy";

export function ProgramUpdatePolicyEditor(props: Props) {
  return <PolicyForm key={props.details.summary.id} {...props} />;
}

function PolicyForm({ details, moduleDetails, t, readOnly, onSaveSettings }: Props) {
  const coordinator = useInstanceSettingsSaveCoordinator();
  const mounted = useRef(true);
  const pending = useRef(false);
  const [draft, setDraft] = useState<{ baseline: string; policy: ProgramUpdatePolicy } | null>(null);
  const [confirmed, setConfirmed] = useState<{ observed: string; saved: string } | null>(null);
  const [status, setStatus] = useState<"idle" | "saving" | "saved" | "failed">("idle");
  const [error, setError] = useState<string | null>(null);
  const titleId = useId();
  const helpId = useId();
  const controlId = useId();
  const latestJson = confirmed?.observed === details.settings_json ? confirmed.saved : details.settings_json;
  const parsed = parseSettingsObject(latestJson, t);
  let policy: ProgramUpdatePolicy | null = null;
  let readError = parsed.error;
  if (parsed.value) {
    try { policy = readProgramUpdatePolicy(parsed.value); }
    catch { readError = t("servers.programPolicy.invalid"); }
  }
  const ready = moduleDetails?.summary.id === details.summary.module_id;
  const supported = ready && supportsProgramUpdates(moduleDetails);
  const idle = ["stopped", "error"].includes(details.summary.status.toLowerCase())
    && details.active_run == null && details.summary.active_process_count === 0;
  const saving = status === "saving";
  const disabled = Boolean(readOnly) || !supported || !idle || saving || Boolean(readError) || !onSaveSettings;
  const value = draft?.policy ?? policy ?? "";
  const dirty = Boolean(draft && draft.policy !== policy);

  useEffect(() => {
    mounted.current = true;
    if (!readOnly && !readError) void coordinator.runOperation(details.summary.id, async () => undefined, SAVE_OWNER);
    return () => { mounted.current = false; };
  }, [coordinator, details.summary.id, readOnly]);

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (disabled || pending.current || !dirty || !draft || !parsed.value || !onSaveSettings) return;
    const latestSettings = parsed.value;
    pending.current = true;
    setStatus("saving"); setError(null);
    try {
      let savedJson = "";
      await coordinator.runOperation(details.summary.id, async () => {
        const baseline = parseSettingsObject(draft.baseline, t);
        if (!baseline.value) throw new Error(baseline.error ?? t("servers.programPolicy.invalid"));
        const next = mergeProgramUpdatePolicy(latestSettings, baseline.value, draft.policy);
        const result = await onSaveSettings({ id: details.summary.id, settings_json: serializeSettingsObject(next),
          bind_ip: details.summary.bind_ip, ports: details.ports,
          auto_backup_on_stop: details.auto_backup_on_stop, backup_retention_count: details.backup_retention_count
        }, { expectedSettingsJson: latestJson, silent: true, throwOnError: true });
        const persisted = result && parseSettingsObject(result.settings_json, t).value;
        if (!result || result.summary.id !== details.summary.id || !persisted
          || readProgramUpdatePolicy(persisted) !== draft.policy) throw new Error(t("servers.programPolicy.saveFailed"));
        savedJson = result.settings_json;
      }, SAVE_OWNER);
      if (!mounted.current) return;
      // Only hide the pre-save snapshot; a later refresh remains authoritative.
      setConfirmed({ observed: latestJson, saved: savedJson });
      setDraft(null); setStatus("saved");
    } catch (cause) {
      if (!mounted.current) return;
      const failure = normalizeConfigurationSaveError(cause);
      setError(failure.state === "conflict" || cause instanceof Error && cause.message === "program_update_conflict"
        ? t("servers.programPolicy.conflict") : failure.message);
      setStatus("failed");
    } finally { pending.current = false; }
  }

  return <section className="server-workbench-surface server-maintenance-card" aria-labelledby={titleId}>
    <form className="server-save-policy-editor" onSubmit={(event) => void save(event)} aria-busy={saving}>
      <h3 className="server-workbench-section-label" id={titleId}>{t("servers.programPolicy.title")}</h3>
      {readError && <ActivityNotice tone="error">{readError}</ActivityNotice>}
      {!ready ? <p className="form-note" role="status">{t("servers.programPolicy.loading")}</p>
        : !supported ? <p className="form-note">{t("servers.programPolicy.unsupported")}</p>
          : <>
            <label className="server-backup-policy-field" htmlFor={controlId}>
              <span className="detail-label">{t("servers.programPolicy.label")}</span>
              <select id={controlId} name="programUpdatePolicy" className="settings-schema-input" value={value}
                disabled={disabled} aria-describedby={helpId} aria-invalid={Boolean(readError)}
                onChange={(event) => {
                  if (disabled || pending.current) return;
                  const next = event.target.value;
                  if (next !== "automatic" && next !== "pinned") return;
                  setDraft({ baseline: draft?.baseline ?? latestJson, policy: next });
                  setStatus("idle"); setError(null);
                }}>
                {value === "" && <option value="" disabled>{t("servers.programPolicy.invalid")}</option>}
                <option value="automatic">{t("servers.programPolicy.automatic")}</option>
                <option value="pinned">{t("servers.programPolicy.pinned")}</option>
              </select>
            </label>
            <p className="form-note" id={helpId}>{t(value === "pinned" ? "servers.programPolicy.pinnedHint" : "servers.programPolicy.automaticHint")}</p>
            <p className="form-note">{t("servers.programPolicy.sharedHint")}</p>
            {!idle && !readOnly && <p className="form-note">{t("servers.programPolicy.stopFirst")}</p>}
          </>}
      {error && <ActivityNotice tone="error">{error}</ActivityNotice>}
      {!readOnly && supported && <div className="server-backup-policy-footer">
        <ActivityNotice tone={saving ? "info" : "success"}>
          {saving ? t("servers.backups.policySaving") : status === "saved" ? t("servers.backups.policySaved") : ""}
        </ActivityNotice>
        {(draft || status === "failed") && <button type="button" className="ghost-button" disabled={saving}
          onClick={() => {
            if (pending.current) return;
            setDraft(null); setError(null); setStatus("idle");
            if (!readError) void coordinator.runOperation(details.summary.id, async () => undefined, SAVE_OWNER);
          }}>{t("servers.savePolicy.reload")}</button>}
        <button type="submit" className="secondary-button" disabled={disabled || !dirty}>{t("servers.backups.policySave")}</button>
      </div>}
    </form>
  </section>;
}
