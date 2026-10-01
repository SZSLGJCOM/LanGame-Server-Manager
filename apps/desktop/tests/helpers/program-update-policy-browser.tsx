import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { bootstrapApp, readInstanceDetails, readModuleDetails } from "../../src/api";
import { I18nProvider, useI18n } from "../../src/i18n";
import { ProgramUpdatePolicyEditor } from "../../src/views/servers/ProgramUpdatePolicyEditor";
import { InstanceSettingsSaveProvider, useInstanceSettingsSaveCoordinator } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceSettingsSaveCoordinator } from "../../src/views/settings/instance-settings-save-coordinator";
import type { InstanceDetails, ModuleDetails, UpdateInstanceInput } from "../../src/types";
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
let moduleDetails: ModuleDetails | null;
let coordinator: InstanceSettingsSaveCoordinator;
let readOnly = false;
let checks = 0;
let attempts = 0;
const writes: UpdateInstanceInput[] = [];
let saveFailure: string | null = null;
let waitForSave: Promise<void> | null = null;
let noReadback = false;
const storageKey = `program-policy-fixture-${nonce}`;
function check(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
function select<T extends Element>(selector: string): T {
  const value = fixture.querySelector<T>(selector); check(value, `Missing ${selector}`); return value;
}
function View() {
  const { t } = useI18n();
  coordinator = useInstanceSettingsSaveCoordinator();
  return <ProgramUpdatePolicyEditor details={details} moduleDetails={moduleDetails} t={t} readOnly={readOnly}
    onSaveSettings={async (input, options) => {
      attempts++;
      check(options?.expectedSettingsJson === details.settings_json, "Save lost compare-and-swap baseline");
      check(options?.throwOnError === true, "Save would hide persistence failures");
      if (saveFailure) throw new Error(saveFailure);
      if (waitForSave) await waitForSave;
      if (noReadback) return undefined;
      writes.push(input);
      localStorage.setItem(storageKey, input.settings_json);
      details = { ...details, settings_json: input.settings_json };
      return details;
    }} />;
}
async function render() {
  await act(async () => root.render(<I18nProvider><InstanceSettingsSaveProvider><View /></InstanceSettingsSaveProvider></I18nProvider>));
}
async function change(value: "automatic" | "pinned") {
  await act(async () => {
    const control = select<HTMLSelectElement>("select");
    control.value = value;
    control.dispatchEvent(new Event("change", { bubbles: true }));
  });
}
async function save() { await act(async () => select<HTMLFormElement>("form").requestSubmit()); }
async function discard() { await act(async () => select<HTMLButtonElement>("button.ghost-button").click()); }
async function waitFor(ready: () => boolean) {
  const deadline = performance.now() + 5000;
  while (!ready()) {
    check(performance.now() < deadline, "Policy editor did not settle");
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function run() {
  await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]);
  const bootstrap = await bootstrapApp({ includeSystemSnapshot: false });
  const instance = bootstrap.state.instances.find((item) => item.module_id === "dontstarve");
  check(instance, "Isolated catalog fixture missing");
  const source = await readInstanceDetails(instance.id);
  const supportedModule = await readModuleDetails(instance.module_id);
  moduleDetails = supportedModule;
  details = { ...source, summary: { ...source.summary, status: "Stopped", active_process_count: 0 }, active_run: null,
    settings_json: '{"server_name":"Keep this world","mods":["123"]}' };
  await render(); await waitFor(() => Boolean(fixture.querySelector("select")));
  check(select<HTMLSelectElement>("select").value === "automatic", "Existing instance did not default to automatic");
  check(select<HTMLButtonElement>("button[type=submit]").disabled, "Unchanged policy can be submitted");
  check(fixture.textContent?.includes("每次启动前检查官方版本"), "Automatic behavior is unclear"); checks++;

  await change("pinned");
  check(fixture.textContent?.includes("任一实例"), "Shared version protection is not explained");
  check(select<HTMLSelectElement>("select").getAttribute("aria-describedby"), "Policy has no accessible explanation");
  await act(async () => select<HTMLSelectElement>("select").focus());
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/Tab`, { method: "POST" });
    check(response.ok, "Native Tab dispatch failed");
  });
  check(document.activeElement === select("button.ghost-button"), "Keyboard focus does not reach discard control"); checks++;

  saveFailure = "fixture write rejected";
  await save();
  check(fixture.textContent?.includes("fixture write rejected"), "Save failure was hidden");
  check(select<HTMLSelectElement>("select").value === "pinned" && writes.length === 0, "Failed save lost the draft or fabricated success");
  let failedBarrier = false;
  try { await coordinator.flush(instance.id); } catch { failedBarrier = true; }
  check(failedBarrier, "Startup barrier ignored the failed policy write"); checks++;

  saveFailure = null;
  details = { ...details, settings_json: '{"server_name":"Concurrent name","mods":["123"],"backup":{"retention":9}}' };
  await render(); await save();
  check(writes.length === 1, "Retry did not persist exactly once");
  const saved = JSON.parse(writes[0].settings_json);
  check(saved.program_update.policy === "pinned" && saved.server_name === "Concurrent name"
    && saved.backup.retention === 9 && saved.mods[0] === "123", "Policy overwrote unrelated settings");
  check(JSON.stringify(writes[0].ports) === JSON.stringify(source.ports)
    && writes[0].auto_backup_on_stop === source.auto_backup_on_stop, "Policy changed network or backup fields");
  await coordinator.flush(instance.id); checks++;

  await act(async () => root.unmount());
  root = createRoot(fixture);
  details = { ...details, settings_json: localStorage.getItem(storageKey)! };
  await render(); await waitFor(() => Boolean(fixture.querySelector("select")));
  check(select<HTMLSelectElement>("select").value === "pinned", "Remount did not read the saved pin"); checks++;

  await change("automatic");
  saveFailure = "settings changed while this edit was pending";
  await save();
  check(fixture.textContent?.includes("更新策略已被其他操作修改"), "Concurrent write conflict was not explained");
  check(select<HTMLSelectElement>("select").value === "automatic" && writes.length === 1, "Conflict discarded draft or persisted a stale write");
  saveFailure = null; await discard();
  check(select<HTMLSelectElement>("select").value === "pinned", "Discard did not restore saved policy");
  await coordinator.flush(instance.id); checks++;

  await change("automatic");
  let release: (() => void) | undefined;
  waitForSave = new Promise<void>((resolve) => { release = resolve; });
  const priorAttempts = attempts;
  await save(); await save();
  check(attempts === priorAttempts + 1 && select<HTMLSelectElement>("select").disabled, "Pending save is not single-flight");
  let flushed = false;
  const flush = coordinator.flush(instance.id).then(() => { flushed = true; });
  await Promise.resolve(); check(!flushed, "Start barrier completed before the write");
  await act(async () => { release?.(); await flush; });
  waitForSave = null;
  await waitFor(() => !select<HTMLSelectElement>("select").disabled);
  check(writes.length === 2 && flushed, "Pending save did not settle"); checks++;

  await change("pinned"); noReadback = true; await save();
  check(fixture.textContent?.includes("未能确认更新策略已保存"), "Missing saved result was reported as success");
  noReadback = false; await discard(); checks++;

  details = { ...details, summary: { ...details.summary, status: "Running", active_process_count: 1 } };
  await render();
  check(select<HTMLSelectElement>("select").disabled && fixture.textContent?.includes("先停止实例"), "Running instance policy is editable");
  details = { ...details, summary: { ...details.summary, status: "Starting", active_process_count: 0 } };
  await render(); check(select<HTMLSelectElement>("select").disabled, "Starting instance policy is editable"); checks++;

  details = { ...details, summary: { ...details.summary, status: "Error", active_process_count: 0 }, active_run: null };
  await render();
  check(!select<HTMLSelectElement>("select").disabled, "Idle error state blocks recovery by changing the update policy");
  await change("pinned"); await save();
  check(writes.length === 3 && JSON.parse(writes[2].settings_json).program_update.policy === "pinned",
    "Idle error instance could not persist its recovery policy"); checks++;
  await change("automatic");
  const beforeActiveError = attempts;
  details = { ...details, summary: { ...details.summary, active_process_count: 1 } };
  await render(); await save();
  check(select<HTMLSelectElement>("select").disabled && attempts === beforeActiveError,
    "Error status with an active process allowed a policy write");
  details = { ...details, summary: { ...details.summary, active_process_count: 0 }, active_run: { run_id: 999 } };
  await render(); await save();
  check(select<HTMLSelectElement>("select").disabled && attempts === beforeActiveError,
    "Error status with an active run allowed a policy write");
  await discard(); checks++;

  details = { ...details, summary: { ...details.summary, status: "Stopped", active_process_count: 0 }, active_run: null };
  readOnly = true; await render();
  check(select<HTMLSelectElement>("select").disabled && !fixture.querySelector("button[type=submit]"), "Archived policy is writable");
  readOnly = false; checks++;

  moduleDetails = { ...supportedModule, install: { ...supportedModule.install!, download_url_windows: "https://example.invalid/server.zip" } };
  await render();
  check(!fixture.querySelector("select") && fixture.textContent?.includes("此安装源需手动维护"), "Whole-package source incorrectly advertises automatic updates");
  moduleDetails = null; await render();
  check(fixture.textContent?.includes("正在读取安装源"), "Loading source is not represented"); checks++;

  moduleDetails = supportedModule;
  for (const settings_json of ['{"program_update":{"policy":"unrecognized"}}', '{"program_update":{"polciy":"pinned"}}']) {
    details = { ...details, settings_json };
    await render();
    check(select<HTMLSelectElement>("select").value === "" && select<HTMLSelectElement>("select").disabled,
      "Malformed persisted policy silently enabled updates");
  }
  details = { ...details, settings_json: localStorage.getItem(storageKey)! };
  await render(); checks++;

  fixture.style.width = "340px"; await render();
  check(fixture.scrollWidth <= fixture.clientWidth + 1, "Narrow maintenance column overflows");
  const box = select("select").getBoundingClientRect();
  check(box.width > 100 && box.right <= fixture.getBoundingClientRect().right + 1, "Policy control is clipped");
  fixture.style.width = "640px"; checks++;
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`); checks++;
  localStorage.removeItem(storageKey);
  return { status: "passed", checks, browser_errors: errors, saves: writes.length };
}
void run().catch((error) => ({ status: "failed", checks, error: String(error instanceof Error ? error.stack : error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
