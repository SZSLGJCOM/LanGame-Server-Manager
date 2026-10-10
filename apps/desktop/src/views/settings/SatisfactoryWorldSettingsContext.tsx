import { createContext, useCallback, useContext, useEffect, useRef, useState } from "react";
import { readSatisfactoryWorldSettings, type SatisfactoryWorldOperationResult, type SatisfactoryWorldSnapshot } from "../../satisfactory-world-settings";
import type { ConfigurationWorkspaceProviderProps } from "./module-types";

interface NativeWorldState {
  snapshot: SatisfactoryWorldSnapshot | null;
  loading: boolean;
  busy: boolean;
  pending: boolean;
  error: string | null;
  result: "saved" | "accepted" | "loaded" | null;
  refresh(): Promise<void>;
  getSnapshot(): SatisfactoryWorldSnapshot | null;
  mutate(request: (snapshot: SatisfactoryWorldSnapshot | null) => Promise<SatisfactoryWorldSnapshot | SatisfactoryWorldOperationResult>): Promise<boolean>;
}
const NativeWorldContext = createContext<NativeWorldState | null>(null);

export function SatisfactoryWorldSettingsProvider({ children, details }: ConfigurationWorkspaceProviderProps) {
  const instanceId = details.summary.id;
  const [snapshot, setSnapshot] = useState<SatisfactoryWorldSnapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<NativeWorldState["result"]>(null);
  const generation = useRef(0);
  const mutationGeneration = useRef(0);
  const mounted = useRef(true);
  const operation = useRef(false);
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  const queued = useRef(0);
  const snapshotRef = useRef<SatisfactoryWorldSnapshot | null>(null);
  const activeInstance = useRef(instanceId);
  activeInstance.current = instanceId;
  const previousStatus = useRef(details.summary.status);
  const pendingSession = useRef<string | null>(null);

  const accept = useCallback((next: SatisfactoryWorldSnapshot) => {
    if (next.instance_id !== instanceId) throw new Error("The server response belongs to another instance.");
    snapshotRef.current = next;
    if (mounted.current) setSnapshot(next);
    if (pendingSession.current !== null && next.is_game_running && next.active_session_name === pendingSession.current) {
      pendingSession.current = null; if (mounted.current) setResult("loaded");
    }
  }, [instanceId]);
  const refresh = useCallback(async () => {
    if (operation.current || queued.current) return;
    const current = ++generation.current;
    setLoading(true); setError(null);
    try {
      const next = await readSatisfactoryWorldSettings(instanceId);
      if (current === generation.current) accept(next);
    } catch (cause) {
      if (current === generation.current) setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      if (current === generation.current) setLoading(false);
    }
  }, [accept, instanceId]);
  useEffect(() => {
    let disposed = false;
    mounted.current = true;
    snapshotRef.current = null; setSnapshot(null); setResult(null); setError(null); operation.current = false; pendingSession.current = null; setBusy(false);
    previousStatus.current = details.summary.status;
    // Finish an older instance's owned write before reading the new instance.
    void queue.current.then(() => { if (!disposed) void refresh(); });
    return () => { disposed = true; mounted.current = false; generation.current += 1; };
  }, [refresh]);
  useEffect(() => {
    if (!busy && previousStatus.current !== details.summary.status) {
      previousStatus.current = details.summary.status;
      void refresh();
    }
  }, [busy, details.summary.status, refresh]);

  const execute = useCallback(async (request: (snapshot: SatisfactoryWorldSnapshot | null) => Promise<SatisfactoryWorldSnapshot | SatisfactoryWorldOperationResult>) => {
    if (activeInstance.current !== instanceId || pendingSession.current !== null) return false;
    operation.current = true;
    generation.current += 1;
    const current = ++mutationGeneration.current;
    if (mounted.current) { setBusy(true); setError(null); setResult(null); }
    try {
      const response = await request(snapshotRef.current);
      if (current !== mutationGeneration.current) return false;
      if (response.instance_id !== instanceId) throw new Error("The server response belongs to another instance.");
      if ("connection_status" in response) {
        accept(response); if (mounted.current) setResult("saved"); return true;
      }
      if (!response.accepted) throw new Error("The server did not accept the world operation.");
      pendingSession.current = response.session_name;
      if (mounted.current) setResult("accepted");
      // The native API returns 202 before map travel completes. Poll read-only
      // state with a finite budget; never resend the accepted mutation.
      for (let attempt = 0; attempt < 12 && current === mutationGeneration.current; attempt += 1) {
        await new Promise<void>((resolve) => setTimeout(resolve, 1000));
        if (current !== mutationGeneration.current) break;
        const next = await readSatisfactoryWorldSettings(instanceId);
        if (current !== mutationGeneration.current) break;
        accept(next);
        if (next.is_game_running && next.active_session_name === response.session_name) {
          if (mounted.current) setResult("loaded"); break;
        }
      }
      return current === mutationGeneration.current;
    } catch (cause) {
      if (current === mutationGeneration.current && mounted.current) setError(cause instanceof Error ? cause.message : String(cause));
      return false;
    } finally {
      if (current === mutationGeneration.current) { operation.current = false; if (mounted.current) setLoading(false); }
    }
  }, [accept, instanceId]);
  const mutate = useCallback((request: Parameters<typeof execute>[0]) => {
    queued.current += 1; if (mounted.current) setBusy(true);
    const completion = queue.current.then(() => execute(request)).finally(() => {
      queued.current -= 1; if (!queued.current && mounted.current) setBusy(false);
    });
    queue.current = completion.then(() => undefined, () => undefined);
    return completion;
  }, [execute]);
  const getSnapshot = useCallback(() => snapshotRef.current, []);
  return <NativeWorldContext.Provider value={{ snapshot, loading, busy, pending: pendingSession.current !== null, error, result, refresh, getSnapshot, mutate }}>
    {children}
  </NativeWorldContext.Provider>;
}

export function useSatisfactoryWorldSettings() {
  const context = useContext(NativeWorldContext);
  if (!context) throw new Error("Satisfactory settings require their instance provider.");
  return context;
}
