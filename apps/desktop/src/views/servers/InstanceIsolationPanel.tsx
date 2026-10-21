import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { formatInstancePanelError, instancePanelReader } from "../../instance-panel-loader";
import { describeError } from "../../app-state";
import { ShellIcon } from "../../components/ShellIcon";
import { ActivityNotice } from "../../components/ActivityNotice";
import { useI18n } from "../../i18n";
import type { InstanceIsolationReport } from "../../types";
import { MaintenanceAction, MaintenanceGroup, MaintenanceHelp, MaintenanceToolbar } from "./MaintenanceToolbar";
import { StorageRecordList, StorageRecordRow } from "./StorageRecordList";
import "./instance-isolation.css";

interface InstanceIsolationPanelProps {
  instanceId: string;
  onOpenLocalPath: (path: string) => void;
  /** Card title; the standalone diagnostics view keeps its own. */
  title?: string;
  /** Instance status tag shown beside the title. */
  status?: ReactNode;
  /** Policy group rendered above the directories (the maintenance Runtime section). */
  policies?: ReactNode;
  renderProgramMaintenance?: (report: InstanceIsolationReport) => ReactNode;
}

const conflictLabels = {
  configuration: "servers.isolation.conflict.configuration",
  saves: "servers.isolation.conflict.saves",
  runtime: "servers.isolation.conflict.runtime"
} as const;

export function InstanceIsolationPanel({ instanceId, onOpenLocalPath, title, status, policies, renderProgramMaintenance }: InstanceIsolationPanelProps) {
  const { t } = useI18n();
  const titleId = useId();
  const [report, setReport] = useState<InstanceIsolationReport | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [copied, setCopied] = useState<{ text: string; failed: boolean } | null>(null);
  const generation = useRef(0);
  const mounted = useRef(false);
  const readAbort = useRef(new AbortController());

  async function refresh() {
    const request = ++generation.current;
    setLoading(true);
    setLoadError(null);
    try {
      const next = await instancePanelReader.readIsolation(instanceId, readAbort.current.signal);
      if (!mounted.current || request !== generation.current) return;
      setReport(next);
    } catch (error) {
      if (!mounted.current || request !== generation.current) return;
      setLoadError(formatInstancePanelError(describeError(error), t));
      setReport(null);
    } finally {
      if (mounted.current && request === generation.current) setLoading(false);
    }
  }

  useEffect(() => {
    mounted.current = true;
    readAbort.current = new AbortController();
    setReport(null);
    void refresh();
    return () => {
      mounted.current = false;
      readAbort.current.abort();
      generation.current += 1;
    };
  }, [instanceId]);

  const paths = report ? [
    { label: t("servers.isolation.runtimePath"), path: report.runtime_path },
    { label: t("servers.isolation.dataPath"), path: report.data_path },
    { label: t("servers.isolation.configPath"), path: report.config_path },
    { label: t("servers.isolation.savesPath"), path: report.saves_path },
    { label: t("servers.files.backupFolder"), path: report.backups_path }
  ] : [];
  const diagnosticState = report?.mode === "damaged" ? "damaged"
    : report && (report.conflicts.length > 0 || report.issues.length > 0) ? "warning" : report?.mode === "shared" ? "shared" : "private";

  async function copy(label: string, path: string) {
    try {
      await navigator.clipboard.writeText(path);
      if (mounted.current) setCopied({ text: t("servers.isolation.pathCopied", { label }), failed: false });
    } catch (error) {
      if (mounted.current) setCopied({ text: t("servers.isolation.copyFailed", { message: describeError(error) }), failed: true });
    }
  }

  return <section className="server-workbench-surface server-maintenance-card instance-isolation-panel"
    aria-labelledby={titleId} aria-busy={loading}>
    <div className="server-workbench-section-label server-maintenance-card-label">
      <ShellIcon name={policies ? "server" : "shield"} className="server-workbench-section-icon" />
      <span id={titleId}>{title ?? t("servers.isolation.title")}</span>
      {status}
      {report ? <span className={"instance-isolation-mode is-" + diagnosticState}>
        {t(`servers.isolation.mode.${diagnosticState}`)}</span> : null}
      <MaintenanceToolbar label={title ?? t("servers.isolation.title")}>
        <MaintenanceAction icon="refresh" iconOnly className="ghost-button instance-isolation-refresh" label={t("common.refresh")}
          busy={loading} onClick={() => void refresh()} />
        {report && !loadError ? renderProgramMaintenance?.(report) : null}
      </MaintenanceToolbar>
    </div>
    {loading ? <ActivityNotice>{t("servers.isolation.loading")}</ActivityNotice> : null}
    {loadError ? <ActivityNotice tone="error" action={
      <button type="button" className="secondary-button" disabled={loading} onClick={() => void refresh()}>{t("common.retry")}</button>}>
      {t("servers.isolation.readFailed", { message: loadError })}
    </ActivityNotice> : null}
    {copied ? <ActivityNotice tone={copied.failed ? "error" : "success"} onDismiss={() => setCopied(null)}>{copied.text}</ActivityNotice> : null}
    {policies}
    {report && !loadError ? <MaintenanceGroup className="instance-isolation-content" label={t("servers.maintenance.directoriesGroup")}
      help={t("servers.maintenance.directoriesHelp")}>
      {report.mode === "damaged" ? <p className="form-note instance-isolation-error" role="note">
        <ShellIcon name="alert-triangle" className="maintenance-help-icon" />{t("servers.isolation.damagedDescription")}</p> : null}
      {report.issues.map((issue, index) => <p key={index} className="form-note instance-isolation-issue" role="note">
        <ShellIcon name="alert-triangle" className="maintenance-help-icon" />{issue}</p>)}
      <StorageRecordList label={t("servers.isolation.title")}>
        <div className="instance-isolation-paths">{paths.map(({ label, path }) => <StorageRecordRow key={label} name={label}
          facts={<code>{path || t("servers.isolation.pathUnavailable")}</code>} details={path || t("servers.isolation.pathUnavailable")}
          actions={<>
            <MaintenanceAction icon="folder" iconOnly className="ghost-button instance-isolation-open"
              label={path ? t("servers.isolation.openPath", { path }) : t("servers.isolation.pathUnavailable")} disabled={!path}
              onClick={() => { if (path) onOpenLocalPath(path); }} />
            <MaintenanceAction icon="copy" iconOnly label={t("servers.isolation.copyPath")} disabled={!path}
              onClick={() => { if (path) void copy(label, path); }} />
          </>} />)}</div>
        {report.conflicts.map((conflict, index) => <StorageRecordRow key={conflict.instance_id + ":" + conflict.kind + ":" + index}
          name={conflict.instance_name}
          badge={<MaintenanceHelp className="instance-isolation-conflict" help={`${conflict.path}\n${conflict.other_path}`}>
            {t(conflictLabels[conflict.kind])}</MaintenanceHelp>}
          facts={<code>{conflict.other_path}</code>} />)}
      </StorageRecordList>
    </MaintenanceGroup> : null}
  </section>;
}
