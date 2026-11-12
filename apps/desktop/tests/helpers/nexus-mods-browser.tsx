import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type { Channel } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { bridge } from "./runtime-browser-events";
import { readInstanceDetails } from "../../src/api";
import { invokeMock } from "../../src/api-mock";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import { ModWorkbench } from "../../src/views/servers/ModWorkbench";
import { I18nProvider } from "../../src/i18n";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { ActivityBar } from "../../src/components/ActivityBar";
import { createActivityNoticeStore } from "../../src/components/activity-notice-store";
import { InstanceSettingsSaveProvider, useInstanceSettingsSaveCoordinator } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceDetails, ManualModStageResult } from "../../src/types";
import type { InstanceArchiveDetails } from "../../src/storage-management-types";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "en-US"); document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!, root = createRoot(fixture);
fixture.style.cssText = "height:100vh;padding:24px;box-sizing:border-box;display:grid;grid-template-rows:minmax(0,1fr) auto";
const checks: string[] = [], errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const consoleError = console.error; console.error = (...args) => { errors.push(args.map(String).join(" ")); consoleError(...args); };
let details: InstanceDetails, epoch = 0, archived = false, installed = false, websiteInstalled = false;
let localImports = 0, inventoryReads = 0, websiteInstalls = 0;
const openedLinks: string[] = [], commands: string[] = [];
const openedSessions: string[] = [], closedSessions: string[] = [], websiteReferences: string[] = [], presentationStages: string[] = [];
const websiteReference = "https://www.nexusmods.com/sevendaystodie/mods/9559?tab=files";
const presentationTitle = "Nexus server Mod with artwork", presentationName = "nexus-sevendaystodie-9559";
// The browser fixture substitutes image delivery, while official-CDN validation is tested in official-media-fallback.
const presentationIcon = "/__nexus_mod_avatar.svg";
const openingCopy = "Preparing the Nexus download automatically…";
const phaseCopy = {
  opening: openingCopy,
  selecting: "Finding and preparing the Nexus file…",
  waiting: "Waiting for the Nexus download countdown…",
  verifying: "Waiting for Nexus automatic verification…",
  installing: "Downloading and installing the Nexus file…",
} as const;
const observedPhases = new Set<keyof typeof phaseCopy>(), blockedErrors: string[] = [];
const legacyApiCommands = new Set(["nexus_account_status", "nexus_store_api_key", "nexus_clear_api_key",
  "review_nexus_mod_references", "install_nexus_mod_files"]);
const progressEvent = "nexus-website-progress";
type WebsiteRequest = { sessionId: string; instanceId: string; reference: string; onProgress: Channel<string> };
type Transfer = { instanceId: string; operationId: string; attempt: number; sequence: number; phase: string;
  receivedBytes: number; totalBytes: number | null; elapsedSeconds: number };
type Bounds = { x: number; y: number; width: number; height: number };
const loginSelector = '[data-nexus-browser="login"]';
const loginSessions: string[] = [], closedLogins: string[] = [];
const loginBounds = new Map<string, (Bounds | null)[]>();
let activeLogin: string | null = null, expectedLogin = false, loginAttempts = 0;
type AuthState = "unknown" | "signed_out" | "signed_in";
let nativeAuthState: AuthState = "unknown", authReads = 0, delayLoginClose = false;
const loginAuthChannels = new Map<string, Channel<string>>();
let pendingLoginClose: { sessionId: string; resolve: () => void } | null = null;
let delayLoginOpen = false, failLoginOpen = false, pendingLoginOpen: { sessionId: string; resolve: (id: string) => void } | null = null;
const embeddedSelector = '[data-nexus-browser], .mw-reference-browser, iframe, dialog[open], [role="dialog"], [role="alertdialog"]';
let embeddedMounts = 0;
const pageObserver = new MutationObserver((records) => {
  for (const record of records) for (const node of record.addedNodes) {
    if (!expectedLogin && node instanceof Element && (node.matches(embeddedSelector) || node.querySelector(embeddedSelector))) embeddedMounts++;
  }
});
pageObserver.observe(fixture, { childList: true, subtree: true });
let pendingWebsite: { request: WebsiteRequest; resolve: (result: ManualModStageResult) => void; reject: (reason: unknown) => void } | null = null;
let delayNextOpen = false, pendingOpen: { sessionId: string; resolve: (id: string) => void } | null = null;
let expectedReference = websiteReference;
let coordinator: ReturnType<typeof useInstanceSettingsSaveCoordinator>, cancelledFlushes = 0;
const moduleDetails = buildMockModuleDetails("sevendaystodie");
const successSelector = ".shell-activity-bar .shell-activity-notice.is-success";
function check(condition: unknown, label: string): asserts condition { if (!condition) throw new Error(`${label}: ${fixture.textContent}`); checks.push(label); }
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const node = document.querySelector<T>(selector); check(node, `Found ${selector}`); return node;
}
async function settle(predicate: () => boolean, label: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) { if (performance.now() > deadline) throw new Error(`${label}: ${fixture.textContent}`); await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); }); }
}
async function click(selector: string) {
  const node = element(selector); node.scrollIntoView({ block: "nearest" });
  check(!(node instanceof HTMLButtonElement || node instanceof HTMLInputElement) || !node.disabled, `${selector} is enabled`);
  await act(async () => { node.focus(); node.click(); });
}
async function tab(label: string) {
  const node = [...fixture.querySelectorAll<HTMLButtonElement>('[role="tab"]')].find((item) => item.textContent === label);
  check(node, `Found ${label} workspace tab`); await act(async () => node.click());
}
function inventory() { const originalInstance = details.summary.id === "fixture-nexus"; return { instance_id: details.summary.id, module_id: "sevendaystodie", source_label: "Nexus Mods", target_label: "Mods",
  target_path: "fixture://nexus/Mods", target_exists: originalInstance && (installed || websiteInstalled), items: [
    ...(originalInstance && installed ? [{ name: "FixtureNexusMod", path: "fixture://nexus/Mods/FixtureNexusMod",
      item_type: "directory", inferred_id: null, file_count: 2, total_bytes: 4096 }] : []),
    ...(originalInstance && websiteInstalled ? [{ name: presentationName, path: `fixture://nexus/Mods/${presentationName}`,
      item_type: "directory", inferred_id: null, file_count: 2, total_bytes: 4096,
      display_name: presentationTitle, icon_url: presentationIcon }] : [])] }; }
function stage(rootName = "FixtureNexusMod"): ManualModStageResult { return { instance_id: details.summary.id, module_id: "sevendaystodie", source_label: "Nexus Mods", target_label: "Mods",
  target_path: "fixture://nexus/Mods", affected_root_names: [rootName], items: [], copied_file_count: 2, copied_total_bytes: 4096 }; }
function Harness() {
  coordinator = useInstanceSettingsSaveCoordinator();
  const [noticeStore] = useState(createActivityNoticeStore);
  const archive: InstanceArchiveDetails | undefined = archived ? { archive_id: "fixture-nexus-archive", instance: details,
    maintenance: { autostart: false, auto_backup_on_stop: false, backup_retention_count: 3, crash_restart_limit: null, runtime_mode: null },
    runs: { entries: [], total: 0, truncated: false }, log: { relative_path: null, text: "", truncated: false, issues: [] }, backups: { entries: [], issues: [], truncated: false } } : undefined;
  return <ActivityNoticeTarget.Provider value={{ element: null, store: noticeStore, dismissLabel: "Close" }}><div style={{ minHeight: 0 }}>
    <ModWorkbench key={details.summary.id} details={details} moduleDetails={moduleDetails} archive={archive} launchPlan={null}
      onSaveSettings={async () => { throw new Error("Nexus references and credentials must never be saved as server settings"); }} />
  </div><footer className="shell-activity-bar"><span className="shell-activity-label">Activity</span><ActivityBar store={noticeStore} entries={[]} /></footer></ActivityNoticeTarget.Provider>;
}
async function renderInstance() {
  await act(async () => root.render(<React.StrictMode><I18nProvider><InstanceSettingsSaveProvider key={epoch}><Harness /></InstanceSettingsSaveProvider></I18nProvider></React.StrictMode>));
  await settle(() => Boolean(fixture.querySelector(".mw-source-open-btn")), "Nexus source entry did not mount");
}
async function mount(options: { running?: boolean; archived?: boolean } = {}) {
  archived = Boolean(options.archived); epoch++;
  details = { ...details, summary: { ...details.summary, status: options.running ? "Running" : "Stopped", active_process_count: options.running ? 1 : 0 } };
  await renderInstance();
}
function header() {
  const desktop = Boolean((window as Window & { isTauri?: boolean }).isTauri);
  check(!fixture.querySelector("[data-nexus-auth-open],[data-nexus-credentials],[data-nexus-file-review]"),
    "Nexus exposes no API credentials or in-app website file picker");
  check(Boolean(fixture.querySelector(".mw-reference-row")) === desktop,
    desktop ? "Desktop accepts a Nexus project link or ID" : "Web access offers local-file imports without a website reference control");
  check(!fixture.textContent?.includes("API Key") && !fixture.textContent?.includes("Nexus authorization"),
    "Unavailable authorization is absent from the product copy");
  const store = [...fixture.querySelectorAll('[role="tab"]')].some((item) => item.textContent === "Store" && item.getAttribute("aria-selected") === "true");
  check(Boolean(fixture.querySelector("[data-nexus-login-open]")) === (desktop && store),
    "Only the local desktop Store offers the explicit Nexus sign-in entry");
  const source = element(".mw-source-open-btn").getBoundingClientRect();
  check(source.width > 0 && source.height > 0 && source.left >= 0 && source.right <= innerWidth,
    "Nexus website action is reachable within the viewport");
  check(document.documentElement.scrollWidth <= innerWidth + 1, "Nexus workspace has no horizontal overflow");
  check(!fixture.querySelector(".mw-nexus-local-note"), "Nexus import instructions do not occupy a permanent footer row");
  check(!element(".mw-source-open-btn").hasAttribute("aria-describedby")
    && !element(".mw-source-open-btn").hasAttribute("data-configuration-help-anchor"),
    "The Nexus website action has no long hover or focus instructions");
}
async function dragHint(target = element(".mw-dropzone"), localFile = false) {
  const transfer = new DataTransfer();
  if (localFile) transfer.items.add(new File(["isolated package"], "fixture.zip"));
  else transfer.setData("text/plain", websiteReference);
  Object.defineProperty(transfer, "effectAllowed", { value: localFile ? "copy" : "link" });
  await act(async () => {
    target.dispatchEvent(new DragEvent("dragenter", { bubbles: true, cancelable: true, dataTransfer: transfer }));
    target.dispatchEvent(new DragEvent("dragover", { bubbles: true, cancelable: true, dataTransfer: transfer }));
  });
  const message = localFile ? "Release to import Mod files." : "Release to start the Nexus download.";
  await settle(() => fixture.querySelector(".shell-activity-content .shell-activity-notice-text")?.textContent === message,
    "The real activity bar did not show the concise Nexus drag hint");
  check(target.classList.contains("mw-dropzone--active") && !fixture.querySelector(".mw-dropzone-feedback"),
    "Nexus dragging keeps the drop outline and puts instructions only in the activity bar");
  check(!document.querySelector(".configuration-field-help-tooltip.is-visible")
    && !fixture.querySelector(".shell-activity-panel"),
    "A Nexus drag does not open a help bubble or message panel");
}
async function endDrag(target = element(".mw-dropzone")) {
  await act(async () => target.dispatchEvent(new DragEvent("dragleave", { bubbles: true, relatedTarget: document.body })));
  await settle(() => !fixture.querySelector(".shell-activity-notice"), "Leaving the drop area did not retire its transient activity hint");
}
async function sourceGuidance() {
  const before = commands.length, source = element(".mw-source-open-btn");
  const rect = source.getBoundingClientRect();
  await act(async () => {
    const result = await fetch(`/__reliability_pointer/${new URLSearchParams(location.search).get("nonce")}`, {
      method: "POST", body: JSON.stringify({ type: "move", x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }),
    });
    check(result.ok, "Real browser pointer reaches the Nexus website button");
    await new Promise((resolve) => setTimeout(resolve, 320));
  });
  check(!document.querySelector(".configuration-field-help-tooltip.is-visible"),
    "First pointer hover past the help delay does not reveal a long Nexus explanation");
  await act(async () => { source.focus(); await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))); });
  check(!document.querySelector(".configuration-field-help-tooltip.is-visible"),
    "Keyboard focus keeps the Nexus website button free of instructional bubbles");
  await dragHint(); await endDrag();
  await dragHint(element(".mw-dropzone"), true); await endDrag();
  await tab("My Mods"); await dragHint(element(".mw-config-pane .mw-dropzone")); await endDrag(element(".mw-config-pane .mw-dropzone"));
  await tab("Store");
  check(commands.slice(before).every((command) => ["read_manual_mod_inventory", "read_background_jobs", "read_nexus_website_auth_state"].includes(command)),
    "Hover, focus and drag guidance never open a website session or change files");
}
async function dropFile(target = element(".mw-dropzone")) {
  const file = new File(["isolated package"], "fixture.zip");
  Object.defineProperty(file, "path", { value: "fixture://nexus/package.zip" });
  const transfer = new DataTransfer(); transfer.items.add(file);
  Object.defineProperty(transfer, "effectAllowed", { value: "copy" });
  await act(async () => target.dispatchEvent(new DragEvent("drop", {
    bubbles: true, cancelable: true, dataTransfer: transfer })));
}
async function dropReference(reference: string, target = element(".mw-dropzone")) {
  const transfer = new DataTransfer(); transfer.setData("text/plain", reference);
  Object.defineProperty(transfer, "effectAllowed", { value: "link" });
  await act(async () => target.dispatchEvent(new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: transfer })));
}
function cancelButton() {
  return [...fixture.querySelectorAll<HTMLButtonElement>(".shell-activity-bar button")]
    .find((button) => button.textContent === "Cancel");
}
async function verifyNoWebpage() {
  await act(async () => { await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))); });
  check(embeddedMounts === 0 && !document.querySelector(embeddedSelector),
    "Nexus never creates a webpage, embedded slot or HTML popup");
  check(!commands.includes("update_nexus_website_host"), "Background download never sends native webpage display bounds");
  check(!activeLogin && !pendingLoginOpen, "Background downloads never open a Nexus login session");
  const login = fixture.querySelector<HTMLButtonElement>("[data-nexus-login-open]");
  if (pendingWebsite && login) check(login.disabled, "An active background download disables the sign-in action");
  const activity = fixture.querySelector(".shell-activity-bar");
  check(!activity?.querySelector('[role="progressbar"][aria-valuenow]') && !/\d+\s*%/.test(activity?.textContent ?? ""),
    "Nexus activity reports actual stages without fabricated download percentages");
}
async function progress(request: WebsiteRequest, phase: keyof typeof phaseCopy) {
  await act(async () => request.onProgress.onmessage(phase));
  await settle(() => fixture.querySelector(".shell-activity-bar")?.textContent?.includes(phaseCopy[phase]) === true,
    `Native ${phase} did not display its actual local activity stage`);
  observedPhases.add(phase);
  check(Boolean(cancelButton()) === (phase !== "installing"),
    "Preparation remains cancellable while runtime-owned installation cannot be cancelled");
  if (phase === "waiting") {
    const target = element(".mw-dropzone"), transfer = new DataTransfer();
    transfer.setData("text/plain", websiteReference);
    Object.defineProperty(transfer, "effectAllowed", { value: "link" });
    await act(async () => target.dispatchEvent(new DragEvent("dragover", { bubbles: true, cancelable: true, dataTransfer: transfer })));
    check(element(".shell-activity-bar").textContent?.includes(phaseCopy.waiting)
      && !fixture.textContent?.includes("Release to start the Nexus download."),
      "Dragging during an existing download cannot replace its actual activity stage with a hint");
    await act(async () => target.dispatchEvent(new DragEvent("dragleave", { bubbles: true, relatedTarget: document.body })));
  }
  await verifyNoWebpage();
}
async function byteProgress(request: WebsiteRequest, update: Partial<Transfer>) {
  await act(async () => emit(progressEvent, { instanceId: request.instanceId, operationId: request.sessionId,
    attempt: 1, sequence: 1, phase: "downloading", receivedBytes: 0, totalBytes: 4096, elapsedSeconds: 0, ...update }));
}
async function beginWebsite(reference = websiteReference, input = false, target = element(".mw-dropzone")) {
  const installsBefore = websiteInstalls, beforeMounts = embeddedMounts, sessionsBefore = openedSessions.length;
  expectedReference = reference;
  if (input) {
    const field = element<HTMLInputElement>(".mw-reference-input");
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(field, reference);
      field.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true })));
  } else await dropReference(reference, target);
  await settle(() => Boolean(pendingWebsite && cancelButton()), "Nexus background session did not start loading");
  check(openedSessions.length === sessionsBefore + 1, "A StrictMode user action reserves exactly one background session");
  check(element(".shell-activity-bar").textContent?.includes(openingCopy),
    "Automatic preparation uses its compact local activity message");
  observedPhases.add("opening");
  await verifyNoWebpage();
  check(!fixture.querySelector(successSelector) && websiteInstalls === installsBefore,
    "Opening a background session does not report an installation");
  return beforeMounts;
}
async function verifyClosed(sessionId: string, nativeProgress?: Channel<string>) {
  await settle(() => closedSessions.includes(sessionId), "A cancelled Nexus session did not close");
  if (nativeProgress) await act(async () => {
    nativeProgress.onmessage("verifying");
    nativeProgress.onmessage("browsing");
  });
  await verifyNoWebpage();
  check(!cancelButton(), "Late progress cannot revive a cancelled background operation");
}
async function verifyCancelledFlush() {
  await act(async () => { await coordinator.flush(details.summary.id); });
  cancelledFlushes++;
  checks.push("Normal Nexus cancellation does not leave the real settings coordinator blocking a later start");
}
async function verifyPresentation(stageName: string) {
  const row = () => [...fixture.querySelectorAll<HTMLElement>(".mw-entry-row")]
    .find((node) => node.querySelector(".mw-entry-row-title")?.textContent === presentationTitle);
  await settle(() => Boolean(row()?.querySelector<HTMLImageElement>(".mw-entry-row-media img")?.complete),
    `${stageName}: website Mod avatar finishes loading`);
  const image = row()!.querySelector<HTMLImageElement>(".mw-entry-row-media img")!;
  await act(async () => { await image.decode(); });
  check(image.naturalWidth === 32 && image.naturalHeight === 32 && new URL(image.currentSrc).pathname === presentationIcon,
    `${stageName}: browser decodes the website Mod avatar`);
  check(![...fixture.querySelectorAll(".mw-entry-row-title")].some((node) => node.textContent === presentationName),
    `${stageName}: the website title replaces its technical directory name`);
  presentationStages.push(stageName);
}
async function dispatch(command: string, payload: unknown) {
  commands.push(command);
  check(!legacyApiCommands.has(command), "No Nexus account, API key or API download command is issued");
  switch (command) {
    case "read_manual_mod_inventory": inventoryReads++; return inventory();
    case "read_background_jobs": return [];
    case "read_nexus_website_auth_state":
      check((window as Window & { isTauri?: boolean }).isTauri, "Only the desktop reads the native authentication observation cache");
      authReads++; return nativeAuthState;
    case "read_instance_details_from_storage": return details;
    case "open_nexus_website_host": {
      check(!activeLogin, "Background downloads and sign-in cannot reserve native hosts together");
      check(payload === undefined || (payload !== null && typeof payload === "object" && Object.keys(payload).length === 0),
        "Nexus reserves its background host without native display arguments");
      const sessionId = `fixture-nexus-session-${openedSessions.length + 1}`; openedSessions.push(sessionId);
      if (!delayNextOpen) return sessionId;
      delayNextOpen = false;
      return new Promise<string>((resolve) => { pendingOpen = { sessionId, resolve }; });
    }
    case "open_nexus_login_host": {
      loginAttempts++;
      check(expectedLogin && !pendingWebsite && !activeLogin,
        "Only an explicit sign-in action opens a native login host, mutually exclusive with download");
      const args = payload as { bounds: null; onAuthChange: Channel<string> };
      check(args.bounds === null && typeof args.onAuthChange?.onmessage === "function"
        && Object.keys(args).sort().join(",") === "bounds,onAuthChange",
        "Sign-in reserves a hidden native host with only a status channel and no URL or credentials");
      if (failLoginOpen) { failLoginOpen = false; throw { code: "nexus_browser_unavailable" }; }
      const sessionId = `fixture-nexus-login-${loginSessions.length + 1}`;
      loginSessions.push(sessionId); activeLogin = sessionId; loginBounds.set(sessionId, [null]);
      loginAuthChannels.set(sessionId, args.onAuthChange);
      args.onAuthChange.onmessage("unknown");
      if (!delayLoginOpen) return sessionId;
      delayLoginOpen = false;
      return new Promise<string>((resolve) => { pendingLoginOpen = { sessionId, resolve }; });
    }
    case "update_nexus_login_host": {
      const { sessionId, bounds } = payload as { sessionId: string; bounds: Bounds | null };
      check(activeLogin === sessionId && loginBounds.has(sessionId) && !pendingWebsite,
        "Login bounds only affect the currently reserved login host");
      check(bounds === null || [bounds.x, bounds.y, bounds.width, bounds.height].every(Number.isFinite),
        "Login display updates contain finite physical coordinates or hide the view");
      loginBounds.get(sessionId)!.push(bounds); return;
    }
    case "close_nexus_login_host": {
      const { sessionId } = payload as { sessionId: string };
      check(loginSessions.includes(sessionId), "Only the reserved login host is closed");
      const complete = () => { if (activeLogin === sessionId) activeLogin = null; closedLogins.push(sessionId); };
      if (!delayLoginClose) { complete(); return; }
      delayLoginClose = false;
      return new Promise<void>((resolve) => { pendingLoginClose = { sessionId, resolve: () => { complete(); resolve(); } }; });
    }
    case "install_nexus_website_mod": {
      const request = payload as WebsiteRequest;
      check(request.sessionId === openedSessions.at(-1) && request.instanceId === details.summary.id
        && request.reference === expectedReference && typeof request.onProgress.onmessage === "function",
        "Website installation uses the reserved session, instance and original project reference");
      check(!pendingWebsite, "Only one Nexus project is active at a time");
      websiteReferences.push(request.reference);
      return new Promise<ManualModStageResult>((resolve, reject) => {
        pendingWebsite = { request, resolve, reject };
      });
    }
    case "close_nexus_website_host": {
      const { sessionId } = payload as { sessionId: string };
      check(openedSessions.includes(sessionId), "Only a reserved Nexus website session is closed");
      closedSessions.push(sessionId);
      if (pendingWebsite?.request.sessionId === sessionId) {
        const pending = pendingWebsite; pendingWebsite = null; pending.reject({ code: "nexus_browser_cancelled" });
      }
      return;
    }
    case "open_external_url": {
      const url = (payload as { url: string }).url;
      check(url === moduleDetails.mods?.source?.url, "System browser receives the declared Nexus game website");
      openedLinks.push(url); return;
    }
    case "stage_manual_mod_files": {
      const args = payload as { instanceId: string; sourcePaths: string[] };
      check(args.instanceId === details.summary.id && args.sourcePaths.length === 1
        && args.sourcePaths[0] === "fixture://nexus/package.zip", "Local import uses this instance and the dropped file");
      localImports++; installed = true; return stage();
    }
    default: throw new Error(`Unexpected Nexus fixture command: ${command}`);
  }
}
async function websiteRegression() {
  await mount(); header();
  const initialInventory = JSON.stringify(inventory());
  const beforeCancel = await beginWebsite();
  const loadingCancelled = pendingWebsite!;
  await act(async () => cancelButton()!.click());
  await verifyClosed(loadingCancelled.request.sessionId, loadingCancelled.request.onProgress);
  await verifyCancelledFlush();
  check(embeddedMounts === beforeCancel && JSON.stringify(inventory()) === initialInventory,
    "Cancelling during loading never flashes a webpage or installs files");

  const beforeFailure = await beginWebsite();
  const loadingFailed = pendingWebsite!;
  const preparationReason = "The selected instance runtime is not available for Nexus installation.";
  await act(async () => { pendingWebsite = null; loadingFailed.reject({ code: "nexus_preparation_failed", message: preparationReason }); });
  await settle(() => fixture.querySelector(".shell-activity-notice.is-error")?.textContent?.includes(preparationReason) === true,
    "A preparation failure did not show its actual actionable reason");
  await verifyClosed(loadingFailed.request.sessionId, loadingFailed.request.onProgress);
  check(embeddedMounts === beforeFailure && JSON.stringify(inventory()) === initialInventory && !fixture.querySelector(successSelector),
    "A native preparation failure never flashes a webpage or reports installation");

  const blockedCases = [
    ["nexus_website_login_required", "Nexus requires sign-in", "selecting"],
    ["nexus_website_verification_required", "Nexus website verification did not finish", "verifying"],
    ["nexus_file_selection_required", "The Nexus file could not be selected unambiguously", "selecting"],
    ["nexus_website_action_required", "The Nexus website requires a manual action", "waiting"],
    ["nexus_website_controls_unavailable", "Download controls could not be identified", "selecting"],
    ["nexus_website_file_quarantined", "Nexus has quarantined this file", "selecting"],
    ["nexus_browser_page_unavailable", "The Nexus project page could not load or be recognized", "opening"],
    ["nexus_browser_timed_out", "The Nexus background download timed out without installing", "waiting"],
  ] as const;
  for (const [code, reason, phase] of blockedCases) {
    await mount(); await beginWebsite();
    const blocked = pendingWebsite!;
    await progress(blocked.request, phase);
    await act(async () => { pendingWebsite = null; blocked.reject({ code, message: "synthetic-private-download-details" }); });
    await settle(() => fixture.querySelector(".shell-activity-notice.is-error")?.textContent?.includes(reason) === true,
      `${code} did not explain why background installation did not complete`);
    await verifyClosed(blocked.request.sessionId, blocked.request.onProgress);
    check(fixture.querySelector(".shell-activity-content .shell-activity-notice.is-error")?.textContent?.includes(reason)
      && !document.querySelector(".configuration-field-help-tooltip.is-visible")
      && !fixture.querySelector(".mw-workbench .shell-activity-notice"),
      `${code} remains specific and appears only in the real activity bar`);
    check(!fixture.textContent?.includes("synthetic-private-download-details") && !fixture.querySelector(successSelector)
      && JSON.stringify(inventory()) === initialInventory && websiteInstalls === 0,
      `${code} exposes no private native details, installed files or success feedback`);
    let barrierError: unknown;
    await act(async () => { try { await coordinator.flush(details.summary.id); } catch (error) { barrierError = error; } });
    check(barrierError instanceof Error && barrierError.message === code,
      `${code} remains a real failed operation at the settings coordinator barrier`);
    blockedErrors.push(code);
  }

  await mount(); await beginWebsite();
  const cancelledByNative = pendingWebsite!;
  await progress(cancelledByNative.request, "verifying");
  await act(async () => { pendingWebsite = null; cancelledByNative.reject({ code: "nexus_browser_cancelled" }); });
  await settle(() => !cancelButton() && closedSessions.includes(cancelledByNative.request.sessionId),
    "Native cancellation did not finish its background session");
  await verifyClosed(cancelledByNative.request.sessionId, cancelledByNative.request.onProgress);
  await verifyCancelledFlush();
  check(!fixture.querySelector(successSelector) && JSON.stringify(inventory()) === initialInventory && websiteInstalls === 0,
    "Native cancellation leaves inventory unchanged and reports no installation success");

  await beginWebsite("9559", true);
  const cancelled = pendingWebsite!, cancelledSession = cancelled.request.sessionId;
  await act(async () => cancelButton()!.click());
  await settle(() => !pendingWebsite && !cancelButton() && closedSessions.includes(cancelledSession),
    "Cancel did not close the background session");
  await verifyClosed(cancelledSession, cancelled.request.onProgress);
  await verifyCancelledFlush();
  check(!fixture.querySelector(successSelector) && JSON.stringify(inventory()) === initialInventory && websiteInstalls === 0,
    "Cancelling an ID-based background import preserves inventory without success feedback");

  await beginWebsite();
  const navigation = pendingWebsite!, closesBeforeNavigation = closedSessions.length;
  await progress(navigation.request, "selecting");
  await tab("My Mods");
  check(pendingWebsite === navigation && closedSessions.length === closesBeforeNavigation
    && element(".shell-activity-bar").textContent?.includes(phaseCopy.selecting),
    "Switching to My Mods preserves preparation and its actual stage in the same instance");
  await progress(navigation.request, "waiting"); await tab("Store");
  check(pendingWebsite === navigation && closedSessions.length === closesBeforeNavigation
    && element(".shell-activity-bar").textContent?.includes(phaseCopy.waiting),
    "Returning to Store keeps the website countdown running in the background");
  await tab("My Mods"); await act(async () => cancelButton()!.click());
  await verifyClosed(navigation.request.sessionId, navigation.request.onProgress);
  await verifyCancelledFlush();
  check(!fixture.querySelector(successSelector) && JSON.stringify(inventory()) === initialInventory && websiteInstalls === 0,
    "Cancelling after a same-instance tab change closes only the pending preparation");

  for (const [reference, input] of [[websiteReference, false], ["9559", true]] as const) {
    await beginWebsite(reference, input, element(".mw-config-pane .mw-dropzone"));
    const myMods = pendingWebsite!;
    await progress(myMods.request, "selecting");
    await act(async () => cancelButton()!.click());
    await verifyClosed(myMods.request.sessionId, myMods.request.onProgress);
    await verifyCancelledFlush();
    check(!fixture.querySelector(successSelector) && JSON.stringify(inventory()) === initialInventory && websiteInstalls === 0,
      input ? "My Mods ID entry stays in the background and cancels without installation"
        : "My Mods link drop stays in the background and cancels without installation");
  }

  await tab("Store"); await beginWebsite();
  const unmounted = pendingWebsite!;
  await act(async () => root.render(null));
  await verifyClosed(unmounted.request.sessionId, unmounted.request.onProgress);
  check(JSON.stringify(inventory()) === initialInventory && websiteInstalls === 0,
    "Unmounting the workbench closes preparation without installing");
  await mount(); await beginWebsite();
  const previousDetails = details, previousCoordinator = coordinator, switched = pendingWebsite!;
  await progress(switched.request, "waiting");
  details = { ...details, summary: { ...details.summary, id: "fixture-nexus-other", name: "Other Nexus instance" } };
  await renderInstance();
  await verifyClosed(switched.request.sessionId, switched.request.onProgress);
  await act(async () => { await previousCoordinator.flush(previousDetails.summary.id); });
  check(!fixture.querySelector(".shell-activity-notice") && !fixture.textContent?.includes(presentationTitle),
    "Changing instances cancels old preparation without leaking its stage, result or title into the new instance");
  details = previousDetails; await renderInstance();

  const beforeMultiple = openedSessions.length;
  await dropReference(`${websiteReference}\nhttps://www.nexusmods.com/sevendaystodie/mods/9560`);
  await settle(() => fixture.textContent?.includes("Drop one Nexus project page at a time") === true,
    "Multiple projects did not receive actionable feedback");
  check(openedSessions.length === beforeMultiple && websiteInstalls === 0, "A multi-project drop opens no website session");
  let referenceFailure: unknown;
  await act(async () => { try { await coordinator.flush(details.summary.id); } catch (error) { referenceFailure = error; } });
  check(referenceFailure instanceof Error && referenceFailure.message === "nexus_one_project_required",
    "A genuine Nexus reference failure still rejects the settings coordinator start barrier");

  await mount(); delayNextOpen = true; expectedReference = websiteReference;
  const attemptsBeforeUnmount = websiteReferences.length, beforeLateOpen = embeddedMounts;
  await dropReference(websiteReference);
  await settle(() => Boolean(pendingOpen), "The delayed native background reservation did not begin");
  const reservation = pendingOpen!;
  await verifyNoWebpage();
  check(cancelButton() && element(".shell-activity-bar").textContent?.includes(openingCopy),
    "A delayed background reservation stays cancellable and reports preparation locally");
  await mount();
  await act(async () => { pendingOpen = null; reservation.resolve(reservation.sessionId); });
  await settle(() => closedSessions.includes(reservation.sessionId), "An unmounted request did not close its late reserved session");
  check(websiteReferences.length === attemptsBeforeUnmount && !fixture.querySelector(successSelector),
    "Unmounting during host creation closes the returned session without starting installation");
  await verifyClosed(reservation.sessionId);
  check(embeddedMounts === beforeLateOpen, "A late open response cannot flash a webpage after unmount");

  await beginWebsite();
  const accepted = pendingWebsite!;
  for (const phase of ["selecting", "waiting", "verifying", "waiting", "installing"] as const) await progress(accepted.request, phase);
  check(!cancelButton() && !fixture.querySelector(successSelector), "An accepted download has no cancellation action or premature success");
  for (const update of [{ operationId: openedSessions[0] }, { instanceId: "unrelated-instance" }, { receivedBytes: -1 }]) {
    await byteProgress(accepted.request, update);
    check(element(".shell-activity-bar").textContent?.includes(phaseCopy.installing),
      "Wrong sessions, other instances and malformed byte events cannot replace the current activity");
  }
  await byteProgress(accepted.request, {});
  check(element(".shell-activity-bar").textContent?.includes("0 KB / 4 KB") && !cancelButton(),
    "Runtime byte observation shows actual totals while keeping the installation ownership barrier");
  await byteProgress(accepted.request, { sequence: 2, receivedBytes: 1024, elapsedSeconds: 1 });
  check(element(".shell-activity-bar").textContent?.includes("1 KB / 4 KB")
    && element(".shell-activity-bar").textContent?.includes("1 KB/s"),
    "The existing download metrics compute speed from increasing native byte observations");
  const beforeOld = element(".shell-activity-bar").textContent;
  await byteProgress(accepted.request, { sequence: 1, receivedBytes: 4096, elapsedSeconds: 2 });
  check(element(".shell-activity-bar").textContent === beforeOld, "A late event cannot roll progress forward or backward with an old sequence");
  await byteProgress(accepted.request, { attempt: 2, sequence: 3, totalBytes: null, elapsedSeconds: 2 });
  check(element(".shell-activity-bar").textContent?.includes("0 KB · —")
    && !element(".shell-activity-bar").textContent?.includes(" / 4 KB"),
    "A retry resets speed sampling and displays unknown totals without inventing a denominator");
  await byteProgress(accepted.request, { attempt: 1, sequence: 4, receivedBytes: 4096, elapsedSeconds: 3 });
  check(element(".shell-activity-bar").textContent?.includes("0 KB · —"), "A previous HTTP attempt stays isolated even with a newer event sequence");
  await byteProgress(accepted.request, { attempt: 2, sequence: 4, receivedBytes: 1024, totalBytes: null, elapsedSeconds: 3 });
  check(element(".shell-activity-bar").textContent?.includes("1 KB · 1 KB/s"),
    "Unknown-length transfers retain real bytes and a fresh per-attempt speed");
  await byteProgress(accepted.request, { attempt: 2, sequence: 5, receivedBytes: 4096, elapsedSeconds: 4 });
  check(element(".shell-activity-bar").textContent?.includes("4 KB / 4 KB") && !fixture.querySelector(successSelector) && !cancelButton(),
    "Receiving all advertised bytes neither reports installed files nor re-enables cancellation");
  const closesBeforeInstallingTab = closedSessions.length;
  await tab("My Mods");
  check(closedSessions.length === closesBeforeInstallingTab && pendingWebsite === accepted && !cancelButton(),
    "Switching workspaces lets the accepted runtime installation continue without cancellation");
  check(element(".shell-activity-bar").textContent?.includes("Downloading the Nexus file… · 4 KB / 4 KB"),
    "Switching to My Mods preserves the same transfer observation in the global activity bar");
  // Late preparation events may arrive after runtime ownership; they must not restore cancellation.
  await act(async () => accepted.request.onProgress.onmessage("selecting"));
  check(element(".shell-activity-bar").textContent?.includes("Downloading the Nexus file… · 4 KB / 4 KB") && !cancelButton(),
    "Out-of-order preparation progress cannot downgrade a runtime-owned installation");
  await byteProgress(accepted.request, { attempt: 2, sequence: 6, phase: "verifying", receivedBytes: 4096, elapsedSeconds: 5 });
  check(element(".shell-activity-bar").textContent?.includes("Checking the Nexus file…")
    && !element(".shell-activity-bar").textContent?.includes("/s") && !fixture.querySelector(successSelector) && !cancelButton(),
    "File verification is a separate stage with no stale transfer speed or installation success");
  await byteProgress(accepted.request, { attempt: 2, sequence: 7, receivedBytes: 4096, elapsedSeconds: 6 });
  check(element(".shell-activity-bar").textContent?.includes("Checking the Nexus file…"),
    "Late download stages cannot replace verification within an attempt");
  await byteProgress(accepted.request, { attempt: 2, sequence: 7, phase: "installing", receivedBytes: 4096, elapsedSeconds: 6 });
  check(element(".shell-activity-bar").textContent?.includes("Installing the Nexus file…") && !fixture.querySelector(successSelector) && !cancelButton(),
    "Publication keeps an explicit installation stage until the native result arrives");
  const readsBeforeCompletion = inventoryReads;
  bridge.failNextRelease = progressEvent;
  await act(async () => { websiteInstalled = true; websiteInstalls++; pendingWebsite = null; accepted.resolve(stage(presentationName)); });
  await settle(() => inventoryReads > readsBeforeCompletion && Boolean(fixture.querySelector(successSelector)),
    "Successful background installation did not refresh inventory and show its staged file count");
  check(element(successSelector).textContent?.includes("2 files installed") && closedSessions.includes(accepted.request.sessionId),
    "The completed background session closes and reports the actual imported file count");
  check(bridge.releaseFailures === 1, "An asynchronous observer-release rejection preserves the completed native installation result");
  await byteProgress(accepted.request, { attempt: 2, sequence: 8, receivedBytes: 4096, elapsedSeconds: 7 });
  check(Boolean(fixture.querySelector(successSelector)) && !element(".shell-activity-bar").textContent?.includes("Downloading the Nexus file…"),
    "A completed promise removes its byte subscription and stale progress cannot replace success");
  await verifyPresentation("installed");
  await tab("Store"); await tab("My Mods"); await verifyPresentation("tab-return");
  await mount(); await tab("My Mods"); await verifyPresentation("remounted");
  check(websiteInstalls === 1 && localImports === 1 && details.settings_json === "{}",
    "Background presentation survives remount without reinstalling or saving credentials and references in settings");
}
async function automaticWebsiteRegression() {
  await mount();
  const beforeMounts = await beginWebsite();
  const automatic = pendingWebsite!, originalDetails = details, owningCoordinator = coordinator;
  const result = stage(presentationName);
  await progress(automatic.request, "installing");
  check(!fixture.querySelector(successSelector), "Automatic download reports no success before the runtime publishes its files");
  let ownerFlushed = false;
  const ownerFlush = owningCoordinator.flush(originalDetails.summary.id).then(() => { ownerFlushed = true; });
  const closesBeforeDetach = closedSessions.length;
  details = { ...details, summary: { ...details.summary, id: "fixture-nexus-other", name: "Other Nexus instance" } };
  await renderInstance();
  check(!ownerFlushed && pendingWebsite === automatic && closedSessions.length === closesBeforeDetach,
    "Changing instances retains runtime ownership and the original instance start barrier until publication");
  await act(async () => automatic.request.onProgress.onmessage("verifying"));
  await byteProgress(automatic.request, { receivedBytes: 1024, elapsedSeconds: 1 });
  check(!fixture.querySelector(".shell-activity-notice") && !fixture.textContent?.includes(presentationTitle),
    "An owned installation sends no stale progress or previous-instance artwork into a different instance");
  await act(async () => { websiteInstalls++; pendingWebsite = null; automatic.resolve(result); await ownerFlush; });
  await settle(() => closedSessions.includes(automatic.request.sessionId),
    "Detached automatic installation did not publish and close its background session");
  check(ownerFlushed && !fixture.querySelector(successSelector),
    "Publication releases the original start barrier without reporting success in the unrelated instance");
  details = originalDetails; await renderInstance(); await tab("My Mods"); await verifyPresentation("automatic");
  await verifyNoWebpage();
  const outcome = { webpage_mounts: embeddedMounts - beforeMounts,
    native_visibility_commands: commands.filter((command) => command === "update_nexus_website_host").length };
  check(outcome.webpage_mounts === 0 && outcome.native_visibility_commands === 0 && websiteInstalls === 2 && localImports === 1,
    "Automatic preparation through detached completion never asks the user to operate a webpage");
  return outcome;
}
function currentLoginBounds(sessionId: string) { return loginBounds.get(sessionId)?.at(-1); }
function checkLoginBounds(sessionId: string) {
  const bounds = currentLoginBounds(sessionId);
  check(bounds && devicePixelRatio === 1.5, "The explicit login page is measured at 1.5 device scale");
  const rect = element(`${loginSelector} .mw-reference-browser-page`).getBoundingClientRect();
  for (const [physical, css] of [[bounds.x, rect.left], [bounds.y, rect.top], [bounds.width, rect.width], [bounds.height, rect.height]]) {
    check(Number.isInteger(physical) && Math.abs(physical / devicePixelRatio - css) <= 1,
      "Login native bounds match its Store slot in physical client pixels");
  }
}
async function beginLogin() {
  const before = loginAttempts, downloadsBefore = openedSessions.length;
  expectedLogin = true;
  await click("[data-nexus-login-open]");
  await settle(() => Boolean(activeLogin && fixture.querySelector(loginSelector) && currentLoginBounds(activeLogin)),
    "An explicit sign-in action did not reveal its embedded Store page");
  const sessionId = activeLogin!;
  check(loginAttempts === before + 1, "StrictMode reserves exactly one login host per user action");
  check(openedSessions.length === downloadsBefore && !pendingWebsite,
    "Opening sign-in does not start a download or pretend to install anything");
  check(fixture.querySelectorAll(loginSelector).length === 1 && element(loginSelector).closest(".mw-dropzone") === element(".mw-manual-pane > .mw-dropzone"),
    "The login page stays inside the Store drop area without a second page");
  check(!document.querySelector('dialog[open], [role="dialog"], [role="alertdialog"]')
    && !fixture.querySelector(successSelector), "Sign-in opens no HTML popup or false success notice");
  check(element(loginSelector).textContent?.includes("retry your download") || element(loginSelector).textContent?.includes("Manage your account"),
    "The page distinguishes signing in from managing an already observed account");
  check(element<HTMLInputElement>(".mw-reference-input").disabled && element<HTMLButtonElement>("[data-nexus-login-open]").disabled,
    "While sign-in is open, reference installation and another sign-in action are disabled");
  checkLoginBounds(sessionId);
  return sessionId;
}
async function verifyLoginClosed(sessionId: string) {
  await settle(() => closedLogins.includes(sessionId) && !activeLogin && !fixture.querySelector(loginSelector),
    "Leaving or closing sign-in did not release its native host");
  await act(async () => { await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))); });
  expectedLogin = false;
  check(!fixture.querySelector(successSelector), "Closing an account page never reports an installation");
  await verifyNoWebpage();
}
async function closeLogin(sessionId: string) {
  await click(`${loginSelector} button`);
  await verifyLoginClosed(sessionId);
}
async function loginRegression() {
  await mount(); header();
  check(loginAttempts === 0 && loginSessions.length === 0, "All automatic download paths and failures leave sign-in unopened");
  const initialInventory = JSON.stringify(inventory()), originalWrites = localImports, originalInstalls = websiteInstalls;
  const sessionId = await beginLogin();
  const beforeDrop = openedSessions.length;
  await dropReference(websiteReference);
  check(openedSessions.length === beforeDrop && !pendingWebsite,
    "Dropping a project while sign-in is open cannot start a competing download");
  check(element(".shell-activity-bar").textContent?.includes("the download has not started")
    && [...fixture.querySelectorAll(".shell-activity-bar button")].some((node) => node.textContent === "Return to Store"),
    "A blocked project drop explains the occupied account page and offers Return to Store");
  check(element("[data-nexus-login-open]").textContent === "Nexus account",
    "An unknown website observation cannot mark the account as signed in");
  const initialWidth = currentLoginBounds(sessionId)!.width;
  await act(async () => { fixture.style.width = "calc(100% - 100px)"; });
  await settle(() => Boolean(currentLoginBounds(sessionId) && currentLoginBounds(sessionId)!.width < initialWidth - 100),
    "Resizing the Store did not resize the login native child");
  checkLoginBounds(sessionId);
  const initialX = currentLoginBounds(sessionId)!.x;
  await act(async () => { fixture.style.transform = "translate(8px, 6px)"; });
  await settle(() => Boolean(currentLoginBounds(sessionId) && currentLoginBounds(sessionId)!.x > initialX + 5),
    "Moving the Store did not move the login native child");
  checkLoginBounds(sessionId);
  await act(async () => { fixture.style.transform = "translateX(-100vw)"; });
  await settle(() => currentLoginBounds(sessionId) === null, "An offscreen Store did not hide its login native child");
  await act(async () => { fixture.style.transform = ""; fixture.style.width = ""; });
  await settle(() => Boolean(currentLoginBounds(sessionId)), "Returning the Store onscreen did not restore login bounds");
  const dialog = document.createElement("div");
  dialog.setAttribute("role", "dialog"); dialog.setAttribute("aria-modal", "true");
  dialog.style.cssText = "position:fixed;inset:40% 30%;z-index:100000;background:white";
  await act(async () => document.body.append(dialog));
  await settle(() => currentLoginBounds(sessionId) === null, "An HTML modal did not hide the login native child");
  await act(async () => dialog.remove());
  await settle(() => Boolean(currentLoginBounds(sessionId)), "Closing the HTML modal did not restore login bounds");
  checkLoginBounds(sessionId);
  await closeLogin(sessionId);
  check(!element<HTMLInputElement>(".mw-reference-input").disabled, "Closing sign-in makes manual retry available");
  const loginCount = loginAttempts;
  await beginWebsite(); const retried = pendingWebsite!;
  await progress(retried.request, "selecting");
  check(loginAttempts === loginCount, "Retry after sign-in returns to the background without reopening a webpage");
  await act(async () => cancelButton()!.click());
  await verifyClosed(retried.request.sessionId, retried.request.onProgress); await verifyCancelledFlush();

  const tabSession = await beginLogin();
  await tab("My Mods"); await verifyLoginClosed(tabSession); header();
  await tab("Store"); check(!fixture.querySelector(loginSelector), "Returning to Store does not reopen sign-in automatically");
  await act(async () => loginAuthChannels.get(tabSession)!.onmessage("signed_in"));
  check(element("[data-nexus-login-open]").textContent === "Nexus account" && !activeLogin,
    "An authentication event from the previous Store scope cannot relabel or reopen the current account");

  const instanceSession = await beginLogin(), originalDetails = details;
  details = { ...details, summary: { ...details.summary, id: "fixture-nexus-other", name: "Other Nexus instance" } };
  await renderInstance(); await verifyLoginClosed(instanceSession);
  details = originalDetails; await renderInstance();
  const unmounted = await beginLogin();
  await act(async () => root.render(null)); await verifyLoginClosed(unmounted); await mount();

  expectedLogin = true; delayLoginOpen = true;
  const beforeCancel = embeddedMounts;
  await click("[data-nexus-login-open]");
  await settle(() => Boolean(pendingLoginOpen && cancelButton()), "Delayed sign-in did not show cancellable local opening feedback");
  check(!fixture.querySelector(loginSelector) && activeLogin && currentLoginBounds(activeLogin) === null,
    "Pending sign-in reservation exposes no blank page or native bounds");
  const delayed = pendingLoginOpen!;
  await act(async () => cancelButton()!.click());
  await act(async () => { pendingLoginOpen = null; delayed.resolve(delayed.sessionId); });
  await verifyLoginClosed(delayed.sessionId);
  check(embeddedMounts === beforeCancel && loginBounds.get(delayed.sessionId)!.every((bounds) => bounds === null),
    "Cancelling pending sign-in closes its late result without ever showing the page");

  expectedLogin = true; delayLoginOpen = true;
  await click("[data-nexus-login-open]");
  await settle(() => Boolean(pendingLoginOpen), "Unmounted late sign-in reservation did not begin");
  const late = pendingLoginOpen!;
  await mount();
  await act(async () => { pendingLoginOpen = null; late.resolve(late.sessionId); });
  await verifyLoginClosed(late.sessionId);
  check(loginBounds.get(late.sessionId)!.every((bounds) => bounds === null),
    "A late sign-in response after unmount cannot resurrect its native page");

  expectedLogin = true; failLoginOpen = true;
  const beforeFailedOpen = loginSessions.length;
  await click("[data-nexus-login-open]");
  await settle(() => fixture.querySelector(".shell-activity-notice.is-error")?.textContent?.includes("The Nexus website session could not start") === true,
    "Sign-in host failure did not show actionable local feedback");
  expectedLogin = false;
  await verifyNoWebpage();
  check(loginSessions.length === beforeFailedOpen && JSON.stringify(inventory()) === initialInventory
    && localImports === originalWrites && websiteInstalls === originalInstalls,
    "Opening, closing and failed sign-in never alter installed files");
  await mount();
  return { explicit_only: true, mutually_exclusive: true, dpi: devicePixelRatio, physical_bounds: true,
    resize: true, movement: true, offscreen_hidden: true, modal_hidden: true, tab_cleanup: true,
    instance_cleanup: true, unmount_cleanup: true, pending_cancel_cleanup: true, late_open_cleanup: true,
    failure_without_page: true, retry_background_only: true, no_authenticated_claim: true };
}
async function emitAuth(sessionId: string, value: AuthState) {
  await act(async () => { nativeAuthState = value; loginAuthChannels.get(sessionId)!.onmessage(value); });
}
async function authenticationRegression() {
  nativeAuthState = "unknown"; await mount();
  await settle(() => element("[data-nexus-login-open]").textContent === "Nexus account", "The initial unknown cache was not shown neutrally");
  const startingEpoch = epoch, sameCoordinator = coordinator, beforeLoginAttempts = loginAttempts;
  await beginWebsite(); const needsLogin = pendingWebsite!;
  await act(async () => {
    nativeAuthState = "signed_out"; pendingWebsite = null;
    needsLogin.reject({ code: "nexus_website_login_required" });
  });
  await verifyClosed(needsLogin.request.sessionId, needsLogin.request.onProgress);
  await settle(() => fixture.querySelector(".shell-activity-notice.is-error")?.textContent?.includes("Nexus requires sign-in") === true
    && element("[data-nexus-login-open]").textContent === "Nexus sign-in",
    "A download's observed sign-out did not refresh the account button and explain the login requirement");
  check(loginAttempts === beforeLoginAttempts, "A login-required download never opens the website automatically");
  let blockedStart: unknown;
  await act(async () => { try { await coordinator.flush(details.summary.id); } catch (reason) { blockedStart = reason; } });
  check(blockedStart instanceof Error && blockedStart.message === "nexus_website_login_required",
    "The failed pre-login attempt remains visible to the start coordinator until a real retry");

  const sessionId = await beginLogin();
  const attemptsBeforeAuth = openedSessions.length;
  check(element("[data-nexus-login-open]").textContent === "Nexus account" && activeLogin === sessionId,
    "The early unknown authentication event neither claims sign-in nor closes the page");
  await emitAuth(sessionId, "signed_out");
  check(element("[data-nexus-login-open]").textContent === "Nexus sign-in" && Boolean(fixture.querySelector(loginSelector)),
    "A signed-out observation keeps the page available for the user");
  delayLoginClose = true;
  await emitAuth(sessionId, "signed_in");
  await settle(() => Boolean(pendingLoginClose), "Confirmed sign-in did not begin returning to Store");
  check(!fixture.querySelector(loginSelector) && element(".shell-activity-bar").textContent?.includes("Closing the Nexus account page")
    && element<HTMLInputElement>(".mw-reference-input").disabled,
    "Confirmed first sign-in hides the page and shows closing until the native reservation is released");
  await dropReference(websiteReference);
  check(openedSessions.length === attemptsBeforeAuth && !pendingWebsite
    && element(".shell-activity-bar").textContent?.includes("Wait before starting a download"),
    "A link dropped during delayed close gives visible feedback without competing download or queued retry");
  await act(async () => loginAuthChannels.get(sessionId)!.onmessage("signed_out"));
  check(element("[data-nexus-login-open]").textContent === "Nexus signed in",
    "Late authentication events cannot downgrade a closing session's verified result");
  const closing = pendingLoginClose!;
  await act(async () => { pendingLoginClose = null; closing.resolve(); });
  await verifyLoginClosed(sessionId);
  await settle(() => !element<HTMLInputElement>(".mw-reference-input").disabled
    && element("[data-nexus-login-open]").textContent === "Nexus signed in",
    "Awaiting native close did not restore the verified account and install controls");
  check(openedSessions.length === attemptsBeforeAuth && !pendingWebsite,
    "Returning after confirmed sign-in does not create an unrequested download");

  bridge.failNext = progressEvent;
  await beginWebsite(); const retried = pendingWebsite!;
  check(bridge.failures === 1, "An event-subscription rejection does not prevent the same native download command from starting");
  await progress(retried.request, "selecting"); await progress(retried.request, "installing");
  const beforeCompletionReads = authReads;
  await act(async () => { websiteInstalls++; pendingWebsite = null; retried.resolve(stage(presentationName)); });
  await settle(() => Boolean(fixture.querySelector(successSelector)) && authReads > beforeCompletionReads,
    "The same mounted post-login retry did not publish files and refresh the authentication cache");
  check(epoch === startingEpoch && coordinator === sameCoordinator && websiteInstalls === 3,
    "Login-required, confirmed sign-in and successful retry complete in the same mounted workspace");
  await act(async () => { await coordinator.flush(details.summary.id); });
  await tab("My Mods"); await verifyPresentation("authenticated");
  await click(`${successSelector} .shell-activity-notice-close`);
  const readsBeforeReturn = authReads;
  await tab("Store");
  await settle(() => authReads > readsBeforeReturn, "Returning to Store did not refresh the website observation cache");

  const managed = await beginLogin();
  await emitAuth(managed, "signed_in");
  check(activeLogin === managed && Boolean(fixture.querySelector(loginSelector))
    && element(loginSelector).textContent?.includes("Manage your account"),
    "Opening an already verified account lets the user manage it without automatic closure");
  await emitAuth(managed, "signed_out");
  check(element("[data-nexus-login-open]").textContent === "Nexus sign-in",
    "A genuine sign-out observation clears the signed-in account label");
  await emitAuth(managed, "unknown");
  check(element("[data-nexus-login-open]").textContent === "Nexus account",
    "An indeterminate website observation uses the neutral account label");
  await emitAuth(managed, "signed_in"); await closeLogin(managed);
  return { cache_read_without_page: true, unknown_is_neutral: true, signed_out_is_explicit: true,
    first_sign_in_returns_to_store: true, known_account_stays_open: true, close_is_awaited: true,
    closing_drop_has_feedback: true, no_automatic_retry: true, same_mount_retry_installs: true,
    stale_events_ignored: true, download_completion_refreshes_cache: true, store_return_refreshes_cache: true };
}
async function verifyBrowserPreview() {
  const desktopDetails = details, beforeCommands = commands.length, beforeSessions = openedSessions.length;
  Object.assign(window, { isTauri: false });
  check(!sessionStorage.getItem("langameLanToken"), "Browser preview runs without a LAN token or native host");
  try {
    const created = await invokeMock<{ summary: { id: string } }>("create_instance_record", {
      input: { module_id: "sevendaystodie", name: "Nexus browser preview" },
    });
    details = await readInstanceDetails(created.summary.id);
    await mount(); header();
    await dropReference(websiteReference);
    await tab("My Mods"); await dropReference(websiteReference, element(".mw-config-pane .mw-dropzone"));
    check(!fixture.querySelector(`${embeddedSelector}, .mw-reference-input, .shell-activity-notice.is-error`)
      && !fixture.textContent?.includes(openingCopy) && openedSessions.length === beforeSessions,
      "Ordinary browser preview ignores website drops without opening a blank page or unsupported-operation error");
    check(commands.slice(beforeCommands).every((command) => !(command.includes("nexus_website") || command.includes("nexus_login"))),
      "Ordinary browser preview issues no native Nexus website commands");
  } finally {
    await act(async () => root.render(null));
    details = desktopDetails; Object.assign(window, { isTauri: true });
  }
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const template = await readInstanceDetails("srv-dst-terminal-error");
  details = { ...template, summary: { ...template.summary, id: "fixture-nexus", module_id: "sevendaystodie", name: "Nexus fixture" }, settings_json: "{}", active_run: null };
  mockWindows("nexus-fixture");
  mockIPC(dispatch, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  await mount(); header();
  check(!commands.some((command) => legacyApiCommands.has(command)) && openedSessions.length === 0,
    "Initial Nexus rendering reads inventory without any account API or website session");
  await sourceGuidance();
  await click(".mw-source-open-btn");
  check(openedLinks.length === 1, "Nexus website opens from the Store");
  const reads = inventoryReads;
  await dropFile();
  await settle(() => localImports === 1 && inventoryReads > reads && Boolean(fixture.querySelector(successSelector)),
    "Local import did not refresh inventory and display its result");
  const successShownAt = performance.now();
  check(element(successSelector).textContent?.includes("2 files installed")
    && !fixture.querySelector(".mw-manual-success-bar,.mw-manual-feedback .mw-chip--on"),
    "Local installation reports its actual file count only in the activity bar");
  await tab("My Mods"); header();
  await settle(() => fixture.textContent?.includes("FixtureNexusMod") === true, "Imported local Mod did not appear in My Mods");
  const installedInventory = JSON.stringify(inventory());
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 4200)); });
  check(Boolean(fixture.querySelector(successSelector)), "Local installation success stays visible before five seconds");
  await settle(() => !fixture.querySelector(successSelector), "Local installation success did not disappear after five seconds");
  const successAutoDismissMs = performance.now() - successShownAt;
  check(successAutoDismissMs >= 4800 && successAutoDismissMs < 6500, "Local installation success expires after five real seconds");
  check(JSON.stringify(inventory()) === installedInventory && localImports === 1
    && [...fixture.querySelectorAll(".mw-entry-row-title")].some((node) => node.textContent === "FixtureNexusMod"),
    "Automatic notice expiry preserves the installed inventory without another import");
  check(details.settings_json === "{}", "Local file management does not create Nexus credentials or references in settings");
  await websiteRegression();
  const automatic = await automaticWebsiteRegression();
  for (const options of [{ running: true }, { archived: true }]) {
    await mount(options); header();
    const writes = localImports, sessions = openedSessions.length;
    check(element<HTMLInputElement>(".mw-reference-input").disabled, "Running and archived instances disable website reference installation");
    if (options.archived) {
      const store = [...fixture.querySelectorAll<HTMLButtonElement>('[role="tab"]')].find((item) => item.textContent === "Store");
      check(store?.disabled && !fixture.querySelector(".mw-dropzone"), "Archived instances expose no installation workspace");
      await dropFile(fixture); await dropReference(websiteReference, fixture);
    } else {
      check(element(".mw-dropzone").getAttribute("aria-disabled") === "true", "Running instances disable local import");
      await dropFile(); await dropReference(websiteReference);
    }
    check(localImports === writes && openedSessions.length === sessions, "Running and archived instances cannot import files or open website installation sessions");
  }
  const originalFetch = window.fetch;
  window.fetch = async (request, options) => {
    if (request === "/__langame/api") {
      const { command, args } = JSON.parse(String(options?.body)) as { command: string; args: unknown };
      const value = await dispatch(command, args);
      return new Response(JSON.stringify({ ok: true, value }), { headers: { "Content-Type": "application/json" } });
    }
    return originalFetch(request, options);
  };
  Object.assign(window, { isTauri: false }); sessionStorage.setItem("langameLanToken", "isolated-fixture-token");
  const lanStart = commands.length;
  try {
    await mount(); header();
    const sessions = openedSessions.length, writes = localImports;
    await dropReference(websiteReference);
    check(openedSessions.length === sessions && localImports === writes, "LAN ignores website-reference drops without opening a local native window");
    await dropFile();
    await settle(() => localImports === 2 && Boolean(fixture.querySelector(successSelector)), "LAN local import did not complete");
    check(element(successSelector).textContent?.includes("files installed"), "LAN keeps local file installation feedback in the activity bar");
    await tab("My Mods");
    await settle(() => fixture.textContent?.includes("FixtureNexusMod") === true, "LAN installation inventory did not render");
    const beforeDismiss = JSON.stringify(inventory());
    await click(`${successSelector} .shell-activity-notice-close`);
    await settle(() => !fixture.querySelector(successSelector), "The success notice did not close manually");
    await tab("Store"); await tab("My Mods");
    check(!fixture.querySelector(successSelector) && JSON.stringify(inventory()) === beforeDismiss && localImports === 2
      && [...fixture.querySelectorAll(".mw-entry-row-title")].some((node) => node.textContent === "FixtureNexusMod"),
      "Manual dismissal survives a workspace tab change without removing installed files or importing again");
    check(commands.slice(lanStart).every((command) => !command.includes("nexus_website") && !command.includes("nexus_login")),
      "The entire LAN workflow issues no native website-window commands");
  } finally {
    window.fetch = originalFetch; sessionStorage.removeItem("langameLanToken"); Object.assign(window, { isTauri: true });
  }
  await verifyBrowserPreview();
  const login = await loginRegression();
  const authentication = await authenticationRegression();
  await mount(); header(); await tab("My Mods"); await verifyPresentation("final");
  check(commands.every((command) => !legacyApiCommands.has(command)),
    "Desktop and LAN workflows issue no Nexus API operations");
  check(!pendingOpen && !pendingWebsite && openedSessions.every((session) => closedSessions.includes(session)),
    "All reserved website sessions are closed at fixture completion");
  check(openedLinks.length === 1, "Background operations and blocked states never open the system browser");
  // Verify the background stage before capturing the explicit login layout.
  // The native boundary is mocked; no website pixels or login status are fabricated.
  await tab("Store"); await beginWebsite(); await progress(pendingWebsite!.request, "waiting");
  const capturedBackground = pendingWebsite!;
  await act(async () => cancelButton()!.click());
  await verifyClosed(capturedBackground.request.sessionId, capturedBackground.request.onProgress); await verifyCancelledFlush();
  const finalLogin = await beginLogin();
  await emitAuth(finalLogin, "signed_in");
  check(Boolean(fixture.querySelector(loginSelector)) && element("[data-nexus-login-open]").textContent === "Nexus signed in",
    "The final account layout shows a verified native observation while management remains available");
  const accountBounds = currentLoginBounds(finalLogin);
  await closeLogin(finalLogin);
  await dragHint();
  await document.fonts.ready;
  await settle(() => {
    const slide = fixture.querySelector<HTMLElement>(".shell-activity-slide");
    return Boolean(slide && slide.getAnimations().every((animation) => animation.playState === "finished")
      && Number(getComputedStyle(slide).opacity) >= 0.99);
  }, "The activity hint entrance animation did not finish before capture");
  check(errors.length === 0, "Nexus browser has no console or uncaught errors");
  const progressEntries = bridge.entries.filter((entry) => entry.name === progressEvent);
  check(progressEntries.every((entry) => entry.released && !entry.pending && !bridge.active.has(entry.id)),
    "Completed, cancelled and detached promises retire every successful byte-progress subscription");
  return { status: "passed", checks: [...new Set(checks)], assertions: checks.length,
    local_imports: localImports, success_auto_dismiss_ms: Math.round(successAutoDismissMs),
    success_manual_dismiss: true, inventory_retained_after_feedback: true,
    nexus_api_calls: commands.filter((command) => legacyApiCommands.has(command)).length,
    website_installs: websiteInstalls, website_references: websiteReferences,
    automatic_installation: automatic,
    byte_progress: { real_counters: true, unknown_total: true, retry_sampling_reset: true, old_attempt_ignored: true,
      old_sequence_ignored: true, instance_session_isolation: true, owned_cancel_disabled: true, full_bytes_not_installed: true,
      stages: ["downloading", "verifying", "installing"], subscription_failure_does_not_abort: bridge.failures === 1,
      async_release_failure_does_not_abort: bridge.releaseFailures === 1,
      subscriptions: progressEntries.length, unsubscriptions: progressEntries.filter((entry) => entry.released).length },
    website_sessions: { opened: openedSessions.length, closed: new Set(closedSessions).size },
    coordinator: { cancellation_flushes: cancelledFlushes, reference_failure_blocks_flush: true },
    loading: { cancel_without_flash: true, failure_without_flash: true, late_open_without_flash: true,
      browser_preview_has_no_native_commands: true },
    background: { phases: [...observedPhases].sort(), webpage_mounts: embeddedMounts,
      native_visibility_commands: commands.filter((command) => command === "update_nexus_website_host").length,
      blocked_errors: blockedErrors, same_instance_tabs_continue: true, unmount_cleanup: true, late_open_cleanup: true,
      instance_switch_isolated: true, owned_installation_survives_detach: true,
      my_mods_link_drop: true, my_mods_id_entry: true, installing_phase_retained: true },
    login, authentication, auth_cache_reads: authReads, login_sessions: { attempts: loginAttempts, opened: loginSessions.length, closed: new Set(closedLogins).size },
    login_bounds: accountBounds,
    guidance: { hover_without_bubble: true, focus_without_bubble: true, activity_store: true,
      store_link_drag: true, my_mods_link_drag: true, local_file_drag: true, leave_clears_hint: true,
      website_button_click: true, specific_errors_in_activity: true },
    screenshot_state: "nexus-drag-hint-in-activity",
    presentation: { title: presentationTitle, source_name: presentationName, decoded_stages: presentationStages },
    evidence_scope: "Real React and browser interaction with mocked native commands and fixture-served image delivery; no native WebView or live Nexus download",
    browser_errors: errors, viewport: { width: innerWidth, height: innerHeight } };
}
Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  await act(async () => root.unmount());
  await settle(() => !pendingOpen && !pendingWebsite && !pendingLoginOpen && !pendingLoginClose && !activeLogin
    && openedSessions.every((session) => closedSessions.includes(session)) && loginSessions.every((session) => closedLogins.includes(session)),
    "Post-capture cleanup did not close all reserved Nexus download and login sessions");
  check(!fixture.querySelector(embeddedSelector), "Post-capture cleanup leaves no webpage or popup");
  pageObserver.disconnect();
  return { browser_errors: errors, native_dialogs: 0, login_sessions: { opened: loginSessions.length, closed: new Set(closedLogins).size }, website_sessions: { opened: openedSessions.length, closed: new Set(closedSessions).size } };
} });
void run().catch((error) => ({ status: "failed", error: String(error), checks: [...new Set(checks)], assertions: checks.length, browser_errors: errors }))
  .finally(() => Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }))
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`, { method: "POST", body: JSON.stringify(report) }));
