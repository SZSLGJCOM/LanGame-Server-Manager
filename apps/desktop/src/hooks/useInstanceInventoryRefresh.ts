import { useEffect, useRef } from "react";
import { resolveSelectedId } from "../app-state";
import { instancePanelReader } from "../instance-panel-loader";
import type { InstanceSummary } from "../types";

interface InstanceInventoryRefreshOptions {
  enabled: boolean;
  getCurrentInstances: () => InstanceSummary[];
  getCurrentInstanceId: () => string | null;
  onInstancesSynced: (payload: {
    instances: InstanceSummary[];
    selectedInstanceId: string | null;
    requestedInstances: InstanceSummary[];
  }) => void;
  onSelectionCleared: () => void;
  onError: (error: unknown) => void;
}

/** Reconcile external directory changes even when detail polling has paused. */
export function useInstanceInventoryRefresh(options: InstanceInventoryRefreshOptions) {
  const latest = useRef(options);
  latest.current = options;

  useEffect(() => {
    if (!options.enabled) return;
    const controller = new AbortController();
    let refreshing = false;
    async function refresh() {
      if (refreshing || document.visibilityState !== "visible") return;
      refreshing = true;
      const requestedInstances = latest.current.getCurrentInstances();
      try {
        const instances = await instancePanelReader.readInstances(controller.signal);
        if (controller.signal.aborted || latest.current.getCurrentInstances() !== requestedInstances) return;
        const selectedInstanceId = resolveSelectedId(latest.current.getCurrentInstanceId(), instances);
        latest.current.onInstancesSynced({ instances, requestedInstances, selectedInstanceId });
        if (!selectedInstanceId) latest.current.onSelectionCleared();
      } catch (error) {
        if (!controller.signal.aborted) latest.current.onError(error);
      } finally {
        refreshing = false;
      }
    }
    const onForeground = () => { void refresh(); };
    onForeground();
    window.addEventListener("focus", onForeground);
    document.addEventListener("visibilitychange", onForeground);
    return () => {
      controller.abort();
      window.removeEventListener("focus", onForeground);
      document.removeEventListener("visibilitychange", onForeground);
    };
  }, [options.enabled]);
}
