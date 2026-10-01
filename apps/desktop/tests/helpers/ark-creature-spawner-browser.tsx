import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider } from "../../src/i18n";
import type { ArkSpawnInput, ArkSpawnResult, ArkToolsStatus } from "../../src/api-ark-tools";
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
let props = { instanceId: "ark-a", moduleId: "arksurvivalascended", status: "Stopped",
  settingsJson: JSON.stringify({ rcon_enabled: true, admin_password: "fixture-admin-password", mod_ids_csv: "123456" }) };
const spawnCalls: ArkSpawnInput[] = [];
const installs: Array<{ instanceId: string; allowMatchingSymbolsDownload: boolean }> = [];
let clipboardCalls = 0;

function result(input: ArkSpawnInput): ArkSpawnResult {
  return { instanceId: input.instanceId, requestId: input.requestId, creature: {
    id1: 123, id2: 456, className: "Rex_Character_BP_C", level: input.level, team: input.tamed ? 1000 : 1,
    x: input.x, y: input.y, z: input.z, tamed: input.tamed
  } };
}
// Only the native IPC boundary is controlled; the production form, API input,
// result validation and operation lifetime run unchanged in a real browser.
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: (command: string, args: { input: ArkSpawnInput & { allowMatchingSymbolsDownload: boolean } }) => {
  if (command === "read_ark_tools_status") return holdStatus
    ? new Promise<ArkToolsStatus>(resolve => statusReplies.push(resolve))
    : Promise.resolve({ installed, connected, issue: null });
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
  const button = Array.from(fixture.querySelectorAll<HTMLButtonElement>("button")).find(node => node.textContent?.trim() === name);
  if (!button) throw new Error(`Missing button ${name}; rendered: ${fixture.textContent}`); return button;
}
async function click(node: HTMLElement) { await act(async () => { node.click(); }); }
async function render(next: Partial<typeof props> = {}) {
  props = { ...props, ...next };
  await act(async () => root.render(<StrictMode><I18nProvider><main style={{ padding: 24, maxWidth: 1000, margin: "0 auto" }}>
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
const submit = () => element<HTMLButtonElement>('button[type="submit"]');
async function coordinates() {
  const fields = fixture.querySelectorAll<HTMLInputElement>(".ark-creature-spawner__coordinates input");
  for (const [index, value] of ["100", "200", "300"].entries()) await input(fields[index], value);
}
async function run() {
  await render();
  check(!namedButton("安装服务端扩展").disabled, "Stopped runtime must permit installation");
  await click(namedButton("安装服务端扩展"));
  check(installs.length === 1 && installs[0].instanceId === "ark-a" && installs[0].allowMatchingSymbolsDownload,
    "Installation must pass the reviewed native request");
  check(submit().disabled, "A stopped instance must not allow spawning");
  connected = true;
  await render({ status: "Running" });
  check(!submit().disabled && fixture.textContent?.includes("服务端扩展已连接"), "Running extension connection must enable spawning");
  await click(submit());
  check(spawnCalls.length === 0 && !element<HTMLFormElement>("form").checkValidity(), "Empty world coordinates must never dispatch");
  await input(element<HTMLInputElement>('input[type="search"]'), "rex");
  check(Array.from(fixture.querySelectorAll(".ark-creature-spawner__choices button")).some(node => node.textContent?.includes("霸王龙")),
    "Real creature search must show translated names");
  await input(element<HTMLInputElement>(".ark-creature-spawner__saved input"), "回归测试霸王龙");
  await click(namedButton("收藏生物"));
  check(JSON.parse(localStorage.getItem("lsgm.gmTools.arkCreatures.ark-a") ?? "[]")[0]?.label === "回归测试霸王龙",
    "Saved creature entries must preserve the existing per-instance storage contract");
  await coordinates();
  await click(submit());
  check(spawnCalls.length === 1 && spawnCalls[0].x === 100 && spawnCalls[0].y === 200 && spawnCalls[0].z === 300
    && spawnCalls[0].creature.endsWith("/Rex_Character_BP.Rex_Character_BP_C") && !spawnCalls[0].tamed,
    "One-click wild spawning must send typed native parameters with world coordinates");
  check(/^[a-f0-9]{32}$/.test(spawnCalls[0].requestId), "Spawn requests must have a unique correlation ID");
  check(element(".ark-creature-spawner__result").textContent?.includes("ID 123:456"), "Native entity identity must be visible");
  check(clipboardCalls === 0, "One-click spawning must never copy an in-game command");
  await input(element<HTMLSelectElement>(".ark-creature-spawner__coordinates select"), "true");
  await click(submit());
  check(spawnCalls.length === 1, "Tamed creatures must require an online player ID");
  await input(element<HTMLInputElement>('input[max="4294967295"]'), "12345");
  await click(submit());
  check(spawnCalls.length === 2 && spawnCalls[1].tamed && spawnCalls[1].playerId === 12345,
    "Tamed creatures must send the selected player target");
  mode = "invalid";
  await click(submit());
  check(!fixture.querySelector(".ark-creature-spawner__result") && element("[role=alert]").textContent?.includes("Invalid ARK creature read-back"),
    "Mismatched native read-back must never display a verified creature");
  mode = "failure";
  await click(submit());
  check(element("[role=alert]").textContent?.includes("Native creature class could not be loaded"), "Native failure must remain visible without retry");
  mode = "deferred";
  await click(submit());
  const pending = spawnCalls[spawnCalls.length - 1];
  check(submit().disabled, "Pending spawn must prevent another click");
  await render({ settingsJson: JSON.stringify({ ...JSON.parse(props.settingsJson), ui_refresh: true }) });
  check(submit().disabled && spawnCalls.length === 5, "Settings refresh must not release a native spawn lock");
  await render({ instanceId: "ark-b" });
  check(submit().disabled && !fixture.querySelector(".ark-creature-spawner__result"), "Instance switch must keep the in-flight lock and clear prior results");
  await act(async () => resolveSpawn!(result(pending)));
  check(!submit().disabled && !fixture.querySelector(".ark-creature-spawner__result"), "Late results must not pollute the selected instance");
  holdStatus = true;
  await render({ settingsJson: JSON.stringify({ ...JSON.parse(props.settingsJson), status_revision: 1 }) });
  await render({ settingsJson: JSON.stringify({ ...JSON.parse(props.settingsJson), status_revision: 2 }) });
  check(statusReplies.length === 2, "Settings changes must recheck the extension connection");
  await act(async () => statusReplies[1]({ installed: true, connected: false, issue: "new disconnected state" }));
  await act(async () => statusReplies[0]({ installed: true, connected: true, issue: null }));
  check(submit().disabled && fixture.textContent?.includes("new disconnected state") && !fixture.textContent?.includes("服务端扩展已连接"),
    "An older handshake must not overwrite the current connection state");
  holdStatus = false;
  await render({ settingsJson: JSON.stringify({ ...JSON.parse(props.settingsJson), status_revision: 3 }) });
  mode = "response";
  await input(element<HTMLSelectElement>(".ark-creature-spawner__coordinates select"), "false");
  await coordinates();
  await input(element<HTMLInputElement>('input[type="search"]'), "rex");
  await click(submit());
  const feedback = element(".ark-creature-spawner__result");
  feedback.scrollIntoView({ block: "nearest" });
  check(feedback.textContent?.includes("已生成并读回确认") && feedback.textContent?.includes("ID 123:456"), "Verified result must remain inspectable");
  check(document.documentElement.scrollWidth <= innerWidth + 1, "ARK spawning must not overflow horizontally");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, browser_errors: errors, spawn_calls: spawnCalls.length, clipboard_calls: clipboardCalls,
    viewport: { width: innerWidth, height: innerHeight } };
}
void run().catch(error => ({ status: "failed", checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .finally(() => Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }))
  .then(report => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
