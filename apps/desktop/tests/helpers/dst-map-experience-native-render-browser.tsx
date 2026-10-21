import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { readDstMapCache } from "../../src/dst-map-cache";
import { parseDstWorldMap, type DstWorldMap } from "../../src/dst-world-map";
import { I18nProvider } from "../../src/i18n";
import type { InstanceDetails } from "../../src/types";
import { GMToolsWorkbench } from "../../src/views/servers/GMToolsWorkbench";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { DontStarveShard } from "../../src/views/settings/modules/dontstarve-shards";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import { checkDstMapLayout } from "./dst-map-layout-checks";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

interface ReplayOptions {
  shard: "master" | "caves"; mode: "fit" | "camera";
  instanceId: string; runId: number; case: string;
  sources: Record<"master" | "caves", { sha256: string; capturedAtUnixMs: number }>;
  arrangedStructures: boolean; noCustomTerrainClaimed: boolean;
}
interface Input { instanceId: string; shard: DontStarveShard; includeTerrain: boolean }
const fixture = document.getElementById("fixture")!, root = createRoot(fixture);
const nonce = new URLSearchParams(location.search).get("nonce");
const calls: Input[] = [], errors: string[] = [];
let checks = 0;
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
addEventListener("error", event => errors.push(event.message));
addEventListener("unhandledrejection", event => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
function check(value: unknown, message: string): asserts value { if (!value) throw new Error(message); checks++; }
function element<T extends Element = HTMLElement>(selector: string): T {
  const value = fixture.querySelector<T>(selector);
  if (!value) throw new Error(`Missing production element: ${selector}`);
  return value;
}
async function wait(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 25000;
  while (!predicate()) {
    if (performance.now() > deadline) throw new Error(message);
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 25)); });
  }
}
function camera(snapshot: DstWorldMap) {
  const viewport = element<HTMLElement>(".dst-map-viewport"), bounds = viewport.getBoundingClientRect();
  const plane = element<HTMLElement>(".dst-map-plane").getBoundingClientRect();
  return { x: (bounds.left + viewport.clientLeft + viewport.clientWidth / 2 - plane.left) / plane.width * snapshot.width,
    y: (bounds.top + viewport.clientTop + viewport.clientHeight / 2 - plane.top) / plane.height * snapshot.height,
    scale: plane.width / snapshot.width };
}
async function loaded(width: number) {
  await wait(() => fixture.querySelector(".dst-map-viewport")?.getAttribute("aria-busy") === "false"
    && fixture.querySelector("canvas")?.width === width * 4, "Production Workbench did not finish rendering the captured terrain");
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const response = await fetch("/native-dst-experience-options.json", { cache: "no-store" });
  check(response.ok, "The byte-bound replay options must be supplied");
  const options: ReplayOptions = await response.json();
  check(["master", "caves"].includes(options.shard) && ["fit", "camera"].includes(options.mode), "Replay view must be explicit");
  const snapshots = {} as Record<"master" | "caves", DstWorldMap>;
  for (const shard of ["master", "caves"] as const) {
    const source = await fetch(`/native-dst-experience-${shard}.json?terrain=1`, { cache: "no-store" });
    check(source.ok && source.headers.get("X-Lgsm-Native-Source") === options.sources[shard].sha256, "Replay source hash must match the validated input");
    snapshots[shard] = parseDstWorldMap(await source.json(), options.instanceId, shard, true);
    check(snapshots[shard].runId === options.runId && snapshots[shard].capturedAtUnixMs === options.sources[shard].capturedAtUnixMs,
      "The original native run identity and capture clock must be retained");
  }
  Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args: { input: Input }) => {
    if (command !== "read_dontstarve_world_map") throw new Error(`Unexpected replay IPC: ${command}`);
    const input = args.input;
    check(input.instanceId === options.instanceId && (input.shard === "master" || input.shard === "caves"), "Replay IPC may only read the two bound native shards");
    calls.push({ ...input });
    const source = await fetch(`/native-dst-experience-${input.shard}.json?terrain=${input.includeTerrain ? 1 : 0}`, { cache: "no-store" });
    if (!source.ok) throw new Error(`Replay source HTTP ${source.status}`);
    return source.json();
  } } });
  // Only the IPC boundary and read-only instance metadata are fixture-owned.
  // World geometry, artwork, players, sessions and capture timestamps are original server bytes.
  const details: InstanceDetails = {
    summary: { id: options.instanceId, name: `${options.case} · 历史快照`, module_id: "dontstarve", status: "Running",
      active_process_count: 2, autostart: false, bind_ip: "", port_count: 0 },
    ports: [], settings_json: JSON.stringify({ enable_caves: true }), config_file_path: "", saves_path: "",
    backup_uses_declared_saves_path: true, auto_backup_on_stop: false, backup_retention_count: 1,
    active_run: { run_id: options.runId, processes: ["master", "caves"].map(process_key => ({ process_key, status: "Running" })) },
  };
  async function render(show = true) {
    await act(async () => root.render(<I18nProvider><InstanceSettingsSaveProvider><main style={{ padding: 24, width: "100%", height: "100vh",
      boxSizing: "border-box", display: "grid", gridTemplateRows: "auto minmax(0, 1fr)", gap: 16 }}>
      <header><h1 style={{ margin: 0 }}>真实服务器快照离线回放</h1>
        <p className="dst-map-note">正式地图工作台 · {options.case} · 服务器已经结束运行，保留原始采集时间与历史玩家位置；此页不是实时连接。</p>
        {options.arrangedStructures ? <p className="dst-map-note">建筑为隔离实服的 test_arranged 测试布置，不表示自然生成。</p> : null}
        {options.noCustomTerrainClaimed ? <p className="dst-map-note">本版本样本无独立 Mod 地皮注册，不据此声称覆盖 Mod 地皮。</p> : null}
      </header>
      {show ? <GMToolsWorkbench details={details} /> : <p>已离开地图，正在验证离线相机缓存。</p>}
    </main></InstanceSettingsSaveProvider></I18nProvider>));
    if (show) {
      await wait(() => Boolean(fixture.querySelector('[data-configuration-section-id="world"] button')), "The shared tools sidebar did not mount");
    }
  }
  await render(); await loaded(snapshots.master.width);
  if (options.shard === "caves") {
    await act(async () => {
      const select = element<HTMLSelectElement>(".dst-map-toolbar select");
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!.call(select, "caves");
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await loaded(snapshots.caves.width);
  }
  const snapshot = snapshots[options.shard];
  check(element<HTMLSelectElement>(".dst-map-toolbar select").value === options.shard, "The production shard control must select the captured shard");
  check(fixture.querySelector("h1")?.textContent === "真实服务器快照离线回放", "Offline provenance must stay visible above the production UI");
  check(!fixture.textContent?.includes("已过期"), "The rail must not show a stale history-position indicator");
  check([...fixture.querySelectorAll<HTMLElement>(".dst-map-status[title], .dst-map-status [title]")]
    .some(node => node.title.includes(new Date(snapshot.capturedAtUnixMs).toLocaleString("zh-CN"))), "The original capture timestamp must remain available in the production status title");
  checkDstMapLayout(check, fixture);
  const toolsRail = element<HTMLElement>(".gmt-tools-sidebar").getBoundingClientRect();
  check(toolsRail.right <= element<HTMLElement>(".dst-map-viewport").getBoundingClientRect().left,
    "The replay must show the real shared tools sidebar to the left of the map");
  check(fixture.querySelectorAll(".configuration-section-navigation__button.is-active").length === 1
    && fixture.querySelector('[data-configuration-section-id="world"] .is-active'), "The production shared sidebar must select only the world tools panel");
  check(!fixture.textContent?.includes("地图绘制未完成"), "The original artwork must render without a production error");
  const canvas = element<HTMLCanvasElement>("canvas"), context = canvas.getContext("2d");
  check(context && canvas.width === snapshot.width * 4, "The production terrain canvas must use captured native geometry");
  check(context.getImageData(Math.floor(canvas.width / 2), Math.floor(canvas.height / 2), 1, 1).data[3] > 0,
    "The native terrain must contain actual painted pixels");
  check(element<HTMLSelectElement>("select.dst-map-zoom").value === "1", "The initial map must use the production full-map view");

  let cameraRestored = false;
  if (options.mode === "camera") {
    await act(async () => {
      const zoomSelect = element<HTMLSelectElement>("select.dst-map-zoom");
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!.call(zoomSelect, "4");
      zoomSelect.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await act(async () => element(".dst-map-viewport").dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true, cancelable: true })));
    const selected = camera(snapshot), zoom = Number(element<HTMLSelectElement>("select.dst-map-zoom").value);
    check(zoom === 4 && selected.scale > 0, "The production zoom overlay must change the camera");
    await render(false);
    let cached = await readDstMapCache(options.instanceId, options.shard);
    const deadline = performance.now() + 10000;
    while ((!cached?.view || Math.abs(cached.view.center.x - selected.x) > .1 || cached.view.zoom !== zoom) && performance.now() < deadline) {
      await act(async () => { await new Promise(resolve => setTimeout(resolve, 25)); cached = await readDstMapCache(options.instanceId, options.shard); });
    }
    check(cached?.view?.zoom === zoom && Math.abs(cached.view.center.x - selected.x) < .1,
      `The actual production hook must persist the camera in IndexedDB: ${JSON.stringify({ cachedView: cached?.view, selected, zoom })}`);
    await render(); await loaded(snapshot.width);
    const restored = camera(snapshot);
    check(Number(element<HTMLSelectElement>("select.dst-map-zoom").value) === zoom && Math.abs(restored.x - selected.x) < .1
      && Math.abs(restored.y - selected.y) < .1, "Remounting the production Workbench must restore the saved camera without resetting its center");
    cameraRestored = true;
  }
  await act(async () => document.dispatchEvent(new Event("visibilitychange")));
  await wait(() => calls.some(input => input.shard === options.shard && !input.includeTerrain), "Production polling did not read a position-only projection");
  await loaded(snapshot.width);
  await act(async () => { await new Promise<void>(resolve => requestAnimationFrame(() => resolve())); });
  const viewportBounds = element<HTMLElement>(".dst-map-viewport").getBoundingClientRect();
  check(viewportBounds.top >= 0 && viewportBounds.bottom <= innerHeight && viewportBounds.right <= innerWidth,
    "The production map viewport must remain inside the replay screenshot");
  check(document.documentElement.scrollWidth <= innerWidth + 1, "The complete Workbench must not overflow horizontally");
  checkDstMapLayout(check, fixture);
  const images = [...fixture.querySelectorAll<SVGImageElement>(".dst-map-landmarks image")];
  check(images.length > 0 && images.every(image => image.getBoundingClientRect().width <= 24.1), "Original landmark icons must retain the bounded production size");
  check(fixture.querySelectorAll(".dst-map-players g").length === snapshot.players.length, "Historical player markers must match the original native packet");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, browser_errors: errors, shard: options.shard, mode: options.mode,
    offline_native_replay: true, production_workbench: true, native_game_gpu_capture: false,
    timestamp_unchanged: snapshot.capturedAtUnixMs === options.sources[options.shard].capturedAtUnixMs,
    captured_at_unix_ms: snapshot.capturedAtUnixMs, session: snapshot.session, native_run_id: snapshot.runId,
    camera_restored_from_indexed_db: cameraRestored, historical_players: snapshot.players.length, calls };
}
void run().catch(error => ({ status: "failed", checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .finally(() => Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }))
  .then(report => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
