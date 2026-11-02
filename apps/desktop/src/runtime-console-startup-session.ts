import type { LogTailSnapshot, RuntimeConsoleSourceSnapshot, RuntimeStartupSession } from "./types";
import type { RuntimeLogStartupBoundary, RuntimeLogStreamEvent } from "./runtime-log-stream";
import { normalizeRuntimeLogPath, runtimeLogEventMatchesStartup } from "./runtime-log-stream";
import { appendRuntimeConsoleEvent, mergeRuntimeConsoleSnapshot, runtimeConsoleEventNeedsRefresh } from "./runtime-console-stream";

interface StartupPage {
  start: number;
  lines: string[];
  beforeCursor: string | null;
}

export interface RuntimeStartupConsole {
  session: RuntimeStartupSession;
  pages: StartupPage[];
  live: LogTailSnapshot | null;
  expanded: boolean;
}

function retainStartupPages(pages: StartupPage[], expanded: boolean): StartupPage[] {
  if (expanded || !pages.length) return pages;
  const page = pages[pages.length - 1];
  const lines = page.lines.slice(-400);
  return [{ ...page, start: page.start + page.lines.length - lines.length, lines }];
}

export function startupSessionMatchesBoundary(session: RuntimeStartupSession, boundary?: RuntimeLogStartupBoundary | null): boolean {
  return !boundary || (boundary.startupAttemptId ? session.attempt_id === boundary.startupAttemptId
    : session.started_at_unix_ms >= boundary.startedAtUnixMs
      && !boundary.previousLogPaths.has(normalizeRuntimeLogPath(session.source_path)));
}

function sourceDocument(source: RuntimeConsoleSourceSnapshot): LogTailSnapshot {
  return { source_path: source.log_path, lines: source.lines, total_lines: source.total_lines,
    console_sources: [source], console_rows: source.lines.map(line => ({ stream_id: source.stream_id, line })) };
}

export function restoreRuntimeStartupConsole(
  current: RuntimeStartupConsole | null, captured: LogTailSnapshot | null, boundary?: RuntimeLogStartupBoundary | null
): RuntimeStartupConsole | null {
  if (captured?.startup_session) current = mergeRuntimeStartupSession(current, captured.startup_session, { boundary });
  const source = captured?.console_sources?.find(item => item.process_key === "startup");
  if (!source || !boundary || source.started_at_unix_ms < boundary.startedAtUnixMs
    || boundary.previousLogPaths.has(normalizeRuntimeLogPath(source.log_path))) return current;
  const session: RuntimeStartupSession = current?.session ?? { attempt_id: boundary.startupAttemptId ?? `local:${boundary.startedAtUnixMs}`,
    status: "starting", started_at_unix_ms: boundary.startedAtUnixMs, finished_at_unix_ms: null,
    source_path: source.log_path, run_ids: [], error: null, reader_error: null, snapshot: sourceDocument(source),
    console_source: source, before_cursor: null, retained_start_cursor: "0", line_generation: `stream:${source.stream_id}` };
  return mergeRuntimeStartupSession(current, { ...session, snapshot: sourceDocument(source), console_source: source,
    line_generation: `stream:${source.stream_id}` }, { boundary });
}

// Positions are within the receipt's retained file generation. Equal text at
// distinct positions is intentional output, never a deduplication identity.
function mergePage(pages: StartupPage[], incoming: StartupPage): StartupPage[] {
  const result: StartupPage[] = [];
  let merged = { ...incoming, lines: [...incoming.lines] };
  for (const page of pages) {
    const end = page.start + page.lines.length;
    const mergedEnd = merged.start + merged.lines.length;
    if (end < merged.start || page.start > mergedEnd) { result.push(page); continue; }
    const start = Math.min(page.start, merged.start);
    const lines = Array.from({ length: Math.max(end, mergedEnd) - start }, (_, index) => {
      const position = start + index;
      return position >= merged.start && position < mergedEnd
        ? merged.lines[position - merged.start] : page.lines[position - page.start];
    });
    merged = { start, lines, beforeCursor: page.start < merged.start ? page.beforeCursor : merged.beforeCursor };
  }
  result.push(merged);
  return result.sort((a, b) => a.start - b.start);
}

export function mergeRuntimeStartupSession(
  current: RuntimeStartupConsole | null, incoming: RuntimeStartupSession,
  options: { boundary?: RuntimeLogStartupBoundary | null; page?: boolean;
    readGeneration?: string | null; readRetainedStartCursor?: string | null } = {}
): RuntimeStartupConsole | null {
  if (!startupSessionMatchesBoundary(incoming, options.boundary)) return current;
  const sameAttempt = current?.session.attempt_id === incoming.attempt_id;
  if (current && !sameAttempt && (options.page || current.session.started_at_unix_ms > incoming.started_at_unix_ms)) return current;
  if (sameAttempt && "readGeneration" in options
    && (current.session.line_generation !== incoming.line_generation || current.session.retained_start_cursor !== incoming.retained_start_cursor)
    && (options.readGeneration !== current.session.line_generation || options.readRetainedStartCursor !== current.session.retained_start_cursor)) return current;
  const sameGeneration = sameAttempt && current.session.retained_start_cursor === incoming.retained_start_cursor
    && current.session.line_generation === incoming.line_generation;
  let live = sameGeneration ? current.live : null;
  if (incoming.console_source) live = mergeRuntimeConsoleSnapshot(live, sourceDocument(incoming.console_source), { maxLines: 400 });
  // A finished receipt is monotonic even when an initial read resolves late.
  const session = sameAttempt && current.session.status !== "starting" && incoming.status === "starting"
    ? current.session : { ...incoming, snapshot: { ...incoming.snapshot, startup_session: undefined } };
  let pages = sameGeneration ? current.pages : [];
  const snapshot = incoming.snapshot;
  const total = snapshot.total_lines ?? snapshot.lines.length;
  pages = mergePage(pages, { start: Math.max(0, total - snapshot.lines.length), lines: snapshot.lines,
    beforeCursor: incoming.before_cursor });
  if (live?.console_sources?.[0]) {
    const source = live.console_sources[0];
    pages = mergePage(pages, { start: Math.max(0, source.total_lines - source.lines.length), lines: source.lines,
      beforeCursor: incoming.before_cursor });
  }
  const expanded = Boolean(options.page || (sameGeneration && current.expanded));
  return { session, live, pages: retainStartupPages(pages, expanded), expanded };
}

/** Only the independent owned startup producer enters this document. Game
 * stdout/native logs continue to follow their existing selected run and source. */
export function appendRuntimeStartupEvent(
  current: RuntimeStartupConsole | null, event: RuntimeLogStreamEvent, boundary?: RuntimeLogStartupBoundary | null
): RuntimeStartupConsole | null {
  if (event.process_key !== "startup" || !event.console || event.stream_error) return current;
  if (boundary && !runtimeLogEventMatchesStartup(event, boundary)) return current;
  if (!current || (boundary?.startupAttemptId && current.session.attempt_id !== boundary.startupAttemptId)) {
    if (!boundary || !event.console.reset) return current;
    current = { session: { attempt_id: boundary.startupAttemptId ?? `local:${boundary.startedAtUnixMs}`, status: "starting",
      started_at_unix_ms: boundary.startedAtUnixMs, finished_at_unix_ms: null, source_path: event.log_path,
      run_ids: [], error: null, reader_error: null, snapshot: { lines: [], total_lines: 0 },
      console_source: null, before_cursor: null, retained_start_cursor: "0", line_generation: `stream:${event.console.stream_id}` }, pages: [], live: null, expanded: false };
  }
  if (normalizeRuntimeLogPath(current.session.source_path) !== normalizeRuntimeLogPath(event.log_path)) return current;
  const live = appendRuntimeConsoleEvent(current.live, event, { maxLines: 400 });
  if (live === current.live || !live?.console_sources?.[0]) return current;
  const source = live.console_sources[0];
  const lineGeneration = `stream:${source.stream_id}`;
  const sameGeneration = current.session.line_generation === lineGeneration;
  return { ...current, session: { ...current.session, line_generation: lineGeneration }, live,
    expanded: sameGeneration && current.expanded, pages: retainStartupPages(mergePage(sameGeneration ? current.pages : [], {
    start: Math.max(0, source.total_lines - source.lines.length), lines: source.lines, beforeCursor: current.session.before_cursor
  }), sameGeneration && current.expanded) };
}

export function runtimeStartupEventNeedsRefresh(current: RuntimeStartupConsole | null, event: RuntimeLogStreamEvent): boolean {
  return runtimeConsoleEventNeedsRefresh(current?.live ?? null, event);
}

export function runtimeStartupBeforeCursor(current: RuntimeStartupConsole | null): string | null {
  if (!current?.pages.length) return current?.session.before_cursor ?? null;
  // Fill a missing middle window before offering the next older page.
  return current.pages[current.pages.length > 1 ? current.pages.length - 1 : 0].beforeCursor;
}

export function runtimeStartupLines(current: RuntimeStartupConsole | null, gapLabel: string): string[] {
  if (!current) return [];
  const lines = current.pages.flatMap((page, index) => index ? [gapLabel, ...page.lines] : page.lines);
  const partial = current.session.status === "starting" ? current.live?.console_sources?.[0]?.partial_lines ?? [] : [];
  return [...lines, ...partial];
}

export function withoutRuntimeStartupSource(snapshot: LogTailSnapshot | null, startupPath: string | null, mergeProcessLogs = false): LogTailSnapshot | null {
  if (!snapshot || !startupPath) return snapshot;
  const key = normalizeRuntimeLogPath(startupPath);
  if (!snapshot.console_sources) return normalizeRuntimeLogPath(snapshot.source_path) === key ? null : snapshot;
  const sources = snapshot.console_sources.filter(source => normalizeRuntimeLogPath(source.log_path) !== key);
  if (sources.length === snapshot.console_sources.length) return snapshot;
  if (!sources.length) return null;
  const byStream = new Map(sources.map(source => [source.stream_id, source]));
  const rows = snapshot.console_rows?.filter(row => byStream.has(row.stream_id))
    ?? sources.flatMap(source => source.lines.map(line => ({ stream_id: source.stream_id, line })));
  return { ...snapshot, source_path: sources[0].log_path, console_sources: sources, console_rows: rows,
    lines: rows.map(row => {
      const source = byStream.get(row.stream_id);
      return mergeProcessLogs && source?.process_key ? `[${source.process_key}] ${row.line}` : row.line;
    }), total_lines: sources.reduce((count, source) => count + source.total_lines, 0) };
}

export function startupDiagnosticId(source: string, message: string): string {
  let hash = 2166136261;
  for (let index = 0; index < message.length; index++) hash = Math.imul(hash ^ message.charCodeAt(index), 16777619);
  return `ui:${source}:${(hash >>> 0).toString(16)}`;
}
