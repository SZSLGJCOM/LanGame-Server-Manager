import minecraftCoverUrl from "./assets/minecraft-cover.png";

export interface ModulePresentationMedia {
  id: string;
  imageSrc: string;
  titleKey: string;
  badgeKey: string;
}

interface ModulePresentation {
  fallbackCoverSrc?: string;
  fallbackMedia: readonly ModulePresentationMedia[];
}

const EMPTY_PRESENTATION_MEDIA: readonly ModulePresentationMedia[] = [];

const MODULE_PRESENTATIONS: Record<string, ModulePresentation> = {
  minecraft: {
    fallbackCoverSrc: minecraftCoverUrl,
    fallbackMedia: [
      {
        id: "project-cover",
        imageSrc: minecraftCoverUrl,
        titleKey: "library.media.projectArtworkTitle",
        badgeKey: "library.media.projectArtworkBadge"
      }
    ]
  }
};

export function resolveModulePresentationFallbackCoverSrc(moduleId: string): string | null {
  return MODULE_PRESENTATIONS[moduleId]?.fallbackCoverSrc ?? null;
}

export function getModulePresentationFallbackMedia(moduleId: string): readonly ModulePresentationMedia[] {
  return MODULE_PRESENTATIONS[moduleId]?.fallbackMedia ?? EMPTY_PRESENTATION_MEDIA;
}
