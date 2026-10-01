import { ActivityNotice } from "../../components/ActivityNotice";
import { useEffect, useRef, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { shouldUseLanApi } from "../../api-transport";
import { createArkClusterBackup, listArkClusterBackups, readPendingArkClusterRestore, recoverArkClusterRestore, restoreArkClusterBackup } from "../../api";
import type { PendingArkClusterRestore } from "../../ark-cluster-backups";
import type { ArkClusterReport } from "../../ark-clusters";
import { describeError } from "../../app-state";
import { selectLocaleText, useI18n } from "../../i18n";
import { ArkClusterBackupPanel } from "./ArkClusterBackupPanel";

interface Props {
  instanceId: string;
  report: ArkClusterReport | null;
  busy: boolean;
  onChanged: () => Promise<void>;
  onBusyChange: (busy: boolean) => void;
}

/** Recovery remains reachable when an interrupted directory swap makes the
 * ordinary member report unreadable. Inspecting it never writes game data. */
export function ArkClusterMaintenance(props: Props) {
  const { locale } = useI18n();
  if (!isTauri() && shouldUseLanApi()) {
    return <p className="ark-cluster-note" role="note">{selectLocaleText(locale,
      "集群备份、恢复和中断恢复处理需要在本机 LanGame 桌面端操作。",
      "Cluster backups, restores, and interrupted restore recovery require the local LanGame desktop app.")}</p>;
  }
  return <LocalArkClusterMaintenance {...props} />;
}

function LocalArkClusterMaintenance({ instanceId, report, busy, onChanged, onBusyChange }: Props) {
  const { locale, t } = useI18n();
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const [pending, setPending] = useState<PendingArkClusterRestore | null>(null);
  const [loading, setLoading] = useState(true);
  const [inspected, setInspected] = useState(false);
  const [working, setWorking] = useState(false);
  const [backupBusy, setBackupBusy] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [exclusive, setExclusive] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [inspectionError, setInspectionError] = useState<string | null>(null);
  const requestId = useRef(0);
  const lifecycle = useRef(0);
  const pendingScope = useRef("");
  const lastReport = useRef(report);
  const operation = useRef(false);
  const currentInstance = useRef(instanceId);
  currentInstance.current = instanceId;

  async function inspect() {
    const request = ++requestId.current;
    const owner = lifecycle.current;
    const isCurrent = () => request === requestId.current && owner === lifecycle.current && currentInstance.current === instanceId;
    setLoading(true);
    setInspectionError(null);
    try {
      const value = await readPendingArkClusterRestore(instanceId);
      if (!isCurrent()) return;
      const scope = value ? JSON.stringify([value.identity, value.backup_id, value.safeguard_backup.backup_id]) : "";
      if (scope !== pendingScope.current) { setConfirm(false); setExclusive(false); }
      pendingScope.current = scope;
      setPending(value);
      setInspected(true);
    } catch (failure) {
      if (isCurrent()) setInspectionError(describeError(failure));
    } finally {
      if (isCurrent()) setLoading(false);
    }
  }

  useEffect(() => {
    lifecycle.current += 1;
    pendingScope.current = "";
    operation.current = false;
    setPending(null); setConfirm(false); setExclusive(false); setNotice(null); setWarnings([]);
    setWorking(false); setBackupBusy(false); setInspected(false); setError(null);
    void inspect();
    return () => { requestId.current += 1; lifecycle.current += 1; };
  }, [instanceId]);

  useEffect(() => {
    if (lastReport.current === report) return;
    lastReport.current = report;
    if (!operation.current && !backupBusy) void inspect();
  }, [report]);

  const maintenanceBusy = loading || working || backupBusy || !!pending || !!inspectionError;
  useEffect(() => {
    onBusyChange(maintenanceBusy);
  }, [maintenanceBusy, onBusyChange]);
  useEffect(() => () => onBusyChange(false), [onBusyChange]);

  async function recover() {
    if (!pending || !confirm || !exclusive || busy || loading || inspectionError || working || operation.current) return;
    operation.current = true;
    setWorking(true); setError(null); setNotice(null); setWarnings([]);
    const owner = lifecycle.current;
    const isCurrent = () => owner === lifecycle.current && currentInstance.current === instanceId;
    try {
      const result = await recoverArkClusterRestore({ instance_id: instanceId,
        expected_identity: pending.identity, backup_id: pending.backup_id, exclusive_root_confirmed: true });
      if (!isCurrent()) return;
      setNotice(result.outcome === "rolled_back"
        ? text("已将整个集群回滚到恢复操作之前。", "The entire cluster was rolled back to its state before the restore.")
        : text("此前的整组恢复已经完成，现已完成清理。", "The previous cluster restore was committed; cleanup is now complete."));
      setConfirm(false); setExclusive(false);
      await inspect();
      if (!isCurrent()) return;
      setWarnings(result.cleanup_warnings);
      await onChanged();
    } catch (failure) {
      if (isCurrent()) {
        setError(describeError(failure));
        await inspect();
      }
    } finally {
      if (isCurrent()) { operation.current = false; setWorking(false); }
    }
  }

  return <>
    {loading ? <ActivityNotice>{text("正在检查集群恢复记录…", "Checking cluster recovery state…")}</ActivityNotice> : null}
    {inspectionError ? <ActivityNotice tone="error" action={
      <button type="button" className="secondary-button" disabled={busy || working || loading} onClick={() => void inspect()}>{t("common.retry")}</button>}>
      {inspectionError}</ActivityNotice> : null}
    {error ? <ActivityNotice tone="error">{error}</ActivityNotice> : null}
    {notice ? <ActivityNotice tone="success">{notice}</ActivityNotice> : null}
    {warnings.length ? <ActivityNotice tone="warning">{warnings.join("\n")}</ActivityNotice> : null}
    {pending ? <section className="ark-cluster-restore-confirm" aria-label={text("处理上次中断的恢复", "Resolve interrupted restore")}>
      <h4>{text("处理上次中断的恢复", "Resolve interrupted restore")}</h4>
      <p>{text("集群恢复尚未结束，启动已被阻止。未完成提交时将整组回滚；已经提交时仅完成清理。", "An unfinished cluster restore blocks startup. An uncommitted restore is rolled back as a group; a committed restore only needs cleanup.")}</p>
      <p>{pending.safeguard_backup.members.map((member) => member.instance_name).join(" · ")}</p>
      <p><code>{pending.identity.directory_key}</code></p>
      <p>{text("恢复前保护快照：", "Pre-restore safeguard: ")}<code>{pending.safeguard_backup.backup_id}</code></p>
      {!confirm ? <button type="button" className="secondary-button" disabled={busy || working || loading} onClick={() => setConfirm(true)}>{text("查看处理范围…", "Review recovery scope…")}</button> : <>
        <p>{text("将处理以上全部成员配置、世界存档及共享传送目录。请停止全部成员，并确认目录专用于本集群且没有其他程序写入。", "This affects every listed member's configuration, worlds, and shared transfer directory. Stop all members and confirm the directory belongs exclusively to this cluster with no external writers.")}</p>
        <label className="ark-cluster-exclusive"><input type="checkbox" checked={exclusive} disabled={working || loading} onChange={(event) => setExclusive(event.target.checked)} />{text("我已核对范围及目录独占条件", "I have verified the scope and directory exclusivity")}</label>
        <div className="ark-cluster-actions"><button type="button" className="secondary-button" disabled={working} onClick={() => { setConfirm(false); setExclusive(false); }}>{t("common.cancel")}</button>
          <button type="button" className="primary-button" disabled={!exclusive || busy || loading || !!inspectionError || working} onClick={() => void recover()}>{working ? text("正在处理…", "Recovering…") : text("确认处理整个集群", "Confirm cluster recovery")}</button></div>
      </>}
    </section> : null}
    {!pending && inspected && report?.identity ? <ArkClusterBackupPanel report={report} busy={busy || working || loading || !!inspectionError}
      listBackups={listArkClusterBackups} createBackup={createArkClusterBackup} restoreBackup={restoreArkClusterBackup}
      onChanged={async () => { await inspect(); await onChanged(); }} onBusyChange={setBackupBusy} /> : null}
  </>;
}
