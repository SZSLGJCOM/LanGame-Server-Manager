import React, { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { readInstanceDetails, updateInstance } from "../../src/api";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { I18nProvider } from "../../src/i18n";
import { ModWorkbench } from "../../src/views/servers/ModWorkbench";
import { InstanceSettingsSaveProvider, useInstanceSettingsSaveCoordinator } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceSettingsSaveCoordinator } from "../../src/views/settings/instance-settings-save-coordinator";
import type { BackgroundJob, InstanceDetails, SteamWorkshopDownloadResult } from "../../src/types";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import { createModWorkbenchFixtureIpc, workshopItem } from "./mod-workbench-browser-support";

// Only native IPC is replaced. The real workbench, save coordinator, download
// state, item lifecycle and navigation render throughout this app session.
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture");
if (!fixture) throw new Error("Missing browser fixture root");
fixture.style.cssText = "height:100vh;padding:16px;box-sizing:border-box;display:flex;flex-direction:column";
const root = createRoot(fixture);
const moduleDetails = buildMockModuleDetails("dontstarve");
const firstInstance = "fixture-workshop-progress-a", otherInstance = "fixture-workshop-progress-b";
const firstItem = "1111111", failedItem = "2222222";
const catalog = [workshopItem(firstItem, "Navigation Progress Mod"), workshopItem(failedItem, "Rejected Download Mod")];
const checks: string[] = [], errors: string[] = [];
const downloads: { instance_id: string; ids: string[] }[] = [];
const writes: { instance_id: string; settings_json: string }[] = [];
const coordinators = new Set<InstanceSettingsSaveCoordinator>();
const instances = new Map<string, { details: InstanceDetails; cached: Set<string> }>();
const pending: PendingDownload[] = [];
let activeInstance = firstInstance, mounted = true, epoch = 0;
let jobs: BackgroundJob[] = [];
let locale = "en-US";
const failureMessage = "Fixture Workshop download failed before any files were installed";

interface PendingDownload {
  instanceId: string;
  ids: string[];
  jobId: string;
  finish: (failure?: string) => void;
}

addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(`${label}: ${fixture?.textContent}`);
  checks.push(label);
}
function state(instanceId = activeInstance) {
  const value = instances.get(instanceId);
  if (!value) throw new Error(`Unknown fixture instance ${instanceId}`);
  return value;
}
function element<T extends HTMLElement = HTMLElement>(selector: string, within: ParentNode = fixture): T {
  const found = within.querySelector<T>(selector);
  if (!found) throw new Error(`Missing ${selector}: ${fixture.textContent}`);
  return found;
}
function maybeCard(itemId: string) {
  const title = catalog.find((item) => item.id === itemId)?.title;
  return [...fixture.querySelectorAll<HTMLElement>(".mw-mod-card")]
    .find((entry) => entry.querySelector(".mw-mod-card-title")?.textContent === title);
}
function card(itemId: string) {
  const found = maybeCard(itemId);
  if (!found) throw new Error(`Missing Workshop card ${itemId}: ${fixture.textContent}`);
  return found;
}
function pendingRow(itemId: string) {
  return fixture.querySelector<HTMLElement>(`.mw-workshop-download-row[data-workshop-id="${itemId}"]`);
}
function rows(itemId: string) {
  const title = catalog.find((item) => item.id === itemId)?.title;
  return [...fixture.querySelectorAll<HTMLElement>(".mw-entry-row")]
    .filter((entry) => entry.querySelector(".mw-entry-row-title")?.textContent === title);
}
function installing(node: HTMLElement | null | undefined, percent?: number) {
  const chip = node?.querySelector(".mw-chip--lifecycle-installing");
  return Boolean(chip?.textContent?.includes(locale === "zh-CN" ? "正在安装" : "Installing") &&
    (percent === undefined || chip.textContent.includes(`${percent}%`)));
}
async function settle(predicate: () => boolean, label: string) {
  const deadline = performance.now() + 7000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(`${label}: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(target: HTMLElement) {
  assert(!(target instanceof HTMLButtonElement) || !target.disabled, "The requested navigation or install action is enabled");
  await act(async () => { target.scrollIntoView({ block: "nearest" }); target.focus(); target.click(); });
}
async function tab(view: "store" | "mine") {
  const label = view === "mine" ? (locale === "zh-CN" ? "我的 Mod" : "My Mods")
    : locale === "zh-CN" ? "本周热门" : "Popular this week";
  const target = [...fixture.querySelectorAll<HTMLButtonElement>('button[role="tab"]')]
    .find((entry) => entry.textContent?.trim() === label);
  assert(target, `The ${view} view remains reachable`);
  await click(target);
}
function Workspace() {
  const coordinator = useInstanceSettingsSaveCoordinator();
  const [noticeTarget, setNoticeTarget] = useState<HTMLDivElement | null>(null);
  coordinators.add(coordinator);
  return <ActivityNoticeTarget.Provider value={{ element: noticeTarget, dismissLabel: "Close" }}>
    <div style={{ flex: 1, minHeight: 0 }} data-fixture-instance={activeInstance}>
      {mounted ? <ModWorkbench key={`${activeInstance}:${epoch}`} details={state().details} moduleDetails={moduleDetails}
        jobs={jobs} launchPlan={null} onSaveSettings={async (input, options) => {
          const saved = await updateInstance(input, options?.expectedSettingsJson ?? "", options?.collectionRemoval);
          draw();
          return saved;
        }} /> : <div data-fixture-away>Another server workspace</div>}
    </div>
    <footer className="shell-activity-bar"><span className="shell-activity-label">Activity</span>
      <div className="shell-activity-notices" ref={setNoticeTarget} /></footer>
  </ActivityNoticeTarget.Provider>;
}
function draw() {
  // These providers and Workspace retain their identities when only the
  // workbench is removed, remounted, or changed to another instance.
  root.render(<I18nProvider><InstanceSettingsSaveProvider><Workspace /></InstanceSettingsSaveProvider></I18nProvider>);
}
async function publishProgress(request: PendingDownload, percent: number, status = "Running") {
  const job: BackgroundJob = { id: request.jobId, kind: "DownloadWorkshop", label: "Fixture Workshop download",
    status, target_id: request.instanceId, progress_percent: percent, install_progress: null,
    cancellable: false, cancel_requested: false, warnings: [], detail: "Fixture batch download progress", output_excerpt: null };
  await act(async () => { jobs = [job, ...jobs.filter((entry) => entry.id !== job.id)]; draw(); });
}
async function leaveAndReturn() {
  await act(async () => { mounted = false; draw(); });
  assert(Boolean(fixture.querySelector("[data-fixture-away]")) && !fixture.querySelector(".mw-mod-card,.mw-config-pane"),
    "Leaving removes the actual ModWorkbench subtree");
  await act(async () => { epoch += 1; mounted = true; draw(); });
  await settle(() => Boolean(maybeCard(firstItem)), "The workbench did not remount its Workshop catalog");
  assert(coordinators.size === 1, "Workbench remount preserves the application save coordinator");
}
async function switchInstance(instanceId: string) {
  await act(async () => { activeInstance = instanceId; draw(); });
  await settle(() => Boolean(maybeCard(firstItem)), "The selected instance did not load its Workshop catalog");
}
async function blockDuplicate(itemId: string, expectedDownloads: number) {
  const action = element<HTMLButtonElement>(".mw-mod-quick-action", card(itemId));
  assert(action.disabled, "A running download disables another install of the same Mod");
  await act(async () => { action.click(); });
  assert(downloads.length === expectedDownloads, "A disabled install action cannot submit a duplicate download");
}

async function run() {
  const response = await fetch("/__workshop_download_progress_locale");
  const setup = await response.json() as { locale: string };
  assert(setup.locale === "zh-CN" || setup.locale === "en-US", "The browser scenario selects a supported locale");
  locale = setup.locale;
  localStorage.setItem("langame.locale", locale);
  const template = await readInstanceDetails("srv-dst-terminal-error");
  for (const instanceId of [firstInstance, otherInstance]) {
    instances.set(instanceId, { cached: new Set(), details: { ...structuredClone(template),
      summary: { ...template.summary, id: instanceId, module_id: "dontstarve", status: "Stopped", active_process_count: 0 },
      active_run: null, settings_json: JSON.stringify({ enable_caves: true, shared_workshop_mod_ids: "",
        shared_workshop_collection_ids: "", master_enabled_workshop_mod_ids: "", caves_enabled_workshop_mod_ids: "",
        steam_workshop_collections: [] }) } });
  }
  const otherSettings = state(otherInstance).details.settings_json;
  mockWindows("workshop-download-progress");
  mockIPC(async (command, payload) => {
    const args = (payload ?? {}) as Record<string, unknown>;
    const updateInput = args.input as { id?: string } | undefined;
    const instanceId = typeof args.instanceId === "string" ? args.instanceId : updateInput?.id ?? activeInstance;
    if (command === "read_background_jobs") return structuredClone(jobs);
    if (command === "download_steam_workshop_items") {
      const ids = [...(args.ids as string[])];
      assert(instanceId === firstInstance && ids.length === 1, "Each fixture download targets one selected Mod on its owning instance");
      downloads.push({ instance_id: instanceId, ids });
      return new Promise<SteamWorkshopDownloadResult>((resolve, reject) => {
        pending.push({ instanceId, ids, jobId: `workshop-download:${instanceId}:fixture-${pending.length + 1}`,
          finish: (failure) => {
            if (failure) { reject(new Error(failure)); return; }
            ids.forEach((id) => state(instanceId).cached.add(id));
            resolve({ consumer_app_id: 322330, install_root: "fixture-instance", workshop_root: "fixture-cache",
              items: ids.map((item_id) => ({ item_id, expected_path: `fixture-cache/${item_id}`, expected_path_exists: true })),
              output_excerpt: "Fixture download completed" });
          } });
      });
    }
    return createModWorkbenchFixtureIpc(() => ({ ...state(instanceId), moduleDetails, catalog, pageSize: 1 }), {
      check: assert,
      save: (saved) => {
        state(saved.summary.id).details = saved;
        writes.push({ instance_id: saved.summary.id, settings_json: saved.settings_json });
      },
    })(command, payload);
  }, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  await act(async () => { await prepareBrowserLocaleCatalogs(); draw(); });
  await settle(() => Boolean(maybeCard(firstItem)?.querySelector(".mw-mod-quick-action:not(:disabled)")), "The initial install action did not become ready");
  await click(element<HTMLButtonElement>(".mw-mod-quick-action", card(firstItem)));
  await settle(() => pending.length === 1, "The first download did not reach its held IPC boundary");
  await tab("mine");
  await settle(() => installing(pendingRow(firstItem)), "My Mods did not immediately show the pending installation");
  assert(jobs.length === 0 && writes.length === 0 && state().cached.size === 0 && rows(firstItem).length === 1,
    "My Mods shows the named Mod exactly once before a background job, files or settings exist");
  await publishProgress(pending[0], 37);
  await settle(() => installing(pendingRow(firstItem), 37), "My Mods did not receive the current batch progress");

  await tab("store");
  await settle(() => installing(maybeCard(firstItem), 37), "The Workshop card lost its running progress");
  await click(element<HTMLButtonElement>(".mw-pager button:last-child"));
  await settle(() => Boolean(maybeCard(failedItem)) && !maybeCard(firstItem), "Workshop page two did not replace page one");
  assert(!installing(maybeCard(failedItem)), "Another Mod does not inherit the active download state");
  await publishProgress(pending[0], 58);
  await click(element<HTMLButtonElement>(".mw-pager button:first-child"));
  await settle(() => installing(maybeCard(firstItem), 58), "Returning to page one did not restore updated download progress");
  await blockDuplicate(firstItem, 1);

  await act(async () => { mounted = false; draw(); });
  assert(Boolean(fixture.querySelector("[data-fixture-away]")), "The downloading workbench can be left without ending the request");
  await publishProgress(pending[0], 71);
  await act(async () => { epoch += 1; mounted = true; draw(); });
  await settle(() => installing(maybeCard(firstItem), 71), "A newly mounted workbench did not recover the same download");
  assert(coordinators.size === 1, "Only ModWorkbench remounts while its application coordinator survives");
  await blockDuplicate(firstItem, 1);
  await tab("mine");
  await settle(() => installing(pendingRow(firstItem), 71), "My Mods did not restore the named pending row after remount");

  await switchInstance(otherInstance);
  await settle(() => Boolean(maybeCard(firstItem)?.querySelector(".mw-mod-quick-action:not(:disabled)")), "Another instance was incorrectly blocked by this download");
  assert(!installing(maybeCard(firstItem)) && !fixture.querySelector(".mw-workshop-download-row"),
    "The other instance's Workshop cards have no unrelated download progress");
  await tab("mine");
  assert(!pendingRow(firstItem) && rows(firstItem).length === 0 && state().details.settings_json === otherSettings,
    "The other instance's My Mods list and settings remain unchanged");
  await switchInstance(firstInstance);
  await settle(() => installing(maybeCard(firstItem), 71), "Returning to the owning instance did not restore progress");
  await blockDuplicate(firstItem, 1);
  await tab("mine");
  await settle(() => installing(pendingRow(firstItem), 71), "The owning instance lost its My Mods pending row");

  await publishProgress(pending[0], 100, "Completed");
  await act(async () => { pending[0].finish(); });
  await settle(() => writes.length === 1 && rows(firstItem).length === 1 && !pendingRow(firstItem) &&
    !installing(rows(firstItem)[0]), "Successful completion did not replace the pending row with one installed Mod");
  const savedSettings = state().details.settings_json;
  assert(state().cached.has(firstItem) && writes[0].instance_id === firstInstance && savedSettings.includes(firstItem),
    "Success persists the selected instance's Mod after the real save workflow completes");
  const successNoticeClose = fixture.querySelector<HTMLButtonElement>(".shell-activity-notice.is-success .shell-activity-notice-close");
  if (successNoticeClose) await click(successNoticeClose);

  state().cached.delete(firstItem);
  await leaveAndReturn();
  await settle(() => Boolean(maybeCard(firstItem)?.querySelector(".mw-chip--lifecycle-pending-download")),
    "A historical successful download hid a fresh missing-file inspection");
  assert(writes.length === 1 && downloads.length === 1,
    "Reopening inspects actual files without replaying a past installation");
  state().cached.add(firstItem);
  await leaveAndReturn();

  await tab("store");
  await settle(() => Boolean(maybeCard(firstItem)), "Workshop did not return after successful installation");
  await click(element<HTMLButtonElement>(".mw-pager button:last-child"));
  await settle(() => Boolean(maybeCard(failedItem)?.querySelector(".mw-mod-quick-action:not(:disabled)")), "The second Mod did not become installable");
  await click(element<HTMLButtonElement>(".mw-mod-quick-action", card(failedItem)));
  await settle(() => pending.length === 2, "The second download did not reach its held IPC boundary");
  await publishProgress(pending[1], 24);
  await leaveAndReturn();
  await tab("mine");
  await settle(() => installing(pendingRow(failedItem), 24), "A remounted workbench did not recover the second pending Mod");
  await publishProgress(pending[1], 24, "Failed");
  await act(async () => { pending[1].finish(failureMessage); });
  await settle(() => Boolean(pendingRow(failedItem)?.querySelector(".mw-chip--warn")) && !installing(pendingRow(failedItem)) &&
    [...fixture.querySelectorAll(".shell-activity-notice.is-error")]
    .some((notice) => notice.textContent?.includes(failureMessage)), "The failed download was not reported after workbench remount");
  assert(writes.length === 1 && state().details.settings_json === savedSettings && !state().cached.has(failedItem) &&
    rows(firstItem).length === 1 && rows(failedItem).length === 1 &&
    !pendingRow(failedItem)?.querySelector(".mw-entry-enabled-toggle") && !fixture.querySelector(".shell-activity-notice.is-success"),
    "Failure keeps the successful Mod and saved settings without fabricating another installed Mod or success notice");
  await tab("store");
  await click(element<HTMLButtonElement>(".mw-pager button:last-child"));
  await settle(() => Boolean(maybeCard(failedItem)?.querySelector(".mw-mod-quick-action:not(:disabled)")), "Failure did not restore an explicit retry action");
  assert(!installing(maybeCard(failedItem)) && downloads.length === 2, "Failure clears progress without submitting an automatic retry");
  await tab("mine");
  const overflow = document.documentElement.scrollWidth > innerWidth + 1;
  assert(!overflow, "The restored My Mods layout fits the desktop viewport");
  assert(errors.length === 0 && coordinators.size === 1, "The same application coordinator completes the scenario without browser errors");
  return { status: "passed", locale, checks, downloads, writes: writes.length, coordinator_count: coordinators.size,
    pending_visible_before_job: true, page_progress_restored: true, remount_progress_restored: true,
    instance_progress_isolated: true, duplicate_download_blocked: true, completion_deduplicated: true,
    failure_preserved: true, fresh_inventory_authoritative: true, document_overflow_x: overflow, browser_errors: errors };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error("Workshop download-progress browser regression exceeded 45 seconds")), 45000);
})]).catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`,
    { method: "POST", body: JSON.stringify(report) }));
