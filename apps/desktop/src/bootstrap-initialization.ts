import type {
  BootstrapResponse,
  InstanceSummary,
  ModuleSummary,
  OverlayFamily,
  StorageStatus
} from "./types";

export interface BootstrapInitializationServices {
  bootstrapApp: () => Promise<BootstrapResponse>;
  ensureStorageReady: () => Promise<StorageStatus>;
  syncModulesToStorage: () => Promise<ModuleSummary[]>;
  listInstancesFromStorage: () => Promise<InstanceSummary[]>;
  fetchAppVersion: () => Promise<string>;
  fetchOverlayFamilies: () => Promise<OverlayFamily[]>;
  logFrontendEvent: (
    level: string,
    action: string,
    message: string,
    context?: Record<string, unknown>
  ) => Promise<void>;
  describeError: (error: unknown) => string;
  now?: () => number;
}

export type BootstrapInitializationOutcome =
  | {
      status: "ready";
      booted: BootstrapResponse;
    }
  | {
      status: "failed";
      error: unknown;
      storage: StorageStatus | null;
    };

export interface BootstrapInitializationMetadata {
  appVersion: string | null;
  overlays: OverlayFamily[] | null;
}

export interface BootstrapInitializationFlight {
  attempt: number;
  initialization: Promise<BootstrapInitializationOutcome>;
  metadata: Promise<BootstrapInitializationMetadata>;
}

export interface BootstrapInitializationCoordinator {
  request: (attempt: number) => BootstrapInitializationFlight;
}

export function createBootstrapInitializationCoordinator(
  services: BootstrapInitializationServices
): BootstrapInitializationCoordinator {
  const flights = new Map<number, BootstrapInitializationFlight>();
  const now = services.now ?? Date.now;

  function log(
    level: string,
    action: string,
    message: string,
    context?: Record<string, unknown>
  ) {
    void services.logFrontendEvent(level, action, message, context).catch(() => undefined);
  }

  async function initialize(attempt: number): Promise<BootstrapInitializationOutcome> {
    const startedAt = now();
    let storageProbe: StorageStatus | null = null;

    log("info", "frontend.bootstrap.request", "Starting desktop bootstrap", { attempt });
    try {
      const booted = await services.bootstrapApp();
      storageProbe = booted.state.storage;
      const storage = await services.ensureStorageReady();
      storageProbe = storage;
      const modules = await services.syncModulesToStorage();
      const instances = await services.listInstancesFromStorage();
      const readyBooted: BootstrapResponse = {
        ...booted,
        state: {
          ...booted.state,
          storage,
          modules,
          instances
        }
      };

      log("info", "frontend.bootstrap.success", "Desktop bootstrap synced", {
        attempt,
        duration_ms: now() - startedAt,
        module_count: modules.length,
        instance_count: instances.length,
        schema_version: storage.schema_version
      });
      return { status: "ready", booted: readyBooted };
    } catch (error) {
      log("error", "frontend.bootstrap.failed", services.describeError(error), {
        attempt,
        duration_ms: now() - startedAt
      });
      return { status: "failed", error, storage: storageProbe };
    }
  }

  async function loadMetadata(attempt: number): Promise<BootstrapInitializationMetadata> {
    const [appVersionResult, overlaysResult] = await Promise.allSettled([
      services.fetchAppVersion(),
      services.fetchOverlayFamilies()
    ]);

    if (appVersionResult.status === "rejected") {
      log(
        "warn",
        "frontend.bootstrap.app_version_failed",
        services.describeError(appVersionResult.reason),
        { attempt }
      );
    }
    if (overlaysResult.status === "rejected") {
      log(
        "warn",
        "frontend.bootstrap.overlays_failed",
        services.describeError(overlaysResult.reason),
        { attempt }
      );
    }

    return {
      appVersion: appVersionResult.status === "fulfilled" ? appVersionResult.value : null,
      overlays: overlaysResult.status === "fulfilled" ? overlaysResult.value : null
    };
  }

  return {
    request(attempt) {
      const existing = flights.get(attempt);
      if (existing) {
        return existing;
      }

      const flight: BootstrapInitializationFlight = {
        attempt,
        initialization: initialize(attempt),
        metadata: loadMetadata(attempt)
      };
      flights.set(attempt, flight);

      void Promise.allSettled([flight.initialization, flight.metadata]).then(() => {
        if (flights.get(attempt) === flight) {
          flights.delete(attempt);
        }
      });
      return flight;
    }
  };
}
