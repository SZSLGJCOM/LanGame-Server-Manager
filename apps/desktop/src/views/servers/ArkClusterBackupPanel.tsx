import { ActivityNotice } from "../../components/ActivityNotice";
import { useEffect, useId, useRef, useState } from "react";
import type { ArkClusterBackupSummary, ArkClusterRestoreResult, CreateArkClusterBackupInput, RestoreArkClusterBackupInput } from "../../ark-cluster-backups";
import type { ArkClusterReport } from "../../ark-clusters";
import { describeError } from "../../app-state";
import { useI18n, selectLocaleText } from "../../i18n";
import "./ark-cluster-backups.css";

interface ArkClusterBackupPanelProps {
  report: ArkClusterReport;
  busy: boolean;
  listBackups: (instanceId: string) => Promise<ArkClusterBackupSummary[]>;
  createBackup: (input: CreateArkClusterBackupInput) => Promise<ArkClusterBackupSummary>;
  restoreBackup: (input: RestoreArkClusterBackupInput) => Promise<ArkClusterRestoreResult>;
  onChanged: () => void | Promise<void>;
  onBusyChange?: (busy: boolean) => void;
}

export function ArkClusterBackupPanel({ report, busy, listBackups, createBackup, restoreBackup, onChanged, onBusyChange }: ArkClusterBackupPanelProps) {
  const { locale, t } = useI18n();
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const titleId = useId();
  const [backups, setBackups] = useState<ArkClusterBackupSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [action, setAction] = useState<"create" | "restore" | null>(null);
  const [exclusive, setExclusive] = useState(false);
  const [selected, setSelected] = useState<ArkClusterBackupSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [warnings, setWarnings] = useState<string[]>([]);
  const generation = useRef(0);
  const operationGeneration = useRef(0);
  const operationPending = useRef(false);
  const key = JSON.stringify([report.instance_id, report.identity]);
  const currentKey = useRef(key);
  currentKey.current = key;
  const identity = report.identity;
  const allStopped = report.members.length > 0 && report.members.every((member) => member.summary.status === "Stopped" && member.summary.active_process_count === 0);
  const safe = !!identity && allStopped && report.issues.length === 0;
  const locked = busy || loading || !!action;
  const canMutate = safe && exclusive && !locked;
  const date = (value: number) => new Date(value).toLocaleString(locale);

  async function refreshList() {
    const request = ++generation.current;
    setLoading(true);
    setError(null);
    try {
      const next = await listBackups(report.instance_id);
      if (request === generation.current && currentKey.current === key) setBackups(next);
    } catch (failure) {
      if (request === generation.current && currentKey.current === key) {
        setBackups([]);
        setError(describeError(failure));
      }
    } finally {
      if (request === generation.current && currentKey.current === key) setLoading(false);
    }
  }

  useEffect(() => {
    setBackups([]);
    setExclusive(false);
    setSelected(null);
    setNotice(null);
    setWarnings([]);
    setAction(null);
    operationPending.current = false;
    void refreshList();
    return () => { generation.current += 1; operationGeneration.current += 1; };
  }, [key, listBackups]);

  useEffect(() => {
    onBusyChange?.(!!action);
  }, [action, onBusyChange]);
  useEffect(() => () => { onBusyChange?.(false); }, [onBusyChange]);

  async function run(kind: "create" | "restore") {
    if (!canMutate || operationPending.current || !identity || (kind === "restore" && !selected)) return;
    operationPending.current = true;
    const request = generation.current;
    const operation = ++operationGeneration.current;
    setAction(kind);
    setError(null);
    setNotice(null);
    setWarnings([]);
    try {
      const input = { instance_id: report.instance_id, expected_identity: identity, exclusive_root_confirmed: exclusive };
      if (kind === "create") {
        const backup = await createBackup(input);
        if (request !== generation.current || currentKey.current !== key) return;
        setNotice(text(`集群快照已保存：${date(backup.created_at_unix_ms)}，${backup.file_count} 个文件。`, `Cluster snapshot saved: ${date(backup.created_at_unix_ms)}, ${backup.file_count} files.`));
      } else if (selected) {
        const restored = await restoreBackup({ ...input, backup_id: selected.backup_id });
        if (request !== generation.current || currentKey.current !== key) return;
        setSelected(null);
        setNotice(text(`已恢复完整集群。恢复前保护快照：${restored.safeguard_backup.backup_id}`, `Cluster restored. Pre-restore safeguard: ${restored.safeguard_backup.backup_id}`));
        setWarnings(restored.cleanup_warnings ?? []);
      }
      await onChanged();
      if (request === generation.current && currentKey.current === key) await refreshList();
    } catch (failure) {
      if (request === generation.current && currentKey.current === key) {
        setError(describeError(failure));
        // A failed restore can leave a transaction that requires explicit group
        // recovery. Refresh its read-only status instead of hiding that entry.
        try { await onChanged(); } catch (refreshFailure) {
          if (request === generation.current && currentKey.current === key) {
            setError(`${describeError(failure)}\n${describeError(refreshFailure)}`);
          }
        }
      }
    } finally {
      if (currentKey.current === key && operation === operationGeneration.current) {
        operationPending.current = false;
        setAction(null);
      }
    }
  }

  return <section className="ark-cluster-backups" aria-labelledby={titleId} aria-busy={loading || !!action}>
    <div className="ark-cluster-heading"><h4 id={titleId}>{text("集群备份与恢复", "Cluster backup and restore")}</h4>
      <button type="button" className="secondary-button" disabled={locked} onClick={() => void refreshList()}>{t("common.refresh")}</button>
    </div>
    <p>{text("完整快照包含所有成员的世界、配置和整个共享传输目录。备份及恢复期间必须停止全部成员；恢复前会自动保存保护快照。", "A complete snapshot includes every member's world, configuration, and the entire shared transfer directory. All members must be stopped. Restore creates a safeguard snapshot first.")}</p>
    {!allStopped ? <p className="ark-cluster-note">{text("请先停止集群全部成员。", "Stop every cluster member first.")}</p> : null}
    {report.issues.length ? <p className="ark-cluster-error">{text("请先解决上方全部集群检查问题。", "Resolve all cluster inspection issues above first.")}</p> : null}
    <label className="ark-cluster-exclusive"><input type="checkbox" checked={exclusive} disabled={locked || !safe} onChange={(event) => setExclusive(event.target.checked)} />
      <span>{text("我已确认此共享目录专用于当前集群，且没有其他程序向其中写入。", "I confirm this shared directory belongs exclusively to this cluster and no other program writes to it.")}
        <code>{report.cluster_directory || "—"}</code></span>
    </label>
    <div><button type="button" className="secondary-button" disabled={!canMutate} onClick={() => void run("create")}>
      {action === "create" ? text("正在备份集群…", "Backing up cluster…") : text("创建集群快照", "Create cluster snapshot")}
    </button></div>
    {error ? <ActivityNotice tone="error">{error}</ActivityNotice> : null}
    {notice ? <ActivityNotice tone="success">{notice}</ActivityNotice> : null}
    {warnings.length ? <ActivityNotice tone="warning">{warnings.join("\n")}</ActivityNotice> : null}
    {loading ? <ActivityNotice>{text("正在读取集群快照…", "Loading cluster snapshots…")}</ActivityNotice> : null}
    {backups.length === 0 ? !loading ? <p className="ark-cluster-note">{text("尚无完整集群快照。单实例备份不包含整个集群。", "No complete cluster snapshots yet. Individual instance backups do not cover the entire cluster.")}</p> : null
        : <ul className="ark-cluster-backup-list">{backups.map((backup) => <li key={backup.backup_id}>
          <div><strong>{date(backup.created_at_unix_ms)}</strong><span>{backup.backup_kind === "pre_restore" ? text("恢复前保护快照", "Pre-restore safeguard") : text("手动快照", "Manual snapshot")}</span>
            <span>{backup.members.length} {text("个成员", "members")} · {backup.file_count} {text("个文件", "files")} · {(backup.total_bytes / 1048576).toFixed(1)} MiB</span></div>
          <button type="button" className="secondary-button" disabled={!canMutate} onClick={() => { setSelected(backup); setError(null); }}>{text("恢复…", "Restore…")}</button>
        </li>)}</ul>}
    {selected ? <div className="ark-cluster-restore-confirm" role="group" aria-label={text("确认恢复集群", "Confirm cluster restore")}>
      <p>{text(`将用 ${date(selected.created_at_unix_ms)} 的快照替换全部成员世界、配置和共享目录。`, `Replace all member worlds, configurations, and the shared directory with the snapshot from ${date(selected.created_at_unix_ms)}.`)}</p>
      <p>{selected.members.map((member) => `${member.instance_name} (${member.map_name})`).join(" · ")}</p>
      <div className="ark-cluster-actions"><button type="button" className="secondary-button" disabled={!!action} onClick={() => setSelected(null)}>{t("common.cancel")}</button>
        <button type="button" className="primary-button" disabled={!canMutate} onClick={() => void run("restore")}>{action === "restore" ? text("正在恢复集群…", "Restoring cluster…") : text("恢复完整集群", "Restore entire cluster")}</button></div>
    </div> : null}
  </section>;
}
