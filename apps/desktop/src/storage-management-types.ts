import type { InstallState, InstanceBackupResult, InstanceDetails } from "./types";

export interface ProgramInstallationSummary {
  id: number;
  install_root: string;
  scope: "library" | "instance";
  install_state: InstallState;
  pending_removal: boolean;
  current_version: string | null;
  used_by: { id: string; name: string }[];
  modification_state: "verified_original" | "modified_or_used" | "unverified";
  size_bytes: number | null;
}

export interface InstanceProgramCreationPlan {
  can_create: boolean;
  action: "existing_install" | "shared_install" | "independent_install";
  program_path: string;
  additional_bytes: number | null;
  reason: string | null;
}

export interface ModuleProgramInventory {
  requires_archive_inventory: boolean;
  installations: ProgramInstallationSummary[];
  creation: InstanceProgramCreationPlan;
}

export interface InstanceRemovalPlan {
  program_path: string;
  data_path: string;
  remove_program: boolean;
  preserved_program_path: string | null;
  owned_data_paths: string[];
  preserved_external_saves_path: string | null;
}

export interface InstanceArchiveSummary {
  archive_id: string;
  instance_id: string | null;
  instance_name: string | null;
  module_id: string | null;
  deleted_at_unix_ms: number | null;
  archived_instance_root: string;
  previous_instance_root: string | null;
  preserved_external_saves_path: string | null;
  external_saves_backup_id: string | null;
  state: "archiving" | "archived" | "restoring" | "purging" | "missing_metadata" | "unrecognized" | "restored" | "purged";
  can_restore: boolean;
  can_purge: boolean;
  program_storage: "full" | "reconstructable";
  omitted_program_bytes: number;
  omitted_program_files: number;
  required_program_fingerprint: string | null;
  required_program_version: string | null;
  program_retention_reason: string | null;
  issues: string[];
}

export interface PendingInstanceDeletion {
  operation_id: string;
  instance_id: string;
  instance_name: string;
  module_id: string;
  deleted_instance_root: string;
  started_at_unix_ms: number | null;
  can_retry: boolean;
  issues: string[];
}

export interface InstanceArchiveList {
  archives: InstanceArchiveSummary[];
  pending_deletions: PendingInstanceDeletion[];
  issues: string[];
}

export interface InstanceArchiveDetails {
  archive_id: string;
  instance: InstanceDetails;
  maintenance: {
    autostart: boolean;
    auto_backup_on_stop: boolean;
    backup_retention_count: number;
    crash_restart_limit: number | null;
    runtime_mode: string | null;
  };
  runs: {
    entries: {
      id: number;
      status: string;
      started_at: string | null;
      stopped_at: string | null;
      exit_code: number | null;
      crash_flag: boolean;
      display_name: string | null;
    }[];
    total: number;
    truncated: boolean;
  };
  log: { relative_path: string | null; text: string; truncated: boolean; issues: string[] };
  backups: { entries: InstanceBackupResult[]; issues: string[]; truncated: boolean };
}

export interface InstanceArchiveRestoreResult {
  archive_id: string;
  instance_id: string;
  instance_name: string;
  restored_instance_root: string;
  external_saves_backup_id: string | null;
  preserved_external_saves_path: string | null;
  external_saves_restore_required: boolean;
}

export interface InstanceArchivePurgeResult {
  archive_id: string;
  purged: boolean;
}

export type StorageUsageCategory = "library" | "instance_program" | "instance_data" | "backups" | "archives" | "other";

export interface StorageUsageEntry {
  id: string;
  category: StorageUsageCategory;
  label: string;
  path: string;
  instance_id: string | null;
  module_id: string | null;
  logical_bytes: number;
  allocated_bytes: number | null;
  file_count: number;
  status: "complete" | "partial" | "missing";
  issues: string[];
}

export interface StorageUsageReport {
  scan_id: string;
  started_at_unix_ms: number;
  finished_at_unix_ms: number;
  status: "complete" | "partial" | "cancelled";
  logical_bytes: number;
  allocated_bytes: number | null;
  file_count: number;
  skipped_links: number;
  entries: StorageUsageEntry[];
  issues: string[];
}
