import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { readInstanceDetails } from "../../src/api";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import type { InstanceDetails, ManualModInventoryResult, ModuleDetails, SteamWorkshopBrowseKind, SteamWorkshopLookupItem } from "../../src/types";
import { createModWorkbenchFixtureIpc, ModWorkbenchBrowserHost, workshopItem } from "./mod-workbench-browser-support";

// Only IPC is replaced. Real components, save coordination, settings plans and
// asynchronous refreshes operate on isolated in-memory instance records.
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "en-US");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:16px;box-sizing:border-box;display:flex;flex-direction:column";
const root = createRoot(fixture);
const errors: string[] = [];
const checks: string[] = [];
const openedPaths: string[] = [];
const downloadRequests: string[][] = [];
const inventoryModules: string[] = [];
let squadManagedPath: string | null = null;
let squadEmptyDirectoryInstalled: boolean | null = null;
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
let details: InstanceDetails;
let moduleDetails: ModuleDetails;
let epoch = 0;
let writes = 0;
let inventory: ManualModInventoryResult | null = null;
let catalog: SteamWorkshopLookupItem[] = [];
let cached = new Set<string>();
const browseRequests: { kind: SteamWorkshopBrowseKind; query: string; sort: string; page: number }[] = [];
let browsePageSize = 12;
let holdNextCollection = false;
let releaseCollection: (() => void) | null = null;
const browseFaults = ["missing-kind", "wrong-kind", "wrong-app", "invalid-kind"] as const;
let browseFault: typeof browseFaults[number] | null = null;
const recoveredBrowseFaults: string[] = [];
const emptyBrowseStates: string[] = [];
const ownedId = "123456";
const newId = "234567";
const childId = "345678";
const collectionId = "456789";

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
  checks.push(label);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const found = fixture.querySelector<T>(selector);
  if (!found) throw new Error(`Missing ${selector}`);
  return found;
}
function button(label: string, within: ParentNode = fixture): HTMLButtonElement {
  const found = [...within.querySelectorAll<HTMLButtonElement>("button")].find((target) =>
    target.textContent?.trim() === label || target.getAttribute("aria-label") === label);
  if (!found) throw new Error(`Missing button ${label}`);
  return found;
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
  assert(rect.width > 0 && rect.height > 0 && rect.left >= -1 && rect.right <= innerWidth + 1 &&
    rect.top >= -1 && rect.bottom <= innerHeight + 1, "Interaction target remains inside the desktop viewport");
  assert(!(target instanceof HTMLButtonElement) || !target.disabled, "Interaction target is enabled");
  await act(async () => { target.focus(); target.click(); });
}
async function input(target: HTMLInputElement, value: string) {
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(target, value);
    target.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
function contentType(kind: SteamWorkshopBrowseKind): HTMLButtonElement {
  return button(kind === "item" ? "Mods" : "Collections", element('[role="group"][aria-label="Workshop content type"]'));
}
function savedSettings(): Record<string, unknown> { return JSON.parse(details.settings_json); }
function modRow(id: string): HTMLElement | undefined {
  return [...fixture.querySelectorAll<HTMLElement>(".mw-entry-row")].find((row) => row.textContent?.includes(id));
}
function card(id: string): HTMLElement {
  const title = catalog.find((item) => item.id === id)?.title;
  const found = [...fixture.querySelectorAll<HTMLElement>(".mw-mod-card")]
    .find((entry) => entry.querySelector(".mw-mod-card-title")?.textContent === title);
  if (!found) throw new Error(`Missing Workshop card ${id}`);
  return found;
}
function quickAction(id: string): HTMLButtonElement {
  const found = card(id).querySelector<HTMLButtonElement>(".mw-mod-quick-action");
  if (!found) throw new Error(`Missing Workshop action ${id}`);
  return found;
}
function draw() {
  root.render(<ModWorkbenchBrowserHost details={details} moduleDetails={moduleDetails} epoch={epoch} onSaved={draw} />);
}
async function render() {
  epoch += 1;
  await act(async () => { draw(); });
  await settle(() => Boolean(fixture.querySelector(".mw-sort-pill")), "Workspace did not mount");
}
async function scenario(moduleId: string, settings: Record<string, unknown>) {
  moduleDetails = buildMockModuleDetails(moduleId);
  assert(moduleDetails.summary.id === moduleId, `Uses the repository contract for ${moduleId}`);
  details = { ...details, summary: { ...details.summary, id: `fixture-behavior-${moduleId}`, module_id: moduleId,
    status: "Stopped", active_process_count: 0 }, active_run: null, settings_json: JSON.stringify(settings) };
  await render();
}
function viewportFits() {
  assert(document.documentElement.scrollWidth <= innerWidth + 1 &&
    document.documentElement.scrollHeight <= innerHeight + 1, "Workspace fits the desktop viewport");
}
function directConfiguration() {
  const panel = element(".mw-detail-config-col");
  const selectedName = fixture.querySelector(".mw-entry-row--active .mw-entry-row-title")?.textContent;
  assert(panel.getAttribute("role") === "region" && panel.getAttribute("aria-label") === `Mod settings: ${selectedName}`,
    "Direct configuration retains its accessible region name");
  assert(!panel.querySelector(".mw-detail-col-title, .mw-selected-config-head, .mw-entry-detail-media") &&
    !fixture.querySelector(".mw-entry-info-button, .mw-entry-info-dialog"),
    "My Mods presents settings directly without a repeated title, preview or Info dialog");
  const options = element<HTMLSelectElement>(".dst-mod-spec-list select");
  const rect = options.getBoundingClientRect();
  const panelRect = panel.getBoundingClientRect();
  assert(rect.width > 80 && rect.height > 0 && rect.left >= panelRect.left && rect.right <= panelRect.right + 1 &&
    rect.top >= panelRect.top && rect.bottom <= panelRect.bottom + 1,
    "Direct configuration controls fit the remaining settings panel");
  assert(Boolean(panel.querySelector(".dst-mod-shard-field select")), "Direct settings retain the shard selector");
}
async function browseResponseScenarios() {
  const savedCatalog = catalog;
  const recoveryItem = workshopItem(ownedId, "Catalog recovered Mod");
  catalog = [recoveryItem];
  for (const fault of browseFaults) {
    browseFault = fault;
    await scenario("dontstarve", {});
    await settle(() => Boolean(fixture.querySelector(".mw-empty")) || fixture.textContent?.includes("Could not load Workshop results:") === true,
      `${fault} response did not settle`);
    assert(fixture.textContent?.includes("Could not load Workshop results:") && !fixture.querySelector(".mw-empty"),
      `${fault} is a retryable load failure instead of no matches; observed: ${fixture.textContent}`);
    assert(fixture.textContent?.includes("The Workshop catalog response is invalid."), `${fault} explains the invalid catalog response`);
    browseFault = null;
    await click(button("Retry"));
    await settle(() => fixture.querySelector(".mw-mod-card-title")?.textContent === recoveryItem.title, `${fault} retry did not recover`);
    assert(!fixture.textContent?.includes("Could not load Workshop results:"), `${fault} retry clears the previous failure`);
    recoveredBrowseFaults.push(fault);
  }
  for (const kind of ["item", "collection"] as const) {
    catalog = [];
    await scenario("dontstarve", {});
    if (kind === "collection") await click(contentType(kind));
    const directoryMessage = kind === "item" ? "No Mods are available in this Workshop view." : "No collections are available in this Workshop view.";
    await settle(() => fixture.querySelector(".mw-empty")?.textContent?.includes(directoryMessage) === true, `${kind} directory empty state is inaccurate`);
    assert(element<HTMLInputElement>(".mw-search-input").value === "", `${kind} directory empty state does not require a search or selection`);
    emptyBrowseStates.push(directoryMessage);
    catalog = kind === "item" ? [recoveryItem] : [{ ...recoveryItem, id: collectionId, title: "Recovered collection", item_kind: "collection" }];
    await click(button("Retry", element(".mw-empty")));
    await settle(() => fixture.querySelectorAll(".mw-mod-card").length === 1, `${kind} empty catalog retry did not refresh the directory`);
    await input(element<HTMLInputElement>(".mw-search-input"), "does-not-exist");
    const searchMessage = kind === "item" ? "No matching Mods. Try another name or paste a Workshop URL or ID."
      : "No matching collections. Try another name or paste a collection URL or ID.";
    await settle(() => fixture.querySelector(".mw-empty")?.textContent?.includes(searchMessage) === true, `${kind} search empty state is inaccurate`);
    emptyBrowseStates.push(searchMessage);
  }
  catalog = savedCatalog;
}
async function fileInventoryScenario(moduleId: string, record = true) {
  const names: Record<string, string> = { minecraft: "Orchard.jar", valheim: "DedicatedTweaks.dll", squad: "FixtureMap" };
  const target = `C:\\Fixture\\${moduleId}\\Mods`;
  inventory = { instance_id: `fixture-behavior-${moduleId}`, module_id: moduleId,
    source_label: "Fixture files", target_label: "Mod files", target_path: target, target_exists: true,
    items: [{ name: names[moduleId], path: `${target}\\${names[moduleId]}`, item_type: moduleId === "squad" ? "directory" : "file",
      inferred_id: null, file_count: 2, total_bytes: 8192 }] };
  if (moduleId === "squad") {
    inventory.items.push(
      { name: ownedId, path: `${target}\\${ownedId}`, item_type: "directory",
        inferred_id: null, file_count: 2, total_bytes: 8192 },
      { name: newId, path: `${target}\\${newId}`, item_type: "directory",
        inferred_id: null, file_count: 0, total_bytes: 0 }
    );
  }
  catalog = moduleId === "squad" ? [
    { ...workshopItem(ownedId, "Squad deployed Workshop Mod"), consumer_app_id: 393380 },
    { ...workshopItem(newId, "Squad empty download directory"), consumer_app_id: 393380 }
  ] : [];
  cached = new Set();
  await scenario(moduleId, {});
  assert(!moduleDetails.mods?.enablement, `${moduleId} exposes file inventory without an enablement list`);
  await click(button("My Mods"));
  await settle(() => Boolean(modRow(names[moduleId])), `${moduleId} inventory did not appear`);
  assert(!fixture.querySelector(".mw-entry-enabled-toggle"), `${moduleId} file inventory has no false enablement toggle`);
  await click(modRow(names[moduleId])!.querySelector<HTMLButtonElement>(".mw-entry-row-select")!);
  await click(button("Open Mod folder"));
  assert(openedPaths.at(-1) === target, `${moduleId} opens its own inventory target directory`);
  if (moduleId === "squad") {
    assert(Boolean(modRow(ownedId)) && Boolean(modRow(newId)), "Squad lists both numeric inventory directories alongside FixtureMap");
    const savesBefore = writes;
    const downloadsBefore = downloadRequests.length;
    await click(element<HTMLButtonElement>(".mw-sort-pills [role=tab]"));
    await settle(() => fixture.querySelectorAll(".mw-mod-card").length === 2 &&
      quickAction(ownedId).getAttribute("aria-label")?.includes("manage") === true &&
      !quickAction(newId).disabled, "Squad deployed inventory did not update its Workshop actions");
    assert(cached.size === 0, "Squad store status is derived without machine cache evidence");
    assert(quickAction(ownedId).classList.contains("mw-mod-quick-action--added") &&
      Boolean(quickAction(ownedId).querySelector('svg path[d="M20 6 9 17l-5-5"]')),
      "Squad deployed numeric directory exposes a checkmark and manage action");
    squadEmptyDirectoryInstalled = card(newId).classList.contains("mw-mod-card--installed");
    assert(squadEmptyDirectoryInstalled === false && !quickAction(newId).classList.contains("mw-mod-quick-action--added") &&
      quickAction(newId).getAttribute("aria-label")?.includes("Download") &&
      Boolean(quickAction(newId).querySelector('svg path[d="M12 5v14"]')) &&
      Boolean(quickAction(newId).querySelector('svg path[d="M5 12h14"]')),
      "Squad empty numeric directory retains the plus download action");
    await click(quickAction(ownedId));
    await settle(() => modRow(ownedId)?.classList.contains("mw-entry-row--active") === true &&
      [...fixture.querySelectorAll(".mw-selected-config-panel dd")].some((entry) => entry.textContent === `${target}\\${ownedId}`),
      "Squad manage action did not select the deployed numeric inventory directory");
    assert(button("My Mods").getAttribute("aria-selected") === "true", "Squad manage action navigates to My Mods");
    assert(!modRow(names[moduleId])!.classList.contains("mw-entry-row--active"), "Squad manage action does not leave FixtureMap selected");
    squadManagedPath = [...fixture.querySelectorAll(".mw-selected-config-panel dd")]
      .find((entry) => entry.textContent === `${target}\\${ownedId}`)?.textContent ?? null;
    assert(squadManagedPath === `${target}\\${ownedId}`, "Squad selected Mod details show the exact deployed inventory path");
    assert(writes === savesBefore && downloadRequests.length === downloadsBefore,
      "Managing a deployed Squad Mod neither downloads nor rewrites instance settings");
  }
  viewportFits();
  if (record) inventoryModules.push(moduleId);
}
async function run() {
  details = await readInstanceDetails("srv-dst-terminal-error");
  mockWindows("mod-behavior-fixture");
  mockIPC(createModWorkbenchFixtureIpc(() => ({ details, moduleDetails, catalog, cached, inventory, pageSize: browsePageSize }), {
    check: assert, save: (saved) => { details = saved; writes += 1; },
    openPath: (path) => { openedPaths.push(path); }, download: (ids) => { downloadRequests.push(ids); },
    browse: async (request, result) => {
      browseRequests.push(request);
      if (request.kind === "collection" && holdNextCollection) {
        holdNextCollection = false;
        await new Promise<void>((resolve) => { releaseCollection = resolve; });
      }
      if (browseFault === "missing-kind") delete result.browse_kind;
      if (browseFault === "wrong-kind") result.browse_kind = request.kind === "item" ? "collection" : "item";
      if (browseFault === "wrong-app") result.app_id = 304930;
      if (browseFault === "invalid-kind") result.browse_kind = 7;
      return result;
    }
  }), { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });

  for (const moduleId of ["minecraft", "valheim", "squad"]) await fileInventoryScenario(moduleId);

  inventory = null;
  catalog = [{ ...workshopItem(ownedId, "Unturned configured Mod"), consumer_app_id: 304930 }];
  cached = new Set([ownedId]);
  await scenario("unturned", { workshop_file_ids: ownedId });
  await click(button("My Mods"));
  await settle(() => Boolean(modRow(ownedId)), "Unturned configured row did not appear");
  assert(!modRow(ownedId)!.querySelector("input[type=checkbox]"), "Unturned has no irreversible disable switch");
  await click(modRow(ownedId)!.querySelector<HTMLButtonElement>(".mw-entry-remove-button")!);
  await settle(() => !modRow(ownedId), "Unturned removal did not complete");
  assert(savedSettings().workshop_file_ids === "", "Unturned removal persists its explicit configuration change");

  const disabledSettings = { enable_caves: true, shared_workshop_mod_ids: ownedId,
    master_enabled_workshop_mod_ids: "", caves_enabled_workshop_mod_ids: "",
    master_mod_configuration_options: { [ownedId]: { difficulty: 10 } },
    caves_mod_configuration_options: { [ownedId]: { difficulty: 10 } } };
  catalog = [workshopItem(ownedId, "Preserved disabled Mod")];
  cached = new Set();
  await scenario("dontstarve", disabledSettings);
  await click(button("My Mods"));
  await settle(() => [...fixture.querySelectorAll("button")].some((target) => target.textContent === "Download and read configuration"),
    "Missing Mod configuration did not offer file preparation");
  const savesBefore = writes;
  const downloadsBefore = downloadRequests.length;
  await click(button("Download and read configuration"));
  await settle(() => Boolean(fixture.querySelector(".dst-mod-spec-list")), "Prepared Mod options did not become available");
  const prepareSaves = writes - savesBefore;
  const prepareDownloads = downloadRequests.length - downloadsBefore;
  assert(prepareDownloads === 1 && prepareSaves === 0, "Reading missing configuration downloads once without saving enablement");
  assert(JSON.stringify(savedSettings()) === JSON.stringify(disabledSettings), "Preparation preserves both shard intentions and saved options");
  await render();
  await click(button("My Mods"));
  await settle(() => Boolean(fixture.querySelector(".dst-mod-spec-list")), "Prepared options did not survive remount");
  assert(modRow(ownedId)!.querySelector<HTMLInputElement>(".mw-entry-enabled-toggle")?.checked === false,
    "Prepared Mod remains disabled after remount");
  assert(element<HTMLSelectElement>(".dst-mod-spec-list select").value === "number:10", "Saved configuration survives preparation and remount");
  directConfiguration();

  const storeSettings = { enable_caves: true, shared_workshop_mod_ids: ownedId,
    master_enabled_workshop_mod_ids: ownedId, caves_enabled_workshop_mod_ids: ownedId };
  const owned = workshopItem(ownedId, "Configured server Mod");
  const fresh = workshopItem(newId, "New server Mod");
  const child = workshopItem(childId, "Remaining server Mod");
  const collection: SteamWorkshopLookupItem = { ...workshopItem(collectionId, "Server Mod collection"), item_kind: "collection",
    child_count: 2, children: [owned, child].map(({ id, title }) => ({ id, title, item_kind: "item", status: "resolved", consumer_app_id: 322330 })) };
  catalog = [owned, fresh, collection, child];
  browsePageSize = 2;
  cached = new Set([ownedId, childId]);
  await scenario("dontstarve", storeSettings);
  await settle(() => fixture.querySelectorAll(".mw-mod-card").length === 2 && !quickAction(newId).disabled,
    "Workshop actions did not become ready");
  assert(contentType("item").getAttribute("aria-pressed") === "true" &&
    contentType("collection").getAttribute("aria-pressed") === "false", "Workshop defaults to Mods with explicit selected content type");
  assert(![...fixture.querySelectorAll("button")].some((target) => target.textContent?.trim() === "Add from an ID list"),
    "Ordinary browsing does not repeat the manifest entry call to action");
  assert(Boolean(button("Manifest mode").compareDocumentPosition(contentType("item")) & Node.DOCUMENT_POSITION_FOLLOWING),
    "Content type controls follow the existing Manifest mode entry");
  assert(!fixture.textContent?.includes(collection.title), "Mods browsing excludes collection cards");
  assert(quickAction(ownedId).getAttribute("aria-label")?.includes("manage"), "Configured card exposes a manage action");
  assert(quickAction(newId).getAttribute("aria-label")?.includes("Download"), "New card exposes a download action");
  assert(!card(ownedId).querySelector(".mw-chip--lifecycle") && !card(newId).querySelector(".mw-chip--lifecycle"),
    "Ordinary configured and not-downloaded states use icons without repeated chips");
  await click(card(newId).querySelector<HTMLButtonElement>(".mw-mod-thumb")!);
  assert(!fixture.querySelector("[aria-label='Add to batch']") && !fixture.querySelector(".mw-install-strip"),
    "Store details do not expose the hidden batch queue");
  await click(button("Close details"));
  await click(quickAction(ownedId));
  await settle(() => Boolean(modRow(ownedId)), "Configured card did not navigate to My Mods");
  await click(element<HTMLButtonElement>(".mw-sort-pills [role=tab]"));
  await click(quickAction(newId));
  await settle(() => String(savedSettings().master_enabled_workshop_mod_ids).includes(newId), "New Mod was not configured");
  await settle(() => quickAction(newId).getAttribute("aria-label")?.includes("manage") === true, "Installed Mod action did not become manage");
  assert(downloadRequests.at(-1)?.join() === newId, "Single add downloads only the selected Mod");

  await input(element<HTMLInputElement>(".mw-search-input"), "server");
  await click(button("Top rated all time"));
  await settle(() => browseRequests.at(-1)?.query === "server" && browseRequests.at(-1)?.sort === "popular" &&
    !button("Next").disabled, "Search and sort did not settle before pagination");
  await click(button("Next"));
  await settle(() => element(".mw-pager-label").textContent?.trim() === "Page 2" &&
    fixture.querySelectorAll(".mw-mod-card").length === 1, "Mod page two did not load");
  await click(card(childId).querySelector<HTMLButtonElement>(".mw-mod-thumb")!);
  assert(Boolean(fixture.querySelector(".mw-store-layout--detail")), "A Mod detail is open before changing content type");
  holdNextCollection = true;
  await click(contentType("collection"));
  await settle(() => releaseCollection !== null, "Collection request was not issued");
  assert(fixture.querySelectorAll(".mw-mod-card").length === 0 && !fixture.querySelector(".mw-store-layout--detail"),
    "Changing content type immediately clears old Mod results and their open detail");
  const collectionRequest = browseRequests.at(-1)!;
  assert(collectionRequest.kind === "collection" && collectionRequest.page === 1 &&
    collectionRequest.query === "server" && collectionRequest.sort === "popular",
    "Changing content type resets pagination while retaining search and sort");
  assert(contentType("collection").getAttribute("aria-pressed") === "true", "Collections remains selected while its results load");
  await act(async () => { releaseCollection!(); releaseCollection = null; });
  await settle(() => fixture.querySelectorAll(".mw-mod-card").length === 1, "Filtered collection summary did not load");
  assert(!card(collectionId).querySelector(".mw-chip--lifecycle-unsupported"),
    "A collection summary without loaded children is not misreported as unsupported");
  assert(quickAction(collectionId).getAttribute("aria-label")?.startsWith("View details for") &&
    !quickAction(collectionId).disabled, "A collection summary offers inspection before installation");
  await click(quickAction(collectionId));
  await settle(() => Boolean(card(collectionId).querySelector(".mw-chip--lifecycle-partially-configured")) &&
    !quickAction(collectionId).disabled, "Loaded collection details did not update its card state and add action");
  assert(Boolean(card(collectionId).querySelector(".mw-chip--lifecycle-partially-configured")), "Collection exposes incomplete instance configuration");
  assert(!button("Download and add", element(".mw-store-detail")).disabled,
    "A partially configured collection can be added after its detail lookup loads members");
  assert(element(".mw-pager-label").textContent?.trim() === "Page 1" &&
    !fixture.querySelector(".mw-mod-card-title")?.textContent?.includes(child.title), "Collections contain only collection results on page one");
  await click(contentType("item"));
  await settle(() => fixture.querySelectorAll(".mw-mod-card").length === 2 &&
    element(".mw-pager-label").textContent?.trim() === "Page 1", "Returning to Mods did not restore its own first page");
  assert(Boolean(card(ownedId)) && Boolean(card(newId)) && !fixture.textContent?.includes(collection.title),
    "Returning to Mods uses its own cached results without leaking collection cards");
  await click(button("Manifest mode"));
  assert(button("Manifest mode").getAttribute("aria-selected") === "true", "Existing Manifest mode remains reachable");
  await click(contentType("collection"));
  await settle(() => fixture.querySelectorAll(".mw-mod-card").length === 1, "Content type selection did not leave Manifest mode for browsing");
  assert(button("Manifest mode").getAttribute("aria-selected") === "false" &&
    element<HTMLInputElement>(".mw-search-input").value === "server" &&
    button("Top rated all time").getAttribute("aria-selected") === "true", "Content type selection returns to browsing with search and sort intact");
  assert(Boolean(card(collectionId).querySelector(".mw-chip--lifecycle-partially-configured")) &&
    quickAction(collectionId).getAttribute("aria-label")?.includes("Download") && !quickAction(collectionId).disabled,
    "Collection details without a full description retain loaded children and the add action after classification round trip");
  await click(quickAction(collectionId));
  await settle(() => String(savedSettings().master_enabled_workshop_mod_ids).includes(childId), "Collection did not add its missing member");
  for (const shard of ["master", "caves"]) assert(String(savedSettings()[`${shard}_enabled_workshop_mod_ids`]).includes(childId),
    `Collection completion enables the remaining member in ${shard}`);
  assert(downloadRequests.at(-1)?.join() === [ownedId, childId].join(), "Collection prepares all members before configuration");
  await settle(() => quickAction(collectionId).getAttribute("aria-label")?.includes("manage") === true, "Completed collection did not become manageable");
  await browseResponseScenarios();

  if (innerWidth >= 1200) {
    cached = new Set([ownedId, childId]);
    await scenario("dontstarve", storeSettings);
    await click(contentType("collection"));
    await settle(() => fixture.querySelectorAll(".mw-mod-card").length === 1, "Final collection summary was not ready");
    await click(card(collectionId).querySelector<HTMLButtonElement>(".mw-mod-thumb")!);
    await settle(() =>
      Boolean(card(collectionId).querySelector(".mw-chip--lifecycle-partially-configured")) === true, "Final store screenshot was not ready");
    await click(button("Close details"));
  } else {
    await scenario("dontstarve", disabledSettings);
    await click(button("My Mods"));
    await settle(() => Boolean(fixture.querySelector(".dst-mod-spec-list")), "Final direct configuration screenshot was not ready");
    directConfiguration();
  }
  catalog = [];
  browseFault = innerWidth >= 1200 ? "missing-kind" : null;
  await scenario("dontstarve", {});
  if (!browseFault) await click(contentType("collection"));
  await settle(() => fixture.textContent?.includes(browseFault ? "The Workshop catalog response is invalid."
    : "No collections are available in this Workshop view.") === true, "Final catalog state screenshot was not ready");
  viewportFits();
  assert(errors.length === 0, "Browser console and uncaught errors remain empty");
  return { status: "passed", checks, inventory_modules: inventoryModules, prepare_saves: prepareSaves,
    prepare_downloads: prepareDownloads, collection_completed: true, browser_errors: errors,
    classified_browsing: true, collection_summary_resolved: true, direct_configuration: true, browse_requests: browseRequests,
    response_failure_recovered: recoveredBrowseFaults, empty_browse_states: emptyBrowseStates,
    squad_managed_path: squadManagedPath, squad_empty_directory_installed: squadEmptyDirectoryInstalled,
    writes, downloads: downloadRequests, opened_paths: openedPaths, viewport: { width: innerWidth, height: innerHeight } };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error("Mod behavior acceptance exceeded 45 seconds")), 45000);
})]).catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => {
    const nonce = new URLSearchParams(location.search).get("nonce");
    return fetch(nonce ? `/__reliability_result/${nonce}` : "/__mod_behavior_result", { method: "POST", body: JSON.stringify(report) });
  });
