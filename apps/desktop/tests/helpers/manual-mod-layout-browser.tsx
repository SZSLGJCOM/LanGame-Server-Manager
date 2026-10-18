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
import type { InstanceArchiveDetails } from "../../src/storage-management-types";
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
let archived = false, settingsWrites = 0, referenceResolves = 0;
let nextStageError: string | null = null;
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
  const [, refresh] = useState(0);
  const archive: InstanceArchiveDetails | undefined = archived ? { archive_id: "fixture-archive", instance: details,
    maintenance: { autostart: false, auto_backup_on_stop: false, backup_retention_count: 3, crash_restart_limit: null, runtime_mode: null },
    runs: { entries: [], total: 0, truncated: false }, log: { relative_path: null, text: "", truncated: false, issues: [] },
    backups: { entries: [], issues: [], truncated: false } } : undefined;
  return <ActivityNoticeTarget.Provider value={{ element: noticeTarget, dismissLabel: "Close" }}>
    <div className="detail-stack detail-stack--server">
      <div className="server-detail-subheader"><ServerDetailTabs
        tabs={[{ id: "settings", label: "Settings", icon: "settings" }, { id: "mods", label: "Mods", icon: "package" }]}
        activeTab="mods" panelId="manual-mod-panel" label="Instance detail sections" onSelect={() => {}} />
      </div>
      <div id="manual-mod-panel" className="server-detail-scroll server-detail-scroll--mods" role="tabpanel">
        <ModWorkbench key={epoch} details={details} moduleDetails={moduleDetails} launchPlan={null} archive={archive}
          onSaveSettings={async (input) => {
            assert(details.summary.module_id === "arksurvivalascended" && !archived, "Only the ASA reference scenario may save fixture settings");
            settingsWrites += 1; details = { ...details, settings_json: input.settings_json };
            refresh((value) => value + 1);
          }} />
      </div>
    </div>
    <footer className="shell-activity-bar"><span className="shell-activity-label">Activity</span>
      <div className="shell-activity-notices" ref={setNoticeTarget} />
    </footer>
  </ActivityNoticeTarget.Provider>;
}
async function render(moduleId: string, options: { running?: boolean; longNote?: boolean; readOnly?: boolean } = {}) {
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
  archived = Boolean(options.readOnly);
  installed = false;
  epoch += 1;
  await act(async () => {
    root.render(<I18nProvider><InstanceSettingsSaveProvider key={epoch}><Harness /></InstanceSettingsSaveProvider></I18nProvider>);
  });
  await settle(() => archived ? Boolean(fixture.querySelector(".mw-community-workspace"))
    : inventoryReads > readsBefore && Boolean(fixture.querySelector(".mw-dropzone")), "Manual workspace did not mount");
}
function visibleIn(target: HTMLElement, container: HTMLElement) {
  const rect = target.getBoundingClientRect();
  const bounds = container.getBoundingClientRect();
  return rect.width > 0 && rect.height > 0 && rect.left >= bounds.left - 1 && rect.right <= bounds.right + 1 &&
    rect.top >= Math.max(0, bounds.top) - 1 && rect.bottom <= Math.min(innerHeight, bounds.bottom) + 1;
}
function headerLayout(state: string, hasReferences: boolean) {
  const toolbar = element(".mw-community-workspace > .mw-workspace-toolbar"), toolbarBounds = toolbar.getBoundingClientRect();
  const actions = element(".mw-manual-toolbar-actions"), actionsBounds = actions.getBoundingClientRect();
  const label = `${moduleDetails.summary.id}/${state}`;
  check(toolbar.classList.contains("mw-manual-toolbar") && !fixture.querySelector(".mw-manual-pane > .mw-manual-actions"),
    `${label}: source controls occupy the existing header without a second action strip`);
  check(actions.parentElement === toolbar && actions.querySelector('[role="tablist"]') && visibleIn(actions, toolbar),
    `${label}: workspace tabs and source actions share the header's right edge`);
  check(fixture.querySelectorAll(".mw-reference-row").length === (hasReferences ? 1 : 0), `${label}: exactly one supported reference input exists across workspace modes`);
  if (hasReferences) {
    const row = element(".mw-reference-row"), input = element<HTMLInputElement>(".mw-reference-row input"), add = element(".mw-reference-row button");
    const rowBounds = row.getBoundingClientRect(), inputBounds = input.getBoundingClientRect(), addBounds = add.getBoundingClientRect();
    check(row.parentElement === toolbar && rowBounds.right < actionsBounds.left && inputBounds.width >= 160,
      `${label}: reference entry fills the header's available left width`);
    check(Math.abs(rowBounds.left - (toolbarBounds.left + 14)) <= 1 && Math.abs(rowBounds.right + 12 - actionsBounds.left) <= 1,
      `${label}: header input uses the standard inset and only the shared action gap`);
    check([inputBounds.height, addBounds.height].every((height) => Math.abs(height - 36) <= 1)
      && Math.abs(inputBounds.top - addBounds.top) <= 1 && Math.abs(inputBounds.top + 18 - (actionsBounds.top + actionsBounds.height / 2)) <= 1,
      `${label}: input, submit and right-side actions align in one 36px header row`);
    check(Boolean(input.getAttribute("aria-label")) && visibleIn(row, toolbar), `${label}: the visible reference entry has an accessible name`);
  } else check(element(".mw-manual-toolbar-title").parentElement === toolbar && visibleIn(element(".mw-manual-toolbar-title"), toolbar),
    `${label}: file-only source name occupies the header's left side`);
  if (fixture.querySelector(".mw-source-open-btn")) {
    const source = element(".mw-source-open-btn"), sourceBounds = source.getBoundingClientRect();
    check(visibleIn(source, toolbar) && sourceBounds.bottom <= toolbarBounds.bottom + 1, `${label}: source access stays on the header's right side`);
  }
  if (moduleDetails.mods?.source?.provider === "thunderstore") {
    const policy = element<HTMLButtonElement>(".mw-version-policy-btn"), bounds = policy.getBoundingClientRect();
    check(fixture.querySelectorAll(".mw-version-policy-btn").length === 1 && actions.contains(policy) && visibleIn(policy, toolbar),
      `${label}: one Thunderstore update-policy button stays in the shared header`);
    check(Math.abs(bounds.height - 36) <= 1 && Math.abs(bounds.top + bounds.height / 2 - actionsBounds.top - actionsBounds.height / 2) <= 1,
      `${label}: update policy shares the header control height and baseline`);
    check(!fixture.querySelector(".mw-version-policy select,.mw-version-policy-help,.mw-manual-pane > .mw-version-policy"),
      `${label}: update-policy text and controls do not consume an auxiliary row`);
  } else check(!fixture.querySelector(".mw-version-policy-btn"), `${label}: non-Thunderstore sources have no update-policy switch`);
}
function layout(state: string, hasReferences: boolean, view: "store" | "my-mods" = "store") {
  const outer = element(".server-detail-scroll--mods");
  const workbench = element(".mw-workbench");
  const workspace = element(".mw-community-workspace");
  const toolbar = element(".mw-community-workspace > .mw-workspace-toolbar");
  const pane = element(view === "store" ? ".mw-manual-pane" : ".mw-config-pane");
  const drop = element(".mw-dropzone");
  const bounds = pane.getBoundingClientRect();
  const dropBounds = drop.getBoundingClientRect();
  const listBounds = view === "my-mods" ? element(".mw-detail-layout").getBoundingClientRect() : null;
  const label = `${moduleDetails.summary.id}/${state}`;
  headerLayout(state, hasReferences);
  check(!fixture.querySelector(".mw-install-help-button, .mw-install-help-dialog, .mw-install-note"), `${label}: installation notes and their entry are absent`);
  if (view === "store") {
    check(pane.parentElement === workspace && pane.contains(drop) && !drop.classList.contains("mw-detail-list-col"),
      `${label}: the store has its own file installation target`);
    check(!workspace.querySelector(".mw-config-pane,.mw-detail-layout,.mw-entry-row,.mw-manual-inventory-row"),
      `${label}: the store never renders My Mods or inventory rows`);
    for (const selector of [".mw-dropzone-icon", ".mw-dropzone-title", ".mw-dropzone-copy"]) {
      check(Boolean(drop.querySelector(selector)) && visibleIn(element(selector), drop), `${label}: ${selector} remains visible`);
    }
    check(dropBounds.height >= 120 && visibleIn(drop, pane), `${label}: the independent drop target stays fully visible and usable`);
    check(!workspace.textContent?.includes("FixtureMod"), `${label}: imported inventory names stay out of the store`);
  } else {
    assert(listBounds, `${label}: My Mods layout bounds are available`);
    check(drop.classList.contains("mw-detail-list-col") && pane.contains(drop), `${label}: My Mods retains its actual list drop target`);
    check(!fixture.querySelector(".mw-manual-pane,.mw-dropzone-copy,.mw-dropzone-title,.mw-dropzone-icon,.mw-dropzone-open-btn"),
      `${label}: My Mods has no duplicate store or permanent drag guidance`);
    check(dropBounds.height >= 120 && Math.abs(dropBounds.height - listBounds.height) <= 3 && listBounds.bottom <= bounds.bottom + 1,
      `${label}: the full list stays visible and usable as its drop target`);
  }
  check(bounds.top >= toolbar.getBoundingClientRect().bottom - 1 && Math.abs(bounds.bottom - workspace.getBoundingClientRect().bottom) <= 2,
    `${label}: the selected pane fills the workspace below the existing controls`);
  check(Boolean(fixture.querySelector(".mw-dropzone-feedback")) === state.includes("drag-active"), `${label}: drag guidance exists only during a valid drag`);
  check(outer.scrollHeight <= outer.clientHeight + 1 && workbench.scrollHeight <= workbench.clientHeight + 1,
    `${label}: outer workspace has no vertical overflow`);
  outer.scrollTop = 100;
  workbench.scrollTop = 100;
  check(outer.scrollTop === 0 && workbench.scrollTop === 0, `${label}: outer workspace remains at its original scroll position`);
  check(document.documentElement.scrollWidth <= innerWidth + 1 && document.documentElement.scrollHeight <= innerHeight + 1,
    `${label}: workspace stays within the desktop viewport`);
  if (hasReferences) {
    check(visibleIn(element(".mw-reference-row"), workspace) && element(".mw-reference-row").getBoundingClientRect().bottom <= dropBounds.top + 1,
      `${label}: keyboard reference controls remain visible above the store drop target`);
  } else check(!fixture.querySelector(".mw-reference-row"), `${label}: file-only source does not display unsupported reference controls`);
  measurements.push({ module: moduleDetails.summary.id, state, pane_height: bounds.height, drop_height: dropBounds.height,
    view, list_height: listBounds?.height ?? null, pane_scroll_height: pane.scrollHeight, pane_client_height: pane.clientHeight,
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
async function selectView(label: "My Mods" | "Store" | "Manifest mode") {
  const target = [...fixture.querySelectorAll<HTMLButtonElement>('[role="tab"]')].find((button) => button.textContent === label);
  assert(target, `Missing ${label} tab`);
  await act(async () => { target.click(); });
}
async function editReference(reference: string) {
  await act(async () => {
    const target = element<HTMLInputElement>(".mw-reference-row input");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(target, reference);
    target.dispatchEvent(new Event("input", { bubbles: true }));
    target.focus();
  });
}
async function nativeKey(value: "Enter" | "Escape") {
  await act(async () => {
    const nonce = new URLSearchParams(location.search).get("nonce");
    const response = await fetch(`/__reliability_key/${nonce}/${value}`, { method: "POST" });
    assert(response.ok, `Native ${value} input failed`);
  });
}
async function pointerMove(target: HTMLElement) {
  const bounds = target.getBoundingClientRect();
  await act(async () => {
    const response = await fetch(`/__reliability_pointer/${new URLSearchParams(location.search).get("nonce")}`, {
      method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ type: "move", x: bounds.left + bounds.width / 2, y: bounds.top + bounds.height / 2 })
    });
    assert(response.ok, "Native policy-help hover failed");
  });
}
function visiblePolicyHelp(): HTMLElement | null {
  return [...document.querySelectorAll<HTMLElement>(".configuration-field-help-tooltip.is-visible")]
    .find((node) => node.textContent?.includes("Versioned links select this download.")
      && Number(getComputedStyle(node).opacity) >= 0.99) ?? null;
}
async function reviewPolicyHelp() {
  const policy = element<HTMLButtonElement>(".mw-version-policy-btn"), value = policy.getAttribute("data-version-policy");
  await act(async () => policy.focus());
  await settle(() => Boolean(visiblePolicyHelp()), "Keyboard focus did not show update-policy help");
  const describedBy = policy.getAttribute("aria-describedby"), description = describedBy ? document.getElementById(describedBy) : null;
  check(description?.getAttribute("role") === "tooltip" && description.textContent?.includes("Click to switch between following latest and pinning the referenced version."),
    "Update-policy button has accessible instructions for its switching behavior");
  const checkBubble = () => {
    const bubble = visiblePolicyHelp(); assert(bubble, "Visible update-policy help is required");
    const bounds = bubble.getBoundingClientRect();
    check(bounds.width > 0 && bounds.height > 0 && bounds.left >= 0 && bounds.right <= innerWidth && bounds.top >= 0 && bounds.bottom <= innerHeight,
      "Update-policy bubble is visible inside the desktop viewport");
  };
  checkBubble(); await nativeKey("Escape");
  await settle(() => !visiblePolicyHelp(), "Escape did not dismiss update-policy help");
  check(document.activeElement === policy && policy.getAttribute("data-version-policy") === value,
    "Escape dismisses only the bubble and preserves button focus and update policy");
  await act(async () => policy.blur());
  await pointerMove(element(".mw-reference-row input")); await pointerMove(policy);
  await settle(() => Boolean(visiblePolicyHelp()), "Pointer hover did not show update-policy help");
  checkBubble();
  check(policy.getAttribute("data-version-policy") === value, "Hovering update-policy help does not change the selected policy");
}
async function inputReference(reference = "https://thunderstore.io/c/valheim/p/FixtureAuthor/FixturePlugin/", submit: "Enter" | "click" = "Enter") {
  await editReference(reference);
  await act(async () => {
    const nonce = new URLSearchParams(location.search).get("nonce"), button = element(".mw-reference-row button"), bounds = button.getBoundingClientRect();
    const response = submit === "Enter" ? await fetch(`/__reliability_key/${nonce}/Enter`, { method: "POST" })
      : await fetch(`/__reliability_pointer/${nonce}`, { method: "POST", headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ type: "click", x: bounds.left + bounds.width / 2, y: bounds.top + bounds.height / 2 }) });
    assert(response.ok, `Native reference ${submit} failed`);
  });
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
      case "read_instance_details_from_storage": return structuredClone(details);
      case "resolve_manual_mod_references":
        assert(moduleDetails.summary.id === "arksurvivalascended" && args && typeof args === "object" && "references" in args
          && Array.isArray(args.references) && args.references.length === 1, "ASA reference resolution uses its existing provider contract");
        referenceResolves += 1;
        return { instance_id: details.summary.id, module_id: details.summary.module_id, source_label: "CurseForge",
          setting_key: "mod_ids_csv", setting_label: "Mod IDs", items: [], resolved_ids: [referenceResolves === 1 ? "928708" : "939713"] };
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
        if (nextStageError) { const error = nextStageError; nextStageError = null; throw new Error(error); }
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
    bubbles: true, relatedTarget: element(".mw-dropzone-title")
  })); });
  check(Boolean(fixture.querySelector(".mw-dropzone-feedback")), "Moving across children keeps the store drag feedback");
  await act(async () => { element(".mw-dropzone").dispatchEvent(new DragEvent("dragleave", { bubbles: true, relatedTarget: fixture })); });
  check(!fixture.querySelector(".mw-dropzone-feedback"), "Leaving the store drop target removes its temporary feedback");
  await act(async () => {
    element(".mw-dropzone").dispatchEvent(new DragEvent("dragenter", { bubbles: true, cancelable: true, dataTransfer: fileTransfer() }));
  });
  await act(async () => { window.dispatchEvent(new DragEvent("dragend")); });
  check(!fixture.querySelector(".mw-dropzone-feedback"), "Cancelling a drag removes temporary store feedback");
  await dropFiles();
  await settle(() => Boolean(fixture.querySelector(".mw-manual-success-bar")), "File installation result did not render");
  layout("installed", false);
  await selectView("My Mods");
  await settle(() => fixture.textContent?.includes("FixtureMod") === true, "The imported file did not appear after switching to My Mods");
  layout("installed-my-mods", false, "my-mods");
  await selectView("Store");
  layout("returned-store", false);

  await render("minecraft");
  layout("empty", false);
  await dropFiles();
  await settle(() => Boolean(fixture.querySelector(".mw-manual-success-bar")), "Minecraft file installation result did not render");
  layout("installed", false);
  await render("minecraft");
  nextStageError = "Fixture installation rejected";
  await dropFiles();
  await settle(() => fixture.textContent?.includes("Fixture installation rejected") === true, "Installation failure feedback did not render");
  check(Boolean(fixture.querySelector(".mw-manual-feedback")) && !fixture.querySelector(".mw-manual-success-bar"),
    "A rejected store installation preserves error feedback without reporting success");
  layout("installation-error", false);
  await dropFiles();
  await settle(() => Boolean(fixture.querySelector(".mw-manual-success-bar")), "The independent store could not retry after installation failure");
  layout("installation-recovered", false);

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
  check(element<HTMLButtonElement>(".mw-version-policy-btn").disabled, "Pending installation disables the update-policy button");
  const busyWrites = writes; await dropFiles();
  check(writes === busyWrites && !fixture.querySelector(".mw-dropzone-feedback"), "A busy store rejects a concurrent file drop without showing a false drop invitation");
  await act(async () => { stageGate!.resolve(stageResult()); });
  stageGate = null;
  await settle(() => Boolean(fixture.querySelector(".mw-manual-success-bar")), "Pending installation did not finish");
  layout("installed", true);

  await render("arksurvivalascended");
  layout("reference-header", true);
  const persistentInput = element<HTMLInputElement>(".mw-reference-row input");
  await editReference("928708");
  for (const mode of ["My Mods", "Manifest mode", "Store"] as const) {
    await selectView(mode);
    headerLayout(`retained-${mode}`, true);
    check(element(".mw-reference-row input") === persistentInput && persistentInput.value === "928708",
      `ASA ${mode} preserves the same header input node and unfinished reference`);
  }
  await selectView("My Mods");
  await inputReference("928708");
  await settle(() => settingsWrites === 1 && element<HTMLInputElement>(".mw-reference-row input").value === "", "ASA Enter did not save the shared Mod ID");
  check(JSON.parse(details.settings_json).mod_ids_csv === "928708" && Boolean(fixture.querySelector(".mw-config-pane")),
    "ASA header Enter enables shared membership while My Mods remains selected");
  layout("reference-enabled-my-mods", true, "my-mods");
  await selectView("Manifest mode");
  await inputReference("https://www.curseforge.com/ark-survival-ascended/mods/fixture-two", "click");
  await settle(() => settingsWrites === 2 && element<HTMLInputElement>(".mw-reference-row input").value === "", "ASA Enable click did not save the second shared Mod ID");
  check(JSON.parse(details.settings_json).mod_ids_csv === "928708\n939713" && referenceResolves === 2,
    "ASA header Enable click in Manifest mode uses the same resolver and preserves the first Mod");
  headerLayout("reference-enabled-manifest", true);
  check(Boolean(fixture.querySelector(".mw-detail-layout--manifest")) && element(".mw-reference-row input") === persistentInput,
    "ASA Manifest save updates its content without replacing the shared header input");
  await render("valheim");
  const thunderstoreInput = element<HTMLInputElement>(".mw-reference-row input");
  await editReference("https://thunderstore.io/c/valheim/p/FixtureAuthor/FixturePlugin/");
  await selectView("My Mods");
  check(element(".mw-reference-row input") === thunderstoreInput && thunderstoreInput.value.includes("FixturePlugin"),
    "Thunderstore My Mods retains its existing reference input and typed link");
  const beforeInstallClick = writes;
  await inputReference(undefined, "click");
  await settle(() => writes === beforeInstallClick + 1 && Boolean(fixture.querySelector(".mw-manual-success-bar")), "Thunderstore Install click did not use the existing installation action");
  layout("reference-click-installed-my-mods", true, "my-mods");

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

  await render("valheim", { readOnly: true });
  const archiveStore = [...fixture.querySelectorAll<HTMLButtonElement>('[role="tab"]')].find((button) => button.textContent === "Store");
  assert(archiveStore, "Archive retains the Store tab");
  check(archiveStore.disabled && element<HTMLInputElement>(".mw-reference-row input").disabled && element<HTMLButtonElement>(".mw-reference-row button").disabled,
    "Read-only archives retain the shared header with reference input and installation disabled");
  check(element<HTMLButtonElement>(".mw-version-policy-btn").disabled, "Read-only archives retain a disabled update-policy button");
  const beforeArchive = writes;
  await act(async () => archiveStore.click());
  check(writes === beforeArchive && !fixture.querySelector(".mw-manual-pane"), "Read-only Store cannot activate an installation pane");

  if (innerWidth < 1000) {
    await render("vrising", { longNote: true });
    await selectView("My Mods");
    layout("final-my-mods", true, "my-mods");
  } else {
    await render("astroneer");
    await selectView("My Mods");
    layout("final-my-mods", false, "my-mods");
  }
  check(!fixture.querySelector(".mw-manual-actions") && Boolean(fixture.querySelector(".mw-dropzone .mw-empty")), "My Mods is a clean empty list that remains a file drop target");
  const finalWrites = writes; await dropFiles();
  await settle(() => writes === finalWrites + 1 && fixture.textContent?.includes("FixtureMod") === true, "Dropping directly into empty My Mods did not refresh its list");
  layout("final-imported-my-mods", moduleDetails.summary.id === "vrising", "my-mods");
  await act(async () => { element(".mw-dropzone").dispatchEvent(new DragEvent("dragenter", {
    bubbles: true, cancelable: true, dataTransfer: fileTransfer()
  })); });
  layout("final-drag-active", moduleDetails.summary.id === "vrising", "my-mods");
  check(element(".mw-dropzone-feedback").textContent?.includes("Drop downloaded mods here"), "Local files receive a file-import hint even when the provider also supports links");
  await act(async () => { window.dispatchEvent(new DragEvent("dragend")); });
  await render("valheim");
  const policy = element<HTMLButtonElement>(".mw-version-policy-btn");
  check(policy.getAttribute("data-version-policy") === "follow_latest", "A fresh instance view starts with following latest");
  await editReference("https://thunderstore.io/c/valheim/p/FixtureAuthor/FixturePlugin/");
  await act(async () => policy.click());
  check(policy.getAttribute("data-version-policy") === "pinned", "The update-policy button switches to a fixed version");
  await selectView("My Mods"); layout("fixed-policy-my-mods-header", true, "my-mods");
  check(element(".mw-version-policy-btn") === policy && policy.getAttribute("data-version-policy") === "pinned",
    "My Mods retains the same update-policy button and fixed-version choice");
  await selectView("Store"); layout("fixed-policy-store-header", true);
  check(element(".mw-version-policy-btn") === policy && policy.getAttribute("data-version-policy") === "pinned",
    "Returning to Store preserves the update-policy choice and header input");
  await act(async () => policy.click());
  check(policy.getAttribute("data-version-policy") === "follow_latest", "The button switches back to following latest");
  if (innerWidth < 1000) { await selectView("My Mods"); layout("final-retained-my-mods-header", true, "my-mods"); }
  else layout("final-retained-store-header", true);
  await reviewPolicyHelp();
  check(errors.length === 0, "Browser console and uncaught errors remain empty");
  // Report a completed fixture so the runner captures evidence before the Node
  // test evaluates the separately collected geometry failures.
  return { status: "passed", checks, failures, measurements, writes, settings_writes: settingsWrites, reference_resolves: referenceResolves, browser_errors: errors,
    viewport: { width: innerWidth, height: innerHeight } };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error("Manual Mod layout acceptance exceeded 40 seconds")), 40000);
})]).catch((error) => ({ status: "failed", error: String(error), failures, measurements, browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`,
    { method: "POST", body: JSON.stringify(report) }));
