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
const helpMeasurements: Record<string, unknown>[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
let details: InstanceDetails;
let moduleDetails: ModuleDetails;
let epoch = 0;
let writes = 0;
let inventoryReads = 0;
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
function helpDialog() { return document.querySelector<HTMLDialogElement>(".mw-install-help-dialog"); }
async function nativeKey(name: "Tab" | "Escape" | "Enter") {
  await act(async () => {
    const nonce = new URLSearchParams(location.search).get("nonce");
    const response = await fetch(`/__reliability_key/${nonce}/${name}`, { method: "POST" });
    assert(response.ok, `Native ${name} dispatch failed`);
  });
}
function layout(state: string, hasReferences: boolean) {
  const outer = element(".server-detail-scroll--mods");
  const workbench = element(".mw-workbench");
  const workspace = element(".mw-community-workspace");
  const toolbar = element(".mw-community-workspace > .mw-workspace-toolbar");
  const pane = element(".mw-manual-pane");
  const drop = element(".mw-dropzone");
  const bounds = pane.getBoundingClientRect();
  const dropBounds = drop.getBoundingClientRect();
  const icon = fixture.querySelector<SVGSVGElement>(".mw-dropzone-icon");
  assert(icon, "Drop target icon is missing");
  const iconBounds = icon.getBoundingClientRect();
  const styles = getComputedStyle(pane);
  const auxiliary = [...pane.children].filter((child) => child !== drop) as HTMLElement[];
  const gap = parseFloat(styles.rowGap) || 0;
  const contentHeight = bounds.height - parseFloat(styles.paddingTop) - parseFloat(styles.paddingBottom);
  const remainder = contentHeight - auxiliary.reduce((sum, child) => sum + child.getBoundingClientRect().height, 0) - gap * auxiliary.length;
  const label = `${moduleDetails.summary.id}/${state}`;
  const note = moduleDetails.mods?.source?.install_note?.trim();
  check(!fixture.querySelector(".mw-install-note"), `${label}: installation guidance occupies no permanent workspace row`);
  check(Boolean(fixture.querySelector(".mw-install-help-button")) === Boolean(note), `${label}: help entry exists only for authored guidance`);
  check(!helpDialog()?.open && (!helpDialog() || helpDialog()!.getBoundingClientRect().height === 0),
    `${label}: installation guidance stays hidden by default`);
  check(Math.abs(bounds.top - toolbar.getBoundingClientRect().bottom) <= 2 &&
    Math.abs(bounds.bottom - workspace.getBoundingClientRect().bottom) <= 2, `${label}: manual pane fills the area below tabs`);
  check(Math.abs(dropBounds.height - remainder) <= 3, `${label}: drop target consumes the remaining height after auxiliary controls`);
  check(dropBounds.height >= Math.min(100, contentHeight / 3), `${label}: guidance does not collapse the drop target`);
  check(iconBounds.width > 0 && iconBounds.height > 0 &&
    Math.abs(iconBounds.width / iconBounds.height - icon.viewBox.baseVal.width / icon.viewBox.baseVal.height) < 0.05,
    `${label}: package icon preserves its original aspect ratio`);
  check(outer.scrollHeight <= outer.clientHeight + 1 && workbench.scrollHeight <= workbench.clientHeight + 1,
    `${label}: outer workspace has no vertical overflow`);
  outer.scrollTop = 100;
  workbench.scrollTop = 100;
  check(outer.scrollTop === 0 && workbench.scrollTop === 0, `${label}: outer workspace remains at its original scroll position`);
  check(document.documentElement.scrollWidth <= innerWidth + 1 && document.documentElement.scrollHeight <= innerHeight + 1,
    `${label}: workspace stays within the desktop viewport`);
  check(visibleIn(element(".mw-dropzone-open-btn"), drop), `${label}: source button is visible inside the drop target`);
  check(visibleIn(element(".mw-dropzone-copy"), drop), `${label}: drop instructions remain readable inside the target`);
  if (hasReferences) {
    check(visibleIn(element(".mw-reference-row"), pane), `${label}: bottom reference controls remain visible`);
  } else check(!fixture.querySelector(".mw-reference-row"), `${label}: file-only source does not display unsupported reference controls`);
  measurements.push({ module: moduleDetails.summary.id, state, pane_height: bounds.height, drop_height: dropBounds.height,
    icon_width: iconBounds.width, icon_height: iconBounds.height,
    available_drop_height: remainder, pane_scroll_height: pane.scrollHeight, pane_client_height: pane.clientHeight,
    outer_scroll_height: outer.scrollHeight, outer_client_height: outer.clientHeight,
    workbench_scroll_height: workbench.scrollHeight, workbench_client_height: workbench.clientHeight,
    reference_visible: !hasReferences || visibleIn(element(".mw-reference-row"), pane) });
}
async function verifyHelp(options: { long?: boolean; leaveOpen?: boolean } = {}) {
  const label = moduleDetails.summary.id;
  const before = element(".mw-dropzone").getBoundingClientRect();
  const writesBefore = writes;
  const trigger = element<HTMLButtonElement>(".mw-install-help-button");
  check(!trigger.disabled, `${label}: installation help remains available as a read-only action`);
  await act(async () => { trigger.focus(); trigger.click(); });
  await settle(() => helpDialog()?.open === true, "Installation help did not open");
  const dialog = helpDialog()!;
  const body = dialog.querySelector<HTMLElement>(".mw-install-help-body")!;
  const close = dialog.querySelector<HTMLButtonElement>(".mw-install-help-close")!;
  assert(body && close, "Installation help content and close control are required");
  check(dialog.matches(":modal"), `${label}: help uses the native modal layer`);
  const backgroundControl = element<HTMLButtonElement>(".mw-sort-pill");
  await act(async () => { backgroundControl.focus(); });
  check(document.activeElement !== backgroundControl, `${label}: native help prevents focus on background controls`);
  check(body.textContent?.trim() === moduleDetails.mods!.source!.install_note!.trim(), `${label}: help preserves the exact module installation prerequisite`);
  check(Boolean(dialog.getAttribute("aria-labelledby") && document.getElementById(dialog.getAttribute("aria-labelledby")!)?.textContent?.trim()),
    `${label}: help dialog has an accessible title`);
  check(visibleIn(close, dialog), `${label}: help close control stays visible`);
  const bounds = dialog.getBoundingClientRect();
  check(bounds.left >= 0 && bounds.top >= 0 && bounds.right <= innerWidth && bounds.bottom <= innerHeight,
    `${label}: help dialog remains inside the desktop viewport`);
  const after = element(".mw-dropzone").getBoundingClientRect();
  check(["top", "left", "width", "height"].every((key) =>
    Math.abs(before[key as keyof DOMRect] as number - (after[key as keyof DOMRect] as number)) <= 1),
    `${label}: opening help does not resize or reposition the drop target`);
  if (options.long) {
    check(body.scrollHeight > body.clientHeight + 1, `${label}: long guidance scrolls inside the help body`);
    body.scrollTop = body.scrollHeight;
    check(body.scrollTop > 0 && body.scrollTop + body.clientHeight >= body.scrollHeight - 1,
      `${label}: the end of long installation guidance is reachable`);
    check(visibleIn(close, dialog), `${label}: scrolling long guidance keeps the close control visible`);
  }
  const focusSequence: { tag: string | undefined; className: string | undefined; documentFocused: boolean }[] = [];
  for (let index = 0; index < 3; index++) {
    await nativeKey("Tab");
    focusSequence.push({ tag: document.activeElement?.tagName, className: document.activeElement?.className,
      documentFocused: document.hasFocus() });
    // Chromium may hand Tab to browser chrome (represented by BODY); it must
    // never reach the inert application's controls, and the next Tab returns.
    check(document.activeElement === document.body || dialog.contains(document.activeElement),
      `${label}: native keyboard navigation never reaches background application controls`);
  }
  check(dialog.contains(document.activeElement), `${label}: native Tab returns from browser chrome to help`);
  const bodyGeometry = { scrollHeight: body.scrollHeight, clientHeight: body.clientHeight };
  const bodyFontWeight = getComputedStyle(body).fontWeight;
  await act(async () => { close.click(); });
  await settle(() => !helpDialog()?.open, "Installation help did not close");
  check(document.activeElement === trigger, `${label}: closing help restores focus to its trigger`);
  await nativeKey("Enter");
  await settle(() => helpDialog()?.open === true, "Native Enter did not open installation help");
  await nativeKey("Escape");
  await settle(() => !helpDialog()?.open, "Native Escape did not close installation help");
  check(document.activeElement === trigger, `${label}: Escape restores focus to the help trigger`);
  check(writes === writesBefore, `${label}: reading installation help never installs or saves anything`);
  const outer = element(".server-detail-scroll--mods");
  const workbench = element(".mw-workbench");
  check(outer.scrollHeight <= outer.clientHeight + 1 && workbench.scrollHeight <= workbench.clientHeight + 1 &&
    outer.scrollTop === 0 && workbench.scrollTop === 0, `${label}: help interaction never scrolls the outer workspace`);
  helpMeasurements.push({ module: label, long: Boolean(options.long), drop_height_before: before.height,
    drop_height_open: after.height, dialog_height: bounds.height, body_scroll_height: bodyGeometry.scrollHeight,
    body_client_height: bodyGeometry.clientHeight, body_font_weight: bodyFontWeight,
    focus_restored: document.activeElement === trigger, focus_sequence: focusSequence });
  if (options.leaveOpen) {
    await nativeKey("Enter");
    await settle(() => helpDialog()?.open === true, "Final help screenshot did not open");
    const finalBody = helpDialog()!.querySelector<HTMLElement>(".mw-install-help-body")!;
    finalBody.scrollTop = 0;
  }
}
function stageResult(): ManualModStageResult {
  return { instance_id: details.summary.id, module_id: details.summary.module_id,
    source_label: moduleDetails.mods!.source!.label, target_label: moduleDetails.mods!.manual_staging!.target_label,
    target_path: "fixture://manual-mods/target", affected_root_names: ["FixtureMod"],
    items: [], copied_file_count: 3, copied_total_bytes: 4096 };
}
async function dropFiles() {
  const file = new File(["isolated fixture"], "example.zip");
  Object.defineProperty(file, "path", { value: "fixture://manual-mods/example.zip" });
  const dataTransfer = new DataTransfer();
  dataTransfer.items.add(file);
  await act(async () => { element(".mw-dropzone").dispatchEvent(new DragEvent("drop", { bubbles: true, dataTransfer })); });
}
async function inputReference() {
  await act(async () => {
    const target = element<HTMLInputElement>(".mw-reference-input");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(target, "https://thunderstore.io/c/valheim/p/FixtureAuthor/FixturePlugin/");
    target.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => { element<HTMLButtonElement>(".mw-reference-row button").click(); });
}
async function run() {
  details = await readInstanceDetails("srv-dst-terminal-error");
  mockWindows("manual-layout-fixture");
  mockIPC(async (command, args) => {
    switch (command) {
      case "read_manual_mod_inventory":
        inventoryReads += 1;
        return { instance_id: details.summary.id, module_id: details.summary.module_id,
          source_label: moduleDetails.mods!.source!.label, target_label: moduleDetails.mods!.manual_staging!.target_label,
          target_path: "fixture://manual-mods/target", target_exists: false, items: [] };
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
        return stageGate ? stageGate.promise : stageResult();
      case "read_background_jobs": return [];
      default: throw new Error(`Unexpected native command ${command}`);
    }
  }, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });

  await render("sevendaystodie");
  layout("empty", false);
  await act(async () => { element(".mw-dropzone").dispatchEvent(new DragEvent("dragenter", { bubbles: true, dataTransfer: new DataTransfer() })); });
  check(element(".mw-dropzone").classList.contains("mw-dropzone--active"), "File drag enters the real drop target");
  layout("drag-active", false);
  await dropFiles();
  await settle(() => Boolean(fixture.querySelector(".mw-manual-success-bar")), "File installation result did not render");
  layout("installed", false);

  await render("minecraft");
  layout("empty", false);
  await dropFiles();
  await settle(() => Boolean(fixture.querySelector(".mw-manual-success-bar")), "Minecraft file installation result did not render");
  layout("installed", false);

  await render("valheim");
  layout("empty", true);
  await inputReference();
  await settle(() => Boolean(fixture.querySelector(".mw-manual-success-bar")), "Thunderstore installation result did not render");
  layout("reference-installed", true);
  await render("valheim");
  let finishStage!: (value: ManualModStageResult) => void;
  stageGate = { promise: new Promise((resolve) => { finishStage = resolve; }), resolve: (value) => finishStage(value) };
  await dropFiles();
  await settle(() => element(".mw-dropzone").classList.contains("mw-dropzone--running"), "Pending installation did not render");
  layout("installing", true);
  check(element<HTMLInputElement>(".mw-reference-input").disabled, "Pending installation disables reference input");
  await act(async () => { stageGate!.resolve(stageResult()); });
  stageGate = null;
  await settle(() => Boolean(fixture.querySelector(".mw-manual-success-bar")), "Pending installation did not finish");
  layout("installed", true);

  await render("vrising", { longNote: true });
  layout("long-install-note", true);
  await verifyHelp({ long: true });

  await render("astroneer");
  layout("loader-prerequisite", false);
  await verifyHelp();

  await render("minecraft", { running: true });
  layout("server-running", false);
  check(element(".mw-dropzone").getAttribute("aria-disabled") === "true", "Running Minecraft keeps file import disabled");
  const beforeMinecraft = writes;
  await dropFiles();
  check(writes === beforeMinecraft, "Dropping onto running Minecraft does not dispatch an installation");
  await verifyHelp();

  await render("valheim", { running: true });
  layout("server-running", true);
  check(element(".mw-dropzone").getAttribute("aria-disabled") === "true" && element<HTMLInputElement>(".mw-reference-input").disabled &&
    element<HTMLButtonElement>(".mw-reference-row button").disabled, "Running server keeps drag and reference controls disabled");
  const before = writes;
  await dropFiles();
  check(writes === before, "Dropping onto a running instance does not dispatch an installation");

  if (innerWidth < 1000) {
    await render("vrising", { longNote: true });
    layout("final-long-install-note", true);
    await verifyHelp({ long: true, leaveOpen: true });
  } else {
    await render("astroneer");
    layout("final-empty", false);
  }
  check(errors.length === 0, "Browser console and uncaught errors remain empty");
  // Report a completed fixture so the runner captures evidence before the Node
  // test evaluates the separately collected geometry failures.
  return { status: "passed", checks, failures, measurements, help_measurements: helpMeasurements, writes, browser_errors: errors,
    viewport: { width: innerWidth, height: innerHeight } };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error("Manual Mod layout acceptance exceeded 40 seconds")), 40000);
})]).catch((error) => ({ status: "failed", error: String(error), failures, measurements, browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`,
    { method: "POST", body: JSON.stringify(report) }));
