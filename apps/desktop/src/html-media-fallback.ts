import { mediaRequestTimeoutMs, officialMediaCandidates, preferredMediaSources, rememberMediaSource, rememberMediaSourceFailure } from "./official-media-sources";
import type { LocaleCode } from "./i18n-config";
import { readPreferredLocale } from "./locale-preference";
import { attachStreamVideo } from "./stream-video-player";
import { createMediaCacheResolver } from "./media-cache";

// Effect replay reuses DOM nodes after disposal removed their live src attributes.
// Weak ownership retains only the original references while those nodes still exist.
const imageReferences = new WeakMap<HTMLImageElement, string>();
const videoReferences = new WeakMap<HTMLVideoElement, {
  sources: string[]; poster: string | null; muted?: boolean; playback?: { time: number; playing: boolean }
}>();

function declaredVideoSources(video: HTMLVideoElement): string[] {
  const declared = [video.getAttribute("src"), ...Array.from(video.querySelectorAll<HTMLSourceElement>("source[src]"))
    .filter((candidate) => !candidate.type || video.canPlayType(candidate.type))
    .map((candidate) => candidate.getAttribute("src"))];
  return [...new Set(declared.filter((source): source is string => Boolean(source)))].slice(0, 4);
}

// Templates are inert: strip live references before insertion so the browser cannot
// race the locale-aware loader with a speculative request to the original host.
export function replaceHtmlMediaContent(container: HTMLElement, sanitizedHtml: string): void {
  const template = container.ownerDocument.createElement("template");
  template.innerHTML = sanitizedHtml;
  for (const image of template.content.querySelectorAll<HTMLImageElement>("img[src]")) {
    const source = image.getAttribute("src");
    if (!source || !officialMediaCandidates(source, "image").length) continue;
    imageReferences.set(image, source);
    image.removeAttribute("src");
  }
  for (const video of template.content.querySelectorAll<HTMLVideoElement>("video")) {
    const sources = declaredVideoSources(video);
    if (sources.length) videoReferences.set(video, { sources, poster: video.getAttribute("poster") });
    video.removeAttribute("src");
    if (officialMediaCandidates(video.getAttribute("poster"), "image").length) video.removeAttribute("poster");
    for (const child of video.querySelectorAll("source[src]")) child.removeAttribute("src");
  }
  container.replaceChildren(template.content);
}

function attachImageFallback(image: HTMLImageElement, locale: LocaleCode, onSource?: (source: string | null) => void): () => void {
  const original = imageReferences.get(image) ?? image.getAttribute("src");
  const sources = officialMediaCandidates(original, "image");
  if (!sources.length) return () => {};
  imageReferences.set(image, sources[0]);
  image.hidden = false;
  const candidates = preferredMediaSources(sources, Date.now(), locale);
  const resolveSource = createMediaCacheResolver("image", locale);
  let index = 0;
  let generation = 0;
  let activeUrl: string | null = null;
  let disposed = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const loaded = () => {
    if (disposed || !activeUrl || image.getAttribute("src") !== activeUrl) return;
    clearTimeout(timer);
    rememberMediaSource(sources, candidates[index], Date.now(), locale);
    onSource?.(activeUrl);
  };
  const next = () => {
    if (disposed || !activeUrl || image.getAttribute("src") !== activeUrl) return;
    if (activeUrl !== candidates[index]) resolveSource.disable(candidates[index]);
    else { rememberMediaSourceFailure(sources, candidates[index], Date.now(), locale); index += 1; }
    load();
  };
  const load = () => {
    clearTimeout(timer);
    const attempt = ++generation;
    activeUrl = null;
    image.removeAttribute("src");
    if (disposed) return;
    const source = candidates[index];
    if (!source) {
      image.removeAttribute("src");
      image.hidden = true;
      onSource?.(null);
      return;
    }
    void resolveSource(source).then((url) => {
      if (disposed || generation !== attempt) return;
      activeUrl = url;
      image.referrerPolicy = "no-referrer";
      image.src = url;
      if (image.complete && image.naturalWidth > 0) { loaded(); return; }
      timer = setTimeout(next, mediaRequestTimeoutMs(source, url));
    });
  };
  image.addEventListener("load", loaded);
  image.addEventListener("error", next);
  load();
  return () => {
    disposed = true;
    clearTimeout(timer);
    image.removeEventListener("load", loaded);
    image.removeEventListener("error", next);
    image.removeAttribute("src");
  };
}

// This binds only media in an already sanitized subtree; it never evaluates or copies markup.
export function attachHtmlMediaFallbacks(container: HTMLElement, locale: LocaleCode = readPreferredLocale()): () => void {
  const cleanups = Array.from(container.querySelectorAll<HTMLImageElement>("img")).map((image) => attachImageFallback(image, locale));
  for (const video of container.querySelectorAll<HTMLVideoElement>("video")) {
    const declaredSources = Array.from(video.querySelectorAll<HTMLSourceElement>("source[src]"));
    const previous = videoReferences.get(video);
    const sources = previous?.sources ?? declaredVideoSources(video);
    if (!sources.length && video.currentSrc) sources.push(video.currentSrc);
    if (!sources.length) continue;
    const poster = previous?.poster ?? video.getAttribute("poster");
    // Parsing the muted attribute sets defaultMuted; synchronize the live flag
    // before autoplay, while preserving a user's later choice across effect replay.
    const muted = previous?.muted ?? Boolean(video.defaultMuted || video.muted);
    video.muted = muted;
    const reference = { sources, poster, muted, playback: previous?.playback };
    videoReferences.set(video, reference);
    // Removing child sources prevents video.load() during disposal from restarting the original request.
    for (const declared of declaredSources) declared.removeAttribute("src");
    if (poster && officialMediaCandidates(poster, "image").length) {
      const image = new Image();
      imageReferences.set(image, poster);
      cleanups.push(attachImageFallback(image, locale, (resolved) => {
        if (resolved) video.poster = resolved; else video.removeAttribute("poster");
      }));
    }
    const detach = attachStreamVideo(video, {
      streamUrl: sources[0], fallbackStreamUrls: sources.slice(1), autoPlay: video.autoplay, muted, locale,
      initialPlayback: reference.playback,
      onUnavailable() { video.pause(); }
    });
    cleanups.push(() => {
      reference.playback = { time: Number.isFinite(video.currentTime) ? video.currentTime : 0, playing: !video.paused };
      reference.muted = video.muted;
      detach();
    });
  }
  return () => { for (const cleanup of cleanups) cleanup(); };
}
