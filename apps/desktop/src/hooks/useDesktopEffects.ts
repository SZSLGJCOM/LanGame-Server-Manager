import { useEffect, useRef, useState } from "react";
import {
  bootstrapApp,
  ensureStorageReady,
  fetchAppVersion,
  fetchOverlayFamilies,
  listInstancesFromStorage,
  logFrontendEvent,
  readBackgroundJobs,
  readModuleDetails,
  syncModulesToStorage
} from "../api";
import {
  describeError,
  resolveRuntimeRefreshMode,
  resolveSelectedId,
  SYSTEM_VIEW_WARMUP_POLL_DELAY_MS
} from "../app-state";
import { instancePanelReader, type InstancePanelLoadState, type InstancePanelPatch } from "../instance-panel-loader";
import { createBootstrapInitializationCoordinator } from "../bootstrap-initialization";
import { SingleFlightPoller } from "../domain/single-flight-poller";
import { RESOURCE_STALE_AFTER_MS } from "../domain/system-resources";
import type {
  BackgroundJob,
  BootstrapResponse,
  ModuleDetails,
  ModuleSummary,
  OverlayFamily,
  StorageStatus
} from "../types";

const bootstrapInitializationCoordinator = createBootstrapInitializationCoordinator({
  bootstrapApp,
  ensureStorageReady,
  syncModulesToStorage,
  listInstancesFromStorage,
  fetchAppVersion,
  fetchOverlayFamilies,
  logFrontendEvent,
  describeError
});

export function useRuntimeRefreshModeSync(onChange: (mode: "live" | "throttled") => void) {
  useEffect(() => {
    if (typeof window === "undefined" || typeof document === "undefined") {
      return;
    }

    function syncRuntimeRefreshMode() {
      onChange(resolveRuntimeRefreshMode());
    }

    syncRuntimeRefreshMode();
    window.addEventListener("focus", syncRuntimeRefreshMode);
    window.addEventListener("blur", syncRuntimeRefreshMode);
    document.addEventListener("visibilitychange", syncRuntimeRefreshMode);

    return () => {
      window.removeEventListener("focus", syncRuntimeRefreshMode);
      window.removeEventListener("blur", syncRuntimeRefreshMode);
      document.removeEventListener("visibilitychange", syncRuntimeRefreshMode);
    };
  }, []);
}

export function useBootstrapInitialization(options: {
  attempt: number;
  onReady: (payload: {
    attempt: number;
    booted: BootstrapResponse;
  }) => void;
  onMetadata: (payload: {
    attempt: number;
    appVersion: string | null;
    overlays: OverlayFamily[] | null;
  }) => void;
  onError: (payload: {
    attempt: number;
    error: unknown;
    storage: StorageStatus | null;
  }) => void;
}) {
  useEffect(() => {
    let cancelled = false;
    const attempt = options.attempt;
    const flight = bootstrapInitializationCoordinator.request(attempt);

    void flight.metadata.then((metadata) => {
      if (cancelled) {
        return;
      }

      options.onMetadata({
        attempt,
        appVersion: metadata.appVersion,
        overlays: metadata.overlays
      });
    });

    void flight.initialization.then((outcome) => {
      if (cancelled) {
        return;
      }
      if (outcome.status === "ready") {
        options.onReady({
          attempt,
          booted: outcome.booted
        });
        return;
      }

      options.onError({ attempt, error: outcome.error, storage: outcome.storage });
    });
    return () => {
      cancelled = true;
    };
  }, [options.attempt]);
}

export function useLibraryJobPolling(options: {
  enabled: boolean;
  onJobsSynced: (jobs: BackgroundJob[]) => void;
}) {
  useEffect(() => {
    if (!options.enabled) {
      return;
    }

    const poller = new SingleFlightPoller<BackgroundJob[], number>({
      intervalMs: 1200,
      poll: readBackgroundJobs,
      schedule: (callback, delayMs) => window.setTimeout(callback, delayMs),
      cancel: (handle) => window.clearTimeout(handle),
      onValue: options.onJobsSynced
    });
    poller.start();

    return () => {
      poller.dispose();
    };
  }, [options.enabled]);
}

export function useRuntimeViewPolling(options: {
  enabled: boolean;
  selectedInstanceId: string | null;
  intervalMs: number;
  retryGeneration: number;
  getCurrentInstances?: () => BootstrapResponse["state"]["instances"];
  isInstancePending?: (instanceId: string | null) => boolean;
  onInstancesSynced: (payload: {
    instances: BootstrapResponse["state"]["instances"];
    selectedInstanceId: string | null;
    requestedInstances?: BootstrapResponse["state"]["instances"];
  }) => void;
  onSelectionCleared: () => void;
  onSelectionLoaded: (instanceId: string, payload: InstancePanelPatch) => void;
  onProgress: (state: InstancePanelLoadState) => void;
  onSettled: (state: InstancePanelLoadState) => void;
  onError: (error: unknown) => void;
}) {
  useEffect(() => {
    if (!options.enabled) {
      return;
    }

    let cancelled = false;
    let refreshing = false;
    const controller = new AbortController();

    async function syncRuntimeView() {
      if (refreshing) {
        return;
      }
      refreshing = true;

      try {
        const requestedInstances = options.getCurrentInstances?.();
        const syncedInstances = await instancePanelReader.readInstances(controller.signal);
        if (cancelled) {
          return;
        }
        // Another request/mutation may have already changed the inventory.
        // Discard this entire observation before it can change selection or
        // launch file reads for an instance which has since been retired.
        if (requestedInstances && options.getCurrentInstances?.() !== requestedInstances) return;

        const nextInstanceId = resolveSelectedId(options.selectedInstanceId, syncedInstances);
        options.onInstancesSynced({
          instances: syncedInstances,
          selectedInstanceId: nextInstanceId,
          requestedInstances
        });

        if (options.isInstancePending?.(options.selectedInstanceId)) return;

        if (!nextInstanceId) {
          options.onSelectionCleared();
          return;
        }

        // A selection change restarts this effect; do not launch a discarded batch.
        if (nextInstanceId !== options.selectedInstanceId) return;
        const result = await instancePanelReader.load(nextInstanceId, {
          signal: controller.signal,
          onUpdate: (patch) => options.onSelectionLoaded(nextInstanceId, patch),
          onProgress: options.onProgress
        });
        if (!cancelled) options.onSettled(result);
      } catch (error) {
        if (!cancelled) {
          options.onError(error);
        }
      } finally {
        refreshing = false;
      }
    }

    void syncRuntimeView();
    const timer = window.setInterval(() => {
      void syncRuntimeView();
    }, options.intervalMs);

    return () => {
      cancelled = true;
      controller.abort();
      window.clearInterval(timer);
    };
  }, [options.enabled, options.intervalMs, options.selectedInstanceId, options.retryGeneration]);
}

async function requestSystemViewSnapshot() {
  const latest = await bootstrapApp({ includeSystemSnapshot: true });
  const observedAt = latest.state.snapshot.telemetry?.observed_at_unix_ms;
  // The native wait can expire while its single shared collector continues.
  // Join that collector once more instead of leaving an expired result for a
  // complete 60/120-second polling interval. Never retry indefinitely.
  return typeof observedAt === "number" && Date.now() - observedAt >= RESOURCE_STALE_AFTER_MS
    ? bootstrapApp({ includeSystemSnapshot: true }) : latest;
}

export function useSystemViewPolling(options: {
  enabled: boolean;
  intervalMs: number;
  requestRevision: number;
  getCurrentSnapshot?: () => BootstrapResponse;
  onSynced: (latest: BootstrapResponse, requestRevision: number, requested?: BootstrapResponse) => void;
  onError: (error: unknown, requestRevision: number) => void;
}) {
  const pending = useRef<{ revision: number; requested?: BootstrapResponse; promise: Promise<BootstrapResponse> } | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  useEffect(() => {
    if (!options.enabled) {
      setRefreshing(false);
      return;
    }

    let cancelled = false;
    let receiving = false;
    let warmupRequested = false;
    let warmupTimer: number | undefined;

    async function syncSystemView() {
      if (receiving || cancelled) {
        return;
      }
      receiving = true;
      setRefreshing(true);
      // App owns this promise across navigation and foreground/background
      // effect changes. A returning view subscribes to the existing request.
      // Retirement changes metadata ownership even if the host collector is
      // still running. Only requests from the same revision can be reused.
      if (!pending.current || pending.current.revision !== options.requestRevision) {
        pending.current = { revision: options.requestRevision, requested: options.getCurrentSnapshot?.(), promise: requestSystemViewSnapshot() };
      }
      const flight = pending.current;

      try {
        const latest = await flight.promise;
        if (!cancelled) {
          options.onSynced(latest, flight.revision, flight.requested);
          const telemetry = latest.state.snapshot.telemetry;
          const warming = !telemetry?.observed_at_unix_ms || Object.values(telemetry).includes("warming_up") ||
            latest.state.snapshot.network_adapters?.some((adapter) =>
              adapter.status.toLowerCase() === "up" && adapter.rate_status === "warming_up");
          if (warming && !warmupRequested) {
            warmupRequested = true;
            warmupTimer = window.setTimeout(() => {
              warmupTimer = undefined;
              void syncSystemView();
            }, SYSTEM_VIEW_WARMUP_POLL_DELAY_MS);
          } else if (!warming && warmupTimer !== undefined) {
            window.clearTimeout(warmupTimer);
            warmupTimer = undefined;
          }
        }
      } catch (error) {
        if (!cancelled) {
          options.onError(error, flight.revision);
        }
      } finally {
        if (pending.current === flight) pending.current = null;
        receiving = false;
        if (!cancelled) setRefreshing(false);
      }
    }

    void syncSystemView();
    const timer = window.setInterval(() => {
      void syncSystemView();
    }, options.intervalMs);

    return () => {
      cancelled = true;
      window.clearInterval(timer);
      if (warmupTimer !== undefined) window.clearTimeout(warmupTimer);
    };
  }, [options.enabled, options.intervalMs, options.requestRevision]);
  return options.enabled && refreshing;
}

export function useSelectedModuleDetailsSync(options: {
  enabled: boolean;
  selectedModuleId: string | null;
  modules: ModuleSummary[];
  onLoaded: (details: ModuleDetails | null) => void;
}) {
  const selectedSummary = options.modules.find((module) => module.id === options.selectedModuleId);
  useEffect(() => {
    if (!options.enabled || !options.selectedModuleId) {
      options.onLoaded(null);
      return;
    }

    let cancelled = false;
    readModuleDetails(options.selectedModuleId, { includePreservedProgramCounts: false })
      .then((details) => {
        if (!cancelled) {
          options.onLoaded({ ...details, summary: { ...details.summary,
            instance_program_count: selectedSummary?.instance_program_count,
            archived_program_count: selectedSummary?.archived_program_count
          } });
        }
      })
      .catch(() => {
        if (!cancelled) {
          options.onLoaded(null);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [options.enabled, options.selectedModuleId, selectedSummary]);
}

export function useSelectedInstanceModuleDetailsSync(options: {
  enabled: boolean;
  selectedModuleId: string | null;
  retryGeneration: number;
  onLoaded: (details: ModuleDetails | null, error?: { moduleId: string; message: string }) => void;
}) {
  useEffect(() => {
    options.onLoaded(null);
    if (!options.enabled || !options.selectedModuleId) {
      return;
    }

    const moduleId = options.selectedModuleId;
    let cancelled = false;
    Promise.resolve().then(() => readModuleDetails(moduleId, { includePreservedProgramCounts: false }))
      .then((details) => {
        if (!cancelled) {
          options.onLoaded(details);
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          options.onLoaded(null, { moduleId, message: describeError(error) });
        }
      });

    return () => {
      cancelled = true;
    };
  }, [options.enabled, options.selectedModuleId, options.retryGeneration]);
}

export function useSelectedInstancePanelSync(options: {
  selectedInstanceId: string | null;
  enabled: boolean;
  retryGeneration: number;
  onClear: () => void;
  onLoaded: (instanceId: string, payload: InstancePanelPatch) => void;
  onProgress: (state: InstancePanelLoadState) => void;
  onSettled: (state: InstancePanelLoadState) => void;
}) {
  useEffect(() => {
    if (!options.enabled) {
      return;
    }

    if (!options.selectedInstanceId) {
      options.onClear();
      return;
    }

    const instanceId = options.selectedInstanceId;
    const controller = new AbortController();
    void instancePanelReader.load(instanceId, {
      signal: controller.signal,
      onUpdate: (patch) => options.onLoaded(instanceId, patch),
      onProgress: options.onProgress
    }).then(result => {
      if (!controller.signal.aborted) options.onSettled(result);
    });
    return () => {
      controller.abort();
    };
  }, [options.selectedInstanceId, options.enabled, options.retryGeneration]);
}
