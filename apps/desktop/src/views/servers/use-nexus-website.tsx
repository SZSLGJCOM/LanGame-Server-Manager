import { Channel, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { useDownloadMetrics } from "../../hooks/useDownloadMetrics";
import { NexusApiError, nexusStageResult, requestNexusCommand } from "../../nexus-mods-api";

type Phase = "idle" | "opening" | "selecting" | "waiting" | "verifying" | "installing";
type TransferProgress = { instanceId: string; operationId: string; attempt: number; sequence: number;
  phase: "downloading" | "verifying" | "installing"; receivedBytes: number; totalBytes: number | null; elapsedSeconds: number };
type Request = { sessionId: string | null; cancelled: boolean; phase: Phase; closing?: Promise<unknown>; transfer?: TransferProgress };
const transferPhases = ["downloading", "verifying", "installing"];
function readTransfer(value: unknown): TransferProgress | null {
  if (!value || typeof value !== "object") return null;
  const entry = value as Record<string, unknown>;
  const count = (value: unknown): value is number => typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
  if (typeof entry.instanceId !== "string" || typeof entry.operationId !== "string"
    || typeof entry.phase !== "string" || !transferPhases.includes(entry.phase)
    || !count(entry.attempt) || entry.attempt === 0 || !count(entry.sequence) || entry.sequence === 0
    || !count(entry.receivedBytes) || !count(entry.elapsedSeconds)
    || (entry.totalBytes !== null && (!count(entry.totalBytes) || entry.totalBytes < entry.receivedBytes))) return null;
  return { instanceId: entry.instanceId, operationId: entry.operationId, attempt: entry.attempt, sequence: entry.sequence,
    phase: entry.phase as TransferProgress["phase"], receivedBytes: entry.receivedBytes,
    totalBytes: entry.totalBytes as number | null, elapsedSeconds: entry.elapsedSeconds };
}
// Background website downloads need the native host, never browser preview data.
export const canUseNexusWebsite = isTauri;

export function useNexusWebsite(instanceId: string) {
  const active = useRef<Request | null>(null);
  const generation = useRef(0);
  const [phase, setPhase] = useState<Phase>("idle");
  const [transfer, setTransfer] = useState<(TransferProgress & { observedAt: number }) | null>(null);
  const metrics = useDownloadMetrics({ operation_id: transfer ? `${transfer.operationId}:${transfer.attempt}` : "",
    active: Boolean(transfer), phase: transfer?.phase ?? "idle", downloaded_bytes: transfer?.receivedBytes ?? null,
    total_bytes: transfer?.totalBytes ?? null, elapsed_seconds: transfer?.elapsedSeconds ?? 0 }, transfer?.observedAt);
  const close = useCallback(async (request: Request) => {
    if (request.sessionId) await (request.closing ??= requestNexusCommand("close_nexus_website_host", { sessionId: request.sessionId }));
  }, []);
  const cancel = useCallback(() => {
    const request = active.current;
    // A selected website download belongs to the runtime until publication or rollback.
    if (!request || request.phase === "installing") return;
    generation.current++;
    request.cancelled = true;
    void close(request).catch((error: unknown) => console.warn("Could not close the Nexus background session", error));
  }, [close]);
  const detach = useCallback(() => {
    generation.current++;
    cancel();
    active.current = null;
    setPhase("idle");
    setTransfer(null);
  }, [cancel]);
  // Tab changes stay within this instance's operation. Leaving the instance
  // cancels preparation; a download already owned by the runtime may finish.
  useLayoutEffect(() => detach, [instanceId, detach]);
  const installAttempt = useCallback(async (reference: string, release?: { closed: boolean; owned: boolean }) => {
    if (!canUseNexusWebsite()) throw new NexusApiError("nexus_local_desktop_required");
    if (active.current) throw new NexusApiError("nexus_browser_busy");
    const request: Request = { sessionId: null, cancelled: false, phase: "opening" };
    active.current = request;
    setPhase("opening");
    setTransfer(null);
    let unlisten: UnlistenFn | undefined;
    try {
      const sessionId = await requestNexusCommand("open_nexus_website_host");
      if (typeof sessionId !== "string" || !sessionId) throw new NexusApiError("nexus_invalid_response");
      request.sessionId = sessionId;
      if (request.cancelled || active.current !== request) throw new NexusApiError("nexus_browser_cancelled");
      try {
        unlisten = await listen<unknown>("nexus-website-progress", ({ payload }) => {
          if (active.current !== request || request.cancelled) return;
          const next = readTransfer(payload), previous = request.transfer;
          if (!next || next.instanceId !== instanceId || next.operationId !== sessionId
            || (previous && (next.sequence <= previous.sequence || next.attempt < previous.attempt
              || next.elapsedSeconds < previous.elapsedSeconds
              || (previous.phase !== "downloading" && next.attempt > previous.attempt)
              || (next.attempt === previous.attempt && (next.receivedBytes < previous.receivedBytes
                || transferPhases.indexOf(next.phase) < transferPhases.indexOf(previous.phase)))))) return;
          request.transfer = next;
          // A runtime byte event confirms the same ownership barrier as the
          // native phase channel; transfer stages never re-enable cancellation.
          request.phase = "installing";
          setPhase("installing");
          setTransfer({ ...next, observedAt: performance.now() });
        });
      } catch {
        console.warn("Could not observe Nexus download progress");
      }
      if (request.cancelled || active.current !== request) throw new NexusApiError("nexus_browser_cancelled");
      const onProgress = new Channel<string>((value) => {
        if (active.current !== request || request.cancelled || request.phase === "installing"
          || !["opening", "selecting", "waiting", "verifying", "installing"].includes(value)) return;
        request.phase = value as Phase;
        setPhase(request.phase);
      });
      const result = await requestNexusCommand("install_nexus_website_mod", { sessionId, instanceId, reference, onProgress });
      if (request.cancelled) throw new NexusApiError("nexus_browser_cancelled");
      return nexusStageResult(instanceId, result);
    } finally {
      if (release) release.owned = request.phase === "installing";
      try { await unlisten?.(); } catch { console.warn("Could not stop observing Nexus download progress"); }
      await close(request).then(() => { if (release) release.closed = true; })
        .catch((error: unknown) => console.warn("Could not close the Nexus background session", error));
      if (active.current === request) { active.current = null; setPhase("idle"); setTransfer(null); }
    }
  }, [instanceId, close]);
  const install = useCallback(async (reference: string) => {
    const currentGeneration = generation.current;
    const release = { closed: false, owned: false };
    try {
      return await installAttempt(reference, release);
    } catch (error) {
      // A cold website verification can stall while the next ordinary page
      // load succeeds. Retry preparation once, after its host is fully closed.
      // Never repeat an accepted download, cancellation, or another instance's work.
      if (!(error instanceof NexusApiError) || error.code !== "nexus_website_verification_required"
        || !release.closed || release.owned || generation.current !== currentGeneration || active.current) throw error;
      return installAttempt(reference);
    }
  }, [installAttempt]);
  return { install, cancel, phase, transfer, metrics };
}
