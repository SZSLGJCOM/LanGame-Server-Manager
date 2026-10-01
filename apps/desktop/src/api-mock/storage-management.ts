import type { AppSettings, InstanceBackupResult, InstanceArchiveResult, InstanceDetails, InstanceSummary, ModuleSummary } from "../types";
import type { InstanceArchiveDetails, InstanceArchiveList, InstanceArchiveSummary, StorageUsageEntry, StorageUsageReport } from "../storage-management-types";

interface ArchivedFixture {
  summary: InstanceArchiveSummary;
  details: InstanceDetails;
  backups: InstanceBackupResult[];
  program: { mode: "shared" | "independent"; root: string } | undefined;
}

function normalizedPath(value: string) {
  return value.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
}

function within(path: string, root: string) {
  return normalizedPath(path).startsWith(`${normalizedPath(root)}/`);
}

export class MockStorageManagement {
  private readonly archives = new Map<string, ArchivedFixture>();
  private readonly scans = new Map<string, { cancelled: boolean }>();

  validatePathChange(current: AppSettings, next: AppSettings) {
    const rootsChanged = normalizedPath(current.archives_root) !== normalizedPath(next.archives_root)
      || normalizedPath(current.servers_root) !== normalizedPath(next.servers_root);
    if (rootsChanged && this.archives.size > 0) {
      throw new Error("Restore or permanently delete all archived instances before changing the archive or instance workspace path.");
    }
  }

  withProgramCounts(modules: ModuleSummary[], instances: InstanceSummary[], programs: ReadonlyMap<string, { mode: "shared" | "independent" }>, root: string): ModuleSummary[] {
    return modules.map((module) => ({ ...module,
      instance_program_count: instances.filter((instance) => instance.module_id === module.id
        && programs.get(instance.id)?.mode !== "shared").length,
      archived_program_count: [...this.archives.values()].filter((archive) => archive.summary.module_id === module.id
        && within(archive.summary.archived_instance_root, root) && archive.program?.mode !== "shared").length
    }));
  }

  archive(result: InstanceArchiveResult, details: InstanceDetails, backups: InstanceBackupResult[], program: ArchivedFixture["program"]) {
    if (!result.archived_instance_root) return;
    const archiveId = result.archive_id;
    this.archives.set(archiveId, structuredClone({
      summary: {
        archive_id: archiveId, instance_id: result.instance_id, instance_name: result.instance_name,
        module_id: result.module_id, deleted_at_unix_ms: result.deleted_at_unix_ms,
        archived_instance_root: result.archived_instance_root, previous_instance_root: result.previous_instance_root,
        preserved_external_saves_path: result.preserved_external_saves_path ?? null,
        external_saves_backup_id: result.external_saves_backup_id,
        program_storage: "full", omitted_program_bytes: 0, omitted_program_files: 0,
        required_program_fingerprint: null, required_program_version: null, program_retention_reason: null,
        state: "archived", can_restore: true, can_purge: true, issues: []
      }, details, backups, program
    }));
  }

  list(root: string): InstanceArchiveList {
    return { archives: [...this.archives.values()].filter((value) => within(value.summary.archived_instance_root, root))
      .map((value) => structuredClone(value.summary)), pending_deletions: [], issues: [] };
  }

  private require(archiveId: string, root: string) {
    const value = this.archives.get(archiveId);
    if (!value || !within(value.summary.archived_instance_root, root)) throw new Error("The selected archive no longer exists in this workspace.");
    return value;
  }

  details(archiveId: string, root: string): InstanceArchiveDetails {
    const value = this.require(archiveId, root);
    if (value.summary.state !== "archived") throw new Error("Details are available only for a completed archive.");
    return structuredClone({
      archive_id: archiveId,
      instance: { ...value.details, summary: { ...value.details.summary, active_process_count: 0 }, active_run: null,
        config_file_path: `${value.summary.archived_instance_root}/config/instance.json` },
      maintenance: {
        autostart: value.details.summary.autostart,
        auto_backup_on_stop: value.details.auto_backup_on_stop,
        backup_retention_count: value.details.backup_retention_count,
        crash_restart_limit: null,
        runtime_mode: value.program?.mode ?? null
      },
      runs: { entries: [], total: 0, truncated: false },
      log: { relative_path: null, text: "", truncated: false, issues: ["The development preview does not retain runtime logs."] },
      backups: { entries: value.backups.map((backup) => ({ ...backup,
        backup_path: `${value.summary.archived_instance_root}/backups/${backup.backup_id}` })), issues: [], truncated: false }
    });
  }

  restore(archiveId: string, root: string, activeIds: string[]): ArchivedFixture {
    const value = this.require(archiveId, root);
    if (!value.summary.can_restore) throw new Error(value.summary.issues.join("; ") || "This archive cannot be restored.");
    if (activeIds.includes(value.details.summary.id)) throw new Error("An active instance already owns this identifier.");
    this.archives.delete(archiveId);
    return structuredClone(value);
  }

  purge(archiveId: string, root: string) {
    const value = this.require(archiveId, root);
    if (!value.summary.can_purge) throw new Error("The selected archive cannot be safely removed.");
    this.archives.delete(archiveId);
    return { archive_id: archiveId, purged: true };
  }

  cancel(scanId: string) {
    const scan = this.scans.get(scanId);
    if (!scan) return false;
    scan.cancelled = true;
    return true;
  }

  async scan(scanId: string, settings: AppSettings, instances: InstanceDetails[]): Promise<StorageUsageReport> {
    if (this.scans.size) throw new Error("A storage scan is already running.");
    const started = Date.now();
    const operation = { cancelled: false };
    this.scans.set(scanId, operation);
    try {
      await new Promise<void>((resolve) => setTimeout(resolve, 0));
      const entries: StorageUsageEntry[] = operation.cancelled ? [] : instances.map((details) => ({
        id: `data:${details.summary.id}`, category: "instance_data", label: details.summary.name,
        path: details.config_file_path, instance_id: details.summary.id, module_id: details.summary.module_id,
        logical_bytes: new TextEncoder().encode(details.settings_json).length, allocated_bytes: null,
        file_count: 1, status: "partial", issues: ["Preview configuration data only; no host filesystem was scanned."]
      }));
      for (const archive of operation.cancelled ? [] : this.list(settings.archives_root).archives) {
        entries.push({ id: archive.archive_id, category: "archives", label: archive.instance_name ?? archive.archive_id,
          path: archive.archived_instance_root, instance_id: archive.instance_id, module_id: archive.module_id,
          logical_bytes: 0, allocated_bytes: null, file_count: 0, status: "partial", issues: ["Preview archive size is not measured."] });
      }
      return { scan_id: scanId, started_at_unix_ms: started, finished_at_unix_ms: Date.now(),
        status: operation.cancelled ? "cancelled" : "partial", logical_bytes: entries.reduce((sum, entry) => sum + entry.logical_bytes, 0),
        allocated_bytes: null, file_count: entries.reduce((sum, entry) => sum + entry.file_count, 0), skipped_links: 0, entries,
        issues: operation.cancelled ? [] : ["Development preview does not measure disk usage. Run the desktop host to scan managed directories."] };
    } finally { this.scans.delete(scanId); }
  }
}
