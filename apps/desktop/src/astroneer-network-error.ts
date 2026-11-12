import type { TranslateFn } from "./i18n";

const SOURCES: Record<string, string> = { ipify: "ipify (api.ipify.org)", aws: "AWS (checkip.amazonaws.com)" };
const REASONS = new Set(["timeout", "connection", "http", "response", "request"]);
const SETTING_REASONS: Record<string, string> = {
  adapter_query_failed: "errors.astroneerNetwork.adapterQueryFailed",
  invalid_public_ip: "errors.astroneerNetwork.invalidPublicIp",
  invalid_connection_mode: "errors.astroneerNetwork.invalidConnectionMode",
  manual_address_required: "errors.astroneerNetwork.manualAddressRequired",
  radmin_unavailable: "errors.astroneerNetwork.radminUnavailable",
  radmin_address_unavailable: "errors.astroneerNetwork.radminAddressUnavailable",
  radmin_address_ambiguous: "errors.astroneerNetwork.radminAddressAmbiguous"
};

export function formatAstroneerNetworkError(t: TranslateFn, value: Record<string, unknown>): string | null {
  if (value.code !== "astroneer_network_preflight_failed" || typeof value.message !== "string") return null;
  if (typeof value.reason === "string" && Object.prototype.hasOwnProperty.call(SETTING_REASONS, value.reason)) {
    if (value.failures != null && (!Array.isArray(value.failures) || value.failures.length !== 0)) return null;
    return t(SETTING_REASONS[value.reason]);
  }
  if (value.reason !== "discovery_failed" || !Array.isArray(value.failures)
    || value.failures.length === 0 || value.failures.length > 2) return null;
  const sources = new Set<string>();
  const diagnostics: string[] = [];
  for (const failure of value.failures) {
    if (!failure || typeof failure !== "object" || Array.isArray(failure)
      || typeof failure.source !== "string" || !Object.prototype.hasOwnProperty.call(SOURCES, failure.source)
      || sources.has(failure.source) || typeof failure.reason !== "string" || !REASONS.has(failure.reason)
      || (failure.reason === "http"
        ? typeof failure.status !== "number" || !Number.isInteger(failure.status) || failure.status < 100 || failure.status > 599
        : failure.status != null)) return null;
    sources.add(failure.source);
    diagnostics.push(t("errors.astroneerNetwork.sourceFailure", {
      source: SOURCES[failure.source],
      reason: t(`errors.astroneerNetwork.reason.${failure.reason}`, { status: failure.status ?? "" })
    }));
  }
  return [t("errors.astroneerNetwork.discoveryFailed"), ...diagnostics,
    t("errors.astroneerNetwork.discoveryAdvice")].join("\n");
}
