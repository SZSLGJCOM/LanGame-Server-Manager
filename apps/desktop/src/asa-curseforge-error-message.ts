import type { TranslateFn } from "./i18n";

const STAGES = ["query", "install", "validation"];
const REASONS = [
  "server_unreachable", "authentication", "access_denied", "request", "not_found", "expired",
  "response", "cancelled", "unknown", "missing_mods", "unavailable_mod", "missing_file", "timeout", "http", "process_exited"
];
const CONTEXT = ["instance_id", "map_key", "before_process_key", "native_log_path", "cleanup_error"] as const;

/** Render only the native startup error contract; response bodies remain in native diagnostics. */
export function formatAsaCurseForgeError(t: TranslateFn, value: Record<string, unknown>): string | null {
  if (typeof value.stage !== "string" || !STAGES.includes(value.stage)
    || typeof value.reason !== "string" || !REASONS.includes(value.reason)
    || (value.reason === "http"
      ? typeof value.http_status !== "number" || !Number.isInteger(value.http_status)
        || value.http_status < 100 || value.http_status > 599
      : value.http_status !== null)
    || CONTEXT.some((key) => value[key] != null && typeof value[key] !== "string")
    || ["cache_verified", "native_process_exited"].some((key) => value[key] != null && typeof value[key] !== "boolean")) return null;

  const connectionAdvice = value.reason === "server_unreachable"
    || value.reason === "http" && typeof value.http_status === "number"
      && (value.http_status === 429 || value.http_status >= 500);
  const lines = [
    t(`errors.asaCurseForge.stage.${value.stage}`),
    t(`errors.asaCurseForge.reason.${value.reason}`, {
      status: typeof value.http_status === "number" ? value.http_status : ""
    }),
    t(connectionAdvice ? "errors.asaCurseForge.checkConnection" : "errors.asaCurseForge.checkLog"),
    t(value.cache_verified === true ? "errors.asaCurseForge.cacheVerified"
      : value.cache_verified === false ? "errors.asaCurseForge.cacheUnavailable" : "errors.asaCurseForge.cacheUnchecked")
  ];
  if (typeof value.native_process_exited === "boolean") {
    lines.push(t(value.native_process_exited ? "errors.asaCurseForge.nativeExited" : "errors.asaCurseForge.nativeNotExited"));
  }
  const cleanupFailed = typeof value.cleanup_error === "string" && value.cleanup_error.trim().length > 0;
  lines.push(t(cleanupFailed ? "errors.asaCurseForge.cleanupUnconfirmed" : "errors.asaCurseForge.startAborted"));
  for (const key of CONTEXT) {
    const context = value[key];
    if (typeof context === "string" && context.trim()) {
      lines.push(t(`errors.asaCurseForge.context.${key}`, { value: context }));
    }
  }
  return lines.join("\n");
}
