import { SingleFlightPoller } from "./domain/single-flight-poller";
import type { LaunchPlan, ModuleDetails, ModuleSummary } from "./types";

export interface LaunchPreviewResult {
  launchPlan: LaunchPlan | null;
  launchPlanError: string | null;
}

export interface InstallationSnapshot {
  modules: ModuleSummary[];
  instanceId: string | null;
  preview: LaunchPreviewResult;
}

interface InstallationReadPorts {
  readModules: () => Promise<ModuleSummary[]>;
  readPreview: (instanceId: string) => Promise<LaunchPreviewResult>;
  getCurrentInstanceId: () => string | null;
}

interface PreviewApplyPorts {
  getCurrentInstanceId: () => string | null;
  onPreview: (preview: LaunchPreviewResult) => void;
}

export class LibraryInstallationReader {
  private current: Promise<InstallationSnapshot> | null = null;
  private queued: Promise<InstallationSnapshot> | null = null;

  constructor(private readonly ports: InstallationReadPorts) {}

  read(): Promise<InstallationSnapshot> {
    if (this.queued) return this.queued;
    if (!this.current) return this.startRead();

    // A completed install must not reuse a filesystem snapshot started before it.
    // Coalesce simultaneous focus/poll/mutation requests into one fresh successor.
    this.queued = this.current.then(
      () => this.startQueuedRead(),
      () => this.startQueuedRead()
    );
    return this.queued;
  }

  private startQueuedRead(): Promise<InstallationSnapshot> {
    this.queued = null;
    return this.startRead();
  }

  private startRead(): Promise<InstallationSnapshot> {
    const instanceId = this.ports.getCurrentInstanceId();
    const flight = Promise.allSettled([
      this.ports.readModules(),
      instanceId ? this.ports.readPreview(instanceId) : Promise.resolve({ launchPlan: null, launchPlanError: null })
    ]).then(([modules, preview]) => {
      // A failed read still waits for its sibling before another scan starts.
      if (modules.status === "rejected") throw modules.reason;
      if (preview.status === "rejected") throw preview.reason;
      return { modules: modules.value, instanceId, preview: preview.value };
    });
    const tracked = flight.finally(() => {
      if (this.current === tracked) this.current = null;
    });
    this.current = tracked;
    return tracked;
  }
}

function sameModuleSummary(left: ModuleSummary, right: ModuleSummary): boolean {
  return left.id === right.id && left.name === right.name && left.version === right.version
    && (left.description ?? null) === (right.description ?? null)
    && (left.steam_app_id ?? null) === (right.steam_app_id ?? null)
    && left.install_state === right.install_state
    && (left.instance_program_count ?? 0) === (right.instance_program_count ?? 0)
    && (left.archived_program_count ?? 0) === (right.archived_program_count ?? 0)
    && left.supported_platforms.length === right.supported_platforms.length
    && left.supported_platforms.every((platform, index) => platform === right.supported_platforms[index]);
}

export function mergeModuleSummaries(current: ModuleSummary[], incoming: ModuleSummary[]): ModuleSummary[] {
  const previousById = new Map(current.map((module) => [module.id, module]));
  const next = incoming.map((module) => {
    const previous = previousById.get(module.id);
    const next = previous ? {
      ...module,
      instance_program_count: module.instance_program_count ?? previous.instance_program_count,
      archived_program_count: module.archived_program_count ?? previous.archived_program_count
    } : module;
    return previous && sameModuleSummary(previous, next) ? previous : next;
  });
  return next.length === current.length && next.every((module, index) => module === current[index]) ? current : next;
}

// Inventory reads may finish after a newer filesystem scan. Apply only their
// counts, retaining the current catalog membership and installation status.
export function mergeModuleProgramCounts(current: ModuleSummary[], incoming: ModuleSummary[]): ModuleSummary[] {
  const countsById = new Map(incoming.map((module) => [module.id, module]));
  return mergeModuleSummaries(current, current.map((module) => {
    const counts = countsById.get(module.id);
    return counts ? { ...module, instance_program_count: counts.instance_program_count,
      archived_program_count: counts.archived_program_count } : module;
  }));
}

export function updateModuleDetailsSummary(details: ModuleDetails | null, modules: ModuleSummary[]): ModuleDetails | null {
  if (!details) return null;
  const summary = modules.find((module) => module.id === details.summary.id);
  if (!summary) return null;
  const [merged] = mergeModuleSummaries([details.summary], [summary]);
  return merged === details.summary ? details : { ...details, summary: merged };
}

export function applyInstallationSnapshot(
  snapshot: InstallationSnapshot,
  ports: PreviewApplyPorts & { onModules: (modules: ModuleSummary[]) => void }
): void {
  ports.onModules(snapshot.modules);
  if (ports.getCurrentInstanceId() === snapshot.instanceId) ports.onPreview(snapshot.preview);
}

export function createForegroundInstallationPolling<T, TimerHandle>(options: {
  read: () => Promise<T>;
  onValue: (value: T) => void;
  onError: (error: unknown) => void;
  schedule: (callback: () => void, delayMs: number) => TimerHandle;
  cancel: (handle: TimerHandle) => void;
}) {
  let poller: SingleFlightPoller<T, TimerHandle> | null = null;
  let disposed = false;
  return {
    setForeground(foreground: boolean) {
      if (disposed) return;
      if (!foreground) {
        poller?.dispose();
        poller = null;
      } else if (poller) {
        void poller.pollNow();
      } else {
        poller = new SingleFlightPoller({ intervalMs: 15000, poll: options.read, ...options });
        poller.start();
      }
    },
    dispose() {
      disposed = true;
      poller?.dispose();
      poller = null;
    }
  };
}
