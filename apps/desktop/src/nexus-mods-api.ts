import { canUseLocalDesktopApi, invokeOrMock } from "./api-transport";
import type { ManualModStageResult } from "./types";

const errorCodes = ["nexus_api_unavailable", "nexus_auth_required", "nexus_auth_invalid", "nexus_premium_required", "nexus_rate_limited",
  "nexus_game_mismatch", "nexus_reference_invalid", "nexus_download_expired", "nexus_file_unavailable", "nexus_network_error",
  "nexus_browser_unavailable", "nexus_browser_busy", "nexus_browser_cancelled", "nexus_browser_page_unavailable", "nexus_browser_timed_out", "nexus_one_project_required",
  "nexus_format_unsupported", "nexus_preparation_failed", "nexus_desktop_update_required",
  "nexus_website_login_required", "nexus_website_verification_required", "nexus_file_selection_required",
  "nexus_website_action_required", "nexus_website_controls_unavailable", "nexus_website_file_quarantined",
  "nexus_local_desktop_required", "nexus_invalid_response", "nexus_operation_failed", "nexus_installation_failed"] as const;
export type NexusErrorCode = typeof errorCodes[number];
export class NexusApiError extends Error {
  constructor(readonly code: NexusErrorCode, readonly safeMessage?: string) { super(code); }
}
export type NexusWebsiteAuthState = "unknown" | "signed_out" | "signed_in";
export function isNexusWebsiteAuthState(value: unknown): value is NexusWebsiteAuthState {
  return value === "unknown" || value === "signed_out" || value === "signed_in";
}
export async function readNexusWebsiteAuthState(): Promise<NexusWebsiteAuthState> {
  const state = await requestNexusCommand("read_nexus_website_auth_state");
  if (!isNexusWebsiteAuthState(state)) throw new NexusApiError("nexus_invalid_response");
  return state;
}
const record = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === "object" && !Array.isArray(value);
const nullableText = (value: unknown) => value === null || typeof value === "string";
const nonnegativeInteger = (value: unknown): value is number => typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
function requiredText(value: unknown): string {
  if (typeof value !== "string") throw new NexusApiError("nexus_invalid_response");
  return value;
}
export async function requestNexusCommand(command: string, args?: Record<string, unknown>): Promise<unknown> {
  if (!canUseLocalDesktopApi()) throw new NexusApiError("nexus_local_desktop_required");
  try { return await invokeOrMock<unknown>(command, args); }
  catch (reason) {
    let error: unknown = reason instanceof Error ? reason.message : reason;
    if (error === `Command ${command} not found` || error === `unknown LanGame command: ${command}`) {
      throw new NexusApiError("nexus_desktop_update_required");
    }
    if (typeof error === "string") { try { error = JSON.parse(error); } catch { error = null; } }
    const code = record(error) ? error.code : null;
    // Unknown backend messages may contain user input or signed URLs. Render only known error codes.
    const safeMessage = ["nexus_installation_failed", "nexus_preparation_failed"].includes(String(code))
      && record(error) && typeof error.message === "string" ? error.message : undefined;
    throw new NexusApiError(errorCodes.find((candidate) => candidate === code) ?? "nexus_operation_failed", safeMessage);
  }
}
export function nexusStageResult(instanceId: string, value: unknown): ManualModStageResult {
  if (!record(value) || value.instance_id !== instanceId || !nonnegativeInteger(value.copied_file_count)
    || !nonnegativeInteger(value.copied_total_bytes) || !Array.isArray(value.items) || !Array.isArray(value.affected_root_names)) throw new NexusApiError("nexus_invalid_response");
  return { instance_id: instanceId, module_id: requiredText(value.module_id), source_label: requiredText(value.source_label),
    target_label: requiredText(value.target_label), target_path: requiredText(value.target_path),
    copied_file_count: value.copied_file_count, copied_total_bytes: value.copied_total_bytes,
    affected_root_names: value.affected_root_names.map(requiredText), items: value.items.map((item) => {
      if (!record(item) || !nonnegativeInteger(item.file_count) || !nonnegativeInteger(item.total_bytes)
        || (item.message !== undefined && !nullableText(item.message))) throw new NexusApiError("nexus_invalid_response");
      return { source_path: requiredText(item.source_path), target_path: requiredText(item.target_path), status: requiredText(item.status),
        file_count: item.file_count, total_bytes: item.total_bytes,
        ...(item.message === undefined ? {} : { message: item.message === null ? null : requiredText(item.message) }) };
    }) };
}
