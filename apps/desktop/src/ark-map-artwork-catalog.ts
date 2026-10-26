import catalog from "./data/ark-map-artwork.json";
import sourcePolicy from "../../../crates/app-network/official-sources.json";

export interface ArkArtworkBounds { minX: number; maxX: number; minY: number; maxY: number }
export interface ArkMapArtwork {
  url: string; source: string; sourceUrl?: string; bounds: ArkArtworkBounds;
}
export interface ArkMapArtworkDefinition extends ArkMapArtwork { width: number; height: number; sources: readonly string[] }
export const ARK_FULL_GPS_BOUNDS: ArkArtworkBounds = { minX: 0, maxX: 100, minY: 0, maxY: 100 };

const definitions = new Map<string, ArkMapArtworkDefinition>(catalog.maps.map(entry => {
  const name = entry.mapName.toLowerCase();
  const edition = entry.edition === "arksurvivalevolved" ? "ase" : "asa";
  const mirror = sourcePolicy.exactResources.find(resource => resource.id === `ark-map-${edition}-${name}`);
  const url = `https://ark.wiki.gg/images/${entry.image}`;
  return [`${entry.edition}:${name}`, { url, source: "ARK Wiki", sources: [...(mirror?.urls ?? []), url],
    sourceUrl: `https://ark.wiki.gg/wiki/File:${entry.image}`, bounds: entry.bounds, width: entry.width, height: entry.height }];
}));

// Match native map identifiers, never names of unrelated mod maps or the other edition.
export function findArkMapArtwork(edition: string, mapName: string): ArkMapArtworkDefinition | null {
  return definitions.get(`${edition}:${mapName.toLowerCase()}`) ?? null;
}

// Artwork may include a border or cover only part of GPS 0–100. Position its
// verified rectangle without cropping or deriving any player/command coordinates.
export function arkArtworkRect(bounds: ArkArtworkBounds) {
  return { x: bounds.minX * 10, y: bounds.minY * 10,
    width: (bounds.maxX - bounds.minX) * 10, height: (bounds.maxY - bounds.minY) * 10 };
}
