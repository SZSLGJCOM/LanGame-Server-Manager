import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { readInstanceDetails } from "../../src/api";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import type { InstanceDetails, ManualModInventoryResult, ModuleDetails } from "../../src/types";
import { I18nProvider, useI18n } from "../../src/i18n";
import { modWorkbenchAvailability, moduleHasModWorkbench } from "../../src/views/servers/mod-workbench-capability";
import { ServerDetailTabs, type ServerDetailTabSpec } from "../../src/views/servers/ServerDetailTabs";
import { buildServerDetailTabSpecs } from "../../src/views/servers/server-detail-tab-specs";
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
const checks: string[] = [], errors: string[] = [], reports: Record<string, unknown>[] = [], opened: string[] = [], openedSources: string[] = [];
let game = "", epoch = 0, inventoryReads = 0;
let details: InstanceDetails, moduleDetails: ModuleDetails, inventory: ManualModInventoryResult | null;
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
function check(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(`${game}: ${label}: ${fixture.textContent}`);
  const entry = `${game}: ${label}`;
  if (!checks.includes(entry)) checks.push(entry);
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
function DetailTabsProbe(props: { moduleId: string; moduleDetails: ModuleDetails; onSelect(tab: ServerDetailTabSpec): void }) {
  const { t } = useI18n();
  return <ServerDetailTabs tabs={buildServerDetailTabSpecs({ moduleId: props.moduleId, moduleDetails: props.moduleDetails, t })}
    activeTab="runtime" panelId="probe" label="Instance detail sections" onSelect={props.onSelect} />;
}
async function mountTabs(id: string, details: ModuleDetails, selected: string[]) {
  await act(async () => root.render(<I18nProvider>
    <DetailTabsProbe moduleId={id} moduleDetails={details} onSelect={(tab) => selected.push(tab.id)} />
  </I18nProvider>));
  await settle(() => Boolean(fixture.querySelector("#probe-mods")), "Detail tabs did not render");
  return fixture.querySelector<HTMLButtonElement>("#probe-mods")!;
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
function independentStore(references: boolean) {
  check(!fixture.querySelector(".mw-config-pane,.mw-entry-row,.mw-detail-list-col,.mw-detail-config-col,[data-asa-mod-config]"),
    "Community Store stays independent of My Mods inventory and configuration");
  check(Boolean(fixture.querySelector(".mw-manual-pane > .mw-dropzone")), "Store owns a standalone file installation drop zone");
  for (const selector of [".mw-dropzone-title", ".mw-dropzone-copy", ".mw-dropzone-icon", ".mw-source-open-btn"]) {
    const target = fixture.querySelector<HTMLElement>(selector);
    check(target && target.getBoundingClientRect().width > 0 && target.getBoundingClientRect().height > 0,
      `Store keeps its visible ${selector} entry`);
  }
  check(Boolean(fixture.querySelector(".mw-reference-row")) === references,
    "Store shows reference installation only for its declared provider");
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
  independentStore(references);
  check(Boolean(fixture.querySelector(".mw-reference-input")) === references, "Only supported online/reference contracts expose an input");
  check(!fixture.querySelector("[data-nexus-auth-open],.mw-nexus-auth-btn"), "Community workflows do not expose a Nexus API key button");
  const sourceUrl = moduleDetails.mods?.source?.url;
  check(sourceUrl, "Community installation has an actual source URL");
  const sourcesBefore = openedSources.length;
  await click(fixture.querySelector<HTMLButtonElement>(".mw-source-open-btn")!);
  await settle(() => openedSources.length === sourcesBefore + 1, "Source entry did not open");
  check(openedSources.at(-1) === new URL(sourceUrl).href, "Source entry opens this game's actual Mod provider");
  await click(button("My Mods"));
  await settle(() => Boolean(fixture.querySelector(".mw-config-pane .mw-empty")), "Empty library did not render");
  check(!fixture.querySelector(".mw-entry-row"), "An empty instance has no invented Mod rows");
  check(!fixture.querySelector(".mw-detail-config-col"), "Empty library omits an empty detail column");
  await click(button("Store"));
  independentStore(references);

  const extension = moduleDetails.mods!.manual_staging!.accepts.find((value) => !["folder", "zip"].includes(value));
  const name = id === "arksurvivalascended" ? "424242-FixtureMod" : extension ? `FixtureMod.${extension}` : "FixtureMod";
  inventory = { ...inventory!, target_exists: true, items: [{ name, path: `${inventory!.target_path}\\${name}`,
    item_type: extension ? "file" : "directory", file_count: 2, total_bytes: 8192,
    inferred_id: id === "arksurvivalascended" ? "424242" : null }] };
  const nextRead = inventoryReads;
  await mount();
  await settle(() => inventoryReads > nextRead && Boolean(fixture.querySelector(".mw-dropzone")), "Store with saved inventory did not initialize");
  independentStore(references);
  check(!fixture.textContent?.includes(name) && !fixture.textContent?.includes(inventory.items[0].path),
    "Store does not show existing payload names or local details");
  await click(button("My Mods"));
  await settle(() => inventoryReads > nextRead && fixture.querySelectorAll(".mw-entry-row").length === 1, "Saved inventory must appear after remount");
  noSteamControls();
  const row = fixture.querySelector<HTMLElement>(".mw-entry-row")!;
  check(row.textContent?.includes(name), "Library displays the instance's persisted payload");
  if (id !== "arksurvivalascended") {
    check(!row.querySelector('input[type="checkbox"],.mw-entry-enabled-toggle'), "File-only module does not invent an enablement checkbox");
    check(!row.querySelector(".mw-entry-remove-button"), "File-only module does not label unsupported removal as an action");
  }
  await click(row.querySelector<HTMLButtonElement>(".mw-entry-row-select")!);
  if (id === "arksurvivalascended") {
    const configuration = fixture.querySelector(".mw-detail-config-col [data-asa-mod-config]");
    check(configuration?.getAttribute("data-asa-mod-config") === inventory.items[0].inferred_id,
      "ASA configuration belongs to the selected installed Mod");
    const editor = configuration?.querySelector<HTMLTextAreaElement>(".asa-mod-config__input");
    check(editor && !editor.disabled, "Selected ASA Mod exposes its real INI editor");
  } else {
    check(fixture.querySelector(".mw-detail-config-col")?.textContent?.includes(inventory.items[0].path), "Selected payload details show its actual instance path");
  }
  await click(button("Open Mod folder"));
  check(opened.at(-1) === inventory.target_path, "Folder action opens this instance's target");
  await click(button("Store"));
  independentStore(references);
  check(!fixture.textContent?.includes(inventory.items[0].path), "Returning to Store clears selected payload details from the page");
  if (id === "arksurvivalascended") {
    const installedId = inventory.items[0].inferred_id;
    check(installedId, "ASA inventory identifies the installed CurseForge Mod");
    details = { ...details, settings_json: JSON.stringify({ mod_ids_csv: installedId,
      asa_mod_configurations: { [installedId]: { game_ini: "[StoreSeparationFixture]\nKeep=original\n", game_user_settings_ini: "" } } }) };
    const configuredRead = inventoryReads;
    await mount();
    await settle(() => inventoryReads > configuredRead && Boolean(fixture.querySelector(".mw-dropzone")), "Configured ASA Store did not initialize");
    independentStore(references);
    check(!fixture.textContent?.includes(installedId) && !fixture.textContent?.includes("StoreSeparationFixture"),
      "Configured CurseForge IDs and INI contents stay out of Store");
    await click(button("My Mods"));
    await settle(() => Boolean(fixture.querySelector(`[data-asa-mod-config="${installedId}"]`)), "Configured ASA Mod editor did not return in My Mods");
    check(fixture.querySelector<HTMLTextAreaElement>(".asa-mod-config__input")?.value.includes("StoreSeparationFixture"),
      "My Mods retains the installed ASA Mod's saved INI configuration");
    await click(button("Store"));
    independentStore(references);
  }
  check(document.documentElement.scrollWidth <= innerWidth + 1, "Workspace has no horizontal page overflow");
  reports.push({ module: id, gate: true, store_library_switch: true, empty_inventory: true, persisted_inventory: true,
    independent_empty_store: true, independent_populated_store: true, source_entry_opens: true,
    independent_configured_store: id === "arksurvivalascended", selected_detail_stays_in_library: true,
    no_steam_controls: true, file_only_controls_absent: id !== "arksurvivalascended", references });
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const template = await readInstanceDetails("srv-dst-terminal-error");
  const base = createModWorkbenchFixtureIpc(() => ({ details, moduleDetails, inventory, catalog: [], cached: new Set<string>() }), {
    check, save: () => { throw new Error("Capability smoke must not mutate instance settings"); }, openPath: (path) => opened.push(path)
  });
  mockWindows("community-capabilities-fixture");
  mockIPC(async (command, payload) => {
    if (command === "read_manual_mod_inventory") inventoryReads += 1;
    if (command === "open_external_url") {
      openedSources.push(String((payload as { url: string }).url));
      return;
    }
    return base(command, payload);
  }, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  for (const id of unavailable) {
    prepare(template, id);
    check(!moduleHasModWorkbench(id, moduleDetails), "Production gate hides modules without a verified Mod workflow");
    reports.push({ module: id, gate: false });
  }
  prepare(template, "rimworld");
  check(!moduleHasModWorkbench(game, moduleDetails), "Client-only Workshop dependencies expose no server Mod workbench");
  check(modWorkbenchAvailability(game, moduleDetails) === "client_only", "The capability catalog marks RimWorld Workshop content as client-only");
  const selected: string[] = [];
  const modsTab = await mountTabs(game, moduleDetails, selected);
  const tabRect = modsTab.getBoundingClientRect();
  check(tabRect.width > 0 && tabRect.height > 0 && modsTab.textContent?.includes("Mods"), "RimWorld Mods tab stays visible");
  check(modsTab.disabled && modsTab.getAttribute("aria-disabled") === "true" && modsTab.classList.contains("is-disabled"),
    "RimWorld Mods tab is disabled with an accessible disabled state");
  const describedBy = modsTab.closest<HTMLElement>(".server-detail-tab-help")?.getAttribute("aria-describedby");
  check(describedBy && document.getElementById(describedBy)?.textContent?.includes("installed on each player's game client"),
    "Disabled Mods tab explains the client-only reason");
  await act(async () => { modsTab.click(); });
  check(selected.length === 0, "Clicking the disabled Mods tab does not select it");
  check(!fixture.querySelector(".mw-workspace-panel,.mw-dropzone,.mw-reference-input"), "No Mod workspace or install control renders");
  const supportedTab = await mountTabs("valheim", buildMockModuleDetails("valheim"), selected);
  check(!supportedTab.disabled && !supportedTab.closest(".server-detail-tab-help"), "A module with a real Mod workflow keeps an enabled Mods tab");
  reports.push({ module: game, gate: false, mods_tab_disabled: true, disabled_reason: true });
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
