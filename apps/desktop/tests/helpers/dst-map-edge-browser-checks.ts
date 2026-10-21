import { DST_MAP_EDGE_MASKS } from "../../src/dst-map-edge-shapes";
import { renderDstTerrain } from "../../src/dst-map-renderer";
import { renderDstTerrainEdges } from "../../src/dst-map-edge-renderer";
import type { DstWorldMap } from "../../src/dst-world-map";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider } from "../../src/i18n";
import { DstWorldMapCanvas } from "../../src/views/servers/DstWorldMapCanvas";

/** Synthetic shader/crop fixtures only. Actual Mod acceptance uses captured
 * server snapshots and original installed artwork in a separate harness. */
export async function checkDstTerrainEdges(check: (value: unknown, message: string) => void, seed: DstWorldMap) {
  const png = (width: number, height: number, paint: (context: CanvasRenderingContext2D) => void) => {
    const canvas = document.createElement("canvas"); canvas.width = width; canvas.height = height;
    paint(canvas.getContext("2d")!); return canvas.toDataURL("image/png");
  };
  // Five equal UV cells: opaque tint, north fringe, empty, south fringe,
  // and a center hole. Distinct cells expose crop and north/south mistakes.
  const image = png(40, 8, context => {
    context.fillStyle = "rgb(128,255,255)"; context.fillRect(0, 0, 8, 8);
    context.fillStyle = "rgba(128,255,255,0.5)";
    context.fillRect(8, 0, 8, 4); context.fillRect(24, 4, 8, 4);
    context.fillStyle = "rgb(128,255,255)"; context.fillRect(32, 0, 8, 8); context.clearRect(34, 2, 4, 4);
  });
  const blue = png(1, 1, context => { context.fillStyle = "rgb(0,0,200)"; context.fillRect(0, 0, 1, 1); });
  const noise = png(1, 1, context => { context.fillStyle = "rgb(200,100,50)"; context.fillRect(0, 0, 1, 1); });
  const empty = [.4, 0, .2, 1] as [number, number, number, number];
  const samples = Object.fromEntries([...DST_MAP_EDGE_MASKS].map(mask => [String(mask), empty]));
  samples[7] = [.2, 0, .2, 1]; samples[224] = [.6, 0, .2, 1]; samples[255] = [.8, 0, .2, 1];
  const edge = { atlas: "custom.xml", texture: "explicit.tex", image, opaque: [0, 0, .2, 1] as [number, number, number, number], samples };
  const layer = { tileId: 3, atlas: edge.atlas, texture: edge.texture, noiseTexture: "noise.tex", sampleStyle: null };
  const snapshot: DstWorldMap = { ...seed, width: 3, height: 3, roads: [], tileNames: { 3: "MOD_A", 4: "MOD_B" },
    tileKinds: { 3: "land", 4: "land" }, tileColors: {}, tileBaseTextures: {},
    tileTextures: { 3: "noise.tex", 4: "blue.tex" }, tiles: [4, 4, 4, 4, 4, 4, 4, 3, 4], terrainLayers: [layer],
    art: { ...seed.art!, textures: { "noise.tex": noise, "blue.tex": blue },
      terrainBaseColor: [255, 255, 255, 255], terrainBaseColors: {}, terrainEdges: [edge] } };
  const render = (value: DstWorldMap) => renderDstTerrain(value, new AbortController().signal);
  const pixel = (canvas: HTMLCanvasElement, x: number, y: number) => [...canvas.getContext("2d")!.getImageData(x, y, 1, 1).data];
  const close = (actual: number[], expected: number[]) => actual.every((value, index) => Math.abs(value - expected[index]) <= 3);
  const north = await render(snapshot);
  check(close(pixel(north, 6, 2), [100, 100, 50, 255]), "Opaque UV RGB must multiply the continuous noise rather than its majority tint");
  check(close(pixel(north, 6, 4), [50, 50, 125, 255]) && close(pixel(north, 6, 7), [0, 0, 200, 255]),
    "A northern native neighbor must apply only its northern fringe, with straight RGB multiplication before alpha compositing");
  const south = await render({ ...snapshot, tiles: [4, 3, 4, 4, 4, 4, 4, 4, 4] });
  check(close(pixel(south, 6, 7), [50, 50, 125, 255]) && close(pixel(south, 6, 4), [0, 0, 200, 255]),
    "Native Z reversal must select the southern UV crop without reflecting source artwork");
  const hole = await render({ ...snapshot, tiles: [4, 3, 4, 3, 4, 3, 4, 3, 4] });
  check(close(pixel(hole, 6, 6), [0, 0, 200, 255]) && close(pixel(hole, 4, 4), [100, 100, 50, 255]),
    "Four neighboring cardinal tiles must retain the atlas's transparent center rather than becoming a solid own tile");
  const otherLayer = { ...layer, tileId: 4, atlas: "other.xml", noiseTexture: "blue.tex" };
  const otherEdge = { ...edge, atlas: otherLayer.atlas, samples: Object.fromEntries([...DST_MAP_EDGE_MASKS].map(mask => [String(mask), empty])) };
  const ordered = { ...snapshot, art: { ...snapshot.art!, terrainEdges: [edge, otherEdge] } };
  const behind = await render({ ...ordered, terrainLayers: [layer, otherLayer] });
  const above = await render({ ...ordered, terrainLayers: [otherLayer, layer] });
  check(close(pixel(behind, 6, 4), [0, 0, 200, 255]) && close(pixel(above, 6, 4), [50, 50, 125, 255]),
    "Successful native layer order must control overlapping edge artwork independently of numeric tile IDs");
  // Reordering applied layers does not require new tile or PNG references.
  // Exercise the mounted production component, beyond its pure renderer.
  const container = document.createElement("div"); container.style.width = "240px"; document.body.append(container);
  const component = createRoot(container), noAction = () => {};
  const componentSnapshot = { ...ordered, players: [], landmarks: [] };
  const show = async (terrainLayers: DstWorldMap["terrainLayers"]) => act(async () => component.render(createElement(I18nProvider, {
    children: createElement(DstWorldMapCanvas, { snapshot: { ...componentSnapshot, terrainLayers }, selectedPlayer: null,
      zoom: 1, live: true, fitRevision: 0, onZoomChange: noAction, onCancelFollow: noAction, onFit: noAction }),
  })));
  const painted = async (expected: number[]) => {
    const deadline = performance.now() + 3000;
    while (performance.now() < deadline) {
      const canvas = container.querySelector("canvas");
      if (canvas && container.querySelector(".dst-map-viewport")?.getAttribute("aria-busy") === "false"
        && close(pixel(canvas, 6, 4), expected)) return true;
      await act(async () => { await new Promise(resolve => setTimeout(resolve, 20)); });
    }
    return false;
  };
  try {
    await show([layer, otherLayer]);
    check(await painted([0, 0, 200, 255]), "The mounted map must paint its initial native layer ordering");
    await show([otherLayer, layer]);
    check(await painted([50, 50, 125, 255]), "Changing only applied native layer order must repaint the mounted map");
  } finally { await act(async () => component.unmount()); container.remove(); }
  const broken = await render({ ...snapshot, terrainLayers: [layer, { ...layer, atlas: null, texture: null, noiseTexture: null }] });
  const unsupported = await render({ ...snapshot, terrainLayers: [{ ...layer, sampleStyle: 5 }] });
  check(close(pixel(broken, 6, 2), [200, 100, 50, 255]) && close(pixel(unsupported, 6, 2), [200, 100, 50, 255]),
    "A broken final layer or unknown sampling style must retain the terrain fill without borrowing an earlier edge material");
  let unavailable: string[] = [];
  const partial = await renderDstTerrain({ ...snapshot, art: { ...snapshot.art!, terrainEdges: [edge,
    { ...edge, atlas: "bad.xml", image: "data:image/png;base64,AAAA" }] } }, new AbortController().signal, paths => { unavailable = paths; });
  check(unavailable.length === 1 && unavailable[0].includes("bad.xml") && close(pixel(partial, 6, 4), pixel(north, 6, 4)),
    "One undecodable Mod edge atlas must report only that source and preserve other decoded terrain edges");
  const translucentNoise = png(1, 1, context => { context.fillStyle = "rgba(200,100,50,0.5)"; context.fillRect(0, 0, 1, 1); });
  const shader = await render({ ...snapshot, art: { ...snapshot.art!, textures: { ...snapshot.art!.textures, "noise.tex": translucentNoise } } });
  check(close(pixel(shader, 6, 2), [100, 100, 50, 255]) && close(pixel(shader, 6, 4), [50, 50, 125, 255]),
    "The native shader ignores noise alpha for both opaque cells and translucent edges");
  if (innerWidth < 1200) return null;
  // The old bounding-box algorithm performed 67 billion alpha checks here.
  // Exercise the accepted protocol limit once, independently of the viewport.
  const large: DstWorldMap = { ...snapshot, width: 1024, height: 1024,
    tiles: Array.from({ length: 1024 * 1024 }, (_, index) => (index % 1024 + 131 * Math.floor(index / 1024)) % 4096),
    tileKinds: Object.fromEntries(Array.from({ length: 4096 }, (_, id) => [id, "land" as const])),
    tileNames: Object.fromEntries(Array.from({ length: 4096 }, (_, id) => [id, "MOD_" + id])),
    tileColors: {}, terrainLayers: Array.from({ length: 4096 }, (_, tileId) => ({ ...layer, tileId })) };
  const edgeImage = new Image(), noiseImage = new Image(); edgeImage.src = image; noiseImage.src = noise;
  await Promise.all([edgeImage.decode(), noiseImage.decode()]);
  const edgeSources = new Map([[JSON.stringify([edge.atlas, edge.texture]), { ...edge, image: edgeImage }]]);
  const textureSources = new Map([["noise.tex", noiseImage]]);
  const largeCanvas = document.createElement("canvas"); largeCanvas.width = large.width * 4; largeCanvas.height = large.height * 4;
  const largeContext = largeCanvas.getContext("2d", { willReadFrequently: true })!;
  largeContext.fillStyle = "rgb(0,0,200)"; largeContext.fillRect(0, 0, largeCanvas.width, largeCanvas.height);
  const read = largeContext.getImageData.bind(largeContext); let mainReadbacks = 0;
  largeContext.getImageData = (x, y, width, height, settings) => { mainReadbacks++; return read(x, y, width, height, settings); };
  const abort = new AbortController();
  const cancel = () => { if (abort.signal.aborted) throw new DOMException("Cancelled", "AbortError"); };
  const timer = setTimeout(() => abort.abort(), 0); let cancelled: unknown;
  try { await renderDstTerrainEdges(large, largeContext, 4, textureSources, edgeSources, cancel); }
  catch (error) { cancelled = error; } finally { clearTimeout(timer); }
  check(cancelled instanceof DOMException && cancelled.name === "AbortError" && mainReadbacks === 0,
    "A maximum-size sparse edge plan must yield and abort before its full-map readback");
  const started = performance.now();
  const active = new AbortController();
  await renderDstTerrainEdges(large, largeContext, 4, textureSources, edgeSources, () => {
    if (active.signal.aborted) throw new DOMException("Cancelled", "AbortError");
  });
  const elapsedMs = performance.now() - started;
  check(mainReadbacks === 1, "4096 scattered native layers must share exactly one whole-map readback");
  largeCanvas.width = 0; largeCanvas.height = 0;
  return { native_tiles: large.width * large.height, native_layers: large.terrainLayers.length, main_readbacks: mainReadbacks, elapsed_ms: elapsedMs };
}
