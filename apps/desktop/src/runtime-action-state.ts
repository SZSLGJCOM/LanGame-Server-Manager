import { normalizeInstanceStatus } from "./instance-status";

export type RuntimeActionIntent = "start" | "stop";
export type RuntimeActionPending = "starting" | "stopping";

export type RuntimeActionIconName = "zap" | "square" | "refresh";

export interface ServerRuntimeActionModel {
  action: RuntimeActionIntent;
  pending: RuntimeActionPending | null;
  displayedStatus: string;
  disabled: boolean;
  ariaBusy: boolean;
  iconName: RuntimeActionIconName;
  className: string;
  labelKey: "common.start" | "common.stop" | "status.instance.starting" | "status.instance.stopping";
}

interface RuntimeProcessStateLike {
  process_key?: string | null;
  status?: string | null;
}

interface InstanceRuntimeSummaryLike {
  status?: string | null;
  active_process_count?: number | null;
}

interface InstanceActiveRunLike {
  processes?: RuntimeProcessStateLike[] | null;
}

export function runtimeProcessIsRunning(process: RuntimeProcessStateLike): boolean {
  return normalizeInstanceStatus(process.status) === "running";
}

export function instanceHasRunningProcess(
  summary: InstanceRuntimeSummaryLike,
  activeRun?: InstanceActiveRunLike | null
): boolean {
  const processes = activeRun?.processes ?? [];
  if (processes.length > 0) {
    return processes.some(runtimeProcessIsRunning);
  }
  return Number(summary.active_process_count ?? 0) > 0 || normalizeInstanceStatus(summary.status) === "running";
}

export function runtimeProcessKeyIsRunning(
  activeRun: InstanceActiveRunLike | null | undefined,
  processKey: string
): boolean {
  const expectedKey = processKey.trim().toLowerCase();
  if (!expectedKey) {
    return false;
  }
  return (activeRun?.processes ?? []).some((process) => (
    String(process.process_key ?? "").trim().toLowerCase() === expectedKey
    && runtimeProcessIsRunning(process)
  ));
}

export function pendingRuntimeActionForIntent(intent: RuntimeActionIntent): RuntimeActionPending {
  return intent === "start" ? "starting" : "stopping";
}

export function resolveServerRuntimeAction(
  status: string | null | undefined,
  pending: RuntimeActionPending | null | undefined,
  hasRunningProcess = false
): ServerRuntimeActionModel {
  const normalizedStatus = normalizeInstanceStatus(status);
  const pendingState = pending ?? null;

  if (pendingState === "starting" || normalizedStatus === "starting") {
    return {
      action: "start",
      pending: pendingState,
      displayedStatus: "Starting",
      disabled: true,
      ariaBusy: true,
      iconName: "refresh",
      className: "ghost-button success server-list-card-primary-action is-busy",
      labelKey: "status.instance.starting"
    };
  }

  if (pendingState === "stopping" || normalizedStatus === "stopping") {
    return {
      action: "stop",
      pending: pendingState,
      displayedStatus: "Stopping",
      disabled: true,
      ariaBusy: true,
      iconName: "refresh",
      className: "ghost-button danger server-list-card-primary-action is-busy",
      labelKey: "status.instance.stopping"
    };
  }

  if (normalizedStatus === "running" || hasRunningProcess) {
    return {
      action: "stop",
      pending: null,
      displayedStatus: normalizedStatus === "running" ? "Running" : status || "Running",
      disabled: false,
      ariaBusy: false,
      iconName: "square",
      className: "ghost-button danger server-list-card-primary-action",
      labelKey: "common.stop"
    };
  }

  return {
    action: "start",
    pending: null,
    displayedStatus: status || "Stopped",
    disabled: false,
    ariaBusy: false,
    iconName: "zap",
    className: "ghost-button success server-list-card-primary-action",
    labelKey: "common.start"
  };
}
