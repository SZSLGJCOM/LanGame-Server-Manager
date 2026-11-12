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
const firstItem = "1111111", failedItem = "2222222", lastItem = "3333333";
const catalog = [workshopItem(firstItem, "Navigation Progress Mod"), workshopItem(failedItem, "Rejected Download Mod"),
  workshopItem(lastItem, "Following Queued Mod")];
const checks: string[] = [], errors: string[] = [];
const downloads: { instance_id: string; ids: string[] }[] = [];
const commits: { instance_id: string; ids: string[] }[] = [];
const attemptTokens = new Set<string>();
const writes: { instance_id: string; settings_json: string }[] = [];
const coordinators = new Set<InstanceSettingsSaveCoordinator>();
const instances = new Map<string, { details: InstanceDetails; cached: Set<string> }>();
const pending: PendingDownload[] = [];
const pendingCommits: PendingCommit[] = [];
const preparedTickets = new Map<string, { instanceId: string; ids: string[] }>();
let activeInstance = firstInstance, mounted = true, epoch = 0;
let jobs: BackgroundJob[] = [];
let locale = "en-US";
let firstSaveStarted = false;
let finishFirstSave: (() => void) | undefined;
const failureMessage = "Fixture Workshop download failed before any files were installed";

interface PendingDownload {
  instanceId: string;
  ids: string[];
  jobId: string;
  ticketId: string;
  finish: (failure?: string) => void;
}
interface PendingCommit {
  instanceId: string;
  ids: string[];
  ticketId: string;
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
function queued(node: HTMLElement | null | undefined) {
  const chip = node?.querySelector(".mw-chip--lifecycle-queued");
  return chip?.textContent?.trim() === (locale === "zh-CN" ? "等待下载" : "Queued") &&
    !installing(node) && !node?.querySelector('[role="progressbar"]') && !chip.textContent.includes("%");
}
function ready(node: HTMLElement | null | undefined) {
  const chip = node?.querySelector(".mw-chip--lifecycle-ready");
  return chip?.textContent?.trim() === (locale === "zh-CN" ? "等待部署" : "Ready to install") &&
    !installing(node) && !node?.querySelector('[role="progressbar"]') && !chip.textContent.includes("%");
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
async function showCatalogItem(itemId: string) {
  await tab("store");
  const targetIndex = catalog.findIndex((item) => item.id === itemId);
  for (let step = 0; step < catalog.length && !maybeCard(itemId); step++) {
    const currentIndex = catalog.findIndex((item) => maybeCard(item.id));
    assert(currentIndex >= 0 && targetIndex >= 0, "Catalog navigation starts from a known Mod page");
    const direction = currentIndex < targetIndex ? 1 : -1;
    await click(element<HTMLButtonElement>(`.mw-pager button:${direction === 1 ? "last" : "first"}-child`));
    await settle(() => Boolean(maybeCard(catalog[currentIndex + direction].id)), "The requested Workshop page did not load");
  }
  assert(Boolean(maybeCard(itemId)), "Catalog navigation reaches the requested Mod");
}
function Workspace() {
  const coordinator = useInstanceSettingsSaveCoordinator();
  const [noticeTarget, setNoticeTarget] = useState<HTMLDivElement | null>(null);
  coordinators.add(coordinator);
  return <ActivityNoticeTarget.Provider value={{ element: noticeTarget, dismissLabel: "Close" }}>
    <div style={{ flex: 1, minHeight: 0 }} data-fixture-instance={activeInstance}>
      {mounted ? <ModWorkbench key={`${activeInstance}:${epoch}`} details={state().details} moduleDetails={moduleDetails}
        jobs={jobs} launchPlan={null} onSaveSettings={async (input, options) => {
          if (!firstSaveStarted) {
            firstSaveStarted = true;
            await new Promise<void>((resolve) => { finishFirstSave = resolve; });
          }
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
  assert(action.disabled, "An accepted active or queued download disables another install of the same Mod");
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
    if (command === "prepare_steam_workshop_items") {
      const ids = [...(args.ids as string[])];
      assert(instanceId === firstInstance && ids.length === 1, "Each fixture download targets one selected Mod on its owning instance");
      assert(typeof args.attemptToken === "string" && args.attemptToken.length > 0 && !attemptTokens.has(args.attemptToken),
        "Every concurrent preparation and retry uses a distinct nonempty attempt token");
      attemptTokens.add(args.attemptToken);
      downloads.push({ instance_id: instanceId, ids });
      const ticketId = `fixture-workshop-ticket-${pending.length + 1}`;
      return new Promise<{ ticket_id: string; consumer_app_id: number }>((resolve, reject) => {
        pending.push({ instanceId, ids, ticketId, jobId: `workshop-download:${instanceId}:${args.attemptToken}`,
          finish: (failure) => {
            if (failure) { reject(new Error(failure)); return; }
            preparedTickets.set(ticketId, { instanceId, ids });
            resolve({ ticket_id: ticketId, consumer_app_id: 322330 });
          } });
      });
    }
    if (command === "commit_steam_workshop_items") {
      const ticketId = String(args.ticketId);
      const prepared = preparedTickets.get(ticketId);
      assert(prepared?.instanceId === instanceId, "Commit consumes a prepared ticket belonging to its instance");
      const ids = [...prepared.ids];
      preparedTickets.delete(ticketId);
      commits.push({ instance_id: instanceId, ids });
      return new Promise<SteamWorkshopDownloadResult>((resolve, reject) => {
        pendingCommits.push({ instanceId, ids, ticketId, finish: (failure) => {
            if (failure) { reject(new Error(failure)); return; }
            ids.forEach((id) => state(instanceId).cached.add(id));
            resolve({ consumer_app_id: 322330, install_root: "fixture-instance", workshop_root: "fixture-cache",
              items: ids.map((item_id) => ({ item_id, expected_path: `fixture-cache/${item_id}`, expected_path_exists: true })),
              output_excerpt: "Fixture preparation committed" });
          } });
      });
    }
    if (command === "discard_steam_workshop_preparation") {
      const ticketId = String(args.ticketId);
      const prepared = preparedTickets.get(ticketId);
      assert(!prepared || prepared.instanceId === instanceId, "Discard remains scoped to the prepared ticket's instance");
      preparedTickets.delete(ticketId);
      return;
    }
    if (command === "download_steam_workshop_items") throw new Error("Anonymous Workshop downloads must use isolated preparation and commit");
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
  await click(element<HTMLButtonElement>(".mw-mod-quick-action", card(failedItem)));
  await settle(() => pending.length === 2 && installing(maybeCard(failedItem)), "The second Mod did not start a concurrent isolated preparation");
  await publishProgress(pending[1], 24);
  await settle(() => installing(maybeCard(failedItem), 24), "The second Mod did not receive its own preparation progress");
  await blockDuplicate(failedItem, 2);
  await showCatalogItem(lastItem);
  await click(element<HTMLButtonElement>(".mw-mod-quick-action", card(lastItem)));
  await settle(() => queued(maybeCard(lastItem)), "The third Mod did not enter the visible download queue");
  await blockDuplicate(lastItem, 2);
  await tab("mine");
  await settle(() => installing(pendingRow(firstItem), 37) && installing(pendingRow(failedItem), 24) && queued(pendingRow(lastItem)),
    "My Mods did not distinguish both concurrent download percentages from the queued Mod");
  assert(rows(firstItem).length === 1 && rows(failedItem).length === 1 && rows(lastItem).length === 1 && downloads.length === 2 &&
    commits.length === 0 && writes.length === 0 && state().cached.size === 0,
    "Two preparations run concurrently without early deployment, settings changes, or admitting a third request");
  let startBarrierSettled = false;
  let startBarrierError: unknown;
  const coordinator = [...coordinators][0];
  const startBarrier = coordinator.flush(firstInstance).then(() => { startBarrierSettled = true; }, (error) => {
    startBarrierSettled = true; startBarrierError = error;
  });
  await publishProgress(pending[1], 100, "Completed");
  await act(async () => { pending[1].finish(); });
  await settle(() => ready(pendingRow(failedItem)), "The faster second Mod did not wait visibly for deployment");
  assert(downloads.length === 2 && commits.length === 0 && writes.length === 0 && state().cached.size === 0 &&
    queued(pendingRow(lastItem)) && !startBarrierSettled,
    "A ready second Mod retains its full request slot and cannot deploy ahead of the first Mod");
  await publishProgress(pending[0], 58);
  await showCatalogItem(firstItem);
  await settle(() => installing(maybeCard(firstItem), 58), "Returning to page one did not restore updated download progress");
  await blockDuplicate(firstItem, 2);

  await act(async () => { mounted = false; draw(); });
  assert(Boolean(fixture.querySelector("[data-fixture-away]")), "The downloading workbench can be left without ending the request");
  await publishProgress(pending[0], 71);
  await act(async () => { epoch += 1; mounted = true; draw(); });
  await settle(() => installing(maybeCard(firstItem), 71), "A newly mounted workbench did not recover the same download");
  assert(coordinators.size === 1, "Only ModWorkbench remounts while its application coordinator survives");
  await blockDuplicate(firstItem, 2);
  await tab("mine");
  await settle(() => installing(pendingRow(firstItem), 71), "My Mods did not restore the named pending row after remount");
  assert(ready(pendingRow(failedItem)) && queued(pendingRow(lastItem)), "Workbench remount preserves ready and queued requests separately");

  await switchInstance(otherInstance);
  await settle(() => Boolean(maybeCard(firstItem)?.querySelector(".mw-mod-quick-action:not(:disabled)")), "Another instance was incorrectly blocked by this download");
  assert(!installing(maybeCard(firstItem)) && !fixture.querySelector(".mw-workshop-download-row"),
    "The other instance's Workshop cards have no unrelated download progress");
  await tab("mine");
  assert(!pendingRow(firstItem) && !pendingRow(failedItem) && !pendingRow(lastItem) && rows(firstItem).length === 0 &&
    state().details.settings_json === otherSettings,
    "The other instance's My Mods list and settings remain unchanged");
  await switchInstance(firstInstance);
  await settle(() => installing(maybeCard(firstItem), 71), "Returning to the owning instance did not restore progress");
  await blockDuplicate(firstItem, 2);
  await tab("mine");
  await settle(() => installing(pendingRow(firstItem), 71), "The owning instance lost its My Mods pending row");
  assert(ready(pendingRow(failedItem)) && queued(pendingRow(lastItem)), "Returning to the owning instance restores its queue without borrowed progress");

  await publishProgress(pending[0], 100, "Completed");
  await act(async () => { mounted = false; draw(); pending[0].finish(); });
  await settle(() => pendingCommits.length === 1, "The first prepared Mod did not reach its ordered commit boundary");
  assert(pendingCommits[0].ids[0] === firstItem && preparedTickets.has(pending[1].ticketId),
    "The first selected Mod commits first even when the second preparation finished earlier");
  await act(async () => { pendingCommits[0].finish(); });
  await settle(() => firstSaveStarted && Boolean(finishFirstSave), "The first download did not reach the held settings write");
  assert(downloads.length === 2 && commits.length === 1 && writes.length === 0 && !startBarrierSettled,
    "The next commit, third preparation, and instance startup wait until the first Mod's settings write completes");
  await act(async () => { finishFirstSave?.(); });
  await settle(() => writes.length === 1 && pending.length === 3 && pendingCommits.length === 2,
    "The second commit and third preparation did not start after the first complete request released its slot");
  assert(!mounted && pendingCommits[1].ids[0] === failedItem && pending[2].ids[0] === lastItem && !startBarrierSettled,
    "The application advances ordered deployment and bounded preparation while the workbench is unmounted");
  await act(async () => { epoch += 1; mounted = true; draw(); });
  await settle(() => Boolean(maybeCard(firstItem)), "The workbench did not recover after advancing its queue while unmounted");
  await tab("mine");
  await settle(() => writes.length === 1 && rows(firstItem).length === 1 && !pendingRow(firstItem) &&
    !installing(rows(firstItem)[0]), "Successful completion did not replace the pending row with one installed Mod");
  const savedSettings = state().details.settings_json;
  assert(state().cached.has(firstItem) && writes[0].instance_id === firstInstance && savedSettings.includes(firstItem),
    "Success persists the selected instance's Mod after the real save workflow completes");
  const successNoticeClose = fixture.querySelector<HTMLButtonElement>(".shell-activity-notice.is-success .shell-activity-notice-close");
  if (successNoticeClose) await click(successNoticeClose);
  await settle(() => Boolean(pendingRow(failedItem)?.querySelector(".mw-chip--lifecycle-applying")) && installing(pendingRow(lastItem)),
    "The second Mod deploys while the third Mod prepares without sharing either lifecycle");
  await publishProgress(pending[2], 46);
  await leaveAndReturn();
  await tab("mine");
  await settle(() => Boolean(pendingRow(failedItem)?.querySelector(".mw-chip--lifecycle-applying")) && installing(pendingRow(lastItem), 46),
    "A remounted workbench did not recover the separate applying and preparing requests");
  await act(async () => { pendingCommits[1].finish(failureMessage); });
  await settle(() => Boolean(pendingRow(failedItem)?.querySelector(".mw-chip--warn")) && !installing(pendingRow(failedItem)) &&
    [...fixture.querySelectorAll(".shell-activity-notice.is-error")]
    .some((notice) => notice.textContent?.includes(failureMessage)), "The failed download was not reported after workbench remount");
  await settle(() => pending.length === 3 && installing(pendingRow(lastItem), 46), "A failed deployment interrupted the other active preparation");
  await startBarrier;
  assert(startBarrierSettled && String(startBarrierError).includes(failureMessage),
    "The instance-start barrier preserves a queued download failure instead of reporting success");
  assert(writes.length === 1 && state().details.settings_json === savedSettings && !state().cached.has(failedItem) &&
    rows(firstItem).length === 1 && rows(failedItem).length === 1 &&
    !pendingRow(failedItem)?.querySelector(".mw-entry-enabled-toggle") && !fixture.querySelector(".shell-activity-notice.is-success"),
    "Failure keeps the successful Mod and saved settings without fabricating another installed Mod or success notice");
  assert(pending[2].ids[0] === lastItem && !pendingRow(lastItem)?.textContent?.includes("24%"),
    "The following queue entry has its own native request and never borrows the failed Mod's progress");
  await publishProgress(pending[2], 100, "Completed");
  await act(async () => { pending[2].finish(); });
  await settle(() => pendingCommits.length === 3, "The following prepared Mod did not commit after the previous deployment failed");
  assert(pendingCommits[2].ids[0] === lastItem, "Ordered deployment skips the failed request and continues with the next selected Mod");
  await act(async () => { pendingCommits[2].finish(); });
  await settle(() => writes.length === 2 && !pendingRow(lastItem) && rows(lastItem).length === 1,
    "The following download did not complete after an earlier queue failure");
  const settingsAfterQueue = state().details.settings_json;
  assert(settingsAfterQueue.includes(firstItem) && settingsAfterQueue.includes(lastItem) && !settingsAfterQueue.includes(failedItem) &&
    Boolean(pendingRow(failedItem)?.querySelector(".mw-chip--warn")),
    "Later success preserves earlier installed Mods and the failed queue entry without enabling a failed Mod");

  state().cached.delete(firstItem);
  await leaveAndReturn();
  await settle(() => Boolean(maybeCard(firstItem)?.querySelector(".mw-chip--lifecycle-pending-download")),
    "A historical successful download hid a fresh missing-file inspection");
  assert(writes.length === 2 && downloads.length === 3,
    "Reopening inspects actual files without replaying past installations or retrying failures");
  state().cached.add(firstItem);
  await leaveAndReturn();
  await showCatalogItem(failedItem);
  await settle(() => Boolean(maybeCard(failedItem)?.querySelector(".mw-mod-quick-action:not(:disabled)")), "Failure did not restore an explicit retry action");
  assert(!installing(maybeCard(failedItem)) && downloads.length === 3, "Failure clears progress without submitting an automatic retry");
  const failureNotice = [...fixture.querySelectorAll<HTMLElement>(".shell-activity-notice.is-error")]
    .find((notice) => notice.textContent?.includes("Rejected Download Mod") && notice.textContent.includes(failureMessage));
  assert(failureNotice, "The failed Mod has its own named failure notice after navigation");
  const retry = [...failureNotice.querySelectorAll<HTMLButtonElement>("button")]
    .find((button) => button.textContent?.trim() === (locale === "zh-CN" ? "重试" : "Retry"));
  assert(retry, "The failed request exposes a dedicated retry action");
  await click(retry);
  await settle(() => pending.length === 4 && installing(maybeCard(failedItem)), "The explicit retry did not start a fresh request");
  await tab("mine");
  assert(rows(failedItem).length === 1 && !pendingRow(failedItem)?.querySelector(".mw-chip--warn"),
    "Retry replaces the failed queue entry instead of duplicating it");
  await publishProgress(pending[3], 100, "Completed");
  await act(async () => { pending[3].finish(); });
  await settle(() => pendingCommits.length === 4, "The explicitly retried preparation did not receive a new commit");
  assert(pendingCommits[3].ids[0] === failedItem && pending[3].jobId !== pending[1].jobId &&
    pendingCommits[3].ticketId !== pendingCommits[1].ticketId,
    "Retry keeps the failed Mod's intent while receiving fresh progress identity and preparation ticket");
  await act(async () => { pendingCommits[3].finish(); });
  await settle(() => writes.length === 3 && !pendingRow(failedItem) && rows(failedItem).length === 1,
    "The explicit retry did not complete into one installed Mod");
  await coordinator.flush(firstInstance);
  assert([firstItem, failedItem, lastItem].every((id) => state().details.settings_json.includes(id) && state().cached.has(id) && rows(id).length === 1) &&
    !fixture.querySelector(".mw-workshop-download-row") && preparedTickets.size === 0,
    "All successful queue items accumulate in the current configuration and a successful retry clears the start barrier");
  await tab("mine");
  const overflow = document.documentElement.scrollWidth > innerWidth + 1;
  assert(!overflow, "The restored My Mods layout fits the desktop viewport");
  assert(errors.length === 0 && coordinators.size === 1, "The same application coordinator completes the scenario without browser errors");
  return { status: "passed", locale, checks, downloads, commits, attempt_tokens: attemptTokens.size,
    writes: writes.length, coordinator_count: coordinators.size,
    pending_visible_before_job: true, page_progress_restored: true, remount_progress_restored: true,
    instance_progress_isolated: true, duplicate_download_blocked: true, completion_deduplicated: true,
    failure_preserved: true, fresh_inventory_authoritative: true, queue_accepts_while_running: true,
    queued_progress_isolated: true, queue_survives_navigation: true, queue_waits_for_settings: true,
    queue_advances_unmounted: true, queue_continues_after_failure: true, queue_settings_accumulate: true,
    queued_start_barrier: true, explicit_retry_recovers: true, failure_notice_retry: true,
    concurrent_preparation: true, independent_progress: true, ready_waits_for_fifo: true,
    bounded_complete_requests: true, prepared_files_stay_private: true, ordered_commits: true,
    fresh_retry_identity: true, prepared_tickets_consumed: true,
    document_overflow_x: overflow, browser_errors: errors };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error("Workshop download-progress browser regression exceeded 45 seconds")), 45000);
})]).catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`,
    { method: "POST", body: JSON.stringify(report) }));
