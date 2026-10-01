import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { readInstanceDetails } from "../../src/api";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import type { InstanceDetails, ManualModInventoryResult, ModuleDetails } from "../../src/types";
import { moduleHasModWorkbench } from "../../src/views/servers/mod-workbench-capability";
import { createModWorkbenchFixtureIpc, ModWorkbenchBrowserHost } from "./mod-workbench-browser-support";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "en-US");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:16px;box-sizing:border-box;display:flex;flex-direction:column";
const root = createRoot(fixture);
const community = ["abioticfactor", "arksurvivalascended", "astroneer", "corekeeper", "enshrouded", "humanitz",
  "minecraft", "necesse", "rust", "satisfactory", "sevendaystodie", "sonsoftheforest", "valheim", "vrising", "windrose"];
const unavailable = ["nightingale", "returntomoria", "romestead", "runescapedragonwilds", "scum", "theforest"];
const checks: string[] = [], errors: string[] = [], reports: Record<string, unknown>[] = [], opened: string[] = [];
let game = "", epoch = 0, inventoryReads = 0;
let details: InstanceDetails, moduleDetails: ModuleDetails, inventory: ManualModInventoryResult | null;
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
function check(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(`${game}: ${label}: ${fixture.textContent}`);
  checks.push(`${game}: ${label}`);
}
function button(label: string): HTMLButtonElement {
  const result = [...fixture.querySelectorAll<HTMLButtonElement>("button")].find((item) =>
    item.textContent?.trim() === label || item.getAttribute("aria-label") === label);
  if (!result) throw new Error(`${game}: Missing button ${label}: ${fixture.textContent}`);
  return result;
}
async function settle(predicate: () => boolean, label: string) {
  const deadline = performance.now() + 4000;
  while (!predicate()) {
    if (performance.now() > deadline) throw new Error(`${game}: ${label}: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(target: HTMLElement) {
  target.scrollIntoView({ block: "nearest" });
  const rect = target.getBoundingClientRect();
  check(rect.width > 0 && rect.height > 0 && rect.left >= -1 && rect.right <= innerWidth + 1 &&
    rect.top >= -1 && rect.bottom <= innerHeight + 1, "Action is reachable inside the viewport");
  check(!(target instanceof HTMLButtonElement && target.disabled), "Action is enabled");
  await act(async () => { target.focus(); target.click(); });
}
async function mount() {
  epoch += 1;
  await act(async () => root.render(<ModWorkbenchBrowserHost details={details} moduleDetails={moduleDetails}
    epoch={epoch} onSaved={() => { throw new Error("Capability review must not save settings"); }} />));
}
function prepare(template: InstanceDetails, id: string) {
  game = id; moduleDetails = buildMockModuleDetails(id);
  check(moduleDetails.summary.id === id, "Capabilities come from this module's actual manifest");
  details = { ...template, summary: { ...template.summary, id: `fixture-community-${id}`, module_id: id,
    status: "Stopped", active_process_count: 0 }, active_run: null, settings_json: "{}" };
  inventory = moduleDetails.mods?.manual_staging ? { instance_id: details.summary.id, module_id: id,
    source_label: moduleDetails.mods.source?.label ?? "Local files", target_label: moduleDetails.mods.manual_staging.target_label,
    target_path: `C:\\Fixture\\${id}\\Mods`, target_exists: false, items: [] } : null;
}
function noSteamControls() {
  check(!fixture.querySelector('[aria-label="Workshop content type"]'), "Community workflow has no Steam Mod/Collection selector");
  check(!fixture.querySelector(".mw-search-input,.mw-collection-list"), "Community workflow has no Steam browser or collection library");
}
async function runCommunity(template: InstanceDetails, id: string) {
  prepare(template, id);
  check(moduleHasModWorkbench(id, moduleDetails), "Production gate exposes the declared Mod workflow");
  check(!moduleDetails.workshop && Boolean(moduleDetails.mods?.manual_staging), "Non-Steam source has a real staging contract");
  const readsBefore = inventoryReads;
  await mount();
  await settle(() => inventoryReads > readsBefore && Boolean(fixture.querySelector(".mw-dropzone")), "Community store did not initialize");
  noSteamControls();
  const references = Boolean(moduleDetails.mods?.enablement || moduleDetails.mods?.source?.provider === "thunderstore");
  check(Boolean(fixture.querySelector(".mw-reference-input")) === references, "Only supported online/reference contracts expose an input");
  await click(button("My Mods"));
  await settle(() => Boolean(fixture.querySelector(".mw-config-pane .mw-empty")), "Empty library did not render");
  check(!fixture.querySelector(".mw-entry-row"), "An empty instance has no invented Mod rows");
  check(!fixture.querySelector(".mw-detail-config-col"), "Empty library omits an empty detail column");
  await click(button("Store"));
  check(Boolean(fixture.querySelector(".mw-dropzone")), "Store/My Mods switching preserves the source entry");

  const extension = moduleDetails.mods!.manual_staging!.accepts.find((value) => !["folder", "zip"].includes(value));
  const name = id === "arksurvivalascended" ? "424242-FixtureMod" : extension ? `FixtureMod.${extension}` : "FixtureMod";
  inventory = { ...inventory!, target_exists: true, items: [{ name, path: `${inventory!.target_path}\\${name}`,
    item_type: extension ? "file" : "directory", file_count: 2, total_bytes: 8192,
    inferred_id: id === "arksurvivalascended" ? "424242" : null }] };
  const nextRead = inventoryReads;
  await mount(); await click(button("My Mods"));
  await settle(() => inventoryReads > nextRead && fixture.querySelectorAll(".mw-entry-row").length === 1, "Saved inventory must appear after remount");
  noSteamControls();
  const row = fixture.querySelector<HTMLElement>(".mw-entry-row")!;
  check(row.textContent?.includes(name), "Library displays the instance's persisted payload");
  if (id !== "arksurvivalascended") {
    check(!row.querySelector('input[type="checkbox"],.mw-entry-enabled-toggle'), "File-only module does not invent an enablement checkbox");
    check(!row.querySelector(".mw-entry-remove-button"), "File-only module does not label unsupported removal as an action");
  }
  await click(row.querySelector<HTMLButtonElement>(".mw-entry-row-select")!);
  check(fixture.querySelector(".mw-detail-config-col")?.textContent?.includes(inventory.items[0].path), "Selected payload details show its actual instance path");
  await click(button("Open Mod folder"));
  check(opened.at(-1) === inventory.target_path, "Folder action opens this instance's target");
  check(document.documentElement.scrollWidth <= innerWidth + 1, "Workspace has no horizontal page overflow");
  reports.push({ module: id, gate: true, store_library_switch: true, empty_inventory: true, persisted_inventory: true,
    no_steam_controls: true, file_only_controls_absent: id !== "arksurvivalascended", references });
}
async function run() {
  const template = await readInstanceDetails("srv-dst-terminal-error");
  const base = createModWorkbenchFixtureIpc(() => ({ details, moduleDetails, inventory, catalog: [], cached: new Set<string>() }), {
    check, save: () => { throw new Error("Capability smoke must not mutate instance settings"); }, openPath: (path) => opened.push(path)
  });
  mockWindows("community-capabilities-fixture");
  mockIPC(async (command, payload) => {
    if (command === "read_manual_mod_inventory") inventoryReads += 1;
    return base(command, payload);
  }, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  for (const id of unavailable) {
    prepare(template, id);
    check(!moduleHasModWorkbench(id, moduleDetails), "Production gate hides modules without a verified Mod workflow");
    reports.push({ module: id, gate: false });
  }
  prepare(template, "rimworld");
  check(moduleHasModWorkbench(game, moduleDetails), "Client-only workflow retains its explanation entry");
  await mount(); await settle(() => Boolean(fixture.querySelector(".mw-unsupported-pane")), "Client-only explanation did not render");
  check(fixture.textContent?.includes("No server-side Mod installation"), "RimWorld explains the client/server boundary");
  check(!fixture.querySelector(".mw-dropzone,.mw-reference-input,.mw-entry-enabled-toggle,.mw-entry-remove-button"), "Client-only page has no install or mutation control");
  reports.push({ module: game, gate: true, client_only_explanation: true });
  for (const id of community) await runCommunity(template, id);
  check(errors.length === 0, "No browser console or uncaught errors");
  return { status: "passed", checks, modules: reports, browser_errors: errors, viewport: { width: innerWidth, height: innerHeight } };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error("Community capability acceptance exceeded 35 seconds")), 35000);
})]).catch((error) => ({ status: "failed", checks, modules: reports, error: String(error), browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`, { method: "POST", body: JSON.stringify(report) }));
