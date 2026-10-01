import { BackupRenameAction } from "../../components/BackupRenameAction";
import { InlineConfirmAction } from "../../components/InlineConfirmAction";
import type { LocaleCode, TranslateFn } from "../../i18n";
import type { InstanceBackupResult } from "../../types";
import { formatTime } from "../../view-models";

export function formatBackupSize(totalBytes: number): string {
  if (totalBytes >= 1024 * 1024) return `${(totalBytes / (1024 * 1024)).toFixed(1)} MB`;
  if (totalBytes >= 1024) return `${Math.round(totalBytes / 1024)} KB`;
  return `${totalBytes} B`;
}

export function backupName(backup: InstanceBackupResult): string {
  const customLabel = String(backup.display_name ?? "").trim();
  const basename = backup.backup_path.replace(/\\/g, "/").replace(/\/+$/g, "").split("/").slice(-1)[0];
  return customLabel || basename || backup.backup_id;
}
export interface BackupTableProps {
  backups: InstanceBackupResult[];
  locale: LocaleCode;
  t: TranslateFn;
  canRestoreBackup: boolean;
  readOnly?: boolean;
  instanceId: string;
  onRestoreBackup?: (instanceId: string, backupId: string) => void;
  onRenameBackup?: (instanceId: string, backup: InstanceBackupResult, displayName: string) => Promise<boolean>;
  onDeleteBackup?: (instanceId: string, backup: InstanceBackupResult) => void | Promise<void>;
}

export function BackupTable(props: BackupTableProps) {
  if (props.backups.length === 0) {
    return <div className="server-file-backup-empty">{props.t("servers.backups.empty")}</div>;
  }

  return (
    <div
      className="server-file-backup-table"
      role="list"
      aria-label={props.t("servers.backups.eyebrow")}
    >
      {props.backups.map((backup) => (
        <article key={`${props.instanceId}:${backup.backup_id}`} className="server-file-backup-row" role="listitem">
          <div className="server-file-backup-main">
            <div className="server-file-backup-name" title={backupName(backup)}>{backupName(backup)}</div>
            <div className="server-file-backup-meta">
              <span className={`server-file-backup-kind server-file-backup-kind--${backup.backup_kind}`}>
                {props.t(`servers.backups.kind.${backup.backup_kind}`)}
              </span>
              <span>{formatTime(props.locale, backup.created_at_unix_ms)}</span>
              <span>
                {props.t("servers.backups.filesMeta", {
                  count: backup.file_count,
                  size: formatBackupSize(backup.total_bytes)
                })}
              </span>
            </div>
          </div>
          <div className="server-file-backup-actions">
            <button
              type="button"
              className="secondary-button"
              disabled={props.readOnly || !props.canRestoreBackup || !props.onRestoreBackup}
              onClick={() => { if (!props.readOnly && props.canRestoreBackup) props.onRestoreBackup?.(props.instanceId, backup.backup_id); }}
            >
              {props.t("servers.backups.restore")}
            </button>
            <BackupRenameAction
              name={backupName(backup)}
              disabled={props.readOnly || !props.onRenameBackup}
              onRename={props.onRenameBackup && !props.readOnly
                ? (displayName) => props.onRenameBackup!(props.instanceId, backup, displayName) : undefined}
            />
            <InlineConfirmAction
              type="button"
              className="ghost-button danger"
              disabled={props.readOnly || !props.onDeleteBackup}
              scopeKey={`${props.instanceId}:${backup.backup_id}`}
              confirmation={props.t("servers.backups.deleteConfirm", { name: backupName(backup) })}
              confirmLabel={props.t("servers.backups.delete")}
              onConfirm={() => { if (!props.readOnly) return props.onDeleteBackup?.(props.instanceId, backup); }}
            >
              {props.t("servers.backups.delete")}
            </InlineConfirmAction>
          </div>
        </article>
      ))}
    </div>
  );
}
