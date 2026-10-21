import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { invokeMock } from "../../src/api-mock";
import { I18nProvider } from "../../src/i18n";
import type { BootstrapResponse, InstanceDetails } from "../../src/types";
import type { DstWorldMap } from "../../src/dst-world-map";
import { renderDstTerrain } from "../../src/dst-map-renderer";
import { checkDstTerrainEdges } from "./dst-map-edge-browser-checks";
import { checkDstMapBounded, checkDstMapLayout, clampedDstMapTarget } from "./dst-map-layout-checks";
import { GMToolsWorkbench } from "../../src/views/servers/GMToolsWorkbench";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";
import "../../src/views/servers/instance-themes.css";

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
type Input = { instanceId: string; shard: "master" | "caves" | "islands" | "volcano"; includeTerrain: boolean };
const calls: Input[] = [];
const nativeNow = Date.now;
let clockOffset = 0;
Date.now = () => nativeNow() + clockOffset;
const responses: DstWorldMap[] = [];
let mode: "ready" | "error" | "empty" | "deferred" | "bad-art" | "bad-icon" = "ready";
let release: ((value: DstWorldMap) => void) | undefined;
let details: InstanceDetails;
let playerX = 52.5, playerZ = 41.5;
let playerCharacter = "wilson";
let playerIcon: string | null = "wilson.png";
let artworkNotice = false;
let artworkCompletion = false;
let inventoryCompletion = false;
let denseLandmarks = false;
const denseNativeLandmarks = Array.from({ length: 10000 }, (_, index) => ({ id: `dense-${String(index).padStart(5, "0")}`,
  prefab: "mod_observed_object", icon: "firepit.png", priority: index % 7, mapX: index % 100 + .5, mapZ: (Math.floor(index / 100) + .5) * .8 }));
function fixturePng(icon = false) {
  const canvas = document.createElement("canvas"); canvas.width = 64; canvas.height = 64;
  const context = canvas.getContext("2d")!;
  if (icon) {
    context.fillStyle = "#e8c480"; context.beginPath(); context.arc(32, 32, 27, 0, Math.PI * 2); context.fill();
    context.fillStyle = "#392a20"; context.fillRect(22, 18, 8, 18); context.fillRect(37, 18, 8, 18);
    context.fillRect(23, 44, 20, 5);
  } else {
    for (let y = 0; y < 64; y += 8) for (let x = 0; x < 64; x += 8) {
      context.fillStyle = (x + y) % 16 ? "#517b39" : "#648a45"; context.fillRect(x, y, 8, 8);
    }
  }
  return canvas.toDataURL("image/png");
}
const texturePng = fixturePng(), iconPng = fixturePng(true);

function packet(input: Input): DstWorldMap {
  const width = 100, height = 80;
  // Only the native IPC boundary is replaced. This deterministic terrain is a
  // rendering fixture, never a preview or fallback for a production world.
  const tiles = Array.from({ length: width * height }, (_, index) => {
    const x = index % width - 50, z = Math.floor(index / width) - 40;
    const land = (x * x / 1800 + z * z / 900) < 1;
    return land ? (x < -10 ? 7 : z > 10 ? 5 : x > 12 ? 3 : 6) : 201;
  });
  return { instanceId: input.instanceId, shard: input.shard, runId: 7, capturedAtUnixMs: Date.now(),
    session: `world-${input.shard}`, width, height, tileScale: 4, day: 25, season: "autumn", phase: "day",
    tiles: input.includeTerrain ? tiles : null,
    tileNames: input.includeTerrain ? { 201: "OCEAN_COASTAL", 3: "ROCKY", 5: "SAVANNA", 6: "GRASS", 7: "FOREST" } : {},
    tileKinds: input.includeTerrain ? { 201: "ocean", 3: "land", 5: "land", 6: "land", 7: "land" } : {},
    tileTextures: input.includeTerrain ? { 6: "images/fixture_grass.tex" } : {},
    tileBaseTextures: input.includeTerrain ? { 6: "levels/tiles/map_edge.tex" } : {},
    tileColors: input.includeTerrain ? { 201: [25, 71, 83, 32] } : {},
    terrainLayers: [],
    landmarks: input.includeTerrain ? denseLandmarks ? denseNativeLandmarks
      : [{ id: "fixture-firepit", prefab: "firepit", icon: "firepit.png", priority: 0, mapX: 40, mapZ: 35,
        inventoryIcon: inventoryCompletion ? { atlas: "../mods/example/images/item.xml", element: "actual_item.tex" } : undefined }] : null,
    roads: input.includeTerrain ? [[[30, 30], [40, 33], [50, 30]]] : null,
    landmarksTruncated: input.includeTerrain && artworkNotice,
    art: input.includeTerrain ? { textures: { "images/fixture_grass.tex": mode === "bad-art" ? "data:image/png;base64,AAAA" : texturePng },
      icons: { "wilson.png": mode === "bad-icon" ? "data:image/png;base64,AAAA" : iconPng,
        "firepit.png": mode === "bad-icon" ? "data:image/png;base64,AAAA" : iconPng, "mod_cartographer.png": texturePng, "compass_avatar.png": iconPng },
      terrainBaseColor: null, terrainBaseColors: {}, terrainEdges: [], iconFallbacks: inventoryCompletion ? [{ icon: "firepit.png", element: "actual_item.tex",
        atlas: "../mods/example/images/item.xml", kind: "entity_inventory" }] : artworkCompletion ? [{ icon: "firepit.png", element: "firepit.tex",
        atlas: "../mods/example/images/structures.xml", kind: "registered_alias" }] : [], issues: artworkNotice ? ["Fixture artwork notice"] : [] } : null,
    players: mode === "empty" ? [] : [{ id: "KU_FIXTURE", name: "Wilson", character: playerCharacter, icon: playerIcon,
      x: 8 + (playerX - 52.5) * 4, z: 4 + (playerZ - 41.5) * 4, mapX: playerX, mapZ: playerZ }] };
}
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args: { input: Input }) => {
  if (command !== "read_dontstarve_world_map") throw new Error(`Unexpected IPC ${command}`);
  calls.push(args.input);
  if (mode === "error") throw new Error("native shard connection lost");
  if (mode === "deferred") return new Promise<DstWorldMap>(resolve => { release = resolve; });
  const snapshot = packet(args.input); if (args.input.includeTerrain) responses.push(snapshot); return snapshot;
} } });
function check(value: unknown, message: string): asserts value { if (!value) throw new Error(message); checks++; }
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const result = fixture.querySelector<T>(selector);
  if (!result) throw new Error(`Missing ${selector}`);
  return result;
}
async function wait(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 9000;
  while (!predicate()) {
    if (performance.now() > deadline) throw new Error(message);
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 20)); });
  }
}
async function render(next: InstanceDetails) {
  details = next;
  await act(async () => root.render(<I18nProvider><InstanceSettingsSaveProvider><main style={{ padding: 24, height: "100vh", boxSizing: "border-box" }}><GMToolsWorkbench details={details} /></main></InstanceSettingsSaveProvider></I18nProvider>));
  await wait(() => Boolean(fixture.querySelector('[data-configuration-section-id="world"] button')), "Tools did not finish loading");
}
async function select(shard: string) {
  await act(async () => {
    const field = element<HTMLSelectElement>(".dst-map-toolbar select");
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!.call(field, shard);
    field.dispatchEvent(new Event("change", { bubbles: true }));
  });
}
async function refresh() {
  // Advance only this fixture's clock through the existing60s terrain interval.
  // Visibility exercises the production poller; there is no test-only refresh API.
  const before = calls.length;
  const shard = element<HTMLSelectElement>(".dst-map-toolbar select").value;
  clockOffset += 60001;
  await act(async () => document.dispatchEvent(new Event("visibilitychange")));
  await wait(() => calls.slice(before).some(call => call.shard === shard && call.includeTerrain), "Foreground polling did not collect the elapsed terrain interval");
  check(calls.slice(before).some(call => call.shard === shard && call.includeTerrain), "The elapsed terrain interval must request a complete map");
}
function mapPoint(clientX: number, clientY: number) {
  const bounds = element(".dst-map-plane").getBoundingClientRect();
  return { x: (clientX - bounds.left) / bounds.width * 100, y: (clientY - bounds.top) / bounds.height * 80 };
}
function viewportCenter() {
  const viewport = element(".dst-map-viewport"), bounds = viewport.getBoundingClientRect();
  return { x: bounds.left + viewport.clientLeft + viewport.clientWidth / 2,
    y: bounds.top + viewport.clientTop + viewport.clientHeight / 2 };
}
function close(actual: number, expected: number, label: string) {
  check(Math.abs(actual - expected) < .05, `${label}: expected ${expected}, got ${actual}`);
}
async function key(value: string) {
  await act(async () => {
    const viewport = element(".dst-map-viewport"); viewport.focus({ preventScroll: true });
    viewport.dispatchEvent(new KeyboardEvent("keydown", { key: value, bubbles: true, cancelable: true }));
  });
}
async function fit() {
  const button = [...fixture.querySelectorAll<HTMLButtonElement>(".dst-map-controls button")].find(value => value.getAttribute("aria-label") === "适应窗口");
  check(button && !button.disabled, "Fit must be available even after panning at the base zoom");
  await act(async () => button.click());
}
async function wheel(clientX: number, clientY: number, deltaY: number) {
  const event = new WheelEvent("wheel", { clientX, clientY, deltaY, bubbles: true, cancelable: true });
  await act(async () => element(".dst-map-viewport").dispatchEvent(event));
  check(event.defaultPrevented, "Map wheel must own zoom rather than scroll the containing page");
}
async function dragMap(dx: number, dy: number, cancel = false) {
  const point = viewportCenter();
  await act(async () => element(".dst-map-viewport").dispatchEvent(new PointerEvent("pointerdown", {
    pointerId: 11, button: 0, buttons: 1, clientX: point.x, clientY: point.y, bubbles: true, cancelable: true
  })));
  await act(async () => window.dispatchEvent(new PointerEvent("pointermove", {
    pointerId: 11, buttons: 1, clientX: point.x + dx, clientY: point.y + dy, bubbles: true
  })));
  check(element(".dst-map-viewport").classList.contains("is-dragging"), "Pointer movement must enter the visible dragging state");
  await act(async () => window.dispatchEvent(new PointerEvent(cancel ? "pointercancel" : "pointerup", {
    pointerId: 11, button: 0, buttons: 0, clientX: point.x + dx, clientY: point.y + dy, bubbles: true
  })));
  check(!element(".dst-map-viewport").classList.contains("is-dragging"), "Pointer completion or cancellation must release the drag");
}
async function checkMapInteraction() {
  const viewport = element(".dst-map-viewport");
  await wait(() => element(".dst-map-plane").getBoundingClientRect().width > 100, "Map viewport size was not measured");
  const fitted = element(".dst-map-plane").getBoundingClientRect();
  // MouseEvent client coordinates are integer CSS pixels; use the delivered
  // point for both sides of the anchor assertion, independent of layout offsets.
  const pointer = { x: Math.round(fitted.left + fitted.width * .65), y: Math.round(fitted.top + fitted.height * .4) };
  const before = mapPoint(pointer.x, pointer.y);
  await wheel(pointer.x, pointer.y, -240);
  const after = mapPoint(pointer.x, pointer.y);
  close(after.x, before.x, "Wheel zoom must preserve the pointed map X");
  close(after.y, before.y, "Wheel zoom must preserve the pointed map Y");
  check(element(".dst-map-plane").getBoundingClientRect().width > fitted.width && element(".dst-map-zoom").textContent !== "100%", "Wheel must change both rendered scale and its visible readout");
  const zoomedWidth = element(".dst-map-plane").getBoundingClientRect().width;
  await key("+");
  close(element(".dst-map-plane").getBoundingClientRect().width, zoomedWidth * 1.25, "Keyboard plus must increase scale");
  const beforeArrow = element(".dst-map-plane").getBoundingClientRect();
  await key("ArrowRight"); await key("ArrowDown");
  const afterArrow = element(".dst-map-plane").getBoundingClientRect();
  close(afterArrow.left, beforeArrow.left - 64, "Right arrow must move the viewport through the world");
  close(afterArrow.top, beforeArrow.top - 64, "Down arrow must move the viewport through the world");
  await key("-");
  close(element(".dst-map-plane").getBoundingClientRect().width, zoomedWidth, "Keyboard minus must decrease scale");
  await key("Home");
  close(element(".dst-map-plane").getBoundingClientRect().width, fitted.width, "Home must restore fitted scale");
  const centered = viewportCenter(), reset = mapPoint(centered.x, centered.y);
  close(reset.x, 50, "Home must restore the map center X"); close(reset.y, 40, "Home must restore the map center Y");
  const beforeDrag = element(".dst-map-plane").getBoundingClientRect(), dragScale = beforeDrag.width / 100;
  const dragTarget = clampedDstMapTarget(fixture, WORLD, { x: reset.x - 36 / dragScale, y: reset.y - 24 / dragScale });
  await dragMap(36, 24, true);
  const afterDrag = element(".dst-map-plane").getBoundingClientRect();
  close(afterDrag.left, beforeDrag.left + (reset.x - dragTarget.x) * dragScale, "Drag must translate the map horizontally up to the pan bound");
  close(afterDrag.top, beforeDrag.top + (reset.y - dragTarget.y) * dragScale, "Drag must translate the map vertically up to the pan bound");
  checkDstMapBounded(check, fixture, "Fitted drag");
  await act(async () => window.dispatchEvent(new PointerEvent("pointermove", { pointerId: 11, buttons: 1, clientX: 300, clientY: 300 })));
  close(element(".dst-map-plane").getBoundingClientRect().left, afterDrag.left, "A cancelled drag must ignore late movement");
  const plane = element(".dst-map-plane").getBoundingClientRect();
  await act(async () => element("canvas").dispatchEvent(new PointerEvent("pointermove", {
    pointerId: 12, clientX: plane.left + plane.width * .3025, clientY: plane.top + plane.height * .653125, bubbles: true
  })));
  check(element(".dst-map-readout").textContent?.includes("30, 27"), "Terrain hover must use the transformed canvas coordinates after panning");
  await fit();
  close(mapPoint(centered.x, centered.y).x, 50, "Fit button must reset panning even when zoom is already 100%");
  check(viewport.scrollTop === 0 && viewport.scrollLeft === 0, "Map navigation must use the view transform without nested scrollbars");
}
async function checkPlayerFollowing() {
  await act(async () => element<HTMLButtonElement>(".dst-map-roster li button").click());
  const point = viewportCenter();
  let center = mapPoint(point.x, point.y);
  const playerTarget = clampedDstMapTarget(fixture, WORLD, { x: playerX, y: 80 - playerZ });
  close(center.x, playerTarget.x, "Selecting a player must center its native map X within the pan bound");
  close(center.y, playerTarget.y, "Selecting a player must center its native map Y within the pan bound");
  checkDstMapBounded(check, fixture, "Player centering");
  check(element(".dst-map-roster li button").getAttribute("aria-pressed") === "true" && Boolean(fixture.querySelector(".dst-map-roster-clear")),
    "Following must have an explicit selected row and cancellation action");
  playerX = 63.75; playerZ = 27.25;
  await wait(() => fixture.querySelector(".dst-map-players g")?.getAttribute("transform") === "translate(63.75 52.75)"
    && calls.at(-1)?.includeTerrain === false, "Live position refresh did not update the followed player");
  center = mapPoint(point.x, point.y);
  const followTarget = clampedDstMapTarget(fixture, WORLD, { x: playerX, y: 80 - playerZ });
  close(center.x, followTarget.x, "Follow must recenter on an actual position-only update within the pan bound");
  close(center.y, followTarget.y, "Follow must preserve the native flipped Y axis within the pan bound");
  await act(async () => element<HTMLButtonElement>(".dst-map-roster-clear").click());
  const stopped = element(".dst-map-plane").getBoundingClientRect();
  playerX = 42; playerZ = 38; await refresh();
  await wait(() => fixture.querySelector(".dst-map-players g")?.getAttribute("transform") === "translate(42 42)", "Player update after cancellation missing");
  close(element(".dst-map-plane").getBoundingClientRect().left, stopped.left, "Cancelling follow must retain the user's view on later updates");
  check(!fixture.querySelector(".dst-map-roster-clear") && element(".dst-map-roster li button").getAttribute("aria-pressed") === "false", "Follow cancellation must clear the row selection");
  await act(async () => element<HTMLButtonElement>(".dst-map-roster li button").click());
  await dragMap(20, 10);
  check(!fixture.querySelector(".dst-map-roster-clear"), "Manual drag must release player following");
  await act(async () => element<HTMLButtonElement>(".dst-map-roster li button").click());
  await key("ArrowLeft");
  check(!fixture.querySelector(".dst-map-roster-clear"), "Manual keyboard panning must release player following");
  await act(async () => element<HTMLButtonElement>(".dst-map-roster li button").click());
  await fit();
  check(!fixture.querySelector(".dst-map-roster-clear"), "Fit must cancel following and restore the whole map");
}
async function checkObservedPlayerIcon() {
  playerCharacter = "mod_cartographer"; playerIcon = "compass_avatar.png";
  await refresh();
  await wait(() => fixture.querySelector<HTMLButtonElement>(".dst-map-roster li button")?.title.includes("mod_cartographer") === true
    && fixture.querySelector(".dst-map-viewport")?.getAttribute("aria-busy") === "false", "Mod player artwork response did not finish rendering");
  check(fixture.querySelector(".dst-map-players image")?.getAttribute("href") === iconPng,
    "A Mod character must display its runtime-observed icon, even when a different prefab-named image exists");
  playerIcon = null; await refresh();
  await wait(() => fixture.querySelector(".dst-map-viewport")?.getAttribute("aria-busy") === "false", "Null icon response did not finish rendering");
  check(!fixture.querySelector(".dst-map-players image") && Boolean(fixture.querySelector(".dst-map-players circle"))
    && fixture.querySelector(".dst-map-players text")?.textContent === "Wilson",
  "A missing observed icon must retain the circle and player name without guessing a prefab-named image");
  playerIcon = "constructor"; await refresh();
  await wait(() => fixture.querySelector(".dst-map-viewport")?.getAttribute("aria-busy") === "false", "Unresolved icon response did not finish rendering");
  check(!fixture.querySelector(".dst-map-players image"), "An inherited object property must never become player artwork or bypass the PNG dictionary boundary");
  playerCharacter = "wilson"; playerIcon = "wilson.png"; await refresh();
  await wait(() => fixture.querySelector<HTMLButtonElement>(".dst-map-roster li button")?.title.includes("wilson") === true
    && fixture.querySelector(".dst-map-viewport")?.getAttribute("aria-busy") === "false", "Original player artwork did not return");
}
async function checkDenseLandmarkView() {
  denseLandmarks = true; await refresh();
  await wait(() => fixture.querySelectorAll(".dst-map-landmarks g").length > 100
    && fixture.querySelector(".dst-map-viewport")?.getAttribute("aria-busy") === "false", "Dense native landmarks did not render");
  const fittedItems = [...fixture.querySelectorAll<SVGGElement>(".dst-map-landmarks g")];
  const fittedCount = fittedItems.length;
  check(fittedCount < 1000, "A 10,000-object native capture must not create 10,000 SVG nodes at fit scale");
  const layer = element(".dst-map-landmarks");
  check(getComputedStyle(layer).pointerEvents === "none" && !layer.querySelector("[role], [tabindex], title")
    && [...layer.querySelectorAll("*")].every(node => getComputedStyle(node).pointerEvents === "none"),
    "Landmarks must be render-only: no buttons, focus stops, hover titles or pointer targets");
  for (let step = 0; step < 6; step++) await key("+");
  const zoomed = [...fixture.querySelectorAll<SVGGElement>(".dst-map-landmarks g")];
  const localCount = (items: SVGGElement[]) => items.filter(item => {
    const source = denseNativeLandmarks.find(value => value.id === item.getAttribute("data-landmark-id"));
    return source && source.mapX >= 40 && source.mapX <= 60 && source.mapZ >= 30 && source.mapZ <= 50;
  }).length;
  check(zoomed.length > 0 && zoomed.length <= 1024 && localCount(zoomed) > localCount(fittedItems),
    "Zooming must recover more native objects within the same world area while keeping SVG work bounded");
  const viewport = element(".dst-map-viewport").getBoundingClientRect();
  check(zoomed.every(item => { const bounds = item.getBoundingClientRect(); return bounds.right >= viewport.left && bounds.left <= viewport.right
    && bounds.bottom >= viewport.top && bounds.top <= viewport.bottom; }), "Viewport filtering must not retain fully offscreen landmark images");
  const icon = fixture.querySelector<SVGImageElement>(".dst-map-landmarks image")!.getBoundingClientRect();
  check(icon.width <= 24.05, "Close-up landmark artwork must respect its screen-size cap");
  await key("Home");
  // A press that starts on a landmark icon is an ordinary map drag: nothing is selected and no list opens.
  await chooseZoom(4);
  const marker = fixture.querySelector<SVGGElement>(".dst-map-landmarks g")!.getBoundingClientRect();
  const pressAt = { x: marker.left + marker.width / 2, y: marker.top + marker.height / 2 };
  check(document.elementFromPoint(pressAt.x, pressAt.y)?.closest(".dst-map-landmarks") === null, "Pointer hit-testing must pass through landmark icons to the map");
  const beforePress = element(".dst-map-plane").getBoundingClientRect();
  const pressTarget = document.elementFromPoint(pressAt.x, pressAt.y)!;
  await act(async () => pressTarget.dispatchEvent(new PointerEvent("pointerdown", { pointerId: 21, button: 0, buttons: 1, clientX: pressAt.x, clientY: pressAt.y, bubbles: true, cancelable: true })));
  await act(async () => window.dispatchEvent(new PointerEvent("pointermove", { pointerId: 21, buttons: 1, clientX: pressAt.x + 30, clientY: pressAt.y + 20, bubbles: true })));
  await act(async () => window.dispatchEvent(new PointerEvent("pointerup", { pointerId: 21, button: 0, buttons: 0, clientX: pressAt.x + 30, clientY: pressAt.y + 20, bubbles: true })));
  const afterPress = element(".dst-map-plane").getBoundingClientRect();
  check(Math.hypot(afterPress.left - beforePress.left, afterPress.top - beforePress.top) > 1, "Dragging from a landmark icon must pan the map");
  check(!fixture.querySelector(".dst-map-canvas-details section"), "Pressing a landmark must not open a landmark list");
  checkDstMapBounded(check, fixture, "Drag from a landmark");
  await key("Home"); denseLandmarks = false; await refresh();
  await wait(() => fixture.querySelectorAll(".dst-map-landmarks g").length === 1
    && fixture.querySelector(".dst-map-viewport")?.getAttribute("aria-busy") === "false", "Sparse native snapshot did not replace dense landmarks");
}

async function chooseZoom(value: number) {
  await act(async () => {
    const field = element<HTMLSelectElement>("select.dst-map-zoom");
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!.call(field, String(value));
    field.dispatchEvent(new Event("change", { bubbles: true }));
  });
}

const WORLD = { width: 100, height: 80 };

/** At 100% the whole world is shown and dragging must not move it; closer in, drags stop at the world border. */
async function checkPanBounds() {
  await fit();
  for (const zoom of [1, 4]) {
    if (zoom !== 1) await chooseZoom(zoom);
    for (const [dx, dy] of [[5000, 5000], [-5000, -5000], [5000, -5000], [-5000, 5000]]) {
      const before = element(".dst-map-plane").getBoundingClientRect();
      await dragMap(dx, dy);
      const after = element(".dst-map-plane").getBoundingClientRect();
      if (zoom === 1) { close(after.left, before.left, `100% drag ${dx},${dy} must not move the map X`); close(after.top, before.top, `100% drag ${dx},${dy} must not move the map Y`); }
      checkDstMapBounded(check, fixture, `Drag ${dx},${dy} at ${zoom * 100}%`);
      const point = viewportCenter(), at = mapPoint(point.x, point.y);
      const expected = clampedDstMapTarget(fixture, WORLD, { x: dx > 0 ? 0 : 100, y: dy > 0 ? 0 : 80 });
      close(at.x, expected.x, `Drag ${dx},${dy} at ${zoom * 100}% must stop at the X border`);
      close(at.y, expected.y, `Drag ${dx},${dy} at ${zoom * 100}% must stop at the Y border`);
    }
  }
  await key("Home"); checkDstMapBounded(check, fixture, "Home after clamped drags");
}

/** After a zoom anchored at a screen point, that point keeps its world coordinate unless the pan bound moves the center. */
function anchoredTarget(screen: { x: number; y: number }, world: { x: number; y: number }) {
  const middle = viewportCenter(), scale = element(".dst-map-plane").getBoundingClientRect().width / WORLD.width;
  const offset = { x: (screen.x - middle.x) / scale, y: (screen.y - middle.y) / scale };
  const center = clampedDstMapTarget(fixture, WORLD, { x: world.x - offset.x, y: world.y - offset.y });
  return { x: center.x + offset.x, y: center.y + offset.y };
}

async function checkZoomInputs() {
  await fit();
  const fittedWidth = element(".dst-map-plane").getBoundingClientRect().width;
  check(!element<HTMLSelectElement>("select.dst-map-zoom").querySelector('option[value="0.5"]'), "Zoom must not go below the whole-world scale");
  check(element<HTMLButtonElement>('[aria-label="缩小"]').disabled, "The minimum zoom must disable only zooming out");
  await chooseZoom(10);
  close(element(".dst-map-plane").getBoundingClientRect().width, fittedWidth * 10, "Zoom must allow a detailed local view beyond the old fourfold cap");
  check(element<HTMLButtonElement>('[aria-label="放大"]').disabled, "The maximum zoom must disable only zooming in");
  await fit();
  const point = viewportCenter();
  point.x += 20; point.y -= 15;
  const before = mapPoint(point.x, point.y);
  await act(async () => element(".dst-map-viewport").dispatchEvent(new MouseEvent("dblclick", {
    clientX: point.x, clientY: point.y, bubbles: true, cancelable: true })));
  close(element(".dst-map-plane").getBoundingClientRect().width, fittedWidth * 1.25, "Double click must zoom in through the shared step");
  const pointed = anchoredTarget(point, before);
  close(mapPoint(point.x, point.y).x, pointed.x, "Double click must retain the pointed map X within the pan bound");
  close(mapPoint(point.x, point.y).y, pointed.y, "Double click must retain the pointed map Y within the pan bound");
  await act(async () => element(".dst-map-viewport").dispatchEvent(new MouseEvent("dblclick", {
    clientX: point.x, clientY: point.y, shiftKey: true, bubbles: true, cancelable: true })));
  close(element(".dst-map-plane").getBoundingClientRect().width, fittedWidth, "Shift double click must reverse zooming in");
  // Pinch around an off-center midpoint, then cancel both pointers. The map
  // must retain that world point and ignore late events after cancellation.
  const midpoint = viewportCenter(); midpoint.x += 30; midpoint.y -= 20;
  const anchor = mapPoint(midpoint.x, midpoint.y);
  const touch = async (type: string, pointerId: number, x: number) => act(async () => {
    (type === "pointerdown" ? element(".dst-map-viewport") : window).dispatchEvent(new PointerEvent(type, {
      pointerId, pointerType: "touch", isPrimary: pointerId === 51, button: 0, buttons: 1,
      clientX: x, clientY: midpoint.y, bubbles: true, cancelable: true }));
  });
  await touch("pointerdown", 51, midpoint.x - 40); await touch("pointerdown", 52, midpoint.x + 40);
  await touch("pointerdown", 53, midpoint.x + 100); await touch("pointercancel", 53, midpoint.x + 100);
  await touch("pointermove", 51, midpoint.x - 80); await touch("pointermove", 52, midpoint.x + 80);
  close(element(".dst-map-plane").getBoundingClientRect().width, fittedWidth * 2, "A two-finger pinch must scale by its actual distance ratio");
  const pinched = anchoredTarget(midpoint, anchor);
  close(mapPoint(midpoint.x, midpoint.y).x, pinched.x, "Pinch must retain its midpoint map X within the pan bound");
  close(mapPoint(midpoint.x, midpoint.y).y, pinched.y, "Pinch must retain its midpoint map Y within the pan bound");
  await touch("pointercancel", 51, midpoint.x - 80); await touch("pointercancel", 52, midpoint.x + 80);
  const stopped = element(".dst-map-plane").getBoundingClientRect();
  await touch("pointermove", 51, midpoint.x - 120);
  close(element(".dst-map-plane").getBoundingClientRect().width, stopped.width, "Cancelled touches must not keep changing zoom");
  await key("0");
  close(element(".dst-map-plane").getBoundingClientRect().width, fittedWidth, "Zero must reset to the complete world");
}
async function checkNativeTerrainColors() {
  const snapshot = packet({ instanceId: "native-color-contract", shard: "master", includeTerrain: true });
  const grass: DstWorldMap = { ...snapshot, width: 16, height: 16, tiles: Array<number>(256).fill(6), roads: [] };
  const original = await renderDstTerrain(grass, new AbortController().signal);
  const tinted = await renderDstTerrain({ ...grass, art: { ...grass.art!, terrainBaseColor: [128, 255, 255, 255] } }, new AbortController().signal);
  const before = original.getContext("2d")!.getImageData(24, 24, 1, 1).data;
  const after = tinted.getContext("2d")!.getImageData(24, 24, 1, 1).data;
  check(before[0] > 0 && Math.abs(after[0] - before[0] * 128 / 255) <= 1
    && after[1] === before[1] && after[2] === before[2] && after[3] === 255,
  "Native map-edge base must multiply decoded noise RGB without making opaque terrain transparent");
  const perTile = await renderDstTerrain({ ...grass,
    tiles: Array.from({ length: 256 }, (_, index) => index % 16 < 8 ? 6 : 50000),
    tileNames: { 6: "CUSTOM_A", 50000: "CUSTOM_B" }, tileKinds: { 6: "land", 50000: "land" },
    tileTextures: { 6: "images/fixture_grass.tex", 50000: "images/fixture_grass.tex" },
    tileBaseTextures: { 6: "../mods/example/base_a.tex", 50000: "../mods/example/base_b.tex" },
    art: { ...grass.art!, terrainBaseColor: [128, 255, 255, 255], terrainBaseColors: {
      "../mods/example/base_a.tex": [255, 255, 255, 255], "../mods/example/base_b.tex": [255, 128, 255, 255] } } }, new AbortController().signal);
  const customA = perTile.getContext("2d")!.getImageData(8, 24, 1, 1).data;
  const customB = perTile.getContext("2d")!.getImageData(40, 24, 1, 1).data;
  const originalA = original.getContext("2d")!.getImageData(8, 24, 1, 1).data;
  check(customA.every((value, index) => value === originalA[index]) && customB[0] === customA[0]
    && Math.abs(customB[1] - customA[1] * 128 / 255) <= 1 && customB[2] === customA[2],
    "Shared noise with different native Mod base materials must retain independent per-tile tints without prefab tables");
  const ocean = await renderDstTerrain({ ...grass, tiles: Array<number>(256).fill(201),
    tileColors: { 201: [11, 23, 47, 0] }, tileTextures: { 201: "images/fixture_grass.tex" },
    art: { ...grass.art!, terrainBaseColor: [128, 255, 255, 255] } }, new AbortController().signal);
  const water = ocean.getContext("2d")!.getImageData(24, 24, 1, 1).data;
  check(water[0] === 11 && water[1] === 23 && water[2] === 47 && water[3] === 255,
    "Native ocean RGB must override noise and base tint; shader alpha must not make the final map transparent");
  const customOcean = await renderDstTerrain({ ...grass,
    tiles: Array.from({ length: 256 }, (_, index) => index % 16 < 4 ? 6 : index % 16 < 8 ? 201 : 202),
    tileNames: { 6: "GRASS", 201: "MOD_CUSTOM", 202: "OCEAN_COASTAL" },
    tileKinds: { 6: "land", 201: "ocean", 202: "ocean" },
    tileColors: { 201: [11, 23, 47, 0], 202: [11, 23, 47, 0] } }, new AbortController().signal);
  const customContext = customOcean.getContext("2d")!;
  // MOD_CUSTOM and OCEAN_COASTAL are adjacent sea bands with the same native
  // colour. Only the GRASS/MOD_CUSTOM boundary is an actual coastline.
  const seaBoundary = customContext.getImageData(32, 32, 1, 1).data;
  check(seaBoundary[0] === 11 && seaBoundary[1] === 23 && seaBoundary[2] === 47 && seaBoundary[3] === 255,
    "Native sea colour must prevent a false coastline between custom-named and recognized ocean tiles");
  const landBoundary = customContext.getImageData(16, 32, 1, 1).data;
  check((landBoundary[0] !== 11 || landBoundary[1] !== 23 || landBoundary[2] !== 47) && landBoundary[3] === 255,
    "A coastline must still separate real land from custom-named tiles carrying native sea colour");

  const samePixels = (left: HTMLCanvasElement, right: HTMLCanvasElement) => {
    const a = left.getContext("2d")!.getImageData(0, 0, left.width, left.height).data;
    const b = right.getContext("2d")!.getImageData(0, 0, right.width, right.height).data;
    return a.length === b.length && a.every((value, index) => value === b[index]);
  };
  const background: DstWorldMap = { ...grass, tiles: Array<number>(256).fill(50000),
    tileNames: { 50000: "IMPASSABLE" }, tileKinds: { 50000: "void" }, tileColors: {}, tileTextures: {},
    art: { ...grass.art!, textures: { "images/minimap_paper.tex": texturePng } } };
  const reference = await renderDstTerrain(background, new AbortController().signal);
  const fakeGround = await renderDstTerrain({ ...background, tileNames: { 50000: "FAKE_GROUND" } }, new AbortController().signal);
  check(samePixels(reference, fakeGround), "Native void tiles must use the same background regardless of their registered name or dynamic ID");
  const ground: DstWorldMap = { ...grass,
    tiles: Array.from({ length: 256 }, (_, index) => index % 16 < 8 ? 6 : 50001),
    tileNames: { 6: "GRASS", 50001: "GRASS" }, tileKinds: { 6: "land", 50001: "land" }, tileColors: {},
    tileTextures: { 6: "images/fixture_grass.tex", 50001: "images/fixture_grass.tex" } };
  const ordinaryLand = await renderDstTerrain(ground, new AbortController().signal);
  const oceanNamedLand = await renderDstTerrain({ ...ground, tileNames: { 6: "GRASS", 50001: "OCEAN_ICE" } }, new AbortController().signal);
  check(samePixels(ordinaryLand, oceanNamedLand), "A native land tile named OCEAN_ICE must not introduce a false coastline");
  const unnamedOcean = { ...ground, tileKinds: { 6: "land", 50001: "ocean" } } satisfies DstWorldMap;
  const oceanWithName = await renderDstTerrain({ ...unnamedOcean, tileNames: { 6: "GRASS", 50001: "OCEAN_COASTAL" } }, new AbortController().signal);
  const oceanWithoutName = await renderDstTerrain({ ...unnamedOcean, tileNames: { 6: "GRASS", 50001: "FLOWERS" } }, new AbortController().signal);
  check(samePixels(oceanWithName, oceanWithoutName), "A native ocean without a colour override must be classified by its native kind, including Mod-defined names");
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  await checkNativeTerrainColors();
  const edgeBenchmark = await checkDstTerrainEdges(check, packet({ instanceId: "edge-contract", shard: "master", includeTerrain: true }));
  const cancelled = new AbortController(); cancelled.abort();
  let cancelledError: unknown;
  try { await renderDstTerrain(packet({ instanceId: "cancelled", shard: "master", includeTerrain: true }), cancelled.signal); }
  catch (cause) { cancelledError = cause; }
  check(cancelledError instanceof DOMException && cancelledError.name === "AbortError", "A pre-cancelled terrain render must abort before producing a canvas");
  const bootstrap = await invokeMock<BootstrapResponse>("bootstrap", { includeSystemSnapshot: false });
  const instance = bootstrap.state.instances.find(value => value.module_id === "dontstarve")!;
  const original = await invokeMock<InstanceDetails>("read_instance_details_from_storage", { instanceId: instance.id });
  const four = { ...original, summary: { ...original.summary, status: "Running", active_process_count: 4 },
    settings_json: JSON.stringify({ shard_layout: "island_adventures" }),
    active_run: { run_id: 7, processes: ["master", "caves", "islands", "volcano"].map(process_key => ({ process_key, status: "Running" })) } };
  await render(four);
  await wait(() => Boolean(fixture.querySelector("canvas")), "Map never rendered");
  check(!fixture.querySelector('input[type="file"], dialog') && !fixture.textContent?.includes("客户端素材"),
    "The world map must collect materials automatically without client setup or import controls");
  await wait(() => fixture.querySelector(".dst-map-viewport")?.getAttribute("aria-busy") === "false", "Native artwork never finished rendering");
  check(element<HTMLSelectElement>(".dst-map-toolbar select").options.length === 4, "All enabled shards must be selectable");
  check(fixture.querySelectorAll(".configuration-section-navigation__button.is-active").length === 1, "Only the shared map navigation should be active");
  check(Boolean(fixture.querySelector('[data-configuration-section-id="world"] .is-active')), "DST tools must open on the world tools panel beside the map");
  checkDstMapLayout(check, fixture);
  const transform = fixture.querySelector(".dst-map-players g")?.getAttribute("transform");
  check(transform === "translate(52.5 38.5)", `Native fractional coordinates must map to the matching tile: ${transform}`);
  const context = element<HTMLCanvasElement>("canvas").getContext("2d")!;
  check(context.getImageData(200, 160, 1, 1).data[3] === 255 && element<HTMLCanvasElement>("canvas").width === 400, "Native tile rows must paint the canvas at four pixels per tile");
  const color = context.getImageData(200, 160, 1, 1).data;
  check(color[0] !== 112 || color[1] !== 140 || color[2] !== 81, "Decoded PNG texture must replace the flat grass fallback");
  check(fixture.querySelector(".dst-map-landmarks g")?.getAttribute("transform") === "translate(40 45)"
    && fixture.querySelector(".dst-map-landmarks image")?.getAttribute("href") === iconPng, "Native landmarks must render their PNG artwork in map coordinates");
  check(fixture.querySelector(".dst-map-players image")?.getAttribute("href") === iconPng, "Native player artwork must render alongside its name and selection ring");
  check(element(".dst-map-viewport .dst-map-readout").textContent === "—", "The fixed hover readout must sit in the map corner and show a dash before any hover");
  await checkObservedPlayerIcon();
  await checkDenseLandmarkView();
  await checkMapInteraction();
  await checkZoomInputs();
  await checkPanBounds();
  for (const shard of ["caves", "islands", "volcano", "master"]) {
    await select(shard);
    await wait(() => calls.at(-1)?.shard === shard && Boolean(fixture.querySelector("canvas")), `Missing ${shard}`);
    check(calls.at(-1)?.includeTerrain === true, `${shard} must fetch its own terrain`);
  }
  await wait(() => fixture.querySelector(".dst-map-viewport")?.getAttribute("aria-busy") === "false", "Shard terrain did not finish rendering");
  const currentContext = element<HTMLCanvasElement>("canvas").getContext("2d")!;
  const untouched = currentContext.getImageData(0, 0, 1, 1);
  currentContext.fillStyle = "#ff00ff"; currentContext.fillRect(0, 0, 1, 1);
  await wait(() => calls.at(-1)?.includeTerrain === false, "Position refresh should reuse terrain");
  check(Boolean(fixture.querySelector("canvas")), "Position-only response must preserve terrain");
  const probe = currentContext.getImageData(0, 0, 1, 1).data;
  check(probe[0] === 255 && probe[1] === 0 && probe[2] === 255, "Position-only refresh must not redraw terrain or decode its artwork again");
  currentContext.putImageData(untouched, 0, 0);
  check(fixture.querySelector(".dst-map-landmarks image")?.getAttribute("href") === iconPng
    && fixture.querySelector(".dst-map-players image")?.getAttribute("href") === iconPng, "Position-only packets must retain landmark and artwork data");
  await checkPlayerFollowing();
  artworkNotice = true; await refresh();
  await wait(() => Boolean(fixture.querySelector(".dst-map-art-issues")), "Artwork notices missing");
  check(!element<HTMLDetailsElement>(".dst-map-art-issues").open && fixture.textContent?.includes("部分对象未显示"), "Artwork notices must be collapsible and truncated landmarks must be disclosed");
  await act(async () => element(".dst-map-art-issues summary").click());
  check(element<HTMLDetailsElement>(".dst-map-art-issues").open && fixture.textContent?.includes("Fixture artwork notice"), "Artwork detail must be available without leaving the map");
  artworkNotice = false; artworkCompletion = true; await refresh();
  await wait(() => responses.at(-1)?.art?.iconFallbacks.some(source => source.kind === "registered_alias") === true
    && fixture.querySelector(".dst-map-viewport")?.getAttribute("aria-busy") === "false", "Completed registered icon artwork did not render");
  check(responses.at(-1)?.art?.iconFallbacks[0].element === "firepit.tex"
    && responses.at(-1)?.art?.iconFallbacks[0].atlas === "../mods/example/images/structures.xml"
    && fixture.querySelector(".dst-map-landmarks image")?.getAttribute("href") === iconPng
    && !fixture.querySelector(".dst-map-art-completions"), "Successful completion must keep exact source metadata and render its icon without a permanent diagnostics list");
  artworkCompletion = false; inventoryCompletion = true; await refresh();
  await wait(() => responses.at(-1)?.art?.iconFallbacks.some(source => source.kind === "entity_inventory") === true
    && fixture.querySelector(".dst-map-viewport")?.getAttribute("aria-busy") === "false", "Entity-declared inventory artwork did not render");
  check(responses.at(-1)?.art?.iconFallbacks[0].icon === "firepit.png"
    && responses.at(-1)?.art?.iconFallbacks[0].element === "actual_item.tex"
    && responses.at(-1)?.art?.iconFallbacks[0].atlas === "../mods/example/images/item.xml"
    && fixture.querySelector(".dst-map-landmarks image")?.getAttribute("href") === iconPng, "Entity completion must retain its observed source and original icon");
  playerIcon = "firepit.png"; playerX += 1;
  await wait(() => calls.at(-1)?.includeTerrain === false
    && fixture.querySelector(".dst-map-players g")?.getAttribute("transform") === `translate(${playerX} ${80 - playerZ})`,
    "The later position-only avatar update did not arrive");
  check(!fixture.querySelector(".dst-map-players image") && Boolean(fixture.querySelector(".dst-map-players circle"))
    && fixture.querySelector(".dst-map-players text")?.textContent === "Wilson"
    && Boolean(fixture.querySelector(".dst-map-landmarks image")),
    "A later avatar sharing a cached inventory icon must retain its named position without borrowing item artwork");
  playerIcon = "wilson.png"; playerX += 1;
  await wait(() => fixture.querySelector(".dst-map-players image")?.getAttribute("href") === iconPng
    && fixture.querySelector(".dst-map-players g")?.getAttribute("transform") === `translate(${playerX} ${80 - playerZ})`,
    "A later actual native avatar must recover without refreshing terrain artwork");
  inventoryCompletion = false; mode = "bad-art"; await refresh();
  await wait(() => fixture.querySelector(".dst-map-viewport")?.getAttribute("aria-busy") === "false", "A bad Mod texture must not block the terrain render");
  check(!fixture.textContent?.includes("地图绘制未完成") && Boolean(fixture.querySelector(".dst-map-render-issues")),
    "One bad Mod texture must produce a scoped notice while the rest of the map stays available");
  check(element<HTMLCanvasElement>("canvas").getContext("2d")!.getImageData(200, 160, 1, 1).data[3] === 255
    && fixture.querySelectorAll(".dst-map-players g").length === 1,
    "Unknown or corrupt Mod artwork must preserve painted terrain and current native player positions");
  mode = "ready"; await refresh();
  await wait(() => !fixture.textContent?.includes("地图绘制未完成") && fixture.querySelector(".dst-map-viewport")?.getAttribute("aria-busy") === "false", "A fresh terrain response must recover failed artwork rendering");
  mode = "bad-icon"; await refresh();
  await wait(() => Boolean(fixture.querySelector(".dst-map-icon-issues")), "Bad native icon pixels must report a scoped notice");
  check(!fixture.querySelector(".dst-map-landmarks image, .dst-map-players image")
    && fixture.querySelector(".dst-map-landmarks circle") && fixture.querySelector(".dst-map-players circle")
    && fixture.querySelector(".dst-map-players text")?.textContent === "Wilson",
    "A bad Mod icon must retain both landmark coordinates and named player position markers");
  mode = "ready"; await refresh();
  await wait(() => !fixture.querySelector(".dst-map-icon-issues") && Boolean(fixture.querySelector(".dst-map-landmarks image")),
    "New valid artwork must recover previously failed icon names");
  mode = "error"; await refresh();
  await wait(() => fixture.textContent?.includes("native shard connection lost") === true, "Native failure must be visible");
  await act(async () => element<HTMLButtonElement>(".dst-map-roster li button").click());
  const failedCenter = viewportCenter(), failedPoint = mapPoint(failedCenter.x, failedCenter.y);
  const failedTarget = clampedDstMapTarget(fixture, WORLD, { x: playerX, y: 80 - playerZ });
  close(failedPoint.x, failedTarget.x, "Clicking a player after a failed refresh must still center its last known X within the pan bound");
  close(failedPoint.y, failedTarget.y, "Clicking a player after a failed refresh must still center its last known Y within the pan bound");
  const rosterHeight = fixture.querySelector(".dst-map-roster")!.getBoundingClientRect().height;
  const dockTop = fixture.querySelector(".dst-map-rail-dock > :last-child")!.getBoundingClientRect().top;
  mode = "empty"; await refresh();
  await wait(() => fixture.querySelectorAll(".dst-map-roster li").length === 0, "Complete empty players must clear the chips");
  check(!fixture.textContent?.includes("此分片没有在线玩家") && !fixture.querySelector(".dst-map-roster p:not([role=status])"), "No players online shows no text in the chip strip");
  check(Math.abs(fixture.querySelector(".dst-map-roster")!.getBoundingClientRect().height - rosterHeight) <= 1
    && Math.abs(fixture.querySelector(".dst-map-rail-dock > :last-child")!.getBoundingClientRect().top - dockTop) <= 1, "The empty strip keeps its height so the grid below does not move");
  check(fixture.querySelectorAll(".dst-map-players g").length === 0, "Old player markers must be removed");
  // Cross-shard follow deliberately survives an empty source shard. End that
  // action before testing a single pending foreground request below.
  const stopFollow = fixture.querySelector<HTMLButtonElement>(".dst-map-roster-clear");
  if (stopFollow) await act(async () => stopFollow.click());
  mode = "ready"; await refresh();
  await wait(() => fixture.querySelectorAll(".dst-map-players g").length === 1, "Players did not refresh");
  mode = "deferred"; await refresh();
  check(Boolean(release) && fixture.querySelectorAll(".dst-map-players g").length === 1 && !fixture.textContent?.includes("已过期"),
    "A pending native request keeps the last positions without a stale-history indicator");
  await act(async () => {
    Object.defineProperty(document, "hidden", { configurable: true, get: () => true });
    document.dispatchEvent(new Event("visibilitychange"));
    release?.(packet(calls.at(-1)!));
  });
  mode = "ready";
  const hiddenCalls = calls.length;
  await act(async () => { Reflect.deleteProperty(document, "hidden"); document.dispatchEvent(new Event("visibilitychange")); });
  await wait(() => calls.length > hiddenCalls, "Visible map must resume native refresh");
  mode = "deferred"; await refresh();
  const old = calls.at(-1)!;
  await render({ ...four, summary: { ...four.summary, id: "different-instance" }, settings_json: "{}" });
  mode = "ready";
  await act(async () => release?.(packet(old)));
  await wait(() => Boolean(fixture.querySelector("canvas")), "New instance map missing");
  check(calls.at(-1)?.instanceId === "different-instance", "Old reply must not replace the current instance");
  check(element<HTMLSelectElement>(".dst-map-toolbar select").options.length === 1, "Disabled shards must not appear");
  await render({ ...details, summary: { ...details.summary, status: "Stopped" }, active_run: null });
  const stoppedCalls = calls.length;
  check(![...fixture.querySelectorAll("button")].some(button => /刷新地图/.test(button.textContent ?? "")), "Stopped map must not offer a permanent refresh action");
  check(calls.length === stoppedCalls, "Stopped shard must not dispatch native queries");
  await render(four);
  await wait(() => Boolean(fixture.querySelector("canvas")), "Final map missing");
  await wait(() => fixture.querySelector(".dst-map-viewport")?.getAttribute("aria-busy") === "false", "Final map artwork did not finish rendering");
  const bounds = element(".dst-map-viewport").getBoundingClientRect();
  check(bounds.width > 0 && bounds.right <= innerWidth + 1 && document.documentElement.scrollWidth <= innerWidth + 1, "Map must fit desktop viewport");
  check(element(".dst-map-viewport").scrollHeight <= element(".dst-map-viewport").clientHeight + 2, "Fit must show the complete terrain without clipping");
  // End (and screenshot) at 100%, the minimum whole-world zoom.
  await fit(); checkDstMapBounded(check, fixture, "Final fitted map");
  checkDstMapLayout(check, fixture);
  await act(async () => {
    element(".dst-map-viewport").scrollIntoView({ block: "center", behavior: "instant" });
    await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
  });
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  const workspace = element(".dst-world-map");
  return { status: "passed", checks, browser_errors: errors, calls: calls.length, edge_render_benchmark: edgeBenchmark,
    map_workspace: { width: workspace.clientWidth, height: workspace.clientHeight } };
}
void run().catch(error => ({ status: "failed", checks, error: error.stack ?? String(error), browser_errors: errors }))
  .finally(() => Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }))
  .then(report => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
