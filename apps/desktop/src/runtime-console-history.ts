import type { LogTailSnapshot } from "./types";
import type { RuntimeLogStreamDiagnostic } from "./runtime-log-stream";

export interface RuntimeConsoleHistorySnapshot extends LogTailSnapshot {
  startup_stream_diagnostic?: RuntimeLogStreamDiagnostic | null;
}

export interface RuntimeConsoleHistoryScope {
  instanceId: string;
  moduleId: string;
  processKey: string | null;
  runId: number | null;
  source: string;
  startupStartedAtUnixMs?: number;
}

export function runtimeConsoleStartupHistoryScope(
  moduleId: string,
  boundary: { instanceId: string; startedAtUnixMs: number }
): RuntimeConsoleHistoryScope {
  return { instanceId: boundary.instanceId, moduleId, processKey: null, runId: null,
    source: "startup", startupStartedAtUnixMs: boundary.startedAtUnixMs };
}

const MAX_LINES = 400;
const MAX_ENTRIES = 64;
const MAX_BYTES = 4 * 1024 * 1024;

/** Session memory for mounted and revisited consoles, never a durable log store.
 * Only owned documents and operation-owned startup aggregates are cached;
 * short health polls cannot seed history.
 */
export function createRuntimeConsoleHistoryCache() {
  const entries = new Map<string, { instanceId: string; snapshot: RuntimeConsoleHistorySnapshot; bytes: number }>();
  let bytes = 0;
  const key = (scope: RuntimeConsoleHistoryScope) => {
    if (scope.startupStartedAtUnixMs !== undefined) {
      return scope.runId === null && scope.processKey === null && scope.source === "startup"
        && Number.isSafeInteger(scope.startupStartedAtUnixMs) && scope.startupStartedAtUnixMs > 0
        ? JSON.stringify([scope.instanceId, scope.moduleId, "startup", scope.startupStartedAtUnixMs]) : null;
    }
    return scope.runId !== null && Number.isSafeInteger(scope.runId) && scope.runId > 0
      ? JSON.stringify([scope.instanceId, scope.moduleId, scope.processKey, scope.runId, scope.source]) : null;
  };
  const remove = (entryKey: string) => {
    const entry = entries.get(entryKey);
    if (entry) bytes -= entry.bytes;
    entries.delete(entryKey);
  };
  const copy = (snapshot: RuntimeConsoleHistorySnapshot): RuntimeConsoleHistorySnapshot => ({ ...snapshot, lines: [...snapshot.lines],
    ...(snapshot.startup_stream_diagnostic ? { startup_stream_diagnostic: { ...snapshot.startup_stream_diagnostic } } : {}) });

  return {
    read(scope: RuntimeConsoleHistoryScope): RuntimeConsoleHistorySnapshot | null {
      const entryKey = key(scope);
      const entry = entryKey ? entries.get(entryKey) : null;
      if (!entry || !entryKey) return null;
      entries.delete(entryKey);
      entries.set(entryKey, entry);
      const restored = copy(entry.snapshot);
      // A service may have restarted while no console was mounted. Restored
      // rows are a display fallback; only this mount's reads/events own revision
      // ordering, so the first current document must be able to replace them.
      // Startup captures remain subscribed for the operation's entire lifetime
      // and clear on service reset, so their current revisions remain ordered.
      if (scope.startupStartedAtUnixMs === undefined) delete restored.snapshot_revision;
      return restored;
    },
    write(scope: RuntimeConsoleHistoryScope, snapshot: RuntimeConsoleHistorySnapshot) {
      const entryKey = key(scope);
      if (!entryKey || snapshot.read_error || (!snapshot.source_path
        && !(scope.startupStartedAtUnixMs !== undefined && snapshot.startup_stream_diagnostic))) return;
      const retained = { ...snapshot, lines: snapshot.lines.slice(-MAX_LINES),
        ...(snapshot.startup_stream_diagnostic ? { startup_stream_diagnostic: { ...snapshot.startup_stream_diagnostic } } : {}),
        truncated: Boolean(snapshot.truncated) || snapshot.lines.length > MAX_LINES };
      const entryBytes = (JSON.stringify(retained).length + entryKey.length) * 2;
      remove(entryKey);
      if (entryBytes > MAX_BYTES) return;
      entries.set(entryKey, { instanceId: scope.instanceId, snapshot: retained, bytes: entryBytes });
      bytes += entryBytes;
      while (entries.size > MAX_ENTRIES || bytes > MAX_BYTES) {
        const oldest = entries.keys().next().value;
        if (oldest === undefined) break;
        remove(oldest);
      }
    },
    clearInstance(instanceId: string) {
      for (const [entryKey, entry] of entries) if (entry.instanceId === instanceId) remove(entryKey);
    },
    clear(scope: RuntimeConsoleHistoryScope) {
      const entryKey = key(scope);
      if (entryKey) remove(entryKey);
    }
  };
}

export const runtimeConsoleHistory = createRuntimeConsoleHistoryCache();
