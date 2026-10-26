import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { invokeMock } from "../../src/api-mock";
import { I18nProvider } from "../../src/i18n";
import type { BootstrapResponse, InstanceDetails, InstanceRuntimeCommandResult } from "../../src/types";
import type { ArkMapScope, ArkMapSnapshot } from "../../src/ark-world-map";
import type { ArkSpawnInput } from "../../src/api-ark-tools";
import { importArkArtwork, readArkImportedArtwork } from "../../src/ark-map-artwork";
import { GMToolsWorkbench } from "../../src/views/servers/GMToolsWorkbench";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";
import "../../src/views/servers/instance-themes.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN"); document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!, root = createRoot(fixture);
const nonce = new URLSearchParams(location.search).get("nonce");
const errors: string[] = [];
addEventListener("error", event => errors.push(event.message));
addEventListener("unhandledrejection", event => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
let checks = 0, details: InstanceDetails, hidden = false, installed = true;
let mode: "live" | "deferred" | "stale" | "raw" | "empty" | "error" | "older" = "live";
let activeMaps = 0, maximumMaps = 0, holdCommand = false, statusCalls = 0;
const mapCalls: ArkMapScope[] = [], commands: Array<ArkMapScope & { command: string }> = [], spawns: ArkSpawnInput[] = [];
let heldMap: { input: ArkMapScope; resolve: (value: ArkMapSnapshot) => void } | null = null;
let heldCommand: { input: ArkMapScope & { command: string }; resolve: (value: InstanceRuntimeCommandResult) => void } | null = null;
Object.defineProperty(document, "hidden", { configurable: true, get: () => hidden });
function packet(input: ArkMapScope): ArkMapSnapshot {
  const base = input.processKey === "main" ? 123 : 456;
  return { ...input, mapName: input.processKey === "main" ? "TheIsland" : "ScorchedEarth_P", sampledAtUnixMs: Date.now() - (mode === "stale" ? 20000 : mode === "older" ? 5000 : 0),
    projection: mode === "raw" ? null : { minX: -400000, maxX: 400000, minY: -400000, maxY: 400000 }, artwork: null,
    players: mode === "empty" ? [] : [
      { playerId: base, name: mode === "older" ? "REPLAY" : `林间旅人 · ${input.runId}`, accountId: "76561190000000001", team: 1000, x: -90000, y: 60000, z: 900 },
      { playerId: base + 1, name: "北岸巡游者", accountId: "00000000000000000000000000000002", team: 1000, x: 120000, y: -100000, z: 1500 },
      { playerId: base + 2, name: "等待角色", accountId: "guest", team: 0, x: null, y: null, z: null }
    ] };
}
function receipt(input: ArkMapScope & { command: string }): InstanceRuntimeCommandResult {
  return { instance_id: input.instanceId, process_key: input.processKey, command: input.command, display_name: "Fixture map", pid: 1,
    response_text: "Fixture native command receipt", write_confirmation_pending: false, submitted_at_unix_ms: Date.now() };
}
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args: { input: ArkMapScope & ArkSpawnInput & { command: string } }) => {
  const input = args.input;
  if (command === "read_ark_tools_status") { statusCalls++; return { installed, connected: installed, issue: null }; }
  if (command === "read_ark_map") {
    mapCalls.push(input); activeMaps++; maximumMaps = Math.max(maximumMaps, activeMaps);
    try {
      if (mode === "error") throw new Error("Fixture native map query failed");
      if (mode === "deferred") return await new Promise<ArkMapSnapshot>(resolve => { heldMap = { input, resolve }; });
      return packet(input);
    } finally { activeMaps--; }
  }
  if (command === "send_ark_map_gm_command") { commands.push(input);
    return holdCommand ? await new Promise<InstanceRuntimeCommandResult>(resolve => { heldCommand = { input, resolve }; }) : receipt(input); }
  if (command === "spawn_ark_creature") { spawns.push(input); return { ...input, creature: { id1: 1234, id2: 5678, className: "Rex_Character_BP_C",
    level: input.level, team: 1000, x: input.x, y: input.y, z: input.z, tamed: input.tamed } }; }
  throw new Error(`Unexpected IPC: ${command}`);
} } });
function check(value: unknown, message: string): asserts value { if (!value) throw new Error(message); checks++; }
function element<T extends Element = HTMLElement>(selector: string): T { const value = fixture.querySelector<T>(selector); if (!value) throw new Error(`Missing ${selector}`); return value; }
async function settle(predicate: () => boolean, message: string) { const until = performance.now() + 6000;
  while (!predicate()) { if (performance.now() > until) throw new Error(`${message}: ${fixture.textContent?.slice(0, 500)}`); await act(async () => { await new Promise(resolve => setTimeout(resolve, 15)); }); } }
async function click(node: Element) { await act(async () => { (node as HTMLElement).click(); }); }
function button(text: string) { const node = [...fixture.querySelectorAll<HTMLButtonElement>("button")].find(value => value.textContent?.trim() === text); if (!node) throw new Error(`Missing button ${text}`); return node; }
async function input(node: HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement, value: string) {
  const proto = node instanceof HTMLSelectElement ? HTMLSelectElement.prototype : node instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  await act(async () => { Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(node, value); node.dispatchEvent(new Event(node instanceof HTMLSelectElement ? "change" : "input", { bubbles: true })); });
}
async function render() { await act(async () => root.render(<StrictMode><I18nProvider><main style={{ padding: 16, height: "100vh", boxSizing: "border-box" }}>
  <GMToolsWorkbench details={details} /></main></I18nProvider></StrictMode>)); }
async function switchMap(value: string) { await input(element<HTMLSelectElement>('.ark-map-toolbar select'), value); }
async function tool(value: string) { await input(element<HTMLSelectElement>('.ark-map-tool-picker select'), value); }
async function restartMain(runId: number) { details = { ...details, active_run: { ...details.active_run!, processes: details.active_run!.processes!.map(p => p.process_key === "main" ? { ...p, run_id: runId } : p) } }; await render(); }
const live = () => fixture.querySelector('.ark-map-canvas')?.getAttribute('data-live') === 'true';
async function refresh() { await click(button("刷新")); }
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const oversized = new Uint8Array(33);
  oversized.set([137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82]);
  const header = new DataView(oversized.buffer); header.setUint32(16, 50000); header.setUint32(20, 50000);
  const nativeDecode = HTMLImageElement.prototype.decode; let decodeCalls = 0, rejectedLarge = false;
  HTMLImageElement.prototype.decode = async () => { decodeCalls++; };
  try { await importArkArtwork('arksurvivalascended', 'TheIsland', new File([oversized], 'huge.png', { type: 'image/png' })); }
  catch { rejectedLarge = true; } finally { HTMLImageElement.prototype.decode = nativeDecode; }
  check(rejectedLarge && decodeCalls === 0, 'Oversized PNG IHDR is rejected before any browser image allocation');
  const bootstrap = await invokeMock<BootstrapResponse>("bootstrap", { includeSystemSnapshot: false });
  const instance = bootstrap.state.instances.find(value => value.module_id === "minecraft")!;
  const base = await invokeMock<InstanceDetails>("read_instance_details_from_storage", { instanceId: instance.id, instance_id: instance.id });
  details = { ...base, summary: { ...base.summary, id: "ark-map-fixture", module_id: "arksurvivalascended", status: "Running", active_process_count: 2 },
    settings_json: JSON.stringify({ map_name: "TheIsland", rcon_enabled: true, admin_password: "fixture-admin-password",
      additional_maps: [{ id: "desert", map_name: "ScorchedEarth_P", name: "焦土", enabled: true }] }), ports: [{ name: "rcon", protocol: "tcp", port: 27020 }],
    active_run: { run_id: 7, processes: [{ process_key: "main", run_id: 7, status: "Running" }, { process_key: "map-desert", run_id: 8, status: "Running" }] } };
  await render(); await settle(live, "Initial map must become live");
  check(mapCalls[0].runId === 7 && mapCalls[0].processKey === "main", "Initial query binds primary process and run");
  check(fixture.querySelectorAll('[data-player-id]').length === 2 && fixture.querySelectorAll('.ark-map-roster-list button').length === 3, "Unspawned players remain listed without fake origin markers");
  check(fixture.textContent?.includes("坐标网格 · 底图不可用") && !fixture.querySelector('svg image'), "No artwork means an explicitly labelled coordinate grid");
  const beforeView = element('svg.ark-map-surface').getAttribute('viewBox');
  await click(element('[aria-label="放大"]')); check(element('svg.ark-map-surface').getAttribute('viewBox') !== beforeView, "Zoom changes the real map viewport");
  await click(button("适配")); check(element('svg.ark-map-surface').getAttribute('viewBox') === beforeView, "Fit restores the map extent");
  const wheelSurface = element<SVGSVGElement>('svg.ark-map-surface'), wheelRect = wheelSurface.getBoundingClientRect();
  let trustedWheel = false;
  wheelSurface.addEventListener('wheel', event => { trustedWheel = event.isTrusted; }, { once: true });
  const scrollBefore = document.scrollingElement!.scrollTop;
  await act(async () => { const response = await fetch(`/__reliability_pointer/${nonce}`, { method: 'POST', body: JSON.stringify({ type: 'wheel', x: wheelRect.left + wheelRect.width / 2, y: wheelRect.top + wheelRect.height / 2, deltaY: -120 }) });
    if (!response.ok) throw new Error('Native wheel injection failed'); });
  await settle(() => wheelSurface.getAttribute('viewBox') !== beforeView, 'Real wheel input must zoom the map');
  check(trustedWheel && document.scrollingElement!.scrollTop === scrollBefore, 'Trusted native wheel zooms without scrolling the surrounding page');
  await click(button('适配'));
  await click(element('.ark-map-roster-list button')); await tool('ark_give_item_to_player');
  await click(element('.gmt-form .ark-map-use-player'));
  check(element<HTMLInputElement>('#gm-ark_give_item_to_player-playerId-input').value === '123', "Player selection uses PlayerID rather than Steam/EOS ID");
  await tool('ark_set_time'); await click(button('取消选择'));
  check(!element<HTMLButtonElement>('.gmt-form button[type="submit"]').disabled,
    'Clearing the selected reward player does not disable world time controls');
  await click(element('.gmt-form button[type="submit"]'));
  check(commands.length === 1 && commands[0].command === 'SetTimeOfDay 12:00' && commands[0].runId === 7,
    'World time still dispatches to the current map after player selection is cleared');
  await tool('ark_destroy_wild_dinos');
  check(!element<HTMLButtonElement>('.gmt-form .inline-confirm-action > button').disabled,
    'Clearing the selected reward player does not disable world cleanup controls');
  await click(element('.gmt-form .inline-confirm-action > button'));
  check(Boolean(fixture.querySelector('.gmt-form .inline-confirm-review')), 'World cleanup retains its destructive-action confirmation');
  await click(element('.gmt-form .inline-confirm-buttons button'));
  await tool('ark_give_item_to_player');
  check(element<HTMLButtonElement>('.gmt-form button[type="submit"]').disabled
    && element<HTMLInputElement>('#gm-ark_give_item_to_player-playerId-input').value === '123',
    'Returning to rewards still rejects the cleared map player instead of silently trusting the saved ID');
  await click(element('.ark-map-roster-list button')); await click(element('.gmt-form .ark-map-use-player'));
  await input(element<HTMLSelectElement>('#gm-ark_give_item_to_player-itemMode-input'), 'batch');
  await input(element<HTMLTextAreaElement>('#gm-ark_give_item_to_player-lines-input'), '9,2,1,0\n76,1,1,0');
  const commandsBeforeBatch = commands.length;
  holdCommand = true; await click(element('.gmt-form button[type="submit"]'));
  check(commands.length === commandsBeforeBatch + 1 && commands.at(-1)?.runId === 7, "First batch mutation carries exact runtime identity");
  await switchMap('map-desert'); await settle(() => live() && mapCalls.at(-1)?.runId === 8, "Sub-map must use its independent run ID");
  holdCommand = false; await act(async () => heldCommand!.resolve(receipt(heldCommand!.input)));
  check(commands.length === commandsBeforeBatch + 1, "Changing map stops unsent batch mutations");
  check(!fixture.textContent?.includes('Fixture native command receipt'), "Old-map receipts never appear on the new map");
  mode = 'deferred'; await refresh(); await settle(() => Boolean(heldMap), 'One map query must be held');
  const held = heldMap!; const countBeforeSwitch = mapCalls.length;
  await switchMap('main'); check(mapCalls.length === countBeforeSwitch, "A map switch cannot overlap an in-flight query");
  mode = 'live'; await act(async () => held.resolve(packet(held.input))); heldMap = null;
  await settle(() => live() && mapCalls.at(-1)?.processKey === 'main', 'Latest map query must resume after the old one settles');
  check(!fixture.textContent?.includes('林间旅人 · 8'), 'Late foreign-map snapshots cannot populate the new roster');
  mode = 'stale'; await restartMain(9); await settle(() => Boolean(fixture.querySelector('.ark-map-roster-list button')), 'Stale roster remains inspectable');
  check(!live() && [...fixture.querySelectorAll<HTMLButtonElement>('.ark-map-roster-list button')].every(b => b.disabled), 'Stale player snapshots cannot be selected');
  mode = 'raw'; await restartMain(10); await settle(live, 'Raw coordinates must render');
  check(fixture.textContent?.includes('原生 X / Y') && button('导入 PNG').disabled, 'Unknown projection preserves native XY and disables image calibration');
  mode = 'empty'; await restartMain(11); await settle(live, 'Empty response still confirms a live map');
  check(fixture.textContent?.includes('当前地图没有在线玩家'), 'An empty roster has a useful state');
  installed = false; await restartMain(12); await settle(() => fixture.textContent?.includes('服务端扩展未安装') ?? false, 'Missing extension state');
  check(!live(), 'Missing extension never implies live player data');
  installed = true; mode = 'error'; await restartMain(13); await settle(() => fixture.textContent?.includes('Fixture native map query failed') ?? false, 'Native map failures remain visible');
  check(!live(), 'Failed map reads disable live selection');
  mode = 'live'; await refresh(); await settle(live, 'Retry recovers the map');
  const correctName = element('.ark-map-roster-list button strong').textContent;
  const connectedStatusCalls = statusCalls;
  mode = 'older'; await refresh(); check(element('.ark-map-roster-list button strong').textContent === correctName, 'Older samples cannot replace current positions'); mode = 'live';
  check(statusCalls === connectedStatusCalls, 'Connected map polling does not rehash the extension payload on each refresh');
  hidden = true; await act(async () => document.dispatchEvent(new Event('visibilitychange')));
  const hiddenCalls = mapCalls.length;
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 3200)); });
  check(mapCalls.length === hiddenCalls, 'Hidden pages pause the three-second poll');
  hidden = false; await act(async () => document.dispatchEvent(new Event('visibilitychange'))); await settle(live, 'Visibility return refreshes immediately');
  check(mapCalls.length > hiddenCalls && maximumMaps === 1, 'Polling stays single-flight across refreshes and map changes');
  await act(async () => { element<HTMLElement>('main').hidden = true; await new Promise(resolve => setTimeout(resolve, 30)); });
  const hiddenPanelCalls = mapCalls.length;
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 3100)); });
  check(mapCalls.length === hiddenPanelCalls, 'A retained but hidden Tools tab pauses map polling');
  await act(async () => { element<HTMLElement>('main').hidden = false; }); await settle(live, 'Showing Tools refreshes the retained map');
  // Synthetic PNG tests storage and browser decoding only; it is never presented as real game artwork.
  const canvas = document.createElement('canvas'); canvas.width = canvas.height = 128;
  const blob = await new Promise<Blob>(resolve => canvas.toBlob(value => resolve(value!), 'image/png'));
  const file = new File([blob], 'fixture-map.png', { type: 'image/png' });
  const transfer = new DataTransfer(); transfer.items.add(file);
  await act(async () => { const node = element<HTMLInputElement>('.ark-map-artwork-control input[type="file"]'); node.files = transfer.files; node.dispatchEvent(new Event('change', { bubbles: true })); });
  await settle(() => Boolean(fixture.querySelector('svg image')), 'Imported PNG should render with projection');
  check(fixture.textContent?.includes('Local PNG: fixture-map.png'), 'Imported images display their real file source');
  let finishDecode: (() => void) | undefined;
  HTMLImageElement.prototype.decode = function () { return nativeDecode.call(this).then(() => new Promise<void>(resolve => { finishDecode = resolve; })); };
  const pendingFile = new DataTransfer(); pendingFile.items.add(new File([blob], 'pending-main.png', { type: 'image/png' }));
  await act(async () => { const node = element<HTMLInputElement>('.ark-map-artwork-control input[type="file"]'); node.files = pendingFile.files; node.dispatchEvent(new Event('change', { bubbles: true })); });
  await settle(() => Boolean(finishDecode), 'The image decoder boundary must be held');
  HTMLImageElement.prototype.decode = nativeDecode;
  await switchMap('map-desert'); await settle(() => live() && mapCalls.at(-1)?.processKey === 'map-desert', 'Switch to a different map');
  check(!button('导入 PNG').disabled, 'An import pending on the old map cannot lock the newly selected map');
  await act(async () => {
    finishDecode!(); const until = performance.now() + 5000;
    while (!(await readArkImportedArtwork('arksurvivalascended', 'TheIsland'))?.source.includes('pending-main.png')) {
      if (performance.now() > until) throw new Error('Old-map import never completed');
      await new Promise(resolve => setTimeout(resolve, 10));
    }
  });
  check(!fixture.querySelector('svg image'), 'Imported map artwork does not leak to another map');
  await switchMap('main'); await settle(() => Boolean(fixture.querySelector('svg image')), 'Same-map import survives a map switch');
  check(fixture.textContent?.includes('Local PNG: pending-main.png'), 'A completed old-map import restores only on its original map');
  mode = 'raw'; await restartMain(14); await settle(live, 'Map without projection can still remove artwork');
  await settle(() => [...fixture.querySelectorAll('button')].some(node => node.textContent?.trim() === '移除底图'), 'The saved artwork control must finish loading');
  if (!element<HTMLDetailsElement>('.ark-map-artwork-control').open) await click(element('.ark-map-artwork-control summary'));
  await click(button('移除底图')); await settle(() => !fixture.textContent?.includes('本地导入'), 'Removing artwork works without projection');
  await act(async () => check(await readArkImportedArtwork('arksurvivalascended', 'TheIsland') === null, 'Removal affects the selected local map only'));
  await act(async () => new Promise<void>((resolve, reject) => { const open = indexedDB.open('lsgm.ark-map-artwork', 1);
    open.onerror = () => reject(open.error); open.onsuccess = () => { const db = open.result, tx = db.transaction('maps', 'readwrite');
      tx.objectStore('maps').put({ dataUrl: 'corrupt', source: 'fixture corruption' }, JSON.stringify(['arksurvivalascended', 'scorchedearth_p']));
      tx.oncomplete = () => { db.close(); resolve(); }; tx.onerror = () => { db.close(); reject(tx.error); }; }; }));
  await switchMap('map-desert');
  await settle(() => fixture.textContent?.includes('The saved map image is invalid') ?? false, 'Invalid cached artwork must report its error');
  if (!element<HTMLDetailsElement>('.ark-map-artwork-control').open) await click(element('.ark-map-artwork-control summary'));
  check(!button('移除底图').disabled, 'A known corrupt cache remains removable without a decoded artwork object');
  await click(button('移除底图'));
  await settle(() => !fixture.textContent?.includes('The saved map image is invalid'), 'Removing a corrupt cache clears its error');
  await act(async () => check(await readArkImportedArtwork('arksurvivalascended', 'ScorchedEarth_P') === null, 'Only the corrupt map cache is removed'));
  await switchMap('main');
  mode = 'live'; await restartMain(15); await settle(live, 'Final live map');
  await click(element('.ark-map-roster-list button')); await tool('ark_spawn_creature');
  await click(element('.ark-creature-spawner .ark-map-use-player'));
  await input(element<HTMLSelectElement>('.ark-creature-spawner__coordinates select'), 'true');
  await click(element('.ark-creature-spawner button[type="submit"]'));
  check(spawns.length === 1 && spawns[0].processKey === 'main' && spawns[0].runId === 15 && spawns[0].playerId === 123
    && spawns[0].x === -90000 && spawns[0].y === 60000 && spawns[0].z === 900, 'Selected native player supplies confirmed spawn position and independent PlayerID');
  check(fixture.textContent?.includes('ID 1234:5678'), 'Typed native spawn verification remains visible');
  await tool('ark_give_item_to_player'); await click(element('.gmt-form .ark-map-use-player'));
  await input(element<HTMLSelectElement>('#gm-ark_give_item_to_player-itemMode-input'), 'batch');
  await input(element<HTMLTextAreaElement>('#gm-ark_give_item_to_player-lines-input'), '9,1,1,0\n76,1,1,0');
  holdCommand = true; await click(element('.gmt-form button[type="submit"]'));
  const commandsAtStop = commands.length;
  details = { ...details, active_run: { ...details.active_run!, processes: details.active_run!.processes!.map(p => p.process_key === 'main' ? { ...p, status: 'Stopped' } : p) } };
  await render(); holdCommand = false; await act(async () => heldCommand!.resolve(receipt(heldCommand!.input)));
  check(commands.length === commandsAtStop && !live(), 'Stopping the selected process stops unsent commands even while another map runs');
  details = { ...details, active_run: { ...details.active_run!, processes: details.active_run!.processes!.map(p => p.process_key === 'main' ? { ...p, status: 'Running' } : p) } };
  await render(); await settle(live, 'Restarted selected map must refresh before selection');
  await click(element('.ark-map-roster-list button')); await click(element('.gmt-form .ark-map-use-player'));
  await input(element<HTMLInputElement>('.gmt-ark-catalog-search'), 'stone');
  element<HTMLElement>('.ark-map-rail').scrollTop = 0;
  const mapBox = element('.ark-map-canvas').getBoundingClientRect(), railBox = element('.ark-map-rail').getBoundingClientRect();
  const marker = element<SVGCircleElement>('.ark-map-player circle'), label = element<SVGTextElement>('.ark-map-player text');
  const matrix = label.getScreenCTM()!, screenScale = Math.hypot(matrix.a, matrix.b);
  check(Number(label.getAttribute('font-size')) * screenScale >= 12.5 && marker.getBoundingClientRect().width >= 11.5,
    'Player labels and markers retain readable CSS pixel sizes at either viewport');
  const itemName = element<HTMLElement>('.gmt-ark-catalog-option-name');
  check(itemName.textContent === '石头' && itemName.getBoundingClientRect().width >= 70,
    'The narrow tool rail retains readable localized item names rather than squeezing them behind blueprint codes');
  check(mapBox.width > 300 && mapBox.height > 400 && railBox.width >= 280 && mapBox.right <= railBox.left, 'Map and tool rail occupy distinct usable regions');
  check(document.documentElement.scrollWidth <= innerWidth + 1, 'Map workbench has no horizontal overflow');
  check(errors.length === 0, `Browser errors: ${errors.join('; ')}`);
  return { status: 'passed', checks, browser_errors: errors, maximum_map_requests: maximumMaps, map_calls: mapCalls.length,
    commands: commands.length, spawns: spawns.length, viewport: { width: innerWidth, height: innerHeight } };
}
void run().catch(error => ({ status: 'failed', checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .finally(() => Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }))
  .then(report => fetch(`/__reliability_result/${nonce}`, { method: 'POST', body: JSON.stringify(report) }));
