import { useEffect, useMemo, useRef, useState } from "react";
import { mediaRequestTimeoutMs, preferredMediaSources, rememberMediaSource, rememberMediaSourceFailure } from "../official-media-sources";
import { useI18n } from "../i18n";
import { createMediaCacheResolver } from "../media-cache";

export function useMediaSource(sources: readonly string[]) {
  const { locale } = useI18n();
  const key = `${locale}\u0000${sources.join("\u0000")}`;
  const candidates = useMemo(() => preferredMediaSources(sources, Date.now(), locale), [key]);
  const resolveSource = useMemo(() => createMediaCacheResolver("image", locale), [key]);
  const [failure, setFailure] = useState({ key: "", index: 0, direct: false });
  const index = failure.key === key ? failure.index : 0;
  const direct = failure.key === key && failure.direct;
  const remote = candidates[index] ?? null;
  const requestKey = `${key}\u0001${index}\u0001${direct}`;
  const [resolved, setResolved] = useState<{ key: string; src: string } | null>(null);
  const src = resolved?.key === requestKey ? resolved.src : null;
  const current = useRef({ key: requestKey, src, active: true });
  current.current = { key: requestKey, src, active: true };
  const imageRef = useRef<HTMLImageElement | null>(null);
  const loaded = useRef<string | null>(null);

  useEffect(() => {
    if (!remote) return;
    let cancelled = false;
    void (direct ? Promise.resolve(remote) : resolveSource(remote)).then((url) => {
      if (!cancelled) setResolved({ key: requestKey, src: url });
    });
    return () => { cancelled = true; };
  }, [remote, requestKey, resolveSource]);

  function advance() {
    if (!src || !remote || !current.current.active || current.current.key !== requestKey || current.current.src !== src) return;
    const cached = src !== remote;
    if (cached) resolveSource.disable(remote);
    if (!cached) rememberMediaSourceFailure(sources, remote, Date.now(), locale);
    setFailure((previous) => {
      if (!current.current.active || current.current.key !== requestKey || current.current.src !== src) return previous;
      const previousIndex = previous.key === key ? previous.index : 0;
      const previousDirect = previous.key === key && previous.direct;
      if (previousIndex !== index || previousDirect !== direct) return previous;
      return cached ? { key, index, direct: true } : { key, index: index + 1, direct: false };
    });
  }

  useEffect(() => {
    if (!src || !remote) return;
    current.current.active = true;
    let timer: ReturnType<typeof window.setTimeout> | undefined;
    const startTimeout = () => {
      if (timer !== undefined || !current.current.active || current.current.key !== requestKey || current.current.src !== src) return;
      timer = window.setTimeout(() => {
        if (loaded.current !== `${requestKey}\u0002${src}`) advance();
      }, mediaRequestTimeoutMs(remote, src));
    };
    const element = imageRef.current;
    let observer: IntersectionObserver | undefined;
    if (element?.loading === "lazy" && typeof IntersectionObserver !== "undefined") {
      observer = new IntersectionObserver((entries) => {
        if (!entries.some((entry) => entry.isIntersecting)) return;
        observer?.disconnect();
        startTimeout();
      });
      observer.observe(element);
    } else if (element?.loading !== "lazy") {
      startTimeout();
    }
    return () => {
      observer?.disconnect();
      window.clearTimeout(timer);
      if (current.current.key === requestKey && current.current.src === src) current.current.active = false;
    };
  }, [requestKey, src, remote]);

  return {
    src,
    ref: imageRef,
    onError: advance,
    onLoad() {
      if (!src || !remote || !current.current.active || current.current.key !== requestKey || current.current.src !== src) return;
      loaded.current = `${requestKey}\u0002${src}`;
      rememberMediaSource(sources, remote, Date.now(), locale);
    }
  };
}
