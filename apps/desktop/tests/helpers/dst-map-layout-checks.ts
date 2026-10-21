/** Mirrors --dst-map-rail-min in dst-world-map.css. */
const DST_MAP_RAIL_MIN = 220;

/** Geometry and chrome assertions shared by synthetic and byte-bound native replays. */
export function checkDstMapLayout(check: (value: unknown, message: string) => void, fixture: HTMLElement) {
  const viewport = fixture.querySelector<HTMLElement>(".dst-map-viewport");
  const sidebar = fixture.querySelector<HTMLElement>(".dst-map-sidebar");
  check(viewport && sidebar, "The production map must have a viewport and its information sidebar");
  if (!viewport || !sidebar) return;
  const map = viewport.getBoundingClientRect(), rail = sidebar.getBoundingClientRect(), style = getComputedStyle(viewport);
  check(style.backgroundColor === "rgba(0, 0, 0, 0)" && style.backgroundImage === "none", "The map viewport must retain a transparent background");
  check(rail.left >= map.right - 1 && rail.width > 0, "Map controls and information must occupy the right rail");
  const layout = fixture.querySelector<HTMLElement>(".dst-map-layout"), workspace = fixture.querySelector<HTMLElement>(".dst-world-map");
  check(layout && workspace, "The map must receive a bounded production workspace");
  if (layout && workspace) {
    const bounds = workspace.getBoundingClientRect(), gap = parseFloat(getComputedStyle(layout).columnGap) || 0;
    const side = Math.min(workspace.clientHeight, workspace.clientWidth - DST_MAP_RAIL_MIN - gap);
    check(Math.abs(map.width - map.height) <= .5 && Math.abs(map.width - side) <= 1.1,
      `The map pane must be a square as tall as the region unless the rail's minimum width limits it (map ${map.width}x${map.height}, side ${side})`);
    check(rail.width >= DST_MAP_RAIL_MIN - .5 && Math.abs(rail.right - bounds.right) <= 1.1, `The rail must take all remaining width, at least ${DST_MAP_RAIL_MIN}px`);
    if (innerWidth >= 1280 && innerHeight >= 800) check(side === workspace.clientHeight, `At 1280x800 or larger the square must take the full content height (region ${workspace.clientWidth}x${workspace.clientHeight})`);
    const clipped = [sidebar, ...sidebar.querySelectorAll<HTMLElement>("*")].filter(node => {
      const box = node.getBoundingClientRect();
      return box.width > 0 && (box.left < rail.left - .5 || box.right > rail.right + .5 || node.scrollWidth > node.clientWidth + 1 && getComputedStyle(node).overflowX === "visible");
    });
    check(clipped.length === 0 && sidebar.scrollWidth <= sidebar.clientWidth + 1,
      `Rail content must not overflow horizontally: ${clipped.slice(0, 4).map(node => `${node.tagName}.${node.className}`).join(", ")}`);
    const region = workspace.closest<HTMLElement>(".configuration-workspace__main");
    if (region) {
      const slot = region.getBoundingClientRect();
      const heightBound = side === workspace.clientHeight;
      check(Math.abs(map.left - (slot.left + region.clientLeft)) <= .5 && Math.abs(map.top - (slot.top + region.clientTop)) <= .5
        && (!heightBound || Math.abs(map.bottom - (slot.top + region.clientTop + region.clientHeight)) <= .5),
        `The map pane must be flush with the left, top and bottom of its tab region (map ${map.left},${map.top},${map.bottom}; region ${slot.left},${slot.top},${slot.bottom})`);
    }
    const plane = fixture.querySelector<HTMLElement>(".dst-map-plane")?.getBoundingClientRect();
    check(!plane || Math.abs(Math.max(plane.width / map.width, plane.height / map.height) - 1) <= .01,
      "At 100% the whole world must fit the square pane exactly along its longer side");
    check(map.top >= bounds.top - 1 && map.bottom <= bounds.bottom + 1, "The full-height map must remain within its workspace");
  }
  check(!fixture.querySelector(".dst-map-legend"), "The map must not display a terrain legend");
  check(![...fixture.querySelectorAll(".dst-world-map button")].some(button => /刷新地图|Refresh map/.test(button.textContent ?? "")), "Map collection must not add a permanent refresh button");
  check(!fixture.querySelector(".dst-map-help"), "The rail must not repeat map gesture instructions");
  check(sidebar.querySelector(".dst-map-rail-dock")?.contains(fixture.querySelector(".dst-map-roster")) && !fixture.querySelector(".dst-map-search"),
    "The player chip strip must sit in the rail's bottom dock; there is no landmark search");
  check(!fixture.querySelector(".dst-world-map > .gmt-form-header"), "The map must not repeat a permanent title and explanatory banner");
  check(document.documentElement.scrollWidth <= innerWidth + 1 && map.right <= innerWidth + 1 && rail.right <= innerWidth + 1,
    "Map and right rail must fit without horizontal overflow");
}

/** Where a centering request must land once the pan bound applies, computed from the rendered geometry. */
export function clampedDstMapTarget(fixture: HTMLElement, world: { width: number; height: number }, target: { x: number; y: number }) {
  const viewport = fixture.querySelector<HTMLElement>(".dst-map-viewport")!, plane = fixture.querySelector<HTMLElement>(".dst-map-plane")!.getBoundingClientRect();
  const axis = (value: number, length: number, visible: number, drawn: number) => {
    const half = visible / 2 / (drawn / length);
    return half * 2 >= length ? length / 2 : Math.max(half, Math.min(length - half, value));
  };
  return { x: axis(target.x, world.width, viewport.clientWidth, plane.width), y: axis(target.y, world.height, viewport.clientHeight, plane.height) };
}

/** Per axis: a world larger than the viewport must cover it; one that fits must stay centered (no pannable empty space). */
export function checkDstMapBounded(check: (value: unknown, message: string) => void, fixture: HTMLElement, label: string) {
  const viewport = fixture.querySelector<HTMLElement>(".dst-map-viewport")!, bounds = viewport.getBoundingClientRect();
  const plane = fixture.querySelector<HTMLElement>(".dst-map-plane")!.getBoundingClientRect();
  const left = bounds.left + viewport.clientLeft, top = bounds.top + viewport.clientTop;
  const axis = (start: number, end: number, from: number, length: number) => end - start >= length - .5
    ? start <= from + .5 && end >= from + length - .5 : Math.abs((start + end) / 2 - (from + length / 2)) <= .5;
  check(axis(plane.left, plane.right, left, viewport.clientWidth) && axis(plane.top, plane.bottom, top, viewport.clientHeight),
    `${label}: the map must cover or stay centered in the viewport (plane ${plane.left},${plane.top},${plane.right},${plane.bottom})`);
}
