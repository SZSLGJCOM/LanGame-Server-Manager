import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import { checkDstMapBounded, checkDstMapLayout, clampedDstMapTarget } from "./dst-map-layout-checks";
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { invokeMock } from "../../src/api-mock";
import { readDstMapCache } from "../../src/dst-map-cache";
import type { DstWorldMap } from "../../src/dst-world-map";
import { I18nProvider } from "../../src/i18n";
import type { BootstrapResponse, InstanceDetails } from "../../src/types";
import type { DontStarveShard } from "../../src/views/settings/modules/dontstarve-shards";
import { DstWorldMapWorkbench } from "../../src/views/servers/DstWorldMapWorkbench";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!, root = createRoot(fixture);
const errors: string[] = [];
addEventListener("error", event => errors.push(event.message));
addEventListener("unhandledrejection", event => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const shards: DontStarveShard[] = ["master", "caves", "islands", "volcano"];
type Input = { instanceId: string; shard: DontStarveShard; includeTerrain: boolean };
const calls: Input[] = [], migrations: string[] = ["master"];
const nativeNow = Date.now;
let clockOffset = 0;
Date.now = () => nativeNow() + clockOffset;
let checks = 0, details: InstanceDetails, playerShard: DontStarveShard | null = "master";
let failedShard: DontStarveShard | null = null, failure = "", generation = 1, tileScale = 4;
let room = 0;
let hold = false, holdShard: DontStarveShard | null = null;
const pending: { input: Input; resolve: (snapshot: DstWorldMap) => void }[] = [];
const artworkCanvas = document.createElement("canvas"); artworkCanvas.width = artworkCanvas.height = 16;
artworkCanvas.getContext("2d")!.fillRect(2, 2, 12, 12);
const icon = artworkCanvas.toDataURL("image/png");

function check(value: unknown, message: string): asserts value { if (!value) throw new Error(message); checks++; }
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const found = fixture.querySelector<T>(selector);
  if (!found) throw new Error(`Missing ${selector}`);
  return found;
}
async function wait(predicate: () => boolean, message: string, pulse = false) {
  const deadline = performance.now() + 10000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(message);
    await act(async () => {
      if (pulse) document.dispatchEvent(new Event("visibilitychange"));
      await new Promise(resolve => setTimeout(resolve, 25));
    });
  }
}
function packet(input: Input): DstWorldMap {
  const width = 120, height = 90, id = input.shard === "master" ? 6 : input.shard === "caves" ? 13 : input.shard === "islands" ? 5 : 3;
  return { instanceId: input.instanceId, shard: input.shard, runId: details.active_run?.run_id ?? 7,
    capturedAtUnixMs: Date.now(), session: `world-${generation}-${input.shard}`, geometryRevision: `15:${room}:30:40:${room}`, width, height, tileScale,
    day: generation * 10, season: "autumn", phase: "day", tiles: input.includeTerrain ? Array<number>(width * height).fill(id) : null,
    tileNames: input.includeTerrain ? { [id]: `FIXTURE_GENERATION_${generation}_${input.shard}_ROOM_${room}` } : {},
    tileKinds: input.includeTerrain ? { [id]: "land" } : {}, tileTextures: {}, tileBaseTextures: {}, tileColors: {}, terrainLayers: [],
    landmarks: input.includeTerrain ? [{ id: "portal-1", prefab: "multiplayer_portal", icon: "portal.png", priority: 1, mapX: 24.5, mapZ: 30.5 },
      { id: "portal-2", prefab: "multiplayer_portal", icon: "portal.png", priority: 1, mapX: 80.5, mapZ: 60.5 }] : null,
    roads: input.includeTerrain ? [] : null, landmarksTruncated: false,
    art: input.includeTerrain ? { textures: {}, icons: { "portal.png": icon }, iconFallbacks: [], issues: [], terrainBaseColor: null, terrainBaseColors: {}, terrainEdges: [] } : null,
    players: playerShard === input.shard ? [{ id: "KU_SESSION", name: "Traveller", character: "wilson", icon: null,
      x: 0, z: 0, mapX: 65.25, mapZ: 43.75 }] : [] };
}
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args: { input: Input }) => {
  if (command !== "read_dontstarve_world_map") throw new Error(`Unexpected native IPC: ${command}`);
  calls.push({ ...args.input });
  if (failure || failedShard === args.input.shard) throw new Error(failure || "DST_MAP_RESPONSE_TIMEOUT: background shard unavailable");
  if (hold || holdShard === args.input.shard) return new Promise<DstWorldMap>(resolve => pending.push({ input: args.input, resolve }));
  return packet(args.input);
} } });

async function render(show = true) {
  await act(async () => root.render(<I18nProvider><InstanceSettingsSaveProvider><main style={{ padding: 24, height: "100vh", boxSizing: "border-box" }}>{show
    ? <DstWorldMapWorkbench details={details} panel="world" /> : <p>Other server tool</p>}</main></InstanceSettingsSaveProvider></I18nProvider>));
}
async function foreground() { await act(async () => document.dispatchEvent(new Event("visibilitychange"))); }
async function refresh() {
  const before = calls.length, shard = selectedShard();
  clockOffset += 60001;
  await foreground();
  await wait(() => calls.slice(before).some(call => call.shard === shard && call.includeTerrain), "Automatic terrain polling did not collect after its interval elapsed");
  check(calls.slice(before).some(call => call.shard === shard && call.includeTerrain), "Elapsed terrain polling must request a complete map without a refresh button");
}
async function loaded() { await wait(() => fixture.querySelector(".dst-map-viewport")?.getAttribute("aria-busy") === "false", "Map did not finish painting"); }
function selectedShard() { return element<HTMLSelectElement>(".dst-map-toolbar select").value; }
function center() {
  const viewport = element(".dst-map-viewport"), bounds = viewport.getBoundingClientRect(), plane = element(".dst-map-plane").getBoundingClientRect();
  return { x: (bounds.left + viewport.clientLeft + viewport.clientWidth / 2 - plane.left) / plane.width * 120,
    y: (bounds.top + viewport.clientTop + viewport.clientHeight / 2 - plane.top) / plane.height * 90 };
}
async function prepareStorage() {
  // Browser storage startup is outside the optional1500ms cache operation budget.
  const name = `map-session-ready-${nonce}`;
  await new Promise<void>((resolve, reject) => {
    const request = indexedDB.open(name);
    request.onerror = () => reject(request.error);
    request.onsuccess = () => { request.result.close(); resolve(); };
  });
  await new Promise<void>((resolve, reject) => {
    const request = indexedDB.deleteDatabase(name);
    request.onerror = () => reject(request.error); request.onsuccess = () => resolve();
  });
}

async function run() {
  await act(prepareBrowserLocaleCatalogs);
  await prepareStorage();
  const bootstrap = await invokeMock<BootstrapResponse>("bootstrap", { includeSystemSnapshot: false });
  const instance = bootstrap.state.instances.find(value => value.module_id === "dontstarve")!;
  const original = await invokeMock<InstanceDetails>("read_instance_details_from_storage", { instanceId: instance.id });
  details = { ...original, summary: { ...original.summary, status: "Running", active_process_count: 4 },
    settings_json: JSON.stringify({ shard_layout: "island_adventures" }),
    active_run: { run_id: 7, processes: shards.map(process_key => ({ process_key, status: "Running" })) } } as InstanceDetails;
  await render(); await loaded();
  checkDstMapLayout(check, fixture);
  check(selectedShard() === "master", "Initial shard must be Master");
  check(calls[0]?.includeTerrain === true, "Initial map must collect full terrain");
  await act(async () => element<HTMLButtonElement>(".dst-map-roster li button").click());
  check(Boolean(fixture.querySelector(".dst-map-roster-clear")), "Follow must be explicitly active with a cancel action");

  playerShard = null; failedShard = "caves";
  await foreground();
  await wait(() => fixture.textContent?.includes("部分分片暂未响应") === true, "A failed candidate must be reported while follow remains active", true);
  check(selectedShard() === "master" && Boolean(fixture.querySelector(".dst-map-roster-clear")), "Missing or failed candidates must not clear the followed userid");
  check(fixture.textContent?.includes("等待 Traveller") === true, "A migrating player must be shown as waiting, not silently offline");
  failedShard = null;
  playerShard = "master"; await foreground();
  await wait(() => fixture.querySelectorAll(".dst-map-roster li").length === 1, "The source must recover its last live player before the migration case", true);
  for (const shard of ["caves", "islands", "volcano"] as const) {
    playerShard = shard;
    if (shard === "caves") { failedShard = "master"; holdShard = "caves"; }
    await foreground();
    if (shard === "caves") {
      await wait(() => pending.some(request => request.input.shard === "caves"), "A failed source still showing the player must trigger cross-shard search", true);
      check(selectedShard() === "master" && fixture.querySelectorAll(".dst-map-roster li").length === 1, "Cached source positions must not suppress migration lookup");
      failedShard = null; holdShard = null;
      await act(async () => { for (const request of pending.splice(0)) request.resolve(packet(request.input)); });
    }
    await wait(() => selectedShard() === shard && fixture.querySelector(".dst-map-roster li button")?.getAttribute("aria-pressed") === "true",
      `Following did not migrate to ${shard}`, true);
    await loaded(); migrations.push(shard);
    check(fixture.querySelector(".dst-map-roster li button[aria-pressed=true]")?.textContent?.includes("Traveller"), `Follow identity was lost in ${shard}`);
    const point = center(), target = clampedDstMapTarget(fixture, { width: 120, height: 90 }, { x: 65.25, y: 90 - 43.75 });
    check(Math.abs(point.x - target.x) < .1 && Math.abs(point.y - target.y) < .1, `The actual selected player must center ${shard} within the pan bound`);
    checkDstMapBounded(check, fixture, `Following in ${shard}`);
  }
  check(calls.filter(call => !call.includeTerrain).some(call => call.shard === "islands"), "Cross-shard search must use light position responses");
  await act(async () => element<HTMLButtonElement>(".dst-map-roster-clear").click());
  const cancelledCalls = calls.length;
  playerShard = "master"; await foreground();
  await wait(() => calls.length > cancelledCalls, "Visible map did not refresh after cancellation");
  check(!fixture.querySelector(".dst-map-roster-clear") && selectedShard() === "volcano", "Explicit cancel must stop automatic shard switches");
  check(calls.slice(cancelledCalls).every(call => call.shard === "volcano"), "Cancelled follow must stop candidate scans");

  await act(async () => {
    const zoom = element<HTMLSelectElement>("select.dst-map-zoom");
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!.call(zoom, "2");
    zoom.dispatchEvent(new Event("change", { bubbles: true }));
  });
  await act(async () => element(".dst-map-viewport").dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true, cancelable: true })));
  const savedCenter = center();
  check(element<HTMLSelectElement>("select.dst-map-zoom").value === "2", "The camera must record explicit zoom");
  await render(false);
  await wait(() => !fixture.querySelector("canvas"), "Leaving the map must unmount it");
  let saved = await readDstMapCache(details.summary.id, "volcano");
  const savedDeadline = performance.now() + 5000;
  while (saved?.view?.zoom !== 2 && performance.now() < savedDeadline) {
    await new Promise(resolve => setTimeout(resolve, 25)); saved = await readDstMapCache(details.summary.id, "volcano");
  }
  check(saved?.view?.zoom === 2, "Leaving must persist the camera through real IndexedDB");
  check(Math.abs(saved.view.center.x - savedCenter.x) < .1, "Persisted pan must match the actual viewport");
  hold = true;
  await render();
  await loaded();
  check(selectedShard() === "volcano", "Returning must restore the user's selected shard");
  check(element<HTMLSelectElement>("select.dst-map-zoom").value === "2", "Returning to a cached shard must restore zoom before the server responds");
  check(Math.abs(center().x - savedCenter.x) < .1, "Returning must restore the cached center");
  hold = false;
  await act(async () => { for (const request of pending.splice(0)) request.resolve(packet(request.input)); });
  await loaded();

  playerShard = "volcano"; await foreground();
  await wait(() => fixture.querySelectorAll(".dst-map-players g").length === 1, "A final live position must be captured before stopping");

  details = { ...details, summary: { ...details.summary, status: "Stopped" }, active_run: null };
  await render(); await loaded();
  check(!fixture.textContent?.includes("上次采集") && !fixture.textContent?.includes("分片已停止"), "Stopped shards show the last map without stale explanations");
  check(Boolean(fixture.querySelector(".dst-map-landmarks image")) && Boolean(fixture.querySelector(".dst-map-players g")), "Stopped map must retain landmark artwork and the last player marker");
  const stoppedCalls = calls.length; await foreground();
  check(calls.length === stoppedCalls, "Stopped maps must never dispatch IPC");
  check(![...fixture.querySelectorAll("button")].some(button => /刷新地图/.test(button.textContent ?? "")), "Stopped maps must not expose a permanent refresh action");

  generation = 2; hold = true;
  details = { ...details, summary: { ...details.summary, status: "Running" },
    active_run: { run_id: 8, processes: shards.map(process_key => ({ process_key, status: "Running" })) } } as InstanceDetails;
  await render();
  await wait(() => pending.length > 0, "New native run did not request a full map");
  check(!fixture.querySelector("canvas"), "A new run must not display the previous run's terrain or players before session confirmation");
  hold = false;
  await act(async () => { for (const request of pending.splice(0)) request.resolve(packet(request.input)); });
  await loaded();
  await act(async () => {
    const canvas = element<HTMLCanvasElement>("canvas"), bounds = canvas.getBoundingClientRect();
    canvas.dispatchEvent(new PointerEvent("pointermove", { clientX: Math.round(bounds.left + bounds.width / 2),
      clientY: Math.round(bounds.top + bounds.height / 2), bubbles: true }));
  });
  check(fixture.querySelector(".dst-map-readout")?.textContent?.includes("FIXTURE_GENERATION_2_volcano") === true
    && !fixture.querySelector(".dst-map-readout")?.textContent?.includes("FIXTURE_GENERATION_1_volcano"),
    "Hovering actual new-world terrain must report its new type without borrowing the old world's metadata");
  tileScale = 8;
  const scaleCalls = calls.length; await foreground();
  await wait(() => calls.slice(scaleCalls).some(call => !call.includeTerrain)
    && calls.slice(scaleCalls).some(call => call.includeTerrain), "A position-only tileScale change must force a fresh full map");
  await loaded();
  check(calls.at(-1)?.includeTerrain === true, "Changed geometry must complete a new full collection");

  const roomCalls = calls.length;
  room = 1; await foreground();
  await wait(() => calls.slice(roomCalls).some(call => !call.includeTerrain)
    && calls.slice(roomCalls).some(call => call.includeTerrain), "A room change must immediately request new terrain without waiting sixty seconds");
  await loaded();
  await act(async () => {
    const canvas = element<HTMLCanvasElement>("canvas"), bounds = canvas.getBoundingClientRect();
    canvas.dispatchEvent(new PointerEvent("pointermove", { clientX: Math.round(bounds.left + bounds.width / 2),
      clientY: Math.round(bounds.top + bounds.height / 2), bubbles: true }));
  });
  check(fixture.querySelector(".dst-map-readout")?.textContent?.includes("ROOM_1"), "Room terrain metadata must replace the previous room");
  failure = "DST_MAP_GEOMETRY_CHANGED: virtual room changed during collection"; await refresh();
  await wait(() => !fixture.querySelector("canvas"), "An interrupted room capture must discard obsolete terrain");
  failure = ""; await foreground(); await loaded();

  failure = "DST_MAP_WORLD_NOT_READY: original world has not generated"; await refresh();
  await wait(() => fixture.textContent?.includes("服务器的世界尚未就绪") === true, "World-not-ready error must have its distinct explanation");
  check(!fixture.textContent?.includes("分片暂未返回完整地图"), "World-not-ready must not be mislabeled as a timeout");
  failure = "DST_MAP_RESPONSE_TIMEOUT: timed out"; await refresh();
  await wait(() => fixture.textContent?.includes("分片暂未返回完整地图") === true, "Timeout must have its distinct retry explanation");
  check(Boolean(fixture.querySelector("canvas")), "A collection error must retain the last complete map");
  failure = "";
  await render(false);
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, browser_errors: errors, calls: calls.length, migration_shards: migrations,
    real_indexed_db: true, native_game_run: false, fixture_provenance: "Production Workbench/hooks with synthetic IPC boundary and real browser IndexedDB" };
}
void run().catch(error => ({ status: "failed", checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .finally(() => Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }))
  .then(report => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
