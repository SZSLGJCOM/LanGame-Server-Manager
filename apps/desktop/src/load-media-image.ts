import { mediaRequestTimeoutMs, preferredMediaSources, rememberMediaSource, rememberMediaSourceFailure } from "./official-media-sources";
import type { LocaleCode } from "./i18n-config";
import { readPreferredLocale } from "./locale-preference";
import { createMediaCacheResolver } from "./media-cache";

export function loadMediaImage(
  sources: readonly string[],
  onLoad: (image: HTMLImageElement, source: string) => void,
  onUnavailable: () => void,
  locale: LocaleCode = readPreferredLocale()
): () => void {
  const candidates = preferredMediaSources(sources, Date.now(), locale);
  const resolveSource = createMediaCacheResolver("image", locale);
  let disposed = false;
  let generation = 0;
  let activeImage: HTMLImageElement | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const release = () => {
    clearTimeout(timer);
    if (activeImage) {
      activeImage.onload = null;
      activeImage.onerror = null;
      activeImage.removeAttribute("src");
      activeImage = null;
    }
  };
  const load = (index: number) => {
    release();
    const attempt = ++generation;
    if (disposed) return;
    const source = candidates[index];
    if (!source) {
      onUnavailable();
      return;
    }
    void resolveSource(source).then((url) => {
      if (disposed || generation !== attempt) return;
      const image = new Image();
      activeImage = image;
      image.crossOrigin = "anonymous";
      image.referrerPolicy = "no-referrer";
      image.onload = () => {
        if (disposed || activeImage !== image) return;
        clearTimeout(timer);
        image.onload = null;
        image.onerror = null;
        activeImage = null;
        rememberMediaSource(sources, source, Date.now(), locale);
        onLoad(image, source);
      };
      const failed = () => {
        if (disposed || activeImage !== image) return;
        if (url !== source) { resolveSource.disable(source); load(index); }
        else { rememberMediaSourceFailure(sources, source, Date.now(), locale); load(index + 1); }
      };
      image.onerror = failed;
      timer = setTimeout(failed, mediaRequestTimeoutMs(source, url));
      image.src = url;
    });
  };
  load(0);
  return () => {
    disposed = true;
    release();
  };
}
