import { useEffect, useMemo, useRef, useState } from "react";
import { ActivityNotice } from "../../components/ActivityNotice";
import type { TranslateFn } from "../../i18n";
import { readDailyMaintenanceSchedule, updateDailyMaintenanceSchedule, validDailyTime,
  type DailyMaintenanceAction, type DailyMaintenanceSchedule } from "../../maintenance-schedule";
import { MaintenanceChip, MaintenanceHelp } from "./MaintenanceToolbar";
import { normalizeConfigurationSaveError } from "../settings/configuration-save-error";

interface Props {
  instanceId: string;
  action: DailyMaintenanceAction;
  readOnly?: boolean;
  /** Render as its own settings chip (the restart schedule); the backup schedule sits inside the backup policy chip. */
  chip?: boolean;
  t: TranslateFn;
}
interface Scope { instanceId: string; action: DailyMaintenanceAction; request: number; saving: boolean }
interface EditorState {
  scope: Scope;
  policy: DailyMaintenanceSchedule | null;
  enabled: boolean;
  time: string;
  dirty: boolean;
  status: "loading" | "idle" | "saving" | "saved" | "failed";
  error: string | null;
}

export function DailyMaintenanceEditor({ instanceId, action, readOnly = false, chip = false, t }: Props) {
  const scope = useMemo<Scope>(() => ({ instanceId, action, request: 0, saving: false }), [instanceId, action, readOnly]);
  const currentScope = useRef<Scope | null>(scope);
  currentScope.current = scope;
  const [state, setState] = useState<EditorState | null>(null);
  const current = state?.scope === scope ? state : null;
  const valid = validDailyTime(current?.time ?? "04:00");
  const disabled = readOnly || !current?.policy || current.status === "loading" || current.status === "saving";

  async function refresh(owner: Scope) {
    if (owner.saving || currentScope.current !== owner) return;
    const request = ++owner.request;
    try {
      const policy = await readDailyMaintenanceSchedule(owner.instanceId, owner.action);
      if (currentScope.current !== owner || owner.request !== request) return;
      setState((previous) => previous?.scope === owner && previous.dirty
        ? { ...previous, policy: previous.policy ? { ...previous.policy, last_run: policy.last_run } : policy }
        : { scope: owner, policy, enabled: policy.enabled, time: policy.time_local, dirty: false, status: "idle", error: null });
    } catch (error) {
      if (currentScope.current !== owner || owner.request !== request) return;
      setState((previous) => ({ ...(previous?.scope === owner ? previous : {
        scope: owner, policy: null, enabled: false, time: "04:00", dirty: false
      }), status: "failed", error: normalizeConfigurationSaveError(error).message }));
    }
  }

  useEffect(() => {
    currentScope.current = scope;
    if (readOnly) return;
    void refresh(scope);
    const timer = setInterval(() => void refresh(scope), 15_000);
    return () => { clearInterval(timer); if (currentScope.current === scope) currentScope.current = null; };
  }, [scope, readOnly]);

  async function save(enabled: boolean, time: string) {
    const owner = scope;
    if (readOnly || owner.saving || currentScope.current !== owner || !current?.policy || !validDailyTime(time)) return;
    if (enabled === current.policy.enabled && time === current.policy.time_local) {
      setState({ ...current, enabled, time, dirty: false, status: "idle", error: null });
      return;
    }
    owner.saving = true;
    owner.request++;
    setState({ ...current, enabled, time, dirty: true, status: "saving", error: null });
    try {
      const policy = await updateDailyMaintenanceSchedule({ instanceId: owner.instanceId, action: owner.action,
        enabled, timeLocal: time, expectedRevision: current.policy.revision });
      if (currentScope.current !== owner) return;
      setState({ scope: owner, policy, enabled: policy.enabled, time: policy.time_local, dirty: false, status: "saved", error: null });
    } catch (error) {
      if (currentScope.current !== owner) return;
      setState({ ...current, enabled, time, dirty: true, status: "failed", error: normalizeConfigurationSaveError(error).message });
    } finally { owner.saving = false; }
  }

  const run = current?.policy?.last_run;
  const editor = <div className="server-daily-maintenance-editor">
    <label className="settings-toggle-card server-backup-policy-toggle">
      <span className="settings-toggle-copy"><MaintenanceHelp className="settings-field-title" help={[t("servers.dailyMaintenance.description"),
        t(action === "restart" ? "servers.dailyMaintenance.restartDescription" : "servers.dailyMaintenance.backupDescription"),
        readOnly ? t("servers.dailyMaintenance.archived") : null].filter(Boolean).join("\n")}>{t(`servers.dailyMaintenance.${action}`)}</MaintenanceHelp></span>
      <input type="checkbox" name={`daily-${action}-enabled`} checked={current?.enabled ?? false} disabled={disabled}
        onChange={(event) => void save(event.target.checked, current?.time ?? "04:00")} />
    </label>
    <div className="server-backup-policy-editor">
      <label className="server-backup-policy-field"><span className="detail-label">{t("servers.dailyMaintenance.time")}</span>
        <input className="settings-schema-input" name={`daily-${action}-time`} type="time" step={60}
          value={current?.time ?? "04:00"} disabled={disabled} aria-invalid={!valid}
          onChange={(event) => { if (current && !disabled) setState({ ...current, time: event.target.value, dirty: true }); }}
          onBlur={() => { if (current?.dirty) void save(current.enabled, current.time); }}
          onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); }} />
      </label>
    </div>
    {!readOnly && !valid ? <ActivityNotice tone="error">{t("servers.dailyMaintenance.invalidTime")}</ActivityNotice> : null}
    {!readOnly && current?.status === "failed" ? <ActivityNotice tone="error" action={<button type="button"
      className="secondary-button" onClick={() => {
        setState((previous) => previous?.scope === scope ? { ...previous, dirty: false } : previous);
        void refresh(scope);
      }}>{t("servers.dailyMaintenance.reload")}</button>}>{current.error ?? ""}</ActivityNotice> : null}
    <ActivityNotice tone={current?.status === "saved" ? "success" : "info"}>{readOnly || current?.status === "idle" || current?.status === "failed" ? ""
      : t(!current || current.status === "loading" ? "common.loading"
        : current.status === "saving" ? "servers.backups.policySaving" : "servers.backups.policySaved")}</ActivityNotice>
    {run ? <ActivityNotice tone={run.status === "failed" || run.status === "interrupted" ? "error"
      : run.status === "succeeded" ? "success" : "info"}>
      {`${run.local_date} · ${t(`servers.dailyMaintenance.status.${run.status}`)}${run.detail ? ` · ${run.detail}` : ""}`}
    </ActivityNotice> : null}
  </div>;
  if (!chip) return editor;
  const policy = current?.policy;
  return <MaintenanceChip icon="clock" label={t(`servers.dailyMaintenance.${action}`)} className="server-daily-maintenance-chip"
    value={policy ? policy.enabled ? `${t("servers.maintenance.on")} · ${policy.time_local}` : t("servers.maintenance.off") : undefined}>
    {editor}
  </MaintenanceChip>;
}
