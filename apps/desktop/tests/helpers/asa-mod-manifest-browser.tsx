import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { readInstanceDetails } from "../../src/api";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import type { InstanceDetails, ManualModInventoryResult } from "../../src/types";
import { createModWorkbenchFixtureIpc, ModWorkbenchBrowserHost } from "./mod-workbench-browser-support";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import type { InstanceSettingsSaveCoordinator } from "../../src/views/settings/instance-settings-save-coordinator";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const zh = innerWidth < 1000;
localStorage.setItem("langame.locale", zh ? "zh-CN" : "en-US");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:16px;box-sizing:border-box;display:flex;flex-direction:column";
const root = createRoot(fixture), moduleDetails = buildMockModuleDetails("arksurvivalascended");
const checks: string[] = [], errors: string[] = [], commands: string[] = [];
let stored: InstanceDetails, details: InstanceDetails, inventory: ManualModInventoryResult, epoch = 0, saves = 0;
let failSave = false, pending: Promise<void> | null = null;
let failReloadRead = false, coordinator: InstanceSettingsSaveCoordinator | null = null;
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
function button(label: string) {
  const found = [...fixture.querySelectorAll<HTMLButtonElement>("button")].find((entry) => entry.textContent?.trim() === label);
  if (!found) throw new Error(`Missing ${label}: ${fixture.textContent}`); return found;
}
function input() { const result = fixture.querySelector<HTMLTextAreaElement>(".mw-manifest-input");
  if (!result) throw new Error("ASA manifest input unavailable"); return result; }
async function settle(predicate: () => boolean) {
  const deadline = performance.now() + 8000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(`ASA manifest did not settle: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(target: HTMLElement) {
  target.scrollIntoView({ block: "nearest" }); const rect = target.getBoundingClientRect();
  assert(rect.width > 0 && rect.left >= -1 && rect.right <= innerWidth + 1, "Action remains visible inside viewport");
  assert(!(target instanceof HTMLButtonElement) || !target.disabled, "Action is enabled");
  await act(async () => { target.focus(); target.click(); });
}
async function text(value: string) {
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(input(), value);
    input().dispatchEvent(new Event("input", { bubbles: true }));
  });
}
function settings() { return JSON.parse(stored.settings_json) as Record<string, unknown>; }
function toolbarPaint() {
  return [...fixture.querySelectorAll<HTMLButtonElement>('.mw-workspace-toolbar [role="tab"]')].map((tab) => {
    const style = getComputedStyle(tab);
    return { label: tab.textContent?.trim(), selected: tab.getAttribute("aria-selected") === "true",
      hovered: tab.matches(":hover"), background: style.backgroundColor, color: style.color,
      transition_duration: style.transitionDuration, running_animations: tab.getAnimations().filter((animation) => animation.playState === "running").length };
  });
}
async function settleToolbarPaint() {
  const before = toolbarPaint();
  const nonce = new URLSearchParams(location.search).get("nonce");
  await act(async () => {
    const moved = await fetch(`/__reliability_pointer/${nonce}`, { method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ type: "move", x: innerWidth - 2, y: innerHeight - 2 }) });
    assert(moved.ok, "Screenshot pointer is moved away from the toolbar");
    await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
    const animations = [...fixture.querySelectorAll<HTMLButtonElement>('.mw-workspace-toolbar [role="tab"]')]
      .flatMap((tab) => tab.getAnimations());
    let timeout: ReturnType<typeof setTimeout> | undefined;
    try {
      await Promise.race([Promise.all(animations.map((animation) => animation.finished)),
        new Promise<never>((_, reject) => { timeout = setTimeout(() => reject(new Error("Toolbar transition did not finish")), 1500); })]);
    } finally { clearTimeout(timeout); }
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  });
  const after = toolbarPaint();
  assert(after.every((tab) => !tab.hovered && tab.running_animations === 0), "Screenshot toolbar has no pointer hover or unfinished transition");
  const selected = after.find((tab) => tab.selected);
  assert(selected?.label === label.mode && selected.background !== "rgba(0, 0, 0, 0)", "Manifest selection has a painted background");
  assert(after.filter((tab) => !tab.selected).every((tab) => tab.background === "rgba(0, 0, 0, 0)"), "Unselected tabs have no selected or hover background");
  const theme = getComputedStyle(button(label.mode));
  return { before, after, theme: { accent: theme.getPropertyValue("--shell-accent").trim(),
    control_hover: theme.getPropertyValue("--theme-control-hover").trim(), card_muted: theme.getPropertyValue("--theme-card-muted").trim() } };
}
const captureCoordinator = (value: InstanceSettingsSaveCoordinator) => { coordinator = value; };
function draw() { root.render(<ModWorkbenchBrowserHost details={details} moduleDetails={moduleDetails} epoch={epoch} onSaved={draw} onCoordinator={captureCoordinator} />); }
const label = { mode: zh ? "清单模式" : "Manifest mode", apply: zh ? "合并并启用" : "Merge and enable" };
async function mount() {
  details = structuredClone(stored); epoch += 1; await act(async () => draw());
  await settle(() => [...fixture.querySelectorAll("button")].some((entry) => entry.textContent?.trim() === label.mode));
  await click(button(label.mode)); await settle(() => Boolean(fixture.querySelector(".mw-manifest-input")));
  assert(fixture.querySelectorAll('[role="tab"][aria-selected="true"]').length === 1 &&
    button(label.mode).getAttribute("aria-selected") === "true", "Manifest is the only selected tab");
  assert(fixture.querySelectorAll(".mw-sort-pill--active").length === 1 &&
    button(label.mode).classList.contains("mw-sort-pill--active"), "Manifest is the only active toolbar pill");
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const template = await readInstanceDetails("srv-dst-terminal-error");
  stored = { ...template, summary: { ...template.summary, id: "fixture-asa-manifest", module_id: "arksurvivalascended",
    status: "Stopped", active_process_count: 0 }, active_run: null,
    settings_json: JSON.stringify({ mod_ids_csv: "1346144", passive_mod_ids_csv: "1346148", unrelated: "keep" }) };
  inventory = { instance_id: stored.summary.id, module_id: stored.summary.module_id, source_label: "CurseForge",
    target_label: "Mods/83374", target_path: "C:\\Fixture\\ASA\\Mods\\83374", target_exists: true, items: [] };
  const base = createModWorkbenchFixtureIpc(() => ({ details: stored, moduleDetails, inventory, catalog: [], cached: new Set() }), {
    check: assert, save: (saved) => { stored = saved; details = structuredClone(saved); saves += 1; } });
  mockWindows("main");
  mockIPC(async (command, payload) => {
    commands.push(command);
    if (command === "read_instance_details_from_storage" && failReloadRead) throw new Error("Controlled configuration reload read failure");
    if (command === "update_instance_record_if_current") {
      if (pending) await pending;
      if (failSave) { failSave = false; throw new Error("Fixture CAS rejected the manifest save"); }
    }
    return base(command, payload);
  }, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  await mount();
  assert(input().value === "1346144", "Current active IDs initialize the manifest without passive IDs");
  assert(!fixture.textContent?.includes("Download missing") && !fixture.textContent?.includes("下载缺失"), "ASA shows native startup semantics instead of Steam download actions");
  await text("1346145\nwrong-id");
  assert(button(label.apply).disabled, "Malformed tokens block the complete submission");
  assert(fixture.textContent?.includes("wrong-id"), "Invalid token is explained");
  await text("001346145,1346146;1346145");
  let before = saves; await click(button(label.apply));
  await settle(() => saves === before + 1 && !button(label.apply).disabled);
  assert(String(settings().mod_ids_csv).split(/[\n,;]/).join(",") === "1346144,1346145,1346146", "Bulk save preserves existing order and canonicalizes additions");
  assert(settings().passive_mod_ids_csv === "1346148" && settings().unrelated === "keep", "Passive loading and unrelated settings are retained");
  assert(fixture.textContent?.includes(zh ? "下次开服" : "next server start"), "Successful result states configuration persistence and next-start loading");
  await text("1346147"); failSave = true; before = saves; await click(button(label.apply));
  await settle(() => Boolean(fixture.textContent?.includes("Fixture CAS rejected")));
  assert(saves === before && input().value === "1346147", "CAS failure retains the draft and does not report a saved ID");
  let release!: () => void; pending = new Promise<void>((resolve) => { release = resolve; });
  await click(button(label.apply)); await settle(() => input().disabled);
  assert(fixture.querySelector("section.mw-manifest")?.getAttribute("aria-busy") === "true", "Pending save is visible and locks edits");
  await act(async () => { button(zh ? "正在保存…" : "Saving…").click(); });
  assert(saves === before, "Pending mutation is not replayed");
  await act(async () => release()); pending = null; await settle(() => saves === before + 1 && !input().disabled);
  stored = { ...stored, settings_json: JSON.stringify({ ...settings(), custom_launch_flags: "-mods=1346144" }) };
  await mount(); await text("1346149");
  assert(button(label.apply).disabled, "Raw native Mod flags disable manifest application");
  stored = { ...stored, settings_json: JSON.stringify({ ...settings(), custom_launch_flags: "" }),
    summary: { ...stored.summary, status: "Running", active_process_count: 1 } };
  await mount(); assert(button(label.apply).disabled, "Running instance disables manifest writes");
  stored = { ...stored, summary: { ...stored.summary, status: "Stopped", active_process_count: 0 } };
  await mount();
  assert(input().value.split("\n").includes("1346147"), "Saved membership survives remount");
  const previousSettings = stored.settings_json;
  const enabledA = "1346144", inventoryB = "1254808";
  const configurationA = { game_ini: "[FixtureA]\nRetained=yes\n", game_user_settings_ini: "" };
  stored = { ...stored, settings_json: JSON.stringify({ ...settings(), asa_mod_configurations: {
    [enabledA]: configurationA, [inventoryB]: { game_ini: "", game_user_settings_ini: "[BetterVariants]\nDebug=False\n" },
  } }) };
  inventory = { ...inventory, items: [{ name: "Better Variants inventory fixture", inferred_id: inventoryB,
    path: `${inventory.target_path}\\BetterVariants`, item_type: "directory", file_count: 1, total_bytes: 64 }] };
  await mount(); await click(button(zh ? "我的 Mod" : "My Mods"));
  await settle(() => fixture.querySelector(`[data-asa-mod-config="${enabledA}"]`) !== null);
  const inventoryButton = () => [...fixture.querySelectorAll<HTMLButtonElement>(".mw-entry-row-select")]
    .find((row) => row.querySelector(".mw-entry-row-title")?.textContent === "Better Variants inventory fixture");
  await settle(() => Boolean(inventoryButton())); await click(inventoryButton()!);
  await settle(() => fixture.querySelector(`[data-asa-mod-config="${inventoryB}"]`) !== null);
  assert(fixture.querySelector("[data-asa-mod-config]")?.getAttribute("data-asa-mod-config") === inventoryB,
    "Selecting installed inventory B must not fall back to enabled Mod A's configuration");
  const debug = fixture.querySelector<HTMLSelectElement>('[data-asa-mod-parameter="Debug"] select');
  assert(debug, "Inventory B exposes its own verified author configuration");
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!.call(debug, "True");
    debug.dispatchEvent(new Event("change", { bubbles: true }));
  });
  const beforeConfigurationSave = saves;
  await click(fixture.querySelector<HTMLButtonElement>("[data-asa-mod-save]")!);
  await settle(() => saves === beforeConfigurationSave + 1);
  const savedConfigurations = settings().asa_mod_configurations as Record<string, { game_ini: string; game_user_settings_ini: string }>;
  assert(JSON.stringify(savedConfigurations[enabledA]) === JSON.stringify(configurationA), "Saving inventory B keeps enabled A's configuration unchanged");
  assert(savedConfigurations[inventoryB].game_user_settings_ini.includes("Debug=True")
    && savedConfigurations[inventoryB].game_ini === "", "Inventory B save writes only B's declared destination");
  assert(settings().mod_ids_csv === JSON.parse(previousSettings).mod_ids_csv
    && settings().passive_mod_ids_csv === JSON.parse(previousSettings).passive_mod_ids_csv,
    "Per-Mod configuration save does not enable or change loading membership");
  async function flushError() {
    let failure = "";
    await act(async () => { try { await coordinator!.flush(stored.summary.id); } catch (error) { failure = String(error); } });
    return failure;
  }
  async function reloadConfiguration() {
    await click(fixture.querySelector<HTMLButtonElement>("[data-asa-mod-reload]")!);
    await click(fixture.querySelector<HTMLButtonElement>(".inline-confirm-submit")!);
    await settle(() => fixture.querySelector(".asa-mod-config")?.getAttribute("aria-busy") === "false");
  }
  async function changeDebug(value: string) {
    const field = fixture.querySelector<HTMLSelectElement>('[data-asa-mod-parameter="Debug"] select')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!.call(field, value);
      field.dispatchEvent(new Event("change", { bubbles: true }));
    });
  }
  const configurationInput = () => fixture.querySelector<HTMLTextAreaElement>(".asa-mod-config__input")!;
  const configurationSave = () => fixture.querySelector<HTMLButtonElement>("[data-asa-mod-save]")!;
  await changeDebug("False"); failSave = true; await click(configurationSave());
  await settle(() => fixture.textContent?.includes("Fixture CAS rejected") ?? false);
  assert((await flushError()).includes("Fixture CAS rejected"), "Failed configuration save blocks the real start coordinator");
  const latestConfiguration = "[BetterVariants]\nDebug=False\nDebugSpawns=False\n[ReadbackFixture]\nKeep=authority\n";
  stored = { ...stored, settings_json: JSON.stringify({ ...settings(), asa_mod_configurations: {
    ...savedConfigurations, [inventoryB]: { game_ini: "", game_user_settings_ini: latestConfiguration },
  } }) };
  await reloadConfiguration();
  assert(await flushError() === "", "Successful same-owner read releases the failed configuration start barrier");
  assert(configurationInput().value === latestConfiguration, "Reload reads storage instead of stale Workbench props");
  await changeDebug("True"); before = saves; await click(configurationSave());
  await settle(() => saves === before + 1);
  assert((settings().asa_mod_configurations as typeof savedConfigurations)[inventoryB].game_user_settings_ini.includes("Keep=authority"),
    "Immediate save after reload uses the authoritative readback baseline");
  await changeDebug("False"); failSave = true; await click(configurationSave());
  await settle(() => fixture.textContent?.includes("Fixture CAS rejected") ?? false);
  const failedDraft = configurationInput().value;
  failReloadRead = true; await reloadConfiguration();
  assert(configurationInput().value === failedDraft && (await flushError()).includes("Controlled configuration reload read failure"),
    "A failed reload preserves the draft and keeps its failed operation in the start barrier");
  failReloadRead = false;
  if (fixture.querySelector(".inline-confirm-submit")) await click(fixture.querySelector<HTMLButtonElement>(".inline-confirm-submit")!);
  else await reloadConfiguration();
  await settle(() => fixture.querySelector(".asa-mod-config")?.getAttribute("aria-busy") === "false");
  assert(await flushError() === "", "Retrying the read successfully restores the start barrier");
  stored = { ...stored, settings_json: previousSettings }; inventory = { ...inventory, items: [] }; await mount();
  assert(!commands.some((command) => /download_steam|lookup_steam|install_manual_mod_references/.test(command)), "Manifest makes no local download or Steam lookup claims");
  assert(document.documentElement.scrollWidth <= innerWidth + 1, "Manifest does not overflow horizontally");
  const toolbar_paint = await settleToolbarPaint();
  let configuration_layout: { selected_mod: string | null; panel: { left: number; right: number; width: number; height: number } } | undefined;
  if (document.querySelector('meta[name="asa-fixture-final-pane"]')?.getAttribute("content") === "configuration") {
    stored = { ...stored, settings_json: JSON.stringify({ ...settings(), asa_mod_configurations: {
      [enabledA]: configurationA, [inventoryB]: { game_ini: "", game_user_settings_ini: "[BetterVariants]\nDebug=True\nDebugSpawns=False\n" },
    } }) };
    inventory = { ...inventory, items: [{ name: "Better Variants inventory fixture", inferred_id: inventoryB,
      path: `${inventory.target_path}\\BetterVariants`, item_type: "directory", file_count: 1, total_bytes: 64 }] };
    await mount(); await click(button(zh ? "我的 Mod" : "My Mods"));
    await settle(() => Boolean(inventoryButton())); await click(inventoryButton()!);
    await settle(() => fixture.querySelector(`[data-asa-mod-config="${inventoryB}"]`) !== null);
    await act(async () => {
      for (const panel of fixture.querySelectorAll<HTMLElement>(".mw-detail-config-panel, .mw-selected-config-panel")) panel.scrollTop = 0;
      const nonce = new URLSearchParams(location.search).get("nonce");
      const moved = await fetch(`/__reliability_pointer/${nonce}`, { method: "POST", headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ type: "move", x: innerWidth - 2, y: innerHeight - 2 }) });
      assert(moved.ok, "Configuration screenshot pointer leaves the toolbar");
      const animations = [...fixture.querySelectorAll<HTMLButtonElement>('.mw-workspace-toolbar [role="tab"]')].flatMap((tab) => tab.getAnimations());
      await Promise.all(animations.map((animation) => animation.finished));
      await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    });
    const panel = fixture.querySelector<HTMLElement>(".mw-detail-config-panel")!, bounds = panel.getBoundingClientRect();
    configuration_layout = { selected_mod: fixture.querySelector("[data-asa-mod-config]")?.getAttribute("data-asa-mod-config") ?? null,
      panel: { left: bounds.left, right: bounds.right, width: bounds.width, height: bounds.height } };
    assert(configuration_layout.selected_mod === inventoryB && bounds.width >= 200 && bounds.left >= 0 && bounds.right <= innerWidth,
      "Installed B configuration stays inside the actual Workbench right column");
    assert(panel.scrollWidth <= panel.clientWidth + 1, "The actual configuration column has no horizontal overflow");
    const configuration = fixture.querySelector<HTMLElement>(".asa-mod-config")!;
    assert(!configuration.querySelector("[data-asa-mod-clear], [data-asa-mod-reload], [data-asa-mod-destination], .asa-mod-config__dirty"),
      "The actual unchanged Workbench configuration omits redundant actions and status rows");
    const preview = configuration.querySelector<HTMLDetailsElement>(".asa-mod-config__preview")!;
    assert(!preview.open && !preview.querySelector("table")!.checkVisibility(), "The actual Workbench keeps its preview table collapsed");
  }
  assert(errors.length === 0, "No console or unhandled errors");
  return { status: "passed", checks, saves, locale: zh ? "zh-CN" : "en-US", integration: "real_workbench_with_substituted_IPC",
    toolbar_paint, configuration_layout, screenshot_view: configuration_layout ? "inventory-configuration" : "manifest",
    browser_errors: errors, native_download_executed: false };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error("ASA manifest exceeded 40 seconds")), 40000); })])
  .catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`, { method: "POST", body: JSON.stringify(report) }));
