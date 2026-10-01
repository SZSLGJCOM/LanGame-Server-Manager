import type { HlsConfig, Loader, LoaderCallbacks, LoaderConfiguration, LoaderContext, LoaderStats } from "hls.js";
import { MEDIA_SOURCE_COOLDOWN_MS, MEDIA_SOURCE_TIMEOUT_MS, mediaRequestTimeoutMs, officialMediaCandidates, preferredMediaSources, rememberMediaSource, rememberMediaSourceFailure } from "./official-media-sources";
import type { LocaleCode } from "./i18n-config";
import { readPreferredLocale } from "./locale-preference";
import { createMediaCacheResolver } from "./media-cache";

type LoaderConstructor = HlsConfig["loader"];

function hasRetryAfter(details: unknown, delegate: Loader<LoaderContext>): boolean {
  if (details && typeof details === "object") {
    if ("getResponseHeader" in details && typeof details.getResponseHeader === "function") {
      return Boolean(details.getResponseHeader("Retry-After"));
    }
    if ("headers" in details && details.headers instanceof Headers) return details.headers.has("Retry-After");
  }
  return Boolean(delegate.getResponseHeader?.("Retry-After"));
}

function mediaOriginHeader(details: unknown, delegate: Loader<LoaderContext>): string | null {
  if (details && typeof details === "object" && "getResponseHeader" in details && typeof details.getResponseHeader === "function") {
    const header: unknown = details.getResponseHeader("X-LanGame-Media-Origin");
    if (typeof header === "string") return header;
  }
  return delegate.getResponseHeader?.("X-LanGame-Media-Origin") ?? null;
}

// A factory gives each video its own host preference; resources never share a loader or response.
export function createOfficialHlsLoader(BaseLoader: LoaderConstructor, locale: LocaleCode = readPreferredLocale()): LoaderConstructor {
  const preferredOrigins = new Map<string, { preferredOrigin?: string; unavailableOrigins: Map<string, number>; expiresAt: number }>();
  return class OfficialHlsLoader implements Loader<LoaderContext> {
    context: LoaderContext | null = null;
    private delegate: Loader<LoaderContext> | null;
    private lastStats: LoaderStats;
    private callbacks: LoaderCallbacks<LoaderContext> | null = null;
    private generation = 0;
    private completed = false;

    constructor(private hlsConfig: HlsConfig) {
      this.delegate = new BaseLoader(hlsConfig);
      this.lastStats = this.delegate.stats;
    }

    get stats() { return this.delegate?.stats ?? this.lastStats; }
    getCacheAge() { return this.delegate?.getCacheAge?.() ?? null; }
    getResponseHeader(name: string) { return this.delegate?.getResponseHeader?.(name) ?? null; }

    load(context: LoaderContext, config: LoaderConfiguration, callbacks: LoaderCallbacks<LoaderContext>) {
      this.context = context;
      this.callbacks = callbacks;
      const canonical = officialMediaCandidates(context.url, "video");
      // HLS may use an unlisted publisher host. Preserve its original request, without inventing alternates.
      const sources = canonical.length ? canonical : [context.url];
      const origins = [...new Set(sources.map((source) => new URL(source).origin))];
      const originKey = origins.slice().sort().join("\u0000");
      const previous = preferredOrigins.get(originKey);
      const affinity = previous && previous.expiresAt > Date.now() ? previous : {
        unavailableOrigins: new Map<string, number>(), expiresAt: Date.now() + 10 * 60 * 1000, preferredOrigin: undefined
      };
      if (origins.length > 1) {
        preferredOrigins.set(originKey, affinity);
        if (preferredOrigins.size > 16) {
          const oldest = preferredOrigins.keys().next().value;
          if (oldest !== undefined) preferredOrigins.delete(oldest);
        }
      }
      const candidates = preferredMediaSources(sources, Date.now(), locale, affinity);
      const resolveSource = createMediaCacheResolver("hls", locale);

      const attempt = (index: number) => {
        const candidate = candidates[index];
        if (!candidate || !this.context || this.completed) return;
        this.releaseDelegate();
        const generation = ++this.generation;
        const active = () => generation === this.generation && !this.completed && this.context === context;
        void resolveSource(candidate).then((requestUrl) => {
          if (!active()) return;
          const delegate = new BaseLoader(this.hlsConfig);
          this.delegate = delegate;
          let deliveredProgress = false;
          const cached = requestUrl !== candidate;
          const boundedConfig = {
            ...config, maxRetry: 0,
            loadPolicy: {
              ...config.loadPolicy, errorRetry: null, timeoutRetry: null,
              maxTimeToFirstByteMs: mediaRequestTimeoutMs(candidate, requestUrl,
                Math.min(config.loadPolicy.maxTimeToFirstByteMs || MEDIA_SOURCE_TIMEOUT_MS, MEDIA_SOURCE_TIMEOUT_MS)),
              maxLoadTimeMs: mediaRequestTimeoutMs(candidate, requestUrl,
                Math.min(config.loadPolicy.maxLoadTimeMs || MEDIA_SOURCE_TIMEOUT_MS, MEDIA_SOURCE_TIMEOUT_MS))
            }
          };
          const retry = () => {
            if (deliveredProgress) return false;
            if (cached) { resolveSource.disable(candidate); attempt(index); return true; }
            rememberMediaSourceFailure(sources, candidate, Date.now(), locale);
            affinity.unavailableOrigins.set(new URL(candidate).origin, Date.now() + MEDIA_SOURCE_COOLDOWN_MS);
            if (index + 1 >= candidates.length) return false;
            attempt(index + 1);
            return true;
          };
          delegate.load({ ...context, url: requestUrl }, boundedConfig, {
            onSuccess: (response, stats, _context, details) => {
              if (!active()) return;
              this.completed = true;
              const remoteHeader = cached ? mediaOriginHeader(details, delegate) : null;
              const remoteUrl = cached ? (remoteHeader && sources.includes(remoteHeader) ? remoteHeader : candidate) : response.url || candidate;
              rememberMediaSource(sources, remoteUrl, Date.now(), locale);
              if (origins.length > 1) {
                affinity.preferredOrigin = new URL(remoteUrl).origin;
                affinity.unavailableOrigins.delete(affinity.preferredOrigin);
                affinity.expiresAt = Date.now() + 10 * 60 * 1000;
              }
              // The actual URL is required for relative child playlists and fragment paths.
              callbacks.onSuccess({ ...response, url: remoteUrl }, stats, context, details);
            },
            onError: (error, _context, details, stats) => {
              if (!active()) return;
              const stop = [401, 403, 429].includes(error.code) || hasRetryAfter(details, delegate);
              if (!stop && retry()) return;
              this.completed = true;
              callbacks.onError(error, context, details, stats);
            },
            onTimeout: (stats, _context, details) => {
              if (!active() || retry()) return;
              this.completed = true;
              callbacks.onTimeout(stats, context, details);
            },
            onProgress: (stats, _context, data, details) => {
              if (!active()) return;
              deliveredProgress = true;
              callbacks.onProgress?.(stats, context, data, details);
            }
          });
        });
      };
      attempt(0);
    }

    abort() {
      if (this.completed) return;
      this.completed = true;
      this.generation += 1;
      this.delegate?.abort();
      if (this.context) this.callbacks?.onAbort?.(this.stats, this.context, null);
    }

    destroy() {
      this.completed = true;
      this.generation += 1;
      this.context = null;
      this.callbacks = null;
      this.releaseDelegate();
    }

    private releaseDelegate() {
      const delegate = this.delegate;
      this.delegate = null;
      if (!delegate) return;
      this.lastStats = delegate.stats;
      delegate.destroy();
    }
  };
}
