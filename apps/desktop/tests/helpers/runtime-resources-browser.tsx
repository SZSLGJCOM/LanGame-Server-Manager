import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { bootstrapApp, readInstanceDetails } from "../../src/api";
import { I18nProvider, useI18n } from "../../src/i18n";
import { RuntimePerformanceEditor } from "../../src/views/servers/RuntimePerformanceEditor";
import type { InstanceDetails, RuntimeResourceLimits, UpdateInstanceInput } from "../../src/types";
import "../../src/app.css";
import "../../src/views/servers/instance-themes.css";
import "../../src/views/servers/workbench.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
let root = createRoot(fixture);
let details: InstanceDetails;
let applied: RuntimeResourceLimits | null = null;
let checks = 0;
const writes: UpdateInstanceInput[] = [];
let rejectSave = false;
const storageKey = `resource-fixture-${nonce}`;
function check(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
function select<T extends Element>(selector: string): T {
  const value = fixture.querySelector<T>(selector); check(value, `Missing ${selector}`); return value;
}
function View() {
  const { t } = useI18n();
  return <RuntimePerformanceEditor details={details} t={t} appliedLimits={applied} onSaveSettings={async (input, options) => {
    check(options?.expectedSettingsJson === details.settings_json, "Save lost compare-and-swap baseline");
    if (rejectSave) throw new Error("fixture write rejected");
    writes.push(input);
    localStorage.setItem(storageKey, input.settings_json);
    details = { ...details, settings_json: input.settings_json };
    return details;
  }} />;
}
async function render() { await act(async () => root.render(<I18nProvider><View /></I18nProvider>)); }
async function waitFor(ready: () => boolean) {
  const deadline = performance.now() + 8000;
  while (!ready()) { check(performance.now() < deadline, "Timed out waiting for resource editor");
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); }); }
}
async function input(name: string, value: string) {
  await act(async () => {
    const control = select<HTMLInputElement>(`input[name=${name}]`);
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(control, value);
    control.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
async function save() { await act(async () => select<HTMLFormElement>("form").requestSubmit()); }
async function run() {
  // Load the real catalog before mounting so its dynamic import settles within
  // React's act boundary rather than racing the interaction checks.
  await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]);
  const bootstrap = await bootstrapApp({ includeSystemSnapshot: false });
  const instance = bootstrap.state.instances.find((item) => item.module_id === "dontstarve");
  check(instance, "DST fixture missing");
  const source = await readInstanceDetails(instance.id);
  details = { ...source, summary: { ...source.summary, status: "Stopped", active_process_count: 0 }, active_run: null,
    settings_json: '{"server_name":"Keep this world"}' };
  await render(); await waitFor(() => Boolean(fixture.querySelector("input[name=cpu]")));
  check(select<HTMLInputElement>("input[name=cpu]").value === "", "Default unexpectedly limits CPU");
  check(fixture.textContent?.includes("地表和洞穴共用一份预算"), "Shared-world budget explanation missing"); checks++;

  await input("cpu", "101");
  check(select<HTMLInputElement>("input[name=cpu]").getAttribute("aria-invalid") === "true", "Invalid CPU not exposed accessibly");
  await save(); check(writes.length === 0, "Invalid cap was saved"); checks++;
  await input("cpu", "25"); await input("memory", "8192"); await input("reserve", "2048");
  rejectSave = true; await save();
  check(fixture.textContent?.includes("fixture write rejected"), "Write failure hidden");
  check(select<HTMLInputElement>("input[name=memory]").value === "8192", "Failed save discarded draft"); checks++;
  rejectSave = false; await save();
  check(writes.length === 1, "Valid retry did not persist once");
  const saved = JSON.parse(writes[0].settings_json);
  check(saved.server_name === "Keep this world" && saved.runtime_performance.resource_limits.memory_limit_mib === 8192, "Resource save changed game data or units"); checks++;

  await act(async () => root.unmount());
  root = createRoot(fixture);
  details = { ...details, settings_json: localStorage.getItem(storageKey)! };
  await render(); await waitFor(() => Boolean(fixture.querySelector("input[name=cpu]")));
  check(select<HTMLInputElement>("input[name=cpu]").value === "25" && select<HTMLInputElement>("input[name=memory]").value === "8192", "Remount did not read saved caps"); checks++;

  await act(async () => select<HTMLInputElement>("input[name=cpu]").focus());
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/Tab`, { method: "POST" });
    check(response.ok, "Native Tab dispatch failed");
  });
  check(document.activeElement === select("input[name=memory]"), "Keyboard tab order does not follow resource fields"); checks++;

  details = { ...details, summary: { ...details.summary, status: "Running", active_process_count: 2 } };
  applied = { cpu_percent: 50, memory_limit_mib: 4096, host_memory_reserve_mib: 2048 };
  await render();
  check(Array.from(fixture.querySelectorAll<HTMLInputElement>("input")).every((node) => node.disabled), "Running instance limits remain editable");
  check(select(".server-resource-effective").textContent?.includes("4096 MiB"), "UI substituted saved 8192 MiB for actually applied 4096 MiB"); checks++;
  applied = null; await render();
  check(select(".server-resource-effective").textContent?.includes("尚未确认"), "Unknown applied limits were represented as unlimited"); checks++;

  fixture.style.width = "340px";
  applied = { cpu_percent: 50, memory_limit_mib: 4096, host_memory_reserve_mib: 2048 };
  await render();
  check(fixture.scrollWidth <= fixture.clientWidth + 1, "Narrow resource panel overflows horizontally");
  for (const control of fixture.querySelectorAll("input")) {
    const box = control.getBoundingClientRect();
    check(box.width > 40 && box.right <= fixture.getBoundingClientRect().right + 1, "Resource input is clipped");
  }
  fixture.style.width = "640px"; await render();
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`); checks++;
  localStorage.removeItem(storageKey);
  return { status: "passed", checks, browser_errors: errors, saves: writes.length };
}
void run().catch((error) => ({ status: "failed", checks, error: String(error instanceof Error ? error.stack : error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
