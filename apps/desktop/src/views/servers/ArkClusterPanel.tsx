import { ActivityNotice } from "../../components/ActivityNotice";
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { describeError } from "../../app-state";
import { clusterOperationCounts, type ArkClusterAction, type ArkClusterIssue, type ArkClusterOperationResult, type ArkClusterReport, type OperateArkClusterInput } from "../../ark-clusters";
import { useI18n, selectLocaleText } from "../../i18n";
import "./ark-cluster.css";

interface ArkClusterPanelProps {
  instanceId: string;
  readReport: (instanceId: string) => Promise<ArkClusterReport>;
  operate: (input: OperateArkClusterInput) => Promise<ArkClusterOperationResult>;
  onChanged: () => void | Promise<void>;
  renderMaintenance?: (report: ArkClusterReport | null, refresh: () => Promise<void>, busy: boolean, onBusyChange: (busy: boolean) => void) => ReactNode;
}

const issueLabels: Record<string, [string, string]> = {
  id_directory_mismatch: ["相同集群 ID 指向不同目录，传输数据不会互通。", "The same Cluster ID points to another directory; transfer data is not shared."],
  directory_id_conflict: ["此传输目录还配置了其他集群 ID，请核对共享范围。", "This transfer directory is also configured with another Cluster ID. Verify the sharing boundary."],
  directory_edition_conflict: ["ASE 与 ASA 不能共用同一传输目录。", "ASE and ASA must use separate transfer directories."],
  directory_overlap: ["传输目录与其他 ARK 实例的传输目录相互包含，请分离目录后再统一维护。", "Transfer directories overlap with another ARK instance. Separate them before cluster maintenance."],
  unmanaged_cluster_options: ["自定义启动参数中存在集群选项，请移至集群 ID 和共享目录字段后再统一管理。", "Move cluster options from custom launch flags to the managed Cluster ID and shared directory fields."],
  peer_inspection_incomplete: ["此 ARK 实例无法归类，请核对配置后再纳入集群操作。", "This ARK instance could not be classified. Review its configuration before cluster operations."]
};

export function ArkClusterPanel({ instanceId, readReport, operate, onChanged, renderMaintenance }: ArkClusterPanelProps) {
  const { locale, t } = useI18n();
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const titleId = useId();
  const [report, setReport] = useState<ArkClusterReport | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<ArkClusterAction | null>(null);
  const [maintenanceBusy, setMaintenanceBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<ArkClusterOperationResult | null>(null);
  const generation = useRef(0);
  const operationGeneration = useRef(0);
  const operationPending = useRef(false);
  const currentInstance = useRef(instanceId);
  currentInstance.current = instanceId;

  async function refresh() {
    const request = ++generation.current;
    setLoading(true);
    setError(null);
    try {
      const next = await readReport(instanceId);
      if (request === generation.current && currentInstance.current === instanceId) setReport(next);
    } catch (failure) {
      if (request === generation.current && currentInstance.current === instanceId) {
        setError(describeError(failure));
        setReport(null);
      }
    } finally {
      if (request === generation.current && currentInstance.current === instanceId) setLoading(false);
    }
  }

  useEffect(() => {
    setReport(null);
    setResult(null);
    setBusy(null);
    operationPending.current = false;
    setMaintenanceBusy(false);
    void refresh();
    return () => { generation.current += 1; operationGeneration.current += 1; };
  }, [instanceId, readReport]);

  async function run(action: ArkClusterAction) {
    if (!report?.identity || loading || busy || maintenanceBusy || operationPending.current) return;
    operationPending.current = true;
    const request = generation.current;
    const operation = ++operationGeneration.current;
    setBusy(action);
    setError(null);
    setResult(null);
    try {
      const outcome = await operate({ instance_id: instanceId, expected_identity: report.identity, action });
      if (request !== generation.current || currentInstance.current !== instanceId) return;
      setResult(outcome);
      await onChanged();
      if (request === generation.current && currentInstance.current === instanceId) await refresh();
    } catch (failure) {
      if (currentInstance.current === instanceId && request === generation.current) setError(describeError(failure));
    } finally {
      if (operation === operationGeneration.current && currentInstance.current === instanceId) {
        operationPending.current = false;
        setBusy(null);
      }
    }
  }

  const counts = result ? clusterOperationCounts(result) : null;
  const issueMessage = (issue: ArkClusterIssue) => {
    const label = issueLabels[issue.code];
    return label ? text(...label) : issue.message;
  };

  return <section className="server-workbench-surface server-maintenance-card ark-cluster-panel" aria-labelledby={titleId} aria-busy={loading || !!busy || maintenanceBusy}>
    <div className="ark-cluster-heading">
      <h3 id={titleId}>{text("ARK 集群", "ARK cluster")}</h3>
      <button type="button" className="secondary-button" disabled={loading || !!busy || maintenanceBusy} onClick={() => void refresh()}>{t("common.refresh")}</button>
    </div>
    {loading ? <ActivityNotice>{text("正在检查集群成员…", "Inspecting cluster members…")}</ActivityNotice> : null}
    {error ? <ActivityNotice tone="error">{error}</ActivityNotice> : null}
    {report ? <>
      {report.identity ? <>
        <dl className="ark-cluster-identity">
          <div><dt>{text("集群 ID", "Cluster ID")}</dt><dd><code>{report.identity.cluster_id}</code></dd></div>
          <div><dt>{text("传输目录", "Transfer directory")}</dt><dd><code>{report.cluster_directory}</code></dd></div>
        </dl>
        <div className="ark-cluster-table-wrap"><table className="ark-cluster-members">
          <caption>{text(`当前集群的 ${report.members.length} 个成员`, `${report.members.length} members in this cluster`)}</caption>
          <thead><tr><th>{text("实例", "Instance")}</th><th>{text("地图", "Map")}</th><th>{text("状态", "Status")}</th><th>{text("端口", "Ports")}</th></tr></thead>
          <tbody>{report.members.map((member) => <tr key={member.summary.id}>
            <th scope="row">{member.summary.name}</th><td>{member.map_name || "—"}</td>
            <td>{t(`status.instance.${member.summary.status.toLowerCase()}`, undefined, member.summary.status)}</td>
            <td>{member.ports.map((port) => <span key={`${port.name}:${port.protocol}:${port.port}`}>{port.name}: {port.port}/{port.protocol}</span>)}</td>
          </tr>)}</tbody>
        </table></div>
        <div className="ark-cluster-actions">
          <button type="button" className="primary-button" disabled={loading || !!busy || maintenanceBusy || report.start_blocked} onClick={() => void run("start")}>
            {busy === "start" ? text("正在逐个启动…", "Starting members…") : text("启动集群", "Start cluster")}
          </button>
          <button type="button" className="secondary-button" disabled={loading || !!busy || maintenanceBusy} onClick={() => void run("stop")}>
            {busy === "stop" ? text("正在逐个停止…", "Stopping members…") : text("停止集群", "Stop cluster")}
          </button>
        </div>
      </> : <p>{text("为各地图设置相同的集群 ID 和共享传输目录后，可在此统一管理。", "Set the same Cluster ID and shared transfer directory for each map to manage the group here.")}</p>}
      {report.issues.length > 0 ? <ul className="ark-cluster-issues">{report.issues.map((issue, index) => <li key={`${issue.instance_id}:${issue.code}:${index}`} className={issue.severity === "error" ? "ark-cluster-error" : ""}>
        <strong>{issue.instance_name}</strong>：{issueMessage(issue)}{issue.path ? <code>{issue.path}</code> : null}
      </li>)}</ul> : null}
      {report.related_instances.length > 0 ? <p className="ark-cluster-note">{text("上述配置不一致的实例不会被本集群的批量操作包含。", "Instances with different cluster identities are excluded from this group's operations.")}</p> : null}
    </> : null}
    {renderMaintenance?.(report, refresh, !!busy || loading, setMaintenanceBusy)}
    {result && counts ? <div className="ark-cluster-results" role="status">
      <p>{text(`已完成 ${counts.succeeded}，跳过 ${counts.skipped}，失败 ${counts.failed}。`, `${counts.succeeded} completed, ${counts.skipped} skipped, ${counts.failed} failed.`)}</p>
      <ul>{result.members.map((member) => <li key={member.instance_id} className={member.outcome === "failed" ? "ark-cluster-error" : ""}>
        <strong>{member.instance_name}</strong>：{member.outcome === "failed" ? member.message
          : member.outcome === "skipped" ? text("已处于目标状态，跳过", "Already in the requested state; skipped")
            : result.action === "start" ? text("已启动", "Started") : text("已停止", "Stopped")}
      </li>)}</ul>
    </div> : null}
  </section>;
}
