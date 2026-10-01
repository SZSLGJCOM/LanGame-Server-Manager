import type { ArkClusterIdentity } from "./ark-clusters";

export interface ArkClusterBackupSummary {
  backup_id: string;
  created_at_unix_ms: number;
  backup_kind: "manual" | "pre_restore";
  identity: ArkClusterIdentity;
  backup_path: string;
  file_count: number;
  total_bytes: number;
  members: Array<{ instance_id: string; instance_name: string; map_name: string }>;
}

export interface CreateArkClusterBackupInput {
  instance_id: string;
  expected_identity: ArkClusterIdentity;
  exclusive_root_confirmed: boolean;
}

export interface RestoreArkClusterBackupInput extends CreateArkClusterBackupInput {
  backup_id: string;
}

export interface ArkClusterRestoreResult {
  backup: ArkClusterBackupSummary;
  safeguard_backup: ArkClusterBackupSummary;
  restored_at_unix_ms: number;
  cleanup_warnings: string[];
}

export interface PendingArkClusterRestore {
  identity: ArkClusterIdentity;
  backup_id: string;
  safeguard_backup: ArkClusterBackupSummary;
}

export interface ArkClusterRecoveryResult {
  outcome: "rolled_back" | "completed";
  backup: ArkClusterBackupSummary;
  cleanup_warnings: string[];
}
