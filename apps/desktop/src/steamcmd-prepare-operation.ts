import { SingleFlightPoller } from "./domain/single-flight-poller";
import { isInstallationCancelled } from "./installation-cancellation";
import type { SteamCmdPrepareSnapshot, SteamCmdStatus } from "./types";

export function queuedSteamCmdProgress(operationId: string): SteamCmdPrepareSnapshot {
  return {
    operation_id: operationId, active: true, phase: "queued", detail: "",
    cancellable: false, cancel_requested: false, cancelled: false,
    downloaded_bytes: null, total_bytes: null, output_excerpt: "",
    elapsed_seconds: 0, idle_seconds: 0, error: null
  };
}

interface PrepareOperationOptions<TimerHandle> {
  operationId: string;
  ensure: (operationId: string) => Promise<SteamCmdStatus>;
  read: (operationId: string) => Promise<SteamCmdPrepareSnapshot | null>;
  schedule: (callback: () => void, delayMs: number) => TimerHandle;
  cancel: (handle: TimerHandle) => void;
  onProgress: (snapshot: SteamCmdPrepareSnapshot) => void;
  onStatus: (status: SteamCmdStatus) => void;
  onError: (error: unknown) => void;
  onReadError: (error: unknown) => void;
  onSettled: () => void;
}

// A request owns its reader until completion or unmount. Disposing never cancels installation.
export function startSteamCmdPreparation<TimerHandle>(options: PrepareOperationOptions<TimerHandle>) {
  let disposed = false;
  let latest = queuedSteamCmdProgress(options.operationId);
  const poller = new SingleFlightPoller({
    intervalMs: 1000,
    poll: () => options.read(options.operationId),
    schedule: options.schedule,
    cancel: options.cancel,
    onValue: (snapshot: SteamCmdPrepareSnapshot | null) => {
      if (!snapshot || snapshot.operation_id !== options.operationId) return;
      latest = snapshot;
      options.onProgress(snapshot);
      if (!snapshot.active) poller.dispose();
    },
    onError: options.onReadError
  });

  options.onProgress(latest);
  const finished = (async () => {
    try {
      const ready = options.ensure(options.operationId);
      poller.start();
      const status = await ready;
      if (disposed) return;
      options.onProgress({ ...latest, active: false, phase: "ready", error: null, cancellable: false, cancel_requested: false, cancelled: false });
      options.onStatus(status);
    } catch (error) {
      if (disposed) return;
      if (isInstallationCancelled(error)) {
        options.onProgress({ ...latest, active: false, cancellable: false, cancel_requested: true, cancelled: true, error: null });
        return;
      }
      const detail = error instanceof Error ? error.message : typeof error === "string" ? error : JSON.stringify(error);
      options.onProgress({ ...latest, active: false, cancellable: false, error: detail });
      options.onError(error);
    } finally {
      poller.dispose();
      if (!disposed) options.onSettled();
    }
  })();

  return {
    finished,
    dispose() { disposed = true; poller.dispose(); }
  };
}

interface ResumeOperationOptions<TimerHandle> extends Omit<PrepareOperationOptions<TimerHandle>, "operationId" | "ensure" | "read"> {
  read: (operationId: string | null) => Promise<SteamCmdPrepareSnapshot | null>;
  probe: () => Promise<SteamCmdStatus>;
  onRecovered: () => void;
}

// Recovery observes the backend's existing operation; it never starts another installation.
export function resumeSteamCmdPreparation<TimerHandle>(options: ResumeOperationOptions<TimerHandle>) {
  let disposed = false;
  let recovered = false;
  let poller: SingleFlightPoller<SteamCmdPrepareSnapshot | null, TimerHandle> | null = null;
  let resolveTerminal: ((snapshot: SteamCmdPrepareSnapshot | null) => void) | null = null;
  const finished = (async () => {
    try {
      const current = await options.read(null);
      if (disposed || !current?.active) return;
      recovered = true;
      options.onRecovered();
      options.onProgress(current);
      let latest = current;
      const terminalResult = new Promise<SteamCmdPrepareSnapshot | null>((resolve) => { resolveTerminal = resolve; });
      poller = new SingleFlightPoller({
        intervalMs: 1000,
        poll: () => options.read(current.operation_id),
        schedule: options.schedule,
        cancel: options.cancel,
        onValue: (snapshot) => {
          if (snapshot && snapshot.operation_id !== current.operation_id) return;
          latest = snapshot ?? {
            ...latest,
            active: false,
            error: JSON.stringify({
              code: "steamcmd_prepare_progress_lost",
              message: "SteamCMD preparation progress is no longer available."
            })
          };
          options.onProgress(latest);
          if (!latest.active) { poller?.dispose(); resolveTerminal?.(latest); }
        },
        onError: options.onReadError
      });
      poller.start();
      const terminal = await terminalResult;
      if (disposed || !terminal) return;
      if (terminal.cancelled) return;
      if (terminal.error) {
        options.onError(terminal.error);
      } else {
        const status = await options.probe();
        if (!disposed) options.onStatus(status);
      }
    } catch (error) {
      if (!disposed) options.onError(error);
    } finally {
      poller?.dispose();
      if (!disposed && recovered) options.onSettled();
    }
  })();
  return {
    finished,
    dispose() {
      disposed = true;
      poller?.dispose();
      resolveTerminal?.(null);
    }
  };
}
