import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { readInstanceDetails } from "../../src/api";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import type { InstanceDetails, SteamWorkshopLookupItem } from "../../src/types";
import { createModWorkbenchFixtureIpc, ModWorkbenchBrowserHost, workshopItem } from "./mod-workbench-browser-support";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "en-US");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:16px;box-sizing:border-box;display:flex;flex-direction:column";
const root = createRoot(fixture);
const checks: string[] = [];
const errors: string[] = [];
const writes: { instanceId: string; settings: Record<string, unknown> }[] = [];
const downloads: string[][] = [];
const moduleDetails = buildMockModuleDetails("dontstarve");
const members = ["111111", "222222", "333333"];
const collectionA = "456789", collectionB = "567890", nativeOnlyId = "678901";
const cached = new Set(members);
const instances = new Map<string, InstanceDetails>();
let details: InstanceDetails;
let epoch = 0;
let offline = false;
let failSave = false;
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(`${label}: ${fixture.textContent}`);
  checks.push(label);
}
function element<T extends HTMLElement = HTMLElement>(selector: string, within: ParentNode = fixture): T {
  const value = within.querySelector<T>(selector);
  if (!value) throw new Error(`Missing ${selector}`);
  return value;
}
function button(label: string, within: ParentNode = fixture): HTMLButtonElement {
  const value = [...within.querySelectorAll<HTMLButtonElement>("button")].find((entry) =>
    entry.textContent?.trim() === label || entry.getAttribute("aria-label") === label);
  if (!value) throw new Error(`Missing button ${label}`);
  return value;
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
  const bounds = target.getBoundingClientRect();
  assert(bounds.width > 0 && bounds.height > 0 && bounds.left >= -1 && bounds.right <= innerWidth + 1 &&
    bounds.top >= -1 && bounds.bottom <= innerHeight + 1, "Collection interaction stays inside the viewport");
  assert(!(target instanceof HTMLButtonElement) || !target.disabled, "Collection interaction is enabled");
  await act(async () => { target.focus(); target.click(); });
}
async function input(target: HTMLTextAreaElement, value: string) {
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(target, value);
    target.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
function draw() { root.render(<ModWorkbenchBrowserHost details={details} moduleDetails={moduleDetails} epoch={epoch} onSaved={draw} />); }
async function mount(instanceId = details.summary.id) {
  details = structuredClone(instances.get(instanceId)!);
  epoch += 1;
  await act(async () => { draw(); });
  await settle(() => Boolean(fixture.querySelector(".mw-sort-pill")), "Workbench did not mount");
}
function settings(): Record<string, unknown> { return JSON.parse(details.settings_json); }
function records(): { id: string; title: string; member_ids: string[] }[] {
  return (settings().steam_workshop_collections ?? []) as { id: string; title: string; member_ids: string[] }[];
}
function collection(id: string, title: string, ids: string[]): SteamWorkshopLookupItem {
  return { ...workshopItem(id, title), item_kind: "collection", child_count: ids.length,
    children: ids.map((memberId) => ({ id: memberId, title: `Member ${memberId}`, item_kind: "item", status: "resolved", consumer_app_id: 322330 })) };
}
const catalog = [...members.map((id) => workshopItem(id, `Member ${id}`)),
  collection(collectionA, "Shared survival collection", members.slice(0, 2)),
  collection(collectionB, "Shared caves collection", members.slice(1)),
  collection(nativeOnlyId, "Remote native collection", [members[0]])];
function contentType(label: "Mods" | "Collections") { return button(label, element('[role="group"][aria-label="Workshop content type"]')); }
function card(id: string) {
  const title = catalog.find((item) => item.id === id)!.title;
  const found = [...fixture.querySelectorAll<HTMLElement>(".mw-mod-card")].find((item) => item.querySelector(".mw-mod-card-title")?.textContent === title);
  if (!found) throw new Error(`Missing public collection ${id}`);
  return found;
}
function quickAction(id: string) { return element<HTMLButtonElement>(".mw-mod-quick-action", card(id)); }
function row(id: string): HTMLElement | undefined {
  return [...fixture.querySelectorAll<HTMLElement>(".mw-collection-row")].find((entry) => entry.textContent?.includes(`#${id}`));
}
function selection(id: string) {
  const found = row(id);
  if (!found) throw new Error(`Missing owned collection ${id}`);
  return element<HTMLButtonElement>(".mw-collection-select", found);
}
async function expandCollection(id: string) {
  if (selection(id).getAttribute("aria-expanded") !== "true") await click(selection(id));
}
function visibleMembers() {
  return [...fixture.querySelectorAll<HTMLButtonElement>(".mw-collection-member")].filter((entry) => entry.getBoundingClientRect().height > 0);
}
async function openLibrary() {
  await click(contentType("Collections"));
  await click(button("My collections"));
  await settle(() => fixture.querySelector(".mw-detail-col-title")?.textContent?.startsWith("Collections") === true,
    "Owned collection library did not appear");
}
function withoutCollections(value: Record<string, unknown>) {
  const { steam_workshop_collections, shared_workshop_collection_ids, ...rest } = value;
  void steam_workshop_collections; void shared_workshop_collection_ids;
  return rest;
}

async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const template = await readInstanceDetails("srv-dst-terminal-error");
  const initial = { enable_caves: true, shared_workshop_mod_ids: members.join("\n"),
    master_enabled_workshop_mod_ids: members.join("\n"), caves_enabled_workshop_mod_ids: members.join("\n"),
    master_mod_configuration_options: { [members[1]]: { difficulty: 10 } },
    caves_mod_configuration_options: { [members[1]]: { difficulty: 10 } },
    shared_workshop_collection_ids: "", steam_workshop_collections: [] };
  for (const id of ["fixture-collections-a", "fixture-collections-b"]) instances.set(id, {
    ...template, summary: { ...template.summary, id, module_id: "dontstarve", status: "Stopped", active_process_count: 0 },
    active_run: null, settings_json: JSON.stringify(initial)
  });
  details = instances.get("fixture-collections-a")!;
  const baseIpc = createModWorkbenchFixtureIpc(() => ({ details, moduleDetails, catalog, cached }), {
    check: assert,
    save: (saved) => { details = saved; instances.set(saved.summary.id, structuredClone(saved));
      writes.push({ instanceId: saved.summary.id, settings: JSON.parse(saved.settings_json) }); },
    download: (ids) => { downloads.push(ids); }
  });
  mockWindows("mod-collections-fixture");
  mockIPC((command, payload) => {
    if (offline && ["lookup_steam_workshop_items", "read_steam_workshop_item_details", "search_steam_workshop_items"].includes(command)) throw new Error("Fixture Workshop is offline");
    if (failSave && command === "update_instance_record_if_current") throw new Error("Fixture collection save failed");
    return baseIpc(command, payload);
  }, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  await mount();
  await click(contentType("Collections"));
  await settle(() => fixture.querySelectorAll(".mw-mod-card").length === 3, "Public collection catalog did not load");
  await click(quickAction(collectionA));
  await settle(() => !quickAction(collectionA).getAttribute("aria-label")?.startsWith("View details"), "Full collection metadata did not load");
  assert(!quickAction(collectionA).classList.contains("mw-mod-quick-action--added") &&
    Boolean(quickAction(collectionA).querySelector('svg path[d="M12 5v14"]')),
    "A public collection with every member installed remains addable until this instance owns its record");
  const writesBeforeAdd = writes.length;
  await click(quickAction(collectionA));
  await settle(() => records().some((record) => record.id === collectionA), "Ordinary collection add did not save ownership");
  const ownedA = records().find((record) => record.id === collectionA)!;
  assert(ownedA.title === "Shared survival collection" && JSON.stringify(ownedA.member_ids) === JSON.stringify(members.slice(0, 2)),
    "Ordinary add persists collection ID, title and member snapshot");
  assert(writes.length === writesBeforeAdd + 1 && settings().shared_workshop_collection_ids === initial.shared_workshop_collection_ids,
    "Ordinary add persists the collection snapshot in one version-checked save without adding native collection subscriptions");
  await click(button("My collections"));
  await settle(() => Boolean(row(collectionA)), "Owned collection did not appear in My collections");
  assert(selection(collectionA).getAttribute("aria-expanded") === "true" && visibleMembers().length === 2,
    "The initially selected collection exposes its two member rows");
  await click(selection(collectionA));
  assert(selection(collectionA).getAttribute("aria-expanded") === "false" && visibleMembers().length === 0,
    "Collapsing a collection hides its member rows");
  await expandCollection(collectionA);
  assert(visibleMembers().length === 2, "Reopening a collection exposes its member rows");
  await click(contentType("Mods"));
  assert(button("My Mods").getAttribute("aria-selected") === "true" && !fixture.querySelector(".mw-mod-grid"),
    "Changing to Mods while in the library stays in My Mods");
  await click(contentType("Collections"));
  assert(button("My collections").getAttribute("aria-selected") === "true" && Boolean(row(collectionA)),
    "Changing back to Collections stays in the owned library");

  await click(button("Manifest mode"));
  await input(element<HTMLTextAreaElement>(".mw-manifest-input"), collectionB);
  await click(button("Check list"));
  await settle(() => Boolean(fixture.querySelector(".mw-manifest-summary")) && !button("Download missing and enable").disabled,
    "Manifest did not inspect the second collection");
  const writesBeforeManifest = writes.length;
  await click(button("Download missing and enable"));
  await settle(() => records().some((record) => record.id === collectionB), "Manifest add did not persist collection ownership");
  assert(writes.length === writesBeforeManifest + 1 && records().length === 2 &&
    JSON.stringify(records().find((record) => record.id === collectionB)?.member_ids) === JSON.stringify(members.slice(1)),
    "Manifest add stores the second overlapping collection in one version-checked save");
  assert(JSON.stringify(withoutCollections(settings())) === JSON.stringify(withoutCollections(initial)),
    "Registering already-installed collections preserves existing enablement and options");

  offline = true;
  await mount();
  await openLibrary();
  await settle(() => Boolean(row(collectionA)) && Boolean(row(collectionB)), "Saved collections did not survive an offline remount");
  await expandCollection(collectionA);
  await expandCollection(collectionB);
  assert(selection(collectionA).getAttribute("aria-expanded") === "true" && selection(collectionB).getAttribute("aria-expanded") === "true" &&
    visibleMembers().length === 4, "Two collections can expand together using their offline member snapshots");
  await click(selection(collectionA));
  assert(selection(collectionA).getAttribute("aria-expanded") === "false" && selection(collectionB).getAttribute("aria-expanded") === "true" &&
    visibleMembers().length === 2, "Collapsing a previously expanded collection leaves the other collection expanded");
  await click(visibleMembers().find((entry) => entry.textContent?.includes(members[1]))!);
  await settle(() => button("My collections").getAttribute("aria-selected") === "true" &&
    fixture.querySelector('.mw-collection-member[aria-pressed="true"]')?.textContent?.includes(members[1]) === true &&
    Boolean(fixture.querySelector(".dst-mod-spec-list")), "Member did not open its settings within My collections");
  assert(fixture.querySelector('.mw-detail-config-col[role="region"]')?.getAttribute("aria-label")?.includes(members[1]),
    "Owned member navigation preserves the collection view and opens the selected member's real settings offline");
  const beforeSnapshotChange = settings();
  const changedSnapshot = { ...beforeSnapshotChange, steam_workshop_collections: records().map((record) => record.id === collectionB
    ? { ...record, member_ids: record.member_ids.filter((id) => id !== members[1]) } : record) };
  details = { ...details, settings_json: JSON.stringify(changedSnapshot) };
  instances.set(details.summary.id, structuredClone(details));
  await act(async () => { draw(); });
  await settle(() => fixture.querySelector(".mw-detail-config-panel")?.textContent?.includes("Select a Mod from a collection") === true &&
    !fixture.querySelector(".dst-mod-spec-list"), "Changed collection snapshot left the removed member's configuration visible");
  assert(String(settings().master_enabled_workshop_mod_ids).includes(members[1]) && button("My collections").getAttribute("aria-selected") === "true",
    "Removing a selected member from this collection snapshot clears its panel even while that Mod stays globally enabled");
  details = { ...details, settings_json: JSON.stringify(beforeSnapshotChange) };
  instances.set(details.summary.id, structuredClone(details));
  await act(async () => { draw(); });
  const beforeRemoval = settings();
  const downloadsBeforeRemoval = downloads.length;
  const filesBeforeRemoval = [...cached].sort();
  failSave = true;
  await click(button("Remove collection: Shared survival collection"));
  await settle(() => Boolean(fixture.querySelector(".mw-collection-removal-dialog[open]")), "Removal review did not open");
  await click(button("Remove collection record only"));
  await settle(() => fixture.textContent?.includes("Fixture collection save failed") === true, "Failed removal did not report its save failure");
  assert(records().length === 2 && Boolean(row(collectionA)), "Failed collection removal preserves its saved ownership row");
  failSave = false;
  await click(button("Remove collection record only"));
  await settle(() => !row(collectionA) && records().length === 1, "Collection removal did not persist");
  const remainingNativeIds = String(beforeRemoval.shared_workshop_collection_ids).split(/\D+/).filter((id) => id && id !== collectionA);
  assert(records()[0].id === collectionB && JSON.stringify(String(settings().shared_workshop_collection_ids).split(/\D+/).filter(Boolean)) ===
    JSON.stringify(remainingNativeIds), "Removing one collection preserves the overlapping collection and unrelated native IDs");
  assert(JSON.stringify(withoutCollections(settings())) === JSON.stringify(withoutCollections(beforeRemoval)) &&
    JSON.stringify([...cached].sort()) === JSON.stringify(filesBeforeRemoval) && downloads.length === downloadsBeforeRemoval,
    "Collection removal leaves all enabled Mods, options and downloaded files unchanged");
  await mount();
  await openLibrary();
  assert(!row(collectionA) && Boolean(row(collectionB)), "Collection removal survives remount");
  offline = false;
  const incomplete = settings();
  for (const key of ["shared_workshop_mod_ids", "master_enabled_workshop_mod_ids", "caves_enabled_workshop_mod_ids"]) {
    incomplete[key] = String(incomplete[key]).split(/\D+/).filter((id) => id && id !== members[2]).join("\n");
  }
  // Explicit removal retains the collection snapshot but marks its member absent.
  incomplete.dst_removed_workshop_mod_ids = [members[2]];
  instances.set(details.summary.id, { ...details, settings_json: JSON.stringify(incomplete) });
  cached.delete(members[2]);
  await mount();
  await openLibrary();
  await expandCollection(collectionB);
  await settle(() => !button("Add missing Mods", row(collectionB)).disabled, "Missing collection member was not repairable");
  await click(button("Add missing Mods", row(collectionB)));
  await settle(() => String(settings().master_enabled_workshop_mod_ids).includes(members[2]) && cached.has(members[2]), "Missing member repair did not restore installation and enablement");
  assert(JSON.stringify(downloads.at(-1)) === JSON.stringify([members[2]]) && String(settings().caves_enabled_workshop_mod_ids).includes(members[2]),
    "Add missing Mods prepares only the missing member and enables it in both shards");
  assert(records().length === 1 && records()[0].id === collectionB && JSON.stringify(records()[0].member_ids) === JSON.stringify(members.slice(1)) &&
    JSON.stringify(settings().master_mod_configuration_options) === JSON.stringify(initial.master_mod_configuration_options),
    "Repair preserves the collection snapshot and existing member options");
  offline = true;
  await mount("fixture-collections-b");
  await openLibrary();
  assert(!fixture.querySelector(".mw-collection-row") && records().length === 0, "Another instance does not inherit collection ownership");

  const legacySettings = { ...initial, shared_workshop_collection_ids: nativeOnlyId };
  instances.set(details.summary.id, { ...details, settings_json: JSON.stringify(legacySettings) });
  await mount();
  await openLibrary();
  await settle(() => Boolean(row(nativeOnlyId)), "DST native collection without a snapshot did not appear offline");
  await expandCollection(nativeOnlyId);
  await settle(() => Boolean(fixture.querySelector(".mw-collection-members-empty button")), "Native collection member fallback did not finish loading");
  const memberEmpty = element(".mw-collection-members-empty"), memberEmptyBounds = memberEmpty.getBoundingClientRect();
  const memberEmptyContent = [...memberEmpty.childNodes].filter((node) => node.textContent?.trim()).map((node) => {
    if (node instanceof HTMLElement) return node.getBoundingClientRect();
    const range = document.createRange(); range.selectNodeContents(node); return range.getBoundingClientRect();
  }).filter((rect) => rect.width && rect.height);
  const emptyCenter = { x: (Math.min(...memberEmptyContent.map((rect) => rect.left)) + Math.max(...memberEmptyContent.map((rect) => rect.right))) / 2,
    y: (Math.min(...memberEmptyContent.map((rect) => rect.top)) + Math.max(...memberEmptyContent.map((rect) => rect.bottom))) / 2 };
  assert(Math.abs(emptyCenter.x - (memberEmptyBounds.left + memberEmptyBounds.right) / 2) <= 2 &&
    Math.abs(emptyCenter.y - (memberEmptyBounds.top + memberEmptyBounds.bottom) / 2) <= 2,
    "Native-only collection fallback and retry are centered inside their member panel");
  await click(element<HTMLButtonElement>('button[aria-label^="Remove collection:"]', row(nativeOnlyId)));
  await settle(() => Boolean(fixture.querySelector(".mw-collection-removal-dialog[open]")), "Native-only removal review did not open");
  await click(button("Remove collection record only"));
  await settle(() => !row(nativeOnlyId), "Native-only collection could not be removed offline");
  assert(!String(settings().shared_workshop_collection_ids).includes(nativeOnlyId) &&
    JSON.stringify(withoutCollections(settings())) === JSON.stringify(withoutCollections(legacySettings)),
    "Removing a native-only DST collection preserves its Mods and options");

  await mount("fixture-collections-a");
  await openLibrary();
  await expandCollection(collectionB);
  assert(visibleMembers().length === 2, "Remaining collection member snapshot is visible in the final layout");
  assert(document.documentElement.scrollWidth <= innerWidth + 1 && document.documentElement.scrollHeight <= innerHeight + 1,
    "Collection library fits the desktop viewport");
  assert(errors.length === 0, "Collection browser has no console or uncaught errors");
  return { status: "passed", checks, browser_errors: errors, normal_add_persisted: true, manifest_add_persisted: true,
    offline_snapshot: true, accordion: true, overlap_preserved: true, missing_members_repaired: true, instance_isolated: true, native_only_removed: true,
    writes, downloads, viewport: { width: innerWidth, height: innerHeight } };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error("Collection acceptance exceeded 45 seconds")), 45000); })])
  .catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`, { method: "POST", body: JSON.stringify(report) }));
