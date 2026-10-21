import { ActivityNotice } from "../../components/ActivityNotice";
import { useEffect, useId, useRef, useState } from "react";
import type { TranslateFn } from "../../i18n";
import type { InstanceDetails, RuntimeResourceLimits, SaveInstanceSettingsOptions, UpdateInstanceInput } from "../../types";
import { instanceHasRunningProcess } from "../../runtime-action-state";
import { mergeResourceLimits, parseResourceDraft, readResourceLimits, resourceDraft, type ResourceDraft } from "../../runtime-resource-policy";
import { parseSettingsObject, serializeSettingsObject } from "../settings/guided-settings";
import { ConfigurationHelp, useConfigurationFieldHelp } from "../settings/ConfigurationFieldHelp";
import { useAutoSaveInstanceSettings } from "../settings/useAutoSaveInstanceSettings";
import { MAINTENANCE_TYPING_DELAY_MS, MaintenanceAutoSaveFooter } from "./MaintenanceAutoSaveFooter";
import { MaintenanceChip } from "./MaintenanceToolbar";
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

interface ResourceEdits { values: ResourceDraft | null; base: Record<string, unknown> }
const limitsKey = (limits: RuntimeResourceLimits) => JSON.stringify(limits);

/** Limits apply only when the server launches, so saving is persistence for the next start. */
function ResourceForm(props: Props) {
  const [edits, setEdits] = useState<ResourceEdits>({ values: null, base: {} });
  // Limits this editor handed to auto-save; their refreshed echo rebases the edit instead of looking external.
  const ownLimits = useRef(new Set<string>());
  const validationId = useId();
  const admissionHelp = useConfigurationFieldHelp(useId(), props.t("servers.resources.admission"),
    undefined, undefined, "instructions");
  let data: ReturnType<typeof read> | null = null;
  let sourceError: string | null = null;
  try { data = read(props.details.settings_json, props.readOnly); } catch (error) { sourceError = String(error); }
  const editing = !props.readOnly && edits.values !== null;
  const values = (editing ? edits.values : null) ?? data?.draft ?? { cpu: "", memory: "", reserve: props.readOnly ? "" : "2048" };
  let invalid: string | null = null;
  let limits: RuntimeResourceLimits | null = null;
  try { limits = parseResourceDraft(values); } catch (error) { invalid = error instanceof Error ? error.message : "memory"; }
  const persistedLimits = data && !props.readOnly ? readResourceLimits(data.settings) : null;
  const base = persistedLimits && ownLimits.current.has(limitsKey(persistedLimits)) ? data!.settings : edits.base;
  let merged: Record<string, unknown> | null = null;
  let conflict = false;
  if (editing && limits && data) {
    // Limits edited here and also changed elsewhere are never overwritten silently.
    try { merged = mergeResourceLimits(data.settings, base, limits); } catch { conflict = true; }
  }
  const changed = Boolean(merged && limits && persistedLimits && limitsKey(limits) !== limitsKey(persistedLimits));
  if (changed && limits) ownLimits.current.add(limitsKey(limits));
  const settingsJson = changed && merged ? serializeSettingsObject(merged) : props.details.settings_json;
  const running = instanceHasRunningProcess(props.details.summary, props.details.active_run)
    || String(props.details.summary.status ?? "").toLowerCase() === "starting";
  const disabled = props.readOnly || running || Boolean(sourceError);
  const valid = !sourceError && !conflict && !invalid;
  const save = useAutoSaveInstanceSettings({
    enabled: !props.readOnly,
    details: props.details,
    bindIp: props.details.summary.bind_ip,
    autoBackupOnStop: props.details.auto_backup_on_stop,
    backupRetentionCount: String(props.details.backup_retention_count),
    settingsJson,
    ports: props.details.ports,
    ready: !sourceError,
    disabled: !valid,
    delayMs: MAINTENANCE_TYPING_DELAY_MS,
    adoptExternalWrites: true,
    onSave: props.onSaveSettings
  });

  useEffect(() => {
    // Persisted limits that caught up with the edit end it, so later refreshes are followed again.
    setEdits((current) => {
      if (!current.values) return current;
      let persisted: RuntimeResourceLimits;
      let draft: RuntimeResourceLimits;
      try {
        persisted = readResourceLimits(read(props.details.settings_json).settings);
        draft = parseResourceDraft(current.values);
      } catch { return current; }
      if (limitsKey(persisted) !== limitsKey(draft)) return current;
      ownLimits.current.clear();
      return { values: null, base: {} };
    });
  }, [props.details.settings_json]);

  function change(patch: Partial<ResourceDraft>) {
    if (disabled || !data) return;
    const persisted = data.settings;
    const start = values;
    setEdits((current) => current.values
      ? { ...current, values: { ...current.values, ...patch } }
      : { values: { ...start, ...patch }, base: persisted });
  }
  const blocked = !props.readOnly && !sourceError && !valid;
  const failed = save.status.state === "failed" || save.status.state === "conflict";
  const effective = data && !props.readOnly ? readResourceLimits(data.settings) : null;
  const savedLimits = data && props.readOnly ? retainedLimits(data.settings) : {};
  const unlimited = props.t("servers.resources.unlimited");
  const summary = data ? props.t("servers.maintenance.resourcesValue", {
    cpu: data.draft.cpu ? `${data.draft.cpu}%` : unlimited, memory: data.draft.memory ? `${data.draft.memory} MiB` : unlimited }) : undefined;
  return <MaintenanceChip icon="cpu" label={props.t("servers.resources.title")} value={summary} className="server-resource-policy"
    help={[props.t(props.details.summary.module_id === "dontstarve" ? "servers.resources.scopeDontStarve" : "servers.resources.scope"),
      props.t("servers.resources.description")].join("\n")}>
    <form className="server-save-policy-editor" onSubmit={(event) => event.preventDefault()} noValidate>
      {running && <p className="form-note" role="note">{props.t("servers.resources.stopFirst")}</p>}
      {editing && (blocked || failed) && effective && <p className="form-note">{props.t("servers.resources.savedValues", {
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
      {sourceError && <ActivityNotice tone="error">{sourceError}</ActivityNotice>}
      {!props.readOnly && !sourceError && <MaintenanceAutoSaveFooter status={save.status} blocked={blocked}
        conflictMessage={conflict ? props.t("servers.resources.conflict") : null} edited={editing} t={props.t}
        onRetry={save.retry} onDiscard={() => { ownLimits.current.clear(); setEdits({ values: null, base: {} }); }} />}
    </form>
  </MaintenanceChip>;
}
