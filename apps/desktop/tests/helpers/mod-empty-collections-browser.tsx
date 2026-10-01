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
const ids = ["111111", "222222"], collectionId = "456789";
let details: InstanceDetails;
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
function assert(value: unknown, label: string): asserts value { if (!value) throw new Error(label); checks.push(label); }
async function settle(predicate: () => boolean, label: string) {
  const deadline = performance.now() + 6000;
  while (!predicate()) {
    if (performance.now() > deadline) throw new Error(`${label}: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
function button(label: string) {
  const found = [...fixture.querySelectorAll<HTMLButtonElement>("button")].find((entry) => entry.textContent?.trim() === label || entry.getAttribute("aria-label") === label);
  if (!found) throw new Error(`Missing button ${label}`); return found;
}
async function click(node: HTMLElement) { await act(async () => { node.focus(); node.click(); }); }
function emptyGeometry(node: HTMLElement) {
  const panel = node.closest<HTMLElement>(".dst-mod-config-shell, .mw-detail-panel, .mw-detail-config-panel, .mw-browse-pane")!;
  const content = [...node.children].map((child) => child.getBoundingClientRect()).filter((rect) => rect.width && rect.height);
  if (!content.length) { const range = document.createRange(); range.selectNodeContents(node); content.push(range.getBoundingClientRect()); }
  const bounds = { left: Math.min(...content.map((rect) => rect.left)), right: Math.max(...content.map((rect) => rect.right)),
    top: Math.min(...content.map((rect) => rect.top)), bottom: Math.max(...content.map((rect) => rect.bottom)) };
  const panelBounds = panel.getBoundingClientRect(), style = getComputedStyle(panel);
  const centerX = (panelBounds.left + parseFloat(style.paddingLeft) + panelBounds.right - parseFloat(style.paddingRight)) / 2;
  const pager = panel.querySelector<HTMLElement>(".mw-pager");
  const contentTop = panel.classList.contains("mw-browse-pane") && node.previousElementSibling
    ? node.previousElementSibling.getBoundingClientRect().bottom + parseFloat(style.rowGap || "0") : panelBounds.top + parseFloat(style.paddingTop);
  const contentBottom = pager ? pager.getBoundingClientRect().top - parseFloat(style.rowGap || "0") : panelBounds.bottom - parseFloat(style.paddingBottom);
  const centerY = (contentTop + contentBottom) / 2;
  return { text: node.textContent, panel: panel.className, panel_bounds: panelBounds.toJSON(), content_bounds: bounds,
    delta_x: (bounds.left + bounds.right) / 2 - centerX, delta_y: (bounds.top + bounds.bottom) / 2 - centerY };
}
async function run() {
  const scene = await fetch("/__mod_empty_scene").then((response) => response.text());
  const moduleId = scene === "no-config" ? "unturned" : "dontstarve", moduleDetails = buildMockModuleDetails(moduleId);
  const template = await readInstanceDetails("srv-dst-terminal-error");
  const content = scene.endsWith("-content") || scene === "no-config" || scene === "no-options";
  const collection: SteamWorkshopLookupItem = { ...workshopItem(collectionId, "Survival collection"), item_kind: "collection", child_count: ids.length,
    children: ids.map((id) => ({ id, title: `Member ${id}`, item_kind: "item", status: "resolved", consumer_app_id: 322330 })) };
  const catalog = [workshopItem(ids[0], "Server controls"), workshopItem(ids[1], "World balance"), collection]
    .map((item) => ({ ...item, consumer_app_id: moduleDetails.workshop!.consumer_app_id }));
  const settings = moduleId === "unturned" ? { workshop_file_ids: ids.join("\n") } : content ? {
    enable_caves: true, shared_workshop_mod_ids: ids.join("\n"), master_enabled_workshop_mod_ids: ids.join("\n"),
    caves_enabled_workshop_mod_ids: ids.join("\n"), steam_workshop_collections: [{ id: collectionId, title: collection.title, member_ids: ids }]
  } : {};
  details = { ...template, summary: { ...template.summary, id: "fixture-empty-collections", module_id: moduleId, status: "Stopped", active_process_count: 0 },
    active_run: null, settings_json: JSON.stringify(settings) };
  const draw = () => root.render(<ModWorkbenchBrowserHost details={details} moduleDetails={moduleDetails} epoch={0} onSaved={draw} />);
  mockWindows("mod-empty-collections");
  const baseIpc = createModWorkbenchFixtureIpc(() => ({ details, moduleDetails, catalog: scene === "browse-empty" ? [] : catalog, cached: new Set(ids) }), {
    check: assert, save: (saved) => { details = saved; }
  });
  mockIPC((command, payload) => scene === "no-options" && command === "read_dontstarve_mod_configuration_specs"
    ? ids.map((mod_id) => ({ mod_id, client_only: false, status: "no_options", options: [] })) : baseIpc(command, payload), { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  await act(async () => { draw(); });
  await settle(() => Boolean(fixture.querySelector(".mw-sort-pill")), "Workbench did not mount");
  let modRowReference: { height: number; action_rail_width: number | undefined } | null = null;
  if (scene === "collections-content") {
    await click(button("My Mods"));
    await settle(() => Boolean(fixture.querySelector(".mw-entry-row")), "Reference Mod row did not load");
    const reference = fixture.querySelector<HTMLElement>(".mw-entry-row")!;
    modRowReference = { height: reference.getBoundingClientRect().height,
      action_rail_width: reference.querySelector(".mw-entry-row-actions")?.getBoundingClientRect().width };
  }
  if (scene.startsWith("collections-")) { await click(button("Collections")); await click(button("My collections")); }
  else if (scene !== "browse-empty" && scene !== "search-empty") await click(button("My Mods"));
  if (scene === "search-empty") {
    const input = fixture.querySelector<HTMLInputElement>(".mw-search-input")!;
    await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "No such mod fixture"); input.dispatchEvent(new Event("input", { bubbles: true })); });
  }
  if (content) {
    await settle(() => Boolean(fixture.querySelector(".mw-entry-row")), "Owned rows did not load");
    if (scene === "collections-content") {
      const selection = fixture.querySelector<HTMLButtonElement>(".mw-collection-select")!;
      if (selection.getAttribute("aria-expanded") !== "true") await click(selection);
    }
    if (scene === "mods-content") await settle(() => Boolean(fixture.querySelector(".dst-mod-spec-row")), "Mod settings did not load");
  } else await settle(() => Boolean(fixture.querySelector(".mw-empty")), "Empty state did not load");
  if (scene === "no-config") await settle(() => Boolean(fixture.querySelector(".mw-detail-config-panel .mw-empty")), "No-editor state did not load");
  if (scene === "no-options") await settle(() => Boolean(fixture.querySelector(".dst-mod-config-shell .mw-empty")), "No-options state did not load");
  const empty = [...fixture.querySelectorAll<HTMLElement>(".mw-empty, .mw-entry-detail-empty, .mw-selected-config-empty-note")].filter((node) => node.getBoundingClientRect().height > 0);
  const geometry = empty.map(emptyGeometry);
  const row = fixture.querySelector<HTMLElement>(".mw-entry-row"), rowBounds = row?.getBoundingClientRect();
  const pager = fixture.querySelector<HTMLElement>(".mw-pager"), browsePane = fixture.querySelector<HTMLElement>(".mw-browse-pane");
  const actions = row ? [...row.querySelectorAll<HTMLButtonElement>("button")].map((node) => ({ label: node.getAttribute("aria-label") || node.textContent,
    fits: node.getBoundingClientRect().left >= rowBounds!.left && node.getBoundingClientRect().right <= rowBounds!.right + 1 })) : [];
  assert(errors.length === 0, "Empty and collection fixture has no browser errors");
  return { status: "passed", scene, checks, geometry, actions, mod_row_reference: modRowReference, structure: { headers: [...fixture.querySelectorAll(".mw-detail-col-title")].map((node) => node.textContent),
    has_frame: Boolean(fixture.querySelector(".mw-detail-layout")), has_settings: Boolean(fixture.querySelector(".mw-detail-config-col")),
    row_columns: row ? getComputedStyle(row).gridTemplateColumns : null, row_height: rowBounds?.height,
    action_rail_width: row?.querySelector(".mw-entry-row-actions")?.getBoundingClientRect().width,
    pager_bottom_gap: pager && browsePane ? browsePane.getBoundingClientRect().bottom - parseFloat(getComputedStyle(browsePane).paddingBottom) - pager.getBoundingClientRect().bottom : null,
    visible_members: [...fixture.querySelectorAll<HTMLElement>(".mw-collection-member")].filter((node) => node.getBoundingClientRect().height > 0).length },
    browser_errors: errors, viewport: { width: innerWidth, height: innerHeight } };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error("Empty and collection acceptance exceeded 30 seconds")), 30000); })])
  .catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`, { method: "POST", body: JSON.stringify(report) }));
