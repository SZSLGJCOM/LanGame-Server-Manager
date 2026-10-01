import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, useState } from "react";
import { createRoot } from "react-dom/client";
import schema from "../../../../modules/satisfactory/schema.json";
import storageContract from "../../../../modules/satisfactory/config-fixtures/2026-09-30-native_options_missing_settings.json";
import { I18nProvider } from "../../src/i18n";
import { ConfigurationWorkspace } from "../../src/views/settings/ConfigurationWorkspace";
import { InstanceSettingsSaveProvider, useInstanceSettingsSaveCoordinator } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceDetails, ModuleDetails, UpdateInstanceInput } from "../../src/types";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture");
if (!fixture) throw new Error("Fixture root is missing");
const root = createRoot(fixture);
const checks: string[] = [];
const errors: string[] = [];
const calls: UpdateInstanceInput[] = [];
const keys = ["auto_pause_when_empty", "network_quality", "send_gameplay_data"];
const sections = ["world", "network", "advanced"];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: (command: string) => {
  throw new Error(`Unexpected native command: ${command}`);
} } });

const moduleDetails: ModuleDetails = {
  summary: { id: "satisfactory", name: "Satisfactory", version: "1", install_state: "Installed", supported_platforms: ["windows"] },
  schema_json: JSON.stringify(schema), default_ports: [], runtime: {}
};
let instance: InstanceDetails = {
  summary: { id: "native-options", module_id: "satisfactory", name: "Native options", status: "Stopped", active_process_count: 0,
    autostart: false, bind_ip: "0.0.0.0" },
  settings_json: "{}", ports: [], config_file_path: "", saves_path: "", backup_uses_declared_saves_path: false,
  auto_backup_on_stop: false, backup_retention_count: 3
};
// Share the missing-override input with the storage acceptance case.
let saved = JSON.stringify({ ...storageContract.settings, max_players: 8, future_setting: "retained" });
type StorageReadbacks = Record<"created" | "unmanaged" | "managed" | "released", InstanceDetails>;
let readbacks: StorageReadbacks | null = null;
let flush: () => Promise<void>;
function Workspace() {
  const coordinator = useInstanceSettingsSaveCoordinator();
  const [details, setDetails] = useState({ ...instance, settings_json: saved });
  flush = () => coordinator.flush(instance.summary.id);
  return <ConfigurationWorkspace details={details} moduleDetails={moduleDetails} moduleDetailsError={null}
    onRetryModuleDetails={() => { throw new Error("Unexpected descriptor retry"); }} bindAddressCandidates={[]}
    runtime={null} launchPlan={null} launchPlanError={null} onSave={async (input, options) => {
      if (input.id !== instance.summary.id || !sameSettings(options?.expectedSettingsJson, saved)) throw new Error("Unexpected save baseline");
      calls.push(input);
      saved = input.settings_json;
      const readback = { ...instance, settings_json: saved };
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
  if (!result) throw new Error(`Missing control: ${selector}`);
  return result;
}
async function draw(key: string) {
  await act(async () => { root.render(<I18nProvider key={key}><InstanceSettingsSaveProvider>
    <main className="server-detail-panel" style={{ height: "calc(100vh - 32px)", margin: 16 }}><Workspace /></main>
  </InstanceSettingsSaveProvider></I18nProvider>); });
  const deadline = performance.now() + 5000;
  while (!fixture?.querySelector(".configuration-workspace__body")) {
    if (performance.now() >= deadline) throw new Error(`Workspace did not load: ${fixture?.textContent}`);
    await act(async () => { await new Promise<void>((resolve) => requestAnimationFrame(() => resolve())); });
  }
}
async function section(id: string) {
  if (id === "advanced" && !fixture?.querySelector('[data-configuration-section-id="advanced"]')) {
    await act(async () => { element<HTMLButtonElement>('[data-configuration-section-id="runtime"] > button').click(); });
  }
  await act(async () => { element<HTMLButtonElement>(`[data-configuration-section-id="${id}"] > button`).click(); });
}
function control(key: string) { return element<HTMLSelectElement>(`[data-field-key="${key}"] select`); }
async function change(key: string, value: string) {
  const select = control(key);
  check(!select.disabled && select.getClientRects().length > 0, `Editable native option ${key}`);
  await act(async () => { select.value = value; select.dispatchEvent(new Event("change", { bubbles: true })); });
}
async function save() { await act(async () => { await flush(); }); }
function absent() { return keys.every((key) => !Object.hasOwn(JSON.parse(saved), key)); }
function sameSettings(left: string | undefined, right: string) {
  if (left === undefined) return false;
  const first = JSON.parse(left);
  const second = JSON.parse(right);
  return JSON.stringify(Object.keys(first).sort()) === JSON.stringify(Object.keys(second).sort()) &&
    Object.keys(first).every((key) => JSON.stringify(first[key]) === JSON.stringify(second[key]));
}
async function verifyUnmanaged(label: string) {
  for (let index = 0; index < keys.length; index++) {
    await section(sections[index]);
    const select = control(keys[index]);
    check(select.value === "" && select.selectedOptions[0]?.textContent === label, `Unmanaged ${keys[index]} shows no inferred value`);
    check(select.options.length === (keys[index] === "network_quality" ? 5 : 3), `All explicit choices remain available for ${keys[index]}`);
    check(!fixture?.querySelector(`[data-field-key="${keys[index]}"] input[type="checkbox"]`), `Unmanaged ${keys[index]} is not presented as a binary fact`);
  }
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const response = await fetch("/__satisfactory_readbacks");
  if (!response.ok) throw new Error("Readback fixture transport failed");
  readbacks = await response.json();
  if (readbacks) {
    instance = readbacks.created;
    moduleDetails.default_ports = instance.ports;
    check(keys.every((key) => !Object.hasOwn(JSON.parse(instance.settings_json), key)), "Actual storage create/read leaves native overrides absent");
  }
  for (const [locale, label] of [
    ["en-US", "Keep native setting (current value not read)"], ["zh-CN", "保持原生设置（当前值未读取）"]
  ]) {
    localStorage.setItem("langame.locale", locale);
    saved = readbacks?.created.settings_json ?? JSON.stringify({ ...storageContract.settings, max_players: 8, future_setting: "retained" });
    const preserved = JSON.parse(saved);
    const editedPlayers = readbacks ? JSON.parse(readbacks.unmanaged.settings_json).max_players : 9;
    const before = calls.length;
    await draw(`${locale}-initial`);
    await verifyUnmanaged(label);
    await save();
    check(calls.length === before && absent(), `${locale}: opening and navigation do not create overrides`);
    await section("room");
    const maxPlayers = element<HTMLInputElement>('[data-field-key="max_players"] input');
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
    if (!setter) throw new Error("Input setter is unavailable");
    await act(async () => { setter.call(maxPlayers, String(editedPlayers)); maxPlayers.dispatchEvent(new Event("input", { bubbles: true })); });
    await save();
    check(calls.length === before + 1 && JSON.parse(saved).max_players === editedPlayers && absent(), `${locale}: saving another field leaves all native options unowned`);
    verifyBackendReadback("unmanaged", locale);
    for (let index = 0; index < keys.length; index++) {
      await section(sections[index]);
      await change(keys[index], index === 1 ? "2" : "false");
    }
    await save();
    check(JSON.parse(saved).auto_pause_when_empty === false && JSON.parse(saved).network_quality === 2 &&
      JSON.parse(saved).send_gameplay_data === false, `${locale}: explicit false/2/false persist with their native types`);
    verifyBackendReadback("managed", locale);
    await draw(`${locale}-managed-readback`);
    for (let index = 0; index < keys.length; index++) {
      await section(sections[index]);
      check(control(keys[index]).value === (index === 1 ? "2" : "false"), `${locale}: explicit ${keys[index]} survives reopening`);
      await change(keys[index], "");
    }
    await save();
    check(absent() && JSON.parse(saved).future_setting === preserved.future_setting && JSON.parse(saved).max_players === editedPlayers,
      `${locale}: releasing management deletes only the three overrides`);
    verifyBackendReadback("released", locale);
    await draw(`${locale}-unmanaged-readback`);
    await verifyUnmanaged(label);
    await save();
    check(absent(), `${locale}: reopening never restores defaults after releasing management`);
  }
  await act(async () => { root.unmount(); });
  check(errors.length === 0, "The browser and React report no errors");
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  return { status: "passed", input_source: readbacks ? "storage-readback" : "storage-contract", checks, browser_errors: errors };
}
function verifyBackendReadback(stage: "unmanaged" | "managed" | "released", locale: string) {
  if (!readbacks) return;
  const persisted = JSON.parse(saved);
  const actual = JSON.parse(readbacks[stage].settings_json);
  check(JSON.stringify(Object.keys(persisted).sort()) === JSON.stringify(Object.keys(actual).sort()),
    `${locale}: ${stage} save has the same ownership keys as the real storage result`);
  for (const key of Object.keys(actual)) {
    check(JSON.stringify(persisted[key]) === JSON.stringify(actual[key]), `${locale}: ${stage} value matches storage readback for ${key}`);
  }
  saved = readbacks[stage].settings_json;
}
void run().catch((error) => ({ status: "failed", error: String(error?.stack ?? error), checks, browser_errors: errors }))
  .then((result) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(result) }));
