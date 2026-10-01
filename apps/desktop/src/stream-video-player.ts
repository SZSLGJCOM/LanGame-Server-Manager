import { createOfficialHlsLoader } from "./official-hls-loader";
import { MEDIA_SOURCE_TIMEOUT_MS, mediaRequestTimeoutMs, officialMediaCandidates, preferredMediaSources, rememberMediaSource, rememberMediaSourceFailure } from "./official-media-sources";
import type { LocaleCode } from "./i18n-config";
import { readPreferredLocale } from "./locale-preference";
import { createMediaCacheResolver } from "./media-cache";

interface StreamPlaybackOptions {
  streamUrl: string;
  fallbackStreamUrls?: readonly string[];
  autoPlay?: boolean;
  muted?: boolean;
  locale?: LocaleCode;
  initialPlayback?: { time: number; playing: boolean };
  onUnavailable: () => void;
}

export function attachStreamVideo(video: HTMLVideoElement, options: StreamPlaybackOptions): () => void {
  const { streamUrl, fallbackStreamUrls = [], autoPlay, muted, onUnavailable, locale = readPreferredLocale(), initialPlayback } = options;
  let disposed = false;
  let failed = false;
  let mediaReleased = false;
  let nativeAttempt = 0;
  let nativeLoaded = false;
  let resumeTime = initialPlayback?.time ?? 0;
  let resumePlaying = initialPlayback?.playing ?? false;
  let nativeCleanup = () => {};
  let sourceError = () => {};
  let hlsInstance: { stopLoad: () => void; destroy: () => void } | null = null;
  const disposeHls = () => {
    const instance = hlsInstance;
    hlsInstance = null;
    if (!instance) return;
    try { instance.stopLoad(); } finally { instance.destroy(); }
  };
  const releaseMedia = () => {
    if (mediaReleased) return;
    mediaReleased = true;
    video.pause();
    video.removeAttribute("src");
    video.load();
  };
  const fail = () => {
    if (disposed || failed) return;
    failed = true;
    nativeAttempt += 1;
    nativeCleanup();
    video.removeEventListener("error", onSourceError);
    disposeHls();
    releaseMedia();
    onUnavailable();
  };
  const reportPlaybackFailure = (error: unknown) => {
    if (disposed || failed) return;
    console.warn("Video autoplay failed.", error);
    fail();
  };
  const playWithPolicyFallback = () => {
    const attempt = nativeAttempt;
    void video.play().catch((error: unknown) => {
      if (disposed || failed || attempt !== nativeAttempt) return;
      if (muted === false || video.muted) {
        reportPlaybackFailure(error);
        return;
      }
      video.muted = true;
      void video.play().catch((nextError: unknown) => {
        if (attempt === nativeAttempt) reportPlaybackFailure(nextError);
      });
    });
  };
  const handleCanPlay = () => {
    if (!disposed && !failed && autoPlay) playWithPolicyFallback();
  };
  // Keep format order separate from CDN ranking: all equivalents of the primary
  // representation are attempted before the next declared format. Both lists are bounded.
  const seenFormats = new Set<string>();
  const sourceGroups = [...new Set([streamUrl, ...fallbackStreamUrls])].slice(0, 4).flatMap((source) => {
    const canonical = officialMediaCandidates(source, "video");
    const group = canonical.length ? canonical : [source];
    const key = [...group].sort().join("\u0000");
    if (seenFormats.has(key)) return [];
    seenFormats.add(key);
    return [group];
  });
  const sources = sourceGroups.flat();
  const candidates = preferredMediaSources(sources, Date.now(), locale);
  const resolveSource = createMediaCacheResolver("video", locale);
  const nextFormatIndex = (index: number) => {
    let boundary = 0;
    for (const group of sourceGroups) {
      boundary += group.length;
      if (index < boundary) return boundary;
    }
    return candidates.length;
  };
  const useNativeSource = (index = 0) => {
    nativeCleanup();
    const source = candidates[index];
    if (disposed || failed) return;
    if (!source) { fail(); return; }
    const generation = ++nativeAttempt;
    const active = () => !disposed && generation === nativeAttempt && !failed;
    if (nativeLoaded || (generation === 2 && !initialPlayback)) {
      resumeTime = Number.isFinite(video.currentTime) ? video.currentTime : 0;
      resumePlaying = !video.paused;
    }
    nativeLoaded = false;
    if (video.src) {
      video.pause();
      video.removeAttribute("src");
      video.load();
    }
    let activeUrl: string | null = null;
    let timer: ReturnType<typeof window.setTimeout> | undefined;
    let playable = false;
    const clearDeadline = () => { window.clearTimeout(timer); timer = undefined; };
    const advance = () => {
      if (!active()) return;
      if (activeUrl !== source) { resolveSource.disable(source); useNativeSource(index); }
      else { rememberMediaSourceFailure(sources, source, Date.now(), locale); useNativeSource(index + 1); }
    };
    const waitForPlayableData = () => {
      if (!active() || !activeUrl || timer !== undefined) return;
      timer = window.setTimeout(() => {
        advance();
      }, mediaRequestTimeoutMs(source, activeUrl));
    };
    const loaded = () => {
      if (!active() || !activeUrl || nativeLoaded) return;
      nativeLoaded = true;
      if (!autoPlay && !resumePlaying && video.paused && video.preload !== "auto") clearDeadline();
      if (resumeTime > 0) video.currentTime = Math.min(resumeTime, video.duration || resumeTime);
    };
    const ready = () => {
      if (!active() || !activeUrl) return;
      loaded();
      clearDeadline();
      rememberMediaSource(sources, source, Date.now(), locale);
      if (playable) return;
      playable = true;
      if (resumePlaying) playWithPolicyFallback(); else handleCanPlay();
    };
    const onPlay = () => {
      if (video.readyState < 3) waitForPlayableData();
    };
    const onWaiting = () => { if (!video.paused) waitForPlayableData(); };
    const onPause = () => { if (nativeLoaded && video.paused) clearDeadline(); };
    nativeCleanup = () => {
      clearDeadline();
      video.removeEventListener("loadedmetadata", loaded);
      video.removeEventListener("canplay", ready);
      video.removeEventListener("play", onPlay);
      video.removeEventListener("waiting", onWaiting);
      video.removeEventListener("pause", onPause);
    };
    sourceError = () => {
      if (!active() || !activeUrl) return;
      // A confirmed decode failure concerns the encoded representation. Code 4
      // also covers HTTP/source failures, so it must retain the ordinary CDN chain.
      if (video.error?.code === 3) useNativeSource(nextFormatIndex(index));
      else advance();
    };
    video.addEventListener("loadedmetadata", loaded);
    video.addEventListener("canplay", ready);
    video.addEventListener("play", onPlay);
    video.addEventListener("waiting", onWaiting);
    video.addEventListener("pause", onPause);
    void resolveSource(source).then((url) => {
      if (!active()) return;
      activeUrl = url;
      video.src = url;
      video.load();
      waitForPlayableData();
    });
  };
  const onSourceError = () => { if (!disposed && !failed) sourceError(); };
  video.addEventListener("error", onSourceError);
  const isHlsStream = /\.m3u8(?:[?#]|$)/i.test(streamUrl);
  if (isHlsStream) {
    const recoverHls = () => {
      if (disposed || failed) return;
      if (sourceGroups.length < 2) { fail(); return; }
      resumeTime = Number.isFinite(video.currentTime) ? video.currentTime : 0;
      resumePlaying = !video.paused;
      disposeHls();
      useNativeSource(sourceGroups[0].length);
    };
    sourceError = recoverHls;
    void import("hls.js").then((module) => {
      if (disposed || failed) return;
      const Hls = module.default;
      if (!Hls.isSupported()) { useNativeSource(); return; }
      const policy = { default: {
        maxTimeToFirstByteMs: MEDIA_SOURCE_TIMEOUT_MS,
        maxLoadTimeMs: MEDIA_SOURCE_TIMEOUT_MS,
        timeoutRetry: null, errorRetry: null
      } };
      const hls = new Hls({
        enableWorker: true,
        loader: createOfficialHlsLoader(Hls.DefaultConfig.loader, locale),
        manifestLoadPolicy: policy, playlistLoadPolicy: policy,
        fragLoadPolicy: policy, keyLoadPolicy: policy, certLoadPolicy: policy
      });
      hlsInstance = hls;
      hls.on(Hls.Events.MANIFEST_PARSED, () => {
        if (disposed || failed || hlsInstance !== hls) return;
        if (resumeTime > 0) { video.currentTime = resumeTime; resumeTime = 0; }
        if (resumePlaying) playWithPolicyFallback(); else handleCanPlay();
      });
      hls.on(Hls.Events.ERROR, (_event, data) => {
        if (data.fatal && !disposed && hlsInstance === hls) recoverHls();
      });
      hls.loadSource(streamUrl);
      hls.attachMedia(video);
    }).catch((error: unknown) => {
      if (disposed || failed) return;
      console.warn("HLS player initialization failed.", error);
      recoverHls();
    });
  } else {
    useNativeSource();
  }
  return () => {
    if (disposed) return;
    disposed = true;
    nativeAttempt += 1;
    nativeCleanup();
    video.removeEventListener("error", onSourceError);
    disposeHls();
    releaseMedia();
  };
}
