import type { DstWorldMap } from "./dst-world-map";

/** Position replies may reuse artwork only from the exact native world geometry. */
export function sameDstMapWorld(left: DstWorldMap, right: DstWorldMap): boolean {
  return left.instanceId === right.instanceId && left.shard === right.shard
    && left.runId === right.runId && left.session === right.session
    && left.width === right.width && left.height === right.height && left.tileScale === right.tileScale;
}

export function mergeDstMapPositions(previous: DstWorldMap | null, next: DstWorldMap): DstWorldMap | null {
  if (next.tiles) return next;
  if (!previous?.tiles || !sameDstMapWorld(previous, next)) return null;
  return { ...previous, capturedAtUnixMs: next.capturedAtUnixMs, players: next.players,
    day: next.day, season: next.season, phase: next.phase };
}

export function dstMapWorldKey(snapshot: DstWorldMap): string {
  return JSON.stringify([snapshot.instanceId, snapshot.shard, snapshot.session,
    snapshot.width, snapshot.height, snapshot.tileScale]);
}
