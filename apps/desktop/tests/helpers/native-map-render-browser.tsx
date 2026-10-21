import React, { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider } from "../../src/i18n";
import { parseDstWorldMap, type DstWorldMap } from "../../src/dst-world-map";
import { indexDstLandmarks, visibleDstLandmarks } from "../../src/dst-map-landmarks";
import { DONTSTARVE_SHARDS, type DontStarveShard } from "../../src/views/settings/modules/dontstarve-shards";
import { DstWorldMapCanvas } from "../../src/views/servers/DstWorldMapCanvas";
import { DstMapArtworkStatus } from "../../src/views/servers/DstMapArtworkStatus";
import { DstMapZoomControls } from "../../src/views/servers/DstMapZoomControls";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";
import "../../src/views/servers/dst-world-map.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const nonce = new URLSearchParams(location.search).get("nonce");
const errors: string[] = [];
addEventListener("error", event => errors.push(event.message));
addEventListener("unhandledrejection", event => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
let checks = 0;
function check(value: unknown, message: string): asserts value { if (!value) throw new Error(message); checks++; }
async function wait(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 20000;
  while (!predicate()) {
    if (performance.now() > deadline) throw new Error(message);
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 20)); });
  }
}
function View({ snapshot }: { snapshot: DstWorldMap }): React.JSX.Element {
  const [zoom, setZoom] = useState(1);
  const [fitRevision, setFitRevision] = useState(0);
  const [selectedPlayer, setSelectedPlayer] = useState<string | null>(null);
  const fit = () => { setZoom(1); setSelectedPlayer(null); setFitRevision(value => value + 1); };
  return <main className="gmt-form dst-world-map settings-schema-section" style={{ padding: 24, maxWidth: 1280, margin: "0 auto" }}>
    <header className="gmt-form-header"><div className="gmt-form-header-left">
      <h1 className="gmt-form-title">原生世界采集画面 · 离线渲染验收</h1>
      <p className="gmt-form-desc">管理员全图 · 已保存的服务器快照，不代表当前实时状态。</p>
    </div></header>
    <div className="dst-map-toolbar"><p className="dst-map-note">
      {snapshot.shard} · {snapshot.width} × {snapshot.height} · 第 {snapshot.day} 天 · 采集于 {new Date(snapshot.capturedAtUnixMs).toLocaleString("zh-CN")}
    </p><div className="dst-map-controls">
      <DstMapZoomControls zoom={zoom} disabled={false} onChange={setZoom} onFit={fit} />
    </div></div>
    <DstWorldMapCanvas snapshot={snapshot} selectedPlayer={selectedPlayer} zoom={zoom} live={false}
      fitRevision={fitRevision} onZoomChange={setZoom} onCancelFollow={() => setSelectedPlayer(null)} onFit={fit} />
    <p className="dst-map-note">地标 {snapshot.landmarks?.length ?? 0} · 纹理 {Object.keys(snapshot.art?.textures ?? {}).length} · 游戏图标 {Object.keys(snapshot.art?.icons ?? {}).length}
      {snapshot.landmarksTruncated ? " · 地标采集达到上限，部分对象未显示" : ""}</p>
    <DstMapArtworkStatus art={snapshot.art} />
  </main>;
}
async function selectZoom(zoom: number) {
  const field = fixture.querySelector<HTMLSelectElement>("select.dst-map-zoom");
  check(field, "The production zoom control must exist");
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!.call(field, String(zoom));
    field.dispatchEvent(new Event("change", { bubbles: true }));
  });
  check(field.value === String(zoom), "The selected zoom must be reflected by the production control");
}
async function focusNativeLandmark(snapshot: DstWorldMap, prefab: string, zoom: number) {
  const candidates = snapshot.landmarks?.filter(item => item.prefab === prefab && snapshot.art?.icons[item.icon])
    .sort((left, right) => right.priority - left.priority || (left.id < right.id ? -1 : left.id > right.id ? 1 : 0)) ?? [];
  check(candidates.length, `No captured landmark with installed artwork matches prefab ${prefab}`);
  const viewport = fixture.querySelector<HTMLElement>(".dst-map-viewport");
  const plane = fixture.querySelector<HTMLElement>(".dst-map-plane");
  check(viewport && plane, "The native map viewport must exist before focusing");
  const bounds = viewport.getBoundingClientRect();
  const center = { x: bounds.left + viewport.clientLeft + viewport.clientWidth / 2,
    y: bounds.top + viewport.clientTop + viewport.clientHeight / 2 };
  // Use real production controls and panning; never replace captured geometry.
  await selectZoom(zoom);
  const index = indexDstLandmarks(snapshot.landmarks ?? []);
  const scale = plane.getBoundingClientRect().width / snapshot.width;
  // A captured instance can legitimately be occluded by higher-priority icons.
  // Choose a visible instance without changing native priorities or geometry;
  // the subsequent DOM and pixel assertions still verify the actual result.
  const target = candidates.slice(0, 128).find(candidate => visibleDstLandmarks(index, {
    centerX: candidate.mapX, centerZ: candidate.mapZ,
    width: viewport.clientWidth, height: viewport.clientHeight, scale,
  }).some(item => item.id === candidate.id));
  check(target, `No visible captured instance among the first 128 matching ${prefab} landmarks`);
  const targetPoint = () => {
    const rect = plane.getBoundingClientRect();
    return { x: rect.left + target.mapX / snapshot.width * rect.width,
      y: rect.top + (snapshot.height - target.mapZ) / snapshot.height * rect.height };
  };
  // Keep each drag within the viewport just as a user would when a landmark
  // starts outside the enlarged view. Recompute from the actual DOM each time.
  for (let index = 0; index < 16; index++) {
    const point = targetPoint(), dx = center.x - point.x, dy = center.y - point.y;
    if (Math.hypot(dx, dy) <= 2) break;
    const fraction = Math.min(1, viewport.clientWidth * .35 / Math.max(1, Math.abs(dx)),
      viewport.clientHeight * .35 / Math.max(1, Math.abs(dy)));
    await act(async () => { viewport.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true,
      pointerId: 91, button: 0, buttons: 1, clientX: center.x, clientY: center.y })); });
    await act(async () => { window.dispatchEvent(new PointerEvent("pointermove", { bubbles: true,
      pointerId: 91, button: 0, buttons: 1, clientX: center.x + dx * fraction, clientY: center.y + dy * fraction })); });
    await act(async () => { window.dispatchEvent(new PointerEvent("pointerup", { bubbles: true,
      pointerId: 91, button: 0, buttons: 0, clientX: center.x + dx * fraction, clientY: center.y + dy * fraction })); });
  }
  const point = targetPoint();
  check(Math.hypot(point.x - center.x, point.y - center.y) <= 3, "The captured target must be centered by actual map panning");
  const landmark = [...fixture.querySelectorAll<SVGGElement>(".dst-map-landmarks g")]
    .find(element => element.getAttribute("data-landmark-id") === target.id);
  check(landmark && target.prefab === prefab, "The actual target landmark must remain visible after native priority filtering");
  const icon = landmark.querySelector("image")?.getBoundingClientRect();
  check(icon && icon.width > 0 && icon.height > 0 && icon.left >= bounds.left && icon.right <= bounds.right
    && icon.top >= bounds.top && icon.bottom <= bounds.bottom, "The actual target artwork must be fully inside the screenshot viewport");
  return { prefab: target.prefab, id: target.id, mapX: target.mapX, mapZ: target.mapZ, zoom, visible: true };
}
async function terrainDiagnostics(snapshot: DstWorldMap, canvas: HTMLCanvasElement) {
  const context = canvas.getContext("2d")!;
  const terrain = [];
  for (const [id, name] of Object.entries(snapshot.tileNames)) {
    const positions = [];
    for (let index = 0; index < (snapshot.tiles?.length ?? 0); index++) {
      if (snapshot.tiles?.[index] === Number(id)) positions.push(index);
    }
    if (!positions.length) continue;
    const texture = snapshot.tileTextures[id], source = snapshot.art?.textures[texture];
    let sourceCenter: number[] | null = null;
    let sourcePaint: CanvasRenderingContext2D | null = null;
    if (source) {
      const image = new Image(); image.src = source; await image.decode();
      const probe = document.createElement("canvas"); probe.width = image.width; probe.height = image.height;
      sourcePaint = probe.getContext("2d")!; sourcePaint.drawImage(image, 0, 0);
      sourceCenter = [...sourcePaint.getImageData(Math.floor(image.width / 2), Math.floor(image.height / 2), 1, 1).data];
    }
    const samples = [...new Set([positions[0], positions[Math.floor(positions.length / 2)], positions[positions.length - 1]])]
      .map(index => {
        const x = index % snapshot.width, z = Math.floor(index / snapshot.width);
        const y = snapshot.height - z - 1;
        const rgba = [...context.getImageData(x * 4 + 2, y * 4 + 2, 1, 1).data];
        let sourceAverage: number[] | null = null;
        if (sourcePaint) {
          // The renderer uses 4 canvas pixels/tile and an eight-pixel noise
          // period per tile. Sample the four source pixels interpolated at
          // this canvas pixel's center without reading the painted terrain.
          const paint = sourcePaint, pixels: number[][] = [];
          for (const dy of [0, 1]) for (const dx of [0, 1]) {
            pixels.push([...paint.getImageData((x * 8 + 4 + dx) % paint.canvas.width,
              (y * 8 + 4 + dy) % paint.canvas.height, 1, 1).data]);
          }
          sourceAverage = [0, 1, 2, 3].map(channel => pixels.reduce((sum, pixel) => sum + pixel[channel], 0) / 4);
        }
        const basePath = snapshot.tileBaseTextures[id];
        const base = snapshot.art && basePath && Object.prototype.hasOwnProperty.call(snapshot.art.terrainBaseColors, basePath)
          ? snapshot.art.terrainBaseColors[basePath] : snapshot.art?.terrainBaseColor;
        const average = sourceAverage;
        const baseMultiplyRgb = average && average[3] === 255 && base && !snapshot.tileColors[id]
          ? [0, 1, 2].map(channel => Math.round(average[channel] * base[channel] / 255)) : null;
        const maximumChannelDelta = baseMultiplyRgb ? Math.max(...baseMultiplyRgb.map((value, channel) => Math.abs(value - rgba[channel]))) : null;
        return { x, z, rgba, sourceAverage, nativeBasePath: basePath ?? null, nativeBaseColor: base,
          baseMultiplyRgb, maximumChannelDelta };
      });
    terrain.push({ id, name, tiles: positions.length, texture, tileColor: snapshot.tileColors[id] ?? null, sourceCenter, samples });
  }
  const geometry = [".dst-map-viewport", ".dst-map-plane", "canvas", ".dst-map-landmarks", ".dst-map-players"]
    .map(selector => {
      const element = fixture.querySelector<HTMLElement>(selector)!;
      const rect = element.getBoundingClientRect(), style = getComputedStyle(element);
      return { selector, rect: { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
        css: { display: style.display, width: style.width, height: style.height, transform: style.transform,
          filter: style.filter, opacity: style.opacity, mixBlendMode: style.mixBlendMode } };
    });
  return { terrainBaseColor: snapshot.art?.terrainBaseColor, geometry, terrain };
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  // No native IPC, generated terrain or generated artwork is used in this
  // harness. The test server serves only the caller's bounded, read-only file.
  const optionsResponse = await fetch("/native-dst-map-render-options.json", { cache: "no-store" });
  check(optionsResponse.ok, "Native rendering options were not provided");
  const options: unknown = await optionsResponse.json();
  check(options && typeof options === "object" && !Array.isArray(options), "Native rendering options must be an object");
  const focusPrefab = (options as Record<string, unknown>).focusPrefab;
  const zoom = (options as Record<string, unknown>).zoom;
  check(typeof zoom === "number" && [.5, 1, 2, 4, 8, 10].includes(zoom), "Native screenshot zoom must be a supported production preset");
  check(focusPrefab === null || (typeof focusPrefab === "string" && /^[A-Za-z0-9_]{1,128}$/.test(focusPrefab)), "Native focus prefab is invalid");
  const response = await fetch("/native-dst-map-snapshot.json", { cache: "no-store" });
  check(response.ok, "Native snapshot file was not provided");
  const raw: unknown = await response.json();
  check(raw && typeof raw === "object" && !Array.isArray(raw), "Native snapshot must be an object");
  const identity = raw as Record<string, unknown>;
  // Earlier saved evidence predates completion provenance. Normalize only
  // this renderer input; never rewrite the captured source file or its hash.
  if (identity.art && typeof identity.art === "object" && !Array.isArray(identity.art)) {
    const artwork = identity.art as Record<string, unknown>;
    if (!("iconFallbacks" in artwork)) artwork.iconFallbacks = [];
  }
  check(typeof identity.instanceId === "string" && identity.instanceId.length > 0 && typeof identity.shard === "string"
    && DONTSTARVE_SHARDS.includes(identity.shard as DontStarveShard), "Native snapshot identity is missing or invalid");
  const snapshot = parseDstWorldMap(raw, identity.instanceId as string, identity.shard as DontStarveShard, true);
  check(snapshot.landmarks !== null && snapshot.landmarks.length > 0, "Native capture must contain at least one real landmark");
  check(snapshot.art !== null && Object.keys(snapshot.art.textures).length > 0, "Installed game terrain artwork must be present in the capture");
  check(snapshot.landmarks.some(item => snapshot.art?.icons[item.icon]), "At least one captured landmark must have installed game artwork");
  const imageSources = [...new Set([...Object.values(snapshot.art.textures), ...Object.values(snapshot.art.icons), ...snapshot.art.terrainEdges.map(edge => edge.image)])];
  for (let index = 0; index < imageSources.length; index += 8) {
    await Promise.all(imageSources.slice(index, index + 8).map(async source => {
      const image = new Image(); image.src = source; await image.decode();
      check(image.naturalWidth > 0 && image.naturalHeight > 0, "Captured PNG artwork must decode to real pixels");
    }));
  }
  await act(async () => root.render(<I18nProvider><View snapshot={snapshot} /></I18nProvider>));
  await wait(() => fixture.querySelector(".dst-map-viewport")?.getAttribute("aria-busy") === "false", "Native snapshot did not finish rendering");
  check(!fixture.textContent?.includes("地图绘制未完成"), "Native terrain rendering must finish without an error notice");
  const canvas = fixture.querySelector<HTMLCanvasElement>("canvas");
  check(canvas && canvas.width === snapshot.width * 4 && canvas.height === snapshot.height * 4, "Canvas must render at four pixels per native tile");
  const context = canvas.getContext("2d");
  check(context, "Rendered canvas context is unavailable");
  let painted = false;
  for (let y = 1; y < 8; y++) for (let x = 1; x < 8; x++) {
    painted ||= context.getImageData(Math.floor(canvas.width * x / 8), Math.floor(canvas.height * y / 8), 1, 1).data[3] > 0;
  }
  check(painted, "Native terrain must paint actual canvas pixels");
  const focus = focusPrefab === null ? null : await focusNativeLandmark(snapshot, focusPrefab, zoom);
  if (focus === null) await selectZoom(zoom);
  const sourceLandmarks = new Map(snapshot.landmarks.map(item => [item.id, item]));
  const visibleLandmarks = [...fixture.querySelectorAll<SVGGElement>(".dst-map-landmarks g")];
  check(visibleLandmarks.length > 0 && visibleLandmarks.length <= snapshot.landmarks.length
    && visibleLandmarks.length < 2000, "The native map must display a bounded visible selection from captured landmarks");
  check(visibleLandmarks.every(element => {
    const item = sourceLandmarks.get(element.getAttribute("data-landmark-id") ?? "");
    return item && element.getAttribute("transform") === `translate(${item.mapX} ${snapshot.height - item.mapZ})`
      && element.querySelector("title")?.textContent === item.prefab;
  }), "Displayed landmark coordinates and labels must come directly from the native capture");
  const landmarkImages = [...fixture.querySelectorAll<SVGImageElement>(".dst-map-landmarks image")];
  check(landmarkImages.length > 0 && landmarkImages.every(image => imageSources.includes(image.getAttribute("href") ?? "")), "Displayed landmark images must come directly from captured game artwork");
  check(fixture.querySelector("h1")?.textContent === "原生世界采集画面 · 离线渲染验收", "Offline capture provenance must remain visible");
  const viewport = fixture.querySelector<HTMLElement>(".dst-map-viewport")!;
  const bounds = viewport.getBoundingClientRect();
  check(bounds.width > 0 && bounds.top >= 0 && bounds.bottom <= innerHeight && bounds.right <= innerWidth,
    "The complete native map viewport must fit the screenshot");
  check(document.documentElement.scrollWidth <= innerWidth + 1, "The native map must not overflow the screenshot horizontally");
  if (focus === null) check(viewport.scrollHeight <= viewport.clientHeight + 2, "Fit mode must display the complete native map without clipping");
  else check(getComputedStyle(viewport).overflowX === "hidden" && getComputedStyle(viewport).overflowY === "hidden",
    "The enlarged native terrain must remain clipped to the map viewport");
  await act(async () => { await new Promise<void>(resolve => requestAnimationFrame(() => resolve())); });
  const iconBounds = landmarkImages.map(image => image.getBoundingClientRect());
  const maximumIconPixels = Math.max(0, ...iconBounds.map(rect => Math.max(rect.width, rect.height)));
  const iconArea = iconBounds.reduce((sum, rect) => sum + Math.max(0, Math.min(rect.right, bounds.right) - Math.max(rect.left, bounds.left))
    * Math.max(0, Math.min(rect.bottom, bounds.bottom) - Math.max(rect.top, bounds.top)), 0);
  const plane = fixture.querySelector<HTMLElement>(".dst-map-plane")!.getBoundingClientRect();
  const visibleMapArea = Math.max(0, Math.min(plane.right, bounds.right) - Math.max(plane.left, bounds.left))
    * Math.max(0, Math.min(plane.bottom, bounds.bottom) - Math.max(plane.top, bounds.top));
  check(maximumIconPixels <= 24.1, "Native-world landmark artwork must obey the close-up cap");
  let diagnostics: Awaited<ReturnType<typeof terrainDiagnostics>> | undefined;
  await act(async () => { diagnostics = await terrainDiagnostics(snapshot, canvas); });
  check(diagnostics, "Terrain diagnostics must complete inside the React observation boundary");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, browser_errors: errors, offline: true, focus, zoom, maximum_icon_pixels: maximumIconPixels,
    icon_rectangle_area_fraction: visibleMapArea > 0 ? iconArea / visibleMapArea : 0, width: snapshot.width, height: snapshot.height,
    landmarks: snapshot.landmarks.length, visible_landmarks: visibleLandmarks.length, texture_images: Object.keys(snapshot.art.textures).length,
    icon_images: Object.keys(snapshot.art.icons).length, terrain_edge_images: snapshot.art.terrainEdges.length,
    loaded_images: imageSources.length, artwork_issues: snapshot.art.issues.length, diagnostics };
}
void run().catch(error => ({ status: "failed", checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .finally(() => Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }))
  .then(report => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
