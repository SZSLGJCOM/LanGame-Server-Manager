import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
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
}

/** The startup operation owns these small subscriptions, even while its page is
 * unmounted. Completion, failure and view disposal revoke callbacks and history.
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
  const subscriptions = [
    dependencies.listenLog((event) => {
      if (disposed || !runtimeLogEventMatchesStartup(event, boundary)) return;
      const current = history.read(scope);
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
    dependencies.listenReset(() => { if (!disposed) history.clear(scope); })
  ].map(promise => promise.catch((error: unknown) => {
    failed("Runtime startup history subscription failed; retained file reads remain available.", error);
    return null;
  }));
  return {
    ready: Promise.all(subscriptions).then(() => {}),
    dispose() {
      if (disposed) return;
      disposed = true;
      history.clear(scope);
      for (const subscription of subscriptions) {
        void subscription.then(unlisten => unlisten?.()).catch((error: unknown) => {
          failed("Runtime startup history subscription cleanup failed.", error);
        });
      }
    }
  };
}

export function beginRuntimeConsoleStartupCapture(moduleId: string, boundary: RuntimeLogStartupBoundary) {
  if (!isTauri()) return { ready: Promise.resolve(), dispose() {} };
  return createRuntimeConsoleStartupCapture(moduleId, boundary, {
    history: runtimeConsoleHistory,
    listenLog: receive => listen<RuntimeLogStreamEvent>(RUNTIME_LOG_STREAM_EVENT, event => receive(event.payload)),
    listenReset: receive => listen(RUNTIME_SERVICE_EVENTS_RESET, receive),
    failed: (message, error) => console.warn(message, error)
  });
}
