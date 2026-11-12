import React, { act, StrictMode, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { invoke } from "@tauri-apps/api/core";
import { AppHeader } from "../../src/components/AppHeader";
import { createDefaultAiSettings } from "../../src/ai-settings";
import { fallbackBootstrap } from "../../src/app-state";
import { I18nProvider } from "../../src/i18n";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true, isTauri: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const errors: string[] = [];
addEventListener("error", event => errors.push(event.message));
addEventListener("unhandledrejection", event => errors.push(String(event.reason)));
const calls: { command: string; anchor?: { xRatio: number; y: number } }[] = [];
let maximized = true;
mockWindows("main");
mockIPC(async (command, args) => {
  if (command === "plugin:window|is_maximized") return maximized;
  if (command === "set_tray_locale") return;
  calls.push({ command, anchor: (args as { anchor?: { xRatio: number; y: number } } | undefined)?.anchor });
  if (command === "start_desktop_window_drag" || command === "plugin:window|toggle_maximize") {
    maximized = command === "start_desktop_window_drag" ? false : !maximized;
    await invoke("plugin:event|emit", { event: "tauri://resize", payload: { width: innerWidth, height: innerHeight } });
  } else if (!["plugin:window|minimize", "plugin:window|close"].includes(command)) {
    throw new Error(`Unexpected header command: ${command}`);
  }
}, { shouldMockEvents: true });
const root = createRoot(document.getElementById("fixture")!);
const noop = () => {};
const aiSettings = createDefaultAiSettings();
const props: ComponentProps<typeof AppHeader> = {
  activeView: "system", theme: "dark", serverCount: 0, aiSettings,
  assistant: { panelTitle: "LAN", tone: "info" }, assistantDraft: "",
  assistantExecution: { status: "idle", promptLabel: null, result: null, error: null },
  assistantMessages: [], assistantConversations: [], assistantActiveConversationId: null,
  assistantInput: { aiSettings, locale: "zh-CN", activeJobsCount: 0, activeView: "system",
    bootstrap: fallbackBootstrap, storageReady: true, libraryPage: "catalog", overlayNames: [],
    runtimeAutoRefreshPaused: false, runtimeRefreshIssue: null, selectedInstanceDetails: null,
    selectedInstanceId: null, selectedModuleId: null, selectedInstanceModuleDetails: null,
    selectedLaunchPlan: null, selectedLaunchPlanError: null, selectedLogDocument: null,
    selectedModuleDetails: null, selectedRuntime: null, serverWorkspaceSection: "overview", steamCmdStatus: null },
  onAssistantAction: noop, onAssistantDeleteConversation: noop, onAssistantDraftChange: noop,
  onAssistantNewConversation: noop, onAssistantSelectConversation: noop, onAssistantRunPrompt: noop,
  onAssistantSendMessage: noop, onClearAiSecret: async next => next, onSaveAiSettings: async next => next,
  onSelectView: noop, onThemeChange: noop,
};
function check(value: unknown, message: string) { if (!value) throw new Error(message); }
function header() { return document.querySelector<HTMLElement>(".shell-header")!; }
async function mouse(target: EventTarget, type: string, options: MouseEventInit = {}) {
  await act(async () => { target.dispatchEvent(new MouseEvent(type, {
    bubbles: true, cancelable: true, clientX: innerWidth / 2, clientY: 20,
    button: 0, buttons: type === "mouseup" ? 0 : 1, detail: 1, ...options,
  })); });
}
async function press(target: EventTarget = header(), options: MouseEventInit = {}) {
  await mouse(target, "mousedown", options);
}
async function move(options: MouseEventInit = {}) { await mouse(window, "mousemove", { clientY: 50, ...options }); }
async function release() { await mouse(window, "mouseup"); }
async function nativePointer(type: "click" | "drag", x: number, y: number) {
  await act(async () => {
    const response = await fetch(`/__reliability_pointer/${nonce}`, { method: "POST",
      body: JSON.stringify({ type, x, y, ...(type === "drag" ? { deltaX: 15, deltaY: 40 } : {}) }) });
    check(response.ok, "Browser pointer command failed");
  });
}
const cases: string[] = [];
async function run() {
  await prepareBrowserLocaleCatalogs();
  await act(async () => { root.render(<StrictMode><I18nProvider><AppHeader {...props} /></I18nProvider></StrictMode>); });
  const deadline = performance.now() + 5000;
  while (!header() || header().dataset.windowMaximized !== "true") {
    check(performance.now() < deadline, "Header did not observe the native maximize state");
    await act(async () => { await new Promise<void>(resolve => requestAnimationFrame(() => resolve())); });
  }
  await nativePointer("click", 8, 20);
  await press(); await move({ clientY: 22 }); await release(); await move();
  check(calls.length === 0 && maximized, "Click and tiny pointer jitter must keep maximized state");
  cases.push("click-and-jitter");

  await nativePointer("drag", 8, 20);
  check(calls.length === 1 && calls[0].command === "start_desktop_window_drag" && !maximized,
    "Real browser drag must request one native drag and observe restoration");
  check(header().dataset.windowMaximized === "false", "Restoration must update maximize button and dial typography state");
  const anchor = calls[0].anchor!;
  check(Math.abs(anchor.xRatio - 8 / innerWidth) < 1e-9 && anchor.y === 20 * devicePixelRatio,
    "Drag anchor must preserve the press point, including WebView device scaling");
  calls.length = 0;
  cases.push("maximized-pointer-drag");

  await press(); await move(); await move(); await release();
  check(calls.length === 1 && calls[0].command === "start_desktop_window_drag", "Windowed dragging must hand off exactly once");
  calls.length = 0;
  cases.push("windowed-drag");

  await press(); await release(); await press(header(), { detail: 2 });
  await mouse(header(), "dblclick", { detail: 2 }); await release();
  check(calls.length === 1 && calls[0].command === "plugin:window|toggle_maximize" && maximized,
    "Double-click must maximize without initiating a drag");
  calls.length = 0;
  cases.push("double-click");

  const controls = [...header().querySelectorAll<HTMLButtonElement>(".shell-window-controls button")];
  for (const control of controls) await act(async () => control.click());
  check(calls.map(call => call.command).join(",") ===
    "plugin:window|minimize,plugin:window|toggle_maximize,plugin:window|close" && !maximized,
    "Minimize, restore, and tray buttons must retain their native commands");
  calls.length = 0;
  cases.push("window-buttons");

  for (const control of header().querySelectorAll("button, input, [role='button']")) {
    await press(control); await move(); await release();
  }
  await press(header(), { button: 2, buttons: 2 }); await move(); await release();
  check(calls.length === 0, "Header controls and secondary mouse button must never start dragging");
  cases.push("controls-and-right-click");

  await press(); await act(async () => { window.dispatchEvent(new Event("blur")); }); await move();
  await press(); await move({ buttons: 0 }); await move();
  await press(); await release(); await move();
  check(calls.length === 0, "Blur, release, and missing held button must cancel pending drag");
  cases.push("gesture-cancellation");

  await press(); await act(async () => root.unmount()); await move();
  check(calls.length === 0, "Unmount must remove pending gesture listeners");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  clearMocks();
  cases.push("unmount");
  return { status: "passed", cases, device_scale: devicePixelRatio, browser_errors: errors };
}
void run().catch(error => ({ status: "failed", cases, error: String(error.stack ?? error), browser_errors: errors }))
  .then(report => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
