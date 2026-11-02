import type { AppPathSettingsInput } from "./types";

export type StorageMigrationPhase = "checking" | "ready" | "copying" | "verifying" | "applying" | "cleaning" | "completed" | "cancelled" | "failed";

export interface StorageMigrationStatus {
  operation_id: string;
  phase: StorageMigrationPhase;
  input: AppPathSettingsInput;
  roots: Array<{ source: string; target: string }>;
  total_files: number;
  total_bytes: number;
  completed_files: number;
  completed_bytes: number;
  reused_files: number;
  conflicts: Array<{ source: string; target: string; message: string }>;
  conflict_count: number;
  can_cancel: boolean;
  recovery_required: boolean;
  error: string | null;
  cleanup_warnings: string[];
  preserved_destinations: Array<{ original_path: string; backup_path: string }>;
}

export interface StorageMigrationApi {
  available: () => boolean;
  preview: (input: AppPathSettingsInput) => Promise<StorageMigrationStatus>;
  read: () => Promise<StorageMigrationStatus | null>;
  execute: (operationId: string) => Promise<StorageMigrationStatus>;
  cancel: (operationId: string) => Promise<StorageMigrationStatus>;
}

export function storageMigrationActive(status: StorageMigrationStatus | null): boolean {
  return Boolean(status && ["checking", "copying", "verifying", "applying", "cleaning"].includes(status.phase));
}

export function storageMigrationTerminal(status: StorageMigrationStatus): boolean {
  return status.phase === "completed" || status.phase === "cancelled" || status.phase === "failed";
}
