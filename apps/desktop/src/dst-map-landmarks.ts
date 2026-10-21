import type { DstMapLandmark } from "./dst-world-map";

const INDEX_CELL = 32;
const MAX_VISIBLE_LANDMARKS = 1024;
interface Cell { x: number; z: number; items: DstMapLandmark[] }
export interface DstLandmarkIndex { cells: Cell[]; iconCounts: ReadonlyMap<string, number> }
export interface DstLandmarkViewport { centerX: number; centerZ: number; width: number; height: number; scale: number }

/** Icons shrink with the map; close-up buildings stay below player markers. */
export function dstLandmarkIconSize(scale: number): number {
  return Number.isFinite(scale) && scale > 0 ? Math.max(4, Math.min(24, scale * 8)) : 0;
}

/** Rebuilt only when the native terrain snapshot supplies a new landmark list. */
export function indexDstLandmarks(items: readonly DstMapLandmark[]): DstLandmarkIndex {
  const cells = new Map<string, Cell>();
  const iconCounts = new Map<string, number>();
  for (const item of items) {
    iconCounts.set(item.icon, (iconCounts.get(item.icon) ?? 0) + 1);
    const x = Math.floor(item.mapX / INDEX_CELL), z = Math.floor(item.mapZ / INDEX_CELL), key = `${x}:${z}`;
    let cell = cells.get(key);
    if (!cell) { cell = { x: x * INDEX_CELL, z: z * INDEX_CELL, items: [] }; cells.set(key, cell); }
    cell.items.push(item);
  }
  return { cells: [...cells.values()], iconCounts };
}

export function visibleDstLandmarks(index: DstLandmarkIndex, view: DstLandmarkViewport): DstMapLandmark[] {
  if (!(view.scale > 0 && view.width > 0 && view.height > 0)
    || ![view.scale, view.width, view.height, view.centerX, view.centerZ].every(Number.isFinite)) return [];
  const size = dstLandmarkIconSize(view.scale), spacing = Math.max(14, size + 2);
  const halfWidth = (view.width + size) / (2 * view.scale);
  const halfHeight = (view.height + size) / (2 * view.scale);
  const left = view.centerX - halfWidth, right = view.centerX + halfWidth;
  const bottom = view.centerZ - halfHeight, top = view.centerZ + halfHeight;
  const candidates: DstMapLandmark[] = [];
  for (const cell of index.cells) {
    if (cell.x > right || cell.x + INDEX_CELL < left || cell.z > top || cell.z + INDEX_CELL < bottom) continue;
    for (const item of cell.items) {
      if (item.mapX < left || item.mapX > right || item.mapZ < bottom || item.mapZ > top) continue;
      candidates.push(item);
    }
  }
  // Observed native priority resolves overlap across all icons. Keeping every
  // distinct icon in one cell made a Mod-rich full map unreadable.
  // For equal priorities, preserve scarce observed artwork before repeated
  // resources. This keeps unique Mod buildings inspectable without a prefab list.
  candidates.sort((a, b) => b.priority - a.priority || (index.iconCounts.get(a.icon) ?? 0) - (index.iconCounts.get(b.icon) ?? 0)
    || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
  const occupied = new Map<string, { x: number; z: number }[]>();
  const items: DstMapLandmark[] = [];
  for (const item of candidates) {
    const x = item.mapX * view.scale, z = item.mapZ * view.scale;
    if (!Number.isFinite(x) || !Number.isFinite(z)) continue;
    const cellX = Math.floor(x / spacing), cellZ = Math.floor(z / spacing);
    let overlaps = false;
    for (let dx = -1; dx <= 1 && !overlaps; dx++) for (let dz = -1; dz <= 1 && !overlaps; dz++) {
      overlaps = occupied.get(`${cellX + dx}:${cellZ + dz}`)?.some(other => Math.abs(other.x - x) < spacing && Math.abs(other.z - z) < spacing) ?? false;
    }
    if (overlaps) continue;
    const key = `${cellX}:${cellZ}`, cell = occupied.get(key);
    if (cell) cell.push({ x, z }); else occupied.set(key, [{ x, z }]);
    if (item.mapX < left || item.mapX > right || item.mapZ < bottom || item.mapZ > top) continue;
    items.push(item);
    if (items.length >= MAX_VISIBLE_LANDMARKS) break;
  }
  return items.sort((a, b) => a.priority - b.priority || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
}
