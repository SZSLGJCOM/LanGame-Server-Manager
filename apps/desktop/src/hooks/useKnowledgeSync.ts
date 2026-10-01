import { useCallback, useEffect, useRef, useState } from "react";
import { knowledgeApi } from "../api-knowledge";
import { isKnowledgeJobActive, type KnowledgeApi, type KnowledgeRuntimeStatus, type KnowledgeSettings } from "../knowledge-types";

const message = (error: unknown) => error instanceof Error ? error.message : String(error);

export function useKnowledgeSync(api: KnowledgeApi = knowledgeApi) {
  const [status, setStatus] = useState<KnowledgeRuntimeStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [pending, setPending] = useState<"start" | "cancel" | "save" | null>(null);
  const mounted = useRef(false);
  const sequence = useRef(0);
  const busy = useRef(false);
  const latest = useRef<KnowledgeRuntimeStatus | null>(null);
  const available = api.available();
  const active = isKnowledgeJobActive(status?.job);

  const reload = useCallback(async () => {
    const request = ++sequence.current;
    if (mounted.current) setLoading(true);
    try {
      const next = await api.status();
      if (mounted.current && request === sequence.current) {
        latest.current = next;
        setStatus(next);
        setLoadError(null);
      }
    } catch (error) {
      if (mounted.current && request === sequence.current) setLoadError(message(error));
    } finally {
      if (mounted.current && request === sequence.current) setLoading(false);
    }
  }, [api]);

  useEffect(() => {
    mounted.current = true;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    async function poll() {
      await reload();
      if (!stopped) timer = setTimeout(() => { void poll(); }, isKnowledgeJobActive(latest.current?.job) ? 1500 : 15000);
    }
    if (available) void poll();
    return () => { stopped = true; mounted.current = false; ++sequence.current; clearTimeout(timer); };
  }, [available, reload, active]);

  const run = useCallback(async (kind: "start" | "cancel" | "save", action: () => Promise<unknown>) => {
    if (busy.current) return;
    busy.current = true;
    ++sequence.current; // A pre-mutation poll cannot overwrite the refreshed result.
    setPending(kind);
    setActionError(null);
    try {
      await action();
      if (mounted.current) await reload();
    } catch (error) {
      if (mounted.current) setActionError(message(error));
    } finally {
      busy.current = false;
      if (mounted.current) setPending(null);
    }
  }, [reload]);

  return {
    available, status, loading, pending, error: actionError ?? loadError, reload,
    start: (moduleId: string | null) => run("start", () => api.start(moduleId)),
    save: (settings: KnowledgeSettings) => run("save", () => api.save(settings)),
    cancel: () => {
      const job = latest.current?.job;
      return job && isKnowledgeJobActive(job) ? run("cancel", () => api.cancel(job.id)) : Promise.resolve();
    }
  };
}
