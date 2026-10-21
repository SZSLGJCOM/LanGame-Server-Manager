/** Top-left image coordinates; the host converts native bottom-origin UVs. */
export type DstMapEdgeRect = [number, number, number, number];

export interface DstMapTerrainLayer {
  tileId: number;
  atlas: string | null;
  texture: string | null;
  noiseTexture: string | null;
  /** Null records an unobserved engine default, rather than an observed call. */
  sampleStyle: number | null;
}

export interface DstMapTerrainEdge {
  atlas: string;
  texture: string;
  image: string;
  opaque: DstMapEdgeRect;
  samples: Record<string, DstMapEdgeRect>;
}

/** Bits run NW, N, NE, W, E, SW, S, SE in image space. */
export function dstMapEdgeMask(neighbors: number): number {
  let result = neighbors;
  if (neighbors & (2 | 8)) result |= 1;
  if (neighbors & (2 | 16)) result |= 4;
  if (neighbors & (64 | 8)) result |= 32;
  if (neighbors & (64 | 16)) result |= 128;
  return result;
}

export const DST_MAP_EDGE_MASKS: ReadonlySet<number> = new Set(Array.from({ length: 256 }, (_, mask) => dstMapEdgeMask(mask)));
