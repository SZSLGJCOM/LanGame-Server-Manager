export interface ArkMapView { zoom: number; x: number; y: number }

export const ARK_MAP_MIN_ZOOM = 1;
export const ARK_MAP_MAX_ZOOM = 12;

/** The SVG fits a normalized 1000 x 1000 map using xMidYMid meet. */
export function clampArkMapView(view: ArkMapView, viewport: { width: number; height: number }): ArkMapView {
  const zoom = Math.min(ARK_MAP_MAX_ZOOM, Math.max(ARK_MAP_MIN_ZOOM, view.zoom));
  const size = 1000 / zoom, pixels = Math.min(viewport.width, viewport.height);
  const axis = (value: number, visible: number) => {
    const half = pixels > 0 ? size * visible / pixels / 2 : size / 2;
    // As in the DST map, a fully visible axis stays centered; otherwise the
    // viewport may reach either world edge but cannot expose space beyond it.
    return half >= 500 ? 500 : Math.max(half, Math.min(1000 - half, value));
  };
  const x = axis(view.x, viewport.width), y = axis(view.y, viewport.height);
  return zoom === view.zoom && x === view.x && y === view.y ? view : { zoom, x, y };
}
