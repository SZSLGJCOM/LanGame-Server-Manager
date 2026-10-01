import sourcePolicy from "../../../crates/app-network/official-sources.json";
import requestPolicy from "../../../crates/app-network/media-request-policy.json";
import { isChineseLocale, type LocaleCode } from "./i18n-config";
import { readPreferredLocale } from "./locale-preference";

export type MediaPurpose = "image" | "video";
export interface OfficialSourcePolicy {
  schemaVersion: number;
  chinaOrigins?: string[];
  groups: {
    id: string; origins: string[]; pathPrefixes: string[]; exactPaths?: string[]; allowedQueryKeys?: string[]; purposes: string[];
    purposePathPrefixes?: Record<string, string[]>; purposePathSuffixes?: Record<string, string[]>;
  }[];
  exactResources: { id: string; urls: string[]; purposes: string[] }[];
}
const officialSources: OfficialSourcePolicy = sourcePolicy;

const MAX_CANDIDATES = 4;
const MAX_PREFERENCES = 128;
const PREFERENCE_TTL_MS = 10 * 60 * 1000;
export const MEDIA_SOURCE_COOLDOWN_MS = 30 * 1000;
export const MEDIA_SOURCE_TIMEOUT_MS = requestPolicy.directSourceTimeoutMs;

// The resolver only changes a source into a local cache endpoint. Its server budget
// includes cache locks, worker admission and origin fallback; allow one direct-load
// window afterwards for the response to cross the desktop protocol or LAN socket.
export function mediaRequestTimeoutMs(source: string, resolvedSource: string, directTimeoutMs = MEDIA_SOURCE_TIMEOUT_MS): number {
  return directTimeoutMs + (resolvedSource === source ? 0 : requestPolicy.cacheRequestBudgetMs);
}
interface SourceHistory { source?: string; expiresAt: number; failures: Map<string, number> }
const preferences = new Map<string, SourceHistory>();

export interface MediaOriginAffinity {
  preferredOrigin?: string;
  unavailableOrigins?: ReadonlyMap<string, number>;
}

function remoteUrl(source: string): URL | null {
  try {
    const url = new URL(source);
    return url.protocol === "https:" && !url.username && !url.password && !url.port ? url : null;
  } catch {
    return null;
  }
}

function mediaGroup(url: URL, purpose: MediaPurpose, policy: OfficialSourcePolicy) {
  const queryKeys = [...url.searchParams.keys()];
  if (new Set(queryKeys).size !== queryKeys.length) return undefined;
  return policy.groups.find((entry) => entry.purposes.includes(purpose) &&
    (!entry.purposePathPrefixes?.[purpose] || entry.purposePathPrefixes[purpose].some((prefix) => url.pathname.startsWith(prefix))) &&
    (!entry.purposePathSuffixes?.[purpose] || entry.purposePathSuffixes[purpose].some((suffix) => url.pathname.endsWith(suffix))) &&
    entry.origins.includes(url.origin) &&
    (entry.exactPaths?.includes(url.pathname) || entry.pathPrefixes.some((prefix) => url.pathname.startsWith(prefix))) &&
    queryKeys.every((key) => entry.allowedQueryKeys?.includes(key)));
}

export function officialMediaCandidates(
  source: string | null | undefined,
  purpose: MediaPurpose,
  policy: OfficialSourcePolicy = officialSources
): string[] {
  const candidate = source?.trim();
  if (!candidate) return [];
  const url = remoteUrl(candidate);
  if (!url) return purpose === "image" && /^\/(?!\/)/.test(candidate) ? [candidate] : [];
  const original = url.toString();
  if (policy.schemaVersion !== 1 || url.hash) return [original];
  const exact = policy.exactResources.find((resource) =>
    resource.purposes.includes(purpose) && resource.urls.includes(original));
  const group = mediaGroup(url, purpose, policy);
  const alternatives = exact?.urls ?? group?.origins.map((origin) => `${origin}${url.pathname}${url.search}${url.hash}`) ?? [];
  return [...new Set([original, ...alternatives.filter((value) => remoteUrl(value))])].slice(0, MAX_CANDIDATES);
}

export function isOfficialMediaUrl(source: string, purpose: MediaPurpose): boolean {
  const url = remoteUrl(source);
  return Boolean(url && !url.hash && (mediaGroup(url, purpose, officialSources) ||
    officialSources.exactResources.some((entry) => entry.purposes.includes(purpose) && entry.urls.includes(url.toString()))));
}

// Keep different assets in caller order: a cached cover must never replace a current screenshot.
function resourceGroups(sources: readonly string[]): string[][] {
  const remaining = new Set(sources);
  const groups: string[][] = [];
  for (const source of sources) {
    if (!remaining.has(source)) continue;
    const equivalents = new Set([
      ...officialMediaCandidates(source, "image"), ...officialMediaCandidates(source, "video")
    ]);
    const group = sources.filter((candidate) => remaining.has(candidate) && (candidate === source || equivalents.has(candidate)));
    for (const candidate of group) remaining.delete(candidate);
    groups.push(group);
  }
  return groups;
}

function historyKey(sources: readonly string[], locale: LocaleCode) {
  return `${locale}\u0000${[...sources].sort().join("\u0000")}`;
}

export function preferredMediaSources(
  sources: readonly string[], now = Date.now(), locale: LocaleCode = readPreferredLocale(), affinity: MediaOriginAffinity = {}
): string[] {
  return resourceGroups(sources).flatMap((group) => {
    if (group.length < 2) return group;
    const key = historyKey(group, locale);
    const history = preferences.get(key);
    if (history && history.expiresAt <= now) preferences.delete(key);
    const current = history && history.expiresAt > now ? history : undefined;
    const rank = (source: string) => {
      const origin = new URL(source).origin;
      const cooling = Math.max(current?.failures.get(source) ?? 0, affinity.unavailableOrigins?.get(origin) ?? 0) > now;
      const regional = Boolean(officialSources.chinaOrigins?.includes(origin)) === isChineseLocale(locale);
      const successful = source === current?.source || origin === affinity.preferredOrigin;
      return Number(cooling) * 4 + Number(!regional) * 2 + Number(!successful);
    };
    return [...group].sort((left, right) => rank(left) - rank(right));
  });
}

function remember(sources: readonly string[], source: string, now: number, locale: LocaleCode, failed: boolean): void {
  const group = resourceGroups(sources).find((candidates) => candidates.includes(source));
  if (!group || group.length < 2) return;
  const key = historyKey(group, locale);
  const previous = preferences.get(key);
  const history: SourceHistory = previous && previous.expiresAt > now ? previous : { expiresAt: 0, failures: new Map() };
  for (const [candidate, until] of history.failures) if (until <= now) history.failures.delete(candidate);
  if (failed) history.failures.set(source, now + MEDIA_SOURCE_COOLDOWN_MS);
  else { history.source = source; history.failures.delete(source); }
  history.expiresAt = now + PREFERENCE_TTL_MS;
  preferences.delete(key);
  preferences.set(key, history);
  while (preferences.size > MAX_PREFERENCES) {
    const oldest = preferences.keys().next().value;
    if (oldest === undefined) break;
    preferences.delete(oldest);
  }
}

export function rememberMediaSource(sources: readonly string[], source: string, now = Date.now(), locale: LocaleCode = readPreferredLocale()): void {
  remember(sources, source, now, locale, false);
}

export function rememberMediaSourceFailure(sources: readonly string[], source: string, now = Date.now(), locale: LocaleCode = readPreferredLocale()): void {
  remember(sources, source, now, locale, true);
}

export function forgetMediaSourcePreferences(): void {
  preferences.clear();
}
