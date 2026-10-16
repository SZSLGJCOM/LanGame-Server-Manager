import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { readInstanceDetails } from "../../src/api";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { I18nProvider } from "../../src/i18n";
import { ModWorkbench } from "../../src/views/servers/ModWorkbench";
import { ServerDetailTabs } from "../../src/views/servers/ServerDetailTabs";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceDetails, SteamWorkshopLookupItem } from "../../src/types";
import { createModWorkbenchFixtureIpc, workshopItem } from "./mod-workbench-browser-support";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";
import "../../src/views/servers/server-archives.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "en-US");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:24px;box-sizing:border-box;display:grid;grid-template-rows:minmax(0,1fr) auto";
const root = createRoot(fixture);
const checks: string[] = [], failures: string[] = [], errors: string[] = [];
const measurements: Record<string, unknown>[] = [];
const moduleDetails = buildMockModuleDetails("dontstarve");
const fullCatalog = Array.from({ length: 31 }, (_, index): SteamWorkshopLookupItem => ({
  ...workshopItem(String(100000 + index), `Workshop server Mod ${index + 1}`),
  description: "A complete description for the isolated layout fixture.",
  description_excerpt: "Server settings and multiplayer gameplay adjustments."
}));
let details: InstanceDetails, catalog = fullCatalog, epoch = 0;
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}
function check(condition: unknown, label: string) { checks.push(label); if (!condition) failures.push(label); }
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const found = fixture.querySelector<T>(selector);
  assert(found, `Missing ${selector}`);
  return found;
}
async function settle(predicate: () => boolean, label: string) {
  const deadline = performance.now() + 6000;
  while (!predicate()) {
    assert(performance.now() < deadline, `${label}: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function layoutFrame() {
  await act(async () => { await new Promise<void>((resolve) => requestAnimationFrame(() => resolve())); });
}
function Harness() {
  const [noticeTarget, setNoticeTarget] = useState<HTMLDivElement | null>(null);
  return <ActivityNoticeTarget.Provider value={{ element: noticeTarget, dismissLabel: "Close" }}>
    <div className="workshop-layout-frame" style={{ height: "100%", minHeight: 0 }}>
      <div className="page-grid workspace-page servers-page servers-page--no-metrics">
        <section className="workspace-section-grid server-layout-grid">
          <aside className="server-list-panel" aria-label="Fixture instances"><div className="panel-head">Workshop fixture instance</div></aside>
          <section className="server-detail-panel"><div className="detail-stack detail-stack--server">
            <div className="server-detail-subheader"><ServerDetailTabs
              tabs={[{ id: "settings", label: "Settings", icon: "settings" }, { id: "mods", label: "Mods", icon: "package" }]}
              activeTab="mods" panelId="workshop-layout-panel" label="Instance detail sections" onSelect={() => {}} /></div>
            <div id="workshop-layout-panel" className="server-detail-scroll server-detail-scroll--mods" role="tabpanel">
              <ModWorkbench key={epoch} details={details} moduleDetails={moduleDetails} launchPlan={null}
                onSaveSettings={async () => { throw new Error("Unexpected write in Workshop layout fixture"); }} />
            </div>
          </div></section>
        </section>
      </div>
    </div>
    <footer className="shell-activity-bar"><span className="shell-activity-label">Activity</span>
      <div className="shell-activity-notices" ref={setNoticeTarget} /></footer>
  </ActivityNoticeTarget.Provider>;
}
async function render(items: SteamWorkshopLookupItem[]) {
  catalog = items;
  epoch += 1;
  await act(async () => { root.render(<I18nProvider><InstanceSettingsSaveProvider><Harness /></InstanceSettingsSaveProvider></I18nProvider>); });
  await settle(() => Boolean(fixture.querySelector(".mw-browse-status")) &&
    fixture.querySelectorAll(".mw-mod-card").length === Math.min(30, items.length), "Workshop catalog did not settle");
  await layoutFrame();
}
function measure(scene: string, fullPage = false) {
  const selectors = [".server-detail-scroll--mods", ".mw-workbench", ".mw-workbench-main", ".mw-workspace-section",
    ".mw-workspace-panel", ".mw-steam-workspace", ".mw-store-layout", ".mw-browse-pane"];
  const nodes = selectors.map((selector) => element(selector));
  const bounds = nodes.map((node) => node.getBoundingClientRect());
  const [outer, workbench, main, , , workspace, store, pane] = nodes;
  const paneBounds = pane.getBoundingClientRect(), paneStyle = getComputedStyle(pane);
  const pager = element(".mw-pager"), pagerBounds = pager.getBoundingClientRect();
  const cards = [...fixture.querySelectorAll<HTMLElement>(".mw-mod-card")];
  const grid = fixture.querySelector<HTMLElement>(".mw-mod-grid");
  const gridBounds = grid?.getBoundingClientRect();
  const rows = grid ? getComputedStyle(grid).gridTemplateRows.split(/\s+/).map(Number.parseFloat) : [];
  const paddingBottom = parseFloat(paneStyle.paddingBottom), gap = parseFloat(paneStyle.rowGap) || 0;
  const fits = pane.scrollHeight <= pane.clientHeight + 1;
  const cardBounds = cards.map((card) => card.getBoundingClientRect());
  const label = `${scene}`;
  check(Math.abs(bounds[0].bottom - bounds[1].bottom) <= 2, `${label}: workbench fills its parent`);
  check(Math.abs(bounds[1].bottom - bounds[2].bottom - 1) <= 2, `${label}: main fills the workbench frame`);
  check(Math.abs(bounds[3].bottom - (bounds[2].bottom - parseFloat(getComputedStyle(main).paddingBottom))) <= 2,
    `${label}: workspace section fills the main content area`);
  for (const index of [4, 5]) check(Math.abs(bounds[index].bottom - bounds[index - 1].bottom) <= 2,
    `${label}: ${selectors[index]} fills its parent`);
  check(Math.abs(store.getBoundingClientRect().top - element(".mw-workspace-toolbar").getBoundingClientRect().bottom) <= 2 &&
    Math.abs(store.getBoundingClientRect().bottom - workspace.getBoundingClientRect().bottom) <= 2,
    `${label}: store fills the area below the toolbar`);
  check(Math.abs(paneBounds.top - store.getBoundingClientRect().top) <= 2 &&
    Math.abs(paneBounds.bottom - store.getBoundingClientRect().bottom) <= 2, `${label}: browser fills the store height`);
  if (fits) check(Math.abs(paneBounds.bottom - paddingBottom - pagerBounds.bottom) <= 2,
    `${label}: pagination sits at the bottom of the browse area`);
  if (grid && gridBounds && fullPage) {
    check(cardBounds.every((rect) => rows.some((height) => Math.abs(rect.height - height) <= 2)),
      `${label}: every full-page card fills its grid row`);
    if (fits) check(Math.abs(pagerBounds.top - gap - gridBounds.bottom) <= 2,
      `${label}: full-page grid uses the space above pagination`);
  } else if (cards.length) {
    check(cardBounds.every((rect) => rect.height <= 220), `${label}: short results retain compact card heights`);
  }
  check(outer.scrollHeight <= outer.clientHeight + 1 && workbench.scrollHeight <= workbench.clientHeight + 1,
    `${label}: scrolling remains inside the browse pane`);
  check(document.documentElement.scrollWidth <= innerWidth + 1 && document.documentElement.scrollHeight <= innerHeight + 1,
    `${label}: workspace stays within the viewport`);
  const detail = fixture.querySelector<HTMLElement>(".mw-store-detail");
  if (detail) check(Math.abs(detail.getBoundingClientRect().bottom - store.getBoundingClientRect().bottom) <= 2,
    `${label}: details fill the store height`);
  const measured = { scene, full_page: fullPage, fits, layers: Object.fromEntries(selectors.map((selector, index) =>
    [selector, { top: bounds[index].top, bottom: bounds[index].bottom, height: bounds[index].height }])),
    pager_bottom_gap: paneBounds.bottom - paddingBottom - pagerBounds.bottom,
    grid_pager_gap: gridBounds ? pagerBounds.top - gap - gridBounds.bottom : null,
    pane_height: paneBounds.height, grid_height: gridBounds?.height ?? 0, card_height: cardBounds[0]?.height ?? 0,
    row_heights: rows, card_count: cards.length, scroll_height: pane.scrollHeight, client_height: pane.clientHeight };
  measurements.push(measured);
  return measured;
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const template = await readInstanceDetails("srv-dst-terminal-error");
  details = { ...template, summary: { ...template.summary, id: "fixture-workshop-layout", module_id: "dontstarve",
    status: "Stopped", active_process_count: 0 }, active_run: null, settings_json: "{}" };
  mockWindows("workshop-layout-fixture");
  mockIPC(createModWorkbenchFixtureIpc(() => ({ details, moduleDetails, catalog, cached: new Set<string>(), pageSize: 30 }), {
    check: assert, save: () => { throw new Error("Unexpected save in layout fixture"); }
  }), { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  await render(fullCatalog);
  element(".workshop-layout-frame").style.height = `${Math.round(element(".workshop-layout-frame").getBoundingClientRect().height * 0.62)}px`;
  await layoutFrame();
  const compact = measure("full-page-compact", true);
  const ownedCard = element(".mw-mod-card");
  element(".workshop-layout-frame").style.height = "100%";
  await layoutFrame();
  const expanded = measure("full-page-expanded", true);
  check(ownedCard === element(".mw-mod-card"), "Resize preserves the mounted catalog");
  check(expanded.pane_height > compact.pane_height + 100, "Browse pane gains height when the same window expands");
  if (expanded.fits) {
    check(expanded.grid_height > compact.grid_height + 20, "Full-page grid grows when expanded height can fit its contents");
    check(expanded.card_height > compact.card_height + 2, "Full-page cards grow with the available row height");
  }
  await act(async () => { element<HTMLButtonElement>(".mw-mod-card-title").click(); });
  await settle(() => Boolean(fixture.querySelector(".mw-store-detail")), "Workshop details did not open");
  await layoutFrame();
  measure("full-page-details", true);
  await render(fullCatalog.slice(0, 2));
  measure("short-catalog");
  await render([]);
  measure("empty-catalog");
  check(Boolean(fixture.querySelector(".mw-empty")), "Empty catalog shows its real empty state");
  await render(fullCatalog);
  measure("final-full-page", true);
  check(errors.length === 0, "Browser console and uncaught errors remain empty");
  // Keep geometry failures separate so the runner captures a baseline screenshot.
  return { status: "passed", checks, failures, measurements, browser_errors: errors,
    viewport: { width: innerWidth, height: innerHeight } };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error("Workshop layout acceptance exceeded 40 seconds")), 40000);
})]).catch((error) => ({ status: "failed", error: String(error), checks, failures, measurements, browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`,
    { method: "POST", body: JSON.stringify(report) }));
