import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { invokeMock } from "../../src/api-mock";
import { I18nProvider } from "../../src/i18n";
import type { DstWorldMap } from "../../src/dst-world-map";
import type { BootstrapResponse, InstanceDetails, InstanceRuntimeCommandResult } from "../../src/types";
import { GMToolsWorkbench } from "../../src/views/servers/GMToolsWorkbench";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import { checkDstMapLayout } from "./dst-map-layout-checks";
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
let stage = "start";
type MapInput = { instanceId: string; shard: string; includeTerrain: boolean };
type Command = { instanceId: string; command: string; processKey?: string | null; transport?: string };
const commands: Command[] = [];
let pending = false, mapRun = 2, mapAvailable = true, regenerationRequested = false, releaseRegeneration: () => void = () => undefined;
const SHARDS = ["master", "caves", "islands", "volcano"];

const grass = (() => {
  const canvas = document.createElement("canvas"); canvas.width = 16; canvas.height = 16;
  const context = canvas.getContext("2d")!; context.fillStyle = "#517b39"; context.fillRect(0, 0, 16, 16);
  return canvas.toDataURL("image/png");
})();
function packet(input: MapInput): DstWorldMap {
  const width = 40, height = 40;
  // Only the native IPC boundary is replaced; the production map, roster and GM panels run unchanged.
  const tiles = Array.from({ length: width * height }, (_, index) => {
    const x = index % width - 20, z = Math.floor(index / width) - 20;
    return x * x + z * z < 300 ? 6 : 201;
  });
  return { instanceId: input.instanceId, shard: input.shard as DstWorldMap["shard"], runId: mapRun, capturedAtUnixMs: Date.now(),
    session: `world-${input.shard}`, width, height, tileScale: 4, day: 12, season: "autumn", phase: "day",
    tiles: input.includeTerrain ? tiles : null, tileNames: input.includeTerrain ? { 6: "GRASS", 201: "OCEAN_COASTAL" } : {},
    tileKinds: input.includeTerrain ? { 6: "land", 201: "ocean" } : {},
    tileTextures: input.includeTerrain ? { 6: "images/fixture_grass.tex" } : {}, tileBaseTextures: input.includeTerrain ? { 6: "levels/tiles/map_edge.tex" } : {},
    tileColors: input.includeTerrain ? { 201: [25, 71, 83, 32] } : {}, terrainLayers: [],
    landmarks: input.includeTerrain ? Array.from({ length: 8 }, (_, index) => ({ id: `firepit-${index}`, prefab: "firepit", icon: "firepit.png",
      priority: 0, mapX: 10 + index * 2, mapZ: 12 })) : null, roads: input.includeTerrain ? [] : null, landmarksTruncated: false,
    art: input.includeTerrain ? { textures: { "images/fixture_grass.tex": grass }, icons: {}, terrainBaseColor: null, terrainBaseColors: {}, terrainEdges: [], iconFallbacks: [], issues: [] } : null,
    players: [{ id: "KU_FIXTURE1", name: "Wilson", character: "wilson", icon: null, x: 0, z: 0, mapX: 20, mapZ: 20 },
      { id: "KU_FIXTURE2", name: "Willow", character: "willow", icon: null, x: 24, z: 8, mapX: 26, mapZ: 22 }] };
}
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args: { input: MapInput & Command }) => {
  if (command === "read_dontstarve_world_map") {
    if (!mapAvailable) throw new Error("DST_MAP_UNAVAILABLE: fixture map not ready");
    return packet(args.input);
  }
  if (command === "regenerate_dontstarve_world") {
    regenerationRequested = true;
    await new Promise<void>(resolve => { releaseRegeneration = resolve; });
    return { run_id: mapRun };
  }
  if (command !== "send_instance_gm_command") throw new Error(`Unexpected IPC ${command}`);
  commands.push(args.input);
  const result: InstanceRuntimeCommandResult = { instance_id: args.input.instanceId, command: args.input.command,
    process_key: args.input.processKey ?? "master", display_name: "Fixture shard", pid: 1, response_text: "[LGSM] fixture reply",
    write_confirmation_pending: pending, submitted_at_unix_ms: Date.now() };
  return result;
} } });

function check(value: unknown, message: string): asserts value { if (!value) throw new Error(message); checks++; stage = message; }
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const node = fixture.querySelector<T>(selector);
  if (!node) throw new Error(`Missing ${selector}`);
  return node;
}
async function wait(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 9000;
  while (!predicate()) {
    if (performance.now() > deadline) throw new Error(`${message}: ${fixture.textContent?.slice(0, 400)}`);
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 20)); });
  }
}
const buttonByText = (text: string) => [...fixture.querySelectorAll<HTMLButtonElement>("button")].find(button => button.textContent?.trim() === text);
async function click(text: string) {
  const button = buttonByText(text);
  check(button && !button.disabled, `${text} must be an enabled button: ${[...fixture.querySelectorAll("button")].map(b => `${b.textContent}${b.disabled ? "(x)" : ""}`).join("|")}`);
  await act(async () => button.click());
}
async function dispatched(text: string, before = commands.length) {
  await click(text);
  await wait(() => commands.length === before + 1 && buttonByText("冬季")?.disabled === false, `${text} did not dispatch exactly once`);
  return commands.at(-1)!;
}
async function render(details: InstanceDetails, map = true) {
  await act(async () => root.render(<I18nProvider><InstanceSettingsSaveProvider><main style={{ padding: 24, height: "100vh", boxSizing: "border-box" }}>
    <GMToolsWorkbench details={details} /></main></InstanceSettingsSaveProvider></I18nProvider>));
  await wait(() => map ? Boolean(fixture.querySelector(".dst-map-roster li button")) : Boolean(buttonByText("冬季")),
    map ? "The map roster never listed the fixture player" : "The GM grid must render without a map");
}
/** The world group (12) and player group (6) share four aligned columns, pinned to the rail's bottom under the player chips. */
function checkGrid(state: string) {
  const groups = [...fixture.querySelectorAll<HTMLElement>(".dst-gm-tools .dst-gm-tool-group")];
  const cells = groups.map(group => [...group.querySelectorAll<HTMLElement>(".dst-gm-tool")].map(node => node.getBoundingClientRect()));
  const columns = cells[0]?.slice(0, 4) ?? [];
  check(groups.length === 2 && cells[0].length === 12 && cells[1].length === 6 && cells.flat().every((box, index, all) => box.width > 0 && box.height === all[0].height
    && columns.some(column => Math.abs(column.left - box.left) <= 1 && Math.abs(column.width - box.width) <= 1)),
  `${state}: GM tools must be 12 world and 6 player buttons in four aligned columns`);
  check(cells.every(group => group.every((box, index) => Math.abs(box.left - columns[index % 4].left) <= 1)), `${state}: each group fills its rows left to right`);
  checkDock(state);
  checkRailFits(state);
}
/** The chip strip sits directly above the GM grid and both stay at the rail's bottom edge. */
function checkDock(state: string) {
  const rail = element(".dst-map-sidebar"), dock = element(".dst-map-rail-dock"), chips = element(".dst-map-roster"), tools = fixture.querySelector(".dst-gm-tools");
  const bottom = rail.getBoundingClientRect().bottom - parseFloat(getComputedStyle(rail).paddingBottom);
  check(Math.abs(dock.getBoundingClientRect().bottom - bottom) <= 1 && dock.contains(chips), `${state}: the player strip and GM grid must be pinned to the rail bottom`);
  check(chips.getBoundingClientRect().height <= 60, `${state}: the player strip must stay slim (${chips.getBoundingClientRect().height}px)`);
  if (tools) check(chips.getBoundingClientRect().bottom <= tools.getBoundingClientRect().top + 1 && dock.lastElementChild === tools, `${state}: the chips sit directly above the GM grid`);
}
const toolsTop = () => Math.round(element(".dst-gm-tools").getBoundingClientRect().top - element(".dst-map-sidebar").getBoundingClientRect().top);
async function clickMap(point: { x: number; y: number }) {
  const init = { pointerId: 21, button: 0, clientX: point.x, clientY: point.y, bubbles: true, cancelable: true };
  await act(async () => element(".dst-map-viewport").dispatchEvent(new PointerEvent("pointerdown", { ...init, buttons: 1 })));
  await act(async () => window.dispatchEvent(new PointerEvent("pointerup", { ...init, buttons: 0 })));
}
function markerCenter(index: number) {
  const box = fixture.querySelectorAll(".dst-map-players g circle")[index].getBoundingClientRect();
  return { x: box.left + box.width / 2, y: box.top + box.height / 2 };
}
async function setField(selector: string, value: string) {
  await act(async () => {
    const field = element<HTMLInputElement>(selector);
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(field, value);
    field.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
const setAmount = (value: string) => setField(".dst-gm-amount input[type=number]", value);
const setSearch = (value: string) => setField(".gmt-dst-prefab-search", value);
const options = () => [...fixture.querySelectorAll<HTMLButtonElement>(".gmt-dst-prefab-option")];
const pager = () => element(".gmt-dst-prefab-pager").textContent?.match(/(\d+)\s*\/\s*(\d+)/)?.slice(1).map(Number) ?? [];
/** A page holds exactly the rows the results box fits (sized by the rail, not its items), without a scrollbar. */
async function checkPage(state: string, total?: number) {
  const list = element(".gmt-dst-prefab-results"), gap = parseFloat(getComputedStyle(list).rowGap) || 0;
  const fit = () => Math.max(1, Math.floor((list.clientHeight + gap) / (26 + gap)));
  // The pager showing or hiding resizes the box; the observer re-measures on the next frame.
  await wait(() => list.dataset.pageSize === String(fit()), `${state}: the page size must follow the list height`);
  const rows = fit(), shown = options(), [page, pages] = pager();
  const bottom = list.getBoundingClientRect().bottom;
  check(Number(list.dataset.pageSize) === rows && (page < pages ? shown.length === rows : shown.length <= rows) && shown.every(node => node.getBoundingClientRect().bottom <= bottom + 1)
    && list.scrollHeight <= list.clientHeight + 1 && getComputedStyle(list).overflowY !== "auto" && (total === undefined || pages === Math.ceil(total / rows)),
  `${state}: the page must fill the ${list.clientHeight}px list with ${rows} rows without scrolling (${shown.length} shown, page ${page}/${pages})`);
  return rows;
}
const waitPageSize = (size: number) => wait(() => element(".gmt-dst-prefab-results").dataset.pageSize === String(size), `Page size never became ${size}`);
async function clickNextUntilLast() {
  const next = () => [...element(".gmt-dst-prefab-pager").querySelectorAll<HTMLButtonElement>("button")].at(-1)!;
  for (let guard = 0; !next().disabled && guard < 1000; guard++) await act(async () => next().click());
}
/** Search and results on top; categories, then 所有人 | 进背包 | 数量 | 发放, pinned under the chips in four GM columns. */
function checkSpawnLayout(state: string) {
  const main = element(".dst-map-rail-main"), dock = element(".dst-map-rail-dock");
  check(main.contains(element(".gmt-dst-prefab-search")) && main.contains(element(".gmt-dst-prefab-results")) && !main.querySelector(".dst-gm-tool"),
    `${state}: the search box and results sit in the flexible top area`);
  const cells = [...dock.querySelectorAll<HTMLElement>(".dst-gm-tools .dst-gm-tool, .dst-gm-tools .dst-gm-amount")].map(node => node.getBoundingClientRect());
  const last = cells.slice(-4), columns = cells.slice(0, 4);
  const labels = [...dock.querySelectorAll(".dst-gm-tools > .dst-gm-tool-grid:last-child > *")].map(node => node.textContent?.trim());
  check(cells.length === 14 && labels.join() === "所有人,进背包,数量,发放" && cells.every(box => Math.abs(box.height - cells[0].height) <= 1)
    && last.every((box, index) => Math.abs(box.left - columns[index].left) <= 1 && Math.abs(box.width - columns[index].width) <= 1 && Math.abs(box.top - last[0].top) <= 1),
  `${state}: categories and the target row must share the four GM columns and height: ${labels}`);
  const amount = getComputedStyle(element(".dst-gm-amount")), tool = getComputedStyle(element(".dst-gm-tools .secondary-button.dst-gm-tool"));
  check(amount.borderTopLeftRadius === tool.borderTopLeftRadius && amount.borderTopWidth === tool.borderTopWidth, `${state}: the amount cell matches the button border and radius`);
  check(element(".dst-map-roster").getBoundingClientRect().bottom <= element(".dst-gm-tools").getBoundingClientRect().top + 1, `${state}: the chips sit directly above the grid`);
}
async function selectShard(shard: string) {
  await act(async () => {
    const field = element<HTMLSelectElement>(".dst-map-toolbar select");
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!.call(field, shard);
    field.dispatchEvent(new Event("change", { bubbles: true }));
  });
  await wait(() => Boolean(fixture.querySelector(".dst-map-roster li button")), `${shard} roster missing`);
}
function checkRailFits(panel: string) {
  const rail = element(".dst-map-sidebar");
  check(rail.scrollWidth <= rail.clientWidth + 1, `${panel} panel must not overflow the rail horizontally`);
  // The compact rail must show every control without scrolling from 1280x800 (and 1366x768) up.
  const main = element(".dst-map-rail-main");
  if (innerWidth >= 1280 && innerHeight >= 768) check(rail.scrollHeight <= rail.clientHeight + 1 && main.scrollHeight <= main.clientHeight + 1,
    `${panel} panel must fit the rail without vertical scrolling or clipping (${rail.scrollHeight} > ${rail.clientHeight}, ${main.scrollHeight} > ${main.clientHeight})`);
  const bounds = rail.getBoundingClientRect();
  for (const node of rail.querySelectorAll<HTMLElement>(".dst-gm-tool, .dst-gm-amount")) {
    const box = node.getBoundingClientRect();
    check(box.left >= bounds.left - 1 && box.right <= bounds.right + 1, `${panel} control ${node.textContent} must stay inside the rail`);
  }
  for (const label of rail.querySelectorAll<HTMLElement>(".dst-gm-tool > span")) check(label.scrollWidth <= label.clientWidth + 1,
    `${panel} tool label ${label.textContent} must be shown in full (${label.scrollWidth} > ${label.clientWidth})`);
}
/** The chip strip's slot within the rail, independent of the rail's scroll position. */
const rosterBox = () => {
  const rail = element(".dst-map-sidebar"), box = element(".dst-map-roster").getBoundingClientRect(), bounds = rail.getBoundingClientRect();
  return `${Math.round(box.left - bounds.left)},${Math.round(box.top - bounds.top + rail.scrollTop)},${Math.round(box.width)}`;
};
/** Zoom floats in the map's top-left corner, never in the rail. */
function checkZoomOverlay() {
  const viewport = element(".dst-map-viewport").getBoundingClientRect(), controls = element(".dst-map-viewport > .dst-map-controls");
  const box = controls.getBoundingClientRect();
  check(getComputedStyle(controls).position === "absolute" && box.left - viewport.left >= 0 && box.left - viewport.left <= 12 && box.top - viewport.top >= 0 && box.top - viewport.top <= 12
    && box.width < viewport.width / 2 && !element(".dst-map-sidebar").querySelector(".dst-map-controls"), "Zoom controls must float in the map's top-left corner");
  check(["放大", "缩小", "适应窗口"].every(label => controls.querySelector(`button[aria-label="${label}"], button[title="${label}"]`)) && Boolean(controls.querySelector("select.dst-map-zoom")),
    "The overlay keeps zoom in/out, the percentage and fit");
}

async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const bootstrap = await invokeMock<BootstrapResponse>("bootstrap", { includeSystemSnapshot: false });
  const instance = bootstrap.state.instances.find(value => value.module_id === "dontstarve")!;
  const original = await invokeMock<InstanceDetails>("read_instance_details_from_storage", { instanceId: instance.id });
  const four: InstanceDetails = { ...original, summary: { ...original.summary, id: "dst-gm", status: "Running", active_process_count: 4 },
    settings_json: JSON.stringify({ shard_layout: "island_adventures" }),
    active_run: { run_id: 2, processes: SHARDS.map(process_key => ({ process_key, status: "Running" })) } };
  await render(four);
  const nav = [...fixture.querySelectorAll(".configuration-section-navigation__button")].map(node => node.textContent?.trim());
  check(nav.length === 2 && nav[0]?.includes("世界工具") && nav[1]?.includes("生成物品"), `DST nav must offer only the two tool panels: ${nav.join(",")}`);
  check(Boolean(fixture.querySelector('[data-configuration-section-id="world"] .is-active')) && Boolean(fixture.querySelector(".dst-map-viewport")),
    "DST tools must open on the world tools beside the always-visible map");
  check(!fixture.querySelector(".gmt-form"), "DST must not fall back to the generic GM form");
  checkDstMapLayout(check, fixture);
  checkRailFits("World tools");
  check(!fixture.querySelector(".dst-map-search, [data-landmark-located]") && !fixture.textContent?.includes("查找地标"), "Landmark search is gone; landmarks are display only");
  checkZoomOverlay();
  checkGrid("World tools");
  check(element(".dst-map-viewport .dst-map-readout").textContent === "—", "The hover readout must always exist in the map corner, showing a dash off-map");
  check(!fixture.textContent?.includes("已过期") && !fixture.textContent?.includes("历史位置") && !fixture.textContent?.includes("未记录玩家位置"), "The rail must not show a stale history-position indicator");
  const worldRoster = rosterBox();
  const regenerate = fixture.querySelector<HTMLButtonElement>('button[aria-label="重新生成世界"]');
  check(regenerate?.classList.contains("dst-gm-tool") && !regenerate.disabled, "Regeneration must be an icon tool button like the season buttons");
  await act(async () => regenerate.click());
  const popover = element(".dst-gm-confirm .inline-confirm-review"), railBounds = element(".dst-map-sidebar").getBoundingClientRect(), popBox = popover.getBoundingClientRect();
  check(getComputedStyle(popover).position === "absolute" && popBox.left >= railBounds.left - 1 && popBox.right <= railBounds.right + 1 && popBox.bottom <= railBounds.bottom + 1,
    "Regeneration must confirm in a popover over the rail");
  checkRailFits("Regeneration confirmation");
  await act(async () => [...popover.querySelectorAll<HTMLButtonElement>("button")].find(button => button.textContent === "取消")!.click());
  check(!fixture.querySelector(".inline-confirm-review"), "Cancelling must close the regeneration popover");

  const season = await dispatched("冬季");
  check(season.processKey === "master" && season.transport === "stdin" && season.command.includes('"ms_setseason", "winter"'), "Season buttons must set the Master season over stdin");
  check(element(".dst-gm-tools + .dst-map-roster, .dst-map-sidebar").textContent?.includes("[LGSM] fixture reply"), "The native response must be shown in the rail");
  const labels = (group: string) => [...element(`.dst-gm-tool-group[aria-label="${group}"]`).querySelectorAll(".dst-gm-tool > span")].map(node => node.textContent);
  check(labels("世界").join() === "白天,黄昏,夜晚,下一天,春季,夏季,秋季,冬季,下雨,停雨,清理,重抽"
    && labels("玩家").join() === "复活,回满,无敌,到玩家,到地点,解锁", `GM tools must be grouped as world and player: ${labels("世界")} / ${labels("玩家")}`);
  check(buttonByText("白天")?.getAttribute("aria-pressed") === "true", "The map's current phase must be shown as pressed");
  const dusk = await dispatched("黄昏");
  check(dusk.processKey === "master" && dusk.transport === "stdin" && dusk.command.includes('"ms_setphase", "dusk"'), "Phase buttons must set the Master clock over stdin");
  const nextDay = await dispatched("下一天");
  check(nextDay.processKey === "master" && nextDay.command.includes('"ms_nextcycle"'), "Skipping a day must advance the Master clock to the next cycle");
  const rain = await dispatched("下雨");
  check(rain.processKey === "master" && rain.command !== season.command, "Precipitation must dispatch its own Master command");
  check(["复活", "回满", "无敌", "到玩家", "到地点", "解锁"].every(label => buttonByText(label)?.disabled === true && buttonByText(label)?.title.includes("先在地图或玩家列表中选择玩家")),
    "Every player tool explains that it needs a selected player");
  const unselectedTop = toolsTop();
  await clickMap(markerCenter(0));
  check(element(".dst-map-roster li button").getAttribute("aria-pressed") === "true", "Clicking a player marker on the map selects that player's chip");
  check(toolsTop() === unselectedTop, "Selecting a player must not move the GM grid");
  checkGrid("Player selected");
  const revive = await dispatched("复活");
  check(revive.processKey === "master" && revive.command.includes('userid=="KU_FIXTURE1"') && revive.command.includes("respawnfromghost"),
    "Revive must target the selected player's Klei user id on the viewed shard");
  for (const [label, call] of [["回满", "health:SetPercent(1)"], ["无敌", "SetInvincible(not h:IsInvincible())"], ["解锁", "GiveAllRecipes()"]]) {
    const sent = await dispatched(label);
    check(sent.processKey === "master" && sent.command.includes('userid=="KU_FIXTURE1"') && sent.command.includes(call), `${label} must send its builder command for the selected player`);
  }
  await click("到玩家");
  check(buttonByText("到玩家")?.getAttribute("aria-pressed") === "true" && Boolean(fixture.querySelector(".dst-map-roster")?.textContent?.includes("点击目标玩家")), "Teleport to player waits for a destination");
  const beforeTeleport = commands.length;
  await act(async () => fixture.querySelectorAll<HTMLButtonElement>(".dst-map-roster li button")[1].click());
  await wait(() => commands.length === beforeTeleport + 1 && buttonByText("冬季")?.disabled === false, "Teleport to player did not dispatch");
  check(commands.at(-1)!.command.includes('c.userid=="KU_FIXTURE2" then q=c') && element(".dst-map-roster li button").getAttribute("aria-pressed") === "true",
    "Picking a destination chip teleports the selected player and keeps the selection");
  await click("到地点");
  const point = markerCenter(1), beforePoint = commands.length;
  await clickMap({ x: point.x + 8, y: point.y });
  await wait(() => commands.length === beforePoint + 1 && buttonByText("冬季")?.disabled === false, "Teleport to point did not dispatch");
  const toPoint = commands.at(-1)!.command.match(/IsPassableAtPoint\((-?[\d.]+),0,(-?[\d.]+)\)/);
  check(toPoint && Number(toPoint[1]) > 24 && Math.abs(Number(toPoint[2]) - 8) < 3 && element(".dst-map-roster li button").getAttribute("aria-pressed") === "true",
    `A map click converts to world coordinates beside the clicked spot: ${toPoint?.[0]}`);
  await act(async () => element<HTMLButtonElement>('button[title^="清理分片 master"]').click());
  const clearReview = element(".dst-gm-confirm .inline-confirm-review");
  check(Boolean(clearReview.textContent?.includes("不可撤销")) && clearReview.contains(element(".inline-confirm-submit")), "Clearing asks for a danger confirmation first");
  const beforeClear = commands.length;
  await act(async () => element<HTMLButtonElement>(".inline-confirm-submit").click());
  await wait(() => commands.length === beforeClear + 1 && buttonByText("冬季")?.disabled === false, "Clear did not dispatch");
  check(commands.at(-1)!.processKey === "master" && commands.at(-1)!.command.includes("dropped items"), "Clearing runs on the viewed shard and reports the count");
  await selectShard("caves");
  check(buttonByText("复活")?.disabled === true, "Changing shard by hand must clear the player target");
  await act(async () => element<HTMLButtonElement>(".dst-map-roster li button").click());
  const cavesRevive = await dispatched("复活");
  check(cavesRevive.processKey === "caves", "Player actions must follow the shard the player is viewed on");

  // The viewed caves shard keeps running while Master (the season target) stops.
  await render({ ...four, active_run: { run_id: 2, processes: SHARDS.map(process_key => ({ process_key, status: process_key === "master" ? "Stopped" : "Running" })) } });
  const beforeStopped = commands.length;
  check(buttonByText("冬季")?.disabled === true && buttonByText("冬季")?.title.includes("主世界未运行") && buttonByText("白天")?.disabled === true,
    "Master-driven buttons must be disabled with their reason while Master is stopped");
  check(buttonByText("复活")?.disabled === false && commands.length === beforeStopped, "Player actions on the running viewed shard stay available");
  await render(four);
  check(element<HTMLSelectElement>(".dst-map-toolbar select").value === "caves", "A settings refresh must keep the viewed shard");
  if (!fixture.querySelector(".dst-map-roster li button[aria-pressed=true]")) await act(async () => element<HTMLButtonElement>(".dst-map-roster li button").click());

  await act(async () => element<HTMLButtonElement>('[data-configuration-section-id="items"] button').click());
  await wait(() => Boolean(fixture.querySelector(".dst-gm-spawn")), "Give items must open the spawn panel");
  check(Boolean(fixture.querySelector('[data-configuration-section-id="items"] .is-active')) && Boolean(fixture.querySelector(".dst-map-viewport")),
    "The spawn panel must keep the map and highlight its nav entry");
  check(element(".dst-map-roster li button").getAttribute("aria-pressed") === "true" && rosterBox().split(",")[0] === worldRoster.split(",")[0]
    && fixture.querySelectorAll(".dst-map-roster").length === 1, `Both panels must reuse one chip strip, keeping the selection (${worldRoster} vs ${rosterBox()})`);
  checkDock("Spawn items");
  checkSpawnLayout("Spawn items");
  const spawnTop = toolsTop();
  const categories = [...fixture.querySelectorAll<HTMLButtonElement>(".gmt-dst-prefab-categories > .dst-gm-tool")];
  const categoryBoxes = categories.map(node => node.getBoundingClientRect());
  check(categories.length === 10 && categories.every(node => node.classList.contains("secondary-button") && node.querySelector("svg") && node.querySelector("span"))
    && categoryBoxes.every((box, index) => Math.abs(box.top - categoryBoxes[index - index % 4].top) <= 1 && Math.abs(box.left - categoryBoxes[index % 4].left) <= 1)
    && Math.abs(categoryBoxes[0].height - 46) <= 1,
  `Item categories must be ten icon+text GM tool buttons in the GM grid's four columns: ${categoryBoxes.map(box => `${Math.round(box.left)},${Math.round(box.top)},${Math.round(box.height)}`).join(" ")}`);
  await act(async () => categories[1].click());
  check(categories[1].getAttribute("aria-pressed") === "true" && element<HTMLInputElement>(".gmt-dst-prefab-search").value === categories[1].textContent, "A category button toggles on and filters");
  const fullRows = await checkPage("Spawn filtered");
  // Shrink the list to one row, page to the end, then let it grow back: the page clamps to the new last page.
  const spawn = element(".dst-gm-spawn");
  spawn.style.flex = "0 0 auto"; element(".gmt-dst-prefab-results").style.height = "26px";
  await waitPageSize(1);
  await checkPage("Spawn shrunk");
  await clickNextUntilLast();
  const [, shrunkPages] = pager(), lastCount = options().length;
  spawn.style.flex = ""; element(".gmt-dst-prefab-results").style.height = "";
  await waitPageSize(fullRows);
  const [page, pages] = pager();
  check((fullRows === 1 || shrunkPages > pages) && page === pages && options().length === (shrunkPages - 1 + lastCount) - fullRows * (pages - 1),
    `After growing back the page clamps to the last of ${pages} (${page}, was ${shrunkPages} pages)`);
  await checkPage("Spawn regrown", shrunkPages - 1 + lastCount);
  await act(async () => categories[1].click());
  check(categories[1].getAttribute("aria-pressed") === "false" && element<HTMLInputElement>(".gmt-dst-prefab-search").value === "", "Clicking the active category clears the filter");
  checkRailFits("Spawn categories");
  check(toolsTop() === spawnTop, "Filtering items must not move the pinned grid");
  check(element<HTMLButtonElement>(".dst-gm-send").title.includes("Wilson"), "The spawn target must be the player selected on the map");
  for (const bad of ["0", "1000", "2.5"]) {
    await setAmount(bad);
    check(element<HTMLButtonElement>(".dst-gm-send").disabled && element<HTMLButtonElement>(".dst-gm-send").title.includes("1–999")
      && element(".dst-gm-amount").classList.contains("is-invalid"), `Amount ${bad} must be rejected with its reason`);
  }
  await setAmount("3");
  const before = commands.length;
  await act(async () => element<HTMLButtonElement>(".dst-gm-send").click());
  await wait(() => commands.length === before + 1 && !element<HTMLButtonElement>(".dst-gm-send").disabled, "Give did not dispatch");
  const give = commands.at(-1)!;
  check(give.processKey === "caves" && give.command.includes('LookupPlayerInstByUserID("KU_FIXTURE1")') && give.command.startsWith('local f="log";') && give.command.includes("SpawnPrefab(f)") && give.command.includes("for i=1,3 do"),
    `Give must reuse the item builder for the selected player on its shard: ${give.processKey} ${give.command}`);
  // A well-formed id outside the catalog is offered first as a custom code; the server checks it exists.
  await setSearch("  My_Mod_Item ");
  check(options()[0]?.textContent?.includes("使用代码") && options()[0].textContent?.includes("my_mod_item"), `A typed id is offered as 使用代码: ${options()[0]?.textContent}`);
  await checkPage("Spawn custom code");
  await act(async () => options()[0].click());
  const customBefore = commands.length;
  await act(async () => element<HTMLButtonElement>(".dst-gm-send").click());
  await wait(() => commands.length === customBefore + 1 && !element<HTMLButtonElement>(".dst-gm-send").disabled, "Custom give did not dispatch");
  check(commands.at(-1)!.command.startsWith('local f="my_mod_item";assert(PrefabExists(f),"Unknown prefab");') && commands.at(-1)!.command.includes("SpawnPrefab(f)"), `The custom code spawns behind the PrefabExists guard: ${commands.at(-1)!.command}`);
  await setSearch('log")');
  check(!options().some(node => node.textContent?.includes("使用代码")), "Text outside the prefab pattern is never offered as a code");
  await setSearch("LOG");
  check(options()[0]?.querySelector("code")?.textContent === "log" && !options().some(node => node.textContent?.includes("使用代码")), "An exact catalog id is simply the top result");
  await act(async () => options()[0].click());
  await setSearch("");

  await act(async () => element<HTMLButtonElement>(".dst-map-roster-clear").click());
  check(element<HTMLButtonElement>(".dst-gm-send").disabled, "Giving without a target or all players must be disabled");
  check(element<HTMLButtonElement>(".dst-gm-send").title.includes("先在地图或玩家列表中选择玩家"), "The disabled give explains the missing target");
  await click("所有人");
  check(buttonByText("所有人")?.title.includes("所有玩家") && buttonByText("所有人")?.getAttribute("aria-pressed") === "true" && toolsTop() === spawnTop, "所有人 (所有玩家) is a toggle that keeps the grid in place");
  pending = true;
  const beforeAll = commands.length;
  await act(async () => element<HTMLButtonElement>(".dst-gm-send").click());
  await wait(() => commands.length === beforeAll + 1 && !element<HTMLButtonElement>(".dst-gm-send").disabled, "All-player give did not dispatch");
  check(commands.at(-1)!.command.includes("t[#t+1]=p") && commands.at(-1)!.command.includes("ipairs(t)")
    && !commands.at(-1)!.command.includes("LookupPlayerInstByUserID"), "All players must target the complete shard snapshot");
  check(element(".dst-map-sidebar").textContent?.includes("仍等待写入确认"), "Pending stdin writes must not be described as delivered");
  pending = false;
  checkDstMapLayout(check, fixture);
  checkRailFits("Spawn items");

  // No map yet, regeneration running, then the regenerated run: the grid never disappears.
  await act(async () => element<HTMLButtonElement>('[data-configuration-section-id="world"] button').click());
  mapAvailable = false;
  await render({ ...four, summary: { ...four.summary, id: "dst-gm-nomap" } }, false);
  check(!fixture.querySelector(".dst-map-viewport canvas") && element(".dst-map-roster").textContent?.includes("等待地图数据"), "Without a map the chip strip stays with a placeholder");
  checkGrid("No map");
  const mounted = [".dst-map-toolbar", ".dst-map-status", ".dst-map-rail-main", ".dst-map-rail-dock", ".dst-map-roster", ".dst-gm-tools"].map(selector => element(selector));
  const steadyTop = toolsTop();
  const stillMounted = (state: string) => check(mounted.every(node => node.isConnected) && toolsTop() === steadyTop, `${state}: every rail section stays mounted and the GM grid does not move`);
  check(buttonByText("冬季")?.disabled === false && buttonByText("复活")?.disabled === true, "Without a map world actions still run; revive needs a player");
  const reroll = element<HTMLButtonElement>('button[aria-label="重新生成世界"]');
  await act(async () => reroll.click());
  await act(async () => element<HTMLButtonElement>(".inline-confirm-submit").click());
  await wait(() => buttonByText("冬季")?.disabled === true, "Regeneration must disable the world actions");
  checkGrid("Regenerating");
  stillMounted("Regenerating");
  check(!element(".dst-map-sidebar").textContent?.includes("正在生成新世界") && element(".dst-map-status").textContent === "—"
    && element(".dst-map-empty").textContent === "正在生成新世界…", "Regeneration stage text shows only over the map; the rail keeps its placeholders");
  check(["白天", "下一天", "冬季", "下雨"].every(label => buttonByText(label)?.title.includes("世界正在重新生成")), "Every world action explains the regeneration");
  await wait(() => regenerationRequested, "Regeneration must reach the backend after flushing settings and the map cache");
  mapRun = 3; mapAvailable = true;
  await act(async () => releaseRegeneration());
  await render({ ...four, summary: { ...four.summary, id: "dst-gm-nomap" }, active_run: { run_id: 3, processes: SHARDS.map(process_key => ({ process_key, status: "Running" })) } });
  check(Boolean(fixture.querySelector(".dst-map-viewport canvas")) && buttonByText("冬季")?.disabled === false, "The new run's map loads and the buttons return without reopening");
  checkGrid("After regeneration");
  stillMounted("After regeneration");
  // End on the spawn tab (the screenshot): it keeps the same pinned layout on the regenerated world.
  await act(async () => element<HTMLButtonElement>('[data-configuration-section-id="items"] button').click());
  await wait(() => Boolean(fixture.querySelector(".dst-gm-spawn")), "The spawn tab must reopen after regeneration");
  await act(async () => fixture.querySelector<HTMLButtonElement>(".gmt-dst-prefab-option")!.click());
  await act(async () => fixture.querySelectorAll<HTMLButtonElement>(".dst-map-roster li button")[0].click());
  checkSpawnLayout("Spawn after regeneration");
  checkDock("Spawn after regeneration");
  checkRailFits("Spawn after regeneration");
  check(document.documentElement.scrollWidth <= innerWidth + 1, "The GM surface must not overflow the viewport");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  const rail = element(".dst-map-sidebar");
  return { status: "passed", checks, browser_errors: errors, commands: commands.length, items: await checkPage("Final spawn page"), rail: { width: rail.clientWidth, height: rail.clientHeight, scrollHeight: rail.scrollHeight } };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`DST GM tools stalled at ${stage} after ${checks} checks`)), 30000);
})]).finally(() => clearTimeout(watchdog)).catch(error => ({ status: "failed", checks, error: error.stack ?? String(error), browser_errors: errors }))
  .finally(() => Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }))
  .then(report => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
