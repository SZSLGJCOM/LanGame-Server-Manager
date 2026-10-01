import type { ModuleInstallDetails, ModuleSummary, SteamCmdStatus } from "./types";

export function moduleRequiresSteamCmd(
  module: Pick<ModuleSummary, "steam_app_id">,
  install: ModuleInstallDetails | null | undefined
): boolean {
  if (install?.source === "minecraft_java" || install?.download_url_windows != null) return false;
  return (module.steam_app_id ?? 0) > 0;
}

export type SteamCmdInstallBlockReason = "busy" | "unknown" | "missing" | "not_ready";

export function steamCmdInstallBlockReason(
  status: Pick<SteamCmdStatus, "ready" | "executable_exists"> | null,
  busy: boolean
): SteamCmdInstallBlockReason | null {
  if (busy) return "busy";
  if (!status) return "unknown";
  if (!status.executable_exists) return "missing";
  return status.ready ? null : "not_ready";
}
