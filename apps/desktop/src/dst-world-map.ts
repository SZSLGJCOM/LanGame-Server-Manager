import { DONTSTARVE_SHARDS, type DontStarveShard } from "./views/settings/modules/dontstarve-shards";
import { DST_MAP_EDGE_MASKS, type DstMapTerrainEdge, type DstMapTerrainLayer } from "./dst-map-edge-shapes";

export interface DstMapPlayer {
  id: string; name: string; character: string; icon: string | null; x: number; z: number; mapX: number; mapZ: number;
}
export interface DstMapInventoryIcon { atlas: string; element: string }
export interface DstMapLandmark {
  id: string; prefab: string; icon: string; priority: number; mapX: number; mapZ: number;
  /** An absent declaration supplies no inventory artwork authority. */
  inventoryIcon?: DstMapInventoryIcon | null;
}
export interface DstMapIconFallback {
  icon: string; element: string; atlas: string; kind: "registered_alias" | "indexed_exact" | "indexed_alias" | "entity_inventory";
}
export interface DstMapArt {
  textures: Record<string, string>; icons: Record<string, string>; iconFallbacks: DstMapIconFallback[];
  issues: string[]; terrainBaseColor: [number, number, number, number] | null;
  terrainBaseColors: Record<string, [number, number, number, number]>;
  terrainEdges: DstMapTerrainEdge[];
}
export type DstTerrainKind = "land" | "ocean" | "void" | "unknown";
export interface DstWorldMap {
  instanceId: string; shard: DontStarveShard; runId: number; capturedAtUnixMs: number;
  session: string; geometryRevision: string; width: number; height: number; tileScale: number;
  day: number; season: string; phase: string;
  tiles: number[] | null; tileNames: Record<string, string>; tileKinds: Record<string, DstTerrainKind>; players: DstMapPlayer[];
  tileTextures: Record<string, string>; tileBaseTextures: Record<string, string>; landmarks: DstMapLandmark[] | null;
  tileColors: Record<string, [number, number, number, number]>;
  terrainLayers: DstMapTerrainLayer[];
  roads: [number, number][][] | null; landmarksTruncated: boolean; art: DstMapArt | null;
}

function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid world map response.");
  return value as Record<string, unknown>;
}
function text(value: unknown, limit: number): value is string {
  return typeof value === "string" && value.length <= limit && !/[\u0000-\u001f\u007f]/.test(value);
}
function integer(value: unknown, min: number, max: number): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= min && value <= max;
}
function mapIconKey(value: string): string { return value.replace(/[A-Z]/g, character => character.toLowerCase()); }
const utf8 = new TextEncoder();
function inventoryText(value: unknown, limit: number): value is string {
  return text(value, limit) && value.length > 0 && !/[\u0080-\u009f]/.test(value) && utf8.encode(value).length <= limit;
}
export function parseDstWorldMap(value: unknown, instanceId: string, shard: DontStarveShard, terrain: boolean): DstWorldMap {
  const data = record(value);
  if (data.instanceId !== instanceId || data.shard !== shard || !DONTSTARVE_SHARDS.includes(shard)
    || !integer(data.runId, 1, Number.MAX_SAFE_INTEGER) || !integer(data.capturedAtUnixMs, 1, Number.MAX_SAFE_INTEGER)
    || !integer(data.width, 1, 2048) || !integer(data.height, 1, 2048) || data.width * data.height > 1048576
    || typeof data.tileScale !== "number" || !Number.isFinite(data.tileScale) || data.tileScale <= 0 || data.tileScale > 100
    || !integer(data.day, 0, 0xffffffff) || !text(data.session, 128) || !text(data.geometryRevision, 2048)
    || !text(data.season, 64) || !text(data.phase, 64)) {
    throw new Error("Invalid world map identity or dimensions.");
  }
  const names = record(data.tileNames);
  const kinds = record(data.tileKinds);
  const textures = record(data.tileTextures);
  const baseTextures = record(data.tileBaseTextures);
  const colors = record(data.tileColors);
  if (!Array.isArray(data.terrainLayers) || data.terrainLayers.length > 4096) throw new Error("Invalid native terrain layers.");
  for (const value of data.terrainLayers) {
    const layer = record(value);
    if (!integer(layer.tileId, 0, 65535)
      || [layer.atlas, layer.texture, layer.noiseTexture].some(path => path !== null && (!text(path, 512) || !path))
      || (layer.sampleStyle !== null && !integer(layer.sampleStyle, 0, 255))) throw new Error("Invalid native terrain layer.");
  }
  const color = (value: unknown) => Array.isArray(value) && value.length === 4 && value.every(channel => integer(channel, 0, 255));
  if (Object.keys(kinds).length > 4096 || Object.entries(kinds).some(([id, kind]) => !/^\d{1,5}$/.test(id) || Number(id) > 65535
    || typeof kind !== "string" || !["land", "ocean", "void", "unknown"].includes(kind))) throw new Error("Invalid native terrain classification.");
  if (Object.keys(colors).length > 4096 || Object.entries(colors).some(([id, value]) => !/^\d{1,5}$/.test(id) || kinds[id] !== "ocean" || !color(value))) throw new Error("Invalid native map colors.");
  for (const paths of [textures, baseTextures]) {
    if (Object.keys(paths).length > 4096 || Object.entries(paths).some(([id, path]) => !/^\d{1,5}$/.test(id) || Number(id) > 65535 || !text(path, 512))) {
      throw new Error("Invalid world map texture references.");
    }
  }
  if (Object.keys(names).length > 4096 || Object.entries(names).some(([id, name]) => !/^\d{1,5}$/.test(id) || Number(id) > 65535 || !text(name, 128))) {
    throw new Error("Invalid world map terrain names.");
  }
  if (terrain ? !Array.isArray(data.tiles) || data.tiles.length !== data.width * data.height
    || data.tiles.some(tile => !integer(tile, 0, 65535) || typeof names[String(tile)] !== "string" || typeof kinds[String(tile)] !== "string") : data.tiles !== null) {
    throw new Error("Incomplete world map terrain.");
  }
  if (!Array.isArray(data.players) || data.players.length > 256) throw new Error("Invalid map player list.");
  const seen = new Set<string>();
  const players = data.players.map(value => {
    const player = record(value);
    if (!text(player.id, 128) || !player.id || seen.has(player.id) || !text(player.name, 256) || !text(player.character, 128)
      || (player.icon !== null && !text(player.icon, 128))
      || [player.x, player.z, player.mapX, player.mapZ].some(v => typeof v !== "number" || !Number.isFinite(v) || Math.abs(v) > 1e7)) {
      throw new Error("Invalid map player coordinates.");
    }
    seen.add(player.id);
    return player as unknown as DstMapPlayer;
  });
  if (typeof data.landmarksTruncated !== "boolean") throw new Error("Invalid map landmark status.");
  if (terrain) {
    if (!Array.isArray(data.landmarks) || data.landmarks.length > 32768 || !Array.isArray(data.roads) || data.roads.length > 1024) throw new Error("Invalid map geometry.");
    const identities = new Set<string>();
    const inventorySources = new Map<string, DstMapInventoryIcon | null>();
    for (const value of data.landmarks) {
      const item = record(value);
      if (!text(item.id, 128) || identities.has(item.id) || !text(item.prefab, 128) || !text(item.icon, 128)
        || typeof item.priority !== "number" || !Number.isFinite(item.priority) || Math.abs(item.priority) > 1e7
        || [item.mapX, item.mapZ].some(v => typeof v !== "number" || !Number.isFinite(v) || Math.abs(v) > 1e7)) throw new Error("Invalid map landmark.");
      identities.add(item.id);
      let inventory: DstMapInventoryIcon | null = null;
      if (item.inventoryIcon !== undefined && item.inventoryIcon !== null) {
        const source = record(item.inventoryIcon);
        if (Object.keys(source).length !== 2 || !inventoryText(source.atlas, 512) || !inventoryText(source.element, 128)) {
          throw new Error("Invalid map inventory artwork declaration.");
        }
        inventory = { atlas: source.atlas, element: source.element };
      }
      const key = mapIconKey(item.icon), previous = inventorySources.get(key);
      if (!inventorySources.has(key)) inventorySources.set(key, inventory);
      else if (!previous || !inventory || previous.atlas !== inventory.atlas
        || mapIconKey(previous.element) !== mapIconKey(inventory.element)) inventorySources.set(key, null);
    }
    let points = 0;
    for (const road of data.roads) {
      if (!Array.isArray(road) || road.length < 2 || road.length > 256 || (points += road.length) > 65536
        || road.some(point => !Array.isArray(point) || point.length !== 2 || point.some(v => typeof v !== "number" || !Number.isFinite(v) || Math.abs(v) > 1e7))) throw new Error("Invalid map road.");
    }
    const art = record(data.art);
    if (art.terrainBaseColor !== null && !color(art.terrainBaseColor)) throw new Error("Invalid native map base color.");
    const baseColors = record(art.terrainBaseColors);
    if (Object.keys(baseColors).length > 128 || Object.entries(baseColors).some(([path, value]) => !text(path, 512) || !color(value))) {
      throw new Error("Invalid native terrain base colors.");
    }
    let imageBytes = 0;
    const png = (value: unknown, limit = 512 * 1024) => typeof value === "string" && /^data:image\/png;base64,[A-Za-z0-9+/]+=*$/.test(value)
      && value.length <= limit && (imageBytes += value.length) <= 12 * 1024 * 1024;
    for (const [collection, keyLimit, countLimit] of [[art.textures, 512, 128], [art.icons, 128, 512]] as const) {
      const images = record(collection);
      if (Object.keys(images).length > countLimit || Object.entries(images).some(([key, value]) => !text(key, keyLimit) || !png(value))) throw new Error("Invalid native map artwork.");
    }
    if (!Array.isArray(art.terrainEdges) || art.terrainEdges.length > 128) throw new Error("Invalid native terrain edge artwork.");
    const edgeKeys = new Set<string>();
    const rect = (value: unknown) => Array.isArray(value) && value.length === 4
      && value.every(channel => typeof channel === "number" && Number.isFinite(channel) && channel >= 0 && channel <= 1)
      && value[2] > 0 && value[3] > 0 && value[0] + value[2] <= 1.00000001 && value[1] + value[3] <= 1.00000001;
    for (const value of art.terrainEdges) {
      const edge = record(value);
      if (!text(edge.atlas, 512) || !edge.atlas || !text(edge.texture, 512) || !edge.texture
        || !png(edge.image, 4 * Math.ceil(512 * 1024 / 3) + 22) || !rect(edge.opaque)) throw new Error("Invalid native terrain edge source.");
      const key = JSON.stringify([edge.atlas, edge.texture]);
      if (edgeKeys.has(key)) throw new Error("Duplicate native terrain edge source.");
      edgeKeys.add(key);
      const samples = record(edge.samples);
      if (Object.keys(samples).length !== DST_MAP_EDGE_MASKS.size
        || Object.entries(samples).some(([mask, area]) => !/^(0|[1-9]\d{0,2})$/.test(mask)
          || !DST_MAP_EDGE_MASKS.has(Number(mask)) || !rect(area))) throw new Error("Incomplete native terrain edge shapes.");
    }
    if (!Array.isArray(art.iconFallbacks) || art.iconFallbacks.length > 512) throw new Error("Invalid map icon completion sources.");
    const completed = new Set<string>();
    for (const value of art.iconFallbacks) {
      const source = record(value);
      if (!text(source.icon, 128) || !source.icon || completed.has(source.icon)
        || !text(source.element, 128) || !source.element || !text(source.atlas, 512) || !source.atlas
        || typeof source.kind !== "string" || !["registered_alias", "indexed_exact", "indexed_alias", "entity_inventory"].includes(source.kind)
        || !Object.prototype.hasOwnProperty.call(record(art.icons), source.icon)) throw new Error("Invalid map icon completion source.");
      if (source.kind === "entity_inventory") {
        const iconKey = mapIconKey(source.icon), declaration = inventorySources.get(iconKey);
        if (data.landmarksTruncated || !declaration || declaration.atlas !== source.atlas
          || mapIconKey(declaration.element) !== mapIconKey(source.element)
          || players.some(player => player.icon !== null && mapIconKey(player.icon) === iconKey)) {
          throw new Error("Unproven entity inventory icon completion source.");
        }
      }
      completed.add(source.icon);
    }
    if (!Array.isArray(art.issues) || art.issues.length > 1024 || art.issues.some(value => !text(value, 2048))) throw new Error("Invalid native map artwork status.");
  } else if (data.landmarks !== null || data.roads !== null || data.art !== null) throw new Error("Unexpected map geometry in position response.");
  return { ...data, tileNames: names, players } as unknown as DstWorldMap;
}

/** Native coordinates already include the fractional offset within each tile. */
export interface DstMapClick { playerId: string | null; x: number; y: number }

/**
 * dst_map_query.lua places map cells linearly in world units (map = tile + (world - center) / tileScale),
 * so a known player's world/map pair anchors the inverse for any canvas point.
 */
export function mapPointToWorld(snapshot: Pick<DstWorldMap, "height" | "tileScale">, anchor: DstMapPlayer, point: { x: number; y: number }): { x: number; z: number } {
  const round = (value: number) => Math.round(value * 100) / 100;
  return { x: round(anchor.x + (point.x - anchor.mapX) * snapshot.tileScale), z: round(anchor.z + (snapshot.height - point.y - anchor.mapZ) * snapshot.tileScale) };
}

export function mapPlayerPoint(player: DstMapPlayer, height: number): [number, number] {
  return [player.mapX, height - player.mapZ];
}

export function mapPlayerArtwork(art: DstMapArt | null, icon: string | null): string | undefined {
  // Position-only updates can introduce an avatar after the terrain/art cache
  // was built. An entity's inventory declaration never authorizes avatar art.
  if (!art || icon === null || art.iconFallbacks.some(source => source.kind === "entity_inventory"
    && mapIconKey(source.icon) === mapIconKey(icon))) return undefined;
  return Object.prototype.hasOwnProperty.call(art.icons, icon) ? art.icons[icon] : undefined;
}

export function terrainColor(name: string): string {
  const value = name.toUpperCase();
  if (/INVALID|IMPASSABLE|UNDERGROUND|NOISE/.test(value)) return "#202833";
  if (/LAVA|MAGMA|VOLCANO/.test(value)) return "#aa5941";
  if (/OCEAN|WATER|COASTAL|BRINE/.test(value)) return /DEEP|ROUGH|SWELL/.test(value) ? "#26465c" : "#3c7185";
  if (/BEACH|SAND|DESERT/.test(value)) return "#c3aa70";
  if (/SNOW|ICE/.test(value)) return "#a9c5ca";
  if (/SAVANNA|MEADOW/.test(value)) return "#b5a357";
  if (/GRASS|JUNGLE|RAINFOREST/.test(value)) return "#708c51";
  if (/FOREST|DECIDUOUS/.test(value)) return "#44664d";
  if (/MARSH|SWAMP/.test(value)) return "#685e75";
  if (/FUNGUS|MUSHROOM/.test(value)) return /RED/.test(value) ? "#9d6564" : /GREEN/.test(value) ? "#6e8b71" : "#6c80a3";
  if (/ROCK|CAVE|BRICK|TILES|METEOR|RUINS/.test(value)) return "#85827c";
  if (/DIRT|ROAD|MUD|SINKHOLE/.test(value)) return "#877255";
  if (/WOOD|CARPET|CHECKER/.test(value)) return "#a18569";
  // Unknown mod terrain remains a distinct native tile type, never relabelled
  // as a vanilla biome. Its name is retained in the legend and hover readout.
  let hash = 0;
  for (const character of value) hash = ((hash * 31) + character.charCodeAt(0)) >>> 0;
  return `hsl(${hash % 360} 24% 48%)`;
}
