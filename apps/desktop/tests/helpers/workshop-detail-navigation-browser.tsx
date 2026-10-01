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
const root = createRoot(fixture), checks: string[] = [], errors: string[] = [];
const moduleDetails = buildMockModuleDetails("dontstarve");
const parentId = "456789", nestedId = "567890", clientCollectionId = "678901";
const serverId = "111111", clientId = "222222", unknownId = "333333", deepId = "777777";
const server = { ...workshopItem(serverId, "Server controls"), description: "Full member details outside the collection catalog." };
const client = { ...workshopItem(clientId, "Client overlay"), tags: ["client_only_mod"] };
const recovered = workshopItem(unknownId, "Recovered unknown member");
const deep = workshopItem(deepId, "Deep server member");
function collection(id: string, title: string, members: SteamWorkshopLookupItem[]): SteamWorkshopLookupItem {
  return { ...workshopItem(id, title), item_kind: "collection", child_count: members.length,
    children: members.map(({ id, title, item_kind, status, tags }) => ({ id, title, item_kind, status, tags, consumer_app_id: 322330 })) };
}
const nested = collection(nestedId, "Nested Collection", [deep]);
const summaryOnly = "SUMMARY ONLY: a short browse excerpt, not the complete description.";
const fullDescription = "Complete collection description received with its member list.\n\nSecond paragraph from the creator.\nA separate final line.";
const incompleteCollectionMessage = "This collection contains nested or unresolved entries. Add the individual Mods instead.";
const parent = { ...collection(parentId, "Fixture Collection", [server, client,
  { ...recovered, title: "Unresolved member with a deliberately long descriptive name", item_kind: "unknown", status: "unavailable", tags: ["client_only_mod"] }, nested]),
  description_excerpt: summaryOnly, description: fullDescription };
const clientCollection = collection(clientCollectionId, "Client Collection", [client]);
const catalog = [parent, nested, clientCollection, server, client, recovered, deep];
let details: InstanceDetails, writes = 0, downloads = 0, unknownAttempts = 0, failUnknown = true;
let holdParent = true;
let parentMetadataReturned = false;
const heldParents: (() => void)[] = [];
const cachedScrollPositions: number[] = [];
const browseRequests: { kind: string; sort: string; query: string }[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
function assert(value: unknown, label: string): asserts value { if (!value) throw new Error(`${label}: ${fixture.textContent}`); checks.push(label); }
function element<T extends HTMLElement = HTMLElement>(selector: string, within: ParentNode = fixture): T {
  const found = within.querySelector<T>(selector); if (!found) throw new Error(`Missing ${selector}`); return found;
}
function button(label: string, within: ParentNode = fixture) {
  const found = [...within.querySelectorAll<HTMLButtonElement>("button")].find((entry) => entry.textContent?.trim() === label || entry.getAttribute("aria-label") === label);
  if (!found) throw new Error(`Missing button ${label}`); return found;
}
async function settle(predicate: () => boolean, label: string) {
  const deadline = performance.now() + 6000;
  while (!predicate()) {
    if (performance.now() > deadline) throw new Error(`${label}: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(node: HTMLElement) {
  node.scrollIntoView({ block: "nearest" });
  const rect = node.getBoundingClientRect();
  assert(rect.width > 0 && rect.height > 0 && rect.left >= -1 && rect.right <= innerWidth + 1 && rect.top >= -1 && rect.bottom <= innerHeight + 1,
    "Workshop navigation target fits the viewport");
  await act(async () => { node.focus(); node.click(); });
}
async function nativeEnter() {
  await act(async () => {
    const response = await fetch(`/__reliability_key/${new URLSearchParams(location.search).get("nonce")}/Enter`, { method: "POST" });
    assert(response.ok, "Native Enter was dispatched");
  });
}
function detail() { return element(".mw-store-detail"); }
function child(id: string) {
  const found = [...detail().querySelectorAll<HTMLButtonElement>(".mw-store-detail-child")].find((node) => node.querySelector("small")?.textContent === id);
  if (!found) throw new Error(`Missing child ${id}`); return found;
}
async function titleIs(title: string) { await settle(() => fixture.querySelector(".mw-store-detail h2")?.textContent === title, `Detail title did not become ${title}`); }
async function openCollection(id = parentId) {
  const title = catalog.find((item) => item.id === id)!.title;
  const card = [...fixture.querySelectorAll<HTMLElement>(".mw-mod-card")].find((node) => node.querySelector(".mw-mod-card-title")?.textContent === title)!;
  await click(element<HTMLButtonElement>(".mw-mod-thumb", card));
  await titleIs(title!);
  await settle(() => !fixture.textContent?.includes("Loading Mod details…"), "Collection details did not finish loading");
}
async function back(title: string) { await click(element<HTMLButtonElement>(".mw-store-detail-back")); await titleIs(title); }
async function releaseParent() {
  holdParent = false;
  await act(async () => { for (const release of heldParents.splice(0)) release(); });
}
function assertCachedBody(label: string) {
  assert(detail().textContent?.includes(fullDescription) && detail().querySelectorAll(".mw-store-detail-child").length === 3 &&
    !detail().querySelector(".mw-store-detail-loading") && detail().getAttribute("aria-busy") !== "true", label);
}
function detailBounds() {
  const { x, y, width, height } = detail().getBoundingClientRect();
  return { x, y, width, height };
}
function loadingCenter() {
  const status = element(".mw-store-detail-loading"), pane = element(".mw-store-detail-scroll");
  const rects: DOMRect[] = [...status.querySelectorAll("svg")].map((node) => node.getBoundingClientRect());
  const walker = document.createTreeWalker(status, NodeFilter.SHOW_TEXT);
  while (walker.nextNode()) {
    if (!walker.currentNode.textContent?.trim()) continue;
    const range = document.createRange(); range.selectNodeContents(walker.currentNode);
    rects.push(...range.getClientRects());
  }
  assert(rects.length > 0, "The loading status has visible content");
  const bounds = pane.getBoundingClientRect(), style = getComputedStyle(pane);
  const contentX = bounds.left + parseFloat(style.paddingLeft) +
    (pane.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight)) / 2;
  const contentY = bounds.top + parseFloat(style.paddingTop) +
    (pane.clientHeight - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom)) / 2;
  return { x: Math.abs((Math.min(...rects.map((rect) => rect.left)) + Math.max(...rects.map((rect) => rect.right))) / 2 - contentX),
    y: Math.abs((Math.min(...rects.map((rect) => rect.top)) + Math.max(...rects.map((rect) => rect.bottom))) / 2 - contentY) };
}
function descriptionLineGaps() {
  const description = element(".mw-store-detail-description"), text = description.firstChild!;
  const characterTop = (index: number) => {
    const range = document.createRange(); range.setStart(text, index); range.setEnd(text, index + 1);
    return range.getBoundingClientRect().top;
  };
  const second = fullDescription.indexOf("Second paragraph"), final = fullDescription.indexOf("A separate");
  assert(description.textContent === fullDescription, "Complete descriptions retain the author's original paragraph and line breaks");
  return { paragraph: characterTop(second) - characterTop(second - 3), line: characterTop(final) - characterTop(final - 2) };
}
async function finishCachedRefresh() {
  const pane = element(".mw-store-detail-scroll");
  await act(async () => { pane.scrollTop = Math.min(64, pane.scrollHeight - pane.clientHeight); });
  const before = pane.scrollTop;
  await releaseParent();
  assert(Math.abs(pane.scrollTop - before) <= 1, "Completing a cached detail refresh does not shift its current reading position");
  cachedScrollPositions.push(before);
}
async function run() {
  const template = await readInstanceDetails("srv-dst-terminal-error");
  details = { ...template, summary: { ...template.summary, id: "fixture-detail-navigation", status: "Stopped", active_process_count: 0 },
    active_run: null, settings_json: JSON.stringify({ shared_workshop_collection_ids: parentId }) };
  const draw = () => root.render(<ModWorkbenchBrowserHost details={details} moduleDetails={moduleDetails} epoch={0} onSaved={draw} />);
  const baseIpc = createModWorkbenchFixtureIpc(() => ({ details, moduleDetails, catalog, cached: new Set<string>() }), {
    check: assert, save: (saved) => { details = saved; writes += 1; }, download: () => { downloads += 1; },
    browse: (request, result) => { browseRequests.push({ kind: request.kind, sort: request.sort, query: request.query }); return result; }
  });
  mockWindows("workshop-detail-navigation");
  mockIPC(async (command, payload) => {
    const args = payload as { id?: string; ids?: string[] } | undefined;
    const ids = args?.ids ?? (args?.id ? [args.id] : []);
    if (command === "read_steam_workshop_item_details" && ids[0] === parentId && holdParent) {
      await new Promise<void>((resolve) => heldParents.push(resolve));
    }
    if (command === "read_steam_workshop_item_details" && ids.length === 1 && ids[0] === unknownId) {
      unknownAttempts += 1;
      if (failUnknown) throw new Error("Fixture member detail unavailable");
    }
    const result = await baseIpc(command, payload);
    if (command === "lookup_steam_workshop_items" && ids.includes(parentId)) parentMetadataReturned = true;
    return result;
  }, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  await act(async () => { draw(); });
  await settle(() => Boolean(fixture.querySelector(".mw-sort-pill")), "Workbench did not mount");
  await click(button("Most subscribed"));
  await settle(() => browseRequests.some((request) => request.kind === "item" && request.sort === "subscribers"), "Item subscribers sort did not load");
  await click(button("Collections", element('[role="group"][aria-label="Workshop content type"]')));
  assert(![...fixture.querySelectorAll(".mw-sort-pill")].some((entry) => entry.textContent?.trim() === "Most subscribed") &&
    button("Popular this week").getAttribute("aria-selected") === "true", "Collections omit unsupported subscribers sorting and reset it to Popular this week");
  const search = element<HTMLInputElement>(".mw-search-input");
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(search, "Collection");
    search.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await settle(() => browseRequests.some((request) => request.kind === "collection" && request.query === "Collection") &&
    fixture.querySelectorAll(".mw-mod-card").length === 3 && fixture.querySelector(".mw-mod-grid")?.getAttribute("aria-busy") === "false",
  "Collection search did not finish loading");
  const parentCard = [...fixture.querySelectorAll<HTMLElement>(".mw-mod-card")].find((node) => node.querySelector(".mw-mod-card-title")?.textContent === parent.title)!;
  await click(element<HTMLButtonElement>(".mw-mod-thumb", parentCard));
  await settle(() => heldParents.length === 1, "The initial complete-detail request was not held");
  assert(parentMetadataReturned, "Existing native collection ownership supplies batch metadata before the localized detail response");
  assert(detail().getAttribute("aria-busy") === "true" && detail().querySelector('.mw-store-detail-loading[role="status"]'),
    "First open presents an explicit loading state until complete details arrive");
  assert(!detail().querySelector(".mw-store-detail-preview, .mw-store-detail-metrics, .mw-store-detail-section, .mw-store-detail-child") &&
    !detail().textContent?.includes(summaryOnly) && !detail().textContent?.includes("No description is available") &&
    !detail().textContent?.includes("No server Mods"), "Loading details do not render browse summaries, missing-description fallbacks or incomplete content sections");
  assert(!fixture.textContent?.includes(incompleteCollectionMessage), "Partial presentation during loading does not misreport existing collection metadata as an incomplete collection");
  const loadingBounds = detailBounds(), loadingOffset = loadingCenter();
  assert(loadingOffset.x <= 2 && loadingOffset.y <= 2, "The loading status is centered in the available detail scroll area");
  await releaseParent();
  await settle(() => detail().textContent?.includes(fullDescription) === true && detail().querySelectorAll(".mw-store-detail-child").length === 3,
    "Complete description and member list did not appear together");
  assertCachedBody("The first successful response reveals the complete description and collection members together");
  assert(fixture.textContent?.includes(incompleteCollectionMessage), "A complete response with actual nested or unresolved members still explains its installation restriction");
  const descriptionGaps = descriptionLineGaps();
  assert(descriptionGaps.line > 0 && descriptionGaps.paragraph > descriptionGaps.line * 1.5,
    "The rendered description preserves separate paragraphs and single line breaks");
  const completeBounds = detailBounds();
  assert((Object.keys(loadingBounds) as (keyof typeof loadingBounds)[]).every((key) => Math.abs(loadingBounds[key] - completeBounds[key]) <= 1),
    "Loading and complete detail states retain the same panel size and position");
  assert(detail().querySelectorAll(".mw-store-detail-child").length === 3 && !detail().textContent?.includes("Client overlay"),
    "Mixed collection details omit only explicitly client-only members");
  assert(detail().textContent?.includes("1 client-only Mods skipped"), "Collection explains its skipped client-only member count");
  assert(child(unknownId).tagName === "BUTTON" && !child(unknownId).disabled, "Unresolved members remain keyboard-operable detail entries");
  const serverEntry = child(serverId);
  await act(async () => { serverEntry.scrollIntoView({ block: "nearest" }); serverEntry.focus(); });
  await nativeEnter();
  await titleIs(server.title);
  assert(document.activeElement === detail().querySelector("h2") && element(".mw-store-detail-scroll").scrollTop === 0,
    "Keyboard member navigation focuses its new heading and resets detail scrolling");
  assert(detail().textContent?.includes(server.description), "Member navigation resolves complete details outside the active collection catalog");
  assert(search.value === "Collection" && button("Collections", element('[role="group"][aria-label="Workshop content type"]')).getAttribute("aria-pressed") === "true",
    "Opening a member preserves the current collection classification and search");
  holdParent = true;
  await back(parent.title!);
  await settle(() => heldParents.length === 1, "Returning to the cached parent did not start its background refresh");
  assertCachedBody("Returning to a cached collection keeps its complete body while revalidation is pending");
  await finishCachedRefresh();
  await click(child(unknownId));
  await settle(() => fixture.textContent?.includes("Fixture member detail unavailable") === true, "Unresolved member lookup failure was not shown");
  assert(Boolean(detail().querySelector(".mw-store-detail-back")), "Failed member details retain a path back to their collection");
  assert(detail().querySelector('.mw-store-detail-error[role="alert"]') &&
    !detail().querySelector(".mw-store-detail-loading, .mw-store-detail-preview, .mw-store-detail-metrics, .mw-store-detail-section"),
    "An initial detail failure shows a local error without leaving a partial body");
  assert(!button("Retry", detail()).disabled && !button("Open in Steam", detail()).disabled,
    "A failed detail remains retryable and offers its original Steam page");
  failUnknown = false;
  await click(button("Retry"));
  await titleIs(recovered.title!);
  assert(!detail().querySelector(".mw-store-detail-error") && Boolean(detail().querySelector(".mw-store-detail-description")),
    "Retry replaces the error with a complete detail body");
  assert(unknownAttempts >= 2, "Retry fetches the failed member details again");
  await back(parent.title!);
  await click(child(nestedId)); await titleIs(nested.title!);
  await click(child(deepId)); await titleIs(deep.title!);
  await back(nested.title!); await back(parent.title!);
  assert(!detail().querySelector(".mw-store-detail-back"), "Nested back navigation returns to the original collection root");
  await click(child(serverId)); await titleIs(server.title);
  await click(button("Close details"));
  holdParent = true;
  await openCollection();
  await settle(() => heldParents.length === 1, "Reopening a cached collection did not start its background refresh");
  assertCachedBody("Reopening cached details preserves the full body without returning to a loading placeholder");
  await finishCachedRefresh();
  assert(!detail().querySelector(".mw-store-detail-back"), "Closing details clears prior navigation history");
  await click(button("Close details")); await openCollection(clientCollectionId);
  assert(!detail().querySelector(".mw-store-detail-child") && detail().textContent?.includes("No server Mods are available in this collection."),
    "A collection containing only client-only Mods shows its server empty state");
  await click(button("Close details")); await openCollection();
  const last = child(nestedId); await act(async () => { last.scrollIntoView({ block: "nearest" }); child(serverId).focus({ preventScroll: true }); });
  for (const member of detail().querySelectorAll<HTMLElement>(".mw-store-detail-child")) {
    const bounds = member.getBoundingClientRect(), pane = detail().getBoundingClientRect();
    assert(bounds.left >= pane.left && bounds.right <= pane.right, "Long member names remain inside the details pane");
  }
  await act(async () => { element(".mw-store-detail-section", detail()).scrollIntoView({ block: "start" }); });
  assert(writes === 0 && downloads === 0, "Inspecting collection members never installs or changes instance settings");
  assert(!browseRequests.some((request) => request.kind === "collection" && request.sort === "subscribers"), "Switching classification never sends unsupported collection sorting");
  assert(errors.length === 0, "Workshop detail navigation has no browser errors");
  return { status: "passed", checks, browser_errors: errors, keyboard_member_open: true, unknown_member_retry: true,
    nested_back: true, client_members_filtered: true, all_client_empty: true, supported_sorting: true,
    atomic_detail_loading: true, cached_detail_stable: true, local_error_retry: true,
    loading_offset: loadingOffset, loading_bounds: loadingBounds, complete_bounds: completeBounds, cached_scroll_positions: cachedScrollPositions,
    description_line_gaps: descriptionGaps,
    writes, downloads, viewport: { width: innerWidth, height: innerHeight } };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error("Workshop navigation acceptance exceeded 40 seconds")), 40000); })])
  .catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`, { method: "POST", body: JSON.stringify(report) }));
