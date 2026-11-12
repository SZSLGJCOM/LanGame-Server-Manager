import { useCallback, useEffect, useSyncExternalStore } from "react";
import { readBackgroundJobs } from "../../api";
import { SingleFlightPoller } from "../../domain/single-flight-poller";
import type { BackgroundJob } from "../../types";
import { useInstanceSettingsSaveCoordinator } from "../settings/InstanceSettingsSaveContext";
import {
  isWorkshopDownloadPending, workshopDownloadOwner, WorkshopDownloadOperations,
  type WorkshopDownloadExecutor, type WorkshopDownloadRequest
} from "./workshop-download-operations";

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
  const register = useCallback((request: WorkshopDownloadRequest, completion: Promise<void> | null) =>
    completion ? owner.runOperation(instanceId, () => completion, workshopDownloadOwner(request)) : null,
  [instanceId, owner]);
  const enqueue = useCallback((request: WorkshopDownloadRequest, execute: WorkshopDownloadExecutor) => {
    if (readOnly) return null;
    return register(request, store.enqueue(instanceId, request, execute));
  }, [store, instanceId, readOnly, register]);
  const retry = useCallback((requestId: number) => {
    if (readOnly) return null;
    const request = store.snapshot(instanceId).requests.find((entry) => entry.requestId === requestId);
    return request ? register(request, store.retry(instanceId, requestId)) : null;
  }, [store, instanceId, readOnly, register]);
  const tracking = state.requests.some((request) => isWorkshopDownloadPending(request) && request.jobId);

  useEffect(() => {
    if (readOnly || !tracking) return;
    const poller = new SingleFlightPoller<BackgroundJob[], number>({
      intervalMs: 350,
      poll: readBackgroundJobs,
      schedule: (callback, delayMs) => window.setTimeout(callback, delayMs),
      cancel: (handle) => window.clearTimeout(handle),
      onValue: (jobs) => {
        const byId = new Map(jobs.map((job) => [job.id, job]));
        for (const request of store.snapshot(instanceId).requests) {
          const job = request.jobId ? byId.get(request.jobId) : undefined;
          if (job && request.attemptToken) store.recordJob(instanceId, request.requestId, request.attemptToken, job);
        }
      }
    });
    poller.start();
    return () => poller.dispose();
  }, [instanceId, readOnly, tracking, store]);

  return { state, enqueue, retry };
}
