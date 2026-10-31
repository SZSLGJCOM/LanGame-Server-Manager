import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider, useI18n, type LocaleCode } from "../../src/i18n";
import type { ArkSpawnInput, ArkSpawnResult, ArkToolsStatus } from "../../src/api-ark-tools";
import type { ArkMapPlayer } from "../../src/ark-world-map";
import { ArkCreatureSpawner } from "../../src/views/servers/ArkCreatureSpawner";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const errors: string[] = [];
addEventListener("error", event => errors.push(event.message));
addEventListener("unhandledrejection", event => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
let checks = 0;
let installed = false;
let connected = false;
let holdStatus = false;
const statusReplies: Array<(value: ArkToolsStatus) => void> = [];
let mode: "response" | "invalid" | "failure" | "deferred" = "response";
let resolveSpawn: ((result: ArkSpawnResult) => void) | undefined;
const player: ArkMapPlayer = { playerId: 12345, name: "测试玩家", accountId: "fixture-account", team: 1000, x: 100, y: 200, z: 300 };
let props: React.ComponentProps<typeof ArkCreatureSpawner> = { instanceId: "ark-a", moduleId: "arksurvivalascended", status: "Stopped", processKey: "main", runId: 7,
  selectedPlayer: null,
  settingsJson: JSON.stringify({ rcon_enabled: true, admin_password: "fixture-admin-password", mod_ids_csv: "123456" }) };
const spawnCalls: ArkSpawnInput[] = [];
const installs: Array<{ instanceId: string; allowMatchingSymbolsDownload: boolean }> = [];
let clipboardCalls = 0;
let changeLocale: ((locale: LocaleCode) => void) | undefined;
function LocaleControl() { changeLocale = useI18n().setLocale; return null; }

function result(input: ArkSpawnInput): ArkSpawnResult {
  return { instanceId: input.instanceId, processKey: input.processKey, runId: input.runId, requestId: input.requestId, creature: {
    id1: 123, id2: 456, className: "Rex_Character_BP_C", level: input.level, team: input.tamed ? 1000 : 1,
    x: input.x, y: input.y, z: input.z, tamed: input.tamed
  } };
}
// Only the native IPC boundary is controlled; the production form, API input,
// result validation and operation lifetime run unchanged in a real browser.
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: (command: string, args: { input: ArkSpawnInput & { allowMatchingSymbolsDownload: boolean } }) => {
  if (command === "read_ark_tools_status") return holdStatus
    ? new Promise<ArkToolsStatus>(resolve => statusReplies.push(resolve))
    : Promise.resolve({ installed, connected, issue: installed ? null : "Prepare the ARK server extension while this instance is stopped" });
  if (command === "prepare_ark_tools") { installs.push(args.input); installed = true; return Promise.resolve({ installed, connected, issue: null }); }
  if (command !== "spawn_ark_creature") throw new Error(`Unexpected IPC ${command}`);
  spawnCalls.push(args.input);
  if (mode === "deferred") return new Promise<ArkSpawnResult>(resolve => { resolveSpawn = resolve; });
  if (mode === "failure") return Promise.reject(new Error("Native creature class could not be loaded"));
  return Promise.resolve(mode === "invalid" ? { ...result(args.input), requestId: "wrong-request" } : result(args.input));
} } });
Object.defineProperty(navigator, "clipboard", { configurable: true, value: {
  writeText: async () => { clipboardCalls++; throw new Error("Spawning must not use the clipboard"); }
} });

function check(value: unknown, message: string): asserts value { if (!value) throw new Error(message); checks++; }
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const node = fixture.querySelector<T>(selector); if (!node) throw new Error(`Missing ${selector}`); return node;
}
function namedButton(name: string) {
  const button = Array.from(fixture.querySelectorAll<HTMLButtonElement>("button")).find(node => (node.getAttribute('aria-label') || node.textContent?.trim()) === name);
  if (!button) throw new Error(`Missing button ${name}; rendered: ${fixture.textContent}`); return button;
}
async function click(node: HTMLElement) { await act(async () => { node.click(); }); }
async function render(next: Partial<typeof props> = {}) {
  props = { ...props, ...next };
  await act(async () => root.render(<StrictMode><I18nProvider><LocaleControl /><main style={{ padding: 24, maxWidth: 1000, margin: "0 auto", height: '100vh', boxSizing: 'border-box', display: 'flex' }}>
    <ArkCreatureSpawner {...props} /></main></I18nProvider></StrictMode>));
  const deadline = performance.now() + 5000;
  while (!fixture.querySelector(".ark-creature-spawner")) {
    if (performance.now() >= deadline) throw new Error("ARK tool did not finish loading");
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 10)); });
  }
}
async function input(node: HTMLInputElement | HTMLSelectElement, value: string) {
  const proto = node instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
  await act(async () => {
    Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(node, value);
    node.dispatchEvent(new Event(node instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  });
}
async function locale(value: LocaleCode) {
  await act(async () => changeLocale!(value));
  const label = value === 'en-US' ? 'Spawn creature' : '生成生物';
  const deadline = performance.now() + 5000;
  while (!fixture.querySelector(`button[type="submit"][aria-label="${label}"]`)) {
    if (performance.now() >= deadline) throw new Error(`Spawner did not finish switching to ${value}`);
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 15)); });
  }
}
const submit = () => element<HTMLButtonElement>('button[type="submit"]');
function actionHelp(button: HTMLElement): string {
  const anchor = button.closest('.configuration-action-help');
  return document.getElementById(anchor?.getAttribute('aria-describedby') ?? '')?.textContent ?? '';
}
const resultSelector = ".ark-creature-spawner > .shell-activity-notice.is-success";
async function coordinates() {
  const fields = fixture.querySelectorAll<HTMLInputElement>(".ark-creature-spawner__coordinates input");
  for (const [index, value] of ["100", "200", "300"].entries()) await input(fields[index], value);
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  await render();
  check(!namedButton("安装服务端扩展").disabled, "Stopped runtime must permit installation");
  check(!fixture.querySelector('.ark-creature-spawner__status, .ark-creature-spawner__note')
    && !fixture.textContent?.includes('Prepare the ARK server extension'), 'Routine extension state must not occupy a banner or expose the raw English setup message');
  check(submit().disabled && actionHelp(submit()).includes('安装服务端扩展'), 'The unavailable spawn action explains its missing extension prerequisite');
  check(!fixture.querySelector('.ark-creature-spawner__coordinates, .ark-map-use-player'),
    'Default player spawning requires neither coordinate fields nor a separate use-player click');
  check(!element<HTMLDetailsElement>('.ark-spawn-options').open && element('.ark-spawn-options summary').textContent?.includes('自定义生物'),
    'Optional manual position and custom creature controls start collapsed');
  await locale('en-US');
  check(actionHelp(submit()).includes('Install the server extension'), 'The missing extension reason follows the selected UI language');
  await locale('zh-CN');
  const installAnchor = namedButton('安装服务端扩展').closest<HTMLElement>('.configuration-action-help')!;
  await act(async () => installAnchor.focus());
  const tooltipUntil = performance.now() + 3000;
  while (!document.querySelector('.configuration-field-help-tooltip.is-visible')) {
    if (performance.now() > tooltipUntil) throw new Error('Install help did not open on keyboard focus');
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 15)); });
  }
  check(document.querySelector('.configuration-field-help-tooltip.is-visible')?.textContent?.includes('停服时安装')
    && document.querySelector('.configuration-field-help-tooltip.is-visible')?.textContent?.includes('cdn.pelayori.com'),
    'The install button bubble retains the stopped prerequisite and ASA download disclosure');
  await act(async () => installAnchor.blur());
  await render({ status: 'Running' });
  check(namedButton('安装服务端扩展').disabled, 'The compact install action still rejects a running instance');
  check(actionHelp(submit()).includes('停止实例') && actionHelp(submit()).includes('安装服务端扩展'), 'Spawn explains that installing a missing extension requires stopping first');
  const runningAnchor = namedButton('安装服务端扩展').closest<HTMLElement>('.configuration-action-help')!;
  const reason = document.getElementById(runningAnchor.getAttribute('aria-describedby')!);
  check(reason?.textContent?.includes('请先停止实例'), 'The disabled action explains how to enable installation');
  await render({ status: 'Stopped' });
  await click(namedButton("安装服务端扩展"));
  check(installs.length === 1 && installs[0].instanceId === "ark-a" && installs[0].allowMatchingSymbolsDownload,
    "Installation must pass the reviewed native request");
  check(submit().disabled, "A stopped instance must not allow spawning");
  check(actionHelp(submit()).includes('启动当前地图'), 'An installed extension explains that the map must be started');
  connected = true;
  await render({ status: "Running" });
  check(submit().disabled && actionHelp(submit()).includes('选择') && actionHelp(namedButton('检查连接')).includes('服务端扩展已连接'),
    'A connected extension still requires a selected player in default mode');
  await click(submit());
  check(spawnCalls.length === 0, 'No player must never silently spawn at zero coordinates');
  await render({ selectedPlayer: player });
  check(!submit().disabled, 'Selecting a live player immediately enables spawning');
  await input(element<HTMLInputElement>('input[type="search"]'), "rex");
  check(Array.from(fixture.querySelectorAll(".gmt-ark-catalog-option")).some(node => node.textContent?.includes("霸王龙")),
    "Real creature search must show translated names");
  await click(element('.ark-spawn-options summary'));
  await input(element<HTMLInputElement>(".ark-creature-spawner__saved input"), "回归测试霸王龙");
  await click(namedButton("收藏生物"));
  check(JSON.parse(localStorage.getItem("lsgm.gmTools.arkCreatures.ark-a") ?? "[]")[0]?.label === "回归测试霸王龙",
    "Saved creature entries must preserve the existing per-instance storage contract");
  await click(element('.ark-spawn-options summary'));
  await click(submit());
  check(spawnCalls.length === 1 && spawnCalls[0].x === 100 && spawnCalls[0].y === 200 && spawnCalls[0].z === 300
    && spawnCalls[0].creature.endsWith("/Rex_Character_BP.Rex_Character_BP_C") && !spawnCalls[0].tamed,
    "Default wild spawning uses the selected player's position without editing coordinates");
  check(/^[a-f0-9]{32}$/.test(spawnCalls[0].requestId), "Spawn requests must have a unique correlation ID");
  check(element(resultSelector).textContent?.includes("ID 123:456"), "Native entity identity must be visible");
  check(clipboardCalls === 0, "One-click spawning must never copy an in-game command");
  await render({ selectedPlayer: { ...player, x: 150, y: 250, z: 350 } });
  await input(element<HTMLSelectElement>(".ark-creature-spawner__parameters select"), "true");
  check(!fixture.querySelector('.ark-creature-spawner__coordinates, input[max="4294967295"]'),
    'Default tamed spawning requires no manual coordinates or owner fields');
  await click(submit());
  check(spawnCalls.length === 2 && spawnCalls[1].tamed && spawnCalls[1].playerId === 12345
    && spawnCalls[1].x === 150 && spawnCalls[1].y === 250 && spawnCalls[1].z === 350,
    'Tamed spawning automatically uses the selected owner and their latest polled position');
  await render({ selectedPlayer: null });
  await click(submit());
  check(submit().disabled && spawnCalls.length === 2, 'Clearing or expiring the selected player prevents reuse of their old position');
  await render({ selectedPlayer: { ...player, y: null } });
  check(submit().disabled && actionHelp(submit()).includes('位置'), 'A player without complete character coordinates cannot be a spawn target');
  await render({ selectedPlayer: { ...player, playerId: 6789, name: '玩家乙', x: 400, y: 500, z: 600 } });
  mode = "invalid";
  await click(submit());
  check(spawnCalls[2].playerId === 6789 && spawnCalls[2].x === 400 && spawnCalls[2].y === 500 && spawnCalls[2].z === 600,
    'Changing the selected player immediately changes both spawn location and owner');
  check(!fixture.querySelector(resultSelector) && element("[role=alert]").textContent?.includes("Invalid ARK creature read-back"),
    "Mismatched native read-back must never display a verified creature");
  await click(element('.ark-spawn-options summary'));
  await input(element<HTMLSelectElement>('[aria-label="生成位置来源"]'), 'manual');
  await act(async () => { element<HTMLDetailsElement>('.ark-spawn-options').open = false; });
  await click(submit());
  check(spawnCalls.length === 3 && element<HTMLDetailsElement>('.ark-spawn-options').open,
    'Incomplete manual coordinates block dispatch and reveal the optional fields');
  await coordinates();
  await click(submit());
  check(spawnCalls.length === 3, 'Manual tamed spawning still requires an owner ID');
  await input(element<HTMLInputElement>('input[max="4294967295"]'), '777');
  await render({ selectedPlayer: { ...player, x: 800, y: 900, z: 1000 } });
  await act(async () => { element<HTMLDetailsElement>('.ark-spawn-options').open = false; });
  check(element('.ark-spawn-options summary').textContent?.includes('已启用'), 'The collapsed summary identifies an active manual location override');
  mode = "failure";
  await click(submit());
  check(spawnCalls[3].x === 100 && spawnCalls[3].y === 200 && spawnCalls[3].z === 300 && spawnCalls[3].playerId === 777,
    'Explicit manual coordinates and owner are not overwritten by subsequent player polling');
  check(element("[role=alert]").textContent?.includes("Native creature class could not be loaded"), "Native failure must remain visible without retry");
  mode = "deferred";
  await click(submit());
  const pending = spawnCalls[spawnCalls.length - 1];
  check(submit().disabled, "Pending spawn must prevent another click");
  await render({ settingsJson: JSON.stringify({ ...JSON.parse(props.settingsJson), ui_refresh: true }) });
  check(submit().disabled && spawnCalls.length === 5, "Settings refresh must not release a native spawn lock");
  await render({ instanceId: "ark-b" });
  check(submit().disabled && !fixture.querySelector(resultSelector), "Instance switch must keep the in-flight lock and clear prior results");
  await act(async () => resolveSpawn!(result(pending)));
  check(!submit().disabled && !fixture.querySelector(resultSelector) && !fixture.querySelector('.ark-creature-spawner__coordinates'),
    'Late results must not pollute the selected instance, which resets to default player targeting');
  holdStatus = true;
  await render({ settingsJson: JSON.stringify({ ...JSON.parse(props.settingsJson), status_revision: 1 }) });
  await render({ settingsJson: JSON.stringify({ ...JSON.parse(props.settingsJson), status_revision: 2 }) });
  check(statusReplies.length === 2, "Settings changes must recheck the extension connection");
  await act(async () => statusReplies[1]({ installed: true, connected: false, issue: "new disconnected state" }));
  await act(async () => statusReplies[0]({ installed: true, connected: true, issue: null }));
  check(submit().disabled && actionHelp(submit()).includes("new disconnected state") && !fixture.textContent?.includes("服务端扩展已连接"),
    "An older handshake must not overwrite the current connection state");
  holdStatus = false;
  await render({ settingsJson: JSON.stringify({ ...JSON.parse(props.settingsJson), status_revision: 3 }) });
  mode = "response";
  await input(element<HTMLSelectElement>(".ark-creature-spawner__parameters select"), "false");
  await input(element<HTMLInputElement>('input[type="search"]'), "rex");
  await click(submit());
  check(spawnCalls[5].x === 800 && spawnCalls[5].y === 900 && spawnCalls[5].z === 1000,
    'The new instance uses the current player rather than the previous manual override');
  const feedback = element(resultSelector);
  await act(async () => { element<HTMLDetailsElement>('.ark-spawn-options').open = false; element<HTMLInputElement>('.gmt-ark-catalog-search').focus(); });
  const catalogBox = element('.gmt-ark-catalog-results').getBoundingClientRect();
  const actionBox = submit().getBoundingClientRect();
  check(catalogBox.height >= 52 && catalogBox.bottom <= actionBox.top && actionBox.bottom <= innerHeight,
    `Creature catalog and bottom actions fit independently: ${JSON.stringify({ catalog: catalogBox.toJSON(), action: actionBox.toJSON(), form: element('form').getBoundingClientRect().toJSON(), fieldset: element('fieldset').getBoundingClientRect().toJSON() })}`);
  check(feedback.textContent?.includes("已生成并读回确认") && feedback.textContent?.includes("ID 123:456"), "Verified result must remain inspectable");
  check(document.documentElement.scrollWidth <= innerWidth + 1, "ARK spawning must not overflow horizontally");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, browser_errors: errors, spawn_calls: spawnCalls.length, clipboard_calls: clipboardCalls,
    viewport: { width: innerWidth, height: innerHeight } };
}
void run().catch(error => ({ status: "failed", checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .finally(() => Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }))
  .then(report => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
