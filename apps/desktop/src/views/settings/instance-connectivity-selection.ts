import type { ShareEndpoint } from "../../view-models";

export function resolveSelectedJoinEndpoint(
  endpoints: ShareEndpoint[],
  preferredAddress: string | null | undefined
): ShareEndpoint | null {
  const preferred = String(preferredAddress ?? "").trim();
  return endpoints.find((endpoint) => endpoint.address === preferred) ?? endpoints[0] ?? null;
}
