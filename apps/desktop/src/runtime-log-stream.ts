import type { InstanceDetails, InstanceRuntimeOverview, LogTailSnapshot } from "./types";

export const RUNTIME_LOG_STREAM_EVENT = "runtime-log-stream";
export const RUNTIME_SERVICE_EVENTS_RESET = "runtime-service-events-reset";

export interface RuntimeLogStreamEvent {
  instance_id: string;
  process_key?: string | null;
  display_name?: string | null;
  run_id?: number | null;
  log_path: string;
  lines: string[];
  byte_offset: number;
  emitted_at_unix_ms: number;
  snapshot?: LogTailSnapshot;
  snapshot_revision?: number;
  stream_error?: string;
}

export interface RuntimeLogStartupBoundary {
  instanceId: string;
  startedAtUnixMs: number;
  previousLogPaths: ReadonlySet<string>;
}

export function collectRuntimeLogPaths(
  details: InstanceDetails | null,
  runtime: InstanceRuntimeOverview | null
): Set<string> {
  const runs = [details?.active_run, ...(runtime?.recent_runs ?? [])];
  return new Set([runtime?.log_tail.source_path,
    ...runs.flatMap((run) => [run?.log_path, ...(run?.processes ?? []).map((process) => process.log_path)])
  ].map(normalizeRuntimeLogPath).filter(Boolean));
}

interface RuntimeLogStreamAppendOptions {
  instanceId: string;
  logPath?: string | null;
  followLatestPath?: boolean;
  mergeProcessLogs?: boolean;
  startupBoundary?: RuntimeLogStartupBoundary | null;
  maxLines: number;
}

export function normalizeRuntimeLogPath(value?: string | null): string {
  return String(value ?? "").replace(/\\/g, "/").replace(/\/+$/g, "").replace(/\/+/g, "/").toLowerCase();
}

export function retainRuntimeLogHistory(
  current: LogTailSnapshot | null,
  polled: LogTailSnapshot | null,
  maxLines: number
): LogTailSnapshot | null {
  if (!polled) return current;
  if (current && normalizeRuntimeLogPath(current.source_path) === normalizeRuntimeLogPath(polled.source_path)) {
    // The health overview contains only 32 lines. It cannot establish the
    // position of those lines in a longer live transcript, so never splice or
    // replace it. Source changes and explicit recovery use a full bounded read.
    return { ...current, read_error: polled.read_error ?? null };
  }
  const limit = Math.max(1, maxLines);
  return { ...polled, lines: polled.lines.slice(-limit),
    truncated: Boolean(polled.truncated) || polled.lines.length > limit };
}

export function appendRuntimeLogStreamEvent(
  snapshot: LogTailSnapshot | null,
  event: RuntimeLogStreamEvent,
  options: RuntimeLogStreamAppendOptions
): LogTailSnapshot | null {
  if (event.instance_id !== options.instanceId || (event.lines.length === 0 && !event.snapshot)) {
    return snapshot;
  }

  const eventPath = normalizeRuntimeLogPath(event.log_path);
  const boundary = options.startupBoundary;
  if (boundary?.instanceId === options.instanceId && (
    boundary.previousLogPaths.has(eventPath) || event.emitted_at_unix_ms < boundary.startedAtUnixMs
  )) {
    return snapshot;
  }
  const selectedPath =
    normalizeRuntimeLogPath(options.logPath) || normalizeRuntimeLogPath(snapshot?.source_path);
  const pathChanged = Boolean(selectedPath) && eventPath !== selectedPath;
  if (pathChanged && !options.followLatestPath) {
    return snapshot;
  }

  if (event.snapshot) {
    if (normalizeRuntimeLogPath(event.snapshot.source_path) !== eventPath) return snapshot;
    return retainNewestRuntimeGameDocument(snapshot, { ...event.snapshot, snapshot_revision: event.snapshot_revision }, options.maxLines);
  }

  const snapshotPath = normalizeRuntimeLogPath(snapshot?.source_path);
  const snapshotPathChanged = Boolean(snapshotPath) && snapshotPath !== eventPath;
  const baseSnapshot = (pathChanged || snapshotPathChanged) && !options.mergeProcessLogs ? null : snapshot;
  // A complete-line batch and final unterminated tail can have the same byte
  // offset, timestamp and text. Diagnostics reuse the cursor; rotation resets
  // it. The publisher supplies no event identity, so every delivery is retained.
  const existingLines = baseSnapshot?.lines ?? [];
  const totalLines = (baseSnapshot?.total_lines ?? existingLines.length) + event.lines.length;
  const maxLines = Math.max(1, options.maxLines);
  const incomingLines = options.mergeProcessLogs && event.process_key
    ? event.lines.map((line) => `[${event.process_key}] ${line}`)
    : event.lines;
  const nextLines = [...existingLines, ...incomingLines];
  const cappedLines = nextLines.slice(Math.max(0, nextLines.length - maxLines));

  const result: LogTailSnapshot = {
    source_path: baseSnapshot?.source_path || event.log_path,
    lines: cappedLines,
    total_lines: totalLines,
    truncated: Boolean(baseSnapshot?.truncated) || nextLines.length > maxLines,
    read_error: null
  };
  return result;
}

export function retainNewestRuntimeGameDocument(current: LogTailSnapshot | null, incoming: LogTailSnapshot, maxLines: number): LogTailSnapshot {
  if (current && normalizeRuntimeLogPath(current.source_path) === normalizeRuntimeLogPath(incoming.source_path)
    && current.snapshot_revision !== undefined && incoming.snapshot_revision !== undefined
    && current.snapshot_revision >= incoming.snapshot_revision) return current;
  const limit = Math.max(1, maxLines);
  return { ...incoming, lines: incoming.lines.slice(-limit), truncated: Boolean(incoming.truncated) || incoming.lines.length > limit };
}
