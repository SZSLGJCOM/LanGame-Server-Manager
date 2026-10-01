import { selectLocaleText } from "./i18n";
import type { BindAddressCandidate } from "./types";

export const DEFAULT_BIND_ADDRESS = "0.0.0.0";
export const DEFAULT_BIND_ADDRESS_CANDIDATE: BindAddressCandidate = {
  address: DEFAULT_BIND_ADDRESS,
  kind: "all",
  adapter_name: null,
  family_name: null
};

export function normalizeBindAddressCandidates(candidates: BindAddressCandidate[]) {
  const seen = new Set<string>();
  const normalized = candidates
    .map((candidate) => ({
      ...candidate,
      address: String(candidate.address ?? "").trim()
    }))
    .filter((candidate) => candidate.address.length > 0)
    .filter((candidate) => {
      if (seen.has(candidate.address)) {
        return false;
      }
      seen.add(candidate.address);
      return true;
    });

  if (!normalized.some((candidate) => candidate.address === DEFAULT_BIND_ADDRESS)) {
    normalized.unshift(DEFAULT_BIND_ADDRESS_CANDIDATE);
  }

  return normalized.length ? normalized : [DEFAULT_BIND_ADDRESS_CANDIDATE];
}

export function resolvePreferredBindIp(candidates: BindAddressCandidate[]) {
  return normalizeBindAddressCandidates(candidates).find((candidate) => candidate.address === DEFAULT_BIND_ADDRESS)?.address
    ?? candidates[0]?.address
    ?? DEFAULT_BIND_ADDRESS;
}

export function isWildcardBindAddress(address: string | null | undefined) {
  const normalized = String(address ?? "").trim();
  return normalized === "" || normalized === "0.0.0.0" || normalized === "::" || normalized === "[::]";
}

function looksLikeIpv4(address: string) {
  const parts = address.split(".");
  if (parts.length !== 4) {
    return false;
  }

  return parts.every((part) => /^\d+$/.test(part) && Number(part) >= 0 && Number(part) <= 255);
}

export function bindAddressCandidateKind(candidate: BindAddressCandidate | null | undefined) {
  return String(candidate?.kind ?? "").trim().toLowerCase();
}

export function isOverlayCandidate(candidate: BindAddressCandidate) {
  return String(candidate.kind ?? "").trim().toLowerCase() === "overlay";
}

export function isPublicCandidate(candidate: BindAddressCandidate) {
  return String(candidate.kind ?? "").trim().toLowerCase() === "public";
}

export function isProxyCandidate(candidate: BindAddressCandidate) {
  return String(candidate.kind ?? "").trim().toLowerCase() === "proxy";
}

export function isLanCandidate(candidate: BindAddressCandidate) {
  return String(candidate.kind ?? "").trim().toLowerCase() === "lan";
}

export function findBindAddressCandidateByKind(candidates: BindAddressCandidate[], kind: string): BindAddressCandidate | null {
  return candidates.find((candidate) => bindAddressCandidateKind(candidate) === kind) ?? null;
}

export function bindAddressCandidateLooksLikeProxyTun(candidate: BindAddressCandidate): boolean {
  const adapterName = String(candidate.adapter_name ?? "").trim();
  const address = String(candidate.address ?? "").trim();
  const [first, second] = address.split(".").map((part) => Number(part));
  return isProxyCandidate(candidate)
    || (first === 198 && second >= 18 && second <= 19)
    || /(^|[^a-z])(meta|clash|mihomo|proxy|tun)([^a-z]|$)/i.test(adapterName);
}

export function formatBindAddressCandidateSummary(candidate: BindAddressCandidate | null, fallback: string): string {
  if (!candidate) {
    return fallback;
  }
  const adapterName = String(candidate.adapter_name ?? candidate.family_name ?? "").trim();
  return adapterName ? `${adapterName} ${candidate.address}` : candidate.address;
}

export function inferBindAddressCandidate(address: string): BindAddressCandidate {
  const trimmed = String(address ?? "").trim();
  if (!trimmed) {
    return DEFAULT_BIND_ADDRESS_CANDIDATE;
  }

  if (isWildcardBindAddress(trimmed)) {
    return DEFAULT_BIND_ADDRESS_CANDIDATE;
  }

  if (!looksLikeIpv4(trimmed)) {
    return {
      address: trimmed,
      kind: "configured",
      adapter_name: null,
      family_name: null
    };
  }

  const [a, b] = trimmed.split(".").map((part) => Number(part));
  const isPrivate = a === 10 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168) || (a === 100 && b >= 64 && b <= 127);
  const isProxy = a === 198 && b >= 18 && b <= 19;

  return {
    address: trimmed,
    kind: isProxy ? "proxy" : isPrivate ? "lan" : "public",
    adapter_name: null,
    family_name: null
  };
}

export function buildBindAddressOptions(candidates: BindAddressCandidate[], currentValue?: string | null) {
  const normalized = normalizeBindAddressCandidates(candidates);
  const currentAddress = String(currentValue ?? "").trim();
  if (!currentAddress) {
    return normalized;
  }

  if (normalized.some((candidate) => candidate.address === currentAddress)) {
    return normalized;
  }

  return [inferBindAddressCandidate(currentAddress), ...normalized];
}

export function formatBindAddressCandidateLabel(candidate: BindAddressCandidate, locale: string) {
  const address = String(candidate.address ?? "").trim();
  const adapterName = String(candidate.adapter_name ?? "").trim();
  const familyName = String(candidate.family_name ?? "").trim();
  const dot = "\u00b7";

  if (candidate.kind === "configured") {
    return `${address} ${dot} ${selectLocaleText(locale, "\u5df2\u4fdd\u5b58\u5730\u5740", "Saved address")}`;
  }

  if (candidate.kind === "all") {
    return `${address} ${dot} ${selectLocaleText(
      locale,
      "\u76d1\u542c\u5168\u90e8\u7f51\u5361\uff08\u63a8\u8350\uff09",
      "Listen on all adapters (recommended)"
    )}`;
  }

  if (isOverlayCandidate(candidate)) {
    const overlayLabel = familyName || adapterName || selectLocaleText(locale, "\u8054\u673a\u7f51\u7edc", "Overlay network");
    return `${address} ${dot} ${overlayLabel}`;
  }

  if (isPublicCandidate(candidate)) {
    return `${address} ${dot} ${selectLocaleText(locale, "\u516c\u7f51 IPv4", "Public IPv4")}`;
  }

  if (isProxyCandidate(candidate)) {
    const adapterLabel = adapterName ? ` / ${adapterName}` : "";
    return `${address} ${dot} ${selectLocaleText(locale, "\u4ee3\u7406/TUN \u865a\u62df\u7f51\u5361", "Proxy/TUN virtual adapter")}${adapterLabel}`;
  }

  if (isLanCandidate(candidate) && adapterName) {
    return `${address} ${dot} ${selectLocaleText(locale, "\u5c40\u57df\u7f51", "LAN")} / ${adapterName}`;
  }

  return `${address} ${dot} ${selectLocaleText(locale, "\u5c40\u57df\u7f51", "LAN")}`;
}
