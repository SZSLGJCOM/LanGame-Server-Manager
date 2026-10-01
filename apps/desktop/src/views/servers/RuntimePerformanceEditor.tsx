import { ActivityNotice } from "../../components/ActivityNotice";
import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import type { TranslateFn } from "../../i18n";
import type { InstanceDetails, RuntimeResourceLimits, SaveInstanceSettingsOptions, UpdateInstanceInput } from "../../types";
import { instanceHasRunningProcess } from "../../runtime-action-state";
import { mergeResourceLimits, parseResourceDraft, readResourceLimits, resourceDraft, type ResourceDraft } from "../../runtime-resource-policy";
import { parseSettingsObject, serializeSettingsObject } from "../settings/guided-settings";
import { normalizeConfigurationSaveError } from "../settings/configuration-save-error";
import { ConfigurationHelp, useConfigurationFieldHelp } from "../settings/ConfigurationFieldHelp";
import "./runtime-resources.css";

interface Props {
  details: InstanceDetails;
  readOnly?: boolean;
  t: TranslateFn;
  appliedLimits?: RuntimeResourceLimits | null;
  onSaveSettings?(input: UpdateInstanceInput, options?: SaveInstanceSettingsOptions): Promise<InstanceDetails | undefined>;
}
function retainedLimits(settings: Record<string, unknown>): Record<string, unknown> {
  const performance = settings.runtime_performance;
  const limits = performance && typeof performance === "object" && !Array.isArray(performance)
    ? (performance as Record<string, unknown>).resource_limits : null;
  return limits && typeof limits === "object" && !Array.isArray(limits) ? limits as Record<string, unknown> : {};
}
function read(settingsJson: string, readOnly = false) {
  const parsed = parseSettingsObject(settingsJson);
  if (!parsed.value) throw new Error(parsed.error ?? "Invalid settings JSON");
  if (readOnly) {
    const saved = retainedLimits(parsed.value);
    const value = (key: string) => saved[key] == null ? "" : String(saved[key]);
    return { settings: parsed.value, draft: { cpu: value("cpu_percent"), memory: value("memory_limit_mib"), reserve: value("host_memory_reserve_mib") } };
  }
  return { settings: parsed.value, draft: resourceDraft(readResourceLimits(parsed.value)) };
}

export function RuntimePerformanceEditor(props: Props) {
  return <ResourceForm key={props.details.summary.id} {...props} />;
}

function ResourceForm(props: Props) {
  const mounted = useRef(true);
  const latestDetails = useRef(props.details);
  latestDetails.current = props.details;
  const [edit, setEdit] = useState<{ sourceJson: string; values: ResourceDraft } | null>(null);
  const [confirmed, setConfirmed] = useState<{ observed: string; value: string } | null>(null);
  const [status, setStatus] = useState<"idle" | "saving" | "saved" | "failed">("idle");
  const [saveError, setSaveError] = useState<string | null>(null);
  const savingRef = useRef(false);
  const validationId = useId();
  const admissionHelp = useConfigurationFieldHelp(useId(), props.t("servers.resources.admission"),
    undefined, undefined, "instructions");
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  const settingsJson = confirmed?.observed === props.details.settings_json ? confirmed.value : props.details.settings_json;
  let data: ReturnType<typeof read> | null = null;
  let sourceError: string | null = null;
  try { data = read(settingsJson, props.readOnly); } catch (error) { sourceError = String(error); }
  const values = edit?.values ?? data?.draft ?? { cpu: "", memory: "", reserve: props.readOnly ? "" : "2048" };
  let invalid: string | null = null;
  try { parseResourceDraft(values); } catch (error) { invalid = error instanceof Error ? error.message : "memory"; }
  const dirty = Boolean(edit) && JSON.stringify(values) !== JSON.stringify(data?.draft);
  const running = instanceHasRunningProcess(props.details.summary, props.details.active_run)
    || String(props.details.summary.status ?? "").toLowerCase() === "starting";
  const saving = status === "saving";
  const disabled = props.readOnly || running || saving || Boolean(sourceError);

  function change(patch: Partial<ResourceDraft>) {
    if (disabled || savingRef.current) return;
    setEdit({ sourceJson: edit?.sourceJson ?? settingsJson, values: { ...values, ...patch } });
    setStatus("idle"); setSaveError(null);
  }
  async function save(event: FormEvent) {
    event.preventDefault();
    if (disabled || !props.onSaveSettings || savingRef.current || !dirty || !edit || !data || invalid) return;
    savingRef.current = true; setStatus("saving"); setSaveError(null);
    try {
      const submitted = mergeResourceLimits(data.settings, read(edit.sourceJson).settings, parseResourceDraft(values));
      const submittedJson = serializeSettingsObject(submitted);
      const result = await props.onSaveSettings({ id: props.details.summary.id,
        bind_ip: props.details.summary.bind_ip, ports: props.details.ports,
        auto_backup_on_stop: props.details.auto_backup_on_stop,
        backup_retention_count: props.details.backup_retention_count, settings_json: submittedJson },
      { expectedSettingsJson: settingsJson, silent: true, throwOnError: true });
      if (!mounted.current) return;
      const observed = latestDetails.current.settings_json;
      let confirmedJson = result?.settings_json ?? submittedJson;
      try {
        if (observed !== settingsJson && JSON.stringify(readResourceLimits(read(observed).settings))
          === JSON.stringify(parseResourceDraft(values))) confirmedJson = observed;
      } catch { /* A malformed refresh must not overwrite the verified save result. */ }
      setConfirmed({ observed, value: confirmedJson });
      setEdit(null); setStatus("saved");
    } catch (error) {
      if (!mounted.current) return;
      setSaveError(error instanceof Error && error.message === "runtime_resources_conflict"
        ? props.t("servers.resources.conflict") : normalizeConfigurationSaveError(error).message);
      setStatus("failed");
    } finally { savingRef.current = false; }
  }
  const effective = data && !props.readOnly ? readResourceLimits(data.settings) : null;
  const savedLimits = data && props.readOnly ? retainedLimits(data.settings) : {};
  return <section className="server-resource-policy server-maintenance-card" aria-label={props.t("servers.resources.title")}>
    <div className="server-resource-heading">
      <h3 className="server-workbench-section-label">{props.t("servers.resources.title")}</h3>
      {running && <p className="form-note">{props.t("servers.resources.stopFirst")}</p>}
    </div>
    <form className="server-save-policy-editor" onSubmit={(event) => void save(event)} noValidate>
      <ConfigurationHelp description={props.t("servers.resources.description")}>{(help) =>
        <p className="form-note" ref={help.anchorRef} {...help.interactionProps}
          tabIndex={0} aria-describedby={help.descriptionId}>{props.t(props.details.summary.module_id === "dontstarve"
          ? "servers.resources.scopeDontStarve" : "servers.resources.scope")}</p>}</ConfigurationHelp>
      {dirty && effective && <p className="form-note">{props.t("servers.resources.savedValues", {
        cpu: effective.cpu_percent === null ? props.t("servers.resources.unlimited") : `${effective.cpu_percent}%`,
        memory: effective.memory_limit_mib === null ? props.t("servers.resources.unlimited") : `${effective.memory_limit_mib} MiB`
      })}</p>}
      <div className="server-resource-fields" role="group" aria-label={props.t("servers.resources.title")}
        ref={admissionHelp.anchorRef} {...admissionHelp.interactionProps}
        aria-describedby={admissionHelp.descriptionId} tabIndex={disabled ? 0 : undefined}>
        {admissionHelp.helpNode}
        {([ ["cpu", "servers.resources.cpu", 1, 100], ["memory", "servers.resources.memory", 64, 1048576],
          ["reserve", "servers.resources.reserve", 0, 1048576] ] as const).map(([field, label, min, max]) =>
          <label key={field} className="server-backup-policy-field"><span className="detail-label">{props.t(label)}</span>
            <input className="settings-schema-input" name={field} type="number" min={min} max={max} step={1}
              placeholder={props.readOnly ? props.t(savedLimits[field === "cpu" ? "cpu_percent" : field === "memory" ? "memory_limit_mib" : "host_memory_reserve_mib"] === null
                && field !== "reserve" ? "servers.resources.unlimited" : "servers.archives.configuration.notSaved")
                : field === "reserve" ? undefined : props.t("servers.resources.unlimited")}
              value={values[field]} disabled={disabled} aria-invalid={!props.readOnly && invalid === field}
              aria-describedby={[admissionHelp.descriptionId, !props.readOnly && invalid === field ? validationId : undefined].filter(Boolean).join(" ") || undefined}
              onChange={(event) => change({ [field]: event.target.value })} />
          </label>)}
      </div>
      {running && <ConfigurationHelp description={props.appliedLimits ? undefined : props.t("servers.resources.appliedUnknown")}>
        {(help) => <p className="form-note server-resource-effective" ref={help.anchorRef} {...help.interactionProps}
          tabIndex={help.descriptionId ? 0 : undefined} aria-describedby={help.descriptionId}>{props.appliedLimits
        ? props.t("servers.resources.appliedValues", {
          cpu: props.appliedLimits.cpu_percent === null ? props.t("servers.resources.unlimited") : `${props.appliedLimits.cpu_percent}%`,
          memory: props.appliedLimits.memory_limit_mib === null ? props.t("servers.resources.unlimited") : `${props.appliedLimits.memory_limit_mib} MiB`
        }) : props.t("servers.resources.appliedUnconfirmed")}</p>}</ConfigurationHelp>}
      {!props.readOnly && invalid && <p id={validationId} role="alert" className="form-note form-note--error">{props.t(`servers.resources.invalid.${invalid}`)}</p>}
      {(sourceError || saveError) && <ActivityNotice tone="error">{sourceError ?? saveError ?? ""}</ActivityNotice>}
      {(dirty || status !== "idle") && <div className="server-backup-policy-footer">
        <ActivityNotice tone={saving ? "info" : "success"}>{saving ? props.t("servers.backups.policySaving") : status === "saved" ? props.t("servers.backups.policySaved") : ""}</ActivityNotice>
        {dirty && <button type="button" className="ghost-button" disabled={saving || Boolean(props.readOnly)} onClick={() => { setEdit(null); setStatus("idle"); setSaveError(null); }}>{props.t("servers.savePolicy.reload")}</button>}
        <button type="submit" className="secondary-button" disabled={disabled || !dirty || Boolean(invalid)}>{props.t("servers.backups.policySave")}</button>
      </div>}
    </form>
  </section>;
}
