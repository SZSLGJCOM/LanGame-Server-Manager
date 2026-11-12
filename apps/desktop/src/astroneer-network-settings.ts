import type { BindAddressCandidate } from "./types";

export type AstroneerConnectionMode = "public" | "radmin" | "manual";
type Settings = Readonly<Record<string, unknown>>;

/** Address eligibility only; matches astroneer_public_ip.rs, not a reachability check. */
export function parseAstroneerAdvertisedAddress(value: unknown): string {
  if (typeof value !== "string" || value.trim() !== value
    || !/^(?:0|[1-9]\d{0,2})(?:\.(?:0|[1-9]\d{0,2})){3}$/.test(value)) return "";
  const [a, b, c, d] = value.split(".").map(Number);
  if ([a, b, c, d].some((part) => part > 255)
    || a === 0 || a === 127 || (a === 169 && b === 254)
    || (a === 192 && b === 0 && c === 0 && d !== 9 && d !== 10)
    || (a === 192 && b === 0 && c === 2) || (a === 192 && b === 88 && c === 99 && d === 2)
    || (a === 198 && (b === 18 || b === 19))
    || (a === 198 && b === 51 && c === 100) || (a === 203 && b === 0 && c === 113) || a >= 224) return "";
  return value;
}

function readSettings(settingsJson: string): Settings | null {
  try {
    const value: unknown = JSON.parse(settingsJson);
    return value !== null && typeof value === "object" && !Array.isArray(value) ? value as Settings : null;
  } catch { return null; }
}

export function readAstroneerAdvertisedAddress(settingsJson: string): string {
  return parseAstroneerAdvertisedAddress(readSettings(settingsJson)?.public_ip);
}

export function resolveAstroneerConnectionMode(settings: Settings): AstroneerConnectionMode | null {
  if (settings.connection_mode === undefined) {
    return typeof settings.public_ip === "string" && settings.public_ip.trim() ? "manual" : "public";
  }
  return settings.connection_mode === "public" || settings.connection_mode === "radmin" || settings.connection_mode === "manual"
    ? settings.connection_mode : null;
}

export function readAstroneerConnectionMode(settingsJson: string): AstroneerConnectionMode | null {
  const settings = readSettings(settingsJson);
  return settings ? resolveAstroneerConnectionMode(settings) : null;
}

export function astroneerRadminCandidates(candidates: readonly BindAddressCandidate[]): BindAddressCandidate[] {
  const seen = new Set<string>();
  return candidates.filter((candidate) => {
    const radmin = candidate.kind === "overlay" && candidate.family_name === "Radmin VPN";
    if (!radmin || !parseAstroneerAdvertisedAddress(candidate.address) || seen.has(candidate.address)) return false;
    seen.add(candidate.address);
    return true;
  });
}

export function astroneerJoinAddressCandidates(candidates: readonly BindAddressCandidate[]): BindAddressCandidate[] {
  const seen = new Set<string>();
  return candidates.filter((candidate) => {
    if (candidate.kind === "proxy" || !parseAstroneerAdvertisedAddress(candidate.address) || seen.has(candidate.address)) return false;
    seen.add(candidate.address);
    return true;
  });
}

/** null selects startup discovery; an empty custom draft must be completed. */
export function buildAstroneerJoinAddressPatch(
  address: string | null, candidates: readonly BindAddressCandidate[], knownRadminAddress = ""
): { connection_mode: AstroneerConnectionMode; public_ip: string } {
  return { connection_mode: address === null ? "public"
    : (address && address === knownRadminAddress) || astroneerRadminCandidates(candidates).some((candidate) => candidate.address === address) ? "radmin" : "manual",
  public_ip: address ?? "" };
}

export function astroneerNetworkDraftMatchesPersisted(settingsJson: string, draft: Settings): boolean {
  const persisted = readSettings(settingsJson);
  return persisted !== null && persisted.connection_mode === draft.connection_mode && persisted.public_ip === draft.public_ip;
}

export function astroneerNetworkSettingsIssue(settings: Settings): {
  fieldKey: "public_ip";
  reason: "invalidMode" | "invalidAddress" | "manualRequired" | "radminRequired";
} | null {
  const mode = resolveAstroneerConnectionMode(settings);
  if (mode === null) return { fieldKey: "public_ip", reason: "invalidMode" };
  const empty = settings.public_ip === undefined || (typeof settings.public_ip === "string" && !settings.public_ip.trim());
  if (empty) return mode === "public" ? null
    : { fieldKey: "public_ip", reason: mode === "manual" ? "manualRequired" : "radminRequired" };
  return parseAstroneerAdvertisedAddress(settings.public_ip) ? null : { fieldKey: "public_ip", reason: "invalidAddress" };
}
