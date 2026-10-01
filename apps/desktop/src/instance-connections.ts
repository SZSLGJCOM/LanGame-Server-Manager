import { isWildcardBindAddress } from "./bind-addresses";
import type { LocaleCode, TranslateFn } from "./i18n";
import type { BindAddressCandidate, InstanceDetails } from "./types";
import { buildShareEndpoints, primaryPort, type ShareEndpoint } from "./view-models";
import { resolveSelectedJoinEndpoint } from "./views/settings/instance-connectivity-selection";

export type InstanceConnectionDetails = Pick<InstanceDetails, "summary" | "ports" | "settings_json">;

export function resolveInstanceConnection(
  details: InstanceConnectionDetails,
  candidates: BindAddressCandidate[],
  preferredAddress: string | null,
  locale: LocaleCode,
  t: TranslateFn
): ShareEndpoint | null {
  const endpoints = buildShareEndpoints(details, candidates, locale, t);
  const port = primaryPort(details);
  return resolveSelectedJoinEndpoint(endpoints.filter((endpoint) => endpoint.kind === "relay"
    || (!isWildcardBindAddress(endpoint.address) && port !== null && Number.isInteger(port.port)
      && port.port > 0 && port.port <= 65535)), preferredAddress);
}

export function readPreferredJoinAddress(instanceId: string): string | null {
  try {
    return window.localStorage.getItem(`langame.join-address.${instanceId}`);
  } catch {
    return null;
  }
}
