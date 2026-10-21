import type { TranslateFn } from "./i18n";
import type { BackgroundJob, RuntimeStartWarning } from "./types";

export interface RuntimeStartNotice {
  id: string;
  instanceId: string;
  label: string;
  warnings: RuntimeStartWarning[];
}

const providers = new Set(["steam_workshop", "curse_forge", "thunderstore"]);
const codes = new Set(["mod_update_using_cache", "mod_update_source_required"]);
const reasons = new Set(["network", "authentication", "access_denied", "request", "not_found", "expired",
  "response", "http", "download", "install", "unknown"]);
const contextText = (value: unknown): value is string | null => value === null
  || typeof value === "string" && value.length <= 16_384 && !/[\u0000-\u001f\u007f]/.test(value);

/** Only closed native fields are rendered; backend message/response bodies are never displayed. */
export function readRuntimeStartWarnings(value: unknown): RuntimeStartWarning[] {
  if (!Array.isArray(value)) return [];
  return value.filter((entry): entry is RuntimeStartWarning => {
    if (!entry || typeof entry !== "object" || Array.isArray(entry)) return false;
    const warning = entry as Record<string, unknown>;
    if (warning.code === "world_initialization_incomplete") {
      return warning.provider === null && warning.reason === null
        && Array.isArray(warning.item_ids) && warning.item_ids.length === 0
        && typeof warning.message === "string" && contextText(warning.log_path)
        && (warning.log_path === null || !/^\w+:\/\//.test(warning.log_path))
        && warning.map_key === null && warning.http_status === null;
    }
    const maxItemId = warning.provider === "steam_workshop" ? 18_446_744_073_709_551_615n : 9_223_372_036_854_775_807n;
    return typeof warning.code === "string" && codes.has(warning.code)
      && typeof warning.provider === "string" && providers.has(warning.provider)
      && typeof warning.reason === "string" && reasons.has(warning.reason)
      && typeof warning.message === "string"
      && Array.isArray(warning.item_ids) && warning.item_ids.length <= 8192
      && warning.item_ids.every((id: unknown) => typeof id === "string" && (warning.provider === "thunderstore"
        ? /^[A-Za-z0-9_]{1,128}-[A-Za-z0-9_]{1,128}$/.test(id)
        : /^[1-9]\d{0,19}$/.test(id) && BigInt(id) <= maxItemId))
      && contextText(warning.map_key) && contextText(warning.log_path)
      && (warning.log_path === null || !/^\w+:\/\//.test(warning.log_path))
      && (warning.http_status === null || typeof warning.http_status === "number"
        && Number.isInteger(warning.http_status) && warning.http_status >= 100 && warning.http_status <= 599);
  });
}

export function runtimeStartWarningKey(warning: RuntimeStartWarning): string {
  return JSON.stringify([warning.code, warning.provider, warning.reason,
    [...new Set(warning.item_ids)].sort(), warning.map_key, warning.http_status]);
}

export function formatRuntimeStartWarning(t: TranslateFn, warning: RuntimeStartWarning, label: string) {
  if (warning.code === "world_initialization_incomplete") {
    const details = [t("runtime.startWarning.worldInitializationAdvice")];
    if (warning.log_path) details.push(t("runtime.startWarning.log", { value: warning.log_path }));
    return { text: t("runtime.startWarning.worldInitializationIncomplete", { name: label }), details: details.join("\n") };
  }
  const provider = t(`runtime.startWarning.provider.${warning.provider}`);
  const sourceRequired = warning.code === "mod_update_source_required";
  const text = t(sourceRequired ? "runtime.startWarning.sourceRequired" : "runtime.startWarning.startedUsingCache", { name: label, provider });
  const reason = warning.reason === "http" && warning.http_status === null ? "httpWithoutStatus" : warning.reason;
  const details = sourceRequired ? [t("runtime.startWarning.sourceRequiredAdvice")]
    : [t(`runtime.startWarning.reason.${reason}`, { status: warning.http_status ?? "" }),
      t("runtime.startWarning.latestUnconfirmed")];
  if (warning.item_ids.length) details.push(t("runtime.startWarning.items", { ids: warning.item_ids.join(", ") }));
  if (warning.map_key) details.push(t("runtime.startWarning.map", { value: warning.map_key }));
  if (warning.log_path) details.push(t("runtime.startWarning.log", { value: warning.log_path }));
  return { text, details: details.join("\n") };
}

/** Polling replaces job objects. Operation IDs, not object identity or wording, own notices. */
export function runtimeStartJobNotices(jobs: readonly BackgroundJob[]): RuntimeStartNotice[] {
  return jobs.filter((job) => job.kind === "StartInstance" && job.status === "Completed")
    .flatMap((job) => {
      const warnings = readRuntimeStartWarnings(job.warnings);
      return warnings.length ? [{ id: `start-job:${job.id}`, instanceId: job.target_id ?? "",
        label: job.label.startsWith("Autostart ") ? job.label.slice("Autostart ".length) : job.label, warnings }] : [];
    });
}
