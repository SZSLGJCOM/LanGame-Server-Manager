import { act } from "react";
import { createRoot } from "react-dom/client";
import { readModuleDetails } from "../../src/api";
import { buildMockSettingsForModule } from "../../src/api-mock/module-settings";
import { I18nProvider, useI18n } from "../../src/i18n";
import type { InstanceDetails, ModuleDetails, SaveInstanceSettingsOptions, UpdateInstanceInput } from "../../src/types";
import { MoriaNativeSettingsEditor } from "../../src/views/servers/MoriaNativeSettingsEditor";
import { InstanceSettingsSaveProvider, useInstanceSettingsSaveCoordinator } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceSettingsSaveCoordinator } from "../../src/views/settings/instance-settings-save-coordinator";
import { parseGuidedSettingsSchema } from "../../src/views/settings/guided-settings";
import "../../src/app.css";

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
let kind: "permissions" | "world-upgrade" = "permissions";
let readOnly = false;
let fail = false;
let omitResult = false;
let hold: Promise<void> | null = null;
let coordinator: InstanceSettingsSaveCoordinator;
const calls: Array<{ input: UpdateInstanceInput; options?: SaveInstanceSettingsOptions }> = [];
const cases: string[] = [];
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
function check(condition: unknown, message: string): asserts condition { if (!condition) throw new Error(message); }
const key = () => kind === "permissions" ? "permissions_lines" : "upgrade_optional_dlc_array";
function Harness() {
  const { locale, t } = useI18n();
  coordinator = useInstanceSettingsSaveCoordinator();
  for (const [surface, fieldKey] of [["player_access", "permissions_lines"], ["maintenance", "upgrade_optional_dlc_array"]] as const) {
    const general = parseGuidedSettingsSchema(descriptor, locale, t);
    const owned = parseGuidedSettingsSchema(descriptor, locale, t, { surface });
    check(!general.fields.some((field) => field.key === fieldKey), `${fieldKey} must leave general configuration`);
    check(owned.fields.some((field) => field.key === fieldKey), `${fieldKey} must remain editable in ${surface}`);
  }
  return <MoriaNativeSettingsEditor key={revision} details={details} moduleDetails={descriptor} kind={kind}
    locale={locale} t={t} readOnly={readOnly} onSaveSettings={async (input, options) => {
      calls.push({ input: structuredClone(input), options: structuredClone(options) });
      if (hold) await hold;
      if (fail) throw new Error("Synthetic persistence failure");
      if (omitResult) return undefined;
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
function control() {
  const input = fixture.querySelector<HTMLInputElement | HTMLTextAreaElement>(`[data-field-key="${key()}"] input,[data-field-key="${key()}"] textarea`);
  check(input, `Missing native field ${key()}: ${fixture.textContent}`);
  return input;
}
function submit() {
  const button = fixture.querySelector<HTMLButtonElement>('button[type="submit"]');
  check(button, "Missing explicit save action");
  return button;
}
async function edit(value: string) {
  const input = control();
  await act(async () => {
    const prototype = input instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(prototype, "value")!.set!.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new Event("change", { bubbles: true }));
  });
}
async function save() { await act(async () => { submit().click(); await frame(); }); }
async function run() {
  check(!Object.hasOwn(window, "__TAURI_INTERNALS__"), "Use disposable mock saves only");
  await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]);
  descriptor = await readModuleDetails("returntomoria");
  details = {
    summary: { id: "moria-semantic-review", module_id: "returntomoria", name: "Moria review", status: "Stopped",
      active_process_count: 0, autostart: false, bind_ip: "0.0.0.0" },
    settings_json: JSON.stringify(buildMockSettingsForModule("returntomoria", "Moria review", "moria-semantic-review")),
    ports: descriptor.default_ports, config_file_path: "C:/synthetic/moria/settings.json", saves_path: "C:/synthetic/moria/saves",
    backup_uses_declared_saves_path: true, auto_backup_on_stop: false, backup_retention_count: 5, active_run: null
  };
  for (const scenario of ["permissions", "world-upgrade"] as const) {
    kind = scenario;
    await render(true);
    const value = kind === "permissions" ? "Default = AllConstruction,AllStorage\nExamplePlayer = Blocked" : "DurinsFolk";
    const before = calls.length;
    await edit(value);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 100)); });
    check(calls.length === before, "Editing must not auto-apply permissions or a world upgrade");
    details = { ...details, settings_json: JSON.stringify({ ...JSON.parse(details.settings_json), server_name: "Changed elsewhere" }) };
    await render();
    const expectedJson = details.settings_json;
    await save();
    check(calls.length === before + 1, "Explicit save must call the persistence boundary exactly once");
    const call = calls.at(-1)!;
    check(call.options?.expectedSettingsJson === expectedJson && call.options.throwOnError, "Save must require compare-and-set");
    check(JSON.parse(call.input.settings_json)[key()] === value, "Native field value changed before persistence");
    check(JSON.parse(call.input.settings_json).server_name === "Changed elsewhere", "Save overwrote another workspace's change");
    await render(true);
    check(control().value === value, "Saved value must survive editor remount");
    cases.push(`${kind}: explicit save, scoped merge and readback`);
  }
  kind = "permissions";
  await render(true);
  await edit("Default = AllConstruction");
  details = { ...details, settings_json: JSON.stringify({ ...JSON.parse(details.settings_json), permissions_lines: "Default = AllStorage" }) };
  await render();
  const beforeConflict = calls.length;
  await save();
  check(calls.length === beforeConflict && fixture.textContent?.includes("其他位置"), "Same-field changes must conflict before writing");
  check(control().value === "Default = AllConstruction", "Conflict must preserve the draft");
  let conflictBlockedStart = false;
  try { await coordinator.flush(details.summary.id); } catch { conflictBlockedStart = true; }
  check(conflictBlockedStart, "A rejected explicit conflict save must block starting the instance");
  await act(async () => { fixture.querySelector<HTMLButtonElement>('.server-backup-policy-footer button[type="button"]')!.click(); });
  await coordinator.flush(details.summary.id);
  check(control().value === "Default = AllStorage", "Discard must display the newly loaded value");
  cases.push("same-field conflict preserves draft; reload uses current value");

  await edit("Default = AllConstruction,AllStorage");
  omitResult = true;
  await save();
  check(fixture.textContent?.includes("未能确认"), "Missing saved details must not report success");
  omitResult = false;
  fail = true;
  await save();
  check(fixture.textContent?.includes("Synthetic persistence failure"), "Persistence failures must be visible");
  check(control().value === "Default = AllConstruction,AllStorage" && !submit().disabled, "Failed save must remain retryable");
  fail = false;
  let release!: () => void;
  hold = new Promise<void>((resolve) => { release = resolve; });
  await save();
  let flushed = false;
  const flush = coordinator.flush(details.summary.id).then(() => { flushed = true; });
  await frame();
  check(!flushed && submit().disabled, "A server start must wait for a pending explicit save");
  await act(async () => { release(); await flush; await frame(); });
  hold = null;
  check(flushed && fixture.textContent?.includes("已保存"), "Retry must clear failed start barrier and confirm persistence");
  await render();
  await edit("Default = AllStorage");
  fail = true;
  await save();
  fail = false;
  await act(async () => {
    fixture.querySelector<HTMLButtonElement>('.server-backup-policy-footer button[type="button"]')!.click();
    await coordinator.flush(details.summary.id);
  });
  check(control().value === "Default = AllConstruction,AllStorage", "Discarding a failed write must restore persisted settings");
  cases.push("failure, retry, discard and start barrier");

  for (const status of ["Running", "Starting", "Stopping", "Error"] as const) {
    details = { ...details, summary: { ...details.summary, status, active_process_count: 0 } };
    await render();
    check(control().disabled && submit().disabled, `${status} instances must not allow native file edits`);
  }
  details = { ...details, summary: { ...details.summary, status: "Stopped", active_process_count: 0 } };
  readOnly = true;
  await render();
  check(!fixture.querySelector('button[type="submit"]'), "Archived view must not expose save");
  const archiveInput = fixture.querySelector<HTMLInputElement | HTMLTextAreaElement>(`[data-field-key="${key()}"] input,[data-field-key="${key()}"] textarea`);
  check(!archiveInput || archiveInput.disabled || archiveInput.readOnly, "Archived field must not be editable");
  cases.push("running, transitional, error and archived instance restrictions");
  readOnly = false;
  const persisted = details;
  details = { ...details, settings_json: "{invalid" };
  await render(true);
  check(submit().disabled, "Malformed current settings must block saves");
  details = persisted;
  await render(true);
  await edit("Unsaved first-instance draft");
  details = { ...details, summary: { ...details.summary, id: "second-moria-instance" } };
  await render();
  check(control().value !== "Unsaved first-instance draft", "Draft must not leak across instances");
  cases.push("invalid settings and instance isolation");
  await act(async () => { await document.fonts.ready; await frame(); });
  check(document.documentElement.scrollWidth <= innerWidth + 1, "Editor must fit the viewport");
  check(errors.length === 0, "Browser and React reported errors");
  return { status: "passed", cases, save_boundary: "Synthetic onSaveSettings; no real world is modified or upgraded", browser_errors: errors };
}
Object.assign(window, { __reliabilityFixtureCleanup: async () => {
  await act(async () => { root.unmount(); });
  console.error = originalError;
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  return { browser_errors: errors, native_dialogs: 0 };
} });
void run().catch((error: unknown) => ({ status: "failed", cases, error: String(error instanceof Error ? error.stack : error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
