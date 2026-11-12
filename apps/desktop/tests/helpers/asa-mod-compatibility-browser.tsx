import React, { act, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { readInstanceDetails, updateInstance } from "../../src/api";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import { buildMockPorts, buildMockSettingsForModule } from "../../src/api-mock/module-settings";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { I18nProvider } from "../../src/i18n";
import type { InstanceDetails, ManualModInventoryResult, UpdateInstanceInput } from "../../src/types";
import { ModWorkbench } from "../../src/views/servers/ModWorkbench";
import { ConfigurationWorkspace } from "../../src/views/settings/ConfigurationWorkspace";
import { InstanceSettingsSaveProvider, useInstanceSettingsSaveCoordinator } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceSettingsSaveCoordinator } from "../../src/views/settings/instance-settings-save-coordinator";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import { createModWorkbenchFixtureIpc } from "./mod-workbench-browser-support";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const zh = innerWidth < 1000;
localStorage.setItem("langame.locale", zh ? "zh-CN" : "en-US");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:16px;box-sizing:border-box;display:flex;flex-direction:column";
const root = createRoot(fixture), moduleDetails = buildMockModuleDetails("arksurvivalascended");
const checks: string[] = [], errors: string[] = [], writes: Record<string, unknown>[] = [];
let stored: InstanceDetails, details: InstanceDetails, inventory: ManualModInventoryResult, epoch = 0, saves = 0;
let coordinator: InstanceSettingsSaveCoordinator;
let pending: Promise<void> | null = null, failSave = false, saveAttempts = 0;
let view: "mods" | "settings" = "mods", requestedField: string | undefined;
const initialFlags = '-NoBattlEye -Note="keep -pconlymods"';
const activeMods = "1786680\n947033\n877752", passiveMods = "935408";
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
  await act(async () => root.unmount()); return { browser_errors: errors, native_dialogs: 0 };
} });
function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(`${label}: ${fixture.textContent}`); checks.push(label);
}
function element<T extends HTMLElement>(selector: string): T {
  const found = fixture.querySelector<T>(selector); if (!found) throw new Error(`Missing ${selector}`); return found;
}
function button(label: string): HTMLButtonElement {
  const found = [...fixture.querySelectorAll<HTMLButtonElement>("button")].find((entry) => entry.textContent?.trim() === label);
  if (!found) throw new Error(`Missing button ${label}: ${fixture.textContent}`); return found;
}
const select = () => element<HTMLSelectElement>("[data-asa-platform-mode]");
const settings = () => JSON.parse(stored.settings_json) as Record<string, unknown>;
function replaceSettings(patch: Record<string, unknown>) { stored = { ...stored, settings_json: JSON.stringify({ ...settings(), ...patch }) }; }
async function onSave(input: UpdateInstanceInput, options?: { expectedSettingsJson?: string }) {
  const saved = await updateInstance(input, options?.expectedSettingsJson ?? "");
  draw(); return saved;
}
function Workspace() {
  const [noticeTarget, setNoticeTarget] = useState<HTMLDivElement | null>(null);
  const currentCoordinator = useInstanceSettingsSaveCoordinator();
  useEffect(() => { coordinator = currentCoordinator; }, [currentCoordinator]);
  return <ActivityNoticeTarget.Provider value={{ element: noticeTarget, dismissLabel: "Close" }}>
    <main style={{ flex: 1, minHeight: 0 }}>
      {view === "mods" ? <ModWorkbench details={details} moduleDetails={moduleDetails} launchPlan={null}
        onSaveSettings={onSave} onOpenSettings={(key) => { requestedField = key; view = "settings"; draw(); }} />
        : <ConfigurationWorkspace details={details} moduleDetails={moduleDetails} moduleDetailsError={null}
          onRetryModuleDetails={() => {}} bindAddressCandidates={[]} runtime={null} launchPlan={null} launchPlanError={null}
          requestedFieldKey={requestedField} onFieldNavigationHandled={() => { requestedField = undefined; draw(); }}
          onSave={onSave} />}
    </main>
    <footer className="shell-activity-bar"><span className="shell-activity-label">Activity</span>
      <div className="shell-activity-notices" ref={setNoticeTarget} />
    </footer>
  </ActivityNoticeTarget.Provider>;
}
function draw() {
  root.render(<I18nProvider><InstanceSettingsSaveProvider key={epoch}><Workspace /></InstanceSettingsSaveProvider></I18nProvider>);
}
async function settle(predicate: () => boolean, label: string) {
  const deadline = performance.now() + 6000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(`${label}: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(target: HTMLElement) {
  target.scrollIntoView({ block: "nearest" });
  const rect = target.getBoundingClientRect();
  assert(rect.width > 0 && rect.left >= -1 && rect.right <= innerWidth + 1 && rect.top >= -1 && rect.bottom <= innerHeight + 1,
    "Control is visible in the viewport");
  assert(!(target instanceof HTMLButtonElement) || !target.disabled, "Control is enabled");
  await act(async () => { target.focus(); target.click(); });
}
async function choose(mode: string) {
  assert(!select().disabled, "Platform selector is enabled");
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!.call(select(), mode);
    select().dispatchEvent(new Event("change", { bubbles: true }));
  });
}
async function mount(nextView: "mods" | "settings" = "settings") {
  details = structuredClone(stored); epoch += 1; view = nextView;
  requestedField = nextView === "settings" ? "server_platform" : undefined;
  await act(async () => draw());
  if (nextView === "settings") {
    await settle(() => Boolean(fixture.querySelector("[data-asa-platform-mode]")), "Platform setting did not mount");
  } else {
    await settle(() => Boolean(fixture.querySelector(".mw-reference-input")), "Workbench did not mount");
    await click(button(zh ? "我的 Mod" : "My Mods"));
  }
}
async function saveChoice(mode: string) {
  const before = saves; await choose(mode);
  await settle(() => saves === before + 1, "Platform setting did not autosave");
  await act(async () => { await coordinator.flush(stored.summary.id); });
}
async function flushFails() {
  let rejected = false;
  await act(async () => { try { await coordinator.flush(stored.summary.id); } catch { rejected = true; } });
  assert(rejected, "Starting remains blocked when settings cannot be saved");
}

async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const template = await readInstanceDetails("srv-dst-terminal-error");
  const instanceId = "fixture-asa-compatibility";
  stored = { ...template, summary: { ...template.summary, id: instanceId, module_id: "arksurvivalascended",
    name: "ASA platform settings", status: "Stopped", active_process_count: 0 }, active_run: null,
    ports: buildMockPorts("arksurvivalascended"),
    settings_json: JSON.stringify({ ...buildMockSettingsForModule("arksurvivalascended", "ASA platform settings", instanceId),
      server_platform: "ALL", custom_launch_flags: initialFlags, mod_ids_csv: activeMods,
      passive_mod_ids_csv: passiveMods, unrelated: "keep existing value" }) };
  inventory = { instance_id: instanceId, module_id: stored.summary.module_id, source_label: "CurseForge", target_label: "Mods/83374",
    target_path: "C:\\Fixture\\ASA\\Mods\\83374", target_exists: true, items: [] };
  const base = createModWorkbenchFixtureIpc(() => ({ details: stored, moduleDetails, inventory, catalog: [], cached: new Set() }), {
    check: assert, save: (saved) => { stored = saved; details = structuredClone(saved); saves += 1; writes.push(settings()); } });
  mockWindows("main");
  mockIPC(async (command, payload) => {
    if (command === "read_instance_ark_map_availability") return { instance_id: instanceId, module_id: "arksurvivalascended", maps: [] };
    if (command === "update_instance_record_if_current") {
      saveAttempts += 1;
      if (pending) await pending;
      if (failSave) throw new Error("Fixture configuration save rejected");
      const args = payload as { expectedSettingsJson: string };
      if (args.expectedSettingsJson !== stored.settings_json) throw new Error("Fixture configuration changed elsewhere");
    }
    return base(command, payload);
  }, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  await mount("mods");
  assert(!fixture.querySelector(".asa-mod-compatibility select, .asa-mod-compatibility input"), "Mod page has no platform controls");
  assert(saves === 0 && saveAttempts === 0, "Opening compatibility guidance never changes settings");
  assert(!fixture.querySelector(".mw-config-pane .asa-mod-compatibility")
    && !document.querySelector(".configuration-field-help-tooltip.is-visible"), "No compatibility banner or bubble is shown by default");
  const pane = element(".mw-config-pane").getBoundingClientRect();
  const list = element(".mw-config-pane > .mw-detail-layout").getBoundingClientRect();
  assert(list.top - pane.top < 24, "The Mod list has no reserved compatibility row");
  const help = element<HTMLButtonElement>(".mw-manual-toolbar [data-asa-compatibility-help]");
  const helpRect = help.getBoundingClientRect();
  assert(helpRect.width <= 24 && helpRect.height <= 24 && helpRect.right <= innerWidth, "Compatibility guidance is a small toolbar help icon");
  await act(async () => {
    const response = await fetch(`/__reliability_pointer/${new URLSearchParams(location.search).get("nonce")}`, {
      method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ type: "move", x: helpRect.left + helpRect.width / 2, y: helpRect.top + helpRect.height / 2 }) });
    assert(response.ok, "Move pointer to compatibility help");
  });
  await settle(() => Boolean(document.querySelector(".configuration-field-help-tooltip.is-visible")), "Hover did not open compatibility help");
  const bubble = document.querySelector<HTMLElement>(".configuration-field-help-tooltip.is-visible")!;
  const bubbleRect = bubble.getBoundingClientRect();
  assert(bubble.textContent?.includes("Steam") && bubble.textContent?.includes(zh ? "点击问号" : "Click the question mark"),
    "Compatibility and configuration instructions live in the bubble");
  assert(bubbleRect.left >= 0 && bubbleRect.right <= innerWidth && bubbleRect.top >= 0 && bubbleRect.bottom <= innerHeight,
    "Help bubble stays inside the viewport");
  assert(element(".mw-config-pane > .mw-detail-layout").getBoundingClientRect().top === list.top && saves === 0,
    "Reading help neither moves the list nor writes settings");
  await act(async () => { document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); });
  assert(!document.querySelector(".configuration-field-help-tooltip"), "Escape dismisses compatibility help");
  await click(element("[data-asa-open-settings]"));
  await settle(() => Boolean(fixture.querySelector("[data-asa-platform-mode]")), "Configuration did not open");
  assert(document.activeElement === select() && select().value === "crossplay", "Navigation focuses the existing platform setting");
  assert(fixture.querySelectorAll('[data-field-key="server_platform"]').length === 1, "Configuration has one platform setting");
  assert(saves === 0 && saveAttempts === 0, "Navigation does not save settings");

  let release!: () => void;
  pending = new Promise<void>((resolve) => { release = resolve; });
  await choose("steam");
  assert(element(".asa-platform-settings").textContent?.includes("Xbox") && element(".asa-platform-settings").textContent?.includes("PS5"),
    "Steam-only restrictions are visible with the choice");
  let flushed = false;
  let flush!: Promise<void>;
  await act(async () => { flush = coordinator.flush(stored.summary.id).then(() => { flushed = true; }); });
  await settle(() => saveAttempts > 0, "Start barrier did not flush the configuration");
  assert(!flushed && saves === 0, "Starting waits for persistence of both linked settings");
  await act(async () => { release(); pending = null; await flush; });
  assert(settings().server_platform === "PC" && settings().custom_launch_flags === `${initialFlags} -pconlymods`,
    "Steam mode saves both native options in one update");
  assert(writes.length === 1 && settings().mod_ids_csv === activeMods && settings().passive_mod_ids_csv === passiveMods
    && settings().unrelated === "keep existing value", "Linked platform changes preserve four Mods and unrelated configuration");
  await saveChoice("crossplay");
  assert(settings().server_platform === "ALL" && settings().custom_launch_flags === `${initialFlags} `,
    "Cross-platform mode removes only the actual PC-only flag");

  const beforeCustom = saves;
  await choose("custom");
  assert(saves === beforeCustom, "Opening custom platforms does not write settings");
  const custom = element<HTMLInputElement>(".asa-platform-settings__custom input");
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(custom, "PC+PS5");
    custom.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await settle(() => saves === beforeCustom + 1, "Custom platforms did not save");
  await act(async () => { await coordinator.flush(stored.summary.id); });
  assert(settings().server_platform === "PC+PS5", "Existing custom platform combinations remain editable");

  const beforeConflict = saves;
  await choose("steam"); replaceSettings({ server_platform: "PS5", unrelated: "changed elsewhere" });
  await settle(() => fixture.textContent?.includes("Fixture configuration changed elsewhere") === true, "Concurrent edit was not reported");
  await flushFails();
  assert(saves === beforeConflict && settings().server_platform === "PS5" && settings().unrelated === "changed elsewhere",
    "Compare-and-swap protects concurrent platform and unrelated edits");
  await mount();
  stored = { ...stored, summary: { ...stored.summary, status: "Running", active_process_count: 1 } }; await mount();
  assert(select().disabled, "Running instances cannot change platform access");
  stored = { ...stored, summary: { ...stored.summary, status: "Stopped", active_process_count: 0 } };
  replaceSettings({ custom_launch_flags: "-ServerPlatform=PC+PS5" }); await mount();
  assert(select().disabled && fixture.querySelector('.asa-platform-settings [role="alert"]'),
    "Raw platform overrides have an actionable explanation without being overwritten");
  replaceSettings({ custom_launch_flags: '-Note="unclosed' }); await mount();
  assert(select().disabled && fixture.querySelector('.asa-platform-settings [role="alert"]'), "Malformed flags are preserved and explained");

  replaceSettings({ server_platform: "ALL", custom_launch_flags: initialFlags }); await mount();
  failSave = true; const beforeFailure = saves; await choose("steam");
  await settle(() => fixture.textContent?.includes("Fixture configuration save rejected") === true, "Save failure was hidden");
  await flushFails();
  assert(saves === beforeFailure && settings().server_platform === "ALL", "Save failure cannot report Steam as persisted");
  failSave = false;
  await click(button(zh ? "重试保存" : "Retry save"));
  await settle(() => saves === beforeFailure + 1, "Failed configuration did not retry successfully");
  await act(async () => { await coordinator.flush(stored.summary.id); });
  assert(settings().server_platform === "PC" && settings().mod_ids_csv === activeMods
    && settings().passive_mod_ids_csv === passiveMods, "Retry persists the same linked change without losing Mods");
  const card = element(".asa-platform-settings");
  card.scrollIntoView({ block: "nearest" });
  const bounds = card.getBoundingClientRect();
  assert(bounds.left >= 0 && bounds.right <= innerWidth && bounds.top >= 0 && bounds.bottom <= innerHeight,
    "Platform settings and help fit in the viewport");
  assert(document.documentElement.scrollWidth <= innerWidth + 1, "Configuration has no horizontal overflow");
  await mount("mods");
  await act(async () => { element<HTMLButtonElement>("[data-asa-compatibility-help]").focus(); });
  await settle(() => Boolean(document.querySelector(".configuration-field-help-tooltip.is-visible")), "Keyboard focus did not reveal compatibility help");
  assert(document.querySelector(".configuration-field-help-tooltip.is-visible")?.textContent?.includes("Xbox"),
    "Help reflects the saved Steam-only mode after returning from Configuration");
  await act(async () => {
    const moved = await fetch(`/__reliability_pointer/${new URLSearchParams(location.search).get("nonce")}`, {
      method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ type: "move", x: innerWidth - 2, y: innerHeight - 2 }) });
    assert(moved.ok, "Screenshot pointer is outside the controls");
    await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
    await Promise.all([...document.querySelectorAll(".configuration-field-help-tooltip")]
      .flatMap((node) => node.getAnimations()).map((animation) => animation.finished));
  });
  assert(errors.length === 0, "No browser console or uncaught errors");
  return { status: "passed", checks, saves, configuration_only: true, help_on_demand: true, linked_platform_save: true, start_barrier: true,
    preserves_mods: true, custom_platforms: true, conflict_guard: true, running_guard: true, raw_flags_guard: true,
    failure_visible: true, layout_visible: true, browser_errors: errors };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error("ASA configuration acceptance exceeded 40 seconds")), 40000); })])
  .catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`, { method: "POST", body: JSON.stringify(report) }));
