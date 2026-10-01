import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { invokeMock } from "../../src/api-mock";
import { I18nProvider } from "../../src/i18n";
import type { BootstrapResponse, InstanceDetails, InstanceRuntimeCommandResult } from "../../src/types";
import { GMToolsWorkbench } from "../../src/views/servers/GMToolsWorkbench";
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
type Request = { instanceId: string; command: string; processKey?: string | null; transport?: string };
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
  if (command === "read_ark_tools_status") return Promise.resolve({ installed: true, connected: true, issue: null });
  if (command !== "send_instance_gm_command" || !args.input) throw new Error(`Unexpected IPC: ${command}`);
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
  await act(async () => root.render(<StrictMode><I18nProvider><main style={{ padding: 24 }}>
    <GMToolsWorkbench details={details} /></main></I18nProvider></StrictMode>));
  await settle(() => Boolean(fixture.querySelector(".gmt-workbench")), "Tool workbench did not render");
  if (details.summary.module_id.startsWith("arksurvival")) await choose("ark_set_time");
}
const button = () => element<HTMLButtonElement>('.gmt-form button[type="submit"]');
async function submit() { await act(async () => { button().click(); }); }
async function choose(toolId: string) {
  const index = getGmToolCatalog(details.summary.module_id)!.tools.findIndex((tool) => tool.id === toolId);
  check(index >= 0, `Tool ${toolId} must exist`);
  await act(async () => fixture.querySelectorAll<HTMLButtonElement>(".gmt-nav-btn")[index].click());
}
async function input(id: string, value: string) {
  const field = element<HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement>(`#${id}`);
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
    active_run: running ? { run_id: 1, processes: [{ process_key: "main", status: "Running" },
      { process_key: "master", status: "Running" }, { process_key: "caves", status: "Stopped" }] } : null });
}
async function run() {
  const bootstrap = await invokeMock<BootstrapResponse>("bootstrap", { includeSystemSnapshot: false });
  const instance = bootstrap.state.instances.find((candidate) => candidate.module_id === "minecraft")!;
  details = await invokeMock<InstanceDetails>("read_instance_details_from_storage", { instanceId: instance.id, instance_id: instance.id });
  details = { ...details, settings_json: JSON.stringify({ ...JSON.parse(details.settings_json),
    rcon_enabled: true, admin_password: "fixture-admin-password" }), ports: [
    { name: "rcon", protocol: "tcp", port: 25575 }, { name: "rest_api", protocol: "tcp", port: 8212 }
  ] };
  await moduleDetails("arksurvivalascended", "ark-a");
  await choose("ark_spawn_creature");
  const spawner = element(".ark-creature-spawner");
  await choose("ark_set_time");
  await choose("ark_spawn_creature");
  check(element(".ark-creature-spawner") === spawner, "Switching tools must preserve the mounted spawning operation and its in-flight lock");
  await choose("ark_set_time");
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
  check(calls.length === 1 && calls[0].command === "SetTimeOfDay 12:00" && calls[0].transport === "source_rcon",
    "Real ARK time form must dispatch its builder command through the production API envelope");
  check(element(".gmt-command-results").textContent?.includes("Unknown command: native diagnostic"),
    "Native game rejection text must remain visible");
  check(!fixture.querySelector(".shell-activity-notice.is-success"), "A transport response must not become a game success toast");

  mode = "deferred";
  await submit();
  check(button().disabled && deferred.length === 1, "In-flight command must lock the form");
  await render({ ...details, settings_json: JSON.stringify({ ...JSON.parse(details.settings_json), ui_refresh: true }) });
  await submit();
  check(calls.length === 2 && button().disabled, "Settings refresh must not release the in-flight lock");
  await moduleDetails("arksurvivalascended", "ark-b");
  check(button().disabled && !fixture.querySelector(".gmt-command-results"), "Other instance must not inherit the old response");
  const previous = deferred.shift()!;
  await act(async () => previous.resolve(response(previous.request, "OLD INSTANCE RESPONSE")));
  check(!button().disabled && !fixture.textContent?.includes("OLD INSTANCE RESPONSE"), "Late native result must not pollute the selected instance");

  mode = "pending";
  await moduleDetails("dontstarve", "dst-pending");
  await submit();
  check(element(".gmt-command-results").textContent?.includes("仍等待写入确认"), "Pending stdin must not be described as sent");
  check(element(".gmt-command-results small").textContent === "等待写入确认", "Per-command pending state must be visible");

  await moduleDetails("arksurvivalascended", "ark-invalid");
  await input("gm-ark_set_time-time-input", "99:99");
  const beforeInvalid = calls.length;
  await submit();
  check(button().disabled && calls.length === beforeInvalid && Boolean(fixture.querySelector(".gmt-tool-availability")),
    "Invalid time must explain its error before native dispatch");

  await moduleDetails("dontstarve", "dst-a");
  await input("gm-dst_give_item_to_player-shard-input", "caves");
  check(button().disabled && fixture.textContent?.includes("caves 未运行"), "Unavailable shard must show why dispatch is disabled");
  await moduleDetails("arksurvivalascended", "ark-stopped", false);
  check(button().disabled && Boolean(fixture.querySelector(".gmt-tool-availability")), "Stopped server must show why its tool is unavailable");

  await moduleDetails("arksurvivalascended", "ark-b");
  await choose("ark_give_item_to_player");
  await input("gm-ark_give_item_to_player-playerId-input", "12345");
  await input("gm-ark_give_item_to_player-itemMode-input", "batch");
  await input("gm-ark_give_item_to_player-lines-input", "9,2,1,0\n76,1,1,0\n8,1,1,0");
  mode = "partial";
  const beforeBatch = calls.length;
  await submit();
  check(calls.length === beforeBatch + 2, "Batch must stop at the first unconfirmed command");
  check(element(".gmt-command-results").textContent?.includes("First reward accepted")
    && element(".gmt-command-results [role=alert]").textContent?.includes("已发送 1/3 条"), "Partial batch must retain delivered responses and the failure count");
  check(element(".gmt-command-results [role=alert]").textContent?.includes("native connection lost"), "Original transport error must remain visible");

  mode = "deferred";
  await submit();
  const beforeSwitch = calls.length;
  await moduleDetails("arksurvivalascended", "ark-c");
  const batch = deferred.shift()!;
  await act(async () => batch.resolve(response(batch.request, "old batch reply")));
  check(calls.length === beforeSwitch, "Switching instances must stop unsent batch mutations");

  mode = "response";
  await choose("ark_destroy_wild_dinos");
  await submit();
  const results = element(".gmt-command-results");
  results.scrollIntoView({ block: "nearest" });
  const bounds = results.getBoundingClientRect();
  check(bounds.width > 0 && bounds.left >= 0 && bounds.right <= innerWidth + 1,
    "Dispatch results must fit the desktop viewport");
  check(document.documentElement.scrollWidth <= innerWidth + 1, "Tool feedback must not overflow horizontally");
  check(element(".gmt-no-fields-icon").getBoundingClientRect().width <= 32,
    "Real server-workbench styles must be loaded for visual acceptance");
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, browser_errors: errors, calls: calls.length, viewport: { width: innerWidth, height: innerHeight } };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`GM tool interaction stalled after ${checks} checks`)), 25000);
})]).finally(() => {
  clearTimeout(watchdog);
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
}).catch((error) => ({ status: "failed", checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
