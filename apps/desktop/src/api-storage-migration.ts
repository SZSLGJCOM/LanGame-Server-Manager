import { isTauri } from "@tauri-apps/api/core";
import { invokeOrMock } from "./api-transport";
import type { StorageMigrationApi, StorageMigrationPhase, StorageMigrationStatus } from "./storage-migration-types";

const phases: ReadonlySet<string> = new Set(["checking", "ready", "copying", "verifying", "applying", "cleaning", "completed", "cancelled", "failed"]);
function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
function text(value: unknown): value is string { return typeof value === "string" && value.trim().length > 0; }
function count(value: unknown): value is number { return typeof value === "number" && Number.isSafeInteger(value) && value >= 0; }

export function parseStorageMigrationStatus(value: unknown, expectedOperationId?: string): StorageMigrationStatus {
  if (!record(value) || !text(value.operation_id) || expectedOperationId !== undefined && value.operation_id !== expectedOperationId
    || typeof value.phase !== "string" || !phases.has(value.phase) || !record(value.input)
    || !text(value.input.games_root) || !text(value.input.servers_root) || !text(value.input.archives_root) || !text(value.input.steamcmd_root)
    || !Array.isArray(value.roots) || value.roots.some(root => !record(root) || !text(root.source) || !text(root.target))
    || !count(value.total_files) || !count(value.total_bytes) || !count(value.completed_files) || !count(value.completed_bytes)
    || !count(value.reused_files) || !count(value.conflict_count) || typeof value.can_cancel !== "boolean"
    || value.error !== null && typeof value.error !== "string"
    || !Array.isArray(value.conflicts) || value.conflicts.some(conflict => !record(conflict)
      || !text(conflict.source) || !text(conflict.target) || !text(conflict.message))
    || value.conflict_count < value.conflicts.length
    || typeof value.recovery_required !== "boolean"
    || !Array.isArray(value.cleanup_warnings) || value.cleanup_warnings.some(warning => typeof warning !== "string")
    || !Array.isArray(value.preserved_destinations) || value.preserved_destinations.some(destination => !record(destination)
      || !text(destination.original_path) || !text(destination.backup_path))) {
    throw new Error("Invalid storage migration status. Check the existing operation before retrying.");
  }
  return {
    operation_id: value.operation_id, phase: value.phase as StorageMigrationPhase,
    input: { games_root: value.input.games_root, servers_root: value.input.servers_root,
      archives_root: value.input.archives_root, steamcmd_root: value.input.steamcmd_root },
    roots: value.roots as StorageMigrationStatus["roots"], total_files: value.total_files, total_bytes: value.total_bytes,
    completed_files: value.completed_files, completed_bytes: value.completed_bytes, reused_files: value.reused_files,
    conflicts: value.conflicts as StorageMigrationStatus["conflicts"], conflict_count: value.conflict_count,
    can_cancel: value.can_cancel, recovery_required: value.recovery_required, error: value.error, cleanup_warnings: value.cleanup_warnings as string[],
    preserved_destinations: value.preserved_destinations as StorageMigrationStatus["preserved_destinations"]
  };
}

async function request(command: string, args?: Record<string, unknown>): Promise<unknown> {
  if (!available()) throw new Error("Directory migration is available only on the local Windows LGSM desktop host.");
  return invokeOrMock<unknown>(command, args);
}

function available(): boolean { return isTauri() && /^Win/i.test(navigator.platform); }

export const storageMigrationApi: StorageMigrationApi = {
  available,
  preview: async input => parseStorageMigrationStatus(await request("preview_app_storage_migration", { input })),
  read: async () => {
    const value = await request("read_app_storage_migration");
    return value === null ? null : parseStorageMigrationStatus(value);
  },
  execute: async operationId => parseStorageMigrationStatus(await request("execute_app_storage_migration", { operationId }), operationId),
  cancel: async operationId => parseStorageMigrationStatus(await request("cancel_app_storage_migration", { operationId }), operationId)
};
