import { beginRuntimeConsoleStartupCapture } from "./runtime-console-startup-history";
import { pendingRuntimeActionForIntent, type RuntimeActionIntent, type RuntimeActionPending } from "./runtime-action-state";
import type { RuntimeLogStartupBoundary } from "./runtime-log-stream";

interface StartupCapture {
  ready: Promise<void>;
  dispose: () => void | Promise<void>;
}

type CaptureStartup = (moduleId: string, boundary: RuntimeLogStartupBoundary) => StartupCapture;

function createStartupAttemptId(): string {
  if (typeof crypto.randomUUID === "function") return crypto.randomUUID();
  // LAN HTTP pages may lack randomUUID, while getRandomValues is still available.
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, byte => byte.toString(16).padStart(2, "0"));
  return `${hex.slice(0, 4).join("")}-${hex.slice(4, 6).join("")}-${hex.slice(6, 8).join("")}-${hex.slice(8, 10).join("")}-${hex.slice(10).join("")}`;
}

export interface RuntimeActionOperation {
  intent: RuntimeActionIntent;
  readonly startupAttemptId?: string;
  boundary?: RuntimeLogStartupBoundary;
  ready: Promise<void>;
  cancelsStartup: boolean;
  localCancellation: boolean;
  readonly submitted: boolean;
  isCurrent: () => boolean;
  markStartDispatched: () => boolean;
  finish: () => void;
}

// Requests own their state and log subscriptions until they settle. A page is
// only a subscriber: navigating away must not release an in-flight operation.
export function createRuntimeActionStore(captureStartup: CaptureStartup = beginRuntimeConsoleStartupCapture) {
  const operations = new Map<string, RuntimeActionOperation>();
  const listeners = new Set<() => void>();
  let snapshot: Readonly<Partial<Record<string, RuntimeActionPending>>> = {};
  const publish = () => listeners.forEach(listener => listener());

  return {
    getSnapshot: () => snapshot,
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
    startupBoundary: (instanceId: string) => operations.get(instanceId)?.boundary,
    begin(instanceId: string, intent: RuntimeActionIntent, options?: {
      startup?: { moduleId: string; boundary: RuntimeLogStartupBoundary };
      startupPending?: boolean;
    }) {
      const previous = operations.get(instanceId);
      if (previous && !(previous.intent === "start" && intent === "stop")) return null;
      const startupAttemptId = intent === "start" ? createStartupAttemptId() : previous?.startupAttemptId;
      const startup = options?.startup ? { ...options.startup,
        boundary: { ...options.startup.boundary, startupAttemptId } } : undefined;
      const capture = intent === "start" && startup ? captureStartup(startup.moduleId, startup.boundary) : null;
      let finished = false;
      let submitted = false;
      const operation: RuntimeActionOperation = {
        intent,
        startupAttemptId,
        boundary: intent === "start" ? startup?.boundary : previous?.boundary,
        ready: capture?.ready ?? Promise.resolve(),
        cancelsStartup: intent === "stop" && (previous?.intent === "start" || options?.startupPending === true),
        localCancellation: intent === "stop" && previous?.intent === "start" && !previous.submitted,
        get submitted() { return submitted; },
        isCurrent: () => !finished && operations.get(instanceId) === operation,
        markStartDispatched() {
          if (intent !== "start" || !operation.isCurrent() || operation.submitted) return false;
          submitted = true;
          return true;
        },
        finish() {
          if (finished) return;
          finished = true;
          // The superseded start still owns its capture until its worker settles.
          void capture?.dispose();
          if (operations.get(instanceId) !== operation) return;
          operations.delete(instanceId);
          const { [instanceId]: _finished, ...remaining } = snapshot;
          snapshot = remaining;
          publish();
        }
      };
      operations.set(instanceId, operation);
      snapshot = { ...snapshot, [instanceId]: operation.cancelsStartup ? "cancelling" : pendingRuntimeActionForIntent(intent) };
      publish();
      return operation;
    }
  };
}

export const runtimeActionStore = createRuntimeActionStore();
