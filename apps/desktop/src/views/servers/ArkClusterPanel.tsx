import { ActivityNotice } from "../../components/ActivityNotice";
import { useEffect, useId, useRef, useState } from "react";
import { useArkClusterActivity } from "./ark-cluster-activity";
import { describeError } from "../../app-state";
import { clusterOperationCounts, displayLocalPath, type ArkClusterAction, type ArkClusterIssue, type ArkClusterOperationResult, type ArkClusterReport, type OperateArkClusterInput } from "../../ark-clusters";
import { useI18n, selectLocaleText } from "../../i18n";
import { ShellIcon } from "../../components/ShellIcon";
import "./ark-cluster.css";

interface ArkClusterPanelProps {
  instanceId: string;
  readReport: (instanceId: string) => Promise<ArkClusterReport>;
  operate: (input: OperateArkClusterInput) => Promise<ArkClusterOperationResult>;
  onChanged: () => void | Promise<void>;
}

const issueLabels: Record<string, [string, string]> = {
  id_directory_mismatch: ["相同集群 ID 指向不同目录，传输数据不会互通。", "The same Cluster ID points to another directory; transfer data is not shared."],
  directory_id_conflict: ["此传输目录还配置了其他集群 ID，请核对共享范围。", "This transfer directory is also configured with another Cluster ID. Verify the sharing boundary."],
  directory_edition_conflict: ["ASE 与 ASA 不能共用同一传输目录。", "ASE and ASA must use separate transfer directories."],
  directory_overlap: ["传输目录与其他 ARK 实例的传输目录相互包含，请分离目录后再统一维护。", "Transfer directories overlap with another ARK instance. Separate them before cluster maintenance."],
  unmanaged_cluster_options: ["自定义启动参数中存在集群选项，请移至集群 ID 和共享目录字段后再统一管理。", "Move cluster options from custom launch flags to the managed Cluster ID and shared directory fields."],
  peer_inspection_incomplete: ["此 ARK 实例无法归类，请核对配置后再纳入集群操作。", "This ARK instance could not be classified. Review its configuration before cluster operations."]
};

/** Read-only path/ID value on the standard field control, with a copy action. */
export function ArkClusterValueField(props: { label: string; value: string; copyLabel: string }) {
  const [copied, setCopied] = useState(false);
  return <div className="settings-schema-field">
    <span className="detail-label">{props.label}</span>
    <div className="ark-cluster-value">
      <input className="settings-schema-input ark-cluster-value__input" readOnly value={props.value} title={props.value}
        aria-label={props.label} onFocus={(event) => event.currentTarget.select()} />
      <button type="button" className="secondary-button" disabled={!props.value} aria-label={`${props.copyLabel} ${props.label}`}
        onClick={() => void navigator.clipboard?.writeText(props.value).then(() => setCopied(true), () => setCopied(false))}>
        <ShellIcon name={copied ? "check" : "copy"} aria-hidden="true" />
        {props.copyLabel}
      </button>
    </div>
  </div>;
}

/** "map-world-<hash>-game" -> group "map-world-<hash>", role "game". */
function groupPorts(ports: { name: string; port: number; protocol: string }[]) {
  const groups = new Map<string, { role: string; port: number; protocol: string; name: string }[]>();
  for (const port of ports) {
    const cut = port.name.lastIndexOf("-");
    const group = cut > 0 ? port.name.slice(0, cut) : "";
    const role = cut > 0 ? port.name.slice(cut + 1) : port.name;
    groups.set(group, [...(groups.get(group) ?? []), { role, port: port.port, protocol: String(port.protocol), name: port.name }]);
  }
  return [...groups.entries()];
}

export function ArkClusterPanel({ instanceId, readReport, operate, onChanged }: ArkClusterPanelProps) {
  const { locale, t } = useI18n();
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const titleId = useId();
  const [report, setReport] = useState<ArkClusterReport | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<ArkClusterAction | null>(null);
  // Snapshot backup/restore/recovery in the 备份 category locks start/stop.
  const snapshotBusy = useArkClusterActivity(instanceId, "operation", !!busy);
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
    void refresh();
    return () => { generation.current += 1; operationGeneration.current += 1; };
  }, [instanceId, readReport]);

  async function run(action: ArkClusterAction) {
    if (!report?.identity || loading || busy || snapshotBusy || operationPending.current) return;
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

  return <section className="server-workbench-surface server-maintenance-card ark-cluster-panel" aria-labelledby={titleId} aria-busy={loading || !!busy || snapshotBusy}>
    <div className="ark-cluster-heading server-workbench-section-label server-maintenance-card-label">
      <ShellIcon name="server" className="server-workbench-section-icon" />
      <h3 id={titleId}>{text("ARK 集群", "ARK cluster")}</h3>
      <button type="button" className="secondary-button" disabled={loading || !!busy || snapshotBusy} onClick={() => void refresh()}>{t("common.refresh")}</button>
    </div>
    {loading ? <ActivityNotice>{text("正在检查集群成员…", "Inspecting cluster members…")}</ActivityNotice> : null}
    {error ? <ActivityNotice tone="error">{error}</ActivityNotice> : null}
    {report ? <>
      {report.identity ? <>
        <div className="settings-schema-grid ark-cluster-identity">
          <ArkClusterValueField label={text("\u96c6\u7fa4 ID", "Cluster ID")} value={report.identity.cluster_id} copyLabel={text("\u590d\u5236", "Copy")} />
          <ArkClusterValueField label={text("\u4f20\u8f93\u76ee\u5f55", "Transfer directory")} value={displayLocalPath(report.cluster_directory)} copyLabel={text("\u590d\u5236", "Copy")} />
        </div>
        <div className="ark-cluster-table-wrap"><table className="ark-cluster-members">
          <caption>{text(`当前集群的 ${report.members.length} 个成员`, `${report.members.length} members in this cluster`)}</caption>
          <thead><tr><th>{text("实例", "Instance")}</th><th>{text("地图", "Map")}</th><th>{text("状态", "Status")}</th><th>{text("端口", "Ports")}</th></tr></thead>
          <tbody>{report.members.map((member) => <tr key={member.summary.id}>
            <th scope="row">{member.summary.name}</th><td>{member.map_name || "—"}</td>
            <td>{t(`status.instance.${member.summary.status.toLowerCase()}`, undefined, member.summary.status)}</td>
            <td><div className="ark-cluster-port-groups">{groupPorts(member.ports).map(([group, ports]) => <div key={group} className="ark-cluster-port-group" title={group || undefined}>
              {ports.map((port) => <span key={`${port.name}:${port.protocol}:${port.port}`} className="ark-cluster-port" title={`${port.name}: ${port.port}/${port.protocol}`}>
                <span className="ark-cluster-port__role">{port.role}</span><code>{port.port}</code><small>{port.protocol.toUpperCase()}</small>
              </span>)}
            </div>)}</div></td>
          </tr>)}</tbody>
        </table></div>
        <div className="ark-cluster-actions">
          <button type="button" className="primary-button" disabled={loading || !!busy || snapshotBusy || report.start_blocked} onClick={() => void run("start")}>
            {busy === "start" ? text("正在逐个启动…", "Starting members…") : text("启动集群", "Start cluster")}
          </button>
          <button type="button" className="secondary-button" disabled={loading || !!busy || snapshotBusy} onClick={() => void run("stop")}>
            {busy === "stop" ? text("正在逐个停止…", "Stopping members…") : text("停止集群", "Stop cluster")}
          </button>
        </div>
      </> : <p>{text("为各地图设置相同的集群 ID 和共享传输目录后，可在此统一管理。", "Set the same Cluster ID and shared transfer directory for each map to manage the group here.")}</p>}
      {report.issues.length > 0 ? <ul className="ark-cluster-issues">{report.issues.map((issue, index) => <li key={`${issue.instance_id}:${issue.code}:${index}`} className={issue.severity === "error" ? "ark-cluster-error" : ""}>
        <strong>{issue.instance_name}</strong>：{issueMessage(issue)}{issue.path ? <code>{displayLocalPath(issue.path)}</code> : null}
      </li>)}</ul> : null}
      {report.related_instances.length > 0 ? <p className="ark-cluster-note">{text("上述配置不一致的实例不会被本集群的批量操作包含。", "Instances with different cluster identities are excluded from this group's operations.")}</p> : null}
    </> : null}
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
