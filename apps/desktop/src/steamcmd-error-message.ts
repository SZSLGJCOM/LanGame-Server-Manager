import type { TranslateFn } from "./i18n";

function withDiagnostic(t: TranslateFn, summary: string, diagnostic: string): string {
  return diagnostic ? `${summary}\n${t("errors.originalDiagnostic")}\n${diagnostic}` : summary;
}

export function formatSteamCmdError(t: TranslateFn, value: Record<string, unknown>): string | null {
  if (typeof value.message !== "string"
    || (value.output_excerpt != null && typeof value.output_excerpt !== "string")) return null;
  switch (value.code) {
    case "steamcmd_prepare_progress_lost":
      return t("errors.steamcmdPrepareProgressLost");
    case "steamcmd_preparation_stalled":
      if (typeof value.timeout_seconds !== "number" || !Number.isSafeInteger(value.timeout_seconds)
        || value.timeout_seconds < 0 || typeof value.output_excerpt !== "string") return null;
      return withDiagnostic(t, t("errors.steamcmdPreparationStalled", { seconds: value.timeout_seconds }), value.output_excerpt);
    case "steamcmd_executable_missing":
      if (typeof value.path !== "string") return null;
      return t("errors.steamcmdExecutableMissing", { path: value.path });
    case "steamcmd_not_ready":
      if (typeof value.path !== "string") return null;
      return t("errors.steamcmdNotReady");
    case "installed_executable_missing":
      if (typeof value.module_id !== "string" || typeof value.path !== "string" || typeof value.operation !== "string") return null;
      return t("errors.installedExecutableMissing", { module: value.module_id, path: value.path });
    case "installation_verification_failed":
      if (typeof value.module_id !== "string" || typeof value.operation !== "string" || typeof value.detail !== "string") return null;
      return withDiagnostic(t, t("errors.installationVerificationFailed", { module: value.module_id }), value.detail);
    case "install_operation_timed_out":
      if (typeof value.operation !== "string" || typeof value.timeout_seconds !== "number"
        || !Number.isSafeInteger(value.timeout_seconds) || value.timeout_seconds < 0) return null;
      return withDiagnostic(t, t("errors.installOperationTimedOut", { seconds: value.timeout_seconds }), value.output_excerpt ?? "");
    case "steamcmd_root_unmanaged":
      if (typeof value.path !== "string") return null;
      return t("errors.steamcmdRootUnmanaged", { path: value.path });
    case "steamcmd_ownership_invalid":
      if (typeof value.path !== "string") return null;
      return t("errors.steamcmdOwnershipInvalid", { path: value.path });
    case "module_install_source_missing":
      if (typeof value.module_id !== "string") return null;
      return t("errors.moduleInstallSourceMissing", { module: value.module_id });
    case "module_install_spec_missing":
      if (typeof value.module_id !== "string") return null;
      return t("errors.moduleInstallSpecMissing", { module: value.module_id });
    case "module_process_spec_missing":
      if (typeof value.module_id !== "string") return null;
      return t("errors.moduleProcessSpecMissing", { module: value.module_id });
    case "steamcmd_command_failed":
      if (typeof value.output_excerpt !== "string") return null;
      return withDiagnostic(t, t("errors.steamcmdCommandFailed"), value.output_excerpt);
    case "steamcmd_prepare_failed":
      if (typeof value.output_excerpt !== "string") return null;
      return withDiagnostic(t, t("errors.steamcmdPrepareFailed"), value.output_excerpt);
    case "module_download_failed":
      if (typeof value.output_excerpt !== "string") return null;
      return withDiagnostic(t, t("errors.moduleDownloadFailed"), value.output_excerpt);
    default:
      return null;
  }
}
