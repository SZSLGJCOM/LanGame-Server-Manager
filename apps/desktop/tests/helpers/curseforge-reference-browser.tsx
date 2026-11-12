import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { Channel } from "@tauri-apps/api/core";
import { readInstanceDetails, updateInstance } from "../../src/api";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import { ActivityBar } from "../../src/components/ActivityBar";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { createActivityNoticeStore } from "../../src/components/activity-notice-store";
import { I18nProvider, useI18n } from "../../src/i18n";
import { ModWorkbench } from "../../src/views/servers/ModWorkbench";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceDetails, ManualModInventoryResult, ManualModReferenceResolveResult } from "../../src/types";
import { createModWorkbenchFixtureIpc } from "./mod-workbench-browser-support";

type Bounds = { x: number; y: number; width: number; height: number };
type CaptureMode = "background" | "success" | "verification";
type PageCaptureEvidence = { filename: string; sha256: string; width: number; height: number };
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
type Session = {
  id: string; scenario: string; openBounds: Bounds | null; updates: (Bounds | null)[];
  progress: Channel<"loading" | "verification_required">;
  opening: ReturnType<typeof deferred<string>>; resolution: ReturnType<typeof deferred<ManualModReferenceResolveResult>>;
  closed: number; resolved: boolean;
};

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:16px;box-sizing:border-box;display:flex;flex-direction:column";
const root = createRoot(fixture), moduleDetails = buildMockModuleDetails("arksurvivalascended");
const noticeStore = createActivityNoticeStore();
const projectUrl = "https://www.curseforge.com/ark-survival-ascended/mods/devkitlivemodtesting";
const sessions: Session[] = [], checks: string[] = [], errors: string[] = [], events: string[] = [];
let stored: InstanceDetails, details: InstanceDetails, inventory: ManualModInventoryResult;
let epoch = 0, saves = 0, resolutions = 0, inventoryReads = 0;
let scenario = "", holdOpen = false, permittedSave = false;
let cachedPresentation = false, holdPresentationRead = false;
let locale = "en-US", capture: CaptureMode | null = null;
let verificationSlot: Bounds | null = null;
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
function check(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label + ": " + fixture.textContent); checks.push(label);
}
function element<T extends HTMLElement = HTMLElement>(selector: string, within: ParentNode = fixture): T {
  const value = within.querySelector<T>(selector);
  check(value, "Present: " + selector); return value;
}
function button(label: string): HTMLButtonElement {
  const value = [...fixture.querySelectorAll<HTMLButtonElement>("button")].find((entry) => entry.textContent?.trim() === label);
  check(value, "Present button: " + label); return value;
}
async function settle(predicate: () => boolean, label: string, timeout = 5000) {
  const deadline = performance.now() + timeout;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(label + ": " + fixture.textContent);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
function Workspace() {
  const { t } = useI18n();
  const [visibleDetails, setVisibleDetails] = useState(details);
  return <ActivityNoticeTarget.Provider value={{ element: null, store: noticeStore, dismissLabel: t("servers.mods.closeToast") }}>
    <div style={{ flex: 1, minHeight: 0 }}>
      <ModWorkbench key={`${epoch}:${details.summary.id}`} details={visibleDetails} moduleDetails={moduleDetails} launchPlan={null}
        onSaveSettings={async (input, options) => {
          const saved = await updateInstance(input, options?.expectedSettingsJson ?? "", options?.collectionRemoval);
          saves += 1;
          setVisibleDetails(saved);
          return saved;
        }} />
    </div>
    <footer className="shell-activity-bar" role="status" aria-live="polite" aria-atomic="false">
      <span className="shell-activity-label">{t("shell.activity")}</span>
      <ActivityBar store={noticeStore} entries={[]} />
    </footer>
  </ActivityNoticeTarget.Provider>;
}
async function mount() {
  const before = inventoryReads;
  epoch += 1; details = structuredClone(stored);
  // Match main.tsx: new embedded children must survive StrictMode effect replay
  // without opening a second native host for the same request.
  await act(async () => root.render(<React.StrictMode><I18nProvider><InstanceSettingsSaveProvider key={epoch}>
    <Workspace />
  </InstanceSettingsSaveProvider></I18nProvider></React.StrictMode>));
  await settle(() => inventoryReads > before && Boolean(fixture.querySelector(".mw-dropzone")), "ASA workbench did not mount");
}
async function click(target: HTMLButtonElement) {
  check(!target.disabled, "Action is enabled: " + target.textContent);
  await act(async () => target.click());
}
async function begin(name: string, selector: string, delayedOpen = false): Promise<Session> {
  scenario = name; holdOpen = delayedOpen;
  const previous = sessions.length, zone = element(selector);
  const transfer = new DataTransfer(); transfer.setData("text/uri-list", projectUrl);
  Object.defineProperty(transfer, "effectAllowed", { value: "link" });
  Object.defineProperty(transfer, "dropEffect", { writable: true, value: "none" });
  const drop = new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: transfer });
  await act(async () => zone.dispatchEvent(drop));
  check(drop.defaultPrevented && transfer.dropEffect === "link", "Official project drop reaches the existing link workflow");
  await settle(() => sessions.length === previous + 1, "Embedded host was not opened");
  const session = sessions.at(-1)!;
  if (!delayedOpen) await settle(() => session.resolved, "Resolution did not receive the native session");
  check(session.openBounds === null && session.updates.every((bounds) => bounds === null),
    "Normal recognition reserves a hidden native host without visible bounds");
  check(!fixture.querySelector(".mw-reference-browser"), "Normal recognition does not mount the embedded webpage");
  check(!fixture.querySelector('dialog[open], [role="dialog"], [role="alertdialog"]'), "Recognition does not introduce an HTML popup");
  const activity = element(".shell-activity-bar .shell-activity-notice");
  check(activity.textContent?.includes(locale === "zh-CN" ? "正在添加 Mod" : "Adding mods"),
    "Background recognition uses the compact activity message");
  check(element<HTMLButtonElement>(".shell-activity-notice-actions button", activity).textContent?.trim() === (locale === "zh-CN" ? "取消" : "Cancel"),
    "Background recognition exposes cancellation in the activity bar");
  return session;
}
async function progress(session: Session, state: "loading" | "verification_required") {
  await act(async () => session.progress.onmessage(state));
}
async function verify(session: Session, selector: string) {
  const zone = element(selector);
  await progress(session, "verification_required");
  await settle(() => Boolean(fixture.querySelector('[data-curseforge-browser="embedded"]')) && Boolean(session.updates.at(-1)),
    "Verification progress did not reveal the native page in its original area");
  const embedded = element('[data-curseforge-browser="embedded"]', zone);
  check(fixture.querySelectorAll('[data-curseforge-browser="embedded"]').length === 1
    && embedded.closest(".mw-dropzone") === zone, "Exactly one browser stays inside the originating drop zone");
  const region = embedded.getBoundingClientRect(), original = zone.getBoundingClientRect();
  check(region.left >= original.left && region.top >= original.top
    && region.right <= original.right && region.bottom <= original.bottom, "Embedded toolbar and page are contained by the original drop area");
  check(element<HTMLButtonElement>("button", embedded).textContent?.trim() === (locale === "zh-CN" ? "取消" : "Cancel"),
    "Pending recognition exposes a cancel action in its own area");
  checkBounds(session.updates.at(-1)!);
}
function checkBounds(bounds: Bounds | null) {
  check(bounds, "Visible recognition page supplies non-null native bounds");
  const rect = element(".mw-reference-browser-page").getBoundingClientRect();
  verificationSlot ??= { x: rect.left, y: rect.top, width: rect.width, height: rect.height };
  check(devicePixelRatio === (capture ? 1 : 1.5), "Native bounds use the requested browser DPI");
  for (const [actual, css] of [[bounds.x, rect.left], [bounds.y, rect.top],
    [bounds.width, rect.width], [bounds.height, rect.height]]) {
    check(Number.isInteger(actual) && Math.abs(actual / devicePixelRatio - css) <= 1,
      "Native bounds match the visible DOM slot in physical client pixels");
  }
}
function result(): ManualModReferenceResolveResult {
  return { instance_id: stored.summary.id, module_id: "arksurvivalascended", source_label: "CurseForge",
    setting_key: "mod_ids_csv", setting_label: "Mod IDs", resolved_ids: ["1346144"],
    items: [{ reference: projectUrl, status: "resolved", resolved_id: "1346144", title: "DevKitLiveModTesting", icon_url: "/__mod_fixture_avatar.svg", message: null }] };
}
async function publish(session: Session) {
  if (permittedSave) cachedPresentation = true;
  await act(async () => { events.push("result:" + session.id); session.resolution.resolve(result()); });
}
async function checkPresentation(includeMissing = true) {
  await settle(() => [...fixture.querySelectorAll(".mw-entry-row")].some((row) =>
    row.querySelector(".mw-entry-row-title")?.textContent === "DevKitLiveModTesting"
      && row.querySelector<HTMLImageElement>("img")?.naturalWidth === 64), "Project name and decoded avatar did not reach My Mods");
  if (!includeMissing) return;
  const missing = [...fixture.querySelectorAll(".mw-entry-row")].find((row) => row.querySelector(".mw-entry-row-title")?.textContent === "Missing avatar example");
  check(missing, "A project name remains available when its avatar cannot load");
  await settle(() => Boolean(missing.querySelector(".mw-mod-thumb-placeholder")) && !missing.querySelector("img"), "Failed avatar did not fall back to the Mod placeholder");
}
async function absent(session: Session, previousSaves: number) {
  await settle(() => session.closed === 1 && !fixture.querySelector('[data-curseforge-browser="embedded"]'),
    "Cancelled host was not closed and removed");
  check(saves === previousSaves, "Cancelling recognition does not save Mod settings");
  await publish(session);
  check(saves === previousSaves && session.closed === 1, "A late native result cannot save after cancellation or close twice");
  await progress(session, "verification_required");
  check(!fixture.querySelector(".mw-reference-browser") && saves === previousSaves,
    "Late verification progress cannot reopen a cancelled or unmounted request");
}

async function run() {
  const fixtureSettings = await fetch("/__curseforge_reference_fixture").then((response) => response.json()) as {
    capture: CaptureMode | null; locale: "en-US" | "zh-CN"; pageCapture?: PageCaptureEvidence;
  };
  locale = fixtureSettings.locale; capture = fixtureSettings.capture;
  localStorage.setItem("langame.locale", locale);
  await act(prepareBrowserLocaleCatalogs);
  const template = await readInstanceDetails("srv-dst-terminal-error");
  stored = { ...template, summary: { ...template.summary, id: "fixture-embedded-curseforge", module_id: "arksurvivalascended",
    status: "Stopped", active_process_count: 0 }, active_run: null,
    settings_json: JSON.stringify({ mod_ids_csv: "101010", unrelated_setting: "keep" }) };
  inventory = { instance_id: stored.summary.id, module_id: stored.summary.module_id, source_label: "CurseForge",
    target_label: "Mods/83374", target_path: "C:\\Fixture\\ASA\\Mods\\83374", target_exists: true, items: [] };
  const base = createModWorkbenchFixtureIpc(() => ({ details: stored, moduleDetails, inventory, catalog: [], cached: new Set() }), {
    check, save: (saved) => { check(permittedSave, "A save requires an uncancelled successful native result"); stored = saved; events.push("save"); },
  });
  mockWindows("main");
  mockIPC(async (command, payload) => {
    const args = (payload ?? {}) as { bounds?: Bounds | null; sessionId?: string; browserSession?: string; instanceId?: string;
      references?: string[]; onProgress?: Channel<"loading" | "verification_required"> };
    if (command === "open_curseforge_browser_host") {
      check(args.onProgress instanceof Channel && /^__CHANNEL__:\d+$/.test(args.onProgress.toJSON()),
        "Host creation supplies a native-serializable Tauri progress channel");
      const session: Session = { id: "fixture-session-" + (sessions.length + 1), scenario, openBounds: args.bounds ?? null,
        progress: args.onProgress, updates: [], opening: deferred<string>(), resolution: deferred<ManualModReferenceResolveResult>(), closed: 0, resolved: false };
      sessions.push(session); events.push("open:" + session.id);
      return holdOpen ? session.opening.promise : session.id;
    }
    if (command === "update_curseforge_browser_host" || command === "close_curseforge_browser_host") {
      const session = sessions.find((entry) => entry.id === args.sessionId);
      check(session, "Host updates and cleanup retain their own session ID");
      if (command === "update_curseforge_browser_host") session.updates.push(args.bounds ?? null);
      else { session.closed += 1; events.push("close:" + session.id); }
      return;
    }
    if (command === "resolve_curseforge_browser_references") {
      const session = sessions.find((entry) => entry.id === args.browserSession);
      check(session && session.closed === 0, "Resolution receives the live browserSession returned by open");
      check(args.instanceId === stored.summary.id && args.references?.length === 1 && args.references[0] === projectUrl,
        "Resolution preserves the originating ASA instance and official project link");
      session.resolved = true; resolutions += 1; events.push("resolve:" + session.id);
      return session.resolution.promise;
    }
    if (command === "read_manual_mod_inventory") inventoryReads += 1;
    if (command === "resolve_manual_mod_references") {
      check(args.references?.every((id) => /^\d+$/.test(id)), "Presentation refresh only reads numeric IDs without opening websites");
      if (holdPresentationRead) return new Promise(() => {});
      return { ...result(), resolved_ids: args.references, items: args.references.map((id) => ({ reference: id,
        status: "resolved", resolved_id: id, title: cachedPresentation ? id === "1346144" ? "DevKitLiveModTesting" : "Missing avatar example" : null,
        icon_url: cachedPresentation ? id === "1346144" ? "/__mod_fixture_avatar.svg" : "/__mod_fixture_missing.png" : null })) };
    }
    return base(command, payload);
  }, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });

  await mount();
  if (capture) {
    const session = await begin("screenshot-" + capture, ".mw-manual-pane > .mw-dropzone");
    if (capture === "verification") {
      check(fixtureSettings.pageCapture, "An expanded preview requires a supplied real native page capture");
      await verify(session, ".mw-manual-pane > .mw-dropzone");
      const slot = element(".mw-reference-browser-page");
      const rect = slot.getBoundingClientRect();
      const page = new Image();
      page.alt = "真实 CurseForge 网页截图；验证展开状态由测试触发";
      page.dataset.nativePageCapture = "true";
      page.style.cssText = "position:absolute;inset:0;display:block;width:100%;height:100%;object-fit:fill;pointer-events:none";
      slot.style.position = "relative";
      slot.append(page);
      page.src = "/__curseforge_page_capture.png";
      await page.decode();
      check(page.naturalWidth === fixtureSettings.pageCapture.width && page.naturalHeight === fixtureSettings.pageCapture.height,
        "The preview displays the supplied native PNG without substitute page content");
      check(Math.abs(page.naturalWidth - rect.width) <= 1 && Math.abs(page.naturalHeight - rect.height) <= 1,
        "The native page capture matches the exact product slot viewport");
      const caption = document.createElement("aside");
      caption.textContent = "测试场景，网页为真实加载截图，未触发真实验证码";
      caption.dataset.captureDisclosure = "true";
      caption.style.cssText = "position:fixed;left:48px;top:21px;z-index:10;font:11px/16px sans-serif;color:#c4c4c4;pointer-events:none";
      document.body.append(caption);
      check(saves === 0 && session.closed === 0, "The verification preview retains its pending request without saving");
    } else if (capture === "success") {
      permittedSave = true;
      await publish(session);
      await settle(() => saves === 1 && session.closed === 1 && Boolean(fixture.querySelector(".shell-activity-notice.is-success")),
        "The success screenshot did not reach the real activity notice");
      check(element(".shell-activity-bar .shell-activity-notice.is-success").textContent?.includes("已识别 1 个 ID"),
        "The screenshot uses the actual Chinese recognition summary");
      check(!fixture.querySelector(".mw-manual-feedback .mw-chip--on"), "No permanent green recognition chip remains above the workbench");
      await click(button("我的 Mod"));
      await checkPresentation();
    }
    if (capture !== "verification") check(!fixture.querySelector(".mw-reference-browser"), "The photographed normal path never displays a webpage");
    await document.fonts.ready;
    await settle(() => {
      const slide = fixture.querySelector<HTMLElement>(".shell-activity-slide");
      return Boolean(slide && slide.getAnimations().every((animation) => animation.playState === "finished")
        && Number(getComputedStyle(slide).opacity) >= 0.99);
    }, "The activity notice entrance animation did not finish before capture");
    check(errors.length === 0, "Screenshot has no console errors");
    return { status: "passed", checks, screenshot_state: capture, locale,
      native_boundary: capture === "verification" ? "native-page-capture-preview" : "mocked",
      ...(capture === "verification" ? { verification_trigger: "simulated", page_capture: fixtureSettings.pageCapture,
        verification_slot: verificationSlot, disclosure: "测试场景，网页为真实加载截图，未触发真实验证码" } : {}), saves,
      sessions: sessions.map(({ scenario, closed, resolved }) => ({ scenario, closed, resolved })), browser_errors: errors };
  }
  const backgroundCancel = await begin("store-background-cancel", ".mw-manual-pane > .mw-dropzone");
  await click(element<HTMLButtonElement>(".shell-activity-notice-actions button"));
  await absent(backgroundCancel, 0);

  const cancel = await begin("store-verification-cancel", ".mw-manual-pane > .mw-dropzone");
  check(saves === 0, "A pending Store page does not write settings");
  await verify(cancel, ".mw-manual-pane > .mw-dropzone");
  const originalWidth = cancel.updates.at(-1)!.width;
  await act(async () => { fixture.style.width = "calc(100% - 140px)"; });
  await settle(() => cancel.updates.some((bounds) => bounds && bounds.width < originalWidth - 100), "Container resize did not update native bounds");
  checkBounds(cancel.updates.at(-1)!);
  const dialog = document.createElement("div");
  dialog.setAttribute("role", "dialog"); dialog.setAttribute("aria-modal", "true");
  dialog.style.cssText = "position:fixed;inset:40% 30%;z-index:100000;background:white";
  await act(async () => document.body.append(dialog));
  await settle(() => cancel.updates.at(-1) === null, "A visible HTML modal did not hide the native child");
  await act(async () => dialog.remove());
  await settle(() => Boolean(cancel.updates.at(-1)), "Closing the modal did not restore native bounds");
  const countBeforeNextLink = cancel.updates.length;
  await progress(cancel, "loading");
  await settle(() => !fixture.querySelector(".mw-reference-browser") && cancel.updates.length > countBeforeNextLink && cancel.updates.at(-1) === null,
    "Moving to the next batch link did not hide the prior verification page");
  await act(async () => { await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))); });
  check(cancel.updates.slice(countBeforeNextLink).every((bounds) => bounds === null),
    "Old verification bounds cannot reappear while the next page loads in the background");
  await verify(cancel, ".mw-manual-pane > .mw-dropzone");
  await click(element<HTMLButtonElement>(".mw-reference-browser button"));
  await absent(cancel, 0);
  await act(async () => { fixture.style.width = ""; });

  await click(button("My Mods"));
  const success = await begin("my-mods-success", ".mw-config-pane .mw-dropzone");
  check(saves === 0, "A pending My Mods page does not write settings");
  await verify(success, ".mw-config-pane .mw-dropzone");
  await progress(success, "loading");
  await settle(() => !fixture.querySelector(".mw-reference-browser") && success.updates.at(-1) === null,
    "The completed final page left an empty verification area while waiting for the runtime result");
  check(saves === 0 && success.closed === 0 && !events.includes("result:" + success.id)
    && Boolean(fixture.querySelector(".shell-activity-notice-actions button")),
    "After the final page closes, the pending result stays cancellable in the activity bar without saving");
  permittedSave = true;
  holdPresentationRead = true;
  await publish(success);
  await settle(() => saves === 1 && success.closed === 1 && !fixture.querySelector('[data-curseforge-browser="embedded"]'),
    "Successful recognition did not save and close its embedded host");
  const successNotice = element(".shell-activity-bar .shell-activity-notice.is-success");
  check(successNotice.textContent?.includes("1 ID(s) ready") && Boolean(successNotice.querySelector(".shell-activity-notice-close")),
    "Resolved ID feedback is a dismissible success notice in the activity bar");
  check(!fixture.querySelector(".mw-manual-feedback .mw-chip--on"), "Resolved ID feedback leaves no permanent green chip");
  const successShownAt = performance.now();
  await checkPresentation(false);
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 4200)); });
  check(successNotice.isConnected, "Success feedback remains available before its five-second expiry");
  await settle(() => !successNotice.isConnected && !fixture.querySelector(".shell-activity-notice.is-success"),
    "Success feedback did not disappear after five seconds", 2200);
  const successDuration = performance.now() - successShownAt;
  await checkPresentation(false);
  check(holdPresentationRead, "Imported name and avatar outlive the notice even while the optional cache read is pending");
  holdPresentationRead = false;
  check(successDuration >= 4800 && successDuration < 6500, "Success feedback expires after five real seconds without changing the saved settings");
  const settings = JSON.parse(stored.settings_json);
  check(String(settings.mod_ids_csv).includes("1346144") && String(settings.mod_ids_csv).includes("101010")
    && settings.unrelated_setting === "keep", "Successful recognition adds the resolved ID while preserving existing settings");
  check(events.indexOf("result:" + success.id) < events.indexOf("close:" + success.id)
    && events.indexOf("close:" + success.id) < events.indexOf("save"), "Native completion and host cleanup precede the settings save");
  permittedSave = false;

  await click(button("Store"));
  const navigation = await begin("navigation-cancel", ".mw-manual-pane > .mw-dropzone");
  await click(button("My Mods"));
  await absent(navigation, 1);
  await click(button("Store"));
  const unmount = await begin("unmount-cancel", ".mw-manual-pane > .mw-dropzone");
  await act(async () => root.render(null));
  await absent(unmount, 1);

  await mount();
  await click(button("My Mods"));
  await checkPresentation();
  check(sessions.length === 5 && saves === 1, "Reloading names and avatars opens no native website and performs no settings mutation");
  await click(button("Store"));
  const late = await begin("late-open", ".mw-manual-pane > .mw-dropzone", true);
  check(!late.resolved && resolutions === 5, "Resolution waits for the native host to open");
  await act(async () => root.render(null));
  await act(async () => late.opening.resolve(late.id));
  await settle(() => late.closed === 1, "A late open response was not cleaned up after unmount");
  await progress(late, "verification_required");
  check(!late.resolved && resolutions === 5 && saves === 1 && !fixture.querySelector(".mw-reference-browser"),
    "A disposed late-open request never continues to resolution, verification display or saving");
  check(errors.length === 0, "No console errors or unhandled rejections");
  return { status: "passed", checks, saves, resolutions, sessions: sessions.map(({ scenario, closed, resolved }) => ({ scenario, closed, resolved })),
    presentation_reloaded: true, missing_avatar_fallback: true,
    physical_bounds_checked: true, resize_checked: true, modal_occlusion_checked: true, hidden_by_default: true,
    batch_loading_hides_verification: true, completed_page_hidden_before_result: true,
    verification_slot: verificationSlot, success_auto_dismiss_ms: Math.round(successDuration), browser_errors: errors };
}

Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  await act(async () => root.unmount());
  check(sessions.every((session) => session.closed === 1), "All native hosts are closed once after fixture cleanup");
  return { browser_errors: errors, native_dialogs: 0 };
} });
void run().catch((error) => ({ status: "failed", error: String(error), checks, events, browser_errors: errors }))
  .finally(() => { Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch("/__reliability_result/" + new URLSearchParams(location.search).get("nonce"),
    { method: "POST", body: JSON.stringify(report) }));
