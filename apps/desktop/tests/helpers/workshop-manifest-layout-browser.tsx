import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { readInstanceDetails } from "../../src/api";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import type { InstanceDetails, SteamWorkshopLookupItem } from "../../src/types";
import { createModWorkbenchFixtureIpc, ModWorkbenchBrowserHost, workshopItem } from "./mod-workbench-browser-support";

// Manifest-mode acceptance on the real Workbench: only native IPC is substituted.
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const zh = innerWidth < 1000;
localStorage.setItem("langame.locale", zh ? "zh-CN" : "en-US");
document.documentElement.dataset.theme = "dark";
const finalView = document.querySelector('meta[name="manifest-fixture-final"]')?.getAttribute("content") ?? "review";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:16px;box-sizing:border-box;display:flex;flex-direction:column";
const root = createRoot(fixture);
const moduleDetails = buildMockModuleDetails("dontstarve");
const checks: string[] = [], errors: string[] = [], downloads: string[][] = [];
const writes: Record<string, unknown>[] = [];
const READY = ["1111111", "2222222"], MISSING = ["3333333", "4444444", "5555555"], FOREIGN = "6666666", APPLY = "7777777", OFFLINE = "8888888";
const titles: Record<string, string> = {
  "1111111": "Global Positions", "2222222": "Geometric Placement", "3333333": "Health Info", "4444444": "Wormhole Marks",
  "5555555": "Combined Status", [FOREIGN]: "Another game's Mod", [APPLY]: "Extra Equip Slots"
};
const catalog: SteamWorkshopLookupItem[] = Object.entries(titles).map(([id, title]) =>
  id === FOREIGN ? { ...workshopItem(id, title), consumer_app_id: 4000 } : workshopItem(id, title));
const cached = new Set<string>(READY);
let stored: InstanceDetails, details: InstanceDetails, epoch = 0;
let holdLookup = false, releaseLookup: (() => void) | undefined;
let holdDownload = false, releaseDownload: (() => void) | undefined;
const label = zh
  ? { mode: "清单模式", check: "核对清单", enable: "补齐并启用", missing: (count: number) => `补齐下载（${count}）` }
  : { mode: "Manifest mode", check: "Check list", enable: "Download missing and enable", missing: (count: number) => `Download missing (${count})` };
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
function assert(condition: unknown, name: string): asserts condition {
  if (!condition) throw new Error(`${name}: ${fixture.textContent}`);
  checks.push(name);
}
function maybeButton(text: string): HTMLButtonElement | undefined {
  return [...fixture.querySelectorAll<HTMLButtonElement>("button")].find((entry) => entry.textContent?.trim() === text);
}
function button(text: string): HTMLButtonElement {
  const found = maybeButton(text);
  if (!found) throw new Error(`Missing button ${text}: ${fixture.textContent}`);
  return found;
}
function panel(): HTMLElement {
  const found = fixture.querySelector<HTMLElement>(".mw-manifest");
  if (!found) throw new Error("Manifest panel unavailable");
  return found;
}
function input(): HTMLTextAreaElement { return panel().querySelector<HTMLTextAreaElement>(".mw-manifest-input")!; }
async function settle(predicate: () => boolean, name: string) {
  const deadline = performance.now() + 8000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(`${name}: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(target: HTMLElement) {
  target.scrollIntoView({ block: "nearest" });
  const bounds = target.getBoundingClientRect();
  assert(bounds.width > 0 && bounds.left >= -1 && bounds.right <= innerWidth + 1, "Action stays inside the viewport");
  assert(!(target instanceof HTMLButtonElement) || !target.disabled, "Action is enabled");
  await act(async () => { target.focus(); target.click(); });
}
async function type(value: string) {
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(input(), value);
    input().dispatchEvent(new Event("input", { bubbles: true }));
  });
}
function settings(): Record<string, unknown> { return JSON.parse(stored.settings_json); }
function ids(field: string): string[] { return String(settings()[field] ?? "").split(/\D+/).filter(Boolean); }
function draw() { root.render(<ModWorkbenchBrowserHost details={details} moduleDetails={moduleDetails} epoch={epoch} onSaved={draw} />); }
async function mount(running = false) {
  details = structuredClone(stored);
  if (running) details = { ...details, summary: { ...details.summary, status: "Running", active_process_count: 1 } };
  epoch += 1;
  await act(async () => { draw(); await new Promise((resolve) => setTimeout(resolve, 50)); });
  await settle(() => Boolean(maybeButton(label.mode)), "Manifest mode tab did not mount");
  await click(button(label.mode));
  await settle(() => Boolean(fixture.querySelector(".mw-manifest-input")), "Manifest editor did not open");
}
async function review(text: string) {
  await type(text);
  await click(button(label.check));
  await settle(() => panel().getAttribute("aria-busy") === "false" && Boolean(panel().querySelector(".mw-manifest-table")), "Manifest review did not finish");
}
function rect(element: Element | null) {
  if (!element) return null;
  const bounds = element.getBoundingClientRect();
  return { left: Math.round(bounds.left), top: Math.round(bounds.top), width: Math.round(bounds.width), height: Math.round(bounds.height),
    right: Math.round(bounds.right), bottom: Math.round(bounds.bottom) };
}
function measure() {
  const root = panel();
  const panes = [...root.querySelectorAll<HTMLElement>(":scope > .mw-manifest-pane")];
  const heads = panes.map((pane) => pane.querySelector(":scope > .mw-manifest-pane-head"));
  const actions = root.querySelector<HTMLElement>(".mw-manifest-actions");
  const rows = [...root.querySelectorAll<HTMLElement>(".mw-manifest-table tbody tr")].map((row) => Math.round(row.getBoundingClientRect().height));
  const [inputPane, reviewPane] = panes.map((pane) => pane.getBoundingClientRect());
  const framed = [root, ...panes].filter((node) => {
    const style = getComputedStyle(node);
    return style.borderRadius !== "0px" || style.boxShadow !== "none" || style.borderLeftWidth !== "0px" && style.borderRightWidth !== "0px";
  }).length;
  return {
    panel: rect(root), input: rect(input()), panes: panes.map(rect), heads: heads.map(rect), actions: rect(actions),
    split: Boolean(inputPane && reviewPane && reviewPane.left >= inputPane.right - 1 && Math.abs(reviewPane.top - inputPane.top) <= 1),
    stacked: Boolean(inputPane && reviewPane && reviewPane.top >= inputPane.bottom - 1),
    framed_panes: framed,
    row_heights: rows,
    visible_help_paragraphs: [...root.querySelectorAll<HTMLElement>("p.mw-manifest-help")].filter((entry) => entry.checkVisibility()).length,
    nested_cards: root.querySelectorAll(".mw-manifest .mw-manifest, [class*='card'] [class*='card']").length,
    document_overflow_x: document.documentElement.scrollWidth - innerWidth,
    panel_overflow_x: root.scrollWidth - root.clientWidth
  };
}
async function verifyLayout(view: "initial" | "review") {
  const layout = measure();
  const wide = innerWidth >= 1200;
  assert(wide ? layout.split : layout.stacked, wide ? "Desktop manifest splits input and review panes" : "Narrow manifest stacks its panes");
  assert(layout.panes.length === 2 && layout.framed_panes === 0 && layout.nested_cards === 0, "Manifest panes are dividers, not nested cards");
  assert(layout.visible_help_paragraphs === 0, "Long help is not rendered as paragraphs");
  if (wide) assert(layout.heads[0]?.height === layout.heads[1]?.height && layout.heads[0]?.top === layout.heads[1]?.top, "Pane headings share one baseline row");
  if (view === "review") {
    const actions = panel().querySelector<HTMLElement>(".mw-manifest-actions")!;
    const bounds = actions.getBoundingClientRect();
    assert(bounds.top >= 0 && bounds.bottom <= innerHeight + 1, "Review actions are visible without scrolling");
    const primary = button(label.enable).getBoundingClientRect();
    const hit = document.elementFromPoint(primary.left + primary.width / 2, primary.top + primary.height / 2);
    assert(hit?.closest("button") === button(label.enable), "Primary action is not covered by sticky content");
    assert(layout.row_heights.length > 0 && layout.row_heights.every((height) => height <= 44), `Review rows keep a compact desktop density ${JSON.stringify(layout.row_heights)}`);
  }
  return layout;
}
async function verifyHelp() {
  const trigger = panel().querySelector<HTMLButtonElement>(".mw-manifest-help-trigger");
  assert(trigger && trigger.getAttribute("aria-label"), "Manifest help has a labelled focusable trigger");
  const describedBy = trigger.getAttribute("aria-describedby");
  const description = describedBy ? document.getElementById(describedBy) : null;
  assert(description?.textContent?.includes("modoverrides.lua") && input().getAttribute("aria-describedby") === describedBy,
    "Help text and target note stay in the accessibility tree for the trigger and the list");
  assert(description.getBoundingClientRect().width <= 1, "Accessible help copy is visually hidden");
  await act(async () => { trigger.focus(); });
  await settle(() => Boolean(document.querySelector(".configuration-field-help-tooltip.is-visible")), "Keyboard focus did not reveal manifest help");
  const bubble = document.querySelector<HTMLElement>(".configuration-field-help-tooltip.is-visible")!;
  const bounds = bubble.getBoundingClientRect();
  assert(bubble.textContent?.includes("modoverrides.lua") && bounds.left >= 0 && bounds.right <= innerWidth && bounds.bottom <= innerHeight,
    "Help bubble shows the moved copy inside the viewport");
  await act(async () => { window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); });
  await settle(() => !document.querySelector(".configuration-field-help-tooltip"), "Escape did not dismiss manifest help");
  await act(async () => { trigger.blur(); });
}

async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const template = await readInstanceDetails("srv-dst-terminal-error");
  stored = { ...template, summary: { ...template.summary, id: "fixture-manifest-layout", module_id: "dontstarve", status: "Stopped", active_process_count: 0 },
    active_run: null, settings_json: JSON.stringify({ enable_caves: true, shared_workshop_mod_ids: "", shared_workshop_collection_ids: "",
      master_enabled_workshop_mod_ids: "", caves_enabled_workshop_mod_ids: "", steam_workshop_collections: [] }) };
  details = structuredClone(stored);
  const baseIpc = createModWorkbenchFixtureIpc(() => ({ details: stored, moduleDetails, catalog, cached }), {
    check: assert, save: (saved) => { stored = structuredClone(saved); details = saved; writes.push(JSON.parse(saved.settings_json)); },
    download: (itemIds) => downloads.push(itemIds)
  });
  mockWindows("manifest-layout-fixture");
  mockIPC(async (command, payload) => {
    if (command === "lookup_steam_workshop_items") {
      if (((payload as { ids?: string[] } | undefined)?.ids ?? []).includes(OFFLINE)) throw new Error("Fixture Workshop lookup is offline");
      if (holdLookup) { holdLookup = false; await new Promise<void>((resolve) => { releaseLookup = resolve; }); }
    }
    if (command === "download_steam_workshop_items" && holdDownload) {
      holdDownload = false; await new Promise<void>((resolve) => { releaseDownload = resolve; });
    }
    return baseIpc(command, payload);
  }, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });

  await mount();
  assert(input().value === "" && button(label.check).disabled, "Empty manifest cannot be checked");
  assert(panel().querySelector(".mw-manifest-empty")?.checkVisibility() && !panel().querySelector(".mw-manifest-summary, .mw-manifest-actions"),
    "Review pane shows an empty state before the first check");
  await verifyHelp();
  const initial = await verifyLayout("initial");
  if (finalView === "initial") return finish(initial);

  await type("not-a-workshop-id");
  await settle(() => fixture.textContent?.includes("not-a-workshop-id") === true && button(label.check).disabled, "Invalid token was not reported");
  assert(panel().querySelector('[role="alert"]'), "Invalid tokens are announced");

  await type(OFFLINE);
  await click(button(label.check));
  await settle(() => panel().getAttribute("aria-busy") === "false" && document.body.textContent?.includes("Fixture Workshop lookup is offline") === true,
    "Lookup failure was not surfaced");
  assert(!panel().querySelector(".mw-manifest-table") && !button(label.check).disabled, "Failed review keeps the list editable and retryable");
  await type(READY.join("\n"));

  holdLookup = true;
  await click(button(label.check));
  await settle(() => panel().getAttribute("aria-busy") === "true" && Boolean(releaseLookup), "Checking state did not render");
  assert(maybeButton(label.check)?.disabled !== false, "Check action is busy while the review runs");
  assert(panel().querySelector('.mw-manifest-empty[role="status"]')?.textContent?.includes(zh ? "正在核对" : "Checking"), "Review pane announces the running check");
  await act(async () => { releaseLookup!(); });
  await settle(() => Boolean(panel().querySelector(".mw-manifest-table")), "Held review did not finish");

  await review([...READY, FOREIGN].join("\n"));
  assert(panel().textContent?.includes(zh ? "属于其他游戏" : "For another game"), "Foreign Mod issue is listed");
  assert(maybeButton(label.enable)?.disabled === true, "Issues block enabling");

  await review(APPLY);
  holdDownload = true;
  const beforeWrites = writes.length;
  await click(button(label.enable));
  await settle(() => Boolean(releaseDownload) && panel().getAttribute("aria-busy") === "true", "Processing state did not render");
  assert(button(label.enable).disabled && input().disabled, "Processing locks the manifest");
  await act(async () => { releaseDownload!(); });
  await settle(() => writes.length === beforeWrites + 1 && panel().getAttribute("aria-busy") === "false", "Download and enable did not persist");
  assert(ids("master_enabled_workshop_mod_ids").includes(APPLY) && ids("caves_enabled_workshop_mod_ids").includes(APPLY) && cached.has(APPLY),
    "Download and enable writes both shard enablement lists");

  stored = { ...stored, settings_json: JSON.stringify({ ...settings(), master_enabled_workshop_mod_ids: "", caves_enabled_workshop_mod_ids: "" }) };
  await mount(true);
  await review(MISSING[0]);
  assert(button(label.enable).disabled && button(label.missing(1)).disabled, "Running instance disables apply actions");

  stored = { ...stored, settings_json: JSON.stringify({ ...settings(), shared_workshop_mod_ids: "" }) };
  await mount();
  await review([...READY, ...MISSING].join("\n"));
  assert(!button(label.missing(MISSING.length)).disabled && !button(label.enable).disabled, "Clean review is actionable");
  assert(panel().querySelectorAll(".mw-manifest-table tbody tr").length === READY.length + MISSING.length, "Every Mod has one review row");
  await act(async () => { (document.activeElement as HTMLElement | null)?.blur(); });
  for (const scroller of fixture.querySelectorAll<HTMLElement>(".mw-detail-panel, .mw-manifest-pane-body")) scroller.scrollTop = 0;
  return finish(await verifyLayout("review"));
}
async function finish(layout: ReturnType<typeof measure>) {
  assert(layout.document_overflow_x <= 1 && layout.panel_overflow_x <= 1, "Manifest has no horizontal overflow");
  const nonce = new URLSearchParams(location.search).get("nonce");
  await act(async () => {
    await fetch(`/__reliability_pointer/${nonce}`, { method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ type: "move", x: innerWidth - 2, y: innerHeight - 2 }) }).catch(() => undefined);
    await new Promise((resolve) => setTimeout(resolve, 120));
    await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
  });
  assert(errors.length === 0, "No browser console or uncaught errors");
  return { status: "passed", checks, layout, final_view: finalView, writes: writes.length, downloads, browser_errors: errors,
    locale: zh ? "zh-CN" : "en-US", viewport: { width: innerWidth, height: innerHeight } };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error("Manifest layout acceptance exceeded 40 seconds")), 40000); })])
  .catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`, { method: "POST", body: JSON.stringify(report) }));
