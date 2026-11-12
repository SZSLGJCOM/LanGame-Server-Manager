import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { invokeMock } from "../../src/api-mock";
import { I18nProvider } from "../../src/i18n";
import type { BootstrapResponse, InstanceDetails, InstanceRuntimeCommandResult } from "../../src/types";
import { GMToolsWorkbench } from "../../src/views/servers/GMToolsWorkbench";
import { ArkToolsInstallationProvider } from "../../src/components/ArkToolsInstallationContext";
import { getGmToolCatalog } from "../../src/views/servers/gm-tools";
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
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
let checks = 0;
type Request = { instanceId: string; command: string; processKey?: string | null; runId?: number; transport?: string };
const calls: Request[] = [];
const deferred: Array<{ request: Request; resolve: (result: InstanceRuntimeCommandResult) => void }> = [];
let mode: "deferred" | "pending" | "partial" | "response" = "response";
let partialCount = 0;
let details: InstanceDetails;

function response(request: Request, text = "Unknown command: native diagnostic", pending = false): InstanceRuntimeCommandResult {
  return { instance_id: request.instanceId, command: request.command, process_key: request.processKey ?? "rcon",
    display_name: "Test native boundary", pid: 1, response_text: text,
    write_confirmation_pending: pending, submitted_at_unix_ms: Date.now() };
}
// Only the native IPC boundary is controlled. The real
// forms, builders, API envelope and async execution lifecycle run unchanged.
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: (command: string, args: { input?: Request }) => {
  if (command === "read_ark_tools_install_progress") return Promise.resolve(null);
  if (command === "read_ark_tools_status") return Promise.resolve({ installed: true, updateAvailable: false, connected: true, canUninstall: true, issue: null });
  if (command === "read_ark_map") return Promise.resolve({ ...args.input,
    mapName: args.input?.processKey === "map-desert" ? "ScorchedEarth_P" : "TheIsland",
    sampledAtUnixMs: Date.now(), projection: null, players: [], artwork: null });
  if (!["send_instance_gm_command", "send_ark_map_gm_command"].includes(command) || !args.input) throw new Error(`Unexpected IPC: ${command}`);
  const request = args.input;
  calls.push(request);
  if (mode === "deferred") return new Promise<InstanceRuntimeCommandResult>((resolve) => deferred.push({ request, resolve }));
  if (mode === "partial" && ++partialCount === 2) return Promise.reject(new Error("native connection lost"));
  return Promise.resolve(response(request, mode === "partial" ? "First reward accepted" : undefined, mode === "pending"));
} } });

function check(value: unknown, message: string): asserts value { if (!value) throw new Error(message); checks++; }
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const node = fixture.querySelector<T>(selector);
  if (!node) throw new Error(`Missing ${selector}`);
  return node;
}
async function settle(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(message);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function render(next: InstanceDetails) {
  details = next;
  await act(async () => root.render(<StrictMode><I18nProvider><ArkToolsInstallationProvider><main style={{ padding: 24, height: "100vh", boxSizing: "border-box" }}>
    <GMToolsWorkbench details={details} /></main></ArkToolsInstallationProvider></I18nProvider></StrictMode>));
  await settle(() => Boolean(fixture.querySelector(".gmt-workbench")), "Tool workbench did not render");
}
const button = () => element<HTMLButtonElement>('.ark-world-tools button[type="submit"]');
const itemButton = () => element<HTMLButtonElement>('.gmt-form button[type="submit"]');
const resultSelector = ".ark-world-tools-feedback .shell-activity-notice";
const itemResultSelector = ".gmt-form .shell-activity-notice";
const clearButton = () => element<HTMLButtonElement>(".ark-world-tools .inline-confirm-action > button");
const clearReviewSelector = ".ark-world-tools .inline-confirm-review";
async function submit(target = button()) { await act(async () => { target.click(); }); }
async function choose(toolId: string) {
  const index = getGmToolCatalog(details.summary.module_id)!.tools.findIndex((tool) => tool.id === toolId);
  check(index >= 0, `Tool ${toolId} must exist`);
  const toggle = element<HTMLButtonElement>(".configuration-workspace__navigation-toggle");
  if (toggle.getClientRects().length && toggle.getAttribute("aria-expanded") === "false") {
    await act(async () => toggle.click());
  }
  const target = element<HTMLButtonElement>(`[data-configuration-section-id="${toolId}"] button`);
  check(target.getClientRects().length > 0, `Tool ${toolId} navigation must be visible before selection`);
  await act(async () => target.click());
}
async function input(selector: string, value: string) {
  const field = element<HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement>(selector);
  const prototype = field instanceof HTMLSelectElement ? HTMLSelectElement.prototype
    : field instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  await act(async () => {
    Object.getOwnPropertyDescriptor(prototype, "value")!.set!.call(field, value);
    field.dispatchEvent(new Event(field instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  });
}
async function moduleDetails(moduleId: string, id: string, running = true) {
  await render({ ...details, summary: { ...details.summary, id, module_id: moduleId,
    status: running ? "Running" : "Stopped", active_process_count: running ? 1 : 0 },
    active_run: running ? { run_id: 1, processes: [{ process_key: "main", run_id: 1, status: "Running" },
      { process_key: "master", status: "Running" }, { process_key: "caves", status: "Stopped" }] } : null });
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const bootstrap = await invokeMock<BootstrapResponse>("bootstrap", { includeSystemSnapshot: false });
  const instance = bootstrap.state.instances.find((candidate) => candidate.module_id === "minecraft")!;
  details = await invokeMock<InstanceDetails>("read_instance_details_from_storage", { instanceId: instance.id, instance_id: instance.id });
  details = { ...details, settings_json: JSON.stringify({ ...JSON.parse(details.settings_json),
    rcon_enabled: true, admin_password: "fixture-admin-password" }), ports: [
    { name: "rcon", protocol: "tcp", port: 25575 }, { name: "rest_api", protocol: "tcp", port: 8212 }
  ] };
  await moduleDetails("arksurvivalascended", "ark-a");
  check(!fixture.querySelector('[data-configuration-section-id="ark_set_time"], [data-configuration-section-id="ark_destroy_wild_dinos"]'),
    "World actions must remain beside the spawn panels instead of becoming navigation destinations");
  await choose("ark_spawn_creature");
  const spawner = element(".ark-creature-spawner");
  await choose("ark_give_item_to_player");
  await choose("ark_spawn_creature");
  check(element(".ark-creature-spawner") === spawner, "Switching tools must preserve the mounted spawning operation and its in-flight lock");
  const configured = details;
  await render({ ...details, settings_json: JSON.stringify({ ...JSON.parse(details.settings_json), rcon_enabled: false }) });
  const beforeDisabled = calls.length;
  await submit();
  check(button().disabled && fixture.textContent?.includes("启用 RCON") && calls.length === beforeDisabled,
    "Disabled RCON must explain configuration and block dispatch");
  await render({ ...configured, settings_json: JSON.stringify({ ...JSON.parse(configured.settings_json), admin_password: "" }) });
  check(button().disabled && fixture.textContent?.includes("配置 RCON 管理密码"), "Missing RCON password must have a useful explanation");
  await render({ ...configured, ports: [] });
  check(button().disabled && fixture.textContent?.includes("RCON TCP 端口"), "Missing RCON port must have a useful explanation");
  await render(configured);
  await submit();
  check(calls.length === 1 && calls[0].command === "SetTimeOfDay 12:00" && calls[0].processKey === "main" && calls[0].runId === 1,
    "Real ARK time form must dispatch its builder command through the map/run-bound API envelope");
  check(Boolean(fixture.querySelector('[data-configuration-section-id="ark_spawn_creature"] [aria-current="page"]'))
    && !element(".ark-creature-tool").hidden,
    "Changing time must leave the current creature spawning panel selected and visible");
  check(element(resultSelector).textContent?.includes("Unknown command: native diagnostic")
    && element(resultSelector).textContent?.includes(calls[0].command),
    "Native game rejection text must retain its command identity");
  check(!fixture.querySelector(".shell-activity-notice.is-success"), "A transport response must not become a game success toast");

  mode = "deferred";
  await submit();
  check(button().disabled && deferred.length === 1, "In-flight command must lock the form");
  await render({ ...details, settings_json: JSON.stringify({ ...JSON.parse(details.settings_json), ui_refresh: true }) });
  await submit();
  check(calls.length === 2 && button().disabled, "Settings refresh must not release the in-flight lock");
  await moduleDetails("arksurvivalascended", "ark-b");
  check(button().disabled && !fixture.querySelector(resultSelector), "Other instance must not inherit the old response");
  const previous = deferred.shift()!;
  await act(async () => previous.resolve(response(previous.request, "OLD INSTANCE RESPONSE")));
  check(!button().disabled && !fixture.textContent?.includes("OLD INSTANCE RESPONSE"), "Late native result must not pollute the selected instance");


  await moduleDetails("arksurvivalascended", "ark-invalid");
  await input("#gm-ark_set_time-time-input", "99:99");
  const beforeInvalid = calls.length;
  await submit();
  check(button().disabled && calls.length === beforeInvalid && button().closest(".configuration-action-help")?.hasAttribute("aria-describedby"),
    "Invalid time must explain its error before native dispatch");

  await moduleDetails("arksurvivalascended", "ark-stopped", false);
  check(button().disabled && button().closest(".configuration-action-help")?.hasAttribute("aria-describedby"), "Stopped server must show why its tool is unavailable");

  await moduleDetails("arksurvivalascended", "ark-b");
  await choose("ark_give_item_to_player");
  await act(async () => element<HTMLElement>('.gmt-form .ark-spawn-options summary').click());
  await input('[aria-label="发放目标来源"]', "manual");
  await input("#gm-ark_give_item_to_player-playerId-input", "12345");
  await input("#gm-ark_give_item_to_player-itemMode-input", "batch");
  await input("#gm-ark_give_item_to_player-lines-input", "9,2,1,0\n76,1,1,0\n8,1,1,0");
  mode = "partial";
  const beforeBatch = calls.length;
  await submit(itemButton());
  check(calls.length === beforeBatch + 2, "Batch must stop at the first unconfirmed command");
  check(element(itemResultSelector).textContent?.includes("First reward accepted")
    && element(itemResultSelector).textContent?.includes(calls[beforeBatch].command)
    && element(`${itemResultSelector}[role=alert]`).textContent?.includes("已发送 1/3 条"), "Partial batch must retain delivered responses and the failure count");
  check(element(`${itemResultSelector}[role=alert]`).textContent?.includes("native connection lost"), "Original transport error must remain visible");

  mode = "deferred";
  await submit(itemButton());
  check(button().disabled && clearButton().disabled, "An in-flight item batch must lock the persistent world actions too");
  const beforeSwitch = calls.length;
  await moduleDetails("arksurvivalascended", "ark-c");
  const batch = deferred.shift()!;
  await act(async () => batch.resolve(response(batch.request, "old batch reply")));
  check(calls.length === beforeSwitch, "Switching instances must stop unsent batch mutations");

  mode = "response";
  await choose("ark_give_item_to_player");
  const beforeDestructive = calls.length;
  await act(async () => clearButton().click());
  check(calls.length === beforeDestructive && Boolean(fixture.querySelector(clearReviewSelector)),
    "Destructive GM commands must wait for explicit confirmation");
  await moduleDetails("arksurvivalascended", "ark-confirm");
  check(!fixture.querySelector(clearReviewSelector) && calls.length === beforeDestructive,
    "Changing instances must discard an unsubmitted wild-creature confirmation");
  await render({ ...details, settings_json: JSON.stringify({ ...JSON.parse(details.settings_json),
    additional_maps: [{ id: "desert", map_name: "ScorchedEarth_P", name: "焦土", enabled: true }] }),
    active_run: { run_id: 1, processes: [{ process_key: "main", run_id: 1, status: "Running" },
      { process_key: "map-desert", run_id: 2, status: "Running" }] } });
  await choose("ark_give_item_to_player");
  await act(async () => clearButton().click());
  check(Boolean(fixture.querySelector(clearReviewSelector)), "The current map must have its own explicit confirmation");
  await input(".ark-map-toolbar select", "map-desert");
  check(!fixture.querySelector(clearReviewSelector) && calls.length === beforeDestructive,
    "Changing maps must discard the old map's unsubmitted wild-creature confirmation");
  await act(async () => clearButton().click());
  await act(async () => element<HTMLButtonElement>(".ark-world-tools .inline-confirm-submit").click());
  check(calls.length === beforeDestructive + 1 && calls.at(-1)?.command === "DestroyWildDinos"
    && calls.at(-1)?.processKey === "map-desert" && calls.at(-1)?.runId === 2,
    "Confirming the destructive action must dispatch exactly once");
  check(Boolean(fixture.querySelector('[data-configuration-section-id="ark_give_item_to_player"] [aria-current="page"]'))
    && Boolean(fixture.querySelector(".gmt-form.ark-item-spawn")),
    "Clearing wild creatures must leave the current item spawning panel selected and visible");
  const results = element(resultSelector);
  results.scrollIntoView({ block: "nearest" });
  const bounds = results.getBoundingClientRect();
  check(bounds.width > 0 && bounds.left >= 0 && bounds.right <= innerWidth + 1,
    "Dispatch results must fit the desktop viewport");
  check(document.documentElement.scrollWidth <= innerWidth + 1, "Tool feedback must not overflow horizontally");
  const worldBounds = element(".ark-world-tools").getBoundingClientRect();
  check(worldBounds.top >= itemButton().getBoundingClientRect().bottom
    && worldBounds.bottom <= element(".ark-map-rail").getBoundingClientRect().bottom,
    "The persistent world controls and feedback must stay below the item action without overlapping");
  const worldIcon = element(".ark-world-tool-grid .ark-spawn-tool svg").getBoundingClientRect();
  check(worldIcon.width > 0 && worldIcon.width <= 32,
    "Real server-workbench styles must be loaded for visual acceptance");
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, browser_errors: errors, calls: calls.length,
    viewport: { width: innerWidth, height: innerHeight } };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`GM tool interaction stalled after ${checks} checks`)), 25000);
})]).finally(() => {
  clearTimeout(watchdog);
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
}).catch((error) => ({ status: "failed", checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
