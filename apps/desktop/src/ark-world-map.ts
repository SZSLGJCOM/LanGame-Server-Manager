import { readAdditionalArkMaps, validateAdditionalArkMaps } from "./views/settings/ark-cluster-maps";

export interface ArkMapScope { instanceId: string; processKey: string; runId: number }
export interface ArkMapProjection { minX: number; maxX: number; minY: number; maxY: number }
export interface ArkMapPlayer {
  playerId: number; name: string; accountId: string; team: number;
  x: number | null; y: number | null; z: number | null;
}
export interface ArkMapSnapshot extends ArkMapScope {
  mapName: string; sampledAtUnixMs: number; projection: ArkMapProjection | null;
  players: ArkMapPlayer[]; artwork: { dataUrl: string; source: string } | null;
}
export interface ArkMapTarget { processKey: string; mapName: string; label: string }
export const ARK_MAP_POLL_MS = 3000;
export const ARK_MAP_STALE_MS = 10000;

function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid ARK map response.");
  return value as Record<string, unknown>;
}
const integer = (value: unknown, min: number, max: number): value is number =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= min && value <= max;
const coordinate = (value: unknown): value is number => typeof value === "number" && Number.isFinite(value) && Math.abs(value) <= 10000000;
const boundedText = (value: unknown, max: number): value is string => typeof value === "string"
  && new TextEncoder().encode(value).length <= max && !/[\x00-\x1f\x7f-\x9f]/.test(value);
export function validArkMapScope(scope: ArkMapScope): boolean {
  return boundedText(scope.instanceId, 128) && Boolean(scope.instanceId.trim())
    && /^(?:main|map-[a-z0-9][a-z0-9-]{0,31})$/.test(scope.processKey)
    && integer(scope.runId, 1, Number.MAX_SAFE_INTEGER);
}
export function arkMapScopeKey(scope: ArkMapScope): string { return JSON.stringify([scope.instanceId, scope.processKey, scope.runId]); }

export function parseArkMap(value: unknown, scope: ArkMapScope): ArkMapSnapshot {
  const data = record(value);
  if (!validArkMapScope(scope) || data.instanceId !== scope.instanceId || data.processKey !== scope.processKey || data.runId !== scope.runId
    || !boundedText(data.mapName, 128) || !data.mapName.trim()
    || !integer(data.sampledAtUnixMs, 1, Number.MAX_SAFE_INTEGER)
    || !Array.isArray(data.players) || data.players.length > 512) throw new Error("ARK map identity or bounds do not match the selected runtime.");
  let projection: ArkMapProjection | null = null;
  if (data.projection !== null) {
    const p = record(data.projection);
    if (!coordinate(p.minX) || !coordinate(p.maxX) || !coordinate(p.minY) || !coordinate(p.maxY)
      || p.minX >= p.maxX || p.minY >= p.maxY) throw new Error("Invalid ARK map projection.");
    projection = { minX: p.minX, maxX: p.maxX, minY: p.minY, maxY: p.maxY };
  }
  const ids = new Set<number>();
  const players = data.players.map(value => {
    const p = record(value), coords = [p.x, p.y, p.z];
    if (!integer(p.playerId, 1, 0xffffffff) || ids.has(p.playerId) || !boundedText(p.name, 1024)
      || !boundedText(p.accountId, 256) || !integer(p.team, -0x80000000, 0x7fffffff)
      || !(coords.every(v => v === null) || coords.every(coordinate))) throw new Error("Invalid or duplicate ARK player record.");
    ids.add(p.playerId);
    return { playerId: p.playerId, name: p.name, accountId: p.accountId, team: p.team,
      x: p.x as number | null, y: p.y as number | null, z: p.z as number | null };
  });
  let artwork: ArkMapSnapshot["artwork"] = null;
  if (data.artwork !== null) {
    const art = record(data.artwork);
    if (typeof art.dataUrl !== "string" || art.dataUrl.length > 16 * 1024 * 1024
      || !/^data:image\/(?:png|jpeg|webp);base64,[A-Za-z0-9+/]+={0,2}$/.test(art.dataUrl)
      || !boundedText(art.source, 2048) || !art.source.trim()) throw new Error("Invalid ARK map artwork.");
    artwork = { dataUrl: art.dataUrl, source: art.source };
  }
  return { ...scope, mapName: data.mapName, sampledAtUnixMs: data.sampledAtUnixMs, projection, players, artwork };
}
export function acceptArkMapSnapshot(previous: ArkMapSnapshot | null, next: ArkMapSnapshot, scope: ArkMapScope): ArkMapSnapshot {
  if (arkMapScopeKey(next) !== arkMapScopeKey(scope)) throw new Error("ARK map response belongs to an earlier runtime.");
  return previous && arkMapScopeKey(previous) === arkMapScopeKey(scope) && next.sampledAtUnixMs <= previous.sampledAtUnixMs ? previous : next;
}
export function arkPlayerPoint(player: ArkMapPlayer, projection: ArkMapProjection | null): { x: number; y: number } | null {
  if (player.x === null || player.y === null || player.z === null) return null;
  return projection ? { x: 100 * (player.x - projection.minX) / (projection.maxX - projection.minX),
    y: 100 * (player.y - projection.minY) / (projection.maxY - projection.minY) } : { x: player.x, y: player.y };
}
export function arkMapTargets(settingsJson: string): ArkMapTarget[] {
  let settings: Record<string, unknown> = {};
  try { settings = record(JSON.parse(settingsJson)); } catch { /* A missing configuration still exposes the primary process. */ }
  const main = typeof settings.map_name === "string" ? settings.map_name : "";
  const maps = validateAdditionalArkMaps(settings) ? [] : readAdditionalArkMaps(settings) ?? [];
  return [{ processKey: "main", mapName: main, label: main || "Main" },
    ...maps.filter(map => map.enabled).map(map => ({ processKey: `map-${map.id}`, mapName: map.map_name, label: map.name }))];
}
