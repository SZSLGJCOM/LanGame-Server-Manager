import { isTauri } from "@tauri-apps/api/core";
import { registerMediaCacheSource } from "./api";
import { officialMediaCandidates } from "./official-media-sources";
import type { LocaleCode } from "./i18n-config";

export type MediaKind = "image" | "video" | "hls";
const REGISTRATION_TIMEOUT_MS = 3000;

function cacheCandidates(source: string, kind: MediaKind): string[] {
  return officialMediaCandidates(source, kind === "image" ? "image" : "video");
}

export async function resolveMediaCacheSource(source: string, kind: MediaKind, locale: LocaleCode): Promise<string> {
  if (cacheCandidates(source, kind).length < 2) return source;
  const desktop = isTauri();
  if (desktop && kind !== "video") return `http://lgsm-media.localhost/?url=${encodeURIComponent(source)}&kind=${kind}&locale=${locale}`;
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    const registered = await Promise.race([
      registerMediaCacheSource(source, kind, locale),
      new Promise<null>((resolve) => { timer = setTimeout(() => resolve(null), REGISTRATION_TIMEOUT_MS); })
    ]);
    // A registration can only grant the local opaque endpoint, never another host or a token URL.
    if (!registered || !/^\/__langame\/media\/[a-zA-Z0-9_-]+$/.test(registered)) return source;
    // Native Range playback requires a registered version lease independent of disk LRU eviction.
    return desktop ? `http://lgsm-media.localhost${registered}` : registered;
  } catch {
    // The disk cache is optional; registration failure leaves the bounded direct-source path available.
    return source;
  } finally {
    clearTimeout(timer);
  }
}

// Owned by one image/video lifecycle (or one HLS load), not by the application.
// All CDN aliases share one attempt, while a different fallback asset gets its own.
export function createMediaCacheResolver(kind: MediaKind, locale: LocaleCode) {
  const attempted = new Map<string, Promise<string | null>>();
  const disabled = new Set<string>();
  const resourceKey = (source: string) => cacheCandidates(source, kind).sort().join("\u0000");
  const resolve = async (source: string): Promise<string> => {
    const candidates = cacheCandidates(source, kind);
    if (candidates.length < 2) return source;
    const key = resourceKey(source);
    if (disabled.has(key)) return source;
    let pending = attempted.get(key);
    if (!pending) {
      pending = resolveMediaCacheSource(source, kind, locale).then((url) => url === source ? null : url);
      attempted.set(key, pending);
    }
    return (await pending) ?? source;
  };
  // Replayed effects may share pending registration. Only an actual cache failure disables it.
  return Object.assign(resolve, { disable(source: string) { disabled.add(resourceKey(source)); } });
}
