import { SingleFlightPoller } from "./domain/single-flight-poller";
import type { ArkToolsPreparationResult, ArkToolsStatus } from "./api-ark-tools";

export type ArkToolsInstallPhase = "queued" | "checking" | "connecting" | "downloading" | "verifying" | "installing" | "ready" | "failed";
export interface ArkToolsInstallProgress {
  instanceId: string;
  operationId: string;
  active: boolean;
  phase: ArkToolsInstallPhase;
  source: string | null;
  downloadedBytes: number | null;
  totalBytes: number | null;
  elapsedSeconds: number;
  error: string | null;
}
export interface ArkToolsInstallObservationIssue {
  kind: "progress" | "receipt" | "verification";
  message: string;
}

const phases: ReadonlySet<string> = new Set(["queued", "checking", "connecting", "downloading", "verifying", "installing", "ready", "failed"]);
export function parseArkToolsInstallProgress(value: unknown, instanceId: string, operationId: string | null): ArkToolsInstallProgress | null {
  if (value === null) return null;
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid ARK installation progress.");
  const item = value as Record<string, unknown>;
  const bytes = (input: unknown) => input === null || typeof input === "number" && Number.isSafeInteger(input) && input >= 0;
  if (item.instanceId !== instanceId || typeof item.operationId !== "string" || !/^[A-Za-z0-9_-]{1,128}$/.test(item.operationId)
    || operationId !== null && item.operationId !== operationId || typeof item.active !== "boolean"
    || typeof item.phase !== "string" || !phases.has(item.phase)
    || item.source !== null && (typeof item.source !== "string" || item.source.length > 256)
    || item.error !== null && (typeof item.error !== "string" || item.error.length > 65536)
    || !bytes(item.downloadedBytes) || !bytes(item.totalBytes) || item.totalBytes === 0
    || !Number.isSafeInteger(item.elapsedSeconds) || (item.elapsedSeconds as number) < 0
    || item.active !== (item.phase !== "ready" && item.phase !== "failed")
    || (item.phase === "failed" ? typeof item.error !== "string" || !item.error : item.error !== null)) {
    throw new Error("Invalid ARK installation progress.");
  }
  return item as unknown as ArkToolsInstallProgress;
}

export function queuedArkToolsInstallation(instanceId: string, operationId: string): ArkToolsInstallProgress {
  return { instanceId, operationId, active: true, phase: "queued", source: null,
    downloadedBytes: null, totalBytes: null, elapsedSeconds: 0, error: null };
}

interface InstallObserverOptions<TimerHandle> {
  initial: ArkToolsInstallProgress;
  prepare?: () => Promise<ArkToolsPreparationResult>;
  read: () => Promise<ArkToolsInstallProgress | null>;
  readStatus: () => Promise<ArkToolsStatus>;
  schedule: (callback: () => void, delayMs: number) => TimerHandle;
  cancel: (handle: TimerHandle) => void;
  onProgress: (snapshot: ArkToolsInstallProgress) => void;
  onStatus: (status: ArkToolsStatus) => void;
  onIssue: (issue: ArkToolsInstallObservationIssue | null) => void;
  onSettled: () => void;
}

const errorMessage = (error: unknown) => error instanceof Error ? error.message : String(error);
// These are the runtime transport's explicit outcome-unknown receipts, not a
// generic match on network errors from the installer's download attempts.
function receiptMayBeUnknown(message: string): boolean {
  return message.startsWith("Runtime connection ended after the request was sent (")
    || message.startsWith("Invalid ARK installation completion receipt.")
    || message.startsWith("Runtime response timed out; the operation may still be running")
    || message.startsWith("Sending the runtime request timed out; inspect task state before retrying")
    || message.startsWith("Runtime result exceeds the response size limit; the operation may have completed.")
    || message.startsWith("Runtime service operation exceeded its 30-minute deadline; inspect its task status before retrying");
}

// The observer owns reads only. Disposing it never cancels the native installer.
export function observeArkToolsInstallation<TimerHandle>(options: InstallObserverOptions<TimerHandle>) {
  let latest = options.initial;
  let settled = false;
  let disposed = false;
  let observedNative = !options.prepare;
  let checkingStatus = false;
  let receiptError: { message: string; unknown: boolean; notSent: boolean } | null = null;
  let complete: () => void = () => {};
  const finished = new Promise<void>(resolve => { complete = resolve; });
  function finish(status: ArkToolsStatus | null, error: string | null) {
    if (disposed || settled) return;
    settled = true;
    poller.dispose();
    options.onIssue(null);
    latest = { ...latest, active: false, phase: error ? "failed" : "ready", error };
    options.onProgress(latest);
    if (status) options.onStatus(status);
    options.onSettled();
    complete();
  }
  async function verifyReady() {
    if (checkingStatus || disposed || settled) return;
    checkingStatus = true;
    // The native installer has stopped its clock. Checking the outcome must
    // never turn that completed installation back into a timed active stage.
    options.onProgress(latest);
    try {
      const status = await options.readStatus();
      if (status.installed) finish(status, null);
      else finish(null, status.issue || "ARK extension installation did not pass verification.");
    } catch (error) {
      if (!disposed && !settled) options.onIssue({ kind: "verification", message: errorMessage(error) });
    } finally { checkingStatus = false; }
  }
  function uncertainIssue(detail?: string) {
    options.onIssue({ kind: "receipt", message: [receiptError?.message
      ?? "ARK installation outcome is unconfirmed. Continue checking the existing operation before retrying.", detail].filter(Boolean).join("\n") });
  }
  const poller = new SingleFlightPoller({
    intervalMs: 1000,
    poll: async () => { const observedReceipt = receiptError; return { snapshot: await options.read(), observedReceipt }; },
    schedule: options.schedule, cancel: options.cancel,
    onValue: ({ snapshot, observedReceipt }) => {
      if (!snapshot) {
        // A read started before the rejected receipt cannot establish that its
        // operation was never registered. Wait for the next single-flight read.
        if (receiptError && observedReceipt === receiptError && !receiptError.unknown && !observedNative) {
          finish(null, receiptError.message);
        } else if (receiptError || observedNative) uncertainIssue("ARK installation progress is currently unavailable.");
        return;
      }
      if (snapshot.instanceId !== latest.instanceId || snapshot.operationId !== latest.operationId) return;
      observedNative = true;
      latest = snapshot;
      if (!snapshot.active) {
        if (snapshot.error) finish(null, snapshot.error);
        else void verifyReady();
      } else {
        options.onProgress(snapshot);
        if (receiptError) uncertainIssue();
        else options.onIssue(null);
      }
    },
    onError: error => {
      if (receiptError?.notSent && !observedNative) finish(null, receiptError.message);
      else if (receiptError) uncertainIssue(errorMessage(error));
      else options.onIssue({ kind: "progress", message: errorMessage(error) });
    }
  });
  options.onProgress(latest);
  const prepare = options.prepare;
  if (prepare) void (async () => {
    try {
      const { progress, ...status } = await prepare();
      if (disposed || settled) return;
      latest = progress;
      finish(status, null);
    } catch (error) {
      if (disposed || settled) return;
      const message = errorMessage(error);
      receiptError = { message, unknown: receiptMayBeUnknown(message),
        notSent: message.startsWith("Cannot reach the runtime service: ") && message.endsWith("No operation was sent.") };
      uncertainIssue();
      // This only requests a read; it never repeats prepare. An in-flight read
      // completes first, then the poller schedules a fresh confirmation.
      void poller.pollNow();
    }
  })();
  poller.start();
  return { finished, dispose() { disposed = true; poller.dispose(); complete(); } };
}
