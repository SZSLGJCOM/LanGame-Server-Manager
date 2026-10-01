import type { CSSProperties } from "react";
import { getModuleArtTheme, resolveModuleMediaFallbackSources } from "../../module-art";
import { useMediaSource } from "../../components/useMediaSource";

interface LibraryAtmosphereProps {
  moduleId: string;
  moduleName: string;
  mediaKey: string;
  imageSrc: string | null;
  className?: string;
  variant?: "catalog" | "detail";
}

export function LibraryAtmosphere({
  moduleId,
  moduleName,
  mediaKey,
  imageSrc,
  className,
  variant = "catalog"
}: LibraryAtmosphereProps) {
  const theme = getModuleArtTheme(moduleId, moduleName);
  const imageSources = resolveModuleMediaFallbackSources(moduleId, imageSrc);
  const image = useMediaSource(imageSources);
  const displayImageSrc = image.src;
  const style = {
    "--library-accent": theme.accent,
    "--library-accent-strong": theme.accentStrong,
    "--library-ambient": theme.ambient,
    "--library-surface": theme.surface
  } as CSSProperties;
  const composedClassName = ["library-atmosphere", `library-atmosphere--${variant}`, className]
    .filter(Boolean)
    .join(" ");

  return (
    <div className={composedClassName} style={style} aria-hidden="true">
      {displayImageSrc ? (
        <img
          key={`${moduleId}-${mediaKey}-${displayImageSrc}`}
          ref={image.ref}
          className="library-atmosphere-image"
          src={displayImageSrc}
          alt=""
          loading={variant === "detail" ? "eager" : "lazy"}
          decoding="async"
          referrerPolicy="no-referrer"
          onError={image.onError}
          onLoad={image.onLoad}
        />
      ) : null}
      <div className="library-atmosphere-tint" />
      <div className="library-atmosphere-halo" />
      <div className="library-atmosphere-grid" />
      <div className="library-atmosphere-glyph">{theme.glyph}</div>
    </div>
  );
}
