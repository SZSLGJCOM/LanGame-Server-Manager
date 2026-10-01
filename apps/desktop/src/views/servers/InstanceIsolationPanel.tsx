import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { formatInstancePanelError, instancePanelReader } from "../../instance-panel-loader";
import { describeError } from "../../app-state";
import { ShellIcon } from "../../components/ShellIcon";
import { ActivityNotice } from "../../components/ActivityNotice";
import { useI18n } from "../../i18n";
import type { InstanceIsolationReport } from "../../types";
import { ConfigurationHelp } from "../settings/ConfigurationFieldHelp";
import "./instance-isolation.css";

interface InstanceIsolationPanelProps {
  instanceId: string;
  backupPath?: string;
  onOpenLocalPath: (path: string) => void;
  renderProgramMaintenance?: (report: InstanceIsolationReport) => ReactNode;
}

const conflictLabels = {
  configuration: "servers.isolation.conflict.configuration",
  saves: "servers.isolation.conflict.saves",
  runtime: "servers.isolation.conflict.runtime"
} as const;

export function InstanceIsolationPanel({ instanceId, backupPath, onOpenLocalPath, renderProgramMaintenance }: InstanceIsolationPanelProps) {
  const { t } = useI18n();
  const titleId = useId();
  const [report, setReport] = useState<InstanceIsolationReport | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
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
    { label: t("servers.isolation.savesPath"), path: report.saves_path }
  ] : [];
  if (backupPath?.trim()) paths.push({ label: t("servers.files.backupFolder"), path: backupPath });
  const diagnosticState = report?.mode === "damaged" ? "damaged"
    : report && (report.conflicts.length > 0 || report.issues.length > 0) ? "warning" : report?.mode === "shared" ? "shared" : "private";

  return <section className="server-workbench-surface server-maintenance-card instance-isolation-panel"
    aria-labelledby={titleId} aria-busy={loading}>
    <div className="instance-isolation-header">
      <span className="instance-isolation-title">
        <ShellIcon name="shield" className="server-workbench-section-icon" />
        <span id={titleId}>{t("servers.isolation.title")}</span>
      </span>
      {report ? <span className={"instance-isolation-mode is-" + diagnosticState}>
            {t(`servers.isolation.mode.${diagnosticState}`)}
          </span> : null}
      <button type="button" className="secondary-button instance-isolation-refresh" disabled={loading} aria-busy={loading}
        onClick={() => void refresh()}>{t("common.refresh")}</button>
    </div>
    {loading ? <ActivityNotice>{t("servers.isolation.loading")}</ActivityNotice> : null}
    {loadError ? <ActivityNotice tone="error" action={
      <button type="button" className="secondary-button" disabled={loading} onClick={() => void refresh()}>{t("common.retry")}</button>}>
      {t("servers.isolation.readFailed", { message: loadError })}
    </ActivityNotice> : null}
    {paths.length > 0 || report ? <div className="instance-isolation-content">
      {report?.mode === "damaged" && !loadError ? <p className="instance-isolation-error">
        {t("servers.isolation.damagedDescription")}
      </p> : null}
      {paths.length > 0 ? <ul className="instance-isolation-paths">{paths.map(({ label, path }) => <li key={label}>
        <ConfigurationHelp description={path || t("servers.isolation.pathUnavailable")}>{(help) =>
        <button type="button" className="instance-isolation-path" disabled={!path}
          ref={help.anchorRef} {...help.interactionProps} aria-describedby={help.descriptionId}
          onClick={() => { if (path) onOpenLocalPath(path); }}
          aria-label={`${label}: ${path ? t("servers.isolation.openPath", { path }) : t("servers.isolation.pathUnavailable")}`}>
          <span>{label}</span>
          <code>{path || t("servers.isolation.pathUnavailable")}</code>
        </button>}</ConfigurationHelp>
      </li>)}</ul> : null}
      {report && !loadError ? <>
        {report.conflicts.length ? <div className="instance-isolation-conflicts">
          <h4>{t("servers.isolation.conflicts")}</h4>
          <ul>{report.conflicts.map((conflict, index) => <li
            key={conflict.instance_id + ":" + conflict.kind + ":" + index}>
            <strong>{conflict.instance_name}</strong>
            <span>{t(conflictLabels[conflict.kind])}</span>
            <code>{conflict.path}</code>
            <code>{conflict.other_path}</code>
          </li>)}</ul>
        </div> : null}
        {report.issues.length ? <div className="instance-isolation-issues">
          <h4>{t("servers.isolation.issues")}</h4>
          <ul>{report.issues.map((issue, index) => <li key={index}>{issue}</li>)}</ul>
        </div> : null}
      </> : null}
      {report && !loadError ? renderProgramMaintenance?.(report) : null}
    </div> : null}
  </section>;
}
