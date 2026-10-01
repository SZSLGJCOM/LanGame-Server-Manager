import { isTauri } from "@tauri-apps/api/core";
import { invokeOrMock, shouldUseLanApi } from "./api-transport";
import { storageManagementRequests } from "./storage-management-requests";
import type {
  InstanceArchiveDetails, InstanceArchiveList, InstanceArchivePurgeResult, InstanceArchiveRestoreResult, StorageUsageReport,
  InstanceRemovalPlan, ModuleProgramInventory
} from "./storage-management-types";
import type { InstanceProgramMode } from "./types";

export const isStorageManagementAvailable = () => isTauri() || !shouldUseLanApi();

function onStorageHost<T>(action: () => Promise<T>): Promise<T> {
  if (!isStorageManagementAvailable()) return Promise.reject(new Error("Storage management is available only on the desktop host."));
  return action();
}

function mutateStorage<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return onStorageHost(() => storageManagementRequests.mutate(() => onStorageHost(() => invokeOrMock<T>(command, args))));
}

export const listInstanceArchives = () => onStorageHost(() =>
  storageManagementRequests.list(() => onStorageHost(() => invokeOrMock<InstanceArchiveList>("list_instance_archives"))));
export const readInstanceArchiveDetails = (archiveId: string, options?: { signal?: AbortSignal }) =>
  onStorageHost(() => storageManagementRequests.inspect(() => onStorageHost(() => {
    // Native inspection owns its lock until completion. Cancellation prevents
    // obsolete queued selections from issuing a request; callers ignore late results.
    options?.signal?.throwIfAborted();
    return invokeOrMock<InstanceArchiveDetails>("read_instance_archive_details", { input: { archive_id: archiveId } });
  })));
export const restoreInstanceArchive = (archiveId: string) =>
  mutateStorage<InstanceArchiveRestoreResult>("restore_instance_archive", { input: { archive_id: archiveId } });
export const purgeInstanceArchive = (archiveId: string) =>
  mutateStorage<InstanceArchivePurgeResult>("purge_instance_archive", { input: { archive_id: archiveId } });
export const scanStorageUsage = (scanId: string) =>
  // Never queue a scan whose cancellation identifier has not reached the host.
  onStorageHost(() => storageManagementRequests.mutate(() =>
    invokeOrMock<StorageUsageReport>("scan_storage_usage", { input: { scan_id: scanId } }), { immediate: true }));
export const cancelStorageUsageScan = (scanId: string) =>
  onStorageHost(() => invokeOrMock<boolean>("cancel_storage_usage_scan", { input: { scan_id: scanId } }));
export const inspectModulePrograms = async (moduleId: string, programMode: InstanceProgramMode, programSource: "verified" | "local") => {
  const inspect = (includeArchivedSources: boolean) => invokeOrMock<ModuleProgramInventory>("inspect_module_programs", {
    input: { module_id: moduleId, program_mode: programMode, program_source: programSource, include_archived_sources: includeArchivedSources }
  });
  // Existing library/instance sources do not read archive metadata. Only the
  // host can decide whether the fallback needs the shared archive inventory.
  const current = await inspect(false);
  return current.requires_archive_inventory ? storageManagementRequests.inspect(() => inspect(true)) : current;
};
export const inspectInstanceRemoval = (instanceId: string) =>
  invokeOrMock<InstanceRemovalPlan>("inspect_instance_removal", { input: { instance_id: instanceId } });
