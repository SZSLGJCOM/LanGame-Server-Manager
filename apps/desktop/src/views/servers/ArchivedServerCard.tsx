import { InlineConfirmAction } from "../../components/InlineConfirmAction";
import { ShellIcon } from "../../components/ShellIcon";
import { useI18n } from "../../i18n";
import type { InstanceArchiveSummary, PendingInstanceDeletion } from "../../storage-management-types";
import { ServerCardFrame } from "./ServerCardFrame";

interface ArchiveProps {
  archive: InstanceArchiveSummary;
  active: boolean;
  disabled: boolean;
  restoring: boolean;
  onSelect: () => void;
  onRestore: () => Promise<unknown>;
  onPurge: () => Promise<unknown>;
  onOpenLocalPath: (path: string) => void;
}

export function ArchivedServerCard({ archive, active, disabled, restoring, onSelect, onRestore, onPurge, onOpenLocalPath }: ArchiveProps) {
  const { locale, t } = useI18n();
  const name = archive.instance_name ?? t("storage.unknownArchive");
  const status = t(archive.state === "archived" ? "servers.archives.archived" : `storage.archiveState.${archive.state}`);
  const date = archive.deleted_at_unix_ms === null ? t("storage.unknownDate") : new Date(archive.deleted_at_unix_ms).toLocaleString(locale);
  const dependency = !["missing_metadata", "unrecognized"].includes(archive.state)
    ? t(archive.program_storage === "reconstructable" ? "storage.archiveProgramDependency" : "storage.archiveProgramIncluded") : null;
  const details = [
    name,
    t(`storage.archiveState.${archive.state}`),
    t("storage.archiveNote"),
    `${t("servers.archives.date")}: ${date}`,
    `${t("storage.archivePath")}: ${archive.archived_instance_root}`,
    archive.previous_instance_root && `${t("storage.restorePath")}: ${archive.previous_instance_root}`,
    archive.preserved_external_saves_path && `${t("storage.externalSaves")}: ${archive.preserved_external_saves_path}`,
    dependency && `${t("storage.programStorage")}: ${t(`storage.programStorage.${archive.program_storage}`, { count: archive.omitted_program_files.toLocaleString(locale) })}`,
    archive.required_program_version && `${t("storage.requiredProgramVersion")}: ${archive.required_program_version}`,
    archive.program_storage === "reconstructable" && t("storage.reconstructableNote"),
    archive.program_retention_reason,
    ...archive.issues,
    !archive.can_restore && t("storage.restoreUnavailable"),
    t("storage.restoreNote"),
    archive.external_saves_backup_id && t("storage.externalSnapshotNote", { backupId: archive.external_saves_backup_id })
  ].filter(Boolean).join("\n\n");
  const restoreHelp = archive.can_restore ? t("storage.restoreNote")
    : [t("storage.restoreUnavailable"), ...archive.issues].join("\n\n");
  return <ServerCardFrame id={archive.archive_id} name={name} moduleId={archive.module_id ?? ""} kind="archive"
    active={active} status={archive.state} statusLabel={status} onSelect={onSelect} description={details}
    actions={<div className="row-card-actions row-card-actions--server server-list-card-actions">
      <button type="button" className="secondary-button server-list-card-icon-action server-list-card-open-folder"
        onClick={() => onOpenLocalPath(archive.archived_instance_root)}
        aria-label={`${t("servers.archives.openFolder")}: ${name}`} title={t("servers.archives.openFolder")}>
        <ShellIcon name="folder" className="server-list-card-action-icon" />
      </button>
      <InlineConfirmAction wrapperClassName="server-list-card-confirm server-list-card-delete"
        className="secondary-button danger server-list-card-icon-action" disabled={disabled || !archive.can_purge}
        scopeKey={`${archive.archive_id}:${archive.state}:${active}`} confirmation={t("storage.purgeConfirm", { name })}
        confirmLabel={t("storage.purge")} onConfirm={async () => { await onPurge(); }}
        aria-label={`${t("storage.purge")}: ${name}`} title={t("storage.purge")}>
        <ShellIcon name="trash" className="server-list-card-action-icon" />
      </InlineConfirmAction>
    </div>}
    metadata={<div className="server-list-card-meta" title={details}>
      {[dependency, date].filter(Boolean).join(" · ")}
    </div>}
    primary={<button type="button" className={`secondary-button server-list-card-primary-action${restoring ? " is-busy" : ""}`}
      disabled={disabled || !archive.can_restore} aria-busy={restoring}
      aria-label={`${t("servers.archives.restore")}: ${name}`} title={restoreHelp} aria-description={restoreHelp}
      onClick={() => void onRestore()}><ShellIcon name={restoring ? "refresh" : "restore"} className="server-list-card-action-icon" />
      <span>{t(restoring ? "storage.restoring" : "servers.archives.restore")}</span>
    </button>} />;
}

interface DeletionProps { entry: PendingInstanceDeletion; active: boolean; disabled: boolean; onSelect: () => void; onRetry: () => Promise<unknown> }

export function DeletionErrorCard({ entry, active, disabled, onSelect, onRetry }: DeletionProps) {
  const { t } = useI18n();
  return <ServerCardFrame id={entry.operation_id} name={entry.instance_name} moduleId={entry.module_id} kind="deletion-error"
    active={active} status="error" statusLabel={t("servers.archives.deleteFailed")} onSelect={onSelect}
    metadata={<div className="server-list-card-meta" title={entry.issues.join("\n")}>{entry.issues[0] ?? t("storage.pendingDeletionNote")}</div>}
    primary={<InlineConfirmAction wrapperClassName="server-list-card-confirm server-list-card-retry"
      className="secondary-button danger server-list-card-primary-action" disabled={disabled || !entry.can_retry}
      scopeKey={`${entry.operation_id}:${entry.can_retry}:${active}`} confirmation={t("storage.retryDeletionConfirm", { name: entry.instance_name })}
      confirmLabel={t("storage.retryDeletion")} onConfirm={async () => { await onRetry(); }}
      aria-label={`${t("storage.retryDeletion")}: ${entry.instance_name}`}>
      <ShellIcon name="trash" className="server-list-card-action-icon" /><span>{t("servers.archives.retryDelete")}</span>
    </InlineConfirmAction>} />;
}
