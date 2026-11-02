import { useContext, useState } from "react";
import { openLocalPath } from "../../api";
import { ActivityProgress, ActivityProgressDetails } from "../../components/ActivityProgress";
import { formatDownloadBytes } from "../../download-rate";
import type { StorageMigrationModel } from "../../hooks/useStorageMigration";
import { selectLocaleText, useI18n } from "../../i18n";
import { storageMigrationActive, storageMigrationTerminal, type StorageMigrationPhase, type StorageMigrationStatus } from "../../storage-migration-types";
import "./storage-migration.css";

function phaseLabel(phase: StorageMigrationPhase, locale: string): string {
  const labels: Record<StorageMigrationPhase, [string, string]> = {
    checking: ["正在检查迁移目录", "Checking migration folders"],
    ready: ["迁移尚未执行", "Migration has not started"],
    copying: ["正在迁移文件", "Moving files"],
    verifying: ["正在校验目标文件", "Verifying destination files"],
    applying: ["正在应用目录设置", "Applying folder settings"],
    cleaning: ["新目录已生效，正在清理原文件", "New folders applied; cleaning source files"],
    completed: ["目录迁移已完成", "Folder migration completed"],
    cancelled: ["迁移已取消", "Migration cancelled"],
    failed: ["目录迁移失败", "Folder migration failed"]
  };
  return selectLocaleText(locale, ...labels[phase]);
}

function PreservedDestinations({ destinations }: { destinations: StorageMigrationStatus["preserved_destinations"] }) {
  const { locale } = useI18n();
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const [opening, setOpening] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  async function openBackup(path: string) {
    setOpening(path); setError(null);
    try { await openLocalPath(path); }
    catch (failure) { setError(`${text("打开备份目录失败：", "Could not open the backup folder: ")}${failure instanceof Error ? failure.message : String(failure)}`); }
    finally { setOpening(null); }
  }
  return <div className="system-migration-preserved">
    <p>{text("旧目标目录已完整保留：", "Previous destination folders were preserved:")}</p>
    <ul>{destinations.map(destination => <li key={destination.backup_path} title={destination.original_path}>
      <code>{destination.backup_path}</code>
      <button type="button" className="system-runtime-path-action-btn system-path-picker-button system-migration-backup-open"
        aria-label={`${text("打开备份目录", "Open backup folder")}: ${destination.backup_path}`}
        disabled={opening !== null} onClick={() => void openBackup(destination.backup_path)}>
        {text(opening === destination.backup_path ? "正在打开" : "打开", opening === destination.backup_path ? "Opening" : "Open")}
      </button>
    </li>)}</ul>
    {error ? <p className="system-migration-backup-error" role="alert">{error}</p> : null}
  </div>;
}

export function StorageMigrationActivity({ migration }: { migration: StorageMigrationModel }) {
  const { locale } = useI18n();
  const details = useContext(ActivityProgressDetails);
  const text = (zh: string, en: string) => selectLocaleText(locale, zh, en);
  const status = migration.status;
  if (!migration.hasActivity) return null;
  const active = migration.recovering || Boolean(migration.pending) || migration.unconfirmed || migration.cancelRequested
    || !status?.recovery_required && storageMigrationActive(status);
  const error = migration.error ?? status?.error;
  const label = migration.recovering ? text("正在读取迁移状态", "Reading migration status")
    : migration.unconfirmed ? text("迁移结果未确认，正在查询原任务", "Outcome unconfirmed; checking the existing operation")
    : migration.cancelRequested ? text("已请求取消，正在确认结果", "Cancellation requested; awaiting confirmation")
    : migration.pending === "preview" ? phaseLabel("checking", locale)
    : status?.recovery_required ? text("迁移需要重启恢复", "Migration requires restart recovery")
    : status?.phase === "completed" && status.cleanup_warnings.length ? text("新目录已生效，部分原文件仍保留", "New folders applied; some source files remain")
    : migration.refreshError ? text("迁移已完成，界面状态需刷新", "Migration completed; interface refresh needed")
    : status?.conflict_count ? text(`${status.conflict_count} 项路径问题，未执行迁移`, `${status.conflict_count} path issues; migration blocked`)
    : status ? phaseLabel(status.phase, locale) : text("目录迁移失败", "Folder migration failed");
  const measured = status && ["copying", "verifying"].includes(status.phase);
  const percent = measured && status.total_bytes > 0 ? Math.min(100, status.completed_bytes / status.total_bytes * 100) : null;
  const canCancel = status?.can_cancel && !status.recovery_required && !storageMigrationTerminal(status) && !migration.pending && !migration.unconfirmed && !migration.cancelRequested;
  const canRetry = !active && !migration.refreshing && !status?.recovery_required && status?.phase !== "completed"
    && Boolean(migration.draft || status && ["ready", "failed", "cancelled"].includes(status.phase));
  const outcomeDiagnostic = migration.refreshError
    ? `${text("迁移已完成，无需再次迁移。刷新失败：", "Migration completed; do not repeat it. Refresh failed: ")}${migration.refreshError}`
    : error || undefined;
  const dismissDiagnostic = migration.dismissError
    ? `${text("无法更新消息关闭记录，请重试关闭：", "Could not save this notification dismissal. Please try closing it again: ")}${migration.dismissError}` : undefined;
  const diagnostic = [outcomeDiagnostic, dismissDiagnostic].filter(Boolean).join("\n") || undefined;
  return <div className="system-migration-activity" data-migration-phase={status?.phase ?? "recovering"}>
    <ActivityProgress label={label} title={diagnostic} active={active}
      error={status?.phase === "failed" || Boolean(error) || Boolean(migration.dismissError)} percent={percent} localProgress
      transfer={measured ? `${formatDownloadBytes(status.completed_bytes, locale)} / ${formatDownloadBytes(status.total_bytes, locale)}` : undefined}
      onStop={canCancel ? () => void migration.cancel() : undefined} stopRequested={migration.cancelRequested}
      onDismiss={migration.canDismiss ? migration.dismiss : undefined} dismissLabel={text("关闭迁移消息", "Close migration notification")}
      stopLabel={text(migration.cancelRequested ? "正在取消" : "取消迁移", migration.cancelRequested ? "Cancelling" : "Cancel migration")} />
    {details ? <div className="system-migration-problems">
      {status && status.phase !== "completed" && status.roots.length ? <ul className="system-migration-targets">
        {status.roots.map(root => <li key={root.source}><code>{root.source} → {root.target}</code></li>)}
      </ul> : null}
      {status?.conflict_count ? <div className="system-migration-conflicts" role="alert">
        <p>{text("请处理以下路径问题后重试。", "Resolve the following path issues and retry.")}</p>
        <ul>{status.conflicts.slice(0, 10).map((conflict, index) => <li key={`${index}:${conflict.target}`}>
          <code>{conflict.target}</code><span>{conflict.message}</span>
        </li>)}</ul>
        {status.conflict_count > 10 ? <p>{text(`显示 10 条，共 ${status.conflict_count} 个冲突。`, `Showing 10 of ${status.conflict_count} conflicts.`)}</p> : null}
      </div> : null}
      {status?.cleanup_warnings.length ? <div className="system-migration-warnings">
        <p>{text("新目录已经生效，请核对仍保留的原文件，无需再次迁移。", "New folders are active. Review the remaining source files; do not repeat migration.")}</p>
        <ul>{status.cleanup_warnings.map((warning, index) => <li key={index}>{warning}</li>)}</ul>
      </div> : null}
      {status?.preserved_destinations.length ? <PreservedDestinations key={status.operation_id} destinations={status.preserved_destinations} /> : null}
      <div className="system-migration-recovery-actions">
        {canRetry ? <button type="button" className="system-runtime-path-action-btn system-path-picker-button system-migration-retry"
          onClick={() => { const input = migration.draft ?? status?.input; if (input) void migration.migrate(input); }}>{text("重试迁移", "Retry migration")}</button> : null}
        {canRetry && migration.draft ? <button type="button" className="system-runtime-path-action-btn system-path-picker-button system-migration-discard"
          onClick={() => void migration.discard()}>{text("放弃更改", "Discard changes")}</button> : null}
        {migration.refreshError ? <button type="button" className="system-runtime-path-action-btn system-path-picker-button system-migration-refresh"
          disabled={migration.refreshing} onClick={() => void migration.refresh()}>{text("重新刷新状态", "Refresh status")}</button> : null}
        {migration.unconfirmed || migration.recovering && migration.error ? <button type="button"
          className="system-runtime-path-action-btn system-path-picker-button system-migration-recheck"
          disabled={Boolean(migration.pending)} onClick={migration.recheck}>{text("重新查询原任务", "Check existing operation")}</button> : null}
      </div>
    </div> : null}
  </div>;
}
