import React, { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { readInstanceDetails } from "../../src/api";
import { I18nProvider, useI18n } from "../../src/i18n";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { InstanceAutostartEditor } from "../../src/views/servers/InstanceAutostartEditor";
import { InstancePanelReadStatus } from "../../src/views/servers/InstancePanelReadStatus";
import { LibraryStoryPanel } from "../../src/views/library/LibraryStoryPanel";
import type { InstanceDetails } from "../../src/types";
import type { ModuleStoreEntry } from "../../src/store-media";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "en-US");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:24px;box-sizing:border-box;display:flex;flex-direction:column";
const root = createRoot(fixture);
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const checks: string[] = [];
function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
  checks.push(description);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const selected = fixture.querySelector<T>(selector);
  if (!selected) throw new Error(`Missing ${selector}`);
  return selected;
}
async function waitFor(selector: string) {
  const deadline = performance.now() + 5000;
  while (!fixture.querySelector(selector)) {
    if (performance.now() >= deadline) throw new Error(`Missing ${selector}: ${fixture.textContent}; ${errors.join("; ")}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function checkBounds(selector: string, before: DOMRect, description: string) {
  const after = element(selector).getBoundingClientRect();
  check((["x", "y", "width", "height"] as const).every((property) => Math.abs(before[property] - after[property]) < 1), description);
}
function bottomNotice() {
  const notice = element(".shell-activity-notice");
  const footer = element(".shell-activity-bar");
  const before = footer.getBoundingClientRect();
  const after = notice.getBoundingClientRect();
  check(footer.contains(notice) && after.top >= before.top && after.bottom <= before.bottom,
    "Operation feedback is contained by the fixed activity bar");
  check(!element("main").querySelector('[role="alert"], .shell-activity-notice'), "Operation feedback consumes no content space");
  return notice;
}
async function pressEnter(button: HTMLButtonElement) {
  await act(async () => {
    button.focus();
    const rect = button.getBoundingClientRect();
    check(document.activeElement === button && rect.width > 0 && rect.left >= 0 && rect.right <= innerWidth,
      "Bottom retry is visible and keyboard reachable");
    const response = await fetch(`/__reliability_key/${nonce}/Enter`, { method: "POST" });
    check(response.ok, "Native keyboard retry dispatched");
  });
}

let details: InstanceDetails;
let mode: "autostart" | "read" | "story" = "autostart";
let saveCalls = 0;
let readRetries = 0;
let storyCalls = 0;
let saving = deferred<void>();
let story = deferred<string | null>();
let readError: string | null = null;
let readLoading = false;
const storeEntry: ModuleStoreEntry = { storeSource: "steam", storeAppId: 252490, storeName: "Fixture game",
  coverUrl: null, shortDescription: "Local story", aboutParagraphs: ["Existing local story content."],
  genres: [], categories: [], developers: [], publishers: [], releaseDate: "", storeUrl: "", officialLinks: [], screenshots: [], trailers: [] };
function Harness() {
  const { t } = useI18n();
  const [target, setTarget] = useState<HTMLDivElement | null>(null);
  return <ActivityNoticeTarget.Provider value={{ element: target, dismissLabel: "Close" }}>
    <main style={{ flex: 1, minHeight: 0 }}>
      {mode === "autostart" ? <InstanceAutostartEditor details={details} t={t}
        onSaveAutostart={async (_id, enabled) => {
          saveCalls++; await saving.promise;
          details = { ...details, summary: { ...details.summary, autostart: enabled } }; draw();
        }} /> : mode === "read" ? <section className="read-result">
          <p>Existing backup list content</p>
          <InstancePanelReadStatus part="backups" error={readError} loading={readLoading}
            onRetry={() => { readRetries++; readError = null; readLoading = true; draw(); }} />
        </section> : <LibraryStoryPanel storeEntry={storeEntry} storyParagraphs={[]} />}
    </main>
    <footer className="shell-activity-bar"><span className="shell-activity-label">Activity</span>
      <div className="shell-activity-notices" ref={setTarget} />
    </footer>
  </ActivityNoticeTarget.Provider>;
}
function draw() { root.render(<I18nProvider><Harness /></I18nProvider>); }
async function run() {
  const original = await readInstanceDetails("srv-dst-terminal-error");
  details = { ...original, summary: { ...original.summary, autostart: true } };
  Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string) => {
    if (command !== "fetch_steam_store_about") throw new Error(`Unexpected IPC: ${command}`);
    storyCalls++; return story.promise;
  } } });
  await act(async () => { draw(); });
  await waitFor(".server-autostart-policy-editor");
  const policyBefore = element(".server-autostart-policy-editor").getBoundingClientRect();
  await act(async () => { element<HTMLInputElement>('input[type="checkbox"]').click(); });
  bottomNotice();
  check(element<HTMLInputElement>('input[type="checkbox"]').disabled && saveCalls === 1, "Pending policy save disables duplicate writes");
  checkBounds(".server-autostart-policy-editor", policyBefore, "Saving adds no space beneath the policy control");
  await act(async () => { saving.reject(new Error("Policy write denied SAVE_ERROR_END")); });
  check(bottomNotice().textContent?.includes("SAVE_ERROR_END"), "Save failure retains its cause in the activity bar");
  checkBounds(".server-autostart-policy-editor", policyBefore, "Failed policy save does not add height");
  saving = deferred<void>();
  await pressEnter(element<HTMLButtonElement>(".shell-activity-notice-actions button"));
  check(saveCalls === 2, "Retry resubmits the intended policy save exactly once");
  await act(async () => { saving.resolve(); });
  check(bottomNotice().classList.contains("is-success") && !element<HTMLInputElement>('input[type="checkbox"]').checked,
    "Successful retry applies the requested value and reports success below");
  checkBounds(".server-autostart-policy-editor", policyBefore, "Save success does not add height");

  mode = "read";
  await act(async () => { draw(); });
  check(!fixture.querySelector(".shell-activity-notice"), "Leaving policy editor clears its feedback");
  const readBefore = element(".read-result").getBoundingClientRect();
  readError = "Backup database unavailable READ_ERROR_END";
  await act(async () => { draw(); });
  check(bottomNotice().textContent?.includes("READ_ERROR_END"), "Read error keeps the complete failure detail");
  await pressEnter(element<HTMLButtonElement>(".shell-activity-notice-actions button"));
  check(readRetries === 1 && bottomNotice().getAttribute("role") === "status", "Read retry changes failure into bottom loading status");
  checkBounds(".read-result", readBefore, "Read failure and retry preserve existing result geometry");
  readLoading = false;
  await act(async () => { draw(); });
  check(!fixture.querySelector(".shell-activity-notice"), "Completed read removes the pending message");

  mode = "story";
  await act(async () => { draw(); });
  const storyBefore = element(".library-story-panel").getBoundingClientRect();
  bottomNotice();
  check(element(".library-story-copy").textContent === "Existing local story content.", "Remote loading preserves local story content");
  await act(async () => { story.reject(new Error("Fixture Steam unavailable")); });
  await waitFor(".shell-activity-notice.is-error");
  check(bottomNotice().classList.contains("is-error"), "Story failure is reported in the activity bar");
  checkBounds(".library-story-panel", storyBefore, "Story failure does not insert status or retry above the text");
  story = deferred<string | null>();
  await pressEnter(element<HTMLButtonElement>(".shell-activity-notice-actions button"));
  check(storyCalls === 2, "Story retry actually starts a second request");
  checkBounds(".library-story-panel", storyBefore, "Story retry preserves local fallback geometry");
  await act(async () => { story.resolve("<p>Actual remote story content.</p>"); });
  await waitFor(".library-story-html");
  check(element(".library-story-html").textContent?.includes("Actual remote story content."), "Retry success displays actual remote result");
  check(!fixture.querySelector(".shell-activity-notice"), "Story success removes stale operation feedback");
  check(errors.length === 0, "No browser, React or unhandled promise errors");
  return { status: "passed", checks, save_calls: saveCalls, read_retries: readRetries, story_calls: storyCalls, browser_errors: errors };
}
void run().catch((error) => ({ status: "failed", error: String(error?.stack ?? error), checks, browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
