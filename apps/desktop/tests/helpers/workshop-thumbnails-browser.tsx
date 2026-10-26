import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { readInstanceDetails } from "../../src/api";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import type { InstanceDetails, ManualModInventoryResult, ModuleDetails, SteamWorkshopLookupItem } from "../../src/types";
import { createModWorkbenchFixtureIpc, ModWorkbenchBrowserHost, workshopItem } from "./mod-workbench-browser-support";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "en-US");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:24px;box-sizing:border-box;display:flex;flex-direction:column";
const root = createRoot(fixture);
const checks: string[] = [], failures: string[] = [], errors: string[] = [];
const measurements: Record<string, unknown>[] = [];
const shapes = ["wide", "tall", "square"];
let details: InstanceDetails, moduleDetails: ModuleDetails, catalog: SteamWorkshopLookupItem[];
let inventory: ManualModInventoryResult | null = null, epoch = 0, game = "";
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(`${game}: ${label}`);
}
function check(condition: unknown, label: string) { checks.push(`${game}: ${label}`); if (!condition) failures.push(`${game}: ${label}`); }
function element<T extends HTMLElement = HTMLElement>(selector: string, within: ParentNode = fixture): T {
  const node = within.querySelector<T>(selector); assert(node, `Missing ${selector}`); return node;
}
async function settle(predicate: () => boolean, label: string) {
  const deadline = performance.now() + 6000;
  while (!predicate()) {
    assert(performance.now() < deadline, `${label}: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function frame() { await act(async () => { await new Promise<void>((resolve) => requestAnimationFrame(() => resolve())); }); }
async function click(node: HTMLElement) { await act(async () => { node.click(); }); await frame(); }
async function render() {
  epoch += 1;
  await act(async () => { root.render(<ModWorkbenchBrowserHost details={details} moduleDetails={moduleDetails}
    epoch={epoch} onSaved={() => { throw new Error("Unexpected settings mutation"); }} />); });
  await settle(() => fixture.querySelectorAll(".mw-mod-card").length === Math.min(30, catalog.length), "Catalog did not load");
  await settle(() => fixture.querySelectorAll(".mw-mod-card .mw-chip--lifecycle-pending-download").length >= 4,
    "Configured Mods did not reach their pending-download state");
  await frame();
}
async function measureThumbnails(scene: string, selector: string, requireImages = true) {
  const boxes = [...fixture.querySelectorAll<HTMLElement>(selector)];
  assert(boxes.length >= 4, `Expected image and placeholder thumbnails in ${scene}`);
  for (const [index, box] of boxes.slice(0, 4).entries()) {
    box.scrollIntoView({ block: "nearest" });
    const image = box.querySelector<HTMLImageElement>("img");
    if (index < 3 && requireImages) {
      assert(image, `${scene}: ${shapes[index]} source is rendered by SteamWorkshopPreview`);
      await settle(() => image.complete && image.naturalWidth > 0, `${scene}: image did not load`);
    }
    await frame();
    const rect = box.getBoundingClientRect(), imageRect = image?.getBoundingClientRect();
    check(Math.abs(rect.width - rect.height) <= 1, `${scene}: ${image ? shapes[index] : "placeholder"} thumbnail frame is square`);
    if (image && imageRect) {
      const expectedRatio = index === 0 ? 2 : index === 1 ? 0.5 : 1;
      check(image.naturalWidth / image.naturalHeight === expectedRatio, `${scene}: original ${shapes[index]} dimensions loaded`);
      check(Math.abs(imageRect.width - rect.width) <= 1 && Math.abs(imageRect.height - rect.height) <= 1,
        `${scene}: image stays within the square frame`);
      check(getComputedStyle(image).objectFit === "cover", `${scene}: image scales without distortion`);
    }
    const card = box.closest<HTMLElement>(".mw-mod-card");
    const action = card ? element(".mw-mod-quick-action", card).getBoundingClientRect() : null;
    if (action) {
      check(action.left >= rect.left && action.top >= rect.top && action.right <= rect.right && action.bottom <= rect.bottom,
        `${scene}: ${image ? shapes[index] : "placeholder"} quick action stays entirely inside its thumbnail`);
      check(Math.abs(rect.right - action.right - 7) <= 1 && Math.abs(rect.bottom - action.bottom - 7) <= 1,
        `${scene}: quick action remains at the thumbnail's bottom-right corner`);
    }
    measurements.push({ game, scene, shape: image ? shapes[index] : "placeholder", width: rect.width, height: rect.height,
      image_width: imageRect?.width, image_height: imageRect?.height,
      natural_width: image?.naturalWidth, natural_height: image?.naturalHeight,
      object_fit: image ? getComputedStyle(image).objectFit : null, card_height: card?.getBoundingClientRect().height,
      quick_action: action ? { width: action.width, height: action.height,
        right_inset: rect.right - action.right, bottom_inset: rect.bottom - action.bottom } : null });
  }
  check(document.documentElement.scrollWidth <= innerWidth + 1, `${scene}: no horizontal viewport overflow`);
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const template = await readInstanceDetails("srv-dst-terminal-error");
  mockWindows("workshop-thumbnails-fixture");
  mockIPC(createModWorkbenchFixtureIpc(() => ({ details, moduleDetails, catalog, inventory, cached: new Set<string>(), pageSize: 30 }), {
    check: assert, save: () => { throw new Error("Unexpected settings write"); }
  }), { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  for (game of ["dontstarve", "palworld"]) {
    moduleDetails = buildMockModuleDetails(game);
    const appId = moduleDetails.workshop?.consumer_app_id;
    assert(appId, "Real module declares Workshop support");
    catalog = Array.from({ length: 31 }, (_, index) => ({ ...workshopItem(String(100000 + index), index % 4 === 2 ? "Square source"
      : `Workshop ${shapes[index % 4] ?? "placeholder"} source with a longer multiplayer server modification title`),
      consumer_app_id: appId, preview_url: index % 4 < 3 ? `/__workshop_preview/${shapes[index % 4]}.svg` : null,
      description_excerpt: index % 4 === 2 ? undefined : "A long server Mod summary exercises content-driven card heights beside the source image.",
      tags: index % 4 === 2 ? [] : ["Multiplayer", "Gameplay", "Server", "Configuration"] }));
    const ownedIds = catalog.slice(0, 4).map((item) => item.id);
    details = { ...template, summary: { ...template.summary, id: `fixture-thumbnails-${game}`, module_id: game,
      status: "Stopped", active_process_count: 0 }, active_run: null,
      settings_json: JSON.stringify({ steam_workshop_collections: [{ id: "999999", title: "Owned fixture", member_ids: ownedIds }],
        ...(game === "palworld" ? { mod_package_names: ownedIds.map((id) => `Package${id}`).join("\n") }
          : { shared_workshop_mod_ids: ownedIds.join("\n"), master_enabled_workshop_mod_ids: ownedIds.join("\n") }) }) };
    inventory = moduleDetails.mods?.manual_staging ? { instance_id: details.summary.id, module_id: game, source_label: "Fixture",
      target_label: "Mods", target_path: "C:\\Fixture\\Mods", target_exists: true,
      items: ownedIds.map((id) => ({ name: id, path: `C:\\Fixture\\Mods\\${id}`, item_type: "directory", file_count: 1,
        total_bytes: 1, inferred_id: `Package${id}` })) } : null;
    await render();
    check(Boolean(fixture.querySelector(".mw-mod-grid--full-page")), "Full-page results exercise stretched grid rows");
    await measureThumbnails("full-page", ".mw-mod-thumb");
    catalog = catalog.slice(0, 4);
    await render();
    await measureThumbnails("short-catalog", ".mw-mod-thumb");
    for (const [index, shape] of shapes.entries()) {
      await click([...fixture.querySelectorAll<HTMLElement>(".mw-mod-card-title")][index]);
      await settle(() => Boolean(fixture.querySelector(".mw-store-detail-preview img")), "Details did not open");
      const image = element<HTMLImageElement>(".mw-store-detail-preview img");
      await settle(() => image.complete && image.naturalWidth > 0, "Detail image did not load");
      check(getComputedStyle(image).objectFit === "contain", `${shape}: detail preview retains uncropped source proportions`);
      if (index === 0) await measureThumbnails("detail-open-catalog", ".mw-mod-thumb");
    }
    check(new Set(measurements.filter((entry) => entry.game === game && entry.card_height)
      .map((entry) => Math.round(Number(entry.card_height)))).size > 1,
      "Full-page and detail layouts exercise different card heights");
    const myMods = [...fixture.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent?.trim() === "My Mods");
    assert(myMods, "My Mods navigation exists"); await click(myMods);
    await settle(() => fixture.querySelectorAll(".mw-entry-row-media").length >= 4, "Owned thumbnails did not load");
    await measureThumbnails("owned-mods", ".mw-entry-row-media", game === "dontstarve");
  }
  await render();
  await click(element(".mw-mod-card-title"));
  await settle(() => Boolean(fixture.querySelector(".mw-store-detail-preview img")), "Final detail did not open");
  check(errors.length === 0, "No console or uncaught errors");
  return { status: "passed", checks, failures, measurements, browser_errors: errors, viewport: { width: innerWidth, height: innerHeight } };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error("Workshop thumbnail acceptance exceeded 40 seconds")), 40000);
})]).catch((error) => ({ status: "failed", error: String(error), checks, failures, measurements, browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`,
    { method: "POST", body: JSON.stringify(report) }));
