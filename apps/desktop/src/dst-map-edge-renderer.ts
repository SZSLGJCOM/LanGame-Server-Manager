import { dstMapEdgeMask, type DstMapEdgeRect } from "./dst-map-edge-shapes";
import type { DstWorldMap } from "./dst-world-map";

interface EdgeImage { image: HTMLImageElement; opaque: DstMapEdgeRect; samples: Record<string, DstMapEdgeRect> }
interface Material { shapes: (Uint8ClampedArray | undefined)[]; noise: ImageData; cells: number[] }

function noisePixels(image: HTMLImageElement, density: number): ImageData {
  const canvas = document.createElement("canvas");
  // Four pixels/tile includes two whole noise periods, even with odd source
  // dimensions. Its integer global-pixel phase is shared across all cells.
  canvas.width = image.width; canvas.height = image.height;
  const context = canvas.getContext("2d", { willReadFrequently: true });
  if (!context) throw new Error("Terrain noise sampling is unavailable.");
  const pattern = context.createPattern(image, "repeat");
  if (!pattern) throw new Error("Terrain noise sampling is unavailable.");
  pattern.setTransform(new DOMMatrix().scale(density / 8));
  context.fillStyle = pattern; context.fillRect(0, 0, canvas.width, canvas.height);
  const pixels = context.getImageData(0, 0, canvas.width, canvas.height);
  canvas.width = 0; canvas.height = 0;
  return pixels;
}

function sampleShapes(edge: EdgeImage, density: number): (Uint8ClampedArray | undefined)[] {
  const canvas = document.createElement("canvas"); canvas.width = density; canvas.height = density;
  const context = canvas.getContext("2d", { willReadFrequently: true });
  if (!context) throw new Error("Terrain edge sampling is unavailable.");
  const sample = (rect: DstMapEdgeRect) => {
    context.clearRect(0, 0, density, density);
    context.drawImage(edge.image, rect[0] * edge.image.width, rect[1] * edge.image.height,
      rect[2] * edge.image.width, rect[3] * edge.image.height, 0, 0, density, density);
    const pixels = context.getImageData(0, 0, density, density).data;
    return pixels.some((value, index) => index % 4 === 3 && value !== 0) ? pixels : undefined;
  };
  const shapes: (Uint8ClampedArray | undefined)[] = [];
  for (const [mask, rect] of Object.entries(edge.samples)) shapes[Number(mask)] = sample(rect);
  shapes[256] = sample(edge.opaque);
  canvas.width = 0; canvas.height = 0;
  return shapes;
}

/** Each native cell contributes at most its own layer and eight neighboring
 * layers. Neither scattered terrain nor many tile IDs multiply whole-world
 * scans/readbacks by the layer count. Packed cells use at most nine entries
 * per input tile; 256 distinguishes the opaque own cell from a neighbor mask. */
export async function renderDstTerrainEdges(snapshot: DstWorldMap, context: CanvasRenderingContext2D,
  density: number, textures: Map<string, HTMLImageElement>, edges: Map<string, EdgeImage>, checkCancelled: () => void): Promise<void> {
  const { width, height, tiles, terrainLayers, tileKinds, tileColors } = snapshot;
  if (!tiles || !terrainLayers.length || !edges.size) return;
  const final = new Map(terrainLayers.map((layer, index) => [layer.tileId, index]));
  const noise = new Map<HTMLImageElement, ImageData>();
  const shapes = new Map<EdgeImage, Material["shapes"]>();
  const materials = new Map<number, Material>();
  let yieldedAt = performance.now();
  const pause = async () => {
    checkCancelled();
    if (performance.now() - yieldedAt < 8) return;
    await new Promise(resolve => setTimeout(resolve, 0)); checkCancelled(); yieldedAt = performance.now();
  };
  for (let index = 0; index < terrainLayers.length; index++) {
    const layer = terrainLayers[index], id = layer.tileId;
    // A broken final application must never borrow an earlier material.
    if (final.get(id) !== index || layer.atlas === null || layer.texture === null || layer.noiseTexture === null
      || (layer.sampleStyle !== null && layer.sampleStyle !== 0) || tileKinds[id] === "void" || tileKinds[id] === "unknown"
      || tileColors[id]) continue;
    const edge = edges.get(JSON.stringify([layer.atlas, layer.texture])), image = textures.get(layer.noiseTexture);
    if (!edge || !image) continue;
    let raster = shapes.get(edge), samples = noise.get(image);
    if (!raster) { raster = sampleShapes(edge, density); shapes.set(edge, raster); }
    if (!samples) { samples = noisePixels(image, density); noise.set(image, samples); }
    materials.set(id, { shapes: raster, noise: samples, cells: [] });
    if (index % 8 === 0) await pause();
  }
  if (!materials.size) return;
  const adjacent = new Uint16Array(8), masks = new Uint8Array(8);
  for (let z = 0; z < height; z++) {
    for (let x = 0; x < width; x++) {
      const index = z * width + x, own = tiles[index], material = materials.get(own);
      if (material?.shapes[256]) material.cells.push(index * 512 + 256);
      let count = 0, bit = 1;
      for (let dy = -1; dy <= 1; dy++) for (let dx = -1; dx <= 1; dx++) {
        if (dx === 0 && dy === 0) continue;
        const nx = x + dx, nz = z - dy;
        if (nx >= 0 && nx < width && nz >= 0 && nz < height) {
          const id = tiles[nz * width + nx];
          if (id !== own && materials.has(id)) {
            let position = 0;
            while (position < count && adjacent[position] !== id) position++;
            if (position === count) { adjacent[count] = id; masks[count] = 0; count++; }
            masks[position] |= bit;
          }
        }
        bit <<= 1;
      }
      for (let position = 0; position < count; position++) {
        const mask = dstMapEdgeMask(masks[position]), neighbor = materials.get(adjacent[position]);
        if (neighbor?.shapes[mask]) neighbor.cells.push(index * 512 + mask);
      }
    }
    if (z % 16 === 0) await pause();
  }
  // One readback and one upload for the complete map. Shape/noise samples are
  // small reusable rasters, and translucent RGB is multiplied before blending.
  const pixels = context.getImageData(0, 0, width * density, height * density), data = pixels.data;
  for (const material of materials.values()) {
    for (let index = 0; index < material.cells.length; index++) {
      const packed = material.cells[index], cell = packed >>> 9, shape = material.shapes[packed & 511];
      if (!shape) continue;
      const x = cell % width * density, y = (height - Math.floor(cell / width) - 1) * density;
      for (let sy = 0; sy < density; sy++) for (let sx = 0; sx < density; sx++) {
        const source = (sy * density + sx) * 4, alpha = shape[source + 3];
        if (!alpha) continue;
        const offset = ((y + sy) * width * density + x + sx) * 4;
        const noiseOffset = (((y + sy) % material.noise.height) * material.noise.width + (x + sx) % material.noise.width) * 4;
        const backdrop = data[offset + 3] * (1 - alpha / 255), resultAlpha = alpha + backdrop;
        for (let channel = 0; channel < 3; channel++) {
          const color = shape[source + channel] * material.noise.data[noiseOffset + channel] / 255;
          data[offset + channel] = Math.round((color * alpha + data[offset + channel] * backdrop) / resultAlpha);
        }
        // minimap.ksh multiplies RGB only. Noise alpha never changes coverage.
        data[offset + 3] = Math.round(resultAlpha);
      }
      if (index % 1024 === 0) await pause();
    }
    material.cells.length = 0;
  }
  checkCancelled(); context.putImageData(pixels, 0, 0);
}
