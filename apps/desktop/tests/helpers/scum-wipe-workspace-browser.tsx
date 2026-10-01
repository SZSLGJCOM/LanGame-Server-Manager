import { act } from "react";
import { createRoot } from "react-dom/client";
import { readModuleDetails } from "../../src/api";
import { buildMockSettingsForModule } from "../../src/api-mock/module-settings";
import { I18nProvider, useI18n } from "../../src/i18n";
import type { InstanceDetails, ModuleDetails, SaveInstanceSettingsOptions, UpdateInstanceInput } from "../../src/types";
import { ScumWipeSettingsEditor } from "../../src/views/servers/ScumWipeSettingsEditor";
import { InstanceSettingsSaveProvider, useInstanceSettingsSaveCoordinator } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceSettingsSaveCoordinator } from "../../src/views/settings/instance-settings-save-coordinator";
import { parseGuidedSettingsSchema } from "../../src/views/settings/guided-settings";
import { buildConfigurationWorkspaceModel } from "../../src/views/settings/configuration-workspace-model";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const errors: string[] = [];
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
window.alert = window.confirm = window.prompt = () => { throw new Error("Unexpected native dialog"); };
let descriptor: ModuleDetails;
let details: InstanceDetails;
let revision = 0;
let readOnly = false;
let fail = false;
let hold: Promise<void> | null = null;
let coordinator: InstanceSettingsSaveCoordinator;
const keys = ["partial_wipe", "gold_wipe", "full_wipe"];
const calls: Array<{ input: UpdateInstanceInput; options?: SaveInstanceSettingsOptions }> = [];
const cases: string[] = [];
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
function check(condition: unknown, message: string): asserts condition { if (!condition) throw new Error(message); }
function Harness() {
  const { locale, t } = useI18n();
  coordinator = useInstanceSettingsSaveCoordinator();
  const schema = parseGuidedSettingsSchema(descriptor, locale, t);
  if (!schema.parseError && schema.presentationFields?.length) {
    const model = buildConfigurationWorkspaceModel(schema);
    for (const key of keys) {
      check(model.items.find((item) => item.fieldKey === `server_general.${key}`)?.owner === "maintenance", `${key} ownership`);
    }
    check(!model.actionableSectionIds.includes("maintenance"), "Wipe fields must not render under configuration");
  }
  return <ScumWipeSettingsEditor key={revision} details={details} moduleDetails={descriptor} locale={locale} t={t}
    readOnly={readOnly} onSaveSettings={async (input, options) => {
      calls.push({ input: structuredClone(input), options: structuredClone(options) });
      if (hold) await hold;
      if (fail) throw new Error("Synthetic persistence failure");
      details = { ...details, settings_json: input.settings_json };
      return details;
    }} />;
}
async function render(remount = false) {
  if (remount) revision++;
  await act(async () => {
    root.render(<I18nProvider><InstanceSettingsSaveProvider>
      <main className="server-detail-panel" style={{ margin: 24, padding: 24 }}><Harness /></main>
    </InstanceSettingsSaveProvider></I18nProvider>);
    await frame();
  });
  const deadline = performance.now() + 5000;
  while (!fixture.querySelector("form")) {
    check(performance.now() < deadline, `Editor did not mount: ${fixture.textContent}`);
    await act(async () => { await frame(); });
  }
}
function control(key = "partial_wipe") {
  const input = fixture.querySelector<HTMLInputElement>(`[data-field-key="server_general.${key}"] input`);
  check(input, `Missing ${key}`); return input;
}
function submit() {
  const button = fixture.querySelector<HTMLButtonElement>('button[type="submit"]');
  check(button, "Missing explicit save action"); return button;
}
async function toggle(key = "partial_wipe", confirm = true) {
  await act(async () => { control(key).click(); });
  if (confirm) await act(async () => { fixture.querySelector<HTMLButtonElement>(".scum-settings-confirmation .danger")?.click(); });
}
async function save() { await act(async () => { submit().click(); await frame(); }); }
async function reload() {
  await act(async () => { fixture.querySelector<HTMLButtonElement>('.server-backup-policy-footer button[type="button"]')!.click(); });
}
function changeGeneral(patch: Record<string, unknown>) {
  const current = JSON.parse(details.settings_json);
  details = { ...details, settings_json: JSON.stringify({ ...current, server_general: { ...current.server_general, ...patch } }) };
}
async function run() {
  await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]);
  check(!Object.hasOwn(window, "__TAURI_INTERNALS__"), "Use synthetic saves only");
  descriptor = await readModuleDetails("scum");
  details = {
    summary: { id: "scum-wipe-review", module_id: "scum", name: "SCUM review", status: "Stopped", active_process_count: 0, autostart: false, bind_ip: "0.0.0.0" },
    settings_json: JSON.stringify(buildMockSettingsForModule("scum", "SCUM review", "scum-wipe-review")),
    ports: descriptor.default_ports, config_file_path: "C:/synthetic/scum/settings.json", saves_path: "C:/synthetic/scum/saves",
    backup_uses_declared_saves_path: true, auto_backup_on_stop: false, backup_retention_count: 5, active_run: null
  };
  await render(true);
  check(fixture.querySelectorAll('input[type="checkbox"]').length === 3, "Only three wipe switches belong here");
  for (const key of keys) {
    const before = calls.length;
    await toggle(key, false);
    check(!control(key).checked && fixture.querySelector(".scum-settings-confirmation"), "Enable requires confirmation");
    check(fixture.querySelector(".scum-settings-confirmation")?.textContent?.includes("持续保留"),
      "Each wipe confirmation must explain persistence until manually disabled");
    await act(async () => { fixture.querySelector<HTMLButtonElement>(".scum-settings-confirmation .secondary-button")!.click(); });
    check(!control(key).checked, "Cancel must leave the switch unchanged");
    await toggle(key);
    await coordinator.flush(details.summary.id);
    check(control(key).checked && calls.length === before, "Draft and start flush must never auto-save a wipe");
    changeGeneral({ server_name: "Updated elsewhere", future_native_key: "preserve" });
    await render();
    const baseline = details.settings_json;
    await save();
    const call = calls.at(-1)!;
    check(calls.length === before + 1 && call.options?.expectedSettingsJson === baseline && call.options.throwOnError, "Explicit save must use CAS once");
    const general = JSON.parse(call.input.settings_json).server_general;
    check(general[key] === true && general.server_name === "Updated elsewhere" && general.future_native_key === "preserve", "Scoped nested merge lost data");
    await render(true);
    check(control(key).checked, "Persisted value must survive remount");
    await toggle(key);
    await save();
    await render(true);
  }
  cases.push("three confirmed switches, explicit CAS saves, scoped merge and readback");
  await toggle();
  changeGeneral({ partial_wipe: true });
  await render();
  await toggle();
  const beforeConflict = calls.length;
  await save();
  check(calls.length === beforeConflict && fixture.textContent?.includes("其他位置"), "Concurrent switch changes must conflict");
  let conflictRejected = false;
  await coordinator.flush(details.summary.id).catch(() => { conflictRejected = true; });
  check(conflictRejected, "A failed conflict save must reject the start barrier");
  await reload();
  await coordinator.flush(details.summary.id);
  check(control().checked, "Reload must display the persisted value");
  cases.push("same-field conflict and explicit reload");
  await toggle();
  fail = true;
  await save();
  check(fixture.textContent?.includes("Synthetic persistence failure") && !control().checked, "Failure must retain draft");
  let rejected = false;
  await coordinator.flush(details.summary.id).catch(() => { rejected = true; });
  check(rejected, "A failed explicit save must reject the start barrier");
  fail = false;
  let release!: () => void;
  hold = new Promise<void>((resolve) => { release = resolve; });
  await save();
  let flushed = false;
  const flush = coordinator.flush(details.summary.id).then(() => { flushed = true; });
  await frame();
  check(!flushed && submit().disabled, "Start must wait for pending persistence");
  await act(async () => { release(); await flush; await frame(); });
  hold = null;
  check(flushed && fixture.textContent?.includes("已保存"), "Retry must confirm persistence");
  cases.push("failure retention, retry and start barrier");
  await render();
  for (const status of ["Starting", "Stopping", "Running", "Error", "Unknown"]) {
    details = { ...details, summary: { ...details.summary, status } };
    await render();
    check(keys.every((key) => control(key).disabled) && submit().disabled, `${status} must block wipe edits`);
  }
  details = { ...details, summary: { ...details.summary, status: "Stopped", active_process_count: 1 } };
  await render();
  check(control().disabled, "A remaining process must block edits");
  details = { ...details, summary: { ...details.summary, active_process_count: 0 }, active_run: { processes: [] } as unknown as InstanceDetails["active_run"] };
  await render();
  check(control().disabled, "Any active run must block edits even with zero processes");
  details = { ...details, active_run: null };
  cases.push("explicit stopped state, process and active-run restrictions");
  readOnly = true;
  await render();
  check(control().disabled && !fixture.querySelector('button[type="submit"]'), "Archive must be read-only");
  readOnly = false;
  const persisted = details;
  details = { ...details, settings_json: "{invalid" };
  await render(true);
  check(submit().disabled, "Malformed current settings must block saves");
  details = persisted;
  const realDescriptor = descriptor;
  for (const schema_json of ["", "{}", "{invalid", JSON.stringify({ properties: { server_general: { properties: { partial_wipe: { type: "boolean" } } } } })]) {
    descriptor = { ...realDescriptor, schema_json };
    await render(true);
    check(submit().disabled && control().disabled, "Missing native wipe schema must block dangerous edits");
  }
  descriptor = realDescriptor;
  changeGeneral({ server_name: "Invalid\nold name" });
  await render(true);
  await toggle();
  check(!submit().disabled, "Unrelated legacy setting validation must not silently block maintenance");
  await reload();
  changeGeneral({ server_name: "Updated elsewhere" });
  await render(true);
  await toggle();
  details = { ...details, summary: { ...details.summary, id: "second-scum-instance" } };
  await render();
  check(!control().checked, "Draft must not leak to another instance");
  cases.push("archive, invalid settings and instance isolation");
  await act(async () => { await document.fonts.ready; await frame(); });
  check(document.documentElement.scrollWidth <= innerWidth + 1, "Editor must fit viewport");
  check(errors.length === 0, "Browser and React errors must be absent");
  cases.push("layout and browser health");
  return { status: "passed", cases, save_boundary: "Synthetic callbacks only; no native wipe or server start", browser_errors: errors };
}
Object.assign(window, { __reliabilityFixtureCleanup: async () => {
  await act(async () => { root.unmount(); });
  console.error = originalError;
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  return { browser_errors: errors, native_dialogs: 0 };
} });
void run().catch((error: unknown) => ({ status: "failed", cases, error: String(error instanceof Error ? error.stack : error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
