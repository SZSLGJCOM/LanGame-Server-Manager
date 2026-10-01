import { useEffect, useState } from "react";
import type { CSSProperties } from "react";
import { useI18n } from "../i18n";
import { getModuleArtTheme, resolveModuleMediaFallbackSources, type ModuleCoverVariant } from "../module-art";
import { StreamVideo } from "./StreamVideo";
import { useMediaSource } from "./useMediaSource";

interface ModuleCoverProps {
  moduleId: string;
  moduleName: string;
  subtitle: string;
  size?: "compact" | "hero";
  variant?: ModuleCoverVariant;
  showOverlay?: boolean;
  imageLoading?: "eager" | "lazy";
  previewVideoUrl?: string | null;
  previewVideoPoster?: string | null;
  previewFallbackImages?: string[];
  previewVideoActive?: boolean;
  imageSrc?: string | null;
  imageAlt?: string;
  className?: string;
}

export function ModuleCover({
  moduleId,
  moduleName,
  subtitle,
  size = "compact",
  variant = "default",
  showOverlay = true,
  imageLoading = "lazy",
  previewVideoUrl = null,
  previewVideoPoster = null,
  previewFallbackImages = [],
  previewVideoActive = false,
  imageSrc = null,
  imageAlt = "",
  className: customClassName
}: ModuleCoverProps) {
  const { t } = useI18n();
  const theme = getModuleArtTheme(moduleId, moduleName);
  const [shouldRenderPreviewVideo, setShouldRenderPreviewVideo] = useState(false);
  const [previewVideoReady, setPreviewVideoReady] = useState(false);
  const [previewVideoFailed, setPreviewVideoFailed] = useState(false);
  const [previewFallbackIndex, setPreviewFallbackIndex] = useState(0);
  const style = {
    "--cover-accent": theme.accent,
    "--cover-accent-strong": theme.accentStrong,
    "--cover-ambient": theme.ambient,
    "--cover-surface": theme.surface
  } as CSSProperties;
  const previewFallbackImage = previewVideoActive && !previewVideoReady
    ? previewFallbackImages[previewFallbackIndex] ?? previewVideoPoster
    : null;
  const imageSources = resolveModuleMediaFallbackSources(moduleId, previewFallbackImage ?? imageSrc, variant);
  const image = useMediaSource(imageSources);
  const displayImageSrc = image.src;

  useEffect(() => {
    setShouldRenderPreviewVideo(false);
    setPreviewVideoReady(false);
    setPreviewVideoFailed(false);
    setPreviewFallbackIndex(0);

    if (!previewVideoActive || !previewVideoUrl || size !== "compact" || typeof window === "undefined") {
      return;
    }

    const motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
    if (motionQuery.matches) {
      return;
    }
    const timer = window.setTimeout(() => {
      setShouldRenderPreviewVideo(true);
    }, 520);

    return () => {
      window.clearTimeout(timer);
    };
  }, [moduleId, previewVideoActive, previewVideoUrl, size]);

  useEffect(() => {
    if (!previewVideoActive || previewVideoReady || previewVideoFailed || previewFallbackImages.length < 2 || typeof window === "undefined") {
      return;
    }

    const motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
    if (motionQuery.matches) {
      return;
    }

    const timer = window.setInterval(() => {
      setPreviewFallbackIndex((index) => (index + 1) % previewFallbackImages.length);
    }, 900);

    return () => {
      window.clearInterval(timer);
    };
  }, [previewFallbackImages.length, previewVideoActive, previewVideoReady, previewVideoFailed]);

  const className = [
    size === "hero" ? "module-cover module-cover--hero" : "module-cover",
    customClassName,
    showOverlay ? "" : "module-cover--bare",
    previewVideoActive && previewVideoUrl ? "module-cover--has-preview-video" : "",
    previewVideoReady ? "module-cover--previewing" : ""
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div className={className} style={style}>
      {displayImageSrc ? (
        <>
          <img
            key={displayImageSrc}
            ref={image.ref}
            className="module-cover-image"
            src={displayImageSrc}
            alt={imageAlt}
            loading={size === "hero" ? "eager" : imageLoading}
            decoding="async"
            referrerPolicy="no-referrer"
            onError={image.onError}
            onLoad={image.onLoad}
          />
          <div className="module-cover-haze" />
        </>
      ) : (
        <div className={`module-cover-art motif-${theme.motif}`}>
          <div className="module-cover-orb module-cover-orb--a" />
          <div className="module-cover-orb module-cover-orb--b" />
          <div className="module-cover-grid" />
          <div className="module-cover-glyph">{theme.glyph}</div>
        </div>
      )}
      {shouldRenderPreviewVideo && previewVideoUrl ? (
        <StreamVideo
          streamUrl={previewVideoUrl}
          className={previewVideoReady ? "module-cover-preview-video is-ready" : "module-cover-preview-video"}
          autoPlay
          muted
          loop
          playsInline
          preload="metadata"
          poster={displayImageSrc ?? undefined}
          onPlaying={() => setPreviewVideoReady(true)}
          onWaiting={() => setPreviewVideoReady(false)}
          onPause={() => setPreviewVideoReady(false)}
          onError={() => { setPreviewVideoReady(false); setPreviewVideoFailed(true); }}
        />
      ) : null}
      {showOverlay ? (
        <div className="module-cover-content">
          <div className="module-cover-badge">{t(`moduleArt.badges.${theme.badgeKey}`)}</div>
          <div className="module-cover-name">{moduleName}</div>
          <div className="module-cover-subtitle">{subtitle}</div>
        </div>
      ) : null}
    </div>
  );
}
