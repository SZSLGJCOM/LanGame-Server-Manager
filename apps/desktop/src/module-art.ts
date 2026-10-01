import { getModuleStoreData } from "./store-media";
import { officialMediaCandidates } from "./official-media-sources";

import { resolveModulePresentationFallbackCoverSrc } from "./module-presentation";

export type ModuleArtBadgeKey =
  | "survival"
  | "exile"
  | "tactics"
  | "frontline"
  | "wilderness"
  | "mist"
  | "sandbox"
  | "arena"
  | "infection"
  | "horde"
  | "pals"
  | "apocalypse"
  | "raider"
  | "factory"
  | "wasteland"
  | "versus"
  | "exploration"
  | "survivor"
  | "northrealm"
  | "bloodnight"
  | "game";

export interface ModuleArtTheme {
  accent: string;
  accentStrong: string;
  ambient: string;
  surface: string;
  badgeKey: ModuleArtBadgeKey;
  glyph: string;
  motif: "embers" | "grid" | "forest" | "signal" | "hazard" | "night";
}

export type ModuleCoverVariant = "default" | "server-list";

const MODULE_ART: Record<string, ModuleArtTheme> = {
  abioticfactor: { accent: "#84cc16", accentStrong: "#a16207", ambient: "#1a2410", surface: "#0f140c", badgeKey: "factory", glyph: "AF", motif: "hazard" },
  arksurvivalevolved: { accent: "#f59e0b", accentStrong: "#b45309", ambient: "#3f2b0d", surface: "#18140d", badgeKey: "survival", glyph: "ARK", motif: "hazard" },
  arksurvivalascended: { accent: "#38bdf8", accentStrong: "#0f766e", ambient: "#0b2230", surface: "#08131b", badgeKey: "survival", glyph: "ASA", motif: "signal" },
  conanexiles: { accent: "#fb7185", accentStrong: "#9f1239", ambient: "#35111a", surface: "#1a0f14", badgeKey: "exile", glyph: "CONAN", motif: "embers" },
  dontstarve: { accent: "#f97316", accentStrong: "#7c2d12", ambient: "#2f160b", surface: "#18100d", badgeKey: "wilderness", glyph: "DST", motif: "night" },
  enshrouded: { accent: "#c084fc", accentStrong: "#6d28d9", ambient: "#24123a", surface: "#120d19", badgeKey: "mist", glyph: "ENSH", motif: "night" },
  minecraft: { accent: "#65a30d", accentStrong: "#166534", ambient: "#14240e", surface: "#0d150b", badgeKey: "sandbox", glyph: "MC", motif: "forest" },
  palworld: { accent: "#06b6d4", accentStrong: "#0f766e", ambient: "#0d2a2f", surface: "#0b1518", badgeKey: "pals", glyph: "PAL", motif: "forest" },
  projectzomboid: { accent: "#f43f5e", accentStrong: "#881337", ambient: "#2d0f19", surface: "#170d12", badgeKey: "apocalypse", glyph: "PZ", motif: "night" },
  rimworld: { accent: "#f59e0b", accentStrong: "#b45309", ambient: "#2a1d0c", surface: "#15110a", badgeKey: "sandbox", glyph: "RW", motif: "grid" },
  rust: { accent: "#ef4444", accentStrong: "#991b1b", ambient: "#2b1010", surface: "#160c0c", badgeKey: "raider", glyph: "RUST", motif: "hazard" },
  satisfactory: { accent: "#f97316", accentStrong: "#c2410c", ambient: "#2e180a", surface: "#17100a", badgeKey: "factory", glyph: "SAT", motif: "grid" },
  sevendaystodie: { accent: "#dc2626", accentStrong: "#7f1d1d", ambient: "#331112", surface: "#1a0d0d", badgeKey: "wasteland", glyph: "7DTD", motif: "hazard" },
  terraria: { accent: "#22c55e", accentStrong: "#15803d", ambient: "#102617", surface: "#0d1510", badgeKey: "exploration", glyph: "TERRA", motif: "forest" },
  unturned: { accent: "#a3e635", accentStrong: "#4d7c0f", ambient: "#1c2d0d", surface: "#101608", badgeKey: "infection", glyph: "UNT", motif: "hazard" },
  valheim: { accent: "#f43f5e", accentStrong: "#9f1239", ambient: "#331222", surface: "#180d13", badgeKey: "northrealm", glyph: "VAL", motif: "night" },
  vrising: { accent: "#e879f9", accentStrong: "#a21caf", ambient: "#321138", surface: "#180d1a", badgeKey: "bloodnight", glyph: "V", motif: "embers" }
};

function hashString(value: string): number {
  let hash = 0;
  for (let index = 0; index < value.length; index += 1) {
    hash = (hash * 31 + value.charCodeAt(index)) >>> 0;
  }
  return hash;
}

function fallbackTheme(moduleId: string, moduleName: string): ModuleArtTheme {
  const hash = hashString(moduleId);
  const hues = [188, 142, 22, 337, 268, 208];
  const hue = hues[hash % hues.length];
  const glyph = moduleName
    .split(/\s+/)
    .map((part) => part[0]?.toUpperCase() ?? "")
    .join("")
    .slice(0, 4) || moduleId.slice(0, 4).toUpperCase();

  return {
    accent: `hsl(${hue} 88% 60%)`,
    accentStrong: `hsl(${hue} 72% 34%)`,
    ambient: `hsl(${hue} 48% 14%)`,
    surface: `hsl(${hue} 28% 8%)`,
    badgeKey: "game",
    glyph,
    motif: (["embers", "grid", "forest", "signal", "hazard", "night"] as const)[hash % 6]
  };
}

export function getModuleArtTheme(moduleId: string, moduleName: string): ModuleArtTheme {
  return MODULE_ART[moduleId] ?? fallbackTheme(moduleId, moduleName);
}

export function resolveModuleCoverSrc(moduleId: string, _variant: ModuleCoverVariant = "default"): string | null {
  return getModuleStoreData(moduleId)?.coverUrl ?? resolveModulePresentationFallbackCoverSrc(moduleId);
}

export function resolveModuleMediaFallbackSources(
  moduleId: string,
  preferredSrc?: string | null,
  variant: ModuleCoverVariant = "default"
): string[] {
  const sources = [
    preferredSrc,
    resolveModuleCoverSrc(moduleId, variant),
    resolveModulePresentationFallbackCoverSrc(moduleId)
  ];
  return [...new Set(sources.flatMap((source) => officialMediaCandidates(source, "image")))];
}
