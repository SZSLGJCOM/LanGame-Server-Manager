import { isTauri } from "@tauri-apps/api/core";
import { invokeOrMock } from "./api-transport";

export interface SaveImportSupport {
  module_id: string;
  supported: boolean;
  source_hint: string;
}

export interface SaveImportPreview {
  instance_id: string;
  module_id: string;
  source_path: string;
  target_path: string;
  file_count: number;
  total_bytes: number;
  existing_file_count: number;
  existing_total_bytes: number;
  fingerprint: string;
  warnings?: string[];
  changes?: Array<{ path: string; file_count: number; total_bytes: number }>;
}

export interface SaveImportResult {
  instance_id: string;
  saves_path: string;
  safeguard_backup_path: string;
  copied_file_count: number;
  copied_total_bytes: number;
  warnings: string[];
  safeguard_backup_paths?: string[];
}

export function localSaveImportAvailable(): boolean {
  return isTauri();
}

function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
function count(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}
function nonempty(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0;
}

export async function readSaveImportSupport(instanceId: string, moduleId: string): Promise<SaveImportSupport> {
  const value = await invokeOrMock<unknown>("read_instance_save_import_support", { instanceId });
  if (!record(value) || value.module_id !== moduleId || typeof value.supported !== "boolean" ||
    typeof value.source_hint !== "string") throw new Error("Invalid save import support response.");
  return { module_id: moduleId, supported: value.supported, source_hint: value.source_hint };
}

export async function previewSaveImport(instanceId: string, moduleId: string, sourcePath: string): Promise<SaveImportPreview> {
  const value = await invokeOrMock<unknown>("preview_instance_save_import", { instanceId, sourcePath });
  if (!record(value) || value.instance_id !== instanceId || value.module_id !== moduleId ||
    !nonempty(value.source_path) || !nonempty(value.target_path) || !nonempty(value.fingerprint) ||
    !count(value.file_count) || value.file_count < 1 || !count(value.total_bytes) ||
    !count(value.existing_file_count) || !count(value.existing_total_bytes) ||
    (value.warnings !== undefined && (!Array.isArray(value.warnings) || value.warnings.some(item => typeof item !== "string"))) ||
    (value.changes !== undefined && (!Array.isArray(value.changes) || value.changes.length > 64 || value.changes.some(change =>
      !record(change) || !nonempty(change.path) || !count(change.file_count) || !count(change.total_bytes))))) {
    throw new Error("Invalid save import preview response.");
  }
  return { instance_id: instanceId, module_id: moduleId, source_path: value.source_path, target_path: value.target_path,
    file_count: value.file_count, total_bytes: value.total_bytes, existing_file_count: value.existing_file_count,
    existing_total_bytes: value.existing_total_bytes, fingerprint: value.fingerprint,
    warnings: value.warnings as string[] | undefined,
    changes: value.changes as SaveImportPreview["changes"] };
}

export async function importSave(instanceId: string, sourcePath: string, expectedFingerprint: string): Promise<SaveImportResult> {
  const value = await invokeOrMock<unknown>("import_instance_save", { instanceId, sourcePath, expectedFingerprint });
  if (!record(value) || value.instance_id !== instanceId || !nonempty(value.saves_path) ||
    !nonempty(value.safeguard_backup_path) || !count(value.copied_file_count) || value.copied_file_count < 1 ||
    !count(value.copied_total_bytes) || !Array.isArray(value.warnings) || value.warnings.some(warning => typeof warning !== "string") ||
    (value.safeguard_backup_paths !== undefined && (!Array.isArray(value.safeguard_backup_paths) ||
      value.safeguard_backup_paths.length > 8 || value.safeguard_backup_paths.some(path => !nonempty(path))))) {
    throw new Error("Invalid save import result response. Check the target and backups before retrying.");
  }
  return { instance_id: instanceId, saves_path: value.saves_path, safeguard_backup_path: value.safeguard_backup_path,
    copied_file_count: value.copied_file_count, copied_total_bytes: value.copied_total_bytes, warnings: value.warnings as string[],
    safeguard_backup_paths: value.safeguard_backup_paths as string[] | undefined };
}
