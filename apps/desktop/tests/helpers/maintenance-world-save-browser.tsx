import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { invokeMock } from "../../src/api-mock";
import { I18nProvider } from "../../src/i18n";
import type { BootstrapResponse, InstanceDetails, InstanceRuntimeCommandResult, ModuleDetails } from "../../src/types";
import { ImmediateWorldSave } from "../../src/views/servers/ImmediateWorldSave";
import { MaintenanceWorkspace } from "../../src/views/servers/MaintenanceWorkspace";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

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
const calls: Array<{ instanceId: string; command: string; runtimeActionId: string }> = [];
let responseMode: "confirmed" | "pending" | "failed" | "deferred" = "confirmed";
let resolveSave: ((value: InstanceRuntimeCommandResult) => void) | undefined;
// Replace only the native IPC boundary. The production component, configuration
// guards and API envelope run against current module TOML declarations.
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args: {
  input: { instanceId: string; command: string; runtimeActionId: string }
}) => {
  if (command !== "send_instance_runtime_command") throw new Error(`Unexpected native command: ${command}`);
  calls.push(args.input);
  if (responseMode === "failed") throw new Error("Native save connection lost");
  if (responseMode === "deferred") return new Promise<InstanceRuntimeCommandResult>(resolve => { resolveSave = resolve; });
  return { instance_id: args.input.instanceId, command: args.input.command, process_key: "main", display_name: "Save transport",
    pid: 1, response_text: "Native save response", write_confirmation_pending: responseMode === "pending", submitted_at_unix_ms: Date.now() };
} } });
let checks = 0;
function check(condition: unknown, message: string): asserts condition { if (!condition) throw new Error(message); checks++; }
async function settle(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(message);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function render(details: InstanceDetails, moduleDetails: ModuleDetails | null, active = true) {
  await act(async () => root.render(<StrictMode><I18nProvider><main style={{ padding: 24 }}>
    <MaintenanceWorkspace active={active} sections={[
      { id: "backups", title: "存档与备份", icon: "history", content: null,
        persistentContent: <ImmediateWorldSave details={details} moduleDetails={moduleDetails} /> },
      { id: "other", title: "其他维护", icon: "settings", content: <p className="maintenance-costly-reader">Other maintenance content</p> }
    ]} broadcast={<p>Broadcast boundary</p>} />
    </main></I18nProvider></StrictMode>));
  await settle(() => Boolean(fixture.querySelector(".immediate-world-save")), "Save panel did not mount");
}
const button = () => fixture.querySelector<HTMLButtonElement>(".immediate-world-save button")!;
async function submit() { await act(async () => { button().click(); }); }
async function chooseSection(title: string) {
  const target = Array.from(fixture.querySelectorAll<HTMLButtonElement>(".configuration-workspace__sidebar button"))
    .find(node => node.textContent?.trim() === title);
  if (!target) throw new Error(`Missing maintenance section ${title}`);
  await act(async () => { target.click(); });
}
async function run() {
  const bootstrap = await invokeMock<BootstrapResponse>("bootstrap", { includeSystemSnapshot: false });
  const original = bootstrap.state.instances.find((entry) => entry.module_id === "minecraft")!;
  const stored = await invokeMock<InstanceDetails>("read_instance_details_from_storage", { instance_id: original.id });
  let last: InstanceDetails = stored;
  let lastModule: ModuleDetails | null = null;
  for (const [moduleId, command] of [["minecraft", "save-all flush"], ["projectzomboid", "save"], ["terraria", "save"], ["palworld", "save"]]) {
    const module = await invokeMock<ModuleDetails>("read_module_details", { moduleId });
    const details: InstanceDetails = { ...stored, summary: { ...stored.summary, id: `${moduleId}-save-fixture`, module_id: moduleId,
      status: "Running", active_process_count: 1 }, active_run: { run_id: 1, processes: [{ process_key: "main", status: "Running" }] },
      settings_json: JSON.stringify({ enable_rcon: true, rcon_password: "fixture-server-password", rest_api_enabled: true, admin_password: "fixture-admin-password" }),
      ports: [{ name: "rcon", port: 25575, protocol: "tcp" }, { name: "rest_api", port: 8212, protocol: "tcp" }] };
    await render(details, module);
    check(!button().disabled, `${moduleId} save must be available with its declared configuration`);
    await submit();
    check(calls.at(-1)?.runtimeActionId === "save_world" && calls.at(-1)?.command === command,
      `${moduleId} must submit the declared save action`);
    check(fixture.textContent?.includes("Native save response"), "Native result must remain visible");
    last = details; lastModule = module;
  }
  await render({ ...last, summary: { ...last.summary, status: "Stopped", active_process_count: 0 }, active_run: null }, lastModule);
  check(button().disabled && fixture.textContent?.includes("请先启动服务器"), "Stopped saves need an explanation");
  await render(last, null);
  check(button().disabled && fixture.textContent?.includes("保存能力尚未就绪"), "Unloaded save capability must remain safely unavailable");
  await render({ ...last, settings_json: JSON.stringify({ rest_api_enabled: false }) }, lastModule);
  check(button().disabled && fixture.textContent?.includes("启用 REST API"), "REST configuration must be discoverable");
  await render(last, lastModule);
  responseMode = "pending"; await submit();
  check(fixture.textContent?.includes("仍等待写入确认"), "Pending save must not be presented as completed");
  responseMode = "failed"; await submit();
  check(fixture.querySelector('[role="alert"]')?.textContent?.includes("Native save connection lost"), "Native failure must remain visible");
  responseMode = "deferred"; await submit();
  const savePanel = fixture.querySelector(".immediate-world-save");
  check(button().disabled, "A pending save must prevent another submission");
  await chooseSection("其他维护");
  check(fixture.querySelector(".immediate-world-save") === savePanel && savePanel?.getClientRects().length === 0,
    "Changing maintenance categories must hide and preserve the save operation");
  await chooseSection("存档与备份");
  check(button().disabled, "Returning to save maintenance must retain the pending request");
  await render(last, lastModule, false);
  check(fixture.querySelector(".immediate-world-save") === savePanel && !fixture.querySelector(".maintenance-costly-reader"),
    "Leaving maintenance must retain only immediate save state, not costly readers");
  const request = calls[calls.length - 1];
  await act(async () => resolveSave!({ instance_id: request.instanceId, command: request.command, process_key: "main",
    display_name: "Save transport", pid: 1, response_text: "Native save response after leaving maintenance",
    write_confirmation_pending: false, submitted_at_unix_ms: Date.now() }));
  await render(last, lastModule);
  check(!button().disabled && fixture.textContent?.includes("Native save response after leaving maintenance"),
    "A save response received on another tab must remain visible on return");
  const rect = fixture.querySelector(".immediate-world-save")!.getBoundingClientRect();
  check(rect.width > 0 && rect.left >= 0 && rect.right <= innerWidth, "Maintenance save must fit the viewport");
  check(document.documentElement.scrollWidth <= innerWidth + 1, "Save feedback must not overflow horizontally");
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, browser_errors: errors, viewport: { width: innerWidth, height: innerHeight } };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error("Save UI stalled")), 25000); })])
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .catch((error) => ({ status: "failed", checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
