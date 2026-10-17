import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { readInstanceDetails } from "../../src/api";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import type { InstanceDetails, ManualModInventoryResult } from "../../src/types";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import { createModWorkbenchFixtureIpc, ModWorkbenchBrowserHost, workshopItem } from "./mod-workbench-browser-support";

// IPC is the only substituted boundary. This fixture never invokes SteamCMD,
// writes real settings, imports packages or starts a game server.
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:16px;box-sizing:border-box;display:flex;flex-direction:column";
const root = createRoot(fixture);
const checks: string[] = [], errors: string[] = [], downloads: string[][] = [];
const appId = 1623730, itemId = "3816899969";
const output = `workshop_download_item ${appId} ${itemId} validate\nERROR! Download item ${itemId} failed (Failure).\nWorkshop content: Missing decryption key\n`;
const downloadError = JSON.stringify({ code: "steam_workshop_decryption_key_missing", consumer_app_id: appId,
  message: "Workshop download failed because Steam did not provide a decryption key", output_excerpt: output });
const moduleDetails = buildMockModuleDetails("palworld");
const catalog = [{ ...workshopItem(itemId, "Workshop fixture Mod"), consumer_app_id: appId }];
const cached = new Set<string>();
let details: InstanceDetails, inventory: ManualModInventoryResult, writes = 0, localImports = 0;
const localSourcePath = `C:\\Fixture\\steamapps\\workshop\\content\\${appId}\\${itemId}`;
const localBoundaryMessage = "Fixture: local file import reached its IPC boundary";
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(`${label}: ${fixture.textContent}`);
  checks.push(label);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const found = fixture.querySelector<T>(selector);
  if (!found) throw new Error(`Missing ${selector}: ${fixture.textContent}`);
  return found;
}
async function settle(predicate: () => boolean, label: string) {
  const deadline = performance.now() + 6000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(`${label}: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(target: HTMLElement) {
  assert(!(target instanceof HTMLButtonElement) || !target.disabled, "The retry target is enabled");
  await act(async () => { target.scrollIntoView({ block: "nearest" }); target.focus(); target.click(); });
}
function draw() {
  root.render(<ModWorkbenchBrowserHost details={details} moduleDetails={moduleDetails} epoch={0} onSaved={draw} />);
}
function localRecoveryLayout() {
  const dropzone = element(".mw-dropzone").getBoundingClientRect();
  const configuration = element(".mw-config-pane").getBoundingClientRect();
  const activity = element(".shell-activity-bar"), activityRect = activity.getBoundingClientRect();
  const documentOverflow = document.documentElement.scrollWidth > innerWidth + 1;
  const activityOverflow = activity.scrollWidth > activity.clientWidth + 1;
  assert(dropzone.height >= 120 && element(".mw-dropzone").classList.contains("mw-detail-list-col")
    && !fixture.querySelector(".mw-manual-pane,.mw-source-open-btn,.mw-dropzone-copy"), "The real My Mods list receives local files without permanent import or source guidance");
  assert(configuration.height >= 120 && configuration.width > 0 && configuration.top <= dropzone.top + 1 &&
    configuration.bottom >= dropzone.bottom - 1 &&
    configuration.bottom <= activityRect.top + 1 && configuration.left >= -1 && configuration.right <= innerWidth + 1,
    "The list drop target stays inside the visible configuration area above the activity bar");
  assert(!documentOverflow && !activityOverflow && activityRect.left >= -1 && activityRect.right <= innerWidth + 1,
    "The document and activity bar fit horizontally without overflow");
  return { dropzone: { height: dropzone.height, width: dropzone.width }, configuration: { height: configuration.height, width: configuration.width },
    document_overflow_x: documentOverflow, activity_overflow_x: activityOverflow };
}
async function run() {
  const response = await fetch("/__workshop_download_error_locale");
  const { locale } = await response.json() as { locale: string };
  assert(locale === "zh-CN" || locale === "en-US", "The browser scenario has an explicit supported locale");
  const chinese = locale === "zh-CN";
  localStorage.setItem("langame.locale", locale);
  const template = await readInstanceDetails("srv-dst-terminal-error");
  details = { ...template, summary: { ...template.summary, id: "fixture-workshop-download-error", module_id: "palworld",
    status: "Stopped", active_process_count: 0 }, active_run: null, settings_json: "{}" };
  const initialSettings = details.settings_json;
  inventory = { instance_id: details.summary.id, module_id: "palworld", source_label: "Fixture files",
    target_label: "Palworld Mods", target_path: "C:\\Fixture\\Palworld\\Mods", target_exists: true, items: [] };
  assert(moduleDetails.workshop?.consumer_app_id === appId && Boolean(moduleDetails.mods?.manual_staging),
    "The scenario uses the real Palworld Workshop and local staging module contract");
  mockWindows("workshop-download-error");
  const base = createModWorkbenchFixtureIpc(() => ({ details, moduleDetails, catalog, cached, inventory }), {
    check: assert,
    save: (saved) => { details = saved; writes += 1; },
    download: (ids) => { downloads.push(ids); throw new Error(downloadError); }
  });
  mockIPC(async (command, payload) => {
    if (command === "stage_manual_mod_files") {
      const args = payload as { instanceId: string; sourcePaths: string[] };
      assert(args.instanceId === details.summary.id && args.sourcePaths.length === 1 && args.sourcePaths[0] === localSourcePath,
        "A local Workshop folder drop invokes the real instance-scoped staging command with its exact native path");
      localImports += 1;
      // Observe the real command invocation without fabricating installed files.
      throw new Error(localBoundaryMessage);
    }
    return base(command, payload);
  }, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  await act(async () => { await prepareBrowserLocaleCatalogs(); draw(); });
  await settle(() => Boolean(fixture.querySelector(".mw-mod-quick-action:not(:disabled)")), "The real Workshop install action did not become available");
  await click(element<HTMLButtonElement>(".mw-mod-quick-action"));
  await settle(() => downloads.length === 1 && Boolean(fixture.querySelector(".shell-activity-notice.is-error")) &&
    !element<HTMLButtonElement>(".mw-mod-quick-action").disabled, "The failed download did not return its install action to an enabled state");
  const notice = element<HTMLElement>(".shell-activity-notice.is-error .shell-activity-notice-text");
  assert(notice.textContent?.includes(chinese ? "Steam 未向本次会话" : "Steam did not provide") &&
    notice.textContent.includes(chinese ? "解密密钥" : "decryption key"), "The real ModWorkbench renders the localized verified decryption-key cause");
  assert(notice.title.endsWith(output) && notice.textContent.includes("Missing decryption key"),
    "The original missing-key diagnostic survives the real error notice unchanged");
  assert(notice.textContent.includes("steamapps/workshop/content/1623730"), "The recovery notice includes the correct Steam library content directory");
  assert(writes === 0 && details.settings_json === initialSettings && cached.size === 0 && !fixture.querySelector(".shell-activity-notice.is-success"),
    "A failed Workshop download neither saves settings nor presents installation success");
  await click(element<HTMLButtonElement>(".mw-mod-quick-action"));
  await settle(() => downloads.length === 2 && !element<HTMLButtonElement>(".mw-mod-quick-action").disabled,
    "A second user retry did not complete its failed lifecycle");
  assert(downloads.every((ids) => ids.length === 1 && ids[0] === itemId), "Explicit retry sends the selected Workshop item without duplicating work");
  const myMods = [...fixture.querySelectorAll<HTMLButtonElement>("button[role=tab]")]
    .find((button) => button.textContent?.trim() === (chinese ? "我的 Mod" : "My Mods"));
  assert(myMods, "My Mods remains reachable after the download failure");
  await click(myMods);
  await settle(() => Boolean(fixture.querySelector(".mw-dropzone")), "The Palworld local file import area is not reachable");
  const dropzone = element<HTMLElement>(".mw-dropzone");
  assert(dropzone.getAttribute("aria-label") === (chinese ? "把下载好的 Mod 拖到这里" : "Drop downloaded mods here") &&
    dropzone.classList.contains("mw-detail-list-col") && !fixture.querySelector(".mw-dropzone-feedback") &&
    dropzone.getAttribute("aria-disabled") === "false", "The recovery route exposes the real enabled Mod list without permanent drag guidance");
  const fileDrop = new DataTransfer();
  const file = new File([], itemId);
  Object.defineProperty(file, "path", { value: localSourcePath });
  fileDrop.items.add(file);
  Object.defineProperty(fileDrop, "effectAllowed", { configurable: true, value: "copyLink" });
  Object.defineProperty(fileDrop, "dropEffect", { configurable: true, writable: true, value: "none" });
  const dragOver = new DragEvent("dragover", { dataTransfer: fileDrop, bubbles: true, cancelable: true });
  await act(async () => { dropzone.dispatchEvent(dragOver); });
  assert(dragOver.defaultPrevented && fileDrop.dropEffect === "copy", "The local import area accepts a downloaded package drag after failure");
  await act(async () => { dropzone.dispatchEvent(new DragEvent("drop", { dataTransfer: fileDrop, bubbles: true, cancelable: true })); });
  await settle(() => localImports === 1 && fixture.textContent?.includes(localBoundaryMessage) === true &&
    element(".mw-dropzone").getAttribute("aria-disabled") === "false", "The local file import did not reach and recover from its IPC boundary");
  const localNotice = [...fixture.querySelectorAll<HTMLElement>(".shell-activity-notice.is-error")]
    .find((entry) => entry.textContent?.includes(localBoundaryMessage));
  assert(localNotice, "The local import boundary failure is reported without claiming installed files");
  await click(localNotice.querySelector<HTMLButtonElement>(".shell-activity-notice-close")!);
  assert(details.summary.status === "Stopped" && details.summary.active_process_count === 0 && details.active_run === null &&
    writes === 0 && details.settings_json === initialSettings && !fixture.querySelector(".shell-activity-notice.is-success"),
    "Download failures and local import rejection preserve the stopped instance and settings without false success");
  const layout = localRecoveryLayout();
  assert(errors.length === 0, "Browser console and uncaught errors remain empty");
  return { status: "passed", locale, checks, writes, downloads, instance_status: details.summary.status,
    original_diagnostic_preserved: true, retry_recovered: true, local_import_available: true, local_import_calls: localImports, layout, browser_errors: errors };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error("Workshop download-error browser regression exceeded 45 seconds")), 45000);
})]).catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => {
    const nonce = new URLSearchParams(location.search).get("nonce");
    return fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) });
  });
