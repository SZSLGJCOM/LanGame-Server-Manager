import React, { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { readInstanceDetails, updateInstance } from "../../src/api";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { I18nProvider } from "../../src/i18n";
import type { InstanceArchiveDetails } from "../../src/storage-management-types";
import type { InstanceDetails, ManualModInventoryResult, ModuleDetails, SteamWorkshopLookupItem } from "../../src/types";
import { ModWorkbench } from "../../src/views/servers/ModWorkbench";
import { moduleHasModWorkbench } from "../../src/views/servers/mod-workbench-capability";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import { createModWorkbenchFixtureIpc, workshopItem } from "./mod-workbench-browser-support";

// The browser renders production components and module contracts. Only native
// IPC is replaced with isolated in-memory records; no real Mod files are touched.
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const zh = innerWidth < 1000;
localStorage.setItem("langame.locale", zh ? "zh-CN" : "en-US");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:16px;box-sizing:border-box;display:flex;flex-direction:column";
const root = createRoot(fixture);
const checks: string[] = [], errors: string[] = [], sourceCalls: string[] = [];
const flags: Record<string, boolean> = {};
const games = ["dontstarve", "projectzomboid", "unturned", "arksurvivalevolved", "barotrauma", "conanexiles", "palworld", "soulmask", "squad", "terraria",
  "corekeeper", "enshrouded", "humanitz", "minecraft", "necesse", "rust", "sevendaystodie", "sonsoftheforest", "valheim", "vrising", "windrose",
  "abioticfactor", "arksurvivalascended", "astroneer", "satisfactory"];
const reviewedModules: string[] = [];
const firstId = "6472275", secondId = "6472552", thirdId = "6456360";
let details: InstanceDetails, moduleDetails: ModuleDetails, inventory: ManualModInventoryResult;
let catalog: SteamWorkshopLookupItem[] = [], epoch = 0, writes = 0, attempts = 0;
let archived = false, failSave = false, pendingSave: Promise<void> | null = null;
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  await act(async () => root.unmount()); return { browser_errors: errors, native_dialogs: 0 };
} });

function check(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(`${label}: ${fixture.textContent}`);
  if (!checks.includes(label)) checks.push(label);
}
function element<T extends HTMLElement = HTMLElement>(selector: string, within: ParentNode = fixture): T {
  const found = within.querySelector<T>(selector);
  if (!found) throw new Error(`Missing ${selector}: ${fixture.textContent}`);
  return found;
}
function button(label: string, within: ParentNode = fixture): HTMLButtonElement {
  const found = [...within.querySelectorAll<HTMLButtonElement>("button")].find((node) => node.textContent?.trim() === label);
  if (!found) throw new Error(`Missing button ${label}: ${fixture.textContent}`);
  return found;
}
const settings = () => JSON.parse(details.settings_json) as Record<string, unknown>;
const rows = () => [...fixture.querySelectorAll<HTMLElement>(".mw-entry-list > .mw-entry-row")];
function officialRow(id: string): HTMLElement {
  const found = rows().find((row) => row.querySelector(".mw-entry-id")?.textContent?.includes(`mod.io / ${id}`));
  if (!found) throw new Error(`Missing official row ${id}: ${fixture.textContent}`);
  return found;
}
const dialog = () => element<HTMLDialogElement>(".mw-source-settings-dialog");
const sourceField = (key: string) => element<HTMLElement>(`[data-field-key="${key}"]`, dialog()).closest<HTMLElement>(".mw-source-field")!;
const sourceInput = (key: string) => element<HTMLInputElement | HTMLTextAreaElement>("input:not([type=checkbox]), textarea", sourceField(key));
async function settle(predicate: () => boolean, label: string) {
  const deadline = performance.now() + 6000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(`${label}: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(target: HTMLElement) {
  target.scrollIntoView({ block: "nearest" });
  const bounds = target.getBoundingClientRect();
  check(bounds.width > 0 && bounds.height > 0 && bounds.left >= -1 && bounds.right <= innerWidth + 1
    && bounds.top >= -1 && bounds.bottom <= innerHeight + 1, "Controls remain inside the desktop viewport");
  check(!(target instanceof HTMLButtonElement) || !target.disabled, "Clicked controls are enabled");
  await act(async () => { target.focus(); target.click(); });
}
async function input(target: HTMLInputElement | HTMLTextAreaElement, value: string) {
  await act(async () => {
    const prototype = target instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(prototype, "value")!.set!.call(target, value);
    target.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
async function escape() {
  await act(async () => {
    const response = await fetch(`/__reliability_key/${new URLSearchParams(location.search).get("nonce")}/Escape`, { method: "POST" });
    check(response.ok, "Native Escape is dispatched to the fixture browser");
  });
}
function viewportFits() {
  check(document.documentElement.scrollWidth <= innerWidth + 1 && document.documentElement.scrollHeight <= innerHeight + 1,
    "Library does not overflow the desktop viewport");
  const activeDialog = fixture.querySelector<HTMLDialogElement>(".mw-source-settings-dialog[open]");
  if (activeDialog) {
    const bounds = activeDialog.getBoundingClientRect();
    check(bounds.left >= 0 && bounds.top >= 0 && bounds.right <= innerWidth && bounds.bottom <= innerHeight,
      "Advanced settings fits the desktop viewport");
  }
}
function Workspace() {
  const [noticeTarget, setNoticeTarget] = useState<HTMLDivElement | null>(null);
  const archive: InstanceArchiveDetails | undefined = archived ? {
    archive_id: "fixture-mod-archive", instance: details,
    maintenance: { autostart: false, auto_backup_on_stop: false, backup_retention_count: 2, crash_restart_limit: null, runtime_mode: null },
    runs: { entries: [], total: 0, truncated: false },
    log: { relative_path: null, text: "", truncated: false, issues: [] }, backups: { entries: [], issues: [], truncated: false }
  } : undefined;
  return <ActivityNoticeTarget.Provider value={{ element: noticeTarget, dismissLabel: "Close" }}>
    <main style={{ flex: 1, minHeight: 0 }}>
      <ModWorkbench details={details} moduleDetails={moduleDetails} launchPlan={null} archive={archive}
        onSaveSettings={async (next, options) => { const saved = await updateInstance(next, options?.expectedSettingsJson ?? ""); draw(); return saved; }} />
    </main>
    <footer className="shell-activity-bar"><span className="shell-activity-label">Activity</span>
      <div className="shell-activity-notices" ref={setNoticeTarget} /></footer>
  </ActivityNoticeTarget.Provider>;
}
function draw() {
  root.render(<I18nProvider><InstanceSettingsSaveProvider key={epoch}><Workspace /></InstanceSettingsSaveProvider></I18nProvider>);
}
function seed(moduleId = "abioticfactor", value: Record<string, unknown> = { mod_ids: `${firstId};${secondId}`, unrelated: "retain this" }, local = true) {
  archived = false;
  moduleDetails = buildMockModuleDetails(moduleId);
  check(moduleDetails.summary.id === moduleId, `${moduleId} uses the repository module contract`);
  details = { ...details, summary: { ...details.summary, id: `fixture-source-settings-${moduleId}`, module_id: moduleId,
    name: `${moduleDetails.summary.name} Mod library`, status: "Stopped", active_process_count: 0 }, active_run: null,
    settings_json: JSON.stringify(value) };
  const target = `C:\\Fixture\\${moduleId}\\Mods`;
  inventory = { instance_id: details.summary.id, module_id: moduleId,
    source_label: moduleDetails.mods?.source?.label ?? "Local Mods", target_label: moduleDetails.mods?.manual_staging?.target_label ?? "Mods",
    target_path: target, target_exists: local, items: local ? [{ name: firstId, path: `${target}\\${firstId}`,
      item_type: "directory", inferred_id: firstId, file_count: 3, total_bytes: 8192 }] : [] };
  catalog = [];
}
async function mount() {
  epoch += 1; await act(async () => draw());
  await settle(() => Boolean(fixture.querySelector(".mw-sort-pill")), "Workbench did not mount");
  await click(button(zh ? "我的 Mod" : "My Mods"));
  await settle(() => Boolean(fixture.querySelector(".mw-config-pane")), "My Mods did not open");
  if (!archived) await settle(() => !fixture.querySelector(".mw-empty[aria-busy=true]"), "Inventory did not settle");
}
async function openSettings() {
  await click(element<HTMLButtonElement>(".mw-source-settings-button"));
  await settle(() => Boolean(fixture.querySelector<HTMLDialogElement>(".mw-source-settings-dialog")?.open), "Advanced settings did not open");
  check(dialog().contains(document.activeElement), "Focus enters the native advanced settings modal");
  viewportFits();
}
async function closeSettings() {
  await click(element<HTMLButtonElement>(".mw-source-settings-close", dialog()));
  await settle(() => !fixture.querySelector<HTMLDialogElement>(".mw-source-settings-dialog")?.open, "Clean advanced settings did not close");
  check(document.activeElement === fixture.querySelector(".mw-source-settings-button"), "Closing settings restores focus to its trigger");
  flags.focus_restored = true;
}
async function removeOfficial(id: string) {
  await click(element<HTMLButtonElement>(".mw-entry-remove-button", officialRow(id)));
}
async function dragOfficial(sourceId: string, targetId: string) {
  const source = element<HTMLButtonElement>(".mw-entry-row-select", officialRow(sourceId));
  const target = element<HTMLButtonElement>(".mw-entry-row-select", officialRow(targetId));
  const from = source.getBoundingClientRect(), to = target.getBoundingClientRect();
  await act(async () => {
    const response = await fetch(`/__reliability_pointer/${new URLSearchParams(location.search).get("nonce")}`, {
      method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ type: "drag", x: from.left + from.width / 2, y: from.top + from.height / 2,
        deltaX: to.left + to.width / 2 - (from.left + from.width / 2), deltaY: to.top + to.height / 2 - (from.top + from.height / 2) }) });
    check(response.ok, "Native pointer drag reorders the official Mod rows");
  });
}
async function exercise() {
  seed("abioticfactor", {}, false); await mount();
  check(rows().length === 0 && fixture.querySelector(".mw-empty") && !fixture.querySelector(".mw-detail-config-col"),
    "An empty library has no invented rows or empty detail column"); flags.empty_library = true;
  check(!fixture.querySelector(".mw-detail-panel .mw-source-lists, .mw-detail-panel .mw-source-field, .mw-detail-panel details"),
    "The shared Mod list is not prefixed with a configuration form");
  check(Boolean(fixture.querySelector(".mw-detail-col-title .mw-source-settings-button"))
    && !fixture.querySelector<HTMLDialogElement>(".mw-source-settings-dialog")?.open,
    "Advanced settings starts as an on-demand list-header action"); flags.advanced_on_demand = true;

  seed(); await mount();
  await settle(() => rows().length === 3, "Official and local Mods did not share one library");
  check(fixture.querySelector(".mw-entry-count-chip")?.textContent === "3", "Library count includes official IDs and local files");
  for (const id of [firstId, secondId]) {
    check(!officialRow(id).querySelector('input[type="checkbox"], img'), "Official IDs do not invent installation toggles or Steam images");
  }
  check(rows().filter((row) => row.querySelector(".mw-entry-row-title")?.textContent === firstId).length === 2,
    "A same-named UE4SS file is retained independently of the official mod.io ID");
  flags.native_membership = flags.separate_file_inventory = true;
  const officialSelect = element<HTMLButtonElement>(".mw-entry-row-select", officialRow(firstId));
  const helpId = officialSelect.getAttribute("aria-describedby");
  const officialHelp = helpId ? document.getElementById(helpId)?.textContent : null;
  check(officialHelp && /next.*start|下次启动/i.test(officialHelp), "Official row guidance explains when native Mods load");
  check(!fixture.querySelector(".mw-native-mod-note"), "Valid official Mods have no permanent instruction paragraph");
  await act(async () => officialSelect.focus());
  await settle(() => [...document.querySelectorAll(".configuration-field-help-tooltip.is-visible")]
    .some((node) => node.textContent === officialHelp), "Official row guidance did not appear on keyboard focus");
  flags.native_help_on_demand = true;
  await act(async () => officialSelect.blur());
  await settle(() => !document.querySelector(".configuration-field-help-tooltip"), "Official row guidance did not dismiss after leaving focus");
  await dragOfficial(secondId, firstId);
  await settle(() => settings().mod_ids === `${secondId}\n${firstId}`, "Official load order did not persist");
  check(settings().unrelated === "retain this", "Reordering preserves unrelated settings"); flags.load_order = true;
  await removeOfficial(firstId);
  await settle(() => settings().mod_ids === secondId && rows().length === 2, "Official removal did not update the shared library");
  check(JSON.stringify(settings()) === JSON.stringify({ mod_ids: secondId, unrelated: "retain this" }) && inventory.items.length === 1,
    "Removing an official ID changes only its list and leaves local files intact"); flags.remove_preserves_settings = true;

  await openSettings();
  const keys = [...dialog().querySelectorAll<HTMLElement>("[data-field-key]")].map((node) => node.dataset.fieldKey);
  check(["mod_ids", "mods_directory", "mod_install_timeout_seconds", "override_mod_warnings", "sideload_mods", "sideload_mods_path"].every((key) => keys.includes(key)),
    "All existing official Mod settings remain reachable in the secondary editor");
  check(!keys.includes("modio_token"), "The private account token stays out of the Mod editor");
  await act(async () => { sourceInput("mod_ids").focus(); });
  await settle(() => Boolean(dialog().querySelector(".configuration-field-help-tooltip.is-visible")),
    "Keyboard focus did not show field guidance inside the native dialog");
  const help = element<HTMLElement>(".configuration-field-help-tooltip.is-visible", dialog());
  const helpBounds = help.getBoundingClientRect();
  check(helpBounds.width > 0 && helpBounds.height > 0 && helpBounds.left >= 0 && helpBounds.right <= innerWidth
    && helpBounds.top >= 0 && helpBounds.bottom <= innerHeight
    && help.contains(document.elementFromPoint(helpBounds.left + helpBounds.width / 2, helpBounds.top + helpBounds.height / 2)),
    "Field guidance stays visible above the native dialog instead of behind its top layer");
  const savedBeforeDraft = details.settings_json;
  await input(sourceInput("mod_ids"), `${secondId}\n${thirdId}`);
  await escape();
  check(dialog().open && sourceInput("mod_ids").value.includes(thirdId) && details.settings_json === savedBeforeDraft,
    "Escape cannot discard unsaved official Mod changes"); flags.draft_close_guard = true;
  let release!: () => void;
  pendingSave = new Promise<void>((resolve) => { release = resolve; });
  const attemptBefore = attempts;
  await click(button(zh ? "保存" : "Save", sourceField("mod_ids")));
  await settle(() => attempts > attemptBefore, "Saving the draft did not reach persistence");
  await escape();
  check(dialog().open && details.settings_json === savedBeforeDraft,
    `Escape keeps the editor open while saving (open=${dialog().open}, persistedUnchanged=${details.settings_json === savedBeforeDraft})`); flags.pending_close_guard = true;
  await act(async () => { pendingSave = null; release(); });
  await settle(() => settings().mod_ids === `${secondId}\n${thirdId}`, "The pending save did not finish");
  await settle(() => !sourceField("mod_ids").querySelector(".mw-manifest-toolbar"), "Saved draft did not clear");
  await closeSettings();
  check(rows().length === 3, "Saved official IDs immediately update the same Mod list");

  await openSettings();
  await input(sourceInput("mod_install_timeout_seconds"), "1200");
  failSave = true;
  await click(button(zh ? "保存" : "Save", sourceField("mod_install_timeout_seconds")));
  await settle(() => Boolean(sourceField("mod_install_timeout_seconds").querySelector('[role="alert"]')), "Save failure was not shown in the editor");
  check(settings().mod_install_timeout_seconds === undefined && sourceInput("mod_install_timeout_seconds").value === "1200",
    "Failed saves preserve the draft and leave persisted settings untouched");
  failSave = false;
  await click(button(zh ? "保存" : "Save", sourceField("mod_install_timeout_seconds")));
  await settle(() => settings().mod_install_timeout_seconds === 1200, "A failed save could not be retried"); flags.save_failure_recovery = true;
  const beforeCancel = details.settings_json;
  await input(sourceInput("mod_ids"), firstId);
  await click(button(zh ? "取消" : "Cancel", sourceField("mod_ids")));
  check(details.settings_json === beforeCancel && sourceInput("mod_ids").value === `${secondId}\n${thirdId}`,
    "Cancelling a field restores the saved value without writing settings"); flags.cancel_preserves_settings = true;
  await closeSettings();

  seed("abioticfactor", { mod_ids: firstId, sideload_mods: true, unrelated: "keep" }, false); await mount();
  const beforeSideload = writes;
  await removeOfficial(firstId);
  await settle(() => Boolean(fixture.querySelector(".shell-activity-notice.is-error")), "Invalid final sideload removal was not explained");
  check(writes === beforeSideload && settings().mod_ids === firstId && settings().sideload_mods === true,
    "The last sideloaded ID cannot be removed while sideloading still requires it"); flags.sideload_guard = true;

  seed(); details = { ...details, summary: { ...details.summary, status: "Running", active_process_count: 1 } }; await mount();
  const beforeRunning = writes;
  check(element<HTMLButtonElement>(".mw-entry-remove-button", officialRow(firstId)).disabled, "Running instances disable official removal");
  await openSettings();
  check(sourceInput("mod_ids").disabled, "Running instances keep official settings read only");
  await closeSettings();
  check(writes === beforeRunning, "Inspecting running settings performs no write"); flags.running_guard = true;

  seed(); archived = true; await mount();
  const beforeArchive = writes;
  check(rows().length === 2 && element<HTMLButtonElement>(".mw-entry-remove-button", officialRow(firstId)).disabled,
    "Archive libraries retain official IDs without file inventory or writable actions");
  await openSettings();
  check(sourceInput("mod_ids").readOnly || sourceInput("mod_ids").disabled, "Archived official settings are read only");
  await closeSettings();
  check(writes === beforeArchive, "Archive inspection performs no write"); flags.archive_guard = true;
  check(sourceCalls.length === 0, "Official mod.io IDs never reach Steam or Nexus APIs"); flags.source_isolation = true;

  seed("abioticfactor", { mod_ids: "invalid-mod-id" }, false); await mount();
  await click(element<HTMLButtonElement>(".mw-entry-row-select", officialRow("invalid-mod-id")));
  check(Boolean(fixture.querySelector(".mw-detail-config-panel [role=alert]")) && rows().length === 1,
    "Invalid official IDs remain visible with actionable validation in their selected details"); flags.invalid_native_help = true;
}

function seedGame(moduleId: string): string {
  seed(moduleId, {}, true);
  check(moduleHasModWorkbench(moduleId, moduleDetails), `${moduleId}: the production capability gate exposes this library`);
  const isWorkshop = moduleDetails.workshop?.provider === "steam";
  const enablement = moduleDetails.mods?.enablement;
  const value: Record<string, unknown> = { unrelated: "retain this" };
  const modId = enablement?.id_strategy === "palworld_package_name" ? "FixturePackage" : firstId;
  if (enablement) value[enablement.setting_key] = modId;
  if (moduleId === "dontstarve") Object.assign(value, { shared_workshop_mod_ids: firstId, master_enabled_workshop_mod_ids: firstId });
  if (moduleId === "projectzomboid") Object.assign(value, { workshop_items: firstId, mods: "FixtureRuntimeMod", map_name: "FixtureMap;Muldraugh, KY" });
  if (moduleId === "unturned") value.workshop_file_ids = firstId;
  if (moduleId === "arksurvivalevolved") Object.assign(value, { auto_managed_mods: true, auto_managed_mod_ids: firstId });
  if (moduleId === "terraria") Object.assign(value, { server_runtime: "tmodloader", tmodloader_enabled_mod_names: "FixtureRuntimeMod" });
  if (moduleId === "abioticfactor") value.mod_ids = firstId;
  details = { ...details, settings_json: JSON.stringify(value) };
  if (isWorkshop) catalog = [{ ...workshopItem(firstId, `${moduleDetails.summary.name} fixture Mod`),
    consumer_app_id: moduleDetails.workshop!.consumer_app_id }];
  const extension = moduleDetails.mods?.manual_staging?.accepts.find((entry) => !["folder", "zip"].includes(entry));
  const name = isWorkshop || enablement ? firstId : extension ? `FixtureMod.${extension}` : "FixtureMod";
  inventory.items = [{ ...inventory.items[0], name, path: `${inventory.target_path}\\${name}`,
    item_type: !isWorkshop && !enablement && extension ? "file" : "directory",
    inferred_id: enablement ? modId : isWorkshop && moduleId !== "squad" ? firstId : null,
    ...(moduleId === "terraria" ? { tmodloader_mod_name: "FixtureRuntimeMod" } : {}) }];
  return isWorkshop || enablement || moduleId === "abioticfactor" ? modId : name;
}
function visible(selector: string, within: ParentNode = fixture): HTMLElement[] {
  return [...within.querySelectorAll<HTMLElement>(selector)].filter((node) => node.getClientRects().length > 0);
}
async function reviewGame(moduleId: string) {
  const expected = seedGame(moduleId), before = writes;
  const originalSettings = details.settings_json, originalInventory = JSON.stringify(inventory);
  await mount();
  await settle(() => rows().some((row) => row.textContent?.includes(expected)), `${moduleId}: current instance Mod did not render`);
  const selected = rows().find((row) => row.textContent?.includes(expected))!;
  await click(element<HTMLButtonElement>(".mw-entry-row-select", selected));
  check(Boolean(fixture.querySelector(".mw-detail-layout .mw-detail-list-col .mw-entry-list"))
    && Boolean(fixture.querySelector(".mw-detail-layout .mw-detail-config-col .mw-detail-config-panel")),
    `${moduleId}: selection uses the shared list and detail columns`);
  check(element(".mw-detail-config-col").getAttribute("aria-label")?.includes(element(".mw-entry-row-title", selected).textContent ?? ""),
    `${moduleId}: selected details identify the current instance row`);
  check(visible(".mw-source-field, details, form, textarea", element(".mw-detail-panel")).length === 0,
    `${moduleId}: the Mod list has no embedded source settings or import form`);
  check(visible(".mw-managed-adopt, .mw-nexus-local-note, .mw-native-mod-note, .pz-map-order-editor").length === 0
    && !fixture.querySelector("dialog[open]"), `${moduleId}: extra instructions and editors do not occupy the default library`);
  if (moduleId === "terraria") check(Boolean(fixture.querySelector(".mw-detail-col-title .mw-terraria-import-button")),
    "terraria: saved-list import is an on-demand header action");
  if (moduleId === "projectzomboid") check(Boolean(fixture.querySelector(".mw-detail-col-title .pz-map-order-open")),
    "projectzomboid: map order is an on-demand header action");
  if (moduleId === "astroneer" || moduleId === "satisfactory") check(!fixture.querySelector(".mw-entry-enabled-toggle, .mw-entry-remove-button"),
    `${moduleId}: external integrations retain read-only inventory without invented file controls`);
  check(writes === before && details.settings_json === originalSettings && JSON.stringify(inventory) === originalInventory,
    `${moduleId}: inspecting the shared library leaves settings and inventory untouched`);
  viewportFits();
  reviewedModules.push(moduleId);
}

async function snapshot(surface: string) {
  if (surface === "workshop") {
    seed("conanexiles", {}, false);
    const key = moduleDetails.mods?.enablement?.setting_key;
    check(key && moduleDetails.workshop?.provider === "steam", "Comparison Workshop uses a real enablement contract");
    details = { ...details, settings_json: JSON.stringify({ [key]: "123456\n234567" }) };
    catalog = [workshopItem("123456", "Server balance adjustments"), workshopItem("234567", "Additional building pieces")]
      .map((item) => ({ ...item, consumer_app_id: moduleDetails.workshop!.consumer_app_id }));
  } else if (surface === "sevendaystodie") {
    seed("sevendaystodie", {}, true);
    inventory.items = ["ServerTools", "BackpackTweaks"].map((name) => ({ ...inventory.items[0], name,
      path: `${inventory.target_path}\\${name}`, inferred_id: null,
      management: { package_id: name, enabled: true, dependencies: [], dependents: [], dependency_status: "unknown" } }));
  } else if (surface === "projectzomboid" || surface === "terraria") seedGame(surface);
  else seed();
  await mount();
  const expectedCount = surface === "terraria" ? 1 : ["workshop", "sevendaystodie", "projectzomboid"].includes(surface) ? 2 : 3;
  await settle(() => rows().length === expectedCount, "Comparison library did not render");
  check(Boolean(fixture.querySelector(".mw-detail-layout .mw-entry-list")), "Every source uses the shared list and detail layout");
  check(!fixture.querySelector(".mw-detail-panel .mw-source-lists, .mw-detail-panel .mw-source-field"), "Comparison lists contain no embedded source form");
  if (surface === "sevendaystodie") check(!fixture.querySelector(".mw-source-settings-button"), "7DTD does not gain unsupported advanced configuration");
  if (surface === "dialog") await openSettings();
  await act(async () => {
    await document.fonts.ready;
    await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
  });
  viewportFits();
}
async function run() {
  const options = await (await fetch("/__mod_source_settings_fixture")).json() as { surface: string; exercise: boolean };
  await act(prepareBrowserLocaleCatalogs);
  details = await readInstanceDetails("srv-dst-terminal-error");
  const base = createModWorkbenchFixtureIpc(() => ({ details, moduleDetails, inventory, catalog,
    cached: new Set(catalog.map((item) => item.id)) }), { check, save: (saved) => { details = saved; writes += 1; } });
  mockWindows("main");
  mockIPC(async (command, payload) => {
    if (moduleDetails.summary.id === "abioticfactor" && (/steam_workshop|nexus/.test(command))) sourceCalls.push(command);
    if (command === "update_instance_record_if_current") {
      attempts += 1;
      if (pendingSave) await pendingSave;
      if (failSave) throw new Error("Fixture Mod save rejected");
    }
    if (command === "read_project_zomboid_workshop_mods_snapshot") {
      const args = payload as { instanceId: string; ids: string[] };
      check(args.instanceId === details.summary.id, "PZ metadata reads remain scoped to the current instance");
      return { workshop_root: "fixture-cache", workshop_root_exists: true, items: args.ids.map((id) => ({
        workshop_item_id: id, item_path: `fixture-cache/${id}`, status: "installed", mods: [{ directory_name: "FixtureRuntimeMod",
          mod_id: "FixtureRuntimeMod", mod_name: "FixtureRuntimeMod", mod_path: `fixture-cache/${id}/mods/FixtureRuntimeMod`,
          status: "loaded", map_ids: ["FixtureMap", "Muldraugh, KY"] }] })) };
    }
    return base(command, payload);
  }, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  if (options.exercise) {
    await exercise();
    for (const moduleId of games) await reviewGame(moduleId);
  }
  await snapshot(options.surface);
  check(errors.length === 0, "Browser console and uncaught errors are empty");
  return { status: "passed", ...flags, checks, writes, source_calls: sourceCalls, browser_errors: errors, modules: reviewedModules,
    locale: zh ? "zh-CN" : "en-US", surface: options.surface,
    evidence_scope: "Production UI with isolated simulated IPC; no backend, game or filesystem changes" };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error("Mod source settings acceptance exceeded 40 seconds")), 40000);
})]).catch((error) => ({ status: "failed", error: String(error), checks, writes, source_calls: sourceCalls, browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`, { method: "POST", body: JSON.stringify(report) }));
