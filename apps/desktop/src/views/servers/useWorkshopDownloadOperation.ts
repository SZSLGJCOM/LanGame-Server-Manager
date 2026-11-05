import { useCallback, useEffect, useSyncExternalStore } from "react";
import { readBackgroundJobs } from "../../api";
import { SingleFlightPoller } from "../../domain/single-flight-poller";
import type { BackgroundJob } from "../../types";
import { useInstanceSettingsSaveCoordinator } from "../settings/InstanceSettingsSaveContext";
import { activeWorkshopDownloadJob, WorkshopDownloadOperations, type WorkshopDownloadState } from "./workshop-download-operations";

// The existing app lifetime also owns the in-flight settings saves. Keeping the
// store in that scope isolates separate app trees and survives page/instance changes.
const stores = new WeakMap<object, WorkshopDownloadOperations>();

export function useWorkshopDownloadOperation(instanceId: string, readOnly: boolean) {
  const owner = useInstanceSettingsSaveCoordinator();
  let operations = stores.get(owner);
  if (!operations) { operations = new WorkshopDownloadOperations(); stores.set(owner, operations); }
  const store = operations;
  const snapshot = useCallback(() => store.snapshot(instanceId), [store, instanceId]);
  const state = useSyncExternalStore(store.subscribe, snapshot, snapshot);
  const begin = useCallback((request: Pick<WorkshopDownloadState, "ids" | "contentIds" | "items" | "locale">) =>
    store.begin(instanceId, request), [store, instanceId]);

  useEffect(() => {
    const excludedJobIds = state.excludedJobIds;
    if (readOnly || state.status !== "running" || !excludedJobIds) return;
    const poller = new SingleFlightPoller<BackgroundJob[], number>({
      intervalMs: 350,
      poll: readBackgroundJobs,
      schedule: (callback, delayMs) => window.setTimeout(callback, delayMs),
      cancel: (handle) => window.clearTimeout(handle),
      onValue: (jobs) => {
        const job = activeWorkshopDownloadJob(jobs, instanceId, excludedJobIds);
        if (job) store.recordJob(instanceId, state.requestId, job);
      }
    });
    poller.start();
    return () => poller.dispose();
  }, [instanceId, readOnly, state.requestId, state.status, state.excludedJobIds, store]);

  return { state, begin };
}
