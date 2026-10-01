import {
  readInstanceDetails,
  readInstanceLogDocument,
  readInstanceRuntime,
  readInstanceRuntimeWindowSnapshot,
  listInstanceBackups,
  previewInstanceLaunch
} from "./api";
import type {
  BackgroundJob,
  BootstrapResponse,
  InstanceBackupResult,
  InstanceDetails,
  InstanceRuntimeOverview,
  RuntimeWindowSnapshot,
  LaunchPlan,
  LogTailSnapshot
} from "./types";

export interface SelectedInstancePanelData {
  details: InstanceDetails;
  backups: InstanceBackupResult[];
  runtime: InstanceRuntimeOverview;
  runtimeWindows: RuntimeWindowSnapshot;
  logDocument: LogTailSnapshot;
  launchPlan: LaunchPlan | null;
  launchPlanError: string | null;
}

export interface RuntimeRefreshIssue {
  message: string;
  failedAt: number;
  consecutiveFailures: number;
}

export type LibraryPageMode = "catalog" | "detail";

export const fallbackBootstrap: BootstrapResponse = {
  booted_at_unix_ms: Date.now(),
  state: {
    settings: {
      servers_root: "D:/LanGame/instances",
      games_root: "D:/LanGame/server-files",
      archives_root: "D:/LanGame/instances/.trash",
      modules_root: "./modules",
      steamcmd_root: "D:/LanGame/cmd/steamcmd"
    },
    storage: {
      database_path: "LocalAppData/LanGame/ServerManager/db/lgs.db",
      migrations_path: "./migrations",
      app_log_path: "LocalAppData/LanGame/ServerManager/logs/desktop-app.log",
      database_exists: false,
      schema_version: 0,
      migrations_applied: false
    },
    modules: [],
    instances: [],
    jobs: [],
    snapshot: {
      cpu_percent: 0,
      cpu_name: "",
      cpu_frequency_mhz: 0,
      cpu_max_frequency_mhz: 0,
      cpu_physical_cores: 0,
      cpu_logical_cores: 0,
      cpu_single_core_peak_percent: 0,
      cpu_performance_percent: 0,
      cpu_cores: [],
      memory_percent: 0,
      memory_total_bytes: 0,
      memory_available_bytes: 0,
      memory_modules: [],
      disk_used_percent: 0,
      disk_used_bytes: 0,
      disk_total_bytes: 0,
      disk_label: "",
      disk_volume_name: "",
      disk_file_system: "",
      disk_read_bps: 0,
      disk_write_bps: 0,
      disk_read_latency_ms: 0,
      disk_write_latency_ms: 0,
      disk_queue_length: 0,
      network_receive_bps: 0,
      network_transmit_bps: 0,
      network_adapters: [],
      instance_process_memory_bytes: 0,
      instance_process_memory_percent: 0,
      instance_process_count: 0,
      instance_process_threads: 0,
      instance_process_handles: 0,
      running_instances: 0,
      total_online_players: 0,
      total_player_capacity: 0,
      player_count_queried_instances: 0,
      player_count_queryable_instances: 0
    }
  }
};

export const RUNTIME_VIEW_POLL_MS = 4000;
export const RUNTIME_VIEW_BACKGROUND_POLL_MS = 12000;
export const SYSTEM_VIEW_POLL_MS = 60000;
export const SYSTEM_VIEW_BACKGROUND_POLL_MS = 120000;
// Counter warm-up needs a second sample after the native 15-second snapshot cache expires.
export const SYSTEM_VIEW_WARMUP_POLL_DELAY_MS = 16000;
export const BIND_ADDRESS_INITIAL_POLL_DELAY_MS = 10000;
export const BIND_ADDRESS_POLL_MS = 60000;
export const RUNTIME_REFRESH_FAILURE_LIMIT = 3;

export function describeError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function resolveSelectedId<T extends { id: string }>(current: string | null, items: T[]): string | null {
  if (current && items.some((item) => item.id === current)) {
    return current;
  }
  return items[0]?.id ?? null;
}

export function resolveLogDocument(runtime: InstanceRuntimeOverview, logDocument: LogTailSnapshot | null): LogTailSnapshot {
  if (!logDocument) {
    return runtime.log_tail;
  }
  if (logDocument.lines.length > 0 || logDocument.read_error || logDocument.source_path) {
    return logDocument;
  }
  return runtime.log_tail;
}

export function isActiveJobStatus(status: string | null | undefined): boolean {
  const normalized = String(status ?? "").toLowerCase();
  return normalized === "pending" || normalized === "running";
}

export function isLibraryJob(job: BackgroundJob): boolean {
  const kind = String(job.kind).toLowerCase();
  return kind === "downloadgame" || kind === "validategame" || kind === "uninstallgame" || kind === "installsteamcmd";
}

export function resolveRuntimeRefreshMode(): "live" | "throttled" {
  if (typeof document === "undefined") {
    return "live";
  }

  const visible = document.visibilityState === "visible";
  const focused = typeof document.hasFocus === "function" ? document.hasFocus() : true;
  return visible && focused ? "live" : "throttled";
}

export async function loadLaunchPlanPreview(
  instanceId: string
): Promise<{ launchPlan: LaunchPlan | null; launchPlanError: string | null }> {
  try {
    const launchPlan = await previewInstanceLaunch(instanceId);
    return { launchPlan, launchPlanError: null };
  } catch (error) {
    return { launchPlan: null, launchPlanError: describeError(error) };
  }
}

export async function loadInstancePanelData(instanceId: string): Promise<SelectedInstancePanelData> {
  const [details, backups, runtime, runtimeWindows, logDocument, launchPreview] = await Promise.all([
    readInstanceDetails(instanceId),
    listInstanceBackups(instanceId),
    readInstanceRuntime(instanceId),
    readInstanceRuntimeWindowSnapshot(instanceId),
    readInstanceLogDocument(instanceId, 400).catch(() => null),
    loadLaunchPlanPreview(instanceId)
  ]);
  return {
    details,
    backups,
    runtime,
    runtimeWindows,
    logDocument: resolveLogDocument(runtime, logDocument),
    launchPlan: launchPreview.launchPlan,
    launchPlanError: launchPreview.launchPlanError
  };
}
