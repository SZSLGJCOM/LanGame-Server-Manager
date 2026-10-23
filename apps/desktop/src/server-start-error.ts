import { message, type UiMessage } from "./app-ui";
import type { StartInstanceResult } from "./types";
import { readRuntimeStartWarnings } from "./runtime-start-warning";

export function serverStartSuccessMessage(result: StartInstanceResult | undefined, instanceId: string): UiMessage {
  const warnings = readRuntimeStartWarnings(result?.warnings);
  if (!result || !warnings.length) return message("activity.serverStartSent", { id: instanceId });
  const key = warnings.some((warning) => warning.code === "world_initialization_incomplete")
    ? "activity.serverStartWorldInitializationIncomplete"
    : warnings.some((warning) => warning.code === "graceful_stop_unavailable")
    ? "activity.serverStartGracefulStopUnavailable"
    : warnings.some((warning) => warning.code === "manual_save_unavailable")
    ? "activity.serverStartManualSaveUnavailable"
    : warnings.some((warning) => warning.code === "program_update_using_installed")
    ? "activity.serverStartUsingInstalledProgram"
    : warnings.some((warning) => warning.code === "mod_update_source_required")
    ? "activity.serverStartModSourceRequired" : "activity.serverStartUsingCache";
  return message(key, { id: instanceId }, { tone: "warning",
    runtimeStartNotice: { id: `manual-start:${instanceId}:${result.run_id}`, instanceId,
      label: result.summary.name, warnings } });
}

export interface ModuleNotReadyError {
  code: "module_not_ready";
  module_id: string;
  module_name: string;
  install_state: string;
  message: string;
}

function readStartErrorFields(error: unknown): Record<string, unknown> | null {
  let value: unknown = error instanceof Error && !("code" in error) ? error.message : error;
  if (typeof value === "string") {
    try {
      value = JSON.parse(value);
    } catch {
      return null;
    }
  }
  if (!value || typeof value !== "object") return null;
  return value as Record<string, unknown>;
}

export function readModuleNotReadyError(error: unknown): ModuleNotReadyError | null {
  const fields = readStartErrorFields(error);
  if (!fields) return null;
  if (fields.code !== "module_not_ready"
    || typeof fields.module_id !== "string" || !fields.module_id.trim()
    || typeof fields.module_name !== "string"
    || typeof fields.install_state !== "string"
    || typeof fields.message !== "string") return null;
  return {
    code: "module_not_ready",
    module_id: fields.module_id,
    module_name: fields.module_name,
    install_state: fields.install_state,
    message: fields.message
  };
}

export function serverStartFailureMessage(error: unknown): UiMessage {
  const code = readStartErrorFields(error)?.code;
  if (code === "dst_world_start_changed") return message("dst.start.error.changed");
  if (code === "instance_settings_draft_invalid") return message("dst.start.error.invalid");
  const issue = readModuleNotReadyError(error);
  if (!issue) {
    return message("activity.startServerFailed", { message: error instanceof Error ? error.message : String(error) });
  }
  const params = { module: issue.module_name.trim() || issue.module_id };
  switch (issue.install_state.toLowerCase()) {
    case "notinstalled": return message("activity.startServerNotInstalled", params);
    case "incomplete":
    case "corrupted": return message("activity.startServerNeedsRepair", params);
    case "installing":
    case "updating":
    case "uninstalling": return message("activity.startServerInstallBusy", params);
    default: return message("activity.startServerInstallNotReady", params);
  }
}
