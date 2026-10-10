import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import romesteadSchema from "../../../../modules/romestead/schema.json";
import astroneerSchema from "../../../../modules/astroneer/schema.json";
import { I18nProvider } from "../../src/i18n";
import { ConfigurationWorkspace } from "../../src/views/settings/ConfigurationWorkspace";
import { AstroneerSaveSelect } from "../../src/views/settings/AstroneerSaveSelect";
import { RomesteadSleepThresholdField } from "../../src/views/settings/RomesteadSleepThresholdField";
import { InstanceSettingsSaveProvider, useInstanceSettingsSaveCoordinator } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { AstroneerSaveCatalog } from "../../src/astroneer-saves";
import type { InstanceDetails, ModuleDetails, UpdateInstanceInput } from "../../src/types";
import "../../src/app.css";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture");
if (!fixture) throw new Error("Fixture root is missing");
const root = createRoot(fixture);
const nonce = new URLSearchParams(location.search).get("nonce");
const checks: string[] = [];
const errors: string[] = [];
const overflowViolations: string[] = [];
const nativeCalls: Array<{ command: string; instanceId: unknown }> = [];
const writes: UpdateInstanceInput[] = [];
const locales = ["en-US", "zh-CN"] as const;
let saved = "{}";
let activeId = "";
let flush: () => Promise<void>;
let mode: "ready" | "empty" | "error" | "delayed" = "ready";
let releaseOld: (() => void) | undefined;
let directPatches: Readonly<Record<string, unknown>>[] = [];
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
window.alert = window.confirm = window.prompt = () => { throw new Error("Unexpected native dialog"); };

function catalog(instanceId: string, names = ["SAVE_1", "Custom Expedition"]): AstroneerSaveCatalog {
  return { instance_id: instanceId, configured_name: "SAVE_1", entries: names.map((name) => ({
    descriptive_name: name, latest_saved_at: "2026.10.09-12.00.00", versions: 2, total_bytes: 8192
  })) };
}
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args: Record<string, unknown> = {}) => {
  nativeCalls.push({ command, instanceId: args.instanceId });
  if (command !== "read_astroneer_save_catalog" || typeof args.instanceId !== "string") {
    throw new Error(`Unexpected native command: ${command}`);
  }
  if (mode === "error") throw new Error("Synthetic catalog read failure");
  if (mode === "empty") return catalog(args.instanceId, []);
  if (mode === "delayed" && args.instanceId === "stale-old") {
    return new Promise<AstroneerSaveCatalog>((resolve) => { releaseOld = () => resolve(catalog("stale-old", ["OLD_SLOT"])); });
  }
  return catalog(args.instanceId, args.instanceId === "stale-new" ? ["NEW_SLOT"] : undefined);
} } });

function descriptor(moduleId: "romestead" | "astroneer"): ModuleDetails {
  return { summary: { id: moduleId, name: moduleId, version: "fixture", install_state: "Installed", supported_platforms: ["windows"] },
    schema_json: JSON.stringify(moduleId === "romestead" ? romesteadSchema : astroneerSchema), default_ports: [], runtime: {} };
}
function instance(moduleId: "romestead" | "astroneer", id: string): InstanceDetails {
  return { summary: { id, module_id: moduleId, name: "Retained server", status: "Stopped", active_process_count: 0,
    autostart: false, bind_ip: "0.0.0.0" }, settings_json: saved, ports: [], config_file_path: "", saves_path: "",
    backup_uses_declared_saves_path: false, auto_backup_on_stop: false, backup_retention_count: 3, active_run: null };
}
function Workspace({ moduleId, archived }: { moduleId: "romestead" | "astroneer"; archived: boolean }) {
  const [details, setDetails] = useState(() => instance(moduleId, activeId));
  const coordinator = useInstanceSettingsSaveCoordinator();
  flush = () => coordinator.flush(details.summary.id);
  return <ConfigurationWorkspace details={details} moduleDetails={descriptor(moduleId)} moduleDetailsError={null}
    archive={archived ? { archive_id: "retained-archive", instance: details,
      maintenance: { autostart: false, auto_backup_on_stop: false, backup_retention_count: 3, crash_restart_limit: null, runtime_mode: null },
      runs: { entries: [], total: 0, truncated: false },
      log: { relative_path: null, text: "", truncated: false, issues: [] },
      backups: { entries: [], issues: [], truncated: false } } : undefined}
    onRetryModuleDetails={() => { throw new Error("Unexpected descriptor retry"); }} bindAddressCandidates={[]}
    runtime={null} launchPlan={null} launchPlanError={null} onSave={async (input) => {
      check(input.id === activeId, "settings writes target the active instance");
      writes.push(structuredClone(input));
      saved = input.settings_json;
      const readback = { ...details, settings_json: saved };
      setDetails(readback);
      return readback;
    }} />;
}
function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
  checks.push(description);
}
function element<T extends Element>(selector: string): T {
  const result = fixture?.querySelector<T>(selector);
  if (!result) throw new Error(`Missing control: ${selector}; ${fixture?.textContent}`);
  return result;
}
async function until(predicate: () => boolean) {
  const end = performance.now() + 5000;
  while (!predicate()) {
    if (performance.now() > end) throw new Error(`UI did not settle: ${fixture?.textContent}`);
    await act(frame);
  }
}
async function draw(moduleId: "romestead" | "astroneer", key: string, archived = false) {
  await act(async () => { root.render(<I18nProvider key={key}><InstanceSettingsSaveProvider>
    <main className="server-detail-panel" style={{ height: "calc(100vh - 32px)", margin: 16 }}>
      <Workspace moduleId={moduleId} archived={archived} />
    </main>
  </InstanceSettingsSaveProvider></I18nProvider>); });
  await until(() => Boolean(fixture?.querySelector(".configuration-workspace__body")));
}
async function section(id: string) {
  const toggle = element<HTMLButtonElement>(".configuration-workspace__navigation-toggle");
  if (toggle.getClientRects().length && toggle.getAttribute("aria-expanded") !== "true") await act(async () => { toggle.click(); });
  let button = fixture?.querySelector<HTMLButtonElement>(`[data-configuration-section-id="${id}"] > button`);
  if (!button && id === "performance") {
    await act(async () => { element<HTMLButtonElement>('[data-configuration-section-id="runtime"] > button').click(); });
    button = fixture?.querySelector<HTMLButtonElement>(`[data-configuration-section-id="${id}"] > button`);
  }
  if (!button) throw new Error(`Missing navigation: ${id}`);
  await act(async () => { button.click(); await frame(); });
}
async function text(input: HTMLInputElement, value: string) {
  check(!input.disabled, `editable input ${input.id}`);
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
async function choose(select: HTMLSelectElement, value: string) {
  check(!select.disabled, `editable select ${select.id}`);
  await act(async () => { select.value = value; select.dispatchEvent(new Event("change", { bubbles: true })); });
}
async function save() { await act(async () => { await flush(); }); }
function settings(): Record<string, unknown> { return JSON.parse(saved); }
function sleep() { return element<HTMLInputElement>('[data-field-key="sleep_threshold_ms"] input[type="checkbox"]'); }
function threshold() { return element<HTMLInputElement>('[data-field-key="sleep_threshold_ms"] input[type="text"]'); }
function slot() { return element<HTMLSelectElement>('[data-field-key="active_save_file_name"] select'); }
function layout(locale: string) {
  const main = element<HTMLElement>(".configuration-workspace__main");
  const bounds = main.getBoundingClientRect();
  for (const field of fixture!.querySelectorAll<HTMLElement>(".configuration-field[data-field-key]")) {
    if (!field.getClientRects().length) continue;
    const box = field.getBoundingClientRect();
    if (box.left < bounds.left - 1 || box.right > bounds.right + 1 || field.scrollWidth > field.clientWidth + 1) {
      overflowViolations.push(`${locale}: ${field.dataset.fieldKey} escapes the workspace`);
    }
  }
  check(main.scrollWidth <= main.clientWidth + 1 && !overflowViolations.length, `${locale}: layout fits ${innerWidth}px`);
  const ids = [...fixture!.querySelectorAll<HTMLElement>("[id]")].map((control) => control.id);
  check(ids.length === new Set(ids).size, `${locale}: focus IDs stay unique`);
}
async function search(key: string, query: string) {
  const toggle = element<HTMLButtonElement>(".configuration-workspace__navigation-toggle");
  if (toggle.getClientRects().length && toggle.getAttribute("aria-expanded") !== "true") await act(async () => { toggle.click(); });
  const input = element<HTMLInputElement>('.configuration-search input[type="search"]');
  await text(input, query);
  await until(() => Boolean(fixture?.querySelector(".configuration-search-results button")));
  await act(async () => { element<HTMLButtonElement>(".configuration-search-results button").click(); await frame(); });
  const target = document.getElementById(`configuration-${key === "sleep_threshold_ms" ? "romestead" : "astroneer"}-${key.replaceAll("_", "-")}-input`);
  check(document.activeElement === target, `search focuses the real ${key} control`);
}
async function romestead(locale: string) {
  activeId = `romestead-${locale}`;
  saved = JSON.stringify({ auto_start_world_name: "RetainedWorld", password: "fixture-password", max_players: 8, future_setting: { keep: true } });
  await draw("romestead", `${locale}-romestead`);
  await section("world");
  const size = element<HTMLSelectElement>('[data-field-key="auto_create_world_size"] select');
  check(JSON.stringify([...size.options].map((entry) => entry.textContent)) === JSON.stringify(locale === "zh-CN" ? ["小型", "标准", "大型"] : ["Small", "Standard", "Large"]), `${locale}: sizes are localized native choices`);
  for (const value of ["0", "2", "1"]) {
    await choose(size, value);
    await save();
    check(settings().auto_create_world_size === Number(value), `${locale}: world size saves native integer ${value}`);
  }
  layout(locale);
  await section("performance");
  check(sleep().checked && threshold().value === "10", `${locale}: sleep starts with native default 10`);
  check(Boolean(sleep().labels?.length) && Boolean(threshold().labels?.length), `${locale}: sleep controls have associated labels`);
  await text(threshold(), "13.875");
  await save();
  check(settings().sleep_threshold_ms === 13.875, `${locale}: fractional threshold is saved without rounding`);
  await act(async () => { sleep().click(); });
  await save();
  check(settings().sleep_threshold_ms === -1 && threshold().value === "13.875" && threshold().disabled, `${locale}: disabling saves -1 and retains the threshold`);
  await section("world");
  await section("performance");
  await act(async () => { sleep().click(); });
  await save();
  check(settings().sleep_threshold_ms === 13.875, `${locale}: enabling after navigation restores the threshold`);
  const writeCount = writes.length;
  await text(threshold(), "16.5001");
  check(threshold().getAttribute("aria-invalid") === "true", `${locale}: invalid threshold is marked accessibly`);
  let blocked = false;
  try { await save(); } catch (error) {
    if (error instanceof Error && error.name === "InstanceSettingsDraftInvalidError") blocked = true;
    else throw error;
  }
  check(blocked && writes.length === writeCount && settings().sleep_threshold_ms === 13.875, `${locale}: invalid threshold cannot reach persistence`);
  await text(threshold(), "1.00123456789");
  await save();
  check(settings().sleep_threshold_ms === 1.00123456789, `${locale}: arbitrary legal decimals remain supported`);
  check(settings().auto_start_world_name === "RetainedWorld" && settings().password === "fixture-password" && settings().max_players === 8 && JSON.stringify(settings().future_setting) === '{"keep":true}', `${locale}: edits preserve unrelated settings`);
  layout(locale);
  await section("room");
  await search("sleep_threshold_ms", "SleepThresholdMs");
  await draw("romestead", `${locale}-romestead-readback`);
  await section("performance");
  check(threshold().value === "1.00123456789", `${locale}: sleep readback survives reopening`);
  await act(async () => { root.render(<I18nProvider key={`${locale}-sleep-disabled`}>
    <main className="server-detail-panel configuration-workspace" style={{ margin: 16, padding: 24 }}>
      <RomesteadSleepThresholdField sectionId="performance" details={instance("romestead", activeId)} moduleDetails={descriptor("romestead")}
        settings={settings()} disabled onPatch={(patch) => { directPatches.push(patch); }} />
    </main>
  </I18nProvider>); });
  check([...fixture!.querySelectorAll<HTMLInputElement>("input")].every((input) => input.disabled), `${locale}: all disabled sleep controls are inert`);
  await draw("romestead", `${locale}-romestead-archive`, true);
  await section("performance");
  check(!fixture!.querySelector('[data-field-key="sleep_threshold_ms"] input[type="checkbox"]') &&
    element<HTMLInputElement | HTMLTextAreaElement>('[data-field-key="sleep_threshold_ms"] input,[data-field-key="sleep_threshold_ms"] textarea').readOnly,
    `${locale}: archived sleep settings use a read-only retained value`);
}
async function astroneer(locale: string) {
  activeId = `astroneer-${locale}`;
  mode = "ready";
  saved = JSON.stringify({ server_name: "Retained Astroneer", max_players: 8, active_save_file_name: "Archived Custom", future_setting: { keep: true } });
  const baseline = structuredClone(settings());
  const beforeWrites = writes.length;
  await draw("astroneer", `${locale}-astroneer`);
  await until(() => !slot().disabled);
  check(slot().value === "Archived Custom" && slot().selectedOptions[0].disabled, `${locale}: unmatched configured name remains selected`);
  check(writes.length === beforeWrites, `${locale}: inventory reads never change configured settings`);
  check([...slot().options].map((entry) => entry.value).includes("Custom Expedition"), `${locale}: native descriptive slots populate the selector`);
  check(Boolean(slot().labels?.length), `${locale}: startup save has a visible label`);
  check(slot().labels?.[0].textContent?.trim() === (locale === "zh-CN" ? "启动存档" : "Startup Save"),
    `${locale}: startup save uses the correct localized copy`);
  check(!fixture!.querySelector('[data-field-key="active_save_file_name"] input'), `${locale}: save selection no longer requires manual typing`);
  await choose(slot(), "Custom Expedition");
  await save();
  check(settings().active_save_file_name === "Custom Expedition", `${locale}: native descriptive name persists`);
  check(JSON.stringify(settings()) === JSON.stringify({ ...baseline, active_save_file_name: "Custom Expedition" }), `${locale}: selection patches only the original save name`);
  layout(locale);
  await section("world");
  check(!element<HTMLElement>('[data-field-key="active_save_file_name"]').getClientRects().length,
    `${locale}: startup selection stays out of world and maintenance controls`);
  await search("active_save_file_name", "ActiveSaveFileDescriptiveName");
  await until(() => !slot().disabled);
  check(slot().value === "Custom Expedition", `${locale}: returning to Room preserves the chosen slot`);
  await draw("astroneer", `${locale}-astroneer-readback`);
  await until(() => !slot().disabled);
  check(slot().value === "Custom Expedition", `${locale}: native selection survives reopening`);
  mode = "error";
  const currentWrites = writes.length;
  await act(async () => { element<HTMLButtonElement>('[data-field-key="active_save_file_name"] button').click(); });
  await until(() => Boolean(fixture?.querySelector('[data-field-key="active_save_file_name"] [role="alert"]')));
  check(slot().disabled && slot().value === "Custom Expedition", `${locale}: real read errors retain the current slot`);
  check(writes.length === currentWrites, `${locale}: errors never produce empty success or settings writes`);
  mode = "ready";
  await act(async () => { element<HTMLButtonElement>('[data-field-key="active_save_file_name"] button').click(); });
  await until(() => !slot().disabled);
  check(!fixture!.querySelector('[data-field-key="active_save_file_name"] [role="alert"]'), `${locale}: retry recovers catalog reading`);
  mode = "empty";
  saved = JSON.stringify({ server_name: "Fresh Astroneer", max_players: 8, future_setting: { keep: true } });
  const fresh = saved;
  await draw("astroneer", `${locale}-astroneer-empty`);
  await until(() => Boolean(fixture?.querySelector('[data-field-key="active_save_file_name"] [role="status"]')) && !fixture!.querySelector('[data-field-key="active_save_file_name"] [aria-busy="true"]'));
  check(slot().disabled && slot().value === "SAVE_1", `${locale}: fresh empty inventory retains native SAVE_1`);
  await save();
  check(saved === fresh, `${locale}: empty inventory adds no synthetic selection`);
  layout(locale);
  saved = JSON.stringify({ ...settings(), active_save_file_name: "Retained Archive" });
  const reads = nativeCalls.length;
  await draw("astroneer", `${locale}-astroneer-archive`, true);
  check(!fixture!.querySelector('[data-field-key="active_save_file_name"] select') &&
    element<HTMLInputElement | HTMLTextAreaElement>('[data-field-key="active_save_file_name"] input,[data-field-key="active_save_file_name"] textarea').readOnly,
    `${locale}: archived save selection uses a read-only retained value`);
  check(nativeCalls.length === reads, `${locale}: archives never query live native saves`);
}
async function direct(locale: string, id: string, disabled = false) {
  await act(async () => { root.render(<I18nProvider key={`${locale}-direct`}>
    <main className="server-detail-panel configuration-workspace" style={{ margin: 16, padding: 24 }}>
      <AstroneerSaveSelect sectionId="room" details={instance("astroneer", id)} moduleDetails={descriptor("astroneer")}
        settings={{ active_save_file_name: "SAVE_1" }} disabled={disabled} onPatch={(patch) => { directPatches.push(patch); }} />
    </main>
  </I18nProvider>); });
}
async function run() {
  await prepareBrowserLocaleCatalogs();
  for (const locale of locales) {
    localStorage.setItem("langame.locale", locale);
    document.documentElement.lang = locale;
    await romestead(locale);
    await astroneer(locale);
    mode = "ready";
    await direct(locale, "disabled-catalog", true);
    await until(() => Boolean(fixture?.querySelector('[data-field-key="active_save_file_name"] button')));
    check(slot().disabled && element<HTMLButtonElement>('[data-field-key="active_save_file_name"] button').disabled, `${locale}: disabled catalog controls are inert`);
    const patchCount = directPatches.length;
    await act(async () => { slot().value = "Custom Expedition"; slot().dispatchEvent(new Event("change", { bubbles: true })); });
    check(directPatches.length === patchCount, `${locale}: disabled synthetic events cannot patch settings`);
    mode = "delayed";
    await direct(locale, "stale-old");
    await until(() => Boolean(releaseOld));
    check(slot().disabled, `${locale}: pending native catalog disables selection`);
    await direct(locale, "stale-new");
    await until(() => [...slot().options].some((entry) => entry.value === "NEW_SLOT"));
    await act(async () => { releaseOld!(); await frame(); });
    check([...slot().options].some((entry) => entry.value === "NEW_SLOT") && ![...slot().options].some((entry) => entry.value === "OLD_SLOT"), `${locale}: stale native results cannot populate another instance`);
    releaseOld = undefined;
  }
  check(nativeCalls.every((call) => call.command === "read_astroneer_save_catalog"), "save selection invokes no load, backup, port, player or maintenance actions");
  check(errors.length === 0, "React and browser report no errors");
  mode = "ready";
  saved = JSON.stringify({ server_name: "Retained Astroneer", max_players: 8, active_save_file_name: "Custom Expedition", future_setting: { keep: true } });
  await draw("astroneer", "zh-CN-final-preview");
  await until(() => !slot().disabled);
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  return { status: "passed", locales, checks, browser_errors: errors, overflow_violations: overflowViolations };
}
Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  await act(async () => { root.unmount(); await frame(); });
  return { browser_errors: errors, native_dialogs: 0 };
} });
void run().catch((error) => ({ status: "failed", error: String(error?.stack ?? error), checks, browser_errors: errors,
  overflow_violations: overflowViolations }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
