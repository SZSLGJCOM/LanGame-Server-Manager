// Relative to fitting the whole world into the square viewport, not native GetZoom.
export const DST_MAP_MIN_ZOOM = 1;
export const DST_MAP_MAX_ZOOM = 10;
export const DST_MAP_ZOOM_PRESETS = [1, 2, 4, 8, 10] as const;
export const DST_MAP_MAX_VIEW_ZOOM = 512;
const CLOSE_TILE_PIXELS = 24;

export interface DstMapPoint { x: number; y: number }

/** Zoom 1 shows the whole world; in the square pane a square DST world fills it exactly. */
export function dstMapFitScale(width: number, height: number, viewportWidth: number, viewportHeight: number): number {
  if (![width, height, viewportWidth, viewportHeight].every(value => Number.isFinite(value) && value > 0)) return 0;
  return Math.min(viewportWidth / width, viewportHeight / height);
}

export function dstMapZoomLimit(width: number, height: number, viewportWidth: number, viewportHeight: number): number {
  const fit = dstMapFitScale(width, height, viewportWidth, viewportHeight);
  if (!fit) return DST_MAP_MAX_ZOOM;
  return Math.min(DST_MAP_MAX_VIEW_ZOOM, Math.max(DST_MAP_MAX_ZOOM, CLOSE_TILE_PIXELS / fit));
}

export function clampDstMapZoom(value: number, maximum = DST_MAP_MAX_ZOOM): number {
  const limit = Number.isFinite(maximum) ? Math.max(DST_MAP_MIN_ZOOM, Math.min(DST_MAP_MAX_VIEW_ZOOM, maximum)) : DST_MAP_MAX_ZOOM;
  return Math.max(DST_MAP_MIN_ZOOM, Math.min(limit, Number.isFinite(value) ? value : 1));
}

export function stepDstMapZoom(value: number, direction: 1 | -1, maximum = DST_MAP_MAX_ZOOM): number {
  return clampDstMapZoom(value * (direction === 1 ? 1.25 : .8), maximum);
}

/** Never pan empty space into view: an axis that already shows the whole world stays centered (so 100% cannot pan). */
export function clampDstMapCenter(center: DstMapPoint, world: { width: number; height: number },
  viewport: { width: number; height: number }, scale: number): DstMapPoint {
  const axis = (value: number, length: number, visible: number) => {
    const half = scale > 0 && visible > 0 ? visible / 2 / scale : 0;
    if (half * 2 >= length) return length / 2;
    return Math.max(half, Math.min(length - half, Number.isFinite(value) ? value : length / 2));
  };
  return { x: axis(center.x, world.width, viewport.width), y: axis(center.y, world.height, viewport.height) };
}

/** Keep a map coordinate under the same viewport point while zooming. */
export function anchoredDstMapCenter(center: DstMapPoint, offset: DstMapPoint, oldScale: number, nextScale: number): DstMapPoint {
  if (!(oldScale > 0 && nextScale > 0)) return center;
  return { x: center.x + offset.x / oldScale - offset.x / nextScale,
    y: center.y + offset.y / oldScale - offset.y / nextScale };
}
