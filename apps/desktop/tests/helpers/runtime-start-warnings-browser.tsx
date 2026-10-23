import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider, useI18n } from "../../src/i18n";
import { ActivityBar } from "../../src/components/ActivityBar";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { createActivityNoticeStore } from "../../src/components/activity-notice-store";
import { RuntimeStartWarnings } from "../../src/components/RuntimeStartWarnings";
import type { RuntimeStartNotice } from "../../src/runtime-start-warning";
import type { BackgroundJob, RuntimeStartWarning } from "../../src/types";
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
const fixture = document.getElementById("fixture");
if (!fixture) throw new Error("Missing fixture root");
const root = createRoot(fixture);
const store = createActivityNoticeStore();
let jobs: BackgroundJob[] = [];
let manual: RuntimeStartNotice | undefined;
let scenarios = 0;
let dismissals = 0;
Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
  await act(async () => { root.unmount(); });
  return { browser_errors: errors, native_dialogs: 0 };
} });

const warning: RuntimeStartWarning = { code: "mod_update_using_cache", provider: "steam_workshop", reason: "network",
  item_ids: ["1234"], message: "https://native.invalid/?signature=PRIVATE_RESPONSE", map_key: "main",
  log_path: "C:/fixture/logs/start.log", http_status: null };
const job: BackgroundJob = { id: "autostart-one", kind: "StartInstance", status: "Running", label: "Autostart Fixture Server",
  target_id: "instance-one", cancellable: false, cancel_requested: false, warnings: [warning] };

function Shell() {
  const { t, setLocale } = useI18n();
  return <ActivityNoticeTarget.Provider value={{ element: null, store, dismissLabel: t("servers.mods.closeToast") }}>
    <RuntimeStartWarnings jobs={jobs} manual={manual} />
    <div className="shell-frame"><div className="shell-main shell-main--console">
      <header />
      <main className="shell-content-scroll"><button type="button" id="locale-en" onClick={() => setLocale("en-US")}>English</button></main>
      <footer className="shell-activity-bar" role="status" aria-live="polite">
        <span className="shell-activity-label">{t("shell.activity")}</span><ActivityBar store={store} entries={[]} />
      </footer>
    </div></div>
  </ActivityNoticeTarget.Provider>;
}
function render() { root.render(<StrictMode><I18nProvider><Shell /></I18nProvider></StrictMode>); }
function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const node = fixture?.querySelector<T>(selector);
  check(node, `Missing ${selector}`);
  return node;
}
async function settleUntil(predicate: () => boolean, description: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    check(performance.now() < deadline, description);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function pressEnter(button: HTMLButtonElement) {
  await act(async () => {
    button.focus();
    check(document.activeElement === button, "Warning control cannot receive keyboard focus");
    const response = await fetch(`/__reliability_key/${nonce}/Enter`, { method: "POST" });
    check(response.ok, "Native Enter dispatch failed");
  });
}
async function rerender(next: BackgroundJob[]) {
  jobs = next;
  await act(async () => { render(); });
}

async function run() {
  await act(async () => { await prepareBrowserLocaleCatalogs(); render(); });
  await rerender([job]);
  check(store.getSnapshot().length === 0, "Running jobs must not claim successful cache fallback"); scenarios++;
  const completed = { ...job, status: "Completed", warnings: [warning, { ...warning }] };
  await rerender([completed]);
  await settleUntil(() => store.getSnapshot().length === 1, "Completed autostart warning missing or duplicated");
  check(element(".shell-activity-notice.is-warning").textContent?.includes("尚未确认最新版本"), "Chinese latest-version uncertainty missing"); scenarios++;
  await pressEnter(element<HTMLButtonElement>(".shell-activity-expand"));
  const panel = element(".shell-activity-panel");
  check(panel.getAttribute("role") === "dialog", "Warning details must be an accessible dialog");
  check(panel.textContent?.includes("C:/fixture/logs/start.log") && panel.textContent.includes("1234"), "Scoped log and Mod IDs missing");
  check(!panel.textContent?.includes("PRIVATE_RESPONSE") && !panel.textContent?.includes("native.invalid"), "Backend response leaked");
  check(panel.scrollWidth <= panel.clientWidth + 1, "Warning details overflowed the activity panel"); scenarios++;
  await pressEnter(element<HTMLButtonElement>(".shell-activity-panel .shell-activity-notice-close"));
  await settleUntil(() => store.getSnapshot().length === 0, "Autostart warning not dismissed"); dismissals++; scenarios++;
  await rerender([JSON.parse(JSON.stringify(completed))]);
  check(store.getSnapshot().length === 0, "Polling replayed a dismissed warning"); scenarios++;
  await pressEnter(element<HTMLButtonElement>("#locale-en"));
  await settleUntil(() => document.documentElement.lang === "en-US", "English locale did not load");
  await rerender([{ ...completed, warnings: [{ ...warning, message: "different native wording" }] }]);
  check(store.getSnapshot().length === 0, "Language or backend wording replayed a dismissed warning"); scenarios++;
  await rerender([{ ...completed, id: "restart-two", label: "Restart Fixture Server" }]);
  await settleUntil(() => store.getSnapshot().length === 1, "A new restart operation lost its warning");
  check(element(".shell-activity-notice.is-warning").textContent?.includes("latest versions were not confirmed"), "English uncertainty missing");
  await pressEnter(element<HTMLButtonElement>(".shell-activity-viewport .shell-activity-notice-close"));
  await settleUntil(() => store.getSnapshot().length === 0, "Restart warning not dismissed"); dismissals++; scenarios++;
  await rerender([{ ...completed, id: "failed-native", status: "Failed" }, { ...completed, id: "download", kind: "DownloadWorkshop" }]);
  check(store.getSnapshot().length === 0, "Failed native initialization or a download job appeared as a successful start"); scenarios++;
  manual = { id: "manual-start:instance-one:3", instanceId: "instance-one", label: "Manual Fixture Server", warnings: [warning] };
  await rerender([]);
  await settleUntil(() => store.getSnapshot().length === 1, "Manual start result did not publish its warning");
  await pressEnter(element<HTMLButtonElement>(".shell-activity-expand"));
  check(element(".shell-activity-panel").textContent?.includes("Manual Fixture Server"), "Manual result label missing"); scenarios++;
  manual = { id: "manual-start:astro:4", instanceId: "astro", label: "ASTRONEER", warnings: [{
    code: "world_initialization_incomplete", provider: null, reason: null, item_ids: [],
    message: "PRIVATE_RESPONSE", map_key: null, http_status: null, log_path: "C:/fixture/logs/astro.log",
  }] };
  await rerender([]);
  await settleUntil(() => store.getSnapshot().some((entry) => entry.text.includes("first-world initialization did not complete")), "Native world warning missing");
  check(element(".shell-activity-panel").textContent?.includes("ASTRONEER"), "Native world warning label missing");
  check(!element(".shell-activity-panel").textContent?.includes("PRIVATE_RESPONSE"), "Native initialization diagnostic body leaked"); scenarios++;
  for (const [code, expected] of [["manual_save_unavailable", "manual saving is unavailable"],
    ["graceful_stop_unavailable", "normal shutdown is unavailable"],
    ["program_update_using_installed", "game update check did not complete"]] as const) {
    manual = { id: `manual-start:${code}:5`, instanceId: code, label: "Native server", warnings: [{
      code, provider: null, reason: null, item_ids: [], message: "PRIVATE_RESPONSE",
      map_key: null, http_status: null, log_path: "C:/fixture/logs/native.log",
    }] };
    await rerender([]);
    await settleUntil(() => store.getSnapshot().some((entry) => entry.text.includes(expected)), "Native limitation warning missing");
    check(!element(".shell-activity-panel").textContent?.includes("PRIVATE_RESPONSE"), "Native limitation leaked diagnostic detail");
    scenarios++;
  }
  const bounds = element(".shell-activity-panel").getBoundingClientRect();
  check(bounds.top >= 0 && bounds.bottom <= innerHeight, "Startup warning details escaped the viewport");
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  return { status: "passed", scenarios, dismissals, browser_errors: errors };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`Warning interaction stalled after ${scenarios} scenarios`)), 40_000);
})]).finally(() => clearTimeout(watchdog))
  .catch((error) => ({ status: "failed", scenarios, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
