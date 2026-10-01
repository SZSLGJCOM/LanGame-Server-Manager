import { useCallback, useEffect, useRef, useState } from "react";
import { deleteInstance } from "../../api";
import { isStorageManagementAvailable, listInstanceArchives, purgeInstanceArchive, restoreInstanceArchive } from "../../api-storage";
import { describeError } from "../../app-state";
import { programCleanupDetails } from "../../app-ui";
import { useI18n } from "../../i18n";
import type { InstanceArchiveList, InstanceArchiveSummary, PendingInstanceDeletion } from "../../storage-management-types";

export function useInstanceArchives(onChanged: () => Promise<void>) {
  const { t } = useI18n();
  const available = isStorageManagementAvailable();
  const [list, setList] = useState<InstanceArchiveList | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notices, setNotices] = useState<Array<{ text: string; tone: "error" | "warning" | "success" }>>([]);
  const [operation, setOperation] = useState<{ id: string; kind: "restore" | "purge" | "delete" } | null>(null);
  const mounted = useRef(false);
  const revision = useRef(0);
  const mutation = useRef(false);
  const refreshGlobal = useRef(onChanged);
  refreshGlobal.current = onChanged;
  const refresh = useCallback(async () => {
    if (!available || !mounted.current) return;
    const request = ++revision.current;
    setLoading(true); setError(null);
    try {
      const result = await listInstanceArchives();
      if (mounted.current && revision.current === request) setList(result);
    } catch (cause) {
      if (mounted.current && revision.current === request) setError(describeError(cause));
    } finally {
      if (mounted.current && revision.current === request) setLoading(false);
    }
  }, [available]);
  useEffect(() => {
    mounted.current = true;
    if (available) void refresh();
    return () => { mounted.current = false; revision.current++; };
  }, [available, refresh]);
  async function run<T>(id: string, kind: "restore" | "purge" | "delete", action: () => Promise<T>, messages: (result: T) => Array<{ text: string; tone: "warning" | "success" }>): Promise<T | null> {
    if (!available || mutation.current) return null;
    mutation.current = true; setOperation({ id, kind }); setNotices([]);
    let result: T | null = null;
    try {
      result = await action();
      if (mounted.current) setNotices(messages(result));
    } catch (cause) {
      if (mounted.current) setNotices([{ text: describeError(cause), tone: "error" }]);
    } finally {
      // Both outcomes can change persistent state. A view unmount must not
      // prevent global instances from reflecting a completed storage operation.
      // Inventory refresh owns an exclusive lock also needed by bootstrap's
      // program counts. Finish it before publishing the refreshed server list.
      if (mounted.current) await refresh();
      const refreshed = await Promise.allSettled([
        Promise.resolve().then(() => refreshGlobal.current())
      ]);
      const failures = refreshed.flatMap((entry) => entry.status === "rejected" ? [describeError(entry.reason)] : []);
      if (mounted.current && failures.length) setNotices((current) => [...current,
        { text: t("storage.refreshFailed", { error: failures.join("; ") }), tone: "error" }]);
      mutation.current = false;
      if (mounted.current) setOperation(null);
    }
    return result;
  }
  function restore(archive: InstanceArchiveSummary) {
    if (!archive.can_restore) return Promise.resolve(null);
    return run(archive.archive_id, "restore", () => restoreInstanceArchive(archive.archive_id), (result) => [
      { text: t("storage.completed.restore"), tone: "success" },
      ...(result.external_saves_restore_required && result.external_saves_backup_id
        ? [{ text: t("storage.externalRestoreRequired", { backupId: result.external_saves_backup_id }), tone: "warning" as const }] : []),
      ...(result.preserved_external_saves_path
        ? [{ text: t("storage.externalSavesRemaining", { path: result.preserved_external_saves_path }), tone: "warning" as const }] : [])
    ]);
  }
  function purge(archive: InstanceArchiveSummary) {
    if (!archive.can_purge) return Promise.resolve(null);
    return run(archive.archive_id, "purge", async () => {
      const result = await purgeInstanceArchive(archive.archive_id);
      if (!result.purged) throw new Error(t("storage.purgeIncomplete"));
      return result;
    }, () => [{ text: t("storage.completed.purge"), tone: "success" }]);
  }
  function retry(entry: PendingInstanceDeletion) {
    if (!entry.can_retry) return Promise.resolve(null);
    return run(entry.operation_id, "delete", () => deleteInstance(entry.instance_id), (result) => {
      const details = programCleanupDetails(result.program_cleanup, t);
      return [
        { text: t("storage.completed.delete"), tone: "success" },
        ...(details ? [{ text: details, tone: "warning" as const }] : []),
        ...(result.preserved_external_saves_path ? [{ text: t("storage.externalSavesRemaining", { path: result.preserved_external_saves_path }), tone: "warning" as const }] : [])
      ];
    });
  }
  return { available, list, loading, error, notices, operation, refresh, restore, purge, retry };
}
