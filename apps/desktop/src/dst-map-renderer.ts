import { terrainColor, type DstWorldMap } from "./dst-world-map";
import { renderDstTerrainEdges } from "./dst-map-edge-renderer";

/** Installed game artwork is decoded by the host; no game assets ship with LGSM. */
async function artwork(source: string, signal: AbortSignal): Promise<HTMLImageElement> {
  const image = new Image();
  image.src = source;
  const cancel = () => { image.src = ""; };
  signal.addEventListener("abort", cancel, { once: true });
  try { await image.decode(); } finally { signal.removeEventListener("abort", cancel); }
  return image;
}

export async function renderDstTerrain(snapshot: DstWorldMap, signal: AbortSignal, onUnavailable?: (paths: string[]) => void): Promise<HTMLCanvasElement> {
  const { width, height, tiles, tileNames, tileKinds, tileTextures, tileBaseTextures, tileColors, art } = snapshot;
  const checkCancelled = () => { if (signal.aborted) throw new DOMException("Map rendering cancelled.", "AbortError"); };
  checkCancelled();
  const unavailable: string[] = [];
  const images = await Promise.all(Object.entries(art?.textures ?? {}).map(async ([path, source]) => {
    try { return [path, await artwork(source, signal)] as const; }
    catch { checkCancelled(); unavailable.push(path); return null; }
  }));
  const decoded = new Map(images.filter((entry): entry is NonNullable<typeof entry> => entry !== null));
  const edgeImages = await Promise.all((art?.terrainEdges ?? []).map(async edge => {
    try { return [JSON.stringify([edge.atlas, edge.texture]), { ...edge, image: await artwork(edge.image, signal) }] as const; }
    catch { checkCancelled(); unavailable.push(`${edge.atlas} (${edge.texture})`); return null; }
  }));
  const decodedEdges = new Map(edgeImages.filter((entry): entry is NonNullable<typeof entry> => entry !== null));
  checkCancelled();
  onUnavailable?.(unavailable.sort());
  const canvas = document.createElement("canvas");
  // Four pixels per native tile keep edges readable without allocating a huge
  // canvas per terrain type. The protocol bounds the total native tile count.
  const density = 4;
  canvas.width = width * density; canvas.height = height * density;
  const context = canvas.getContext("2d");
  if (!context || !tiles) throw new Error("World map canvas is unavailable.");
  context.scale(density, density);
  const styles = new Map<number, string | CanvasPattern>();
  const paper = decoded.get("images/minimap_paper.tex");
  const tinted = new Map<HTMLImageElement, Map<string, HTMLCanvasElement>>();
  function nativeTint(image: HTMLImageElement, base: [number, number, number, number] | null | undefined): HTMLImageElement | HTMLCanvasElement {
    if (!base) return image;
    const key = base.join(","), variants = tinted.get(image);
    const existing = variants?.get(key);
    if (existing) return existing;
    const target = document.createElement("canvas");
    target.width = image.width; target.height = image.height;
    const paint = target.getContext("2d");
    if (!paint) throw new Error("Native terrain texture canvas is unavailable.");
    paint.drawImage(image, 0, 0);
    // minimap.ksh multiplies the map-edge base RGB by the continuous noise RGB.
    // Reading that base from the installed artwork avoids bright, untinted noise.
    paint.globalCompositeOperation = "multiply";
    paint.fillStyle = `rgb(${base[0]},${base[1]},${base[2]})`;
    paint.fillRect(0, 0, image.width, image.height);
    paint.globalCompositeOperation = "destination-in";
    paint.drawImage(image, 0, 0);
    if (variants) variants.set(key, target); else tinted.set(image, new Map([[key, target]]));
    return target;
  }
  for (const [id, name] of Object.entries(tileNames)) {
    const ocean = tileKinds[id] === "ocean" ? tileColors[id] : undefined;
    // Native ocean alpha drives its edge shader, not final image transparency.
    if (ocean) { styles.set(Number(id), `rgb(${ocean[0]},${ocean[1]},${ocean[2]})`); continue; }
    const image = decoded.get(tileTextures[id]) ?? (tileKinds[id] === "void" ? paper : undefined);
    const basePath = tileBaseTextures[id];
    const base = art && basePath && Object.prototype.hasOwnProperty.call(art.terrainBaseColors, basePath)
      ? art.terrainBaseColors[basePath] : art?.terrainBaseColor;
    const pattern = image ? context.createPattern(nativeTint(image, base), "repeat") : null;
    if (pattern) pattern.setTransform(new DOMMatrix().scale(1 / 8));
    styles.set(Number(id), pattern ?? terrainColor(name));
  }
  for (let z = 0; z < height; z++) {
    if (width * height > 262144 && z % 64 === 0) { await new Promise(resolve => setTimeout(resolve, 0)); checkCancelled(); }
    for (let x = 0; x < width;) {
      const tile = tiles[z * width + x];
      let end = x + 1;
      while (end < width && tiles[z * width + end] === tile) end++;
      context.fillStyle = styles.get(tile) ?? "#4a4638";
      context.fillRect(x, height - z - 1, end - x, 1);
      x = end;
    }
  }
  await renderDstTerrainEdges(snapshot, context, density, decoded, decodedEdges, checkCancelled);
  // Trace actual land/water boundaries. No decorative islands or guessed
  // objects are introduced; the edge follows the native tile mask exactly.
  const landTypes = new Set(Object.entries(tileKinds).filter(([, kind]) => kind === "land").map(([id]) => Number(id)));
  const isLand = (tile: number) => landTypes.has(tile);
  context.beginPath();
  for (let z = 0; z < height; z++) {
    if (width * height > 262144 && z % 64 === 0) { await new Promise(resolve => setTimeout(resolve, 0)); checkCancelled(); }
    for (let x = 0; x < width; x++) {
    if (!isLand(tiles[z * width + x])) continue;
    const y = height - z - 1;
    if (x === 0 || !isLand(tiles[z * width + x - 1])) { context.moveTo(x, y); context.lineTo(x, y + 1); }
    if (x + 1 === width || !isLand(tiles[z * width + x + 1])) { context.moveTo(x + 1, y); context.lineTo(x + 1, y + 1); }
    if (z === 0 || !isLand(tiles[(z - 1) * width + x])) { context.moveTo(x, y + 1); context.lineTo(x + 1, y + 1); }
    if (z + 1 === height || !isLand(tiles[(z + 1) * width + x])) { context.moveTo(x, y); context.lineTo(x + 1, y); }
    }
  }
  context.strokeStyle = "#302c22b0"; context.lineWidth = .35; context.lineJoin = "round"; context.stroke();
  // The native road stream contains control points. Smooth the path between
  // those points, retaining its shape rather than drawing angular segments.
  for (const road of snapshot.roads ?? []) {
    context.beginPath();
    context.moveTo(road[0][0], height - road[0][1]);
    for (let i = 0; i < road.length - 1; i++) {
      const before = road[Math.max(0, i - 1)], current = road[i], next = road[i + 1], after = road[Math.min(road.length - 1, i + 2)];
      context.bezierCurveTo(current[0] + (next[0] - before[0]) / 6, height - current[1] - (next[1] - before[1]) / 6,
        next[0] - (after[0] - current[0]) / 6, height - next[1] + (after[1] - current[1]) / 6, next[0], height - next[1]);
    }
    context.lineCap = "round";
    context.strokeStyle = "#33291c99"; context.lineWidth = .8; context.stroke();
    context.strokeStyle = "#a79766cc"; context.lineWidth = .45; context.stroke();
  }
  return canvas;
}
