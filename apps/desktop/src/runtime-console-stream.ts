import type { LogTailSnapshot, RuntimeConsoleSourceSnapshot } from "./types";
import type { RuntimeLogStreamEvent } from "./runtime-log-stream";

interface ConsoleMergeOptions {
  maxLines: number;
  mergeProcessLogs?: boolean;
  startupBoundary?: { startedAtUnixMs: number; previousLogPaths: ReadonlySet<string> } | null;
}

// UTF-16 length is a cheap conservative content budget for the webview. The
// complete transcript remains in the owned log file; these are tail windows.
export const RUNTIME_CONSOLE_TEXT_BYTES = 1024 * 1024;
function retainTextTail<T>(items: readonly T[], maxLines: number, text: (item: T) => string) {
  let start = items.length, bytes = 0;
  const count = Math.max(1, maxLines);
  while (start > 0 && items.length - start < count) {
    const nextBytes = text(items[start - 1]).length * 2;
    if (bytes + nextBytes > RUNTIME_CONSOLE_TEXT_BYTES) break;
    bytes += nextBytes;
    start--;
  }
  return { items: items.slice(start), truncated: start > 0 };
}

const pathKey = (path: string) => path.replace(/\\/g, "/").replace(/\/+$/g, "").replace(/\/+/g, "/").toLowerCase();
const sourceFor = (snapshot: LogTailSnapshot | null, path: string) =>
  snapshot?.console_sources?.find(source => pathKey(source.log_path) === pathKey(path));
const formatLine = (source: RuntimeConsoleSourceSnapshot, line: string, merge: boolean) =>
  merge && source.process_key ? `[${source.process_key}] ${line}` : line;

export function markRuntimeConsoleSourceForRefresh(snapshot: LogTailSnapshot | null, logPath: string): LogTailSnapshot | null {
  if (!sourceFor(snapshot, logPath) || !snapshot) return snapshot;
  const paths = new Set(snapshot.console_refresh_paths ?? []);
  paths.add(pathKey(logPath));
  return { ...snapshot, console_refresh_paths: [...paths] };
}

export function runtimeConsoleSourceNeedsReadback(snapshot: LogTailSnapshot | null, logPath?: string | null): boolean {
  return Boolean(logPath && snapshot?.console_refresh_paths?.some(path => pathKey(path) === pathKey(logPath)));
}

function confirmSources(snapshot: LogTailSnapshot | null, paths: ReadonlySet<string>): LogTailSnapshot | null {
  if (!snapshot?.console_refresh_paths?.length) return snapshot;
  const remaining = snapshot.console_refresh_paths.filter(path => !paths.has(pathKey(path)));
  return remaining.length === snapshot.console_refresh_paths.length ? snapshot : { ...snapshot, console_refresh_paths: remaining };
}

function olderGeneration(source: RuntimeConsoleSourceSnapshot, streamId: string, startedAt: number): boolean {
  return source.stream_id !== streamId && source.started_at_unix_ms >= startedAt;
}

/** Replays and older generations are harmless. Only a missing predecessor needs I/O. */
export function runtimeConsoleEventNeedsRefresh(snapshot: LogTailSnapshot | null, event: RuntimeLogStreamEvent): boolean {
  const delta = event.console;
  if (!delta) return false;
  const source = sourceFor(snapshot, event.log_path);
  if (source && (olderGeneration(source, delta.stream_id, delta.started_at_unix_ms)
    || (source.stream_id === delta.stream_id && source.revision >= delta.revision))) return false;
  if (source?.stream_id === delta.stream_id) return delta.previous_revision !== source.revision;
  return !delta.reset;
}

function materialize(
  current: LogTailSnapshot | null, sources: RuntimeConsoleSourceSnapshot[],
  rows: NonNullable<LogTailSnapshot["console_rows"]>, options: ConsoleMergeOptions, sourcePath?: string | null
): LogTailSnapshot {
  const limit = Math.max(1, options.maxLines);
  const bounded = retainTextTail(rows, limit, row => row.line);
  const retained = bounded.items;
  const byStream = new Map(sources.map(source => [source.stream_id, source]));
  const lines = retained.map(row => {
    const source = byStream.get(row.stream_id);
    return source ? formatLine(source, row.line, Boolean(options.mergeProcessLogs)) : row.line;
  });
  return { ...current, source_path: sourcePath || current?.source_path || sources[0]?.log_path || null,
    console_sources: sources, console_rows: retained, lines,
    total_lines: sources.reduce((total, source) => total + source.total_lines, 0),
    truncated: sources.some(source => source.truncated) || bounded.truncated
      || sources.reduce((total, source) => total + source.total_lines, 0) > retained.length,
    read_error: sources.filter(source => source.read_error).map(source =>
      formatLine(source, source.read_error ?? "", Boolean(options.mergeProcessLogs))).join("\n") || null };
}

export function selectRuntimeConsoleSource(
  snapshot: LogTailSnapshot | null, logPath: string, maxLines = 400
): LogTailSnapshot | null {
  if (!snapshot) return null;
  const key = pathKey(logPath);
  if (!snapshot.console_sources) return snapshot.source_path && pathKey(snapshot.source_path) === key ? snapshot : null;
  const sources = snapshot.console_sources.filter(source => pathKey(source.log_path) === key).map(source => {
    const bounded = retainTextTail(source.lines, maxLines, line => line);
    return { ...source, lines: bounded.items, truncated: source.truncated || bounded.truncated };
  });
  if (!sources.length) return null;
  const streams = new Set(sources.map(source => source.stream_id));
  const rows = snapshot.console_rows?.filter(row => streams.has(row.stream_id))
    ?? sources.flatMap(source => source.lines.map(line => ({ stream_id: source.stream_id, line })));
  return materialize({ ...snapshot, console_refresh_paths: snapshot.console_refresh_paths?.filter(path => pathKey(path) === key) },
    sources, rows, { maxLines }, sources[0].log_path);
}

export function appendRuntimeConsoleEvent(
  snapshot: LogTailSnapshot | null, event: RuntimeLogStreamEvent, options: ConsoleMergeOptions
): LogTailSnapshot | null {
  const delta = event.console;
  if (!delta || runtimeConsoleEventNeedsRefresh(snapshot, event)) return snapshot;
  const previous = sourceFor(snapshot, event.log_path);
  if (previous && (olderGeneration(previous, delta.stream_id, delta.started_at_unix_ms)
    || (previous.stream_id === delta.stream_id && previous.revision >= delta.revision))) return snapshot;
  const base = previous?.stream_id === delta.stream_id ? previous : null;
  const limit = Math.max(1, options.maxLines);
  const lines = [...(base?.lines ?? []), ...event.lines];
  const bounded = retainTextTail(lines, limit, line => line);
  const source: RuntimeConsoleSourceSnapshot = {
    log_path: event.log_path, process_key: event.process_key ?? null, display_name: event.display_name ?? null,
    run_id: event.run_id ?? null, started_at_unix_ms: delta.started_at_unix_ms,
    stream_id: delta.stream_id, revision: delta.revision, lines: bounded.items,
    partial_lines: [...delta.partial_lines], total_lines: (base?.total_lines ?? 0) + event.lines.length,
    truncated: Boolean(base?.truncated) || bounded.truncated
  };
  const sources = (snapshot?.console_sources ?? []).filter(existing =>
    options.mergeProcessLogs || pathKey(existing.log_path) === pathKey(event.log_path));
  const index = previous ? sources.indexOf(previous) : -1;
  if (index >= 0) sources[index] = source;
  else sources.push(source);
  const retainedStreams = new Set(sources.map(existing => existing.stream_id));
  const rows = (snapshot?.console_rows ?? []).filter(row => retainedStreams.has(row.stream_id)
    && (!previous || base || row.stream_id !== previous.stream_id));
  rows.push(...event.lines.map(line => ({ stream_id: source.stream_id, line })));
  return materialize(confirmSources(snapshot, new Set([pathKey(event.log_path)])), sources, rows, options,
    options.mergeProcessLogs ? undefined : event.log_path);
}

/** Reads use the same source revisions as events. Recovery may regroup shards,
 * while ordinary deltas retain their arrival order across every source.
 */
export function mergeRuntimeConsoleSnapshot(
  current: LogTailSnapshot | null, incoming: LogTailSnapshot, options: ConsoleMergeOptions
): LogTailSnapshot | null {
  if (!incoming.console_sources) return current;
  const incomingPaths = new Set(incoming.console_sources.map(source => pathKey(source.log_path)));
  const previousSources = current?.console_sources ?? [];
  const sources = previousSources.filter(source => options.mergeProcessLogs || incomingPaths.has(pathKey(source.log_path)));
  let changed = sources.length !== previousSources.length;
  const confirmed = new Set<string>();
  for (const candidate of incoming.console_sources) {
    const boundary = options.startupBoundary;
    if (boundary && (candidate.started_at_unix_ms < boundary.startedAtUnixMs
      || boundary.previousLogPaths.has(pathKey(candidate.log_path)))) continue;
    const index = sources.findIndex(source => pathKey(source.log_path) === pathKey(candidate.log_path));
    const previous = sources[index];
    if (previous && olderGeneration(previous, candidate.stream_id, candidate.started_at_unix_ms)) continue;
    const sameStream = previous?.stream_id === candidate.stream_id;
    if (sameStream && previous.revision > candidate.revision) continue;
    if (!candidate.read_error) confirmed.add(pathKey(candidate.log_path));
    if (sameStream && previous.revision === candidate.revision && previous.lines.length >= candidate.lines.length
      && (previous.read_error ?? null) === (candidate.read_error ?? null)) continue;
    const limit = Math.max(1, options.maxLines);
    let lines = candidate.lines;
    if (sameStream) {
      // Source windows end at total_lines. Their positions, rather than their
      // text, identify overlap: consecutive identical log lines are distinct.
      const start = candidate.total_lines - candidate.lines.length;
      const previousStart = previous.total_lines - previous.lines.length;
      if (start > previousStart && start <= previous.total_lines && candidate.total_lines >= previous.total_lines) {
        lines = [...previous.lines.slice(0, start - previousStart), ...candidate.lines];
      }
    }
    const bounded = retainTextTail(lines, limit, line => line);
    const source = { ...candidate, lines: bounded.items, partial_lines: [...candidate.partial_lines],
      truncated: candidate.truncated || bounded.truncated };
    if (index >= 0) sources[index] = source;
    else sources.push(source);
    changed = true;
  }
  const confirmedCurrent = confirmSources(current, confirmed);
  if (!changed) return confirmedCurrent;
  const rows = sources.flatMap(source => source.lines.map(line => ({ stream_id: source.stream_id, line })));
  return materialize(confirmedCurrent, sources, rows, options, options.mergeProcessLogs ? undefined : incoming.source_path);
}

export function runtimeConsoleDisplayLines(snapshot: LogTailSnapshot | null, mergeProcessLogs: boolean, maxLines = 400): string[] {
  if (!snapshot) return [];
  const partial = snapshot.console_sources?.flatMap(source =>
    source.partial_lines.map(line => formatLine(source, line, mergeProcessLogs))) ?? [];
  return partial.length ? [...snapshot.lines, ...partial].slice(-Math.max(1, maxLines)) : snapshot.lines;
}
