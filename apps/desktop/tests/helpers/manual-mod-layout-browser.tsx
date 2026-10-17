import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { readInstanceDetails } from "../../src/api";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { I18nProvider } from "../../src/i18n";
import { ModWorkbench } from "../../src/views/servers/ModWorkbench";
import { ServerDetailTabs } from "../../src/views/servers/ServerDetailTabs";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceDetails, ManualModStageResult, ModuleDetails } from "../../src/types";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

// Module contracts come from the repository manifests. IPC alone is substituted;
// file drops and installation results never reach the filesystem or real games.
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "en-US");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:24px;box-sizing:border-box;display:grid;grid-template-rows:minmax(0,1fr) auto";
const root = createRoot(fixture);
const errors: string[] = [];
const failures: string[] = [];
const checks: string[] = [];
const measurements: Record<string, unknown>[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
let details: InstanceDetails;
let moduleDetails: ModuleDetails;
let epoch = 0;
let writes = 0;
let inventoryReads = 0;
let installed = false, sourceOpens = 0;
let stageGate: { promise: Promise<ManualModStageResult>; resolve: (value: ManualModStageResult) => void } | null = null;

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}
function check(condition: unknown, label: string) {
  checks.push(label);
  if (!condition) failures.push(label);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const found = fixture.querySelector<T>(selector);
  assert(found, `Missing ${selector}`);
  return found;
}
async function settle(predicate: () => boolean, label: string) {
  const deadline = performance.now() + 6000;
  while (!predicate()) {
    assert(performance.now() < deadline, `${label}: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
function Harness() {
  const [noticeTarget, setNoticeTarget] = useState<HTMLDivElement | null>(null);
  return <ActivityNoticeTarget.Provider value={{ element: noticeTarget, dismissLabel: "Close" }}>
    <div className="detail-stack detail-stack--server">
      <div className="server-detail-subheader"><ServerDetailTabs
        tabs={[{ id: "settings", label: "Settings", icon: "settings" }, { id: "mods", label: "Mods", icon: "package" }]}
        activeTab="mods" panelId="manual-mod-panel" label="Instance detail sections" onSelect={() => {}} />
      </div>
      <div id="manual-mod-panel" className="server-detail-scroll server-detail-scroll--mods" role="tabpanel">
        <ModWorkbench key={epoch} details={details} moduleDetails={moduleDetails} launchPlan={null}
          onSaveSettings={async () => { throw new Error("Unexpected settings write in layout fixture"); }} />
      </div>
    </div>
    <footer className="shell-activity-bar"><span className="shell-activity-label">Activity</span>
      <div className="shell-activity-notices" ref={setNoticeTarget} />
    </footer>
  </ActivityNoticeTarget.Provider>;
}
async function render(moduleId: string, options: { running?: boolean; longNote?: boolean } = {}) {
  moduleDetails = buildMockModuleDetails(moduleId);
  assert(moduleDetails.summary.id === moduleId && moduleDetails.mods?.manual_staging,
    `Repository manual Mod contract missing for ${moduleId}`);
  if (options.longNote && moduleDetails.mods.source) {
    moduleDetails = structuredClone(moduleDetails);
    moduleDetails.mods!.source!.install_note = `${moduleDetails.mods!.source!.install_note ?? "Installation guidance."} `.repeat(12);
  }
  details = { ...details, summary: { ...details.summary, id: "fixture-manual-mods", module_id: moduleId,
    status: options.running ? "Running" : "Stopped", active_process_count: options.running ? 1 : 0 },
    active_run: null, settings_json: "{}" };
  const readsBefore = inventoryReads;
  installed = false;
  epoch += 1;
  await act(async () => {
    root.render(<I18nProvider><InstanceSettingsSaveProvider key={epoch}><Harness /></InstanceSettingsSaveProvider></I18nProvider>);
  });
  await settle(() => inventoryReads > readsBefore && Boolean(fixture.querySelector(".mw-dropzone")), "Manual workspace did not mount");
}
function visibleIn(target: HTMLElement, container: HTMLElement) {
  const rect = target.getBoundingClientRect();
  const bounds = container.getBoundingClientRect();
  return rect.width > 0 && rect.height > 0 && rect.left >= bounds.left - 1 && rect.right <= bounds.right + 1 &&
    rect.top >= Math.max(0, bounds.top) - 1 && rect.bottom <= Math.min(innerHeight, bounds.bottom) + 1;
}
function layout(state: string, hasReferences: boolean) {
  const outer = element(".server-detail-scroll--mods");
  const workbench = element(".mw-workbench");
  const workspace = element(".mw-community-workspace");
  const toolbar = element(".mw-community-workspace > .mw-workspace-toolbar");
  const pane = element(".mw-config-pane");
  const drop = element(".mw-dropzone");
  const bounds = pane.getBoundingClientRect();
  const dropBounds = drop.getBoundingClientRect();
  const listBounds = element(".mw-detail-layout").getBoundingClientRect();
  const label = `${moduleDetails.summary.id}/${state}`;
  check(!fixture.querySelector(".mw-install-help-button, .mw-install-help-dialog, .mw-install-note"), `${label}: installation notes and their entry are absent`);
  check(drop.classList.contains("mw-detail-list-col") && pane.contains(drop), `${label}: the actual Mod list receives drops`);
  check(!fixture.querySelector(".mw-dropzone-copy,.mw-dropzone-title,.mw-dropzone-icon,.mw-dropzone-open-btn,.mw-manual-pane--has-dropzone"),
    `${label}: permanent drag guidance is absent`);
  check(bounds.top >= toolbar.getBoundingClientRect().bottom - 1 && Math.abs(bounds.bottom - workspace.getBoundingClientRect().bottom) <= 2,
    `${label}: the Mod list fills the workspace below the existing controls`);
  check(dropBounds.height >= 120 && Math.abs(dropBounds.height - listBounds.height) <= 3 && listBounds.bottom <= bounds.bottom + 1,
    `${label}: the full list stays visible and usable as its drop target`);
  check(Boolean(fixture.querySelector(".mw-dropzone-feedback")) === state.includes("drag-active"), `${label}: drag guidance exists only during a valid drag`);
  check(outer.scrollHeight <= outer.clientHeight + 1 && workbench.scrollHeight <= workbench.clientHeight + 1,
    `${label}: outer workspace has no vertical overflow`);
  outer.scrollTop = 100;
  workbench.scrollTop = 100;
  check(outer.scrollTop === 0 && workbench.scrollTop === 0, `${label}: outer workspace remains at its original scroll position`);
  check(document.documentElement.scrollWidth <= innerWidth + 1 && document.documentElement.scrollHeight <= innerHeight + 1,
    `${label}: workspace stays within the desktop viewport`);
  if (fixture.querySelector(".mw-manual-actions")) check(visibleIn(element(".mw-source-open-btn"), workspace), `${label}: source access remains visible outside the list`);
  if (hasReferences) {
    check(visibleIn(element(".mw-reference-row"), workspace), `${label}: keyboard reference controls remain visible`);
  } else check(!fixture.querySelector(".mw-reference-row"), `${label}: file-only source does not display unsupported reference controls`);
  measurements.push({ module: moduleDetails.summary.id, state, pane_height: bounds.height, drop_height: dropBounds.height,
    list_height: listBounds.height, pane_scroll_height: pane.scrollHeight, pane_client_height: pane.clientHeight,
    outer_scroll_height: outer.scrollHeight, outer_client_height: outer.clientHeight,
    workbench_scroll_height: workbench.scrollHeight, workbench_client_height: workbench.clientHeight,
    reference_visible: !hasReferences || visibleIn(element(".mw-reference-row"), workspace) });
}
function stageResult(): ManualModStageResult {
  return { instance_id: details.summary.id, module_id: details.summary.module_id,
    source_label: moduleDetails.mods!.source!.label, target_label: moduleDetails.mods!.manual_staging!.target_label,
    target_path: "fixture://manual-mods/target", affected_root_names: ["FixtureMod"],
    items: [], copied_file_count: 3, copied_total_bytes: 4096 };
}
function fileTransfer(): DataTransfer {
  const file = new File(["isolated fixture"], "example.zip");
  Object.defineProperty(file, "path", { value: "fixture://manual-mods/example.zip" });
  const dataTransfer = new DataTransfer();
  // Synthetic browser events must provide the external file source's operation mask.
  Object.defineProperty(dataTransfer, "effectAllowed", { value: "copyLink" });
  dataTransfer.items.add(file);
  return dataTransfer;
}
async function dropFiles() {
  await act(async () => { element(".mw-dropzone").dispatchEvent(new DragEvent("drop", {
    bubbles: true, cancelable: true, dataTransfer: fileTransfer()
  })); });
}
async function inputReference() {
  await act(async () => {
    const target = element<HTMLInputElement>(".mw-reference-input");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(target, "https://thunderstore.io/c/valheim/p/FixtureAuthor/FixturePlugin/");
    target.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => { element<HTMLInputElement>(".mw-reference-input").dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true })); });
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  details = await readInstanceDetails("srv-dst-terminal-error");
  mockWindows("manual-layout-fixture");
  mockIPC(async (command, args) => {
    switch (command) {
      case "read_manual_mod_inventory":
        inventoryReads += 1;
        return { instance_id: details.summary.id, module_id: details.summary.module_id,
          source_label: moduleDetails.mods!.source!.label, target_label: moduleDetails.mods!.manual_staging!.target_label,
          target_path: "fixture://manual-mods/target", target_exists: installed, items: installed ? [{ name: "FixtureMod", path: "fixture://manual-mods/target/FixtureMod",
            item_type: "directory", inferred_id: null, file_count: 3, total_bytes: 4096 }] : [] };
      case "stage_manual_mod_files":
      case "install_manual_mod_references":
        assert(args && typeof args === "object" && "instanceId" in args && args.instanceId === "fixture-manual-mods",
          "Installation must remain scoped to the isolated fixture");
        if (command === "install_manual_mod_references") {
          assert(moduleDetails.summary.id === "valheim" && "references" in args && Array.isArray(args.references) &&
            args.references.length === 1 && args.references[0] === "https://thunderstore.io/c/valheim/p/FixtureAuthor/FixturePlugin/",
          "Online installation uses the supported Thunderstore contract");
        }
        writes += 1;
        if (stageGate) { const result = await stageGate.promise; installed = true; return result; }
        installed = true; return stageResult();
      case "open_external_url": sourceOpens += 1; return;
      case "read_background_jobs": return [];
      default: throw new Error(`Unexpected native command ${command}`);
    }
  }, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });

  await render("sevendaystodie");
  layout("empty", false);
  await act(async () => { element(".mw-dropzone").dispatchEvent(new DragEvent("dragenter", {
    bubbles: true, cancelable: true, dataTransfer: fileTransfer()
  })); });
  check(element(".mw-dropzone").classList.contains("mw-dropzone--active"), "File drag enters the real drop target");
  layout("drag-active", false);
  await act(async () => { element(".mw-dropzone").dispatchEvent(new DragEvent("dragleave", {
    bubbles: true, relatedTarget: element(".mw-detail-panel")
  })); });
  check(Boolean(fixture.querySelector(".mw-dropzone-feedback")), "Moving across children keeps the list drag feedback");
  await act(async () => { element(".mw-dropzone").dispatchEvent(new DragEvent("dragleave", { bubbles: true, relatedTarget: fixture })); });
  check(!fixture.querySelector(".mw-dropzone-feedback"), "Leaving the list removes its temporary feedback");
  await act(async () => {
    element(".mw-dropzone").dispatchEvent(new DragEvent("dragenter", { bubbles: true, cancelable: true, dataTransfer: fileTransfer() }));
  });
  await act(async () => { window.dispatchEvent(new DragEvent("dragend")); });
  check(!fixture.querySelector(".mw-dropzone-feedback"), "Cancelling a drag removes temporary list feedback");
  await dropFiles();
  await settle(() => Boolean(fixture.querySelector(".mw-manual-success-bar")), "File installation result did not render");
  await settle(() => fixture.textContent?.includes("FixtureMod") === true, "The imported file did not appear in the real inventory list");
  layout("installed", false);

  await render("minecraft");
  layout("empty", false);
  await dropFiles();
  await settle(() => Boolean(fixture.querySelector(".mw-manual-success-bar")), "Minecraft file installation result did not render");
  layout("installed", false);

  await render("valheim");
  layout("empty", true);
  await act(async () => { element<HTMLButtonElement>(".mw-source-open-btn").click(); });
  check(sourceOpens === 1, "The existing community source action remains connected");
  await inputReference();
  await settle(() => Boolean(fixture.querySelector(".mw-manual-success-bar")), "Thunderstore installation result did not render");
  layout("reference-installed", true);
  await render("valheim");
  let finishStage!: (value: ManualModStageResult) => void;
  stageGate = { promise: new Promise((resolve) => { finishStage = resolve; }), resolve: (value) => finishStage(value) };
  await inputReference();
  await settle(() => element(".mw-dropzone").classList.contains("mw-dropzone--running"), "Pending installation did not render");
  layout("installing", true);
  check(fixture.textContent?.includes("Installing..."), "Online loader and plugin preparation still shows installation progress");
  check(element<HTMLInputElement>(".mw-reference-input").disabled, "Pending installation disables reference input");
  const busyWrites = writes; await dropFiles();
  check(writes === busyWrites && !fixture.querySelector(".mw-dropzone-feedback"), "A busy list rejects a concurrent file drop without showing a false drop invitation");
  await act(async () => { stageGate!.resolve(stageResult()); });
  stageGate = null;
  await settle(() => Boolean(fixture.querySelector(".mw-manual-success-bar")), "Pending installation did not finish");
  layout("installed", true);

  await render("vrising", { longNote: true });
  layout("long-install-note", true);

  await render("astroneer");
  layout("loader-prerequisite", false);

  await render("minecraft", { running: true });
  layout("server-running", false);
  check(element(".mw-dropzone").getAttribute("aria-disabled") === "true", "Running Minecraft keeps file import disabled");
  const beforeMinecraft = writes;
  await dropFiles();
  check(writes === beforeMinecraft, "Dropping onto running Minecraft does not dispatch an installation");

  await render("valheim", { running: true });
  layout("server-running", true);
  check(element(".mw-dropzone").getAttribute("aria-disabled") === "true" && element<HTMLInputElement>(".mw-reference-input").disabled &&
    element<HTMLButtonElement>(".mw-reference-row button").disabled, "Running server keeps drag and reference controls disabled");
  const before = writes;
  await dropFiles();
  check(writes === before, "Dropping onto a running instance does not dispatch an installation");

  if (innerWidth < 1000) {
    await render("vrising", { longNote: true });
    await act(async () => { [...fixture.querySelectorAll<HTMLButtonElement>('[role="tab"]')].find((button) => button.textContent === "My Mods")!.click(); });
    layout("final-my-mods", false);
  } else {
    await render("astroneer");
    await act(async () => { [...fixture.querySelectorAll<HTMLButtonElement>('[role="tab"]')].find((button) => button.textContent === "My Mods")!.click(); });
    layout("final-my-mods", false);
  }
  check(!fixture.querySelector(".mw-manual-actions") && Boolean(fixture.querySelector(".mw-dropzone .mw-empty")), "My Mods is a clean empty list that remains a file drop target");
  const finalWrites = writes; await dropFiles();
  await settle(() => writes === finalWrites + 1 && fixture.textContent?.includes("FixtureMod") === true, "Dropping directly into empty My Mods did not refresh its list");
  layout("final-imported-my-mods", false);
  await act(async () => { element(".mw-dropzone").dispatchEvent(new DragEvent("dragenter", {
    bubbles: true, cancelable: true, dataTransfer: fileTransfer()
  })); });
  layout("final-drag-active", false);
  check(element(".mw-dropzone-feedback").textContent?.includes("Drop downloaded mods here"), "Local files receive a file-import hint even when the provider also supports links");
  check(errors.length === 0, "Browser console and uncaught errors remain empty");
  // Report a completed fixture so the runner captures evidence before the Node
  // test evaluates the separately collected geometry failures.
  return { status: "passed", checks, failures, measurements, writes, browser_errors: errors,
    viewport: { width: innerWidth, height: innerHeight } };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error("Manual Mod layout acceptance exceeded 40 seconds")), 40000);
})]).catch((error) => ({ status: "failed", error: String(error), failures, measurements, browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`,
    { method: "POST", body: JSON.stringify(report) }));
