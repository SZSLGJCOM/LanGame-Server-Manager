import { act, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider } from "../../src/i18n";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { PlayerJoinAddressSelect } from "../../src/views/settings/PlayerJoinAddressSelect";
import { ConfigurationWorkspace } from "../../src/views/settings/ConfigurationWorkspace";
import { InstanceSettingsSaveProvider, useInstanceSettingsSaveCoordinator } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceSettingsSaveCoordinator } from "../../src/views/settings/instance-settings-save-coordinator";
import type { BindAddressCandidate, InstanceDetails, ModuleDetails, SaveInstanceSettingsOptions, UpdateInstanceInput } from "../../src/types";
import schemaJson from "../../../../modules/astroneer/schema.json?raw";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const zh = innerWidth < 1000;
localStorage.setItem("langame.locale", zh ? "zh-CN" : "en-US");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:16px;box-sizing:border-box;display:flex;flex-direction:column";
const root = createRoot(fixture);
const nonce = new URLSearchParams(location.search).get("nonce");
const checks: string[] = [], errors: string[] = [], copies: string[] = [], openedUrls: string[] = [];
const writes: { input: UpdateInstanceInput; expected: string | undefined }[] = [];
let epoch = 0, nativeDialogs = 0, failSave = false, scanFails = false;
let saveGate: Promise<void> | null = null, scanGate: Promise<void> | null = null;
let stored: InstanceDetails, details: InstanceDetails, candidates: BindAddressCandidate[] = [], detected: BindAddressCandidate[] = [];
let coordinator: InstanceSettingsSaveCoordinator;
let requestedField: string | undefined;
let generic = false;
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
window.alert = window.confirm = window.prompt = () => { nativeDialogs++; throw new Error("Unexpected native dialog"); };
Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async (text: string) => { copies.push(text); } } });
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args: Record<string, unknown>) => {
  if (command === "read_astroneer_save_catalog") return { instance_id: details.summary.id, entries: [] };
  if (command === "bind_address_candidates") {
    if (scanGate) await scanGate;
    if (scanFails) throw new Error("fixture adapter read failed");
    return detected;
  }
  if (command === "open_external_url") { openedUrls.push(String(args.url)); return; }
  throw new Error(`Unexpected native command: ${command}`);
} } });
const ports = [{ name: "game", port: 8777, protocol: "udp" }, { name: "console", port: 1234, protocol: "tcp" }];
const moduleDetails: ModuleDetails = { summary: { id: "astroneer", name: "ASTRONEER", version: "fixture",
  install_state: "Installed", supported_platforms: ["windows"], steam_app_id: 728470 }, schema_json: schemaJson,
  default_ports: ports, runtime: { port_roles: [{ role: "player", port_names: ["game"] }, { role: "service", port_names: ["console"] }] } };
const defaults = Object.fromEntries(Object.entries(JSON.parse(schemaJson).properties)
  .map(([key, value]) => [key, (value as { default?: unknown }).default]));
const radmin = (address: string): BindAddressCandidate => ({ address, kind: "overlay", family_name: "Radmin VPN", adapter_name: "Renamed virtual adapter" });
const firstRadmin = radmin("26.56.79.75"), secondRadmin = radmin("26.56.79.76");
const lan: BindAddressCandidate = { address: "192.168.1.2", kind: "lan", adapter_name: "Ethernet" };
const wrongVpn: BindAddressCandidate = { address: "100.80.1.2", kind: "overlay", family_name: "Tailscale", adapter_name: "Radmin VPN" };
const settings = () => JSON.parse(stored.settings_json) as Record<string, unknown>;
function normalized(value: Record<string, unknown>) { return JSON.stringify(Object.fromEntries(Object.entries(value).sort(([a], [b]) => a.localeCompare(b))), null, 2); }
function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(`${description}: ${fixture.textContent}`);
  checks.push(description);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const found = fixture.querySelector<T>(selector);
  if (!found) throw new Error(`Missing ${selector}: ${fixture.textContent}`);
  return found;
}
const customAddress = () => element<HTMLInputElement>(".player-join-address__custom");
const copy = () => element<HTMLButtonElement>(".player-join-address__copy");
const rescan = () => element<HTMLButtonElement>(".player-join-address__refresh");
const join = () => element<HTMLSelectElement>(".player-join-address__select");
async function onSave(input: UpdateInstanceInput, options?: SaveInstanceSettingsOptions) {
  writes.push({ input: structuredClone(input), expected: options?.expectedSettingsJson });
  if (saveGate) await saveGate;
  if (failSave) throw new Error("fixture write failed");
  if (options?.expectedSettingsJson !== stored.settings_json) throw new Error("fixture compare-and-swap conflict");
  stored = { ...stored, summary: { ...stored.summary, bind_ip: input.bind_ip }, ports: input.ports,
    settings_json: normalized(JSON.parse(input.settings_json)) };
  details = stored;
  draw();
  return stored;
}
function Workspace() {
  const [noticeTarget, setNoticeTarget] = useState<HTMLDivElement | null>(null);
  const currentCoordinator = useInstanceSettingsSaveCoordinator();
  useEffect(() => { coordinator = currentCoordinator; }, [currentCoordinator]);
  return <ActivityNoticeTarget.Provider value={{ element: noticeTarget, dismissLabel: zh ? "关闭" : "Close" }}>
    <main style={{ flex: 1, minHeight: 0 }}>
      {generic ? <PlayerJoinAddressSelect details={details} candidates={candidates} /> : <ConfigurationWorkspace details={details} moduleDetails={moduleDetails} moduleDetailsError={null}
        onRetryModuleDetails={() => {}} bindAddressCandidates={candidates} runtime={null} launchPlan={null} launchPlanError={null}
        requestedFieldKey={requestedField} onFieldNavigationHandled={() => { requestedField = undefined; draw(); }} onSave={onSave} />}
    </main>
    <footer className="shell-activity-bar"><span className="shell-activity-label">{zh ? "活动" : "Activity"}</span>
      <div className="shell-activity-notices" ref={setNoticeTarget} />
    </footer>
  </ActivityNoticeTarget.Provider>;
}
function draw() { root.render(<I18nProvider><InstanceSettingsSaveProvider key={epoch}><Workspace /></InstanceSettingsSaveProvider></I18nProvider>); }
async function pause(ms = 15) { await act(async () => { await new Promise((resolve) => setTimeout(resolve, ms)); }); }
async function until(predicate: () => boolean, label: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) { if (performance.now() > deadline) throw new Error(`${label}: ${fixture.textContent}`); await pause(); }
}
async function click(target: HTMLElement) { await act(async () => { target.click(); }); }
async function change(target: HTMLInputElement | HTMLSelectElement, value: string) {
  await act(async () => {
    const proto = target instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(target, value);
    target.dispatchEvent(new Event(target instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  });
}
async function flush() { await act(async () => { await coordinator.flush(details.summary.id); }); }
async function blocked(label: string) {
  let failure: unknown;
  await act(async () => { try { await coordinator.flush(details.summary.id); } catch (error) { failure = error; } });
  check(Boolean(failure), label);
}
async function startCase(patch: Record<string, unknown>, available: BindAddressCandidate[] = [firstRadmin], navigate = false) {
  epoch++; generic = false;
  failSave = false; scanFails = false; saveGate = null; scanGate = null;
  candidates = available; detected = available;
  const initial = { ...defaults, server_name: "Keep this room", ...patch };
  if (patch.connection_mode === undefined) delete initial.connection_mode;
  stored = { summary: { id: `astroneer-connection-${epoch}`, name: "ASTRONEER", module_id: "astroneer", status: "Stopped",
    active_process_count: 0, autostart: false, bind_ip: "0.0.0.0" }, settings_json: normalized(initial), ports,
    config_file_path: "", saves_path: "", backup_uses_declared_saves_path: false, auto_backup_on_stop: false,
    backup_retention_count: 3, active_run: null };
  details = stored;
  requestedField = navigate ? "public_ip" : undefined;
  localStorage.setItem(`langame.join-address.${details.summary.id}`, "192.168.1.2");
  await act(async () => { draw(); });
  await until(() => Boolean(fixture.querySelector(".player-join-address__select")), "Network editor must render");
}
async function nativeKey(key: string) {
  await act(async () => { const response = await fetch(`/__reliability_key/${nonce}/${key}`, { method: "POST" });
    if (!response.ok) throw new Error("Native key dispatch failed"); });
}
async function run() {
  await prepareBrowserLocaleCatalogs();
  await startCase({ connection_mode: "public", public_ip: "" }, [firstRadmin, lan, wrongVpn]);
  check(join().value === "" && !join().disabled && join().options[0].textContent!.includes(zh ? "自动选择" : "at startup"),
    "New instances use the shared address selector with automatic startup selection");
  check(!fixture.querySelector("#configuration-astroneer-connection-mode-input") && !fixture.querySelector(".astroneer-network-settings"),
    "No separate connection-method or advertised-address editor remains");
  check([firstRadmin.address, lan.address, wrongVpn.address].every((value) => [...join().options].some((option) => option.value === value)),
    "LAN, Radmin and another virtual LAN appear in one address selector");
  check(copy().disabled, "Unpublished choices never become copyable endpoints");
  const beforeOpen = writes.length;
  await flush();
  check(writes.length === beforeOpen, "Opening a fresh instance does not silently persist an adapter choice");
  check(!document.querySelector(".configuration-field-help-tooltip"), "Requirements remain closed by default");
  let releaseSave!: () => void;
  saveGate = new Promise((resolve) => { releaseSave = resolve; });
  const beforeRadmin = writes.length;
  await change(join(), firstRadmin.address);
  check(join().value === firstRadmin.address && copy().disabled, "Choosing Radmin blocks copying until persistence");
  let pendingFlush!: Promise<void>;
  await act(async () => { pendingFlush = coordinator.flush(details.summary.id); await Promise.resolve(); });
  await until(() => writes.length === beforeRadmin + 1, "Address draft must enter the save queue");
  const radminWrite = JSON.parse(writes.at(-1)!.input.settings_json);
  check(radminWrite.connection_mode === "radmin" && radminWrite.public_ip === firstRadmin.address,
    "One address choice atomically persists PublicIP and its derived startup source");
  check(radminWrite.server_name === "Keep this room" && writes.at(-1)!.input.bind_ip === "0.0.0.0"
    && writes.at(-1)!.input.ports.length === ports.length
    && ports.every((port) => writes.at(-1)!.input.ports.some((saved) => saved.name === port.name && saved.protocol === port.protocol && saved.port === port.port)),
    "The address change preserves room settings, listening and both ports");
  check(copy().disabled, "An in-flight save cannot copy the previous address");
  await act(async () => { releaseSave(); await pendingFlush; }); saveGate = null;
  await until(() => !copy().disabled, "Copy becomes usable after persistence is acknowledged");
  check(join().value === firstRadmin.address, "Remembered LAN copy preferences cannot override the advertised address");
  await click(copy());
  check(copies.at(-1) === `${firstRadmin.address}:8777`, "Copy matches PublicIP and the player UDP port");
  await change(join(), lan.address); await flush(); await click(copy());
  check(settings().public_ip === lan.address && settings().connection_mode === "manual" && copies.at(-1) === `${lan.address}:8777`,
    "LAN selection synchronizes and copies PublicIP without a second mode choice");
  await change(join(), wrongVpn.address); await flush();
  check(settings().public_ip === wrongVpn.address && settings().connection_mode === "manual", "CGNAT overlays are accepted without being mislabeled Radmin");

  const beforeLegacy = writes.length;
  await startCase({ public_ip: "1.1.1.1" }, [firstRadmin], true); await flush();
  check(join().value === "1.1.1.1" && writes.length === beforeLegacy && settings().connection_mode === undefined,
    "Opening a legacy external address preserves it without migration or rewriting");
  check(document.activeElement === join(), "Field navigation focuses the shared player-address control");
  await change(element<HTMLInputElement>('.configuration-search input'), "public_ip");
  const result = fixture.querySelector<HTMLButtonElement>('.configuration-search-results button');
  check(Boolean(result), "Search includes the address owned by the built-in network editor");
  await click(result!);
  check(document.activeElement === join(), "Search navigates to the same player-address control");

  await startCase({ connection_mode: "public", public_ip: "" }, [firstRadmin, secondRadmin]);
  await change(join(), secondRadmin.address); await flush();
  check(settings().public_ip === secondRadmin.address && settings().connection_mode === "radmin", "Multiple Radmin adapters can be chosen directly");
  await change(join(), ""); await flush();
  check(settings().public_ip === "" && settings().connection_mode === "public" && copy().disabled, "Automatic selection clears the old published address atomically");

  await startCase({ connection_mode: "public", public_ip: "" }, []);
  scanFails = true; await click(rescan());
  await until(() => fixture.textContent!.includes(zh ? "无法读取开服电脑" : "Could not read"), "Refresh failures must remain visible");
  check(join().value === "" && !rescan().disabled, "Refresh failure preserves the selection and allows retry");
  scanFails = false; detected = [firstRadmin, lan];
  const beforeRefresh = writes.length;
  await click(rescan());
  await until(() => [...join().options].some((option) => option.value === firstRadmin.address), "Refresh discovers the newly available adapter");
  await flush();
  check(writes.length === beforeRefresh && join().value === "", "Refresh never silently selects or persists a route");
  await change(join(), firstRadmin.address); await flush();
  check(settings().connection_mode === "radmin" && settings().public_ip === firstRadmin.address, "Refreshed adapters use the same persistent flow");

  await startCase({ connection_mode: "radmin", public_ip: "26.9.9.9" }, [firstRadmin], true);
  const beforeStale = writes.length;
  check(join().value === "26.9.9.9" && join().selectedOptions[0].textContent!.includes(zh ? "未检测到" : "not detected"),
    "An unavailable Radmin address stays selected with an explicit label");
  await click(rescan()); await flush();
  check(writes.length === beforeStale && settings().public_ip === "26.9.9.9", "Refresh never replaces an existing address");
  await change(join(), "__custom__"); await change(customAddress(), "1.0.0.1");
  await change(join(), "26.9.9.9"); await flush();
  check(settings().public_ip === "26.9.9.9" && settings().connection_mode === "radmin", "Reselecting a missing saved Radmin address cannot bypass its startup adapter check");

  await startCase({ connection_mode: "public", public_ip: "" }, []);
  let releaseScan!: () => void;
  scanGate = new Promise((resolve) => { releaseScan = resolve; }); detected = [firstRadmin];
  await click(rescan()); await change(join(), "__custom__"); await change(customAddress(), "1.0.0.1");
  await act(async () => { releaseScan(); await Promise.resolve(); }); scanGate = null; await flush();
  check(customAddress().value === "1.0.0.1" && settings().public_ip === "1.0.0.1", "A delayed refresh cannot overwrite a newer custom address");

  await startCase({ connection_mode: "radmin", public_ip: firstRadmin.address }, [firstRadmin], true);
  const beforeRunning = writes.length;
  for (const state of [{ status: "Running", count: 1 }, { status: "Starting", count: 0 }, { status: "Stopping", count: 0 }, { status: "Stopped", count: 1 }]) {
    await act(async () => { details = { ...stored, summary: { ...stored.summary, status: state.status, active_process_count: state.count } }; draw(); });
    check(join().disabled && rescan().disabled, `${state.status}/${state.count}: runtime ownership locks address changes and refresh`);
  }
  check(writes.length === beforeRunning, "Runtime updates do not create network writes");

  await startCase({ connection_mode: "public", public_ip: "" });
  const beforeCustom = writes.length;
  await change(join(), "__custom__"); await blocked("Empty custom drafts block saving and starting");
  await change(customAddress(), "127.0.0.1"); await blocked("Loopback input blocks saving and starting");
  check(writes.length === beforeCustom && copy().disabled, "Invalid custom input is never persisted or copied");
  await change(customAddress(), "1.0.0.1"); failSave = true;
  await blocked("A failed save propagates through the start barrier");
  check(customAddress().value === "1.0.0.1" && copy().disabled, "Failed saves retain the draft and keep copying locked");
  failSave = false;
  const retry = [...fixture.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent?.trim() === (zh ? "重试保存" : "Retry save"));
  check(Boolean(retry), "Failed saves offer the existing retry action");
  await click(retry!); await until(() => !copy().disabled, "Retry should acknowledge the save");
  check(settings().public_ip === "1.0.0.1", "Retry persists the retained address draft");

  await startCase({ connection_mode: "public", public_ip: "" });
  const beforeDiscovery = writes.length;
  await act(async () => {
    stored = { ...stored, settings_json: normalized({ ...settings(), connection_mode: "radmin", public_ip: firstRadmin.address }) };
    details = stored; draw();
  }); await flush();
  check(join().value === firstRadmin.address && !copy().disabled && !fixture.querySelector('.player-join-address__custom') && writes.length === beforeDiscovery,
    "Startup writeback is adopted by a clean editor as a selected address without rewriting or reopening");
  await change(join(), "__custom__"); await change(customAddress(), "1.0.0.1"); await flush();
  check(settings().public_ip === "1.0.0.1", "The next edit uses the startup writeback's CAS baseline");
  await change(customAddress(), "8.8.4.4");
  await act(async () => { stored = { ...stored, settings_json: normalized({ ...settings(), public_ip: "8.8.8.8" }) }; details = stored; draw(); });
  check(customAddress().value === "8.8.4.4" && copy().disabled, "External writes never erase an unsaved local address");
  await blocked("Dirty address edits retain conflict protection against external writes");

  await act(async () => {
    generic = true; epoch++; details = { ...stored, summary: { ...stored.summary, id: "generic-game", module_id: "valheim", status: "Running", active_process_count: 1 } };
    candidates = [lan, firstRadmin]; localStorage.setItem("langame.join-address.generic-game", lan.address); draw();
  });
  check(join().value === lan.address && !join().disabled, "Other games retain their remembered copy-address selection while running");
  const beforeGeneric = writes.length;
  await change(join(), firstRadmin.address); await click(copy());
  check(copies.at(-1) === `${firstRadmin.address}:8777`, "Other games copy the chosen route");
  check(localStorage.getItem("langame.join-address.generic-game") === firstRadmin.address, "Other games remember the chosen route");
  check(writes.length === beforeGeneric, "Changing another game's copy route does not write game configuration");

  await startCase({ connection_mode: "radmin", public_ip: firstRadmin.address }, [firstRadmin, lan, wrongVpn], true);
  await nativeKey("Escape");
  const help = element(".astroneer-network-requirements");
  await act(async () => { help.focus(); });
  await until(() => Boolean(document.querySelector(".configuration-field-help-tooltip")), "Keyboard focus opens network help");
  const bubble = document.querySelector<HTMLElement>(".configuration-field-help-tooltip")!;
  check(bubble.textContent!.includes(zh ? "同一 Radmin 网络" : "same Radmin network") && bubble.textContent!.includes(zh ? "官方在线服务" : "official online services"),
    "Help retains shared-network and official-online-service requirements");
  await nativeKey("Escape"); await until(() => !document.querySelector(".configuration-field-help-tooltip"), "Escape dismisses help");
  await nativeKey("Tab");
  check(document.activeElement === join(), "Native Tab reaches the single labeled player-address control");
  check(document.documentElement.scrollWidth <= innerWidth, "No horizontal document overflow");
  const panel = element(".player-join-address");
  check(panel.scrollWidth <= panel.clientWidth + 1, "The selector and actions fit the narrow workspace");
  check(help.getBoundingClientRect().height <= 24, "Requirements retain a small inline entry");
  check(errors.length === 0 && nativeDialogs === 0, "No React/browser errors or native dialogs");
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  return { status: "passed", checks, writes: writes.length, copies, browser_errors: errors };
}
Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  await act(async () => { root.unmount(); });
  check(!document.querySelector(".configuration-field-help-tooltip"), "Unmount removes the network help portal");
  return { browser_errors: errors, native_dialogs: nativeDialogs };
} });
void run().catch((error) => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  return { status: "failed", error: String(error?.stack ?? error), checks, browser_errors: errors };
})
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
