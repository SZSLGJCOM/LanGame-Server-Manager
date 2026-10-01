import { useEffect, useRef, useState, type Dispatch, type SetStateAction } from "react";
import { cancelSteamCmdPreparation, ensureSteamCmdReady, probeSteamCmdStatus, readSteamCmdPrepareProgress, uninstallSteamCmd } from "../api";
import { message, type UiMessage } from "../app-ui";
import { steamCmdDetailMessage, steamCmdSummaryMessage } from "../steamcmd-ui";
import { resumeSteamCmdPreparation, startSteamCmdPreparation } from "../steamcmd-prepare-operation";
import type { SteamCmdPrepareSnapshot, SteamCmdStatus } from "../types";

type Setter<T> = Dispatch<SetStateAction<T>>;
export interface SteamCmdActionOptions {
  setSteamCmdBusy: (busy: boolean) => void;
  setSteamCmdMessage: Setter<UiMessage>;
  setSteamCmdStatus: Setter<SteamCmdStatus | null>;
  setSteamCmdProgress: Setter<SteamCmdPrepareSnapshot | null>;
  setActivity: Setter<UiMessage>;
  onSteamCmdOperationStart: () => void;
}

function diagnostic(error: unknown): string {
  return error instanceof Error ? error.message : typeof error === "string" ? error : JSON.stringify(error);
}

export function useSteamCmdActions(options: SteamCmdActionOptions) {
  const runningRef = useRef(false);
  const mountedRef = useRef(true);
  const operationRef = useRef<ReturnType<typeof startSteamCmdPreparation> | null>(null);
  const progressRef = useRef<SteamCmdPrepareSnapshot | null>(null);
  const stopPendingRef = useRef<string | null>(null);
  const [steamCmdStopPending, setSteamCmdStopPending] = useState(false);
  const [steamCmdStopError, setSteamCmdStopError] = useState<string | null>(null);
  useEffect(() => {
    mountedRef.current = true;
    const recovery = resumeSteamCmdPreparation({
      ...operationCallbacks(),
      probe: probeSteamCmdStatus,
      onRecovered: () => {
        runningRef.current = true;
        options.onSteamCmdOperationStart();
        options.setSteamCmdBusy(true);
      }
    });
    operationRef.current = recovery;
    void recovery.finished.then(() => {
      if (operationRef.current === recovery) operationRef.current = null;
    });
    return () => {
      mountedRef.current = false;
      runningRef.current = false;
      operationRef.current?.dispose();
    };
  }, []);

  function operationCallbacks() {
    return {
      read: readSteamCmdPrepareProgress,
      schedule: (callback: () => void, delay: number) => window.setTimeout(callback, delay),
      cancel: (handle: number) => window.clearTimeout(handle),
      onProgress: (snapshot: SteamCmdPrepareSnapshot) => {
        progressRef.current = snapshot;
        if (!snapshot.active) {
          stopPendingRef.current = null;
          setSteamCmdStopPending(false);
          setSteamCmdStopError(null);
        }
        if (snapshot.error || snapshot.cancelled) {
          options.setSteamCmdStatus((current) => current ? { ...current, ready: false } : current);
        }
        options.setSteamCmdProgress(snapshot);
        const progress = snapshot.cancelled ? message("steamcmd.cancelled") : snapshot.error
          ? message("activity.steamCmdFailed", { message: snapshot.error })
          : snapshot.cancel_requested ? message("steamcmd.stopping")
          : message(`steamcmd.prepare.${snapshot.phase}`);
        options.setSteamCmdMessage(progress);
        options.setActivity(progress);
      },
      onStatus: (status: SteamCmdStatus) => {
        options.setSteamCmdStatus(status);
        options.setSteamCmdMessage(steamCmdDetailMessage(status));
        options.setActivity(steamCmdSummaryMessage(status));
      },
      onError: (error: unknown) => {
        const failure = message("activity.steamCmdFailed", { message: diagnostic(error) });
        options.setSteamCmdMessage(failure);
        options.setActivity(failure);
      },
      onReadError: (error: unknown) => {
        const warning = message("steamcmd.progressUnavailable", { message: diagnostic(error) });
        options.setSteamCmdMessage(warning);
        options.setActivity(warning);
      },
      onSettled: () => { runningRef.current = false; options.setSteamCmdBusy(false); }
    };
  }

  async function handleEnsureSteamCmd() {
    if (runningRef.current) return;
    operationRef.current?.dispose();
    runningRef.current = true;
    options.onSteamCmdOperationStart();
    options.setSteamCmdBusy(true);
    setSteamCmdStopError(null);
    const operation = startSteamCmdPreparation({
      ...operationCallbacks(),
      operationId: typeof crypto.randomUUID === "function" ? crypto.randomUUID() : `steamcmd-${Date.now()}-${crypto.getRandomValues(new Uint32Array(2)).join("-")}`,
      ensure: ensureSteamCmdReady
    });
    operationRef.current = operation;
    await operation.finished;
    if (operationRef.current === operation) operationRef.current = null;
  }

  async function handleCancelSteamCmd(operationId: string) {
    const snapshot = progressRef.current;
    if (!snapshot?.active || !snapshot.cancellable || snapshot.operation_id !== operationId
      || snapshot.cancel_requested || stopPendingRef.current === operationId) return;
    stopPendingRef.current = operationId;
    setSteamCmdStopPending(true);
    setSteamCmdStopError(null);
    try {
      await cancelSteamCmdPreparation(operationId);
      // The preparation reader owns completion. Acknowledgement never releases busy state.
    } catch (error) {
      if (!mountedRef.current || progressRef.current?.operation_id !== operationId || !progressRef.current.active) return;
      stopPendingRef.current = null;
      setSteamCmdStopPending(false);
      setSteamCmdStopError(diagnostic(error));
    }
  }

  async function handleUninstallSteamCmd() {
    if (runningRef.current) return;
    operationRef.current?.dispose();
    runningRef.current = true;
    options.onSteamCmdOperationStart();
    options.setSteamCmdProgress(null);
    options.setSteamCmdBusy(true);
    options.setSteamCmdMessage(message("activity.uninstallingSteamCmd"));
    options.setActivity(message("activity.uninstallingSteamCmd"));
    try {
      const status = await uninstallSteamCmd();
      if (!mountedRef.current) return;
      options.setSteamCmdStatus(status);
      options.setSteamCmdMessage(steamCmdDetailMessage(status));
      options.setActivity(steamCmdSummaryMessage(status));
    } catch (error) {
      if (!mountedRef.current) return;
      const failure = message("activity.steamCmdUninstallFailed", { message: diagnostic(error) });
      options.setSteamCmdMessage(failure);
      options.setActivity(failure);
    } finally {
      runningRef.current = false;
      if (mountedRef.current) options.setSteamCmdBusy(false);
    }
  }
  return { handleEnsureSteamCmd, handleUninstallSteamCmd, handleCancelSteamCmd, steamCmdStopPending, steamCmdStopError };
}
