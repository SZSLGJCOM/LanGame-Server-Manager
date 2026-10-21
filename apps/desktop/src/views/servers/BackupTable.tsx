import { useId, useState } from "react";
import { BackupRenameAction } from "../../components/BackupRenameAction";
import { InlineConfirmAction } from "../../components/InlineConfirmAction";
import { ShellIcon } from "../../components/ShellIcon";
import type { LocaleCode, TranslateFn } from "../../i18n";
import type { InstanceBackupResult } from "../../types";
import { formatRelativeTime, formatTime } from "../../view-models";
import { formatStorageSize } from "./format-storage-size";
import { MaintenanceAction } from "./MaintenanceToolbar";
import { SaveFileBrowser } from "./SaveFileBrowser";
import { StorageRecordList, StorageRecordRow } from "./StorageRecordList";

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
  onOpenLocalPath?: (path: string) => void;
}

export function BackupTable(props: BackupTableProps) {
  if (props.backups.length === 0) {
    return <div className="server-file-backup-empty">{props.t("servers.backups.empty")}</div>;
  }
  return <StorageRecordList label={props.t("servers.backups.eyebrow")}>
    {props.backups.map((backup) => <BackupRow key={`${props.instanceId}:${backup.backup_id}`} {...props} backup={backup} />)}
  </StorageRecordList>;
}

function BackupRow({ backup, ...props }: BackupTableProps & { backup: InstanceBackupResult }) {
  const [open, setOpen] = useState(false);
  const regionId = useId();
  const { t, locale } = props;
  const name = backupName(backup);
  const restoreReason = props.readOnly ? t("servers.archives.workspace.restoreForMaintenance")
    : !props.canRestoreBackup ? t("servers.backups.restoreRequiresStop") : null;
  const details = [
    formatTime(locale, backup.created_at_unix_ms),
    t("servers.backups.filesMeta", { count: backup.file_count, size: formatStorageSize(backup.total_bytes) }),
    backup.backup_path
  ].filter(Boolean).join("\n");
  return <StorageRecordRow name={name} details={details}
    badge={<span className={`server-file-backup-kind server-file-backup-kind--${backup.backup_kind}`}>
      {t(`servers.backups.kind.${backup.backup_kind}`)}</span>}
    facts={<>
      <span>{formatStorageSize(backup.total_bytes)}</span>
      <span>{formatRelativeTime(locale, backup.created_at_unix_ms)}</span>
    </>}
    actions={<>
      <MaintenanceAction icon="restore" label={t("servers.backups.restore")} className="secondary-button"
        disabledReason={restoreReason} disabled={!props.onRestoreBackup}
        onClick={() => { if (!restoreReason) props.onRestoreBackup?.(props.instanceId, backup.backup_id); }} />
      <BackupRenameAction name={name} disabled={props.readOnly || !props.onRenameBackup}
        onRename={props.onRenameBackup && !props.readOnly
          ? (displayName) => props.onRenameBackup!(props.instanceId, backup, displayName) : undefined} />
      <MaintenanceAction icon="list" iconOnly label={t(open ? "servers.backups.hideFiles" : "servers.backups.showFiles")}
        disabled={props.readOnly} aria-expanded={open} aria-controls={regionId} onClick={() => setOpen((value) => !value)} />
      {props.onOpenLocalPath ? <MaintenanceAction icon="folder" iconOnly label={t("servers.backups.openFolder")}
        disabled={props.readOnly || !backup.backup_path}
        onClick={() => { if (backup.backup_path) props.onOpenLocalPath?.(backup.backup_path); }} /> : null}
      <InlineConfirmAction type="button" className="ghost-button danger maintenance-action maintenance-action--icon"
        aria-label={t("servers.backups.delete")} title={t("servers.backups.delete")}
        disabled={props.readOnly || !props.onDeleteBackup} scopeKey={`${props.instanceId}:${backup.backup_id}`}
        confirmation={t("servers.backups.deleteConfirm", { name })} confirmLabel={t("servers.backups.delete")}
        onConfirm={() => { if (!props.readOnly) return props.onDeleteBackup?.(props.instanceId, backup); }}>
        <ShellIcon name="trash" className="maintenance-action__icon" aria-hidden="true" />
      </InlineConfirmAction>
    </>}>
    {open && !props.readOnly ? <div id={regionId} className="server-file-backup-files">
      <SaveFileBrowser instanceId={props.instanceId} backupId={backup.backup_id} locale={locale} t={t}
        title={t("servers.saveFiles.backupTitle")} />
    </div> : null}
  </StorageRecordRow>;
}
