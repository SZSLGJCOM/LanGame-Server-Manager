import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { ActivityBar } from "../../src/components/ActivityBar";
import { createActivityNoticeStore } from "../../src/components/activity-notice-store";
import { InstallationActivity } from "../../src/components/InstallationActivity";
import { I18nProvider, useI18n, type LocaleCode } from "../../src/i18n";
import type { BackgroundJob, InstallProgress } from "../../src/types";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;display:flex;flex-direction:column";
const root = createRoot(fixture), store = createActivityNoticeStore(), errors: string[] = [], checks: string[] = [];
const nonce = new URLSearchParams(location.search).get("nonce");
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
let setLocale: (locale: LocaleCode) => void;
let stops = 0;
const job: BackgroundJob = { id: "fixture-verification", label: "Valheim", target_id: "valheim", kind: "DownloadGame",
  status: "Running", cancellable: true, cancel_requested: false, detail: "Synthetic verification fixture", output_excerpt: "",
  install_progress: { phase: "verifying", downloaded_bytes: null, total_bytes: null, percent: 25, elapsed_seconds: 36 } };

function Harness() {
  const i18n = useI18n(); setLocale = i18n.setLocale;
  return <>
    <main style={{ flex: 1, padding: 32 }}><h1>Valheim</h1><p>{i18n.t("installation.activity", { name: "Valheim", phase: "" })}</p></main>
    <footer className="shell-activity-bar"><span className="shell-activity-label">{i18n.t("shell.activity")}</span>
      <ActivityBar store={store} entries={[]} progress={<InstallationActivity job={job} name="Valheim" locale={i18n.locale}
        t={i18n.t} onStop={() => { stops++; }} />} />
    </footer>
  </>;
}
function draw() { root.render(<StrictMode><I18nProvider><Harness /></I18nProvider></StrictMode>); }
function check(value: unknown, label: string): asserts value { if (!value) throw new Error(label); checks.push(label); }
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const value = fixture.querySelector<T>(selector);
  if (!value) throw new Error(`Missing ${selector}`);
  return value;
}
async function settle(predicate: () => boolean, label: string) {
  const expires = performance.now() + 6000;
  while (!predicate()) {
    if (performance.now() >= expires) throw new Error(label);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
function insideFooter(selector: string) {
  const outer = element(".shell-activity-bar").getBoundingClientRect(), box = element(selector).getBoundingClientRect();
  check(box.width > 0 && box.left >= outer.left && box.right <= outer.right && box.top >= outer.top && box.bottom <= outer.bottom,
    `${selector} must fit inside footer: ${JSON.stringify({ outer: outer.toJSON(), box: box.toJSON() })}`);
}
async function progress(stage: NonNullable<InstallProgress["verification"]>["stage"], label: string) {
  job.install_progress = { phase: "verifying", downloaded_bytes: null, total_bytes: null, percent: stage === "recording_baseline" ? 100 : 25,
    elapsed_seconds: 36, verification: { stage, completed_bytes: 512 * 1024 * 1024, total_bytes: 2 * 1024 * 1024 * 1024,
      completed_files: 12, total_files: 48 } };
  await act(async () => { draw(); });
  await settle(() => Boolean(fixture.querySelector(".shell-task-phase")?.textContent?.includes(label)), label);
  check(!element(".shell-task-phase").textContent?.includes("安装已完成"), "Verification cannot announce installation completion");
  check(element(".shell-task-speed").textContent === "", "Local verification must not show download speed");
  const transfer = element(".shell-task-transfer");
  check(transfer.textContent?.includes("12 / 48"), "Local file counts must remain visible");
  if (stage === "recording_baseline") {
    check(!element(".shell-task-meter").hasAttribute("aria-valuenow"), "Saving cannot invent a measured percentage");
    check(!transfer.textContent?.includes("MB"), "Saving cannot repeat byte verification progress");
  } else {
    check(element(".shell-task-meter").getAttribute("aria-valuenow") === "25", "Measured percent must reach the progressbar");
    check(transfer.textContent?.includes("512 MB / 2 GB"), "Local byte counts must remain visible");
  }
  for (const selector of [".shell-task-meter", ".shell-task-transfer", ".shell-task-stop"]) insideFooter(selector);
  check(transfer.scrollWidth <= transfer.clientWidth + 1, `Local counters were truncated: ${transfer.scrollWidth}/${transfer.clientWidth}`);
  check(element(".shell-activity-bar").scrollWidth <= element(".shell-activity-bar").clientWidth + 1, "Footer must not overflow");
}
Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
  await act(async () => { root.unmount(); }); return { browser_errors: errors, native_dialogs: 0 };
} });
async function run() {
  await prepareBrowserLocaleCatalogs();
  await act(async () => { draw(); });
  await settle(() => Boolean(fixture.querySelector(".shell-task-phase")), "Initial locale must finish loading");
  for (const [locale, labels] of [
    ["zh-CN", ["正在核验原版文件", "正在复核程序文件", "正在保存程序清单"]],
    ["en-US", ["Verifying original Steam files", "Checking program files", "Saving program inventory"]]
  ] as const) {
    await act(async () => { setLocale(locale); });
    for (const [index, stage] of (["steam_files", "package_files", "recording_baseline"] as const).entries()) {
      await progress(stage, labels[index]);
    }
  }
  await act(async () => { setLocale("zh-CN"); });
  await progress("steam_files", "正在核验原版文件");
  const stop = element<HTMLButtonElement>(".shell-task-stop");
  await act(async () => { stop.focus(); stop.click(); });
  check(stops === 1, "Verification must retain cancellation");
  const expand = element<HTMLButtonElement>(".shell-activity-expand");
  await act(async () => { expand.focus(); expand.click(); });
  check(document.activeElement === element(".shell-activity-panel-close"), "Opening diagnostics must move focus into the panel");
  check(element(".shell-task-diagnostics").textContent?.includes("Synthetic verification fixture"), "Diagnostics must retain exact provider detail");
  await act(async () => { element<HTMLButtonElement>(".shell-activity-panel-close").click(); });
  check(document.activeElement === element(".shell-activity-expand"), "Closing diagnostics must restore focus");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  return { status: "passed", scenarios: 7, stops, checks, browser_errors: errors };
}
void run().catch((error) => ({ status: "failed", error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
