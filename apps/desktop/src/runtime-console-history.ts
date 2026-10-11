import type { LogTailSnapshot } from "./types";

export interface RuntimeConsoleHistoryScope {
  instanceId: string;
  moduleId: string;
  processKey: string | null;
  runId: number | null;
  source: string;
}

const MAX_LINES = 400;
const MAX_ENTRIES = 64;
const MAX_BYTES = 4 * 1024 * 1024;

/** Session memory for mounted and revisited consoles, never a durable log store.
 * Only an owned full document is cached; short health polls cannot seed history.
 */
export function createRuntimeConsoleHistoryCache() {
  const entries = new Map<string, { instanceId: string; snapshot: LogTailSnapshot; bytes: number }>();
  let bytes = 0;
  const key = (scope: RuntimeConsoleHistoryScope) => scope.runId !== null
    && Number.isSafeInteger(scope.runId) && scope.runId > 0
    ? JSON.stringify([scope.instanceId, scope.moduleId, scope.processKey, scope.runId, scope.source]) : null;
  const remove = (entryKey: string) => {
    const entry = entries.get(entryKey);
    if (entry) bytes -= entry.bytes;
    entries.delete(entryKey);
  };
  const copy = (snapshot: LogTailSnapshot): LogTailSnapshot => ({ ...snapshot, lines: [...snapshot.lines] });

  return {
    read(scope: RuntimeConsoleHistoryScope): LogTailSnapshot | null {
      const entryKey = key(scope);
      const entry = entryKey ? entries.get(entryKey) : null;
      if (!entry || !entryKey) return null;
      entries.delete(entryKey);
      entries.set(entryKey, entry);
      const restored = copy(entry.snapshot);
      // A service may have restarted while no console was mounted. Restored
      // rows are a display fallback; only this mount's reads/events own revision
      // ordering, so the first current document must be able to replace them.
      delete restored.snapshot_revision;
      return restored;
    },
    write(scope: RuntimeConsoleHistoryScope, snapshot: LogTailSnapshot) {
      const entryKey = key(scope);
      if (!entryKey || !snapshot.source_path || snapshot.read_error) return;
      const retained = { ...snapshot, lines: snapshot.lines.slice(-MAX_LINES),
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
    }
  };
}

export const runtimeConsoleHistory = createRuntimeConsoleHistoryCache();
