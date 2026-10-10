import { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import schema from "../../../../modules/runescapedragonwilds/schema.json";
import nativeContract from "../../../../modules/runescapedragonwilds/world-settings.json";
import type {
  DragonwildsWorldMode, DragonwildsWorldSettingDefinition, DragonwildsWorldSettingsSnapshot,
  WriteDragonwildsWorldSettingsInput
} from "../../src/dragonwilds-world-settings";
import { I18nProvider } from "../../src/i18n";
import { ConfigurationWorkspace } from "../../src/views/settings/ConfigurationWorkspace";
import { DragonwildsWorldSettingsPanel } from "../../src/views/settings/DragonwildsWorldSettingsPanel";
import { DragonwildsWorldSettingsProvider } from "../../src/views/settings/DragonwildsWorldSettingsContext";
import { InstanceSettingsSaveProvider, useInstanceSettingsSaveCoordinator } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceDetails, ModuleDetails, UpdateInstanceInput } from "../../src/types";
import type { InstanceArchiveDetails } from "../../src/storage-management-types";
import "../../src/app.css";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture");
if (!fixture) throw new Error("Fixture root is missing");
const root = createRoot(fixture);
const checks: string[] = [];
const loadingLifecycleChecks: string[] = [];
const errors: string[] = [];
const overflowViolations: string[] = [];
const calls: Array<{ command: string; args: Record<string, unknown> }> = [];
const instanceSettingsWrites: UpdateInstanceInput[] = [];
const locales = ["en-US", "zh-CN"] as const;
const modes: DragonwildsWorldMode[] = ["Normal", "Hard", "Creative", "Custom"];
const processing = "Difficulty.Progression.ProcessingSpeedScale";
const friendlyFire = "Difficulty.Environment.FriendlyFire";
const unknownRule = "Difficulty.Future.UnknownRule";
const categories = ["survival", "player", "death", "magic", "building", "crafting", "progression", "creatures"] as const;
const worldEvents = ["Difficulty.WorldEvents.MajorWorldEventFrequencyScale", "Difficulty.WorldEvents.MinorWorldEventFrequencyScale"];
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
window.alert = window.confirm = window.prompt = () => { throw new Error("Unexpected native dialog"); };
const definitions: DragonwildsWorldSettingDefinition[] = nativeContract.settings.map((entry) => {
  if ((entry.kind !== "number" && entry.kind !== "boolean") ||
    !["Never", "OnlyCustom", "CustomAndCreative", "AllModes"].includes(entry.player_adjustable)) {
    throw new Error(`Unexpected native contract definition: ${entry.tag}`);
  }
  const playerAdjustable = entry.player_adjustable;
  if (playerAdjustable !== "Never" && playerAdjustable !== "OnlyCustom" &&
    playerAdjustable !== "CustomAndCreative" && playerAdjustable !== "AllModes") throw new Error("Invalid native permission");
  return { ...entry, kind: entry.kind, player_adjustable: playerAdjustable };
});
const moduleDetails: ModuleDetails = {
  summary: { id: "runescapedragonwilds", name: "Dragonwilds", version: "1.0", install_state: "Installed", supported_platforms: ["windows"] },
  schema_json: JSON.stringify(schema), default_ports: [], runtime: {}
};
const initialInstance: InstanceDetails = {
  summary: { id: "dragonwilds-world-ui", module_id: "runescapedragonwilds", name: "Retained world", status: "Stopped",
    active_process_count: 0, autostart: false, bind_ip: "0.0.0.0" },
  settings_json: JSON.stringify({ server_name: "Retained world", default_world_name: "Retained",
    world_password: "fixture-password", future_setting: { retained: true } }),
  ports: [], config_file_path: "", saves_path: "", backup_uses_declared_saves_path: true,
  auto_backup_on_stop: false, backup_retention_count: 3, active_run: null
};
let details = structuredClone(initialInstance);
let current: DragonwildsWorldSettingsSnapshot;
let failNextRead = false;
let flushSettings: () => Promise<void>;
let revision = 0;
let nextWriteGate: Promise<void> | null = null;
let workspaceVisible = true;
interface NativeReadGate {
  completion: Promise<void>;
  failure: string | null;
  release(): void;
}
let currentReadGate: NativeReadGate | null = null;
const outstandingReadGates = new Set<NativeReadGate>();
const completedReads: Array<{ instanceId: string; result: "success" | "failure" }> = [];
function holdNativeReads(failure: string | null = null): NativeReadGate {
  let resolve: () => void = () => { throw new Error("Native read gate was not initialized"); };
  const completion = new Promise<void>((done) => { resolve = done; });
  const gate: NativeReadGate = { completion, failure, release: () => {
    if (currentReadGate === gate) currentReadGate = null;
    outstandingReadGates.delete(gate);
    resolve();
  } };
  outstandingReadGates.add(gate);
  currentReadGate = gate;
  return gate;
}
// Category expectations independently express the public navigation contract.
function expectedCategory(tag: string): string {
  const parts = tag.split(".");
  const key = parts.at(-1) ?? "";
  if (tag === friendlyFire) return "player";
  if (parts[1] === "AI") return "creatures";
  if (parts[1] === "SurvivalCore") return "survival";
  if (/OnDeath|Gravestones/u.test(key)) return "death";
  if (/Spell|Teleportation/u.test(key)) return "magic";
  if (/Building/u.test(key)) return "building";
  if (/Crafting|Processing/u.test(key)) return "crafting";
  if (parts[1] === "Progression") return "progression";
  if (parts[1] === "Player") return "player";
  return "world";
}
function expectedSection(tag: string): string {
  const category = expectedCategory(tag);
  return category === "world" ? "world" : `world_${category}`;
}
const editableDefinitions = definitions.filter((entry) => entry.can_change_after_creation && entry.player_adjustable !== "Never");
// The fake replaces only the native transport. Expected permissions and effective
// values are computed from the canonical native contract, not the UI's helpers.
function effectiveValues(mode: DragonwildsWorldMode, overrides: Record<string, number>): Record<string, number> {
  return Object.fromEntries(definitions.map((entry) =>
    [entry.tag, overrides[entry.tag] ?? entry.preset_defaults[mode === "Custom" ? "Normal" : mode]]));
}
function initialSnapshot(mode: DragonwildsWorldMode = "Normal"): DragonwildsWorldSettingsSnapshot {
  const overrides = { [processing]: 1.7, [unknownRule]: 19 };
  return { instance_id: details.summary.id, status: "ready", world_file: "DedicatedWorld.sav", world_name: "Retained world",
    world_mode: mode, revision: `fixture-revision-${++revision}`, values: effectiveValues(mode, overrides), overrides,
    definitions, writable: true, message: null, backup_id: null };
}
function writeInput(args: Record<string, unknown>): WriteDragonwildsWorldSettingsInput {
  const input = args.input;
  if (!input || typeof input !== "object" || !("instance_id" in input) || typeof input.instance_id !== "string" ||
    !("world_file" in input) || typeof input.world_file !== "string" || !("expected_revision" in input) ||
    typeof input.expected_revision !== "string" || !("world_mode" in input) || !("values" in input) ||
    !input.values || typeof input.values !== "object" || Array.isArray(input.values)) throw new Error("Invalid native write input");
  const mode = modes.find((value) => value === input.world_mode);
  if (!mode) throw new Error("Invalid native world mode");
  const values: Record<string, number> = {};
  for (const [key, value] of Object.entries(input.values)) {
    if (typeof value !== "number" || !Number.isFinite(value)) throw new Error("Invalid native world value");
    values[key] = value;
  }
  return { instance_id: input.instance_id, world_file: input.world_file,
    expected_revision: input.expected_revision, world_mode: mode, values };
}
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args: Record<string, unknown> = {}) => {
  calls.push({ command, args: structuredClone(args) });
  if (command === "read_dragonwilds_world_settings") {
    if (args.instanceId !== details.summary.id) throw new Error("Native read targets the wrong instance");
    // Native replies belong to the requested instance and captured save, even
    // when a later render selects another instance before the reply arrives.
    const snapshot = structuredClone(current);
    const gate = currentReadGate;
    const failure = failNextRead ? "Synthetic world read failure" : gate?.failure;
    failNextRead = false;
    if (gate) await gate.completion;
    completedReads.push({ instanceId: snapshot.instance_id, result: failure ? "failure" : "success" });
    if (failure) throw new Error(failure);
    return snapshot;
  }
  if (command !== "write_dragonwilds_world_settings") throw new Error(`Unexpected native command: ${command}`);
  const input = writeInput(args);
  if (input.instance_id !== current.instance_id || input.world_file !== current.world_file) throw new Error("World identity changed");
  if (input.expected_revision !== current.revision) throw new Error("World settings conflict: refresh the changed world");
  if (details.summary.status !== "Stopped" || details.summary.active_process_count !== 0 || details.active_run || !current.writable) {
    throw new Error("Native boundary rejects writes while unavailable");
  }
  const gate = nextWriteGate;
  nextWriteGate = null;
  if (gate) await gate;
  for (const [tag, value] of Object.entries(input.values)) {
    const definition = definitions.find((entry) => entry.tag === tag);
    if (!definition || !definition.can_change_after_creation || definition.player_adjustable === "Never" ||
      (definition.player_adjustable === "OnlyCustom" && input.world_mode !== "Custom") ||
      (definition.player_adjustable === "CustomAndCreative" && !["Custom", "Creative"].includes(input.world_mode)) ||
      value < definition.minimum || value > definition.maximum ||
      Math.abs(value * 10 ** definition.decimal_places - Math.round(value * 10 ** definition.decimal_places)) > 1e-6) {
      throw new Error("Native boundary rejects invalid world rule");
    }
  }
  const overrides = current.world_mode !== "Custom" && input.world_mode === "Custom"
    ? { ...current.overrides, ...current.values, ...input.values } : { ...current.overrides, ...input.values };
  current = { ...current, world_mode: input.world_mode, overrides, values: effectiveValues(input.world_mode, overrides),
    revision: `fixture-revision-${++revision}`, backup_id: `world-rule-backup-${revision}` };
  return structuredClone(current);
} } });
function Workspace({ archive }: { archive?: InstanceArchiveDetails }) {
  const coordinator = useInstanceSettingsSaveCoordinator();
  const instanceId = details.summary.id;
  flushSettings = () => coordinator.flush(instanceId);
  return workspaceVisible ? <ConfigurationWorkspace details={details} moduleDetails={moduleDetails} moduleDetailsError={null}
    onRetryModuleDetails={() => { throw new Error("Unexpected module retry"); }} bindAddressCandidates={[]}
    runtime={null} launchPlan={null} launchPlanError={null} archive={archive}
    onSave={async (input) => { instanceSettingsWrites.push(structuredClone(input)); throw new Error("World editing must not autosave instance settings"); }} /> : null;
}
function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
  checks.push(description);
}
function element<T extends Element>(selector: string, scope: ParentNode = fixture!): T {
  const result = scope.querySelector<T>(selector);
  if (!result) throw new Error(`Missing ${selector}; ${fixture?.textContent?.slice(0, 1200)}`);
  return result;
}
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
async function until(predicate: () => boolean, description: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(description);
    await act(frame);
  }
}
function panel() { return element<HTMLElement>("[data-dragonwilds-world-editor]"); }
function modeSelect() { return element<HTMLSelectElement>("select", panel()); }
function refreshButton() { return element<HTMLButtonElement>(".panel-head button.secondary-button", panel()); }
function primaryControls() { return [...panel().querySelectorAll<HTMLInputElement>("[data-dragonwilds-setting] input[type=number],[data-dragonwilds-setting] input[type=checkbox]")]; }
function control(tag: string) {
  return element<HTMLInputElement>(`[data-dragonwilds-setting="${tag}"] input[type=number],[data-dragonwilds-setting="${tag}"] input[type=checkbox]`, panel());
}
function writeCalls() { return calls.filter((call) => call.command === "write_dragonwilds_world_settings"); }
async function renderFixture(key: string, archive?: InstanceArchiveDetails, callerDisabled = false) {
  await act(async () => { root.render(<StrictMode><I18nProvider key={key}><InstanceSettingsSaveProvider>
    <main className="server-detail-panel" style={{ height: "calc(100vh - 32px)", margin: 16 }}>
      {callerDisabled ? <div className="configuration-workspace" style={{ padding: 20, overflow: "auto" }}>
        <DragonwildsWorldSettingsProvider details={details}>
          <DragonwildsWorldSettingsPanel sectionId="world_crafting" details={details} moduleDetails={moduleDetails}
            settings={JSON.parse(details.settings_json)} disabled onPatch={() => { throw new Error("Unexpected settings patch"); }} />
        </DragonwildsWorldSettingsProvider>
      </div> : <Workspace archive={archive} />}
    </main>
  </InstanceSettingsSaveProvider></I18nProvider></StrictMode>); });
}
async function draw(key: string, archive?: InstanceArchiveDetails, callerDisabled = false) {
  await renderFixture(key, archive, callerDisabled);
  await until(() => Boolean(fixture?.querySelector(callerDisabled ? "[data-dragonwilds-world-editor]" : ".configuration-workspace__body")), "Workspace did not mount");
  if (!archive && callerDisabled) await until(() => panel().getAttribute("aria-busy") === "false", "Native world read did not finish");
  await act(async () => { await document.fonts.ready; await frame(); });
}
async function section(id: string, waitForIdle = true) {
  const button = element<HTMLButtonElement>(`[data-configuration-section-id="${id}"] > button`);
  if (button.getClientRects().length === 0) {
    await act(async () => { element<HTMLButtonElement>(".configuration-workspace__navigation-toggle").click(); await frame(); });
  }
  check(button.getClientRects().length > 0, `Section ${id} is visible before navigation`);
  await act(async () => { button.click(); await frame(); });
  if (waitForIdle && id.startsWith("world")) await until(() => Boolean(fixture?.querySelector(`[data-dragonwilds-world-editor="${id}"][aria-busy="false"]`)), `Section ${id} did not finish its native read`);
}
async function select(input: HTMLSelectElement, value: string) {
  check(!input.disabled, `Select ${value} is editable`);
  await act(async () => { input.value = value; input.dispatchEvent(new Event("change", { bubbles: true })); });
}
async function setValue(input: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  if (!setter) throw new Error("Native input setter is unavailable");
  await act(async () => { setter.call(input, value); input.dispatchEvent(new Event("input", { bubbles: true })); });
}
async function flush() { await act(async () => { await flushSettings(); await frame(); }); }
async function flushError() {
  let failure = "";
  await act(async () => { try { await flushSettings(); } catch (cause) { failure = String(cause); } });
  return failure;
}
async function elapsed(milliseconds: number) {
  await act(async () => { await new Promise<void>((resolve) => setTimeout(resolve, milliseconds)); });
}
async function clickRefresh() {
  await act(async () => { refreshButton().click(); await frame(); });
  await until(() => panel().getAttribute("aria-busy") === "false", "Native refresh did not finish");
}
async function verifyNavigation(locale: string) {
  await section("room");
  const toggle = element<HTMLButtonElement>(".configuration-workspace__navigation-toggle");
  if (toggle.getClientRects().length && toggle.getAttribute("aria-expanded") !== "true") await act(async () => { toggle.click(); });
  const search = element<HTMLInputElement>('.configuration-search input[type="search"]');
  await setValue(search, locale === "zh-CN" ? "加工" : "processing");
  const hit = [...fixture!.querySelectorAll<HTMLButtonElement>(".configuration-search-results li > button")]
    .find((button) => button.querySelector("code")?.textContent === processing ||
      button.querySelector("strong")?.textContent?.toLocaleLowerCase().includes(locale === "zh-CN" ? "加工耗时" : "processing time"));
  check(hit, `${locale}: global search discovers an individual native processing rule`);
  await act(async () => { hit.click(); await frame(); });
  await until(() => Boolean(fixture?.querySelector('[data-dragonwilds-world-editor="world_crafting"]')), "Search did not navigate to crafting rules");
  const anchorId = "configuration-runescapedragonwilds-difficulty-progression-processingspeedscale-input";
  const anchor = element<HTMLElement>(`#${anchorId}`);
  check(fixture?.querySelectorAll(`#${anchorId}`).length === 1, `${locale}: native processing rule has a unique navigation anchor`);
  check(document.activeElement === anchor || Boolean(document.activeElement && anchor.contains(document.activeElement)),
    `${locale}: global search focuses the actual processing control in its category`);
  if (toggle.getClientRects().length && toggle.getAttribute("aria-expanded") !== "true") await act(async () => { toggle.click(); });
  await setValue(search, friendlyFire);
  const friendlyHit = [...fixture!.querySelectorAll<HTMLButtonElement>(".configuration-search-results li > button")]
    .find((button) => button.querySelector("code")?.textContent === friendlyFire ||
      button.querySelector("strong")?.textContent?.toLocaleLowerCase().includes(locale === "zh-CN" ? "友军伤害" : "friendly fire"));
  check(friendlyHit, `${locale}: global search discovers friendly fire individually`);
  await act(async () => { friendlyHit.click(); await frame(); });
  await until(() => Boolean(fixture?.querySelector('[data-dragonwilds-world-editor="world_player"]')), "Friendly fire search did not navigate to player rules");
  const friendlyAnchorId = "configuration-runescapedragonwilds-difficulty-environment-friendlyfire-input";
  const friendlyAnchor = element<HTMLElement>(`#${friendlyAnchorId}`);
  check(fixture?.querySelectorAll(`#${friendlyAnchorId}`).length === 1 &&
    (document.activeElement === friendlyAnchor || Boolean(document.activeElement && friendlyAnchor.contains(document.activeElement))),
    `${locale}: friendly fire search focuses its unique actual player rule control`);
  await section("world_crafting");
}
async function pointerClick(input: HTMLInputElement, ratio: number) {
  await act(async () => {
    input.scrollIntoView({ block: "center" });
    await frame();
    const bounds = input.getBoundingClientRect();
    const response = await fetch(`/__reliability_pointer/${nonce}`, { method: "POST", body: JSON.stringify({
      type: "click", x: bounds.left + 8 + (bounds.width - 16) * ratio, y: bounds.top + bounds.height / 2
    }) });
    if (!response.ok) throw new Error("Real pointer dispatch failed");
    await frame();
  });
}
async function verifyCategories(locale: string) {
  const seen = new Set<string>();
  const peerIds = ["world", ...categories.map((category) => `world_${category}`)];
  const rootItems = [...fixture!.querySelectorAll<HTMLElement>(".configuration-section-navigation__list--root > [data-configuration-section-id]")];
  check(rootItems.filter((entry) => peerIds.includes(entry.dataset.configurationSectionId!)).map((entry) => entry.dataset.configurationSectionId).join("|") === peerIds.join("|"),
    `${locale}: world and eight gameplay categories are ordered navigation peers without expanding world`);
  for (const id of peerIds) {
    const item = element<HTMLElement>(`[data-configuration-section-id="${id}"]`);
    check(item.parentElement?.classList.contains("configuration-section-navigation__list--root") &&
      !item.querySelector(".configuration-section-navigation__disclosure,.configuration-section-navigation__list--nested"),
      `${locale}: ${id} has a direct root navigation action without a nested world branch`);
  }
  check(!fixture!.querySelector('[data-configuration-section-id="world_environment"]'), `${locale}: obsolete environment navigation is absent`);
  for (const category of ["world", ...categories]) {
    await section(category === "world" ? "world" : `world_${category}`);
    const expected = editableDefinitions.filter((entry) => expectedCategory(entry.tag) === category);
    const controls = primaryControls();
    check(controls.length === expected.length, `${locale}: ${category} owns exactly ${expected.length} native rules`);
    check(panel().querySelectorAll("select,input[type=search]").length === (category === "world" ? 1 : 0),
      `${locale}: ${category} uses workspace navigation and only world owns its mode selector`);
    check(!panel().querySelector("button.primary-button"), `${locale}: ${category} has no manual save button`);
    for (const entry of expected) {
      const input = control(entry.tag);
      check(!input.disabled, `${locale}: stopped ${current.world_mode} allows editing ${entry.tag}`);
      check(!seen.has(entry.tag), `${locale}: ${entry.tag} has exactly one category`);
      seen.add(entry.tag);
      const saved = current.values[entry.tag];
      check(entry.kind === "boolean" ? input.checked === (saved === 1) : Number(input.value) === saved,
        `${locale}: ${entry.tag} displays the current saved value`);
    }
    verifyLabelsAndLayout(locale);
  }
  check(seen.size === 63, `${locale}: world and eight peer gameplay categories cover all 63 editable native rules exactly once`);
}
function verifyLabelsAndLayout(locale: string) {
  const content = element<HTMLElement>(".configuration-workspace__main");
  const bounds = content.getBoundingClientRect();
  if (content.scrollWidth > content.clientWidth + 1) overflowViolations.push(`${locale}: workspace has horizontal overflow`);
  for (const input of primaryControls()) {
    const field = input.closest<HTMLElement>("[data-dragonwilds-setting]");
    if (!field) throw new Error("Native rule lacks a field container");
    const label = input.labels?.[0]?.textContent?.trim();
    check(Boolean(label), `${locale}: ${field.dataset.dragonwildsSetting} has a visible associated label`);
    check(locale !== "zh-CN" || /[\u3400-\u9fff]/u.test(label!), `${locale}: native rule label is localized`);
    const box = field.getBoundingClientRect();
    if (box.width <= 0 || box.left < bounds.left - 1 || box.right > bounds.right + 1 || field.scrollWidth > field.clientWidth + 1) {
      overflowViolations.push(`${locale}: ${field.dataset.dragonwildsSetting} escapes the workspace`);
    }
  }
  check(overflowViolations.length === 0, `${locale}: rules fit the ${innerWidth}px viewport: ${overflowViolations.join("; ")}`);
}
function archiveDetails(): InstanceArchiveDetails {
  return { archive_id: "archive-dragonwilds-fixture", instance: details,
    maintenance: { autostart: false, auto_backup_on_stop: false, backup_retention_count: 3, crash_restart_limit: null, runtime_mode: null },
    runs: { entries: [], total: 0, truncated: false }, log: { relative_path: null, text: "", truncated: false, issues: [] },
    backups: { entries: [], issues: [], truncated: false } };
}

async function verifyReadLifecycle(locale: string) {
  const writesBefore = writeCalls().length;
  function lifecycle(condition: unknown, description: string) {
    check(condition, `${locale}: ${description}`);
    loadingLifecycleChecks.push(`${locale}: ${description}`);
  }
  details = structuredClone(initialInstance);
  current = initialSnapshot();
  await draw(`${locale}-strict-immediate`);
  await section("world");
  lifecycle(modeSelect().value === "Normal" && panel().getAttribute("aria-busy") === "false",
    "StrictMode immediate native success leaves loading and displays saved rules");

  current = initialSnapshot("Custom");
  const success = holdNativeReads();
  const successesBefore = completedReads.length;
  await draw(`${locale}-strict-delayed-success`);
  await section("world", false);
  lifecycle(panel().getAttribute("aria-busy") === "true" && refreshButton().disabled && !panel().querySelector("select"),
    "StrictMode delayed native read remains pending without fabricated controls");
  await act(async () => { success.release(); await frame(); });
  await until(() => completedReads.length > successesBefore && panel().getAttribute("aria-busy") === "false",
    "Resolved delayed native success left the editor loading");
  lifecycle(modeSelect().value === "Custom" && !refreshButton().disabled,
    "StrictMode delayed native success clears loading after the transport resolves");

  current = initialSnapshot();
  const failure = holdNativeReads("Synthetic delayed world read failure");
  const failuresBefore = completedReads.length;
  await draw(`${locale}-strict-delayed-failure`);
  await section("world", false);
  await act(async () => { failure.release(); await frame(); });
  await until(() => completedReads.length > failuresBefore && panel().getAttribute("aria-busy") === "false",
    "Rejected delayed native read left the editor loading");
  lifecycle(panel().textContent?.includes("Synthetic delayed world read failure") && !panel().querySelector("select") && !refreshButton().disabled,
    "StrictMode delayed native failure clears loading and exposes the real error with retry");
  await clickRefresh();
  lifecycle(modeSelect().value === "Normal", "explicit retry recovers a delayed read failure without writing defaults");

  for (const outcome of ["success", "failure"] as const) {
    details = { ...structuredClone(initialInstance), summary: { ...initialInstance.summary, id: `old-${locale}-${outcome}` } };
    current = { ...initialSnapshot("Hard"), world_name: "Old instance world" };
    const oldInstanceId = details.summary.id;
    const late = holdNativeReads(outcome === "failure" ? "Synthetic old instance read failure" : null);
    const key = `${locale}-strict-switch-${outcome}`;
    await draw(key);
    await section("world", false);
    lifecycle(panel().getAttribute("aria-busy") === "true", `old instance native ${outcome} is held before switching`);
    // Retain the actual app coordinator while remounting the provider by its
    // instance key; the old native operation retains its captured read gate.
    currentReadGate = null;
    details = { ...structuredClone(initialInstance), summary: { ...initialInstance.summary, id: `new-${locale}-${outcome}` } };
    current = { ...initialSnapshot("Custom"), world_name: "New instance world" };
    await draw(key);
    await section("world");
    await act(async () => { late.release(); await frame(); });
    await until(() => completedReads.some((read) => read.instanceId === oldInstanceId && read.result === outcome),
      "Old instance native read did not settle after gate release");
    lifecycle(modeSelect().value === "Custom" && panel().getAttribute("aria-busy") === "false" &&
      panel().querySelector("h4")?.textContent === "New instance world" && !panel().textContent?.includes("Synthetic old instance read failure"),
      `late old-instance native ${outcome} cannot replace the selected world's rules, loading or error`);
  }

  details = structuredClone(initialInstance);
  current = initialSnapshot();
  const unmountedRead = holdNativeReads();
  const key = `${locale}-strict-unmount-reading`;
  await draw(key);
  await section("world", false);
  lifecycle(panel().getAttribute("aria-busy") === "true", "native read is pending before editor unmount");
  workspaceVisible = false;
  await renderFixture(key);
  let startBarrierSettled = false;
  const startBarrier = flushSettings().then(() => { startBarrierSettled = true; });
  await until(() => startBarrierSettled, "Detached read-only editor kept the start barrier waiting for native read");
  await startBarrier;
  lifecycle(writeCalls().length === writesBefore && outstandingReadGates.has(unmountedRead),
    "unmounting a read-only editor does not write defaults or make start await its unresolved read");
  await act(async () => { unmountedRead.release(); await frame(); });
  workspaceVisible = true;
  current = { ...initialSnapshot(), status: "empty", world_file: null, world_name: null, world_mode: null,
    revision: null, values: {}, overrides: {}, writable: false };
  await draw(`${locale}-strict-empty`);
  await section("world");
  lifecycle(panel().getAttribute("aria-busy") === "false" && !panel().querySelector("select,[data-dragonwilds-setting]"),
    "StrictMode empty save leaves loading without fabricating rules");
  lifecycle(writeCalls().length === writesBefore && instanceSettingsWrites.length === 0,
    "all read lifecycle scenarios produce zero native or instance writes");
}

async function run() {
  await act(prepareBrowserLocaleCatalogs);
  check(definitions.length === 66 && editableDefinitions.length === 63, "Fixture uses the full native contract without creation-only controls");
  for (const locale of locales) {
    localStorage.setItem("langame.locale", locale);
    await verifyReadLifecycle(locale);
    details = structuredClone(initialInstance);
    current = initialSnapshot();
    const firstWriteCount = writeCalls().length;
    const settingsBefore = details.settings_json;
    await draw(`${locale}-main`);
    await section("world");
    await flush();
    check(writeCalls().length === firstWriteCount && instanceSettingsWrites.length === 0 && details.settings_json === settingsBefore,
      `${locale}: opening world editing creates no default writes or settings_json changes`);
    check(modeSelect().value === "Normal" && primaryControls().length === 2 &&
      [...panel().querySelectorAll<HTMLElement>("[data-dragonwilds-setting]")].every((field) => worldEvents.includes(field.dataset.dragonwildsSetting!)),
      `${locale}: world owns the concrete saved mode and exactly its two events`);
    check(panel().querySelectorAll("select").length === 1 && !panel().querySelector('input[type=search],button.primary-button'),
      `${locale}: native world rules use automatic saving without category, search or manual save controls`);
    check(!panel().textContent?.includes(locale === "zh-CN" ? "没有待保存更改" : "no changes to save") &&
      !panel().textContent?.includes(locale === "zh-CN" ? "已有单项覆盖" : "Existing overrides"),
      `${locale}: unchanged and mode permission explanations do not clutter the editor`);
    await verifyCategories(locale);
    check(!fixture?.querySelector('[data-dragonwilds-setting="Difficulty.Player.NoBuildingStability"]'),
      `${locale}: creation-only building stability has no editable control`);
    await verifyNavigation(locale);
    check(control(processing).value === "1.7" && !control(processing).disabled,
      `${locale}: an existing override is concrete and editable in Normal mode`);
    const nativeProcessing = definitions.find((entry) => entry.tag === processing)!;
    const numeric = control(processing);
    check(numeric.min === String(nativeProcessing.minimum) && numeric.max === String(nativeProcessing.maximum) &&
      numeric.step === String(10 ** -nativeProcessing.decimal_places), `${locale}: numeric bounds and precision come from native definitions`);
    const range = element<HTMLInputElement>(`[data-dragonwilds-setting="${processing}"] input[type=range]`, panel());
    check(!range.disabled && range.min === numeric.min && range.max === numeric.max && range.step === numeric.step,
      `${locale}: slider is enabled and carries its own native bounds and step`);
    check(!range.parentElement?.textContent?.includes(locale === "zh-CN" ? "步长" : "Step"), `${locale}: slider fields omit redundant range and step hints`);
    await setValue(numeric, "3.1");
    check(control(processing).getAttribute("aria-invalid") === "true" && Boolean(await flushError()),
      `${locale}: an out-of-range draft rejects the start barrier`);
    check(writeCalls().length === firstWriteCount, `${locale}: out-of-range controls do not issue native writes`);
    await setValue(control(processing), "1.23");
    check(Boolean(await flushError()) && writeCalls().length === firstWriteCount, `${locale}: unsupported precision rejects persistence without writing`);
    await setValue(control(processing), "0.8");
    const originalRevision = current.revision;
    await until(() => current.values[processing] === 0.8 && panel().getAttribute("aria-busy") === "false", "Debounced native save did not finish");
    const firstWrite = writeInput(writeCalls().at(-1)!.args);
    check(writeCalls().length === firstWriteCount + 1 && firstWrite.instance_id === details.summary.id &&
      firstWrite.world_file === "DedicatedWorld.sav" && firstWrite.expected_revision === originalRevision && firstWrite.world_mode === "Custom" &&
      Object.keys(firstWrite.values).length === 1 && firstWrite.values[processing] === 0.8,
      `${locale}: editing a preset rule automatically enters Custom and saves only the changed value with identity and revision`);
    check(current.overrides[unknownRule] === 19 && current.world_mode === "Custom", `${locale}: automatic native readback retains unknown rules`);
    for (const entry of editableDefinitions.filter((entry) => entry.tag !== processing)) {
      check(current.values[entry.tag] === entry.preset_defaults.Normal,
        `${locale}: automatic Custom transition preserves the effective ${entry.tag} rule`);
    }
    await section("world");
    check(modeSelect().value === "Custom", `${locale}: overview reflects the current Custom mode after an individual edit`);
    await section("world_player");
    await act(async () => { control(friendlyFire).labels![0].click(); });
    await section("room");
    check(!fixture?.querySelector("[data-dragonwilds-world-editor]"), `${locale}: inactive native panels unmount while the shared draft stays owned`);
    await flush();
    check(current.values[friendlyFire] === 1 && writeCalls().length === firstWriteCount + 2 && instanceSettingsWrites.length === 0,
      `${locale}: start barrier flushes a native draft after navigation without writing instance settings`);
    await draw(`${locale}-reopened`);
    await section("world_player");
    check(control(friendlyFire).checked, `${locale}: reopening reads the automatically persisted boolean`);
    await section("world_crafting");
    check(control(processing).value === "0.8", `${locale}: reopening reads the automatically persisted number`);
    const beforePointer = Number(control(processing).value);
    await pointerClick(element<HTMLInputElement>(`[data-dragonwilds-setting="${processing}"] input[type=range]`, panel()), 0.6);
    check(Number(control(processing).value) !== beforePointer, `${locale}: a real browser pointer changes the range control and linked numeric value`);
    await flush();
    check(current.values[processing] === Number(control(processing).value), `${locale}: real slider interaction reaches native automatic persistence`);
    await setValue(control(processing), "0.8");
    await flush();
    await section("world");
    await select(modeSelect(), "Hard");
    await flush();
    const modeWrite = writeInput(writeCalls().at(-1)!.args);
    check(modeWrite.world_mode === "Hard" && Object.keys(modeWrite.values).length === 0 && current.overrides[unknownRule] === 19 &&
      current.overrides[processing] === 0.8, `${locale}: changing only mode saves an empty patch and preserves retained overrides`);
    await section("world_crafting");
    check(control(processing).value === "0.8" && !control(processing).disabled,
      `${locale}: another preset still displays and allows editing its retained effective override`);
    await setValue(control(processing), "2");
    current.revision = `fixture-external-change-${++revision}`;
    const conflictWrites = writeCalls().length;
    check((await flushError()).includes("World settings conflict"), `${locale}: compare-and-swap failure rejects the start barrier`);
    check(writeCalls().length === conflictWrites + 1 && panel().textContent?.includes("World settings conflict") && control(processing).value === "2",
      `${locale}: native conflict keeps the unsaved draft visible`);
    await elapsed(850);
    check(writeCalls().length === conflictWrites + 1 && Boolean(await flushError()) && writeCalls().length === conflictWrites + 1,
      `${locale}: a failed native write is not blindly retried by debounce or the start barrier`);
    check(refreshButton().textContent?.includes(locale === "zh-CN" ? "放弃更改" : "Discard changes"), `${locale}: dirty refresh announces draft discard`);
    failNextRead = true;
    await clickRefresh();
    check(panel().textContent?.includes("Synthetic world read failure") && control(processing).value === "2",
      `${locale}: a failed explicit refresh preserves the complete draft`);
    await clickRefresh();
    check(control(processing).value === "0.8" && current.world_mode === "Hard", `${locale}: successful explicit refresh replaces the rejected draft with native facts`);
    let releaseWrite: () => void = () => { throw new Error("Write gate was not initialized"); };
    nextWriteGate = new Promise<void>((resolve) => { releaseWrite = resolve; });
    await setValue(control(processing), "0.9");
    const mergeWrites = writeCalls().length;
    let pendingFlush: Promise<void> = Promise.resolve();
    await act(async () => { pendingFlush = flushSettings(); await frame(); });
    await until(() => writeCalls().length === mergeWrites + 1, "Held native save was not dispatched");
    check(!control(processing).disabled, `${locale}: an in-flight native save leaves valid controls editable`);
    await setValue(control(processing), "1.1");
    await setValue(control(processing), "1.2");
    await act(async () => { releaseWrite(); await pendingFlush; await frame(); });
    await until(() => current.values[processing] === 1.2 && panel().getAttribute("aria-busy") === "false", "Edits made during native save were lost");
    check(writeCalls().length === mergeWrites + 2 && control(processing).value === "1.2" && current.overrides[unknownRule] === 19,
      `${locale}: edits made during a save merge into one follow-up write and survive the older readback`);
    const mergedWrite = writeInput(writeCalls().at(-1)!.args);
    check(mergedWrite.expected_revision !== writeInput(writeCalls().at(-2)!.args).expected_revision,
      `${locale}: the follow-up write uses the accepted revision of the first save`);
    const beforeModeFlight = { ...current.values };
    const modeFlightWrites = writeCalls().length;
    nextWriteGate = new Promise<void>((resolve) => { releaseWrite = resolve; });
    await setValue(control(processing), "1.5");
    await act(async () => { pendingFlush = flushSettings(); await frame(); });
    await until(() => writeCalls().length === modeFlightWrites + 1, "Custom mode-flight save was not dispatched");
    await setValue(control(processing), "1.8");
    await section("world", false);
    await select(modeSelect(), "Hard");
    await act(async () => { releaseWrite(); await pendingFlush; await frame(); });
    check(writeCalls().length === modeFlightWrites + 2 && current.world_mode === "Hard" && current.values[processing] === 1.5,
      `${locale}: an in-flight Custom save is followed by its ordered Hard mode transition`);
    const hardModeWrite = writeInput(writeCalls().at(-1)!.args);
    check(hardModeWrite.world_mode === "Hard" && !Object.hasOwn(hardModeWrite.values, processing) &&
      Object.keys(hardModeWrite.values).every((tag) => definitions.find((entry) => entry.tag === tag)?.player_adjustable === "AllModes"),
      `${locale}: the Hard mode patch contains no rule forbidden by the native mode contract`);
    await select(modeSelect(), "Custom");
    await section("world_crafting");
    await flush();
    check(control(processing).value === "1.8" && current.world_mode === "Custom" && current.values[processing] === 1.8,
      `${locale}: returning to Custom preserves and saves the later draft after both older mode replies`);
    check(current.overrides[unknownRule] === 19, `${locale}: mode replies retain the unknown native rule`);
    for (const entry of editableDefinitions.filter((entry) => entry.tag !== processing)) {
      check(current.values[entry.tag] === beforeModeFlight[entry.tag], `${locale}: mode-flight draft merging preserves ${entry.tag}`);
    }
    await setValue(control(processing), "1.4");
    const protectedWriteCount = writeCalls().length;
    for (const [name, status, processCount, activeRun] of [
      ["running", "Running", 1, { run_id: 7, pid: 701 }], ["active-run", "Stopped", 0, { run_id: 7, pid: 701 }],
      ["active-process", "Stopped", 1, null]
    ] as const) {
      details = { ...details, summary: { ...details.summary, status, active_process_count: processCount }, active_run: activeRun };
      await draw(`${locale}-reopened`);
      check(primaryControls().length > 0 && primaryControls().every((input) => input.disabled), `${locale}: ${name} state disables native editing`);
      await flushError();
      check(writeCalls().length === protectedWriteCount && control(processing).value === "1.4", `${locale}: ${name} state cannot write or discard the draft`);
    }
    await elapsed(850);
    check(writeCalls().length === protectedWriteCount, `${locale}: debounce cannot bypass the active-process safety guard`);
    details = structuredClone(initialInstance);
    await draw(`${locale}-reopened`);
    await flush();
    check(current.values[processing] === 1.4, `${locale}: returning to a stopped instance can flush the retained draft`);
    const guardWriteCount = writeCalls().length;
    current.writable = false;
    current.message = locale === "zh-CN" ? "当前世界只读，请刷新后检查状态。" : "The current world is read-only; refresh to check its state.";
    await draw(`${locale}-readonly`);
    await section("world_crafting");
    check(primaryControls().length > 0 && primaryControls().every((input) => input.disabled), `${locale}: native read-only snapshot disables editing`);
    await flushError();
    current.writable = true;
    current.message = null;
    await draw(`${locale}-caller-disabled`, undefined, true);
    check(primaryControls().length > 0 && primaryControls().every((input) => input.disabled), `${locale}: caller-disabled native controls cannot mutate the world`);
    check(writeCalls().length === guardWriteCount, `${locale}: both read-only guards issue zero writes`);
    const archiveReadCount = calls.length;
    await draw(`${locale}-archive`, archiveDetails());
    check(!fixture?.querySelector("[data-dragonwilds-world-editor]") && calls.length === archiveReadCount,
      `${locale}: archive uses retained instance settings without accessing live world files`);
    current = { ...initialSnapshot(), status: "empty", world_file: null, world_name: null, world_mode: null,
      revision: null, values: {}, overrides: {}, writable: false };
    await draw(`${locale}-empty`);
    await section("world");
    check(!panel().querySelector("[data-dragonwilds-setting],select,button.primary-button") &&
      panel().textContent?.includes(locale === "zh-CN" ? "启动一次" : "Start the server once"), `${locale}: empty world shows initialization guidance without fabricated controls`);
    current = initialSnapshot();
    // StrictMode replays initial reads; both transport requests encounter the
    // same failing save rather than failing only the already discarded read.
    const initialFailure = holdNativeReads("Synthetic world read failure");
    await draw(`${locale}-read-failure`);
    await section("world", false);
    await act(async () => { initialFailure.release(); await frame(); });
    await until(() => panel().getAttribute("aria-busy") === "false", "Initial native read failure left the editor loading");
    check(panel().textContent?.includes("Synthetic world read failure") && !panel().querySelector("select"),
      `${locale}: initial read failure shows the real error without guessed rules`);
    await clickRefresh();
    check(modeSelect().value === "Normal", `${locale}: explicit refresh recovers the native world after a failed initial read`);
    check(writeCalls().length === guardWriteCount && instanceSettingsWrites.length === 0 && details.settings_json === settingsBefore,
      `${locale}: all world scenarios leave instance settings and runtime data creation untouched`);
    const presetRule = editableDefinitions.find((entry) => entry.tag !== processing && entry.preset_defaults.Normal !== entry.preset_defaults.Hard)!;
    await select(modeSelect(), "Hard");
    await select(modeSelect(), "Custom");
    await section(expectedSection(presetRule.tag));
    const presetControl = control(presetRule.tag);
    check(presetRule.kind === "boolean" ? presetControl.checked === (presetRule.preset_defaults.Hard === 1) :
      Number(presetControl.value) === presetRule.preset_defaults.Hard, `${locale}: entering Custom before preset autosave preserves the displayed Hard rules`);
    await flush();
    check(current.world_mode === "Custom" && current.values[presetRule.tag] === presetRule.preset_defaults.Hard &&
      current.overrides[unknownRule] === 19, `${locale}: rapid preset-to-Custom changes persist the retained preview without losing unknown rules`);
  }
  details = structuredClone(initialInstance);
  current = initialSnapshot("Custom");
  await draw("zh-CN-final-preview");
  await section("world_crafting");
  verifyLabelsAndLayout("zh-CN");
  check(errors.length === 0, "Browser and React report no console errors");
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  return { status: "passed", locales, check_count: checks.length, checks: checks.slice(-20), loading_lifecycle_checks: loadingLifecycleChecks,
    browser_errors: errors, overflow_violations: overflowViolations };
}
Object.assign(window, { __reliabilityFixtureCleanup: async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  for (const gate of outstandingReadGates) gate.release();
  await act(async () => { root.unmount(); });
  console.error = originalError;
  return { browser_errors: errors, native_dialogs: 0 };
} });
void run().catch((cause) => ({ status: "failed", error: String(cause?.stack ?? cause), check_count: checks.length, checks: checks.slice(-40),
  loading_lifecycle_checks: loadingLifecycleChecks, browser_errors: errors, overflow_violations: overflowViolations }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
