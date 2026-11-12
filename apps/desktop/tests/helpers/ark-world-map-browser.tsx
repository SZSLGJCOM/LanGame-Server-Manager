import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { invokeMock } from "../../src/api-mock";
import { I18nProvider, useI18n, type LocaleCode } from "../../src/i18n";
import type { BootstrapResponse, InstanceDetails, InstanceRuntimeCommandResult } from "../../src/types";
import type { ArkMapScope, ArkMapSnapshot } from "../../src/ark-world-map";
import type { ArkSpawnInput } from "../../src/api-ark-tools";
import { GMToolsWorkbench } from "../../src/views/servers/GMToolsWorkbench";
import { ArkToolsInstallationProvider } from "../../src/components/ArkToolsInstallationContext";
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
let changeLocale: ((locale: LocaleCode) => void) | undefined;
function LocaleControl() { changeLocale = useI18n().setLocale; return null; }
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
  if (command === "read_ark_tools_install_progress") return null;
  if (command === "read_ark_tools_status") { statusCalls++; return { installed, updateAvailable: false, connected: installed, canUninstall: installed,
    issue: installed ? null : "Prepare the ARK server extension while this instance is stopped" }; }
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
async function pointerClick(node: Element) {
  const bounds = node.getBoundingClientRect();
  let trustedClick = false;
  const clicked = (event: Event) => { trustedClick = event.isTrusted; };
  node.addEventListener('click', clicked);
  try {
    await act(async () => {
      const response = await fetch(`/__reliability_pointer/${nonce}`, { method: 'POST', body: JSON.stringify({
        type: 'click', x: bounds.left + bounds.width / 2, y: bounds.top + bounds.height / 2,
      }) });
      if (!response.ok) throw new Error('Native click injection failed');
    });
  } finally { node.removeEventListener('click', clicked); }
  check(trustedClick, 'Player selection uses trusted browser pointer input');
}
async function nativeKey(key: 'Tab' | 'Enter') {
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/${key}`, { method: 'POST' });
    if (!response.ok) throw new Error('Native keyboard injection failed');
  });
}
function button(text: string) { const node = [...fixture.querySelectorAll<HTMLButtonElement>("button")].find(value => value.textContent?.trim() === text); if (!node) throw new Error(`Missing button ${text}`); return node; }
async function input(node: HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement, value: string) {
  const proto = node instanceof HTMLSelectElement ? HTMLSelectElement.prototype : node instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  await act(async () => { Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(node, value); node.dispatchEvent(new Event(node instanceof HTMLSelectElement ? "change" : "input", { bubbles: true })); });
}
async function render() { await act(async () => root.render(<StrictMode><I18nProvider><ArkToolsInstallationProvider><LocaleControl /><main style={{ padding: 16, height: "100vh", boxSizing: "border-box" }}>
  <GMToolsWorkbench details={details} /></main></ArkToolsInstallationProvider></I18nProvider></StrictMode>)); }
async function switchMap(value: string) { await input(element<HTMLSelectElement>('.ark-map-toolbar select'), value); }
async function tool(value: string) {
  const target = element<HTMLButtonElement>(`[data-configuration-section-id="${value}"] button`);
  if (!target.getClientRects().length) await click(element('.configuration-workspace__navigation-toggle'));
  await click(target);
}
async function restartMain(runId: number) { details = { ...details, active_run: { ...details.active_run!, processes: details.active_run!.processes!.map(p => p.process_key === "main" ? { ...p, run_id: runId } : p) } }; await render(); }
const live = () => fixture.querySelector('.ark-map-canvas')?.getAttribute('data-live') === 'true';
async function refresh() { await click(button("刷新")); }
async function dragMap(deltaX: number, deltaY: number) {
  const surface = element<SVGSVGElement>('.ark-map-surface'), bounds = surface.getBoundingClientRect();
  let trustedMove = false;
  const moved = (event: PointerEvent) => { if (event.buttons === 1) trustedMove = event.isTrusted; };
  surface.addEventListener('pointermove', moved);
  try {
    await act(async () => {
      const response = await fetch(`/__reliability_pointer/${nonce}`, { method: 'POST', body: JSON.stringify({
        type: 'drag', x: bounds.left + bounds.width * .2, y: bounds.top + bounds.height * .3, deltaX, deltaY,
      }) });
      if (!response.ok) throw new Error('Native drag injection failed');
    });
  } finally { surface.removeEventListener('pointermove', moved); }
  check(trustedMove, 'Map dragging uses trusted browser input, including pointer capture outside the viewport');
}
async function mapKey(key: string) {
  await act(async () => element('.ark-map-surface').dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true })));
}
function checkMapBounds(message: string) {
  const viewport = element('.ark-map-surface').getBoundingClientRect(), map = element('.ark-map-grid').getBoundingClientRect();
  for (const [start, end, length] of [['left', 'right', 'width'], ['top', 'bottom', 'height']] as const) {
    check(map[length] <= viewport[length] + 1
      ? Math.abs((map[start] + map[end]) / 2 - (viewport[start] + viewport[end]) / 2) < 1
      : map[start] <= viewport[start] + 1 && map[end] >= viewport[end] - 1,
    `${message}: the rendered ${length} stays centered when fitted, otherwise covers the viewport`);
  }
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const bootstrap = await invokeMock<BootstrapResponse>("bootstrap", { includeSystemSnapshot: false });
  const instance = bootstrap.state.instances.find(value => value.module_id === "minecraft")!;
  const base = await invokeMock<InstanceDetails>("read_instance_details_from_storage", { instanceId: instance.id, instance_id: instance.id });
  details = { ...base, summary: { ...base.summary, id: "ark-map-fixture", module_id: "arksurvivalascended", status: "Running", active_process_count: 2 },
    settings_json: JSON.stringify({ map_name: "TheIsland", rcon_enabled: true, admin_password: "fixture-admin-password",
      additional_maps: [{ id: "desert", map_name: "ScorchedEarth_P", name: "焦土", enabled: true }] }), ports: [{ name: "rcon", protocol: "tcp", port: 27020 }],
    active_run: { run_id: 7, processes: [{ process_key: "main", run_id: 7, status: "Running" }, { process_key: "map-desert", run_id: 8, status: "Running" }] } };
  await render(); await settle(live, "Initial map must become live");
  check(!fixture.querySelector('.ark-map-rail input[type="file"]') && !fixture.textContent?.includes('导入 PNG')
    && !fixture.textContent?.includes('常用地图会按游戏版本'), 'The map workbench has no manual artwork import or redundant source instructions');
  check(mapCalls[0].runId === 7 && mapCalls[0].processKey === "main", "Initial query binds primary process and run");
  check(fixture.querySelectorAll('[data-player-id]').length === 2 && fixture.querySelectorAll('.ark-map-roster-list button').length === 3, "Unspawned players remain listed without fake origin markers");
  check(fixture.querySelectorAll('.ark-map-grid line').length === 22 && !fixture.querySelector('svg image'), "No artwork retains the coordinate grid");
  check(fixture.querySelectorAll('.gmt-tools-sidebar [data-configuration-section-id]').length === 2
    && !fixture.querySelector('.ark-map-tool-picker'), 'Only the two spawn panels appear in the shared left navigation');
  if (innerWidth > 1100) check(element('.gmt-tools-sidebar').getBoundingClientRect().right <= element('.ark-map-canvas').getBoundingClientRect().left,
    'The spawn navigation is on the far left, before the map');
  const originalCanvas = element('.ark-map-surface');
  const beforeView = element('svg.ark-map-surface').getAttribute('viewBox');
  await dragMap(80, 60);
  check(element('svg.ark-map-surface').getAttribute('viewBox') === beforeView, 'Dragging at the minimum zoom cannot displace the full map');
  for (const key of ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown']) await mapKey(key);
  check(element('svg.ark-map-surface').getAttribute('viewBox') === beforeView, 'Arrow keys cannot displace a fully visible map');
  checkMapBounds('Minimum zoom');
  await click(element('[aria-label="放大"]')); check(element('svg.ark-map-surface').getAttribute('viewBox') !== beforeView, "Zoom changes the real map viewport");
  const centeredZoom = element('svg.ark-map-surface').getAttribute('viewBox');
  await dragMap(30, 20);
  check(element('svg.ark-map-surface').getAttribute('viewBox') !== centeredZoom, 'A zoomed map can still pan');
  checkMapBounds('Zoomed drag');
  for (const distance of [2048, -2048]) { await dragMap(distance, distance); checkMapBounds('Drag beyond the map edge'); }
  for (const key of ['ArrowLeft', 'ArrowUp', 'ArrowRight', 'ArrowDown']) {
    for (let step = 0; step < 12; step++) await mapKey(key);
    checkMapBounds(`Repeated ${key}`);
  }
  await click(element('[aria-label="缩小"]'));
  check(element('svg.ark-map-surface').getAttribute('viewBox') === beforeView, 'Zooming out to 100% recenters an edge-panned map');
  await click(element('[aria-label="放大"]'));
  await click(element('[aria-label="适应窗口"]')); check(element('svg.ark-map-surface').getAttribute('viewBox') === beforeView, "Fit restores the map extent");
  await input(element<HTMLSelectElement>('.map-zoom-select'), '12');
  check(element<HTMLButtonElement>('[aria-label="放大"]').disabled && element<HTMLSelectElement>('.map-zoom-select').value === '12',
    'The shared zoom selector honors ARK\'s existing maximum');
  await input(element<HTMLSelectElement>('.map-zoom-select'), '1');
  check(element<HTMLButtonElement>('[aria-label="缩小"]').disabled && element('svg.ark-map-surface').getAttribute('viewBox') === beforeView,
    'Selecting 100% restores the fitted ARK map and disables further zooming out');
  const wheelSurface = element<SVGSVGElement>('svg.ark-map-surface'), wheelRect = wheelSurface.getBoundingClientRect();
  let trustedWheel = false;
  wheelSurface.addEventListener('wheel', event => { trustedWheel = event.isTrusted; }, { once: true });
  const scrollBefore = document.scrollingElement!.scrollTop;
  await act(async () => { const response = await fetch(`/__reliability_pointer/${nonce}`, { method: 'POST', body: JSON.stringify({ type: 'wheel', x: wheelRect.left + wheelRect.width / 2, y: wheelRect.top + wheelRect.height / 2, deltaY: -120 }) });
    if (!response.ok) throw new Error('Native wheel injection failed'); });
  await settle(() => wheelSurface.getAttribute('viewBox') !== beforeView, 'Real wheel input must zoom the map');
  check(trustedWheel && document.scrollingElement!.scrollTop === scrollBefore, 'Trusted native wheel zooms without scrolling the surrounding page');
  await click(element('[aria-label="适应窗口"]'));
  await pointerClick(element('.ark-map-roster-list button')); await tool('ark_give_item_to_player');
  const rosterSelectedFill = getComputedStyle(element('[data-player-id="123"] circle')).fill;
  check(rosterSelectedFill === 'rgb(251, 191, 36)', 'Roster selection makes the player marker yellow');
  check(!fixture.querySelector('.ark-map-selected') && !/取消选择|停止跟随|跟随玩家/.test(element('.ark-map-roster').textContent ?? ''),
    'Selecting a player only highlights the roster and marker, without a detail row or extra selection controls');
  check(element('svg.ark-map-surface').getAttribute('viewBox') === beforeView, 'Following a player at 100% does not move the map outside its fitted bounds');
  await click(element('[aria-label="放大"]')); checkMapBounds('Following a player near the zoomed edge');
  await click(element('[aria-label="适应窗口"]'));
  check(!fixture.querySelector('.gmt-form .ark-map-use-player, #gm-ark_give_item_to_player-playerId-input')
    && !element<HTMLButtonElement>('.gmt-form button[type="submit"]').disabled,
    'Selecting a player enables item delivery without another button or a manual ID field');
  await tool('ark_spawn_creature'); await tool('ark_give_item_to_player');
  check(element('.ark-map-surface') === originalCanvas && !element<HTMLButtonElement>('.gmt-form button[type="submit"]').disabled,
    'Left navigation preserves the mounted map, selected player and reward target');
  await click(element('.ark-map-roster-list button'));
  check(!fixture.querySelector('.ark-map-roster-list [aria-pressed="true"], .ark-map-player[aria-pressed="true"]')
    && element<HTMLButtonElement>('.gmt-form button[type="submit"]').disabled,
    'Clicking the selected roster entry again clears both selection highlights and the tool target');
  const firstMarker = element('[data-player-id="123"]');
  const firstCircle = element('[data-player-id="123"] circle');
  const markerRadius = firstCircle.getAttribute('r'), markerStroke = getComputedStyle(firstCircle).stroke;
  check(getComputedStyle(firstCircle).fill === 'rgb(125, 211, 252)', 'Unselected player markers are blue');
  await pointerClick(element('[data-player-id="123"] circle'));
  check(firstMarker.getAttribute('aria-pressed') === 'true' && element('.ark-map-roster-list button').getAttribute('aria-pressed') === 'true',
    'Clicking a map marker selects the same player in the roster');
  check(document.activeElement === firstMarker && getComputedStyle(firstMarker).outlineStyle === 'none'
    && getComputedStyle(firstCircle).fill === rosterSelectedFill
    && firstCircle.getAttribute('r') === markerRadius && getComputedStyle(firstCircle).stroke === markerStroke,
    `Pointer-selected markers use the same yellow fill without a label-enclosing focus outline: ${getComputedStyle(firstMarker).outline}`);
  await pointerClick(element('[data-player-id="123"] circle'));
  check(firstMarker.getAttribute('aria-pressed') === 'false' && element<HTMLButtonElement>('.gmt-form button[type="submit"]').disabled
    && element<HTMLButtonElement>('[aria-label="生成生物"]').disabled
    && getComputedStyle(firstCircle).fill === 'rgb(125, 211, 252)' && getComputedStyle(firstMarker).outlineStyle === 'none',
    'Clicking the selected map marker again clears the item and creature targets');
  await act(async () => element<SVGSVGElement>('.ark-map-surface').focus());
  await nativeKey('Tab');
  check(document.activeElement === firstMarker && firstMarker.matches(':focus-visible')
    && getComputedStyle(firstMarker).outlineStyle === 'none'
    && getComputedStyle(firstCircle).fill === 'rgb(125, 211, 252)' && getComputedStyle(firstCircle).stroke === markerStroke,
    'Focusing a player does not add a frame or change its unselected blue dot');
  await nativeKey('Enter');
  check(firstMarker.getAttribute('aria-pressed') === 'true'
    && getComputedStyle(element('[data-player-id="123"] circle')).fill === rosterSelectedFill,
    'Keyboard activation uses the same yellow selection as the roster and pointer');
  await nativeKey('Enter');
  for (const key of ['Enter', ' ']) {
    await act(async () => firstMarker.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true })));
    check(firstMarker.getAttribute('aria-pressed') === String(key === 'Enter'), 'Keyboard activation also toggles player selection');
  }
  check(!element<HTMLButtonElement>('.ark-world-tools button[type="submit"]').disabled,
    'Clearing the selected reward player does not disable world time controls');
  await click(element('.ark-world-tools button[type="submit"]'));
  check(commands.length === 1 && commands[0].command === 'SetTimeOfDay 12:00' && commands[0].runId === 7,
    'World time still dispatches to the current map after player selection is cleared');
  check(!element<HTMLButtonElement>('.ark-world-tools .inline-confirm-action > button').disabled,
    'Clearing the selected reward player does not disable world cleanup controls');
  await click(element('.ark-world-tools .inline-confirm-action > button'));
  check(Boolean(fixture.querySelector('.ark-world-tools .inline-confirm-review')) && Boolean(fixture.querySelector('.ark-item-spawn')),
    'World cleanup retains its destructive-action confirmation alongside the active spawn panel');
  const cleanupReview = element('.ark-world-tools .inline-confirm-review').getBoundingClientRect();
  check(element('.ark-world-tools').getBoundingClientRect().top >= element('.gmt-form button[type="submit"]').getBoundingClientRect().bottom
    && cleanupReview.bottom <= element('.ark-map-rail').getBoundingClientRect().bottom,
    `Cleanup confirmation remains visible without covering the item action: ${JSON.stringify({ review: cleanupReview.toJSON(),
      world: element('.ark-world-tools').getBoundingClientRect().toJSON(), item: element('.gmt-form button[type="submit"]').getBoundingClientRect().toJSON(),
      rail: element('.ark-map-rail').getBoundingClientRect().toJSON() })}`);
  await click(element('.ark-world-tools .inline-confirm-buttons button'));
  check(element<HTMLButtonElement>('.gmt-form button[type="submit"]').disabled
    && !fixture.querySelector('#gm-ark_give_item_to_player-playerId-input'),
    'Returning to rewards still rejects the cleared map player instead of silently trusting the saved ID');
  await click(element('.ark-map-roster-list button'));
  await click(element('.gmt-form button[type="submit"]'));
  check(commands.at(-1)?.command.startsWith('GiveItemToPlayer 123 "'),
    'Selecting the first player directly sends their in-game PlayerID rather than their account ID');
  await click(element('.ark-map-roster-list button:nth-child(2)'));
  await click(element('.gmt-form button[type="submit"]'));
  check(commands.at(-1)?.command.startsWith('GiveItemToPlayer 124 "'),
    'Changing selection immediately changes the item recipient');
  await click(element('.ark-map-roster-list button:nth-child(3)'));
  await click(element('.gmt-form button[type="submit"]'));
  check(commands.at(-1)?.command.startsWith('GiveItemToPlayer 125 "'),
    'Item delivery needs the current PlayerID, not character coordinates');
  await click(element('.gmt-form .ark-spawn-options summary'));
  await input(element<HTMLSelectElement>('[aria-label="发放目标来源"]'), 'manual');
  check(element<HTMLButtonElement>('.gmt-form button[type="submit"]').disabled,
    'Manual targeting requires its own valid ID rather than copying an earlier selection');
  await input(element<HTMLInputElement>('#gm-ark_give_item_to_player-playerId-input'), '777');
  await click(element('.ark-map-roster-list button'));
  await refresh();
  await click(element('.gmt-form .ark-spawn-options summary'));
  check(element('.gmt-form .ark-spawn-options summary').textContent?.includes('手动目标已启用'),
    'A collapsed manual override remains visible');
  await click(element('.gmt-form button[type="submit"]'));
  check(commands.at(-1)?.command.startsWith('GiveItemToPlayer 777 "'),
    'Explicit manual targeting survives player selection and polling');
  await click(element('.gmt-form .ark-spawn-options summary'));
  await input(element<HTMLSelectElement>('[aria-label="发放目标来源"]'), 'player');
  await click(element('.gmt-form .ark-spawn-options summary'));
  await click(element('.gmt-form button[type="submit"]'));
  check(commands.at(-1)?.command.startsWith('GiveItemToPlayer 123 "'),
    'Returning to selected-player targeting immediately uses the current selection');
  mode = 'empty'; await refresh();
  const commandsAfterDeparture = commands.length;
  await click(element('.gmt-form button[type="submit"]'));
  check(element<HTMLButtonElement>('.gmt-form button[type="submit"]').disabled && commands.length === commandsAfterDeparture,
    'A departed selected player cannot receive another item command through a cached ID');
  mode = 'live'; await refresh();
  await input(element<HTMLSelectElement>('#gm-ark_give_item_to_player-itemMode-input'), 'batch');
  await input(element<HTMLTextAreaElement>('#gm-ark_give_item_to_player-lines-input'), '9,2,1,0\n76,1,1,0');
  const commandsBeforeBatch = commands.length;
  holdCommand = true; await click(element('.gmt-form button[type="submit"]'));
  check(commands.length === commandsBeforeBatch + 1 && commands.at(-1)?.runId === 7, "First batch mutation carries exact runtime identity");
  await switchMap('map-desert'); await settle(() => live() && mapCalls.at(-1)?.runId === 8, "Sub-map must use its independent run ID");
  check(element<HTMLSelectElement>('[aria-label="发放目标来源"]').value === 'player'
    && element<HTMLButtonElement>('.gmt-form button[type="submit"]').disabled,
    'A map switch clears the earlier recipient and returns to selected-player targeting');
  holdCommand = false; await act(async () => heldCommand!.resolve(receipt(heldCommand!.input)));
  check(commands.length === commandsBeforeBatch + 1, "Changing map stops unsent batch mutations");
  check(!fixture.textContent?.includes('Fixture native command receipt'), "Old-map receipts never appear on the new map");
  await click(element('.ark-world-tools .inline-confirm-action > button'));
  check(Boolean(fixture.querySelector('.ark-world-tools .inline-confirm-review')), 'Cleanup review opens for the selected sub-map');
  mode = 'deferred'; await refresh(); await settle(() => Boolean(heldMap), 'One map query must be held');
  const held = heldMap!; const countBeforeSwitch = mapCalls.length;
  await switchMap('main'); check(mapCalls.length === countBeforeSwitch, "A map switch cannot overlap an in-flight query");
  check(!fixture.querySelector('.ark-world-tools .inline-confirm-review') && commands.length === commandsBeforeBatch + 1,
    'Changing maps cancels the old destructive confirmation without dispatching a command');
  mode = 'live'; await act(async () => held.resolve(packet(held.input))); heldMap = null;
  await settle(() => live() && mapCalls.at(-1)?.processKey === 'main', 'Latest map query must resume after the old one settles');
  check(!fixture.textContent?.includes('林间旅人 · 8'), 'Late foreign-map snapshots cannot populate the new roster');
  mode = 'stale'; await restartMain(9); await settle(() => Boolean(fixture.querySelector('.ark-map-roster-list button')), 'Stale roster remains inspectable');
  check(!live() && [...fixture.querySelectorAll<HTMLButtonElement>('.ark-map-roster-list button')].every(b => b.disabled), 'Stale player snapshots cannot be selected');
  mode = 'raw'; await restartMain(10); await settle(live, 'Raw coordinates must render');
  check([...fixture.querySelectorAll('.ark-map-grid-label text')].some(node => Math.abs(Number(node.textContent)) > 100)
    && !fixture.querySelector('svg image'), 'Unknown projection preserves native XY without an image overlay');
  mode = 'empty'; await restartMain(11); await settle(live, 'Empty response still confirms a live map');
  check(element('.ark-map-roster-heading strong').textContent?.endsWith('· 0') && element('.ark-map-roster-list').textContent === '',
    'An empty roster shows its count without explanatory placeholder text');
  const checksBeforeMissingExtension = statusCalls;
  installed = false; await restartMain(12);
  await settle(() => statusCalls > checksBeforeMissingExtension && element('.ark-map-roster').getAttribute('aria-busy') === 'false', 'Missing extension state');
  check(!live(), 'Missing extension never implies live player data');
  check(element('.ark-map-roster-list').textContent === '', 'An unavailable map does not add player-reading instructions');
  check(!fixture.textContent?.includes('Prepare the ARK server extension'), 'The map rail does not repeat the native setup message above the creature tool');
  installed = true; mode = 'error'; await restartMain(13); await settle(() => fixture.textContent?.includes('Fixture native map query failed') ?? false, 'Native map failures remain visible');
  check(!live(), 'Failed map reads disable live selection');
  const errorWorldTools = element('.ark-world-tools').getBoundingClientRect();
  check(errorWorldTools.top >= element('.gmt-form button[type="submit"]').getBoundingClientRect().bottom
    && errorWorldTools.bottom <= element('.ark-map-rail').getBoundingClientRect().bottom,
    'Map failures preserve separate, reachable item and world actions');
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
  mode = 'live'; await restartMain(15); await settle(live, 'Final live map');
  await click(element('.ark-map-roster-list button')); await tool('ark_spawn_creature');
  check(!fixture.querySelector('.ark-creature-spawner .ark-map-use-player, .ark-creature-spawner__coordinates'),
    'Selecting a map player supplies the default spawn target without an extra button or coordinate fields');
  const creatureList = element('.ark-creature-spawner .gmt-ark-catalog-results').getBoundingClientRect();
  const creatureAction = element('.ark-creature-spawner button[type="submit"]').getBoundingClientRect();
  check(creatureList.height >= 52 && creatureList.bottom < creatureAction.top && creatureAction.bottom <= innerHeight - 16,
    `The creature catalog and fixed actions also fit inside the map rail: ${JSON.stringify({ catalog: creatureList.toJSON(), action: creatureAction.toJSON(), details: element<HTMLDetailsElement>('.ark-creature-spawner .ark-spawn-options').open })}`);
  const worldTools = element('.ark-world-tools').getBoundingClientRect();
  check(worldTools.top >= creatureAction.bottom && worldTools.bottom <= innerHeight - 16,
    'Time and wild-creature cleanup remain pinned below the creature actions');
  await input(element<HTMLSelectElement>('.ark-creature-spawner__parameters select'), 'true');
  await click(element('.ark-creature-spawner button[type="submit"]'));
  check(spawns.length === 1 && spawns[0].processKey === 'main' && spawns[0].runId === 15 && spawns[0].playerId === 123
    && spawns[0].x === -90000 && spawns[0].y === 60000 && spawns[0].z === 900, 'Selected native player supplies confirmed spawn position and independent PlayerID');
  check(fixture.textContent?.includes('ID 1234:5678'), 'Typed native spawn verification remains visible');
  await tool('ark_give_item_to_player');
  await input(element<HTMLSelectElement>('#gm-ark_give_item_to_player-itemMode-input'), 'batch');
  await input(element<HTMLTextAreaElement>('#gm-ark_give_item_to_player-lines-input'), '9,1,1,0\n76,1,1,0');
  holdCommand = true; await click(element('.gmt-form button[type="submit"]'));
  const commandsAtStop = commands.length;
  details = { ...details, active_run: { ...details.active_run!, processes: details.active_run!.processes!.map(p => p.process_key === 'main' ? { ...p, status: 'Stopped' } : p) } };
  await render(); holdCommand = false; await act(async () => heldCommand!.resolve(receipt(heldCommand!.input)));
  check(commands.length === commandsAtStop && !live(), 'Stopping the selected process stops unsent commands even while another map runs');
  details = { ...details, active_run: { ...details.active_run!, processes: details.active_run!.processes!.map(p => p.process_key === 'main' ? { ...p, status: 'Running' } : p) } };
  await render(); await settle(live, 'Restarted selected map must refresh before selection');
  const rewardTarget = element('.ark-map-roster-list button');
  if (rewardTarget.getAttribute('aria-pressed') !== 'true') await click(rewardTarget);
  const firstItem = element('.gmt-form .gmt-ark-catalog-option').getAttribute('title');
  await click(element('.gmt-form .gmt-ark-catalog-page-btn:last-child'));
  check(element('.gmt-form .gmt-ark-catalog-option').getAttribute('title') !== firstItem,
    'Pagination browses another real catalog page without scrolling the rail');
  await input(element<HTMLInputElement>('.gmt-form .gmt-ark-catalog-search'), 'stone');
  await click(element('.gmt-form .gmt-ark-catalog-option'));
  check(element<HTMLInputElement>('#gm-ark_give_item_to_player-blueprintPath-input').value.includes('PrimalItemResource_Stone'),
    'The compact item row still selects its real blueprint path');
  const settingsToggle = element('.gmt-form .ark-spawn-options summary');
  await click(settingsToggle);
  await input(element<HTMLSelectElement>('#gm-ark_give_item_to_player-itemMode-input'), 'number');
  await input(element<HTMLInputElement>('.gmt-form .gmt-ark-catalog-search'), 'stone');
  await click(element('.gmt-form .gmt-ark-catalog-option'));
  check(element<HTMLInputElement>('#gm-ark_give_item_to_player-itemId-input').value === '8',
    'Numeric mode retains the canonical numeric item identity');
  await input(element<HTMLInputElement>('#gm-ark_give_item_to_player-quantity-input'), '2');
  await input(element<HTMLInputElement>('#gm-ark_give_item_to_player-quality-input'), '3');
  await click(settingsToggle);
  await click(element('.gmt-form button[type="submit"]'));
  check(commands.at(-1)?.command === 'GiveItemNumToPlayer 123 8 2 3 0', 'The pinned action uses the selected item, quantity, quality and player');
  await act(async () => changeLocale!('en-US'));
  await settle(() => element('.gmt-form .gmt-ark-catalog-option-name').textContent === 'Stone', 'English catalog labels');
  check(!fixture.querySelector('.ark-map-rail input[type="file"]') && !fixture.textContent?.includes('Import PNG')
    && !fixture.textContent?.includes('Common maps automatically'), 'English maps also omit the manual artwork controls and instructions');
  const englishSend = element('.gmt-form button[type="submit"]').getBoundingClientRect();
  check(englishSend.bottom <= innerHeight - 16 && document.documentElement.scrollWidth <= innerWidth + 1,
    'English controls retain the fixed action position without horizontal overflow');
  await act(async () => changeLocale!('zh-CN'));
  await settle(() => element('.gmt-form .gmt-ark-catalog-option-name').textContent === '石头', 'Chinese catalog labels');
  element<HTMLElement>('.ark-map-rail').scrollTop = 0;
  const mapBox = element('.ark-map-canvas').getBoundingClientRect(), railBox = element('.ark-map-rail').getBoundingClientRect();
  const sendBox = element('.gmt-form button[type="submit"]').getBoundingClientRect();
  check(sendBox.top >= railBox.top && sendBox.bottom <= railBox.bottom + 1,
    'The item action stays visible at the bottom of the rail without scrolling through the catalog');
  const catalogBox = element('.gmt-form .gmt-ark-catalog-results').getBoundingClientRect();
  check(catalogBox.height > 50 && catalogBox.bottom <= sendBox.top,
    `Item results use the available area above the pinned actions: ${JSON.stringify({ catalog: catalogBox.toJSON(), send: sendBox.toJSON(), rail: railBox.toJSON() })}`);
  check(element('.gmt-form .gmt-ark-catalog-option').getBoundingClientRect().height <= 28,
    'The catalog uses compact single-line rows matching the DST picker');
  const marker = element<SVGCircleElement>('.ark-map-player circle'), label = element<SVGTextElement>('.ark-map-player text');
  const matrix = label.getScreenCTM()!, screenScale = Math.hypot(matrix.a, matrix.b);
  check(Number(label.getAttribute('font-size')) * screenScale >= 12.5 && marker.getBoundingClientRect().width >= 11.5,
    'Player labels and markers retain readable CSS pixel sizes at either viewport');
  const itemName = element<HTMLElement>('.gmt-form .gmt-ark-catalog-option-name');
  check(itemName.textContent === '石头' && itemName.getBoundingClientRect().width >= 70,
    'The narrow tool rail retains readable localized item names rather than squeezing them behind blueprint codes');
  check(mapBox.width > 300 && Math.abs(mapBox.width - mapBox.height) < 1 && railBox.width >= 220 && mapBox.right <= railBox.left,
    'The map stays square and the tool rail retains the same minimum width as DST');
  await tool('ark_spawn_creature');
  await settle(() => Number(fixture.querySelector('.ark-creature-spawner .gmt-ark-catalog-results')?.getAttribute('data-page-size')) >= 4,
    'Default player targeting leaves room for at least four complete creature rows');
  const creatureCatalogBox = element('.ark-creature-spawner .gmt-ark-catalog-results').getBoundingClientRect();
  const creatureSubmitBox = element('.ark-creature-spawner button[type="submit"]').getBoundingClientRect();
  check(!fixture.querySelector('.ark-creature-spawner__coordinates, .ark-creature-spawner__owner')
    && !element<HTMLDetailsElement>('.ark-creature-spawner .ark-spawn-options').open
    && creatureCatalogBox.height >= 110 && creatureCatalogBox.bottom < creatureSubmitBox.top && creatureSubmitBox.bottom <= innerHeight - 16,
    `The default creature catalog has priority over optional form fields: ${JSON.stringify({ catalog: creatureCatalogBox.toJSON(), action: creatureSubmitBox.toJSON() })}`);
  await click(element('.ark-creature-spawner .ark-spawn-options summary'));
  await input(element<HTMLSelectElement>('[aria-label="生成位置来源"]'), 'manual');
  const expandedCatalog = element('.ark-creature-spawner .gmt-ark-catalog-results').getBoundingClientRect();
  const expandedOptions = element('.ark-creature-spawner .ark-spawn-options').getBoundingClientRect();
  const expandedAction = element('.ark-creature-spawner button[type="submit"]').getBoundingClientRect();
  check(expandedCatalog.height >= 54 && expandedCatalog.bottom <= expandedOptions.top && expandedOptions.bottom <= expandedAction.top
    && expandedAction.bottom <= innerHeight - 16,
    `Even expanded manual options retain two creature rows and reachable actions: ${JSON.stringify({ catalog: expandedCatalog.toJSON(), options: expandedOptions.toJSON(), action: expandedAction.toJSON() })}`);
  await input(element<HTMLSelectElement>('[aria-label="生成位置来源"]'), 'player');
  await click(element('.ark-creature-spawner .ark-spawn-options summary'));
  check(document.documentElement.scrollWidth <= innerWidth + 1, 'Map workbench has no horizontal overflow');
  await pointerClick(element('[data-player-id="123"] text'));
  check(element('[data-player-id="123"]').getAttribute('aria-pressed') === 'false', 'Clicking the selected player name also clears selection');
  await pointerClick(element('[data-player-id="123"] circle'));
  check(getComputedStyle(element('[data-player-id="123"]')).outlineStyle === 'none'
    && getComputedStyle(element('[data-player-id="123"] circle')).fill === rosterSelectedFill,
    'The final pointer-selected marker stays yellow without any focus frame');
  check(errors.length === 0, `Browser errors: ${errors.join('; ')}`);
  return { status: 'passed', checks, browser_errors: errors, maximum_map_requests: maximumMaps, map_calls: mapCalls.length,
    commands: commands.length, spawns: spawns.length, viewport: { width: innerWidth, height: innerHeight } };
}
void run().catch(error => ({ status: 'failed', checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .finally(() => Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }))
  .then(report => fetch(`/__reliability_result/${nonce}`, { method: 'POST', body: JSON.stringify(report) }));
