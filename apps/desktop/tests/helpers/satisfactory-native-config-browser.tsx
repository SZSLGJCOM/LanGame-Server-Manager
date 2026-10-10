import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import schema from "../../../../modules/satisfactory/schema.json";
import { I18nProvider } from "../../src/i18n";
import { EN_US_SATISFACTORY_MESSAGES } from "../../src/i18n/games/satisfactory.en";
import { ZH_CN_SATISFACTORY_MESSAGES } from "../../src/i18n/games/satisfactory.zh-cn";
import { SATISFACTORY_RULES, SATISFACTORY_STARTING_LOCATIONS, type SatisfactoryWorldSnapshot } from "../../src/satisfactory-world-settings";
import { ConfigurationWorkspace } from "../../src/views/settings/ConfigurationWorkspace";
import { buildConfigurationFieldIds } from "../../src/views/settings/ConfigurationField";
import { InstanceSettingsSaveProvider, useInstanceSettingsSaveCoordinator } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceDetails, ModuleDetails, UpdateInstanceInput } from "../../src/types";
import "../../src/app.css";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture");
if (!fixture) throw new Error("Fixture root is missing");
const root = createRoot(fixture);
const nonce = new URLSearchParams(location.search).get("nonce");
const checks: string[] = [], errors: string[] = [], overflowViolations: string[] = [];
const calls: Array<{ command: string; args: Record<string, unknown> }> = [];
const settingsWrites: UpdateInstanceInput[] = [];
const locales = ["en-US", "zh-CN"] as const;
const prefix = "satisfactory.settings.native";
const noPower = "FG.GameRules.NoPower", noFuel = "FG.GameRules.NoFuelCost";
const energy = "FG.GameMode.EnergyCostMultiplier", purity = "FG.GameMode.NodePuritySettings", seed = "FG.GameMode.NodeRandomizationSeed";
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
let locale: typeof locales[number] = "en-US";
let current: SatisfactoryWorldSnapshot;
let details: InstanceDetails;
let saved = "{}";
let revision = 0;
let failNext = "";
let deferredCredential = false;
let releaseCredential: (() => void) | undefined;
let deferNextRule = false;
let releaseRule: (() => void) | undefined;
let pending: { session: string; creative: boolean; settings: Record<string, string>; reads: number } | null = null;
let flush: () => Promise<void>;
let flushBeforeStop: () => Promise<void>;
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
window.alert = window.confirm = window.prompt = () => { throw new Error("Unexpected native dialog"); };

function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
  checks.push(description);
}
function record(value: unknown): value is Record<string, unknown> { return value !== null && typeof value === "object" && !Array.isArray(value); }
function strings(value: unknown): Record<string, string> {
  if (!record(value) || Object.values(value).some((entry) => typeof entry !== "string")) throw new Error("Malformed native settings map");
  return Object.fromEntries(Object.entries(value).map(([key, entry]) => [key, String(entry)]));
}
function bump() { current.revision = `native-revision-${++revision}`; }
function snapshot(status: SatisfactoryWorldSnapshot["connection_status"] = "ready"): SatisfactoryWorldSnapshot {
  return { instance_id: details.summary.id, connection_status: status, revision: `native-revision-${++revision}`,
    server_name: "Retained Factory", active_session_name: "Retained", auto_load_session_name: "Retained",
    is_game_running: true, connected_players: 0, creative_mode_enabled: false,
    advanced_game_settings: { "FG.Future.Unknown": "retained" }, server_options: {
      "FG.DSAutoPause": "False", "FG.WeatherPreset": "2", "FG.NetworkQuality": "3", "FG.SendGameplayData": "False"
    }, pending_server_options: { "FG.WeatherPreset": "4" },
    sessions: ["Retained", "Other"].map((name) => ({ session_name: name, saves: [{ save_name: `${name}.auto`,
      save_date_time: "2026.10.09-12.00.00", play_duration_seconds: 7200, is_creative_mode_enabled: name === "Other" }] })),
    rule_definitions: [...SATISFACTORY_RULES], starting_locations: [...SATISFACTORY_STARTING_LOCATIONS] };
}
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args: Record<string, unknown> = {}) => {
  calls.push({ command, args: structuredClone(args) });
  if (command === failNext) { failNext = ""; throw new Error("Synthetic native mutation failure"); }
  if (command === "read_satisfactory_admin_password") {
    check(args.instanceId === details.summary.id, "credential read targets the current instance");
    if (deferredCredential) return new Promise<string>((resolve) => { releaseCredential = () => resolve("synthetic-admin-credential"); });
    return "synthetic-admin-credential";
  }
  if (command === "read_satisfactory_world_settings") {
    check(args.instanceId === details.summary.id, "snapshot read targets the current instance");
    if (pending && ++pending.reads >= 2) {
      current.active_session_name = pending.session; current.is_game_running = true;
      current.creative_mode_enabled = pending.creative; current.advanced_game_settings = { ...pending.settings };
      if (!current.sessions.some((entry) => entry.session_name === pending!.session)) {
        current.sessions.push({ session_name: pending.session, saves: [] });
      }
      bump(); pending = null;
    }
    return structuredClone(current);
  }
  const input = args.input;
  if (!record(input) || input.instance_id !== details.summary.id) throw new Error("Mutation targets the wrong instance");
  if (command === "write_satisfactory_world_rules" && deferNextRule) {
    deferNextRule = false;
    await new Promise<void>((resolve) => { releaseRule = resolve; });
  }
  if (command === "setup_satisfactory_server") {
    check(current.connection_status === "unclaimed" && typeof input.server_name === "string" && input.admin_password === null,
      "setup generates credentials instead of persisting a manual password");
    current.connection_status = "ready"; current.server_name = String(input.server_name); bump();
    return structuredClone(current);
  }
  if (command === "authorize_satisfactory_server") {
    check(current.connection_status === "authorization_required" && input.admin_password === "synthetic-authorize",
      "authorization uses one-time credentials");
    current.connection_status = "ready"; bump(); return structuredClone(current);
  }
  if (current.connection_status !== "ready" || input.expected_revision !== current.revision) throw new Error("Stale native revision");
  if (command === "write_satisfactory_room") {
    if (typeof input.server_name === "string") current.server_name = input.server_name;
    if (typeof input.auto_load_session_name === "string") current.auto_load_session_name = input.auto_load_session_name;
    bump(); return structuredClone(current);
  }
  if (command === "write_satisfactory_world_rules") {
    if (!current.creative_mode_enabled && input.acknowledge_enable_advanced_settings !== true) throw new Error("AGS consent missing");
    current.advanced_game_settings = { ...current.advanced_game_settings, ...strings(input.advanced_game_settings) };
    current.creative_mode_enabled = true; bump(); return structuredClone(current);
  }
  if (command === "create_satisfactory_world") {
    if (typeof input.session_name !== "string" || current.connected_players || current.sessions.some((entry) =>
      entry.session_name.toLowerCase() === String(input.session_name).toLowerCase())) throw new Error("Unsafe creation request");
    check(input.skip_onboarding === true, "dedicated world creation uses the verified native onboarding flag");
    if (Object.keys(strings(input.advanced_game_settings)).length > 0 && input.acknowledge_enable_advanced_settings !== true) {
      throw new Error("Creative creation consent missing");
    }
    pending = { session: input.session_name, creative: Object.keys(strings(input.advanced_game_settings)).length > 0,
      settings: { ...strings(input.game_mode_settings), ...strings(input.advanced_game_settings) }, reads: 0 };
    return { instance_id: current.instance_id, accepted: true, session_name: input.session_name };
  }
  if (command === "load_satisfactory_save") {
    const session = current.sessions.find((entry) => entry.saves.some((save) => save.save_name === input.save_name));
    if (!session || current.connected_players) throw new Error("Unsafe save-load request");
    pending = { session: session.session_name, creative: false, settings: {}, reads: 0 };
    return { instance_id: current.instance_id, accepted: true, session_name: session.session_name };
  }
  throw new Error(`Unexpected native command: ${command}`);
} } });

const moduleDetails: ModuleDetails = { summary: { id: "satisfactory", name: "Satisfactory", version: "fixture", install_state: "Installed", supported_platforms: ["windows"] },
  schema_json: JSON.stringify(schema), default_ports: [], runtime: {} };
function Workspace({ archived }: { archived: boolean }) {
  const [instance, setInstance] = useState(details);
  const coordinator = useInstanceSettingsSaveCoordinator();
  flush = () => coordinator.flush(instance.summary.id);
  flushBeforeStop = () => coordinator.flushBeforeStop(instance.summary.id);
  return <ConfigurationWorkspace details={instance} moduleDetails={moduleDetails} moduleDetailsError={null}
    archive={archived ? { archive_id: "retained", instance, maintenance: { autostart: false, auto_backup_on_stop: false,
      backup_retention_count: 3, crash_restart_limit: null, runtime_mode: null }, runs: { entries: [], total: 0, truncated: false },
      log: { relative_path: null, text: "", truncated: false, issues: [] }, backups: { entries: [], issues: [], truncated: false } } : undefined}
    onRetryModuleDetails={() => { throw new Error("Unexpected descriptor retry"); }} bindAddressCandidates={[]} runtime={null} launchPlan={null} launchPlanError={null}
    onSave={async (input) => {
      settingsWrites.push(structuredClone(input)); saved = input.settings_json;
      const next = { ...instance, settings_json: saved }; setInstance(next); return next;
    }} />;
}
function element<T extends Element>(selector: string): T {
  const result = fixture?.querySelector<T>(selector);
  if (!result) throw new Error(`Missing ${selector}: ${fixture?.textContent}`);
  return result;
}
async function until(condition: () => boolean, timeout = 8000) {
  const expires = performance.now() + timeout;
  while (!condition()) { if (performance.now() >= expires) throw new Error(`UI did not settle: ${fixture?.textContent}`); await act(frame); }
}
async function draw(key: string, archived = false) {
  await act(async () => { root.render(<I18nProvider key={key}><InstanceSettingsSaveProvider>
    <main className="server-detail-panel" style={{ margin: 16, height: "calc(100vh - 32px)" }}><Workspace archived={archived} /></main>
  </InstanceSettingsSaveProvider></I18nProvider>); });
  await until(() => Boolean(fixture?.querySelector(".configuration-workspace__body")));
  if (!archived) await idle();
}
function visiblePanel() {
  const panel = [...fixture!.querySelectorAll<HTMLElement>("[data-satisfactory-native-panel]")].find((entry) => entry.getClientRects().length);
  if (!panel) throw new Error(`No active native panel: ${fixture?.textContent}`);
  return panel;
}
async function idle() { await until(() => ![...fixture!.querySelectorAll<HTMLElement>("[data-satisfactory-native-panel]")]
  .some((panel) => panel.getAttribute("aria-busy") === "true")); }
function fieldSelector(key: string) {
  return `#${buildConfigurationFieldIds(key, "configuration-satisfactory").inputId},#${buildConfigurationFieldIds(key, "configuration-satisfactory", true).inputId}`;
}
function control<T extends HTMLInputElement | HTMLSelectElement>(key: string): T { return element<T>(fieldSelector(key)); }
function hasControl(key: string) { return Boolean(fixture?.querySelector(fieldSelector(key))); }
const ruleKey = (key: string) => `satisfactory_${key.replace(/\./gu, "_")}`;
function button(key: string) {
  const messages = locale === "zh-CN" ? ZH_CN_SATISFACTORY_MESSAGES : EN_US_SATISFACTORY_MESSAGES;
  const title = messages[`${prefix}.${key}`];
  const result = [...visiblePanel().querySelectorAll<HTMLButtonElement>("button")].find((entry) => entry.textContent?.trim() === title);
  if (!result) throw new Error(`Missing action ${key}/${title}: ${visiblePanel().textContent}`);
  return result;
}
async function click(key: string) { check(!button(key).disabled, `editable native action ${key}`); await act(async () => { button(key).click(); }); }
async function text(key: string, value: string) {
  const input = control<HTMLInputElement>(key); check(!input.disabled, `editable ${key}`);
  await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true })); });
}
async function toggle(key: string, value: boolean) {
  const input = control<HTMLInputElement>(key); check(!input.disabled && Boolean(input.labels?.length), `labeled toggle ${key}`);
  await act(async () => { if (input.checked !== value) input.click(); });
}
async function select(key: string, index: number) {
  const input = control<HTMLSelectElement>(key); check(!input.disabled && Boolean(input.options[index]), `editable native select ${key}`);
  await act(async () => { input.value = String(index); input.dispatchEvent(new Event("change", { bubbles: true })); });
}
async function section(id: string) {
  const navToggle = element<HTMLButtonElement>(".configuration-workspace__navigation-toggle");
  if (navToggle.getClientRects().length && navToggle.getAttribute("aria-expanded") !== "true") await act(async () => { navToggle.click(); });
  let option = fixture?.querySelector<HTMLButtonElement>(`[data-configuration-section-id="${id}"] > button`);
  const parent = ["world_generation", "creative_rules"].includes(id) ? "world" : id === "advanced" ? "runtime" : null;
  if (!option && parent) {
    await act(async () => { element<HTMLButtonElement>(`[data-configuration-section-id="${parent}"] > button[aria-expanded]`).click(); });
    option = fixture?.querySelector<HTMLButtonElement>(`[data-configuration-section-id="${id}"] > button`);
  }
  if (!option) throw new Error(`Missing category ${id}`);
  await act(async () => { option.click(); await frame(); });
}
function count(command: string) { return calls.filter((call) => call.command === command).length; }
function latest(command: string) { const call = calls.filter((entry) => entry.command === command).at(-1); if (!record(call?.args.input)) throw new Error("Missing native input"); return call.args.input; }
function layout() {
  const main = element<HTMLElement>(".configuration-workspace__main"), bounds = main.getBoundingClientRect();
  for (const field of fixture!.querySelectorAll<HTMLElement>(".configuration-field")) {
    if (!field.getClientRects().length) continue;
    const box = field.getBoundingClientRect();
    if (box.left < bounds.left - 1 || box.right > bounds.right + 1 || field.scrollWidth > field.clientWidth + 1) overflowViolations.push(field.dataset.fieldKey ?? "unknown");
  }
  check(!overflowViolations.length && main.scrollWidth <= main.clientWidth + 1, `${locale}: native fields fit ${innerWidth}px`);
  const ids = [...fixture!.querySelectorAll<HTMLElement>("[id]")].map((entry) => entry.id);
  check(ids.length === new Set(ids).size, `${locale}: each control has one focus owner`);
  check([...fixture!.querySelectorAll<HTMLElement>("[data-satisfactory-native-panel]")].filter((entry) => entry.getClientRects().length).length === 1,
    `${locale}: only the selected native category is interactive`);
  for (const field of visiblePanel().querySelectorAll<HTMLElement>(".configuration-field")) {
    const title = field.querySelector<HTMLLabelElement>("label")?.textContent?.trim();
    if (!title) continue;
    check(locale === "zh-CN" ? /[\u4e00-\u9fff]/u.test(title) : !/[\u4e00-\u9fff]/u.test(title),
      `${locale}: native label uses the active language`);
  }
}
async function refresh() { await click("refresh"); await idle(); }
async function autosaved(command: string, previous: number) { await until(() => count(command) > previous); await idle(); }
async function search(key: string, query: string) {
  const navToggle = element<HTMLButtonElement>(".configuration-workspace__navigation-toggle");
  if (navToggle.getClientRects().length && navToggle.getAttribute("aria-expanded") !== "true") await act(async () => { navToggle.click(); });
  const input = element<HTMLInputElement>('.configuration-search input[type="search"]');
  await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(input, query);
    input.dispatchEvent(new Event("input", { bubbles: true })); });
  await until(() => Boolean(fixture?.querySelector(".configuration-search-results button")));
  await act(async () => { element<HTMLButtonElement>(".configuration-search-results button").click(); await frame(); });
  check(document.activeElement?.id === buildConfigurationFieldIds(key, "configuration-satisfactory").inputId,
    `${locale}: native search focuses ${key}`);
}

async function runLocale() {
  localStorage.setItem("langame.locale", locale); document.documentElement.lang = locale;
  saved = JSON.stringify({ max_players: 6, future_setting: { retained: true } });
  details = { summary: { id: `satisfactory-${locale}`, module_id: "satisfactory", name: "Retained Manager Name", status: "Running",
    active_process_count: 1, autostart: false, bind_ip: "0.0.0.0" }, settings_json: saved, ports: [], config_file_path: "", saves_path: "",
    backup_uses_declared_saves_path: false, auto_backup_on_stop: false, backup_retention_count: 3, active_run: { run_id: 7, pid: 701 } };
  const baselineWrites = settingsWrites.length;
  const credentialReads = count("read_satisfactory_admin_password");
  current = snapshot("unclaimed");
  await draw(`${locale}-main`);
  await text("satisfactory_server_name", "Owned Factory"); await click("setup"); await idle();
  check(latest("setup_satisfactory_server").server_name === "Owned Factory", `${locale}: setup claims the native server with its chosen name`);
  check(count("read_satisfactory_admin_password") === credentialReads, `${locale}: credentials are not read automatically`);
  layout();
  current.connection_status = "authorization_required"; bump(); await refresh(); await section("access");
  await text("satisfactory_admin_authorization", "synthetic-authorize"); await click("authorize"); await idle();
  check(!hasControl("satisfactory_admin_authorization"), `${locale}: one-time authorization input is cleared after acceptance`);
  await click("showAdminPassword"); await until(() => hasControl("satisfactory_admin_password"));
  check(control<HTMLInputElement>("satisfactory_admin_password").value === "synthetic-admin-credential", `${locale}: credentials appear only after explicit reading`);
  await section("room"); await section("access");
  check(!hasControl("satisfactory_admin_password"), `${locale}: navigating away clears revealed credentials`);
  deferredCredential = true; await click("showAdminPassword"); await until(() => Boolean(releaseCredential));
  await section("room"); await act(async () => { releaseCredential!(); await frame(); }); await section("access");
  check(!hasControl("satisfactory_admin_password"), `${locale}: late credential responses remain cleared after navigation`);
  deferredCredential = false; releaseCredential = undefined;
  await section("room"); await text("satisfactory_server_name", "Changed Factory");
  await toggle("satisfactory_change_join_password", true); await text("satisfactory_join_password", "synthetic-join");
  const roomWrites = count("write_satisfactory_room");
  failNext = "write_satisfactory_room"; await autosaved("write_satisfactory_room", roomWrites);
  check(control<HTMLInputElement>("satisfactory_server_name").value === "Changed Factory" && control<HTMLInputElement>("satisfactory_join_password").value === "synthetic-join",
    `${locale}: failed room save retains its draft`);
  await click("retryAutosave"); await idle();
  check(count("write_satisfactory_room") === roomWrites + 2, `${locale}: room errors issue only the explicit retry`);
  check(latest("write_satisfactory_room").client_password === "synthetic-join" && current.server_name === "Changed Factory", `${locale}: explicit retry saves native room settings`);
  check(control<HTMLInputElement>("satisfactory_join_password").value === "", `${locale}: successful save clears join-password draft`);
  for (const [id, keys] of [["world", ["auto_pause_when_empty", "weather_preset"]], ["network", ["network_quality"]],
    ["advanced", ["send_gameplay_data"]]] as const) {
    await section(id);
    for (const key of keys) {
      const fields = [...fixture!.querySelectorAll<HTMLElement>(`[data-field-key="${key}"]`)].filter((entry) => entry.getClientRects().length);
      check(fields.length === 1, `${locale}: ${key} has one INI editing owner`);
      const input = control<HTMLInputElement | HTMLSelectElement>(key);
      const expected = key === "weather_preset" ? "2" : key === "network_quality" ? "3" : false;
      check(input instanceof HTMLSelectElement ? input.options[input.selectedIndex].value === expected : input.checked === expected,
        `${locale}: ${key} displays the read native value directly`);
      check(!fields[0].textContent?.includes("Keep native") && !fields[0].textContent?.includes("保留游戏设置"),
        `${locale}: ${key} has no ambiguous keep-native choice`);
    }
  }
  await section("room"); current.server_options["FG.WeatherPreset"] = "unknown-future-weather"; bump(); await refresh(); await section("world");
  const unknownWeather = control<HTMLSelectElement>("weather_preset");
  check(!unknownWeather.disabled && unknownWeather.options[unknownWeather.selectedIndex].textContent === "unknown-future-weather",
    `${locale}: an unsupported native value stays visible without inventing or persisting a preset`);
  current.server_options["FG.WeatherPreset"] = "2"; bump(); await section("room"); await refresh();
  await section("creative_rules");
  check(SATISFACTORY_RULES.filter((entry) => entry.scope !== "creation" && visiblePanel().querySelector(fieldSelector(ruleKey(entry.key)))).length === 8,
    `${locale}: existing-world rules cover five world and three new-player defaults`);
  check(visiblePanel().querySelectorAll(".guided-field-group").length === 2, `${locale}: player defaults have their own group`);
  await toggle(ruleKey(noPower), true);
  const firstRules = count("write_satisfactory_world_rules");
  await act(async () => { await new Promise<void>((resolve) => setTimeout(resolve, 950)); });
  check(count("write_satisfactory_world_rules") === firstRules, `${locale}: enabling AGS requires permanent-impact consent`);
  check(!visiblePanel().querySelector(".primary-button"), `${locale}: rules save automatically without a save button`);
  await toggle("satisfactory_creative_consent", true); await autosaved("write_satisfactory_world_rules", firstRules);
  check(latest("write_satisfactory_world_rules").acknowledge_enable_advanced_settings === true && current.advanced_game_settings[noPower] === "True",
    `${locale}: AGS consent and native True are sent together`);
  check(current.advanced_game_settings["FG.Future.Unknown"] === "retained", `${locale}: native rule saves preserve unknown server-owned values`);
  check(control<HTMLInputElement>("satisfactory_creative_consent").checked && control<HTMLInputElement>("satisfactory_creative_consent").disabled,
    `${locale}: irreversible AGS remains enabled after saving`);
  await toggle(ruleKey(noFuel), true); current.advanced_game_settings["FG.External.Rule"] = "changed"; bump(); await refresh();
  const staleRules = count("write_satisfactory_world_rules");
  await act(async () => { await new Promise<void>((resolve) => setTimeout(resolve, 950)); });
  check(count("write_satisfactory_world_rules") === staleRules && Boolean(button("discardDraft")), `${locale}: changed native world rules block saving old world drafts`);
  await click("discardDraft");
  await toggle(ruleKey(noFuel), true); current.active_session_name = "Other"; current.advanced_game_settings = {}; bump(); await refresh();
  check(count("write_satisfactory_world_rules") === staleRules && Boolean(button("discardDraft")), `${locale}: a changed active world blocks saving the previous world's draft`);
  await click("discardDraft");
  check(!control<HTMLInputElement>(ruleKey(noFuel)).checked, `${locale}: discarding reads the new world's rule state`);
  await toggle(ruleKey(noFuel), true); failNext = "write_satisfactory_world_rules";
  await autosaved("write_satisfactory_world_rules", staleRules);
  check(control<HTMLInputElement>(ruleKey(noFuel)).checked && !button("retryAutosave").disabled, `${locale}: rule save errors retain editable drafts`);
  const failedRules = count("write_satisfactory_world_rules");
  await act(async () => { await new Promise<void>((resolve) => setTimeout(resolve, 950)); });
  check(count("write_satisfactory_world_rules") === failedRules, `${locale}: failed native mutations are not automatically resent`);
  await act(async () => { await flush(); });
  check(count("write_satisfactory_world_rules") === failedRules,
    `${locale}: a failed live API draft does not block or resend during the ordinary start barrier`);
  await click("retryAutosave"); await idle();
  deferNextRule = true;
  const queuedRules = count("write_satisfactory_world_rules");
  await toggle(ruleKey(noPower), true); await until(() => Boolean(releaseRule));
  await toggle(ruleKey(noFuel), false);
  check(!control<HTMLInputElement>(ruleKey(noFuel)).disabled, `${locale}: edits remain available during native saving`);
  await act(async () => { releaseRule!(); releaseRule = undefined; await frame(); });
  await until(() => count("write_satisfactory_world_rules") === queuedRules + 2); await idle();
  check(current.advanced_game_settings[noPower] === "True" && current.advanced_game_settings[noFuel] === "False",
    `${locale}: a late save response retains and saves the newer draft`);
  deferNextRule = true;
  await toggle(ruleKey("FG.GameRules.NoUnlockCost"), true);
  let stopped = false;
  const beforeStopWrites = count("write_satisfactory_world_rules");
  const stopping = flushBeforeStop().then(() => {
    stopped = true; current.connection_status = "stopped"; current.is_game_running = false;
  });
  await until(() => Boolean(releaseRule));
  check(!stopped && current.connection_status === "ready" && current.is_game_running && count("write_satisfactory_world_rules") === beforeStopWrites + 1,
    `${locale}: an immediate stop waits for the native save while the live API is still available`);
  await act(async () => { releaseRule!(); releaseRule = undefined; await stopping; });
  check(stopped && current.advanced_game_settings["FG.GameRules.NoUnlockCost"] === "True",
    `${locale}: the API is stopped only after the native rule has been confirmed`);
  await act(async () => { await flush(); });
  check(count("write_satisfactory_world_rules") === beforeStopWrites + 1,
    `${locale}: starting after a native save does not require calling the stopped API`);
  current.connection_status = "ready"; current.is_game_running = true;
  current.advanced_game_settings["FG.GameRules.NoUnlockCost"] = "False"; bump(); await refresh();
  await section("room"); await text("satisfactory_server_name", "Coordinated Factory");
  await section("creative_rules"); await toggle(ruleKey("FG.GameRules.NoUnlockCost"), true);
  const coordinatedWrites = count("write_satisfactory_room") + count("write_satisfactory_world_rules");
  await act(async () => { await flushBeforeStop(); }); await idle();
  check(current.server_name === "Coordinated Factory" && current.advanced_game_settings["FG.GameRules.NoUnlockCost"] === "True" &&
    count("write_satisfactory_room") + count("write_satisfactory_world_rules") === coordinatedWrites + 2,
    `${locale}: the stop barrier flushes room and rule drafts with successive native revisions`);
  layout(); await section("room"); await search("native_creative_rules", noPower);
  await section("world_generation");
  check(SATISFACTORY_RULES.filter((entry) => entry.key.startsWith("FG.GameMode.") && visiblePanel().querySelector(fieldSelector(ruleKey(entry.key)))).length === 6,
    `${locale}: creation exposes all six generation rules`);
  for (const [key, values] of [[energy, ["25", "50", "75", "100", "200", "500"]], [purity, ["0", "1", "5", "2", "6", "3", "4"]]] as const) {
    const definition = SATISFACTORY_RULES.find((entry) => entry.key === key)!;
    check(control<HTMLSelectElement>(ruleKey(key)).options.length === values.length && JSON.stringify(definition.options.map((entry) => entry.value)) === JSON.stringify(values),
      `${locale}: ${key} keeps native option order and values`);
  }
  await text("satisfactory_new_session_name", "retained");
  check(button("createWorld").disabled && control<HTMLInputElement>("satisfactory_confirm_create").disabled, `${locale}: creation refuses an existing name case-insensitively`);
  await text("satisfactory_new_session_name", `Fresh ${locale}`);
  for (const [key, value] of [[energy, "25"], ["FG.GameMode.PartsCostMultiplier", "125"], ["FG.GameMode.SpacePartsCostMultiplier", "5000"],
    [purity, "5"], ["FG.GameMode.NodeRandomization", "4"]]) {
    const definition = SATISFACTORY_RULES.find((entry) => entry.key === key)!;
    await select(ruleKey(key), definition.options.findIndex((entry) => entry.value === value));
  }
  await click("generateSeed");
  check(Number.isInteger(Number(control<HTMLInputElement>(ruleKey(seed)).value)) && Number(control<HTMLInputElement>(ruleKey(seed)).value) !== 0,
    `${locale}: seed generation produces a nonzero native int32`);
  await click("randomSeed"); check(control<HTMLInputElement>(ruleKey(seed)).value === "0", `${locale}: random seed uses native zero`);
  await text(ruleKey(seed), "-12345"); await select(ruleKey("FG.GameRules.StartingTier"), 10);
  check(!hasControl(ruleKey("FG.GameRules.GiveAllTiers")),
    `${locale}: starting tier has one editor for the all-tiers choice`);
  await toggle("satisfactory_confirm_create", true);
  check(button("createWorld").disabled, `${locale}: creative starting progress also requires permanent-impact consent`);
  await toggle("satisfactory_creation_creative_consent", true);
  const creates = count("create_satisfactory_world"), reads = count("read_satisfactory_world_settings");
  await click("createWorld"); await idle();
  const input = latest("create_satisfactory_world");
  check(strings(input.game_mode_settings)[purity] === "5" && strings(input.game_mode_settings)[energy] === "25" && strings(input.game_mode_settings)[seed] === "-12345",
    `${locale}: creation sends native purity values, percentage integers and seed strings`);
  check(strings(input.advanced_game_settings)["FG.GameRules.StartingTier"] === "10" &&
    strings(input.advanced_game_settings)["FG.GameRules.GiveAllTiers"] === "True" && input.acknowledge_enable_advanced_settings === true,
    `${locale}: all-tiers choice sends both native settings with explicit AGS consent`);
  check(count("create_satisfactory_world") === creates + 1 && count("read_satisfactory_world_settings") >= reads + 2,
    `${locale}: API acceptance polls completion without resending creation`);
  check(current.active_session_name === `Fresh ${locale}` && button("createWorld").disabled, `${locale}: accepted creation reaches the requested world and clears confirmation`);
  layout();
  await text("satisfactory_new_session_name", `Pending ${locale}`); await toggle("satisfactory_confirm_create", true);
  const pendingCreates = count("create_satisfactory_world");
  failNext = "read_satisfactory_world_settings"; await click("createWorld"); await idle();
  check(button("createWorld").disabled && count("create_satisfactory_world") === pendingCreates + 1,
    `${locale}: accepted operations stay protected when a completion read fails`);
  await refresh(); await refresh();
  check(current.active_session_name === `Pending ${locale}` && count("create_satisfactory_world") === pendingCreates + 1,
    `${locale}: explicit state refresh resolves an accepted operation without resending it`);
  current.connected_players = 2; bump(); await refresh();
  check(button("createWorld").disabled, `${locale}: connected players block new-world creation`);
  await section("room"); check(control<HTMLSelectElement>("satisfactory_load_save").disabled && button("loadSelectedSave").disabled,
    `${locale}: connected players block destructive save loading`);
  current.connected_players = 0; bump(); await refresh(); await select("satisfactory_load_save", 1); await toggle("satisfactory_confirm_load", true);
  const loads = count("load_satisfactory_save"), loadReads = count("read_satisfactory_world_settings");
  await click("loadSelectedSave"); await idle();
  check(count("load_satisfactory_save") === loads + 1 && count("read_satisfactory_world_settings") >= loadReads + 2 && current.active_session_name === "Retained",
    `${locale}: loading selects a real save and polls once-accepted operations`);
  check(settingsWrites.length === baselineWrites && saved === details.settings_json, `${locale}: native room/world parameters never enter instance JSON`);
  await text("max_players", "7"); await act(async () => { await flush(); });
  check(JSON.parse(saved).max_players === 7 && Object.keys(JSON.parse(saved)).sort().join() === "future_setting,max_players",
    `${locale}: existing INI controls still use the ordinary settings save chain`);
  deferNextRule = true;
  await section("creative_rules"); await toggle(ruleKey("FG.PlayerRules.GodMode"), true);
  await toggle("satisfactory_creative_consent", true);
  await until(() => Boolean(releaseRule));
  const leavingFlush = flushBeforeStop;
  let leftSaveFinished = false;
  const leavingCompletion = leavingFlush().then(() => { leftSaveFinished = true; });
  const leavingWrites = count("write_satisfactory_world_rules");
  details = { ...details, settings_json: saved }; await draw(`${locale}-archive`, true);
  check(!leftSaveFinished, `${locale}: leaving retains the in-flight native save in the stop barrier`);
  await act(async () => { releaseRule!(); releaseRule = undefined; await leavingCompletion; });
  check(current.advanced_game_settings["FG.PlayerRules.GodMode"] === "True" && leftSaveFinished && count("write_satisfactory_world_rules") === leavingWrites,
    `${locale}: an in-flight native write succeeds after leaving without being canceled or resent`);
  const archiveCalls = calls.length;
  await draw(`${locale}-archive-again`, true);
  check(!fixture!.querySelector("[data-satisfactory-native-panel]") && calls.length === archiveCalls, `${locale}: archives never read or mutate live native server state`);
}
async function run() {
  await prepareBrowserLocaleCatalogs();
  for (const language of locales) { locale = language; await runLocale(); }
  check(errors.length === 0, "React and browser report no errors");
  await draw("zh-CN-final-preview"); await section("creative_rules"); layout();
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  return { status: "passed", locales, checks, browser_errors: errors, overflow_violations: overflowViolations };
}
Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  await act(async () => { root.unmount(); await frame(); }); return { browser_errors: errors, native_dialogs: 0 };
} });
void run().catch((error) => ({ status: "failed", error: String(error?.stack ?? error), checks, browser_errors: errors, overflow_violations: overflowViolations }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
