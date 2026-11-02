import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { readInstanceLogDocument, recordStartupSessionDiagnostic } from "./api";
import type { LogTailSnapshot } from "./types";
import { createRuntimeLogRefresh } from "./runtime-log-refresh";
import { startupDiagnosticId, startupSessionMatchesBoundary } from "./runtime-console-startup-session";
import { markRuntimeConsoleSourceForRefresh, mergeRuntimeConsoleSnapshot, runtimeConsoleEventNeedsRefresh } from "./runtime-console-stream";
import {
  runtimeConsoleHistory,
  runtimeConsoleStartupHistoryScope,
  type createRuntimeConsoleHistoryCache
} from "./runtime-console-history";
import {
  appendRuntimeLogStreamEvent,
  retainRuntimeLogStreamDiagnostic,
  runtimeLogEventMatchesStartup,
  RUNTIME_LOG_STREAM_EVENT,
  RUNTIME_SERVICE_EVENTS_RESET,
  type RuntimeLogStartupBoundary,
  type RuntimeLogStreamEvent
} from "./runtime-log-stream";

interface StartupConsoleHistoryDependencies {
  history: ReturnType<typeof createRuntimeConsoleHistoryCache>;
  listenLog: (receive: (event: RuntimeLogStreamEvent) => void) => Promise<() => void>;
  listenReset: (receive: () => void) => Promise<() => void>;
  failed: (message: string, error: unknown) => void;
  readLog?: () => Promise<LogTailSnapshot>;
  recordDiagnostic?: (diagnosticId: string, message: string) => Promise<void>;
}

/** The startup operation owns these small subscriptions, even while its page is
 * unmounted. Completion revokes callbacks, then retains the final owned read.
 * The attempt-scoped key prevents a later startup from adopting this history.
 */
export function createRuntimeConsoleStartupCapture(
  moduleId: string,
  boundary: RuntimeLogStartupBoundary,
  dependencies: StartupConsoleHistoryDependencies
) {
  const { history, failed } = dependencies;
  const scope = runtimeConsoleStartupHistoryScope(moduleId, boundary);
  history.clear(scope);
  let disposed = false;
  let recoveryFailed = false;
  let recoveryGeneration = 0;
  let ownedSessionKnown = false;
  const pendingDiagnostics = new Map<string, string>();
  const recordedDiagnostics = new Set<string>();
  const activeFailureIds = new Map<string, string>();
  let failureSequence = 0;
  const flushDiagnostics = () => {
    if (!ownedSessionKnown || !dependencies.recordDiagnostic) return;
    for (const [id, message] of pendingDiagnostics) {
      if (recordedDiagnostics.has(id)) continue;
      recordedDiagnostics.add(id);
      void dependencies.recordDiagnostic(id, message).then(() => pendingDiagnostics.delete(id)).catch(error => {
        recordedDiagnostics.delete(id);
        failed("Runtime startup diagnostic could not be retained.", error);
      });
    }
  };
  const rememberFailure = (source: string, error: unknown) => {
    const message = error instanceof Error ? error.message : String(error);
    const key = startupDiagnosticId(source, message);
    const id = activeFailureIds.get(key) ?? `${key}:${Date.now()}:${++failureSequence}`;
    activeFailureIds.set(key, id);
    pendingDiagnostics.set(id, `[${source}] ${message}`);
    flushDiagnostics();
  };
  const retainRead = (incoming: LogTailSnapshot) => {
    const session = incoming.startup_session;
    if (session && !startupSessionMatchesBoundary(session, boundary)) return;
    if (session) { ownedSessionKnown = true; flushDiagnostics(); }
    const current = history.read(scope);
    const sources = [...(incoming.console_sources ?? []), ...(session?.console_source ? [session.console_source] : [])];
    const next = mergeRuntimeConsoleSnapshot(current, { ...incoming, console_sources: sources }, { maxLines: 400,
      mergeProcessLogs: true, startupBoundary: boundary });
    const retained = next ?? (session ? { ...session.snapshot, source_path: session.source_path } : null);
    if (retained && (retained !== current || recoveryFailed || session)) history.write(scope, {
      ...retained, ...(session ? { startup_session: session } : {}),
      startup_stream_diagnostic: recoveryFailed ? null : current?.startup_stream_diagnostic
    });
    recoveryFailed = false;
    activeFailureIds.clear();
  };
  const createRecovery = () => dependencies.readLog ? createRuntimeLogRefresh({
    read: async () => {
      const generation = recoveryGeneration;
      const incoming = await dependencies.readLog?.();
      return { incoming, generation };
    },
    publish: ({ incoming, generation }) => {
      if (!incoming || generation !== recoveryGeneration) return;
      retainRead(incoming);
    },
    failed: error => {
      failed("Runtime startup console recovery failed.", error);
      rememberFailure("startup-read", error);
      recoveryFailed = true;
      const current = history.read(scope);
      history.write(scope, { ...(current ?? { source_path: null, lines: [] }), startup_stream_diagnostic: {
        instanceId: boundary.instanceId, processKey: null, runId: null, logPath: "",
        message: error instanceof Error ? error.message : String(error)
      } });
    },
    loading: () => {}
  }) : null;
  let refresh = createRecovery();
  const subscriptions = [
    dependencies.listenLog((event) => {
      if (disposed || !runtimeLogEventMatchesStartup(event, boundary)) return;
      if (event.snapshot || event.document_patch) return;
      if (event.process_key === "startup") { ownedSessionKnown = true; flushDiagnostics(); }
      const current = history.read(scope);
      if (typeof event.stream_error === "string") {
        recoveryGeneration++;
        const marked = markRuntimeConsoleSourceForRefresh(current, event.log_path);
        history.write(scope, { ...(marked ?? { source_path: null, lines: [] }),
          startup_stream_diagnostic: retainRuntimeLogStreamDiagnostic(current?.startup_stream_diagnostic ?? null, event) });
        refresh?.request();
        return;
      }
      if (runtimeConsoleEventNeedsRefresh(current, event)) {
        refresh?.request();
        return;
      }
      const next = appendRuntimeLogStreamEvent(current, event, {
        instanceId: boundary.instanceId, startupBoundary: boundary,
        followLatestPath: true, mergeProcessLogs: true, maxLines: 400
      });
      const diagnostic = retainRuntimeLogStreamDiagnostic(current?.startup_stream_diagnostic ?? null, event);
      if (next !== current || diagnostic !== (current?.startup_stream_diagnostic ?? null)) {
        history.write(scope, { ...(next ?? { source_path: null, lines: [], total_lines: 0, truncated: false, read_error: null }),
          startup_stream_diagnostic: diagnostic });
      }
    }),
    dependencies.listenReset(() => {
      if (disposed) return;
      refresh?.dispose();
      recoveryFailed = false;
      history.clear(scope);
      refresh = createRecovery();
      refresh?.request();
    })
  ].map(promise => promise.catch((error: unknown) => {
    failed("Runtime startup history subscription failed; retained file reads remain available.", error);
    rememberFailure("startup-subscription", error);
    return null;
  }));
  return {
    ready: Promise.all(subscriptions).then(() => { if (!disposed) refresh?.request(); }),
    async dispose() {
      if (disposed) return;
      disposed = true;
      refresh?.dispose();
      for (const subscription of subscriptions) {
        void subscription.then(unlisten => unlisten?.()).catch((error: unknown) => {
          failed("Runtime startup history subscription cleanup failed.", error);
        });
      }
      // This read is independent of the cancelled refresh. Its receipt is
      // checked against the attempt, so a new operation cannot be overwritten.
      if (dependencies.readLog) {
        try { retainRead(await dependencies.readLog()); }
        catch (error) {
          rememberFailure("startup-final-read", error);
          failed("Runtime startup final read failed; the retained session remains available for retry.", error);
        }
      }
    }
  };
}

export function beginRuntimeConsoleStartupCapture(moduleId: string, boundary: RuntimeLogStartupBoundary) {
  if (!isTauri()) return { ready: Promise.resolve(), dispose() {} };
  const attemptId = boundary.startupAttemptId;
  return createRuntimeConsoleStartupCapture(moduleId, boundary, {
    history: runtimeConsoleHistory,
    listenLog: receive => listen<RuntimeLogStreamEvent>(RUNTIME_LOG_STREAM_EVENT, event => receive(event.payload)),
    listenReset: receive => listen(RUNTIME_SERVICE_EVENTS_RESET, receive),
    readLog: () => readInstanceLogDocument(boundary.instanceId, 400),
    recordDiagnostic: attemptId ? (id, message) => recordStartupSessionDiagnostic(boundary.instanceId, attemptId, id, message) : undefined,
    failed: (message, error) => console.warn(message, error)
  });
}
