import React, { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { App } from "../../src/App";
import { DesktopExitBoundary } from "../../src/components/DesktopExitBoundary";
import { DesktopExitLifecycle } from "../../src/desktop-exit-lifecycle";
import { I18nProvider, useI18n } from "../../src/i18n";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import "../../src/app.css";

localStorage.setItem("langame.locale", "zh-CN");
localStorage.setItem("langame.theme", "dark");
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const nonce = new URLSearchParams(location.search).get("nonce");
const root = createRoot(document.getElementById("fixture")!);
const cases: string[] = [];
const listeners = new Set<(payload: unknown) => void>();
const intervals = new Set<number>();
const nativeSetInterval = window.setInterval;
const nativeClearInterval = window.clearInterval;
window.setInterval = ((...args: Parameters<typeof window.setInterval>) => {
  const id = nativeSetInterval(...args); intervals.add(id); return id;
}) as typeof window.setInterval;
window.clearInterval = (id) => { if (id !== undefined) intervals.delete(id); nativeClearInterval(id); };
let resolveInitialStatus: (status: unknown) => void;
let readStatus = () => new Promise<unknown>((resolve) => { resolveInitialStatus = resolve; });
function createLifecycle() {
  return new DesktopExitLifecycle({ enabled: true, readStatus: () => readStatus(),
    listen: async (callback) => { listeners.add(callback); return () => { listeners.delete(callback); }; } });
}
let lifecycle = createLifecycle();
let generation = 0;
let mounts = 0;
let activeChildren = 0;
let settleRead: (error: Error) => void;
let localeChange: ReturnType<typeof useI18n>["setLocale"];
function PendingRead() {
  const [error, setError] = useState("");
  useEffect(() => {
    mounts++; activeChildren++;
    let active = true;
    void new Promise<never>((_, reject) => { settleRead = reject; })
      .catch((failure: Error) => { if (active) setError(failure.message); });
    return () => { active = false; activeChildren--; };
  }, []);
  return error ? <p role="alert">{error}</p> : null;
}
function Harness() {
  localeChange = useI18n().setLocale;
  return <DesktopExitBoundary key={generation} lifecycle={lifecycle}>
    <PendingRead /><InstanceSettingsSaveProvider><App /></InstanceSettingsSaveProvider>
  </DesktopExitBoundary>;
}
function render() { root.render(<StrictMode><I18nProvider><Harness /></I18nProvider></StrictMode>); }
function check(condition: unknown, message: string): asserts condition { if (!condition) throw new Error(message); }
async function until(predicate: () => boolean, message: string) {
  const expires = performance.now() + 6000;
  while (!predicate()) {
    check(performance.now() < expires, message);
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}
function verifyBounds() {
  for (const element of document.querySelectorAll<HTMLElement>(".desktop-exit-content, .desktop-exit-content h1, .desktop-exit-content p")) {
    const bounds = element.getBoundingClientRect();
    check(bounds.width > 0 && bounds.height > 0 && bounds.left >= 0 && bounds.top >= 0
      && bounds.right <= innerWidth && bounds.bottom <= innerHeight
      && element.scrollWidth <= element.clientWidth + 1, "Exit content is clipped");
  }
}
Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
  root.unmount();
  window.setInterval = nativeSetInterval;
  window.clearInterval = nativeClearInterval;
  check(listeners.size === 0 && intervals.size === 0, "Exit fixture left active resources");
  return { browser_errors: errors, native_dialogs: 0 };
} });

async function run() {
  await Promise.all([
    prepareBrowserLocaleCatalogs(),
    import("../../src/api-mock"),
    import("../../src/views/ServerWorkspaceView")
  ]);
  render();
  await until(() => Boolean(resolveInitialStatus), "Initial native status was not requested");
  check(mounts === 0 && !document.querySelector(".shell-frame"), "Work started before native exit status was known");
  resolveInitialStatus!({ requested: false });
  await until(() => Boolean(document.querySelector(".shell-topnav")), "Normal desktop did not load");
  check(activeChildren === 1 && intervals.size > 0, "Desktop polling was not active");
  cases.push("initial-admission");

  const serverButton = [...document.querySelectorAll<HTMLButtonElement>(".shell-topnav button")]
    .find((button) => button.textContent?.includes("服务器"));
  check(serverButton, "Server navigation is missing"); serverButton.click();
  await until(() => Boolean(document.querySelector(".server-list-card")), "Real server workspace did not load");
  cases.push("real-server-workspace");

  for (const listener of listeners) listener({ requested: true });
  await until(() => Boolean(document.querySelector(".desktop-exit-content")) && activeChildren === 0, "Final exit did not release desktop work");
  check(!document.querySelector(".shell-frame") && intervals.size === 0, "Desktop UI or polling survived final exit");
  check(document.activeElement?.tagName === "H1", "Exit state did not receive accessible focus");
  settleRead!(new Error("instance detail reconciliation cannot begin while application shutdown is in progress"));
  await new Promise((resolve) => requestAnimationFrame(resolve));
  check(!document.body.textContent?.includes("reconciliation"), "An in-flight read overwrote exit feedback");
  cases.push("exit-unmounts-and-rejects-stale-read");

  const receipt = lifecycle.getSnapshot();
  for (const listener of listeners) listener({ requested: true });
  check(lifecycle.getSnapshot() === receipt, "Repeated exit replaced its receipt");
  generation++; render();
  await until(() => listeners.size === 0, "Unmount did not release the native listener");
  check(activeChildren === 0, "Remount resumed work after final exit");
  cases.push("repeat-and-remount");

  const priorMounts = mounts;
  readStatus = async () => ({ requested: true });
  lifecycle = createLifecycle(); generation++; render();
  await until(() => lifecycle.getSnapshot().requested, "A lost event was not recovered from native status");
  check(mounts === priorMounts, "Recreated WebView started background work during final exit");
  cases.push("missed-event-recovery");

  for (const theme of ["light", "dark"]) {
    document.documentElement.dataset.theme = theme;
    for (const locale of ["en-US", "zh-CN"] as const) {
      localeChange!(locale);
      await until(() => document.querySelector("h1")?.textContent === (locale === "en-US" ? "Exiting" : "正在退出"), "Exit copy is not localized");
      check(document.querySelector(".desktop-exit-content p")?.textContent === (locale === "en-US"
        ? "Handing off server stop requests." : "正在交接服务器停止任务。"), "Exit does not show the brief stop handoff");
      check(!/\d/.test(document.querySelector(".desktop-exit-content")?.textContent ?? ""), "Desktop exit still presents a server shutdown waiting budget");
      verifyBounds();
    }
  }
  await document.fonts.ready;
  check(errors.length === 0, errors.join("; "));
  cases.push("locales-themes-and-layout");
  return { status: "passed", cases, browser_errors: errors, active_intervals: intervals.size };
}
run().catch((error) => ({ status: "failed", error: String(error), cases, browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
