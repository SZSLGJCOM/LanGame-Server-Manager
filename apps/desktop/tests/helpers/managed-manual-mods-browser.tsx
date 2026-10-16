import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { readInstanceDetails } from "../../src/api";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { I18nProvider } from "../../src/i18n";
import { ModWorkbench } from "../../src/views/servers/ModWorkbench";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceDetails, ManagedModAction, ManagedModActionResult, ManualModInventoryItem,
  ManualModInventoryResult, ModuleDetails } from "../../src/types";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

// Real ReactDOM, native dialog and browser events; only IPC is substituted.
// No filesystem deletion, game process or real backend is exercised here.
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const chinese = innerWidth < 1000;
localStorage.setItem("langame.locale", chinese ? "zh-CN" : "en-US");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:24px;box-sizing:border-box;display:grid;grid-template-rows:minmax(0,1fr) auto";
const root = createRoot(fixture);
const checks: string[] = [];
const errors: string[] = [];
const requests: { instanceId: string; packageId: string; action: ManagedModAction }[] = [];
const openedPaths: string[] = [];
let details: InstanceDetails;
let moduleDetails: ModuleDetails;
let inventory: ManualModInventoryResult;
let epoch = 0;
let inventoryReads = 0;
let gate: { promise: Promise<void>; release: () => void } | null = null;
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(`${message}: ${fixture.textContent}`);
  checks.push(message);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const node = fixture.querySelector<T>(selector);
  if (!node) throw new Error(`Missing ${selector}: ${fixture.textContent}`);
  return node;
}
async function settle(predicate: () => boolean, message: string) {
  const until = performance.now() + 6000;
  while (!predicate()) {
    if (performance.now() >= until) throw new Error(`${message}: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
function row(name: string) {
  return [...fixture.querySelectorAll<HTMLElement>(".mw-entry-row")].find((node) =>
    node.querySelector(".mw-entry-row-title")?.textContent === name);
}
async function click(node: HTMLElement) { await act(async () => { node.click(); }); }
function Harness() {
  const [noticeTarget, setNoticeTarget] = useState<HTMLDivElement | null>(null);
  return <ActivityNoticeTarget.Provider value={{ element: noticeTarget, dismissLabel: chinese ? "关闭" : "Close" }}>
    <div className="detail-stack detail-stack--server">
      <div className="server-detail-scroll server-detail-scroll--mods">
        <ModWorkbench key={epoch} details={details} moduleDetails={moduleDetails} launchPlan={null}
          onSaveSettings={async () => { throw new Error("File Mod operations must not write settings JSON"); }} />
      </div>
    </div>
    <footer className="shell-activity-bar"><span className="shell-activity-label">Activity</span>
      <div className="shell-activity-notices" ref={setNoticeTarget} />
    </footer>
  </ActivityNoticeTarget.Provider>;
}
async function mount(remount = true) {
  if (remount) epoch += 1;
  await act(async () => { root.render(<I18nProvider><InstanceSettingsSaveProvider><Harness /></InstanceSettingsSaveProvider></I18nProvider>); });
  await settle(() => Boolean(fixture.querySelector(".mw-sort-pill")), "Workbench mounted");
}
async function myMods() {
  const tab = [...fixture.querySelectorAll<HTMLButtonElement>(".mw-sort-pill")].find((node) =>
    node.textContent === (chinese ? "我的 Mod" : "My Mods"));
  assert(tab, "My Mods tab exists");
  await click(tab);
  await settle(() => Boolean(row("ExamplePlugin")), "Inventory rows rendered");
}
function item(name: string, management?: ManualModInventoryItem["management"]): ManualModInventoryItem {
  return { name, path: `fixture://managed-mods/${name}`, item_type: "directory", file_count: 3, total_bytes: 4096, management };
}
function seed(moduleId: string) {
  moduleDetails = buildMockModuleDetails(moduleId);
  assert(moduleDetails.mods?.manual_staging && !moduleDetails.mods.enablement, `${moduleId} uses a file Mod workflow`);
  details = { ...details, summary: { ...details.summary, id: `fixture-managed-${moduleId}`, module_id: moduleId,
    status: "Stopped", active_process_count: 0 }, active_run: null, settings_json: "{\"custom\":\"preserve\"}" };
  inventory = { instance_id: details.summary.id, module_id: moduleId, source_label: moduleDetails.mods.source?.label ?? moduleId,
    target_label: moduleDetails.mods.manual_staging.target_label, target_path: "fixture://managed-mods", target_exists: true,
    items: [
      item("ExamplePlugin", { package_id: "author-example", enabled: true, dependencies: ["SharedLibrary"],
        dependents: [], dependency_status: "unknown" }),
      item("SharedLibrary", { package_id: "author-library", enabled: true, dependencies: [],
        dependents: ["ExamplePlugin"], dependency_status: "complete" }),
      item(moduleId === "minecraft" ? "CurseForgeImport" : "LegacyImport"), item("config"), item("0_TFP_Harmony")
    ] };
}
async function deleteDialog(name: string) {
  const button = row(name)?.querySelector<HTMLButtonElement>(".mw-entry-remove-button");
  assert(button, `Delete action exists for ${name}`);
  await click(button);
  await settle(() => Boolean(fixture.querySelector<HTMLDialogElement>(".mw-managed-delete-dialog")?.open), "Delete review opened");
  return element<HTMLDialogElement>(".mw-managed-delete-dialog");
}
async function nativeEscape() {
  const nonce = new URLSearchParams(location.search).get("nonce");
  await act(async () => { const response = await fetch(`/__reliability_key/${nonce}/Escape`, { method: "POST" });
    assert(response.ok, "Native Escape dispatched"); });
}
async function exercise(moduleId: string) {
  seed(moduleId); await mount(); await myMods();
  const importedName = moduleId === "minecraft" ? "CurseForgeImport" : "LegacyImport";
  const settingsBefore = details.settings_json;
  assert(row("ExamplePlugin")?.querySelector<HTMLInputElement>("input")?.checked, `${moduleId}: installed Mod starts enabled`);
  assert(!row("config")?.querySelector(".mw-managed-adopt") && !row("0_TFP_Harmony")?.querySelector(".mw-managed-adopt"),
    `${moduleId}: known configuration and official content are read only`);
  const readsBefore = inventoryReads;
  await click(row("ExamplePlugin")!.querySelector<HTMLInputElement>("input")!);
  await settle(() => row("ExamplePlugin")?.querySelector<HTMLInputElement>("input")?.checked === false,
    "Disabled inventory persisted in the response");
  assert(inventoryReads > readsBefore, `${moduleId}: successful operation refreshes inventory`);
  await mount(); await myMods();
  assert(row("ExamplePlugin")?.querySelector<HTMLInputElement>("input")?.checked === false, `${moduleId}: disabled state survives remount`);
  await click(row("ExamplePlugin")!.querySelector<HTMLInputElement>("input")!);
  await settle(() => row("ExamplePlugin")?.querySelector<HTMLInputElement>("input")?.checked === true, "Mod re-enabled");

  const adoption = row(importedName)!.querySelector<HTMLButtonElement>(".mw-managed-adopt")!;
  const adoptionBounds = adoption.getBoundingClientRect();
  const rowBounds = row(importedName)!.getBoundingClientRect();
  assert(adoptionBounds.left >= rowBounds.left && adoptionBounds.right <= rowBounds.right && adoptionBounds.width > 0,
    `${moduleId}: adoption action fits inside the inventory row`);
  await click(adoption);
  await settle(() => Boolean(row(importedName)?.querySelector("input")), "Existing import adopted");
  assert(requests.at(-1)?.packageId === importedName && requests.at(-1)?.action === "adopt",
    `${moduleId}: adoption identifies the existing top-level entry`);
  assert(row(importedName)?.querySelector<HTMLInputElement>("input")?.checked, `${moduleId}: adoption makes files manageable`);

  const beforeReview = requests.length;
  let dialog = await deleteDialog("SharedLibrary");
  assert(requests.length === beforeReview, `${moduleId}: opening deletion review never mutates files`);
  assert(dialog.textContent?.includes("ExamplePlugin") && /Required by|被以下 Mod 依赖/.test(dialog.textContent ?? ""),
    `${moduleId}: review lists dependent Mods`);
  await click(element(".mw-managed-delete-confirm"));
  await settle(() => Boolean(dialog.querySelector("[role=alert]")), "Backend dependency refusal displayed in review");
  assert(dialog.open && Boolean(row("SharedLibrary")), `${moduleId}: dependency rejection keeps review and inventory intact`);
  await nativeEscape();
  await settle(() => !fixture.querySelector(".mw-managed-delete-dialog"), "Delete review dismissed");

  dialog = await deleteDialog("ExamplePlugin");
  assert(dialog.textContent?.includes("SharedLibrary") && /complete dependency declaration|未提供完整依赖声明/.test(dialog.textContent ?? ""),
    `${moduleId}: review exposes declared dependencies and uncertainty`);
  assert(/Configuration, modified files|配置、修改过的文件/.test(dialog.textContent ?? ""), `${moduleId}: review explains retained files`);
  assert(dialog.contains(document.activeElement), `${moduleId}: initial keyboard focus stays inside the native modal`);
  let release!: () => void;
  gate = { promise: new Promise<void>((resolve) => { release = resolve; }), release: () => release() };
  await click(element(".mw-managed-delete-confirm"));
  await settle(() => element<HTMLButtonElement>(".mw-managed-delete-confirm").disabled, "Pending deletion disables confirmation");
  assert([...dialog.querySelectorAll<HTMLButtonElement>("button")].every((node) => node.disabled), `${moduleId}: pending deletion disables dialog actions`);
  const pendingRequests = requests.length;
  await click(element(".mw-managed-delete-confirm"));
  await nativeEscape();
  assert(dialog.open && requests.length === pendingRequests, `${moduleId}: pending dialog prevents duplicate deletion and Escape`);
  await act(async () => { gate!.release(); }); gate = null;
  await settle(() => !row("ExamplePlugin") && !fixture.querySelector(".mw-managed-delete-dialog"), "Deletion completed");
  assert(fixture.textContent?.includes("fixture://preserved-mods"), `${moduleId}: result reports the preserved directory`);
  const open = [...fixture.querySelectorAll<HTMLButtonElement>("button")].find((node) => /Open preserved files|打开保留文件/.test(node.textContent ?? ""));
  assert(open, `${moduleId}: preserved directory has an open action`); await click(open);
  assert(openedPaths.at(-1) === "fixture://preserved-mods", `${moduleId}: preserved path reaches the native open command`);
  assert(Boolean(row("SharedLibrary")) && details.settings_json === settingsBefore, `${moduleId}: deletion keeps dependencies and settings`);

  seed(moduleId); details = { ...details, summary: { ...details.summary, status: "Running", active_process_count: 1 } };
  await mount(); await myMods();
  const beforeRunning = requests.length;
  const controls = [...fixture.querySelectorAll<HTMLInputElement | HTMLButtonElement>(".mw-entry-row-actions input, .mw-entry-row-actions button")];
  assert(controls.length >= 5 && controls.every((node) => node.disabled), `${moduleId}: running instance disables all file management actions`);
  for (const control of controls) await click(control);
  assert(requests.length === beforeRunning, `${moduleId}: running instance dispatches no management mutations`);
}
async function verifyScopeGeneration() {
  seed("valheim"); await mount(); await myMods();
  const firstVisit = inventory;
  let release!: () => void;
  gate = { promise: new Promise<void>((resolve) => { release = resolve; }), release: () => release() };
  const before = requests.length;
  await click(row("ExamplePlugin")!.querySelector<HTMLInputElement>("input")!);
  await settle(() => requests.length > before, "First visit dispatched a delayed Mod operation");

  // Preserve the hook/component identity to exercise A -> B -> A, rather than
  // relying on React unmounting the original instance view.
  seed("minecraft"); await mount(false); await myMods();
  await settle(() => inventory.module_id === "minecraft" && Boolean(row("CurseForgeImport")), "Second instance inventory loaded");
  seed("valheim");
  inventory.items.push(item("NewVisitOnly"));
  for (const name of ["BepInEx", "plugins", "mods", "save", "saves", "settings", "userdata"]) inventory.items.push(item(name));
  for (const name of ["Imported.lua", "Imported.bundle", "Imported.so", "Imported.cs", "Imported.smod", "Imported.ucas", "Imported.utoc"]) {
    inventory.items.push({ ...item(name), item_type: "file" });
  }
  await mount(false); await myMods();
  await settle(() => Boolean(row("NewVisitOnly")), "Returning to the first instance loaded a new inventory visit");
  const freshVisit = inventory;
  await act(async () => { gate!.release(); }); gate = null;
  await settle(() => row("ExamplePlugin")?.querySelector<HTMLInputElement>("input")?.disabled === false,
    "Old operation finished without keeping the current visit busy");
  assert(firstVisit.items[0].management?.enabled === false, "The already dispatched old operation completed on its original inventory");
  assert(inventory === freshVisit && Boolean(row("NewVisitOnly")) && row("ExamplePlugin")?.querySelector<HTMLInputElement>("input")?.checked,
    "A -> B -> A ignores the first visit's late inventory response even when the instance ID matches again");
  assert(!fixture.querySelector(".shell-activity-notice.is-success"), "A stale response publishes no success message in the new visit");
  for (const name of ["BepInEx", "plugins", "mods", "save", "saves", "settings", "userdata"]) {
    assert(!row(name)?.querySelector(".mw-managed-adopt"), `Protected root ${name} has no adoption action`);
  }
  for (const name of ["Imported.lua", "Imported.bundle", "Imported.so", "Imported.cs", "Imported.smod", "Imported.ucas", "Imported.utoc"]) {
    assert(Boolean(row(name)?.querySelector(".mw-managed-adopt")), `Recognized local payload ${name} can request adoption`);
  }
}

async function run() {
  await act(prepareBrowserLocaleCatalogs);
  details = await readInstanceDetails("srv-dst-terminal-error");
  mockWindows("managed-mods-fixture");
  mockIPC(async (command, payload) => {
    if (command === "read_manual_mod_inventory") { inventoryReads += 1; return structuredClone(inventory); }
    if (command === "read_background_jobs") return [];
    if (command === "open_local_path") { const args = payload as { path: string }; openedPaths.push(args.path); return; }
    if (command === "manage_manual_mod") {
      const args = payload as { instanceId: string; packageId: string; action: ManagedModAction };
      assert(Object.keys(args).sort().join(",") === "action,instanceId,packageId" && args.instanceId === details.summary.id,
        "Management IPC passes the exact instance, package and action contract");
      requests.push(args);
      const operationInventory = inventory;
      const target = operationInventory.items.find((entry) => args.action === "adopt" ? entry.name === args.packageId : entry.management?.package_id === args.packageId);
      assert(target, "Mutation targets one inventory package");
      if (target.management?.dependents.length && ["disable", "delete"].includes(args.action)) throw new Error("Required by ExamplePlugin; remove or disable it first.");
      if (gate) await gate.promise;
      if (args.action === "adopt") target.management = { package_id: "imported-legacy", enabled: true, dependencies: [], dependents: [], dependency_status: "unknown" };
      else if (args.action === "delete") {
        operationInventory.items = operationInventory.items.filter((entry) => entry !== target);
        for (const entry of operationInventory.items) if (entry.management) entry.management.dependents = entry.management.dependents.filter((name) => name !== target.name);
      } else { assert(target.management, "Enabled state belongs to a managed package"); target.management.enabled = args.action === "enable"; }
      return { inventory: structuredClone(operationInventory), preserved_path: args.action === "delete" ? "fixture://preserved-mods" : null,
        removed_file_count: args.action === "delete" ? 2 : 0, warnings: args.action === "delete" ? ["User configuration and changed files were retained."] : [] } satisfies ManagedModActionResult;
    }
    throw new Error(`Unexpected native command ${command}`);
  }, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  for (const moduleId of ["valheim", "sevendaystodie", "minecraft"]) await exercise(moduleId);
  await verifyScopeGeneration();
  for (const moduleId of ["astroneer", "satisfactory"]) {
    const before = requests.length;
    seed(moduleId); await mount(); await myMods();
    assert(Boolean(row("ExamplePlugin")) && Boolean(row("LegacyImport")), `${moduleId}: existing inventory stays visible`);
    assert(!fixture.querySelector(".mw-entry-enabled-toggle, .mw-managed-adopt, .mw-entry-remove-button"),
      `${moduleId}: external integration exposes no unsafe generic file action`);
    assert(requests.length === before, `${moduleId}: rendering inventory makes no mutation request`);
  }
  // Final screenshot keeps both managed and previously imported packages visible.
  seed("sevendaystodie"); await mount(); await myMods(); await deleteDialog("ExamplePlugin");
  const bounds = element(".mw-managed-delete-dialog").getBoundingClientRect();
  assert(bounds.left >= 0 && bounds.top >= 0 && bounds.right <= innerWidth && bounds.bottom <= innerHeight,
    "Deletion review fits the supported desktop viewport");
  assert(document.documentElement.scrollWidth <= innerWidth + 1 && document.documentElement.scrollHeight <= innerHeight + 1,
    "Management workspace has no viewport overflow");
  assert(errors.length === 0, "Browser console and uncaught errors remain empty");
  return { status: "passed", checks, requests, opened_paths: openedPaths, browser_errors: errors, locale: chinese ? "zh-CN" : "en-US",
    viewport: { width: innerWidth, height: innerHeight }, evidence_scope: "Real browser with simulated IPC; no backend or real filesystem changes" };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error("Managed Mods acceptance exceeded 40 seconds")), 40000); })])
  .catch((error) => ({ status: "failed", error: String(error), checks, requests, browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`, { method: "POST", body: JSON.stringify(report) }));
