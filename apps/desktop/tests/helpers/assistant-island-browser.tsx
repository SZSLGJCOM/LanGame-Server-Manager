import React, { act, StrictMode, useState } from "react";
import { createRoot } from "react-dom/client";
import { AssistantCapsule } from "../../src/components/AssistantCapsule";
import { createDefaultAiSettings } from "../../src/ai-settings";
import { fallbackBootstrap } from "../../src/app-state";
import { I18nProvider } from "../../src/i18n";
import type { AssistantBuildInput } from "../../src/assistant-types";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const nonce = new URLSearchParams(location.search).get("nonce");
const cases: string[] = [];
const settings = { ...createDefaultAiSettings(), enabled: true, model: "browser-fixture", apiKeyStored: true };
const context: AssistantBuildInput = {
  aiSettings: settings, locale: "zh-CN", activeJobsCount: 0, activeView: "system",
  bootstrap: fallbackBootstrap, storageReady: true, libraryPage: "catalog", overlayNames: [],
  runtimeAutoRefreshPaused: false, runtimeRefreshIssue: null, selectedInstanceDetails: null,
  selectedInstanceId: null, selectedModuleId: null, selectedInstanceModuleDetails: null,
  selectedLaunchPlan: null, selectedLaunchPlanError: null, selectedLogDocument: null,
  selectedModuleDetails: null, selectedRuntime: null, serverWorkspaceSection: "overview", steamCmdStatus: null,
};

// Control only the preference API boundary. Geometry, frames, DOM, focus and CSS
// remain real browser behavior; this does not emulate the browser's CSS media query.
const nativeMatchMedia = window.matchMedia.bind(window);
const reducedQuery = nativeMatchMedia("(prefers-reduced-motion: reduce)");
let reducedMotion = false;
Object.defineProperty(reducedQuery, "matches", { configurable: true, get: () => reducedMotion });
window.matchMedia = (query) => query.includes("prefers-reduced-motion") ? reducedQuery : nativeMatchMedia(query);
let setConfirmationOpen: (value: boolean) => void = () => { throw new Error("Harness has not mounted"); };
const noop = () => {};

function Harness() {
  const [draft, setDraft] = useState("");
  const [confirmationOpen, setConfirmation] = useState(false);
  setConfirmationOpen = setConfirmation;
  return <>
    <header className="shell-header island-fixture-header">
      <div className="island-fixture-title">
        <span>LanGame Server Manager</span>
        <AssistantCapsule aiSettings={settings}
          assistant={{ panelTitle: "LAN", tone: "info" }} assistantDraft={draft} assistantInput={context}
          execution={{ status: "idle", promptLabel: null, result: null, error: null }} confirmationOpen={confirmationOpen}
          messages={[]} conversations={[]} activeConversationId={null} onAction={noop}
          onClearAiSecret={async (next) => next} onDeleteConversation={noop} onDraftChange={setDraft}
          onNewConversation={() => setDraft("")} onSelectConversation={noop} onRunPrompt={noop}
          onSaveAiSettings={async (next) => next} onSendMessage={noop} />
      </div>
      <nav className="island-fixture-nav" aria-label="应用导航"><button id="outside-control" type="button">服务器</button></nav>
    </header>
    <main className="island-fixture-body">服务器运行状态</main>
  </>;
}

function check(condition: unknown, message: string): asserts condition { if (!condition) throw new Error(message); }
const fixture = document.getElementById("fixture");
check(fixture, "Fixture root is missing");
let root = createRoot(fixture);
Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
  await act(async () => { root.unmount(); });
  window.matchMedia = nativeMatchMedia;
  return { browser_errors: errors, native_dialogs: 0 };
} });
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const result = document.querySelector<T>(selector);
  check(result, `Missing ${selector}`);
  return result;
}
const trigger = () => element<HTMLButtonElement>(".assistant-capsule");
const surface = () => document.querySelector<HTMLDivElement>(".assistant-island-surface");
const input = () => element<HTMLTextAreaElement>(".assistant-chat-input");
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
function verifyFrameGeometry() {
  const current = surface();
  if (!current) return;
  check(current.scrollLeft === 0 && current.scrollTop === 0,
    `Island shell scrolled during ${current.dataset.state}: ${current.scrollLeft}, ${current.scrollTop}`);
  const panel = current.querySelector<HTMLElement>(".assistant-panel");
  const content = current.querySelector<HTMLElement>(".assistant-island-content");
  if (panel && content) {
    const shellTop = current.getBoundingClientRect().top;
    const expectedOffset = (parseFloat(getComputedStyle(content).top) || 0)
      + (parseFloat(getComputedStyle(current).borderTopWidth) || 0);
    const actualOffset = panel.getBoundingClientRect().top - shellTop;
    check(Math.abs(actualOffset - expectedOffset) <= 9,
      `Panel moved away from its shell during ${current.dataset.state}: offset ${actualOffset}, expected ${expectedOffset} ± 9`);
  }
}
async function nextFrame() { await act(frame); verifyFrameGeometry(); }
async function settleUntil(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    check(performance.now() < deadline, `${message}; browser errors: ${errors.join("; ")}`);
    await nextFrame();
  }
}
async function render() {
  await act(async () => { root.render(<StrictMode><I18nProvider><Harness /></I18nProvider></StrictMode>); });
  await settleUntil(() => Boolean(document.querySelector(".assistant-capsule")), "LAN entry did not mount");
}
async function click(target: HTMLElement) { await act(async () => { target.click(); }); }
// Hit-test coordinates before dispatching every event. This catches overlays that
// swallow a second click; these synthetic events do not replace native mouse QA.
async function coordinateClick(x: number, y: number) {
  await act(async () => {
    const hit = document.elementFromPoint(x, y);
    check(hit, `No pointer target at ${x}, ${y}`);
    const common = { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0 };
    hit.dispatchEvent(new PointerEvent("pointerdown", { ...common, pointerId: 1, pointerType: "mouse", isPrimary: true, buttons: 1 }));
    hit.dispatchEvent(new MouseEvent("mousedown", { ...common, buttons: 1 }));
    hit.dispatchEvent(new PointerEvent("pointerup", { ...common, pointerId: 1, pointerType: "mouse", isPrimary: true, buttons: 0 }));
    hit.dispatchEvent(new MouseEvent("mouseup", { ...common, buttons: 0 }));
    hit.dispatchEvent(new MouseEvent("click", { ...common, detail: 1 }));
  });
}
const expandedToggle = () => element<HTMLButtonElement>(".assistant-island-toggle");
async function key(name: "Tab" | "Escape") {
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/${name}`, { method: "POST" });
    check(response.ok, `Native ${name} dispatch failed`);
  });
}
async function open() {
  await click(trigger());
  await settleUntil(() => surface()?.dataset.state === "open" && Boolean(document.querySelector(".assistant-chat-input")), "LAN did not finish opening");
}
async function close() {
  await click(element<HTMLButtonElement>(".assistant-close-button"));
  await settleUntil(() => !surface(), "Closing left the island surface mounted");
}
function expectedBounds() {
  const capsule = trigger().getBoundingClientRect();
  const width = Math.min(480, innerWidth - 40);
  const top = capsule.top;
  const left = Math.min(Math.max(20, capsule.left + capsule.width / 2 - width / 2), innerWidth - width - 20);
  return { width, height: Math.min(560, innerHeight - top - 20), top, left };
}
function verifyBounds() {
  verifyFrameGeometry();
  const expected = expectedBounds();
  const actual = surface()!.getBoundingClientRect();
  for (const name of ["width", "height", "top", "left"] as const) {
    check(Math.abs(actual[name] - expected[name]) <= 2, `Island ${name} ${actual[name]} differs from ${expected[name]}`);
  }
  check(actual.bottom <= innerHeight - 18, "Island extends below the visible viewport");
  const navigation = element("#outside-control").getBoundingClientRect();
  check(document.elementFromPoint(navigation.x + navigation.width / 2, navigation.y + navigation.height / 2) === element("#outside-control"),
    "Expanded island blocks the outside navigation control");
}
async function setReducedMotion(value: boolean) {
  await act(async () => {
    reducedMotion = value;
    reducedQuery.dispatchEvent(new MediaQueryListEvent("change", { matches: value, media: reducedQuery.media }));
  });
  await nextFrame();
}

async function run() {
  await render();
  const origin = trigger().getBoundingClientRect();
  await click(trigger());
  const openingSurface = surface();
  check(openingSurface, "First click must immediately mount a surface while the panel module loads");
  check(trigger().getAttribute("aria-expanded") === "true", "First click did not expose expanded state");
  check(element(".assistant-island-content").inert, "Opening content is interactive before it is visible");
  const sampledWidths: number[] = [];
  let sampledContent = false;
  const deadline = performance.now() + 5000;
  while (surface()?.dataset.state !== "open") {
    check(performance.now() < deadline, "Opening spring did not settle");
    const rect = surface()!.getBoundingClientRect();
    sampledWidths.push(rect.width);
    const panel = document.querySelector<HTMLElement>(".assistant-panel");
    if (panel && rect.width > origin.width + 2 && rect.width < expectedBounds().width - 2) {
      check(Math.abs(panel.getBoundingClientRect().width - expectedBounds().width) <= 2,
        "Opening compressed the panel text instead of revealing fixed-size content");
      sampledContent = true;
    }
    await nextFrame();
  }
  await settleUntil(() => Boolean(document.querySelector(".assistant-chat-input")), "Deferred panel did not load");
  check(Object.is(surface(), openingSurface), "Loading-to-ready replaced the animated island surface");
  cases.push("loading-surface-continuity");
  check(sampledWidths.some((width) => width > origin.width + 2 && width < expectedBounds().width - 2),
    "Expansion skipped all intermediate capsule-to-panel geometry");
  check(sampledContent, "No intermediate frame kept full-size panel content");
  verifyBounds();
  check(!element(".assistant-island-content").inert, "Settled content remains inert");
  check(document.activeElement === input(), "Settled island did not focus the composer");
  check(surface()?.getAttribute("role") === "dialog" && surface()?.getAttribute("aria-modal") === "true",
    "The island and its toggle must share one modal accessibility boundary");
  check(document.querySelectorAll('[role="dialog"]').length === 1, "Island must not nest duplicate dialogs");
  cases.push("spring-geometry-and-content");

  await click(element<HTMLButtonElement>(".assistant-close-button"));
  check(surface()?.dataset.state === "closing", "Explicit close skipped the exit phase");
  check(surface()?.inert && surface()?.getAttribute("aria-hidden") === "true", "Exiting island remains accessible or interactive");
  check(trigger().getAttribute("aria-expanded") === "false", "Close did not update expanded state immediately");
  check(document.elementFromPoint(origin.left + origin.width / 2, origin.top + origin.height / 2)?.closest(".assistant-capsule"),
    "Closing surface blocks the restored capsule click target");
  await settleUntil(() => !surface(), "Explicit close did not release the portal");
  check(document.activeElement === trigger(), "Explicit close did not restore entry focus");
  cases.push("explicit-close-accessibility");

  const point = { x: origin.left + origin.width / 2, y: origin.top + origin.height / 2 };
  await coordinateClick(point.x, point.y);
  await nextFrame();
  check(document.elementFromPoint(point.x, point.y)?.closest(".assistant-island-toggle"),
    "Expanding shell replaced the original click target with an inert surface");
  await coordinateClick(point.x, point.y);
  check(trigger().getAttribute("aria-expanded") === "false", "Second coordinate click failed to reverse the opening");
  await settleUntil(() => !surface(), "Coordinate double activation did not close LAN");
  cases.push("coordinate-toggle-hit-testing");

  await open();
  const reversedSurface = surface();
  await click(expandedToggle());
  await settleUntil(() => Boolean(surface() && surface()!.getBoundingClientRect().width < expectedBounds().width - 8),
    "Return spring did not move toward the entry");
  const beforeReverse = surface()!.getBoundingClientRect();
  await click(trigger());
  check(Object.is(surface(), reversedSurface), "Reopening during return replaced the surface");
  check(Math.abs(surface()!.getBoundingClientRect().width - beforeReverse.width) < 60,
    "Reopening jumped to an endpoint instead of continuing the current geometry");
  await settleUntil(() => surface()?.dataset.state === "open", "Rapid reverse did not finish open");
  check(!surface()!.inert && surface()!.getAttribute("aria-hidden") !== "true", "Reopening retained exit-only accessibility state");
  verifyBounds();
  cases.push("close-reverse-continuity");

  await click(expandedToggle());
  await act(async () => {
    trigger().style.transform = "translateX(-120px)";
    window.dispatchEvent(new Event("resize"));
  });
  let lastExitOffset = Number.POSITIVE_INFINITY;
  await settleUntil(() => {
    if (!surface()) return true;
    const shell = surface()!.getBoundingClientRect();
    const capsule = trigger().getBoundingClientRect();
    lastExitOffset = Math.abs(shell.left + shell.width / 2 - capsule.left - capsule.width / 2);
    return false;
  }, "Exit did not finish after its anchor moved");
  check(lastExitOffset < 1, `Exit returned to stale coordinates, center error ${lastExitOffset}px`);
  trigger().style.transform = "";
  await open();
  cases.push("exit-anchor-tracking");

  const draft = "检查服务器，保留尚未发送的草稿";
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(input(), draft);
    input().dispatchEvent(new Event("input", { bubbles: true }));
  });
  input().focus();
  let visitedToggle = false;
  for (let index = 0; index < 8; index++) {
    await key("Tab");
    check(surface()!.contains(document.activeElement), "Native Tab escaped the expanded island");
    visitedToggle ||= document.activeElement === expandedToggle();
  }
  check(visitedToggle, "The island toggle is missing from the keyboard focus cycle");
  await key("Escape");
  await settleUntil(() => !surface(), "Escape did not finish closing");
  check(document.activeElement === trigger(), "Escape failed to return focus to the entry");
  await open();
  check(input().value === draft, "Closing discarded the unsent draft");
  cases.push("keyboard-focus-and-draft");

  const outside = element<HTMLButtonElement>("#outside-control");
  await act(async () => {
    outside.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    outside.focus();
    outside.click();
  });
  await settleUntil(() => !surface(), "Outside pointer did not close LAN");
  check(document.activeElement === outside, "Outside dismissal stole focus from the clicked control");
  cases.push("outside-pointer-focus");

  await open();
  await act(async () => { setConfirmationOpen(true); });
  await key("Escape");
  check(surface()?.dataset.state === "open", "LAN consumed Escape while operation confirmation owns interaction");
  await act(async () => { setConfirmationOpen(false); });
  await close();
  cases.push("confirmation-ownership");

  const themeColors: string[] = [];
  for (const theme of ["light", "dark"]) {
    document.documentElement.dataset.theme = theme;
    await open();
    verifyBounds();
    themeColors.push(getComputedStyle(surface()!).backgroundColor);
    await close();
  }
  check(themeColors[0] !== themeColors[1], "Island surface does not follow light and dark theme tokens");
  cases.push("theme-geometry");

  fixture!.classList.add("island-fixture-offset");
  await open();
  check(Math.abs(trigger().getBoundingClientRect().left + trigger().getBoundingClientRect().width / 2 - innerWidth / 2) > 50,
    "Offset layout did not distinguish the capsule anchor from the header center");
  verifyBounds();
  await close();
  fixture!.classList.remove("island-fixture-offset");
  cases.push("capsule-anchor-offset");

  await setReducedMotion(true);
  await act(async () => { root.unmount(); });
  root = createRoot(fixture!);
  await render();
  await click(trigger());
  await nextFrame();
  check(surface()?.dataset.state === "open", "Reduced-motion opening retained a spatial spring");
  verifyBounds();
  await click(expandedToggle());
  await nextFrame();
  check(!surface(), "Reduced-motion closing retained an exit animation");
  await setReducedMotion(false);
  await act(async () => { root.unmount(); });
  root = createRoot(fixture!);
  await render();
  cases.push("reduced-motion");

  await open();
  verifyBounds();
  check(document.documentElement.scrollWidth <= innerWidth, "Island creates horizontal viewport overflow");
  cases.push("viewport-bounds");
  await click(expandedToggle());
  await act(async () => { root.unmount(); });
  await nextFrame();
  check(!surface() && !document.querySelector(".assistant-panel"), "Unmount during return left an orphan portal");
  cases.push("unmount-cleanup");

  // Keep a real, settled final surface for the runner's optional screenshot.
  root = createRoot(fixture!);
  await render();
  await open();
  verifyBounds();
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  cases.push("browser-errors");
  return { status: "passed", cases, browser_errors: errors, opening_samples: sampledWidths.length,
    reduced_motion_boundary: "MediaQueryList change", theme_colors: themeColors };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`Island interaction stalled after ${cases.join(", ")}`)), 30_000);
})]).finally(() => clearTimeout(watchdog))
  .catch((error) => ({ status: "failed", cases, error: String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
