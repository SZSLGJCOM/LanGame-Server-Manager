import type { LogTailSnapshot } from "./types";
import type { RuntimeLogStreamEvent } from "./runtime-log-stream";

const pathKey = (value?: string | null) => String(value ?? "").replace(/\\/g, "/")
  .replace(/\/+$/g, "").replace(/\/+/g, "/").toLowerCase();

export function runtimeLogDocumentPatchNeedsRefresh(snapshot: LogTailSnapshot | null, event: RuntimeLogStreamEvent): boolean {
  const patch = event.document_patch;
  if (!patch) return false;
  if (!snapshot || pathKey(snapshot.source_path) !== pathKey(event.log_path) || snapshot.snapshot_revision === undefined) return true;
  if (snapshot.snapshot_revision >= patch.revision) return false;
  return snapshot.snapshot_revision !== patch.previous_revision
    || !Number.isSafeInteger(patch.start) || patch.start < 0 || patch.start > snapshot.lines.length
    || !Number.isSafeInteger(patch.delete_count) || patch.delete_count < 0 || patch.delete_count > snapshot.lines.length - patch.start;
}

export function appendRuntimeLogDocumentPatch(snapshot: LogTailSnapshot | null, event: RuntimeLogStreamEvent, maxLines: number): LogTailSnapshot | null {
  const patch = event.document_patch;
  if (!snapshot || !patch || runtimeLogDocumentPatchNeedsRefresh(snapshot, event)
    || (snapshot.snapshot_revision !== undefined && snapshot.snapshot_revision >= patch.revision)) return snapshot;
  const lines = [...snapshot.lines.slice(0, patch.start), ...patch.lines, ...snapshot.lines.slice(patch.start + patch.delete_count)];
  const limit = Math.max(1, maxLines);
  return { ...snapshot, snapshot_revision: patch.revision, source_path: event.log_path, lines: lines.slice(-limit),
    total_lines: patch.total_lines, truncated: patch.truncated || lines.length > limit, read_error: patch.read_error };
}
