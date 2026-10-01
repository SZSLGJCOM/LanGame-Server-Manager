import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider, useI18n } from "../../src/i18n";
import { LibraryUpdatesPanel } from "../../src/views/library/LibraryUpdatesPanel";
import type { ModuleStoreEntry } from "../../src/store-media";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
sessionStorage.setItem("langameLanToken", "synthetic-updates-test-only");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const nonce = new URLSearchParams(location.search).get("nonce");
const errors: string[] = [];
const scenarios: string[] = [];
const nativeFetch = window.fetch.bind(window);
let checks = 0;
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };

interface PendingRequest { appId: number; locale: string; completed: boolean; resolve: (response: Response) => void }
const requests: PendingRequest[] = [];
window.fetch = (input, init) => {
  const url = new URL(input instanceof Request ? input.url : String(input), location.href);
  if (url.pathname !== "/__langame/api") return nativeFetch(input, init);
  const payload = JSON.parse(String(init?.body)) as { command: string; args: { appId: number; locale: string } };
  check(payload.command === "fetch_steam_news_for_app", `Unexpected command ${payload.command}`);
  check(new Headers(init?.headers).get("X-LanGame-Token") === "synthetic-updates-test-only", "LAN request lost its token");
  return new Promise<Response>((resolve) => requests.push({ ...payload.args, completed: false, resolve }));
};

function check(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
  checks++;
}
function find<T extends HTMLElement = HTMLElement>(selector: string): T {
  const element = fixture.querySelector<T>(selector);
  if (!element) throw new Error(`Missing ${selector}`);
  return element;
}
async function waitFor(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(`Timed out: ${message}`);
    await act(async () => { await new Promise<void>((resolve) => setTimeout(resolve, 10)); });
  }
}
function Harness({ appId }: { appId: number }) {
  const { setLocale } = useI18n();
  const entry: ModuleStoreEntry = {
    storeSource: "steam", storeAppId: appId, storeName: "Fixture game", shortDescription: "", aboutParagraphs: [],
    genres: [], categories: [], developers: [], publishers: [], releaseDate: "", storeUrl: "", officialLinks: [],
    screenshots: [], trailers: [], coverUrl: null
  };
  return <main style={{ width: "min(1040px, calc(100% - 64px))", margin: "32px auto" }}>
    <nav style={{ display: "flex", gap: 12, marginBottom: 24 }}>
      <button id="chinese" className="secondary-button" onClick={() => setLocale("zh-CN")}>中文</button>
      <button id="english" className="secondary-button" onClick={() => setLocale("en-US")}>English</button>
      <button id="keyboard-start" className="secondary-button">Keyboard start</button>
    </nav>
    <LibraryUpdatesPanel storeEntry={entry} />
  </main>;
}
async function render(appId: number) {
  await act(async () => { root.render(<StrictMode><I18nProvider><Harness appId={appId} /></I18nProvider></StrictMode>); });
}
async function request(appId: number, locale: string) {
  await waitFor(() => requests.some((entry) => !entry.completed && entry.appId === appId && entry.locale === locale), `request ${appId}/${locale}`);
  return requests.find((entry) => !entry.completed && entry.appId === appId && entry.locale === locale)!;
}
async function respond(pending: PendingRequest, title: string, failure = false) {
  pending.completed = true;
  await act(async () => { pending.resolve(new Response(JSON.stringify(failure
    ? { ok: false, error: "fixture news unavailable" } : { ok: true, value: [{ gid: title, title,
      url: "https://store.steampowered.com/news/app/322330/view/fixture", excerpt: "Official announcement fixture.",
      feed_label: "Steam", author: "", published_at_unix_ms: 1700000000000 }] }), {
    status: failure ? 503 : 200, headers: { "Content-Type": "application/json" }
  })); });
}
async function click(selector: string) { await act(async () => { find(selector).click(); }); }
async function key(value: "Tab" | "Enter") {
  await act(async () => { check((await nativeFetch(`/__reliability_key/${nonce}/${value}`, { method: "POST" })).ok, `Native ${value} failed`); });
}
const retrySelector = ".library-updates-strip .shell-activity-notice-actions button";
async function article(title: string) {
  await waitFor(() => find(".library-updates-strip").textContent?.includes(title) ?? false, `article ${title}`);
}
async function run() {
  await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]);
  await render(902001);
  await respond(await request(902001, "zh-CN"), "中文公告");
  await article("中文公告");
  await click("#english");
  const english = await request(902001, "en-US");
  check(!fixture.textContent?.includes("中文公告"), "Previous locale remained visible while loading");
  scenarios.push("language-request");
  await respond(english, "", true);
  await waitFor(() => Boolean(fixture.querySelector(retrySelector)), "English retry");
  check(find(retrySelector).textContent === "Retry", "English retry label missing");
  find("#keyboard-start").focus();
  await key("Tab");
  check(document.activeElement === fixture.querySelector(retrySelector), "Retry not reachable using Tab");
  await key("Enter");
  await respond(await request(902001, "en-US"), "English announcement");
  await article("English announcement");
  scenarios.push("english-keyboard-retry");
  const beforeCache = requests.length;
  await click("#chinese"); await article("中文公告");
  await click("#english"); await article("English announcement");
  check(requests.length === beforeCache, "Successful locale cache was not reused independently");
  scenarios.push("locale-cache");
  await render(902002);
  const old = await request(902002, "en-US");
  await click("#chinese");
  const current = await request(902002, "zh-CN");
  await respond(old, "OBSOLETE");
  check(!fixture.textContent?.includes("OBSOLETE"), "Previous locale response overwrote current selection");
  await respond(current, "", true);
  await waitFor(() => Boolean(fixture.querySelector(retrySelector)), "Chinese failure");
  scenarios.push("obsolete-response");
  await render(902003);
  await respond(await request(902003, "zh-CN"), "", true);
  await click("#english");
  await respond(await request(902003, "en-US"), "Recovered after route change");
  await article("Recovered after route change");
  scenarios.push("failed-locale-switch");
  await click("#chinese");
  await respond(await request(902003, "zh-CN"), "", true);
  await waitFor(() => Boolean(fixture.querySelector(retrySelector)), "Chinese retry");
  check(find(retrySelector).textContent === "重试", "Chinese retry label missing");
  await click(retrySelector);
  await respond(await request(902003, "zh-CN"), "重新读取后的公告");
  await article("重新读取后的公告");
  scenarios.push("chinese-retry");
  check(requests.every((entry) => entry.completed), "Unresolved response remained");
  check(document.documentElement.scrollWidth <= innerWidth, "Updates overflow the desktop viewport");
  check(find(".library-updates-strip").getBoundingClientRect().width > 500, "Updates panel has no usable width");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, scenarios, requests: requests.length, browser_errors: errors };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`Update browser test stalled after ${checks} checks`)), 25000);
})]).catch((error: unknown) => ({ status: "failed", checks, scenarios,
  error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); window.fetch = nativeFetch; })
  .then((report) => nativeFetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
