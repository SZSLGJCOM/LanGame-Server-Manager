import React, { act, StrictMode, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { bootstrapApp, readInstanceDetails, readInstanceIsolation, readModuleDetails } from "../../src/api";
import { invokeMock } from "../../src/api-mock";
import { createDefaultAiSettings } from "../../src/ai-settings";
import { I18nProvider } from "../../src/i18n";
import { instancePanelReader } from "../../src/instance-panel-loader";
import { ServersView } from "../../src/views/ServersView";
import { InstanceSettingsSaveProvider, useInstanceSettingsSaveCoordinator } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceSettingsSaveCoordinator } from "../../src/views/settings/instance-settings-save-coordinator";
import type { DstWorldImportResult, InstanceBackupResult, InstanceBroadcastEvent, InstanceDetails, InstanceIsolationReport,
  SendInstanceBroadcastInput, UpdateInstanceInput } from "../../src/types";
import { measureMaintenanceControls } from "./maintenance-control-measurements";
import { controlContractViolations } from "./measure-control-contracts";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const savedSettings: UpdateInstanceInput[] = [];
const autostartWrites: boolean[] = [];
const backupWrites: string[] = [];
const importedSources: string[] = [];
const openedPaths: string[] = [];
const restoredBackups: string[] = [];
const originalReadIsolation = instancePanelReader.readIsolation;
let checks = 0;
let holdNextSettingsSave = false;
let rejectSettingsSave: ((reason: Error) => void) | null = null;
let settingsSaveAttempts = 0;
let coordinator: InstanceSettingsSaveCoordinator;
let holdNextImport = false;
let resolveImport: ((result: DstWorldImportResult) => void) | null = null;
const noOperation = () => {};

function SaveCoordinatorProbe() {
  coordinator = useInstanceSettingsSaveCoordinator();
  return null;
}

function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
}
function select<T extends Element>(selector: string): T {
  const element = fixture.querySelector<T>(selector);
  check(element, `Missing element: ${selector}`);
  return element;
}
async function waitFor(condition: () => boolean, label: string) {
  const deadline = performance.now() + 5000;
  while (!condition()) {
    check(performance.now() < deadline, `Timed out waiting for ${label}`);
    await act(async () => { await new Promise<void>((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(selector: string) {
  await act(async () => {
    const element = select<HTMLElement>(selector);
    check(element.getClientRects().length > 0, `Cannot activate a hidden control: ${selector}`);
    element.scrollIntoView({ block: "nearest", inline: "nearest" });
    element.click();
  });
}
async function input(selector: string, value: string) {
  await act(async () => {
    const element = select<HTMLInputElement | HTMLTextAreaElement>(selector);
    const prototype = element instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    const setter = Object.getOwnPropertyDescriptor(prototype, "value")?.set;
    check(setter, "Native input setter is missing");
    setter.call(element, value);
    element.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
function visibleInside(element: Element, container: Element) {
  const inner = element.getBoundingClientRect();
  const outer = container.getBoundingClientRect();
  return inner.width > 0 && inner.height > 0 && inner.top >= outer.top - 1 && inner.bottom <= outer.bottom + 1;
}
function verifyHorizontalFit(container: HTMLElement, label: string) {
  check(container.scrollWidth <= container.clientWidth + 1, `${label} overflows horizontally`);
  const outer = container.getBoundingClientRect();
  for (const element of container.querySelectorAll<HTMLElement>("button, input, select, textarea, .instance-isolation-paths code")) {
    const box = element.getBoundingClientRect();
    if (!box.width || !box.height) continue;
    check(box.left >= outer.left - 1 && box.right <= outer.right + 1,
      `${label} clips a control or path: ${element.className || element.tagName} (${Math.round(box.left)}-${Math.round(box.right)} outside ${
        Math.round(outer.left)}-${Math.round(outer.right)} at ${innerWidth}px)`);
  }
}
type MaintenanceSection = "saves" | "backups" | "runtime" | "broadcast";
const sectionIds: MaintenanceSection[] = ["saves", "backups", "runtime", "broadcast"];
const workspaceSelector = ".server-detail-scroll--maintenance .configuration-workspace";
function navigationButton(id: MaintenanceSection) {
  return `${workspaceSelector} [data-configuration-section-id="${id}"] > button`;
}
const policyChip = '[data-maintenance-section="backups"] .server-backup-policy-card > .maintenance-chip__summary';
const importChip = '[data-maintenance-section="saves"] .save-import > .maintenance-chip__summary';
const chipSummary = (chip: string) => `.${chip} > .maintenance-chip__summary`;
const previewChips: Record<string, string> = { autosave: '[data-maintenance-section="saves"] .server-save-policy-card > .maintenance-chip__summary',
  import: importChip, policy: policyChip, resources: chipSummary("server-resource-policy"),
  recovery: chipSummary("server-runtime-recovery-chip"), daily: chipSummary("server-daily-maintenance-chip"), program: chipSummary("server-program-policy") };
/** Runtime and storage settings live in chip popovers; open one (closing the others) before using its controls. */
async function openChip(chip: string) {
  for (const open of fixture.querySelectorAll<HTMLButtonElement>('.maintenance-chip__summary[aria-expanded="true"]')) {
    if (!open.parentElement?.classList.contains(chip)) await act(async () => open.click());
  }
  if (select(chipSummary(chip)).getAttribute("aria-expanded") !== "true") await click(chipSummary(chip));
}
/** A settings popover is anchored to its chip (below it, or above when flipped) and opens without reflowing its group. */
async function verifyAnchoredPopover(chip: string, group: string) {
  const groupHeight = select(group).getBoundingClientRect().height;
  await openChip(chip);
  const summary = select(chipSummary(chip)).getBoundingClientRect();
  const panel = select(`.${chip} > .maintenance-chip__panel`).getBoundingClientRect();
  const anchored = panel.top >= summary.bottom - 1 ? panel.top - summary.bottom <= 8 : summary.top - panel.bottom >= -1 && summary.top - panel.bottom <= 8;
  check(anchored, `${chip} popover must sit against its chip (chip ${summary.top}-${summary.bottom}, panel ${panel.top}-${panel.bottom})`);
  check(panel.left >= -1 && panel.right <= innerWidth + 1, `${chip} popover must stay inside the viewport`);
  check(Math.abs(select(group).getBoundingClientRect().height - groupHeight) < 1, `${chip} popover must not change the ${group} layout height`);
  await click(chipSummary(chip));
}

async function maintenanceSection(id: MaintenanceSection) {
  const toggle = select<HTMLButtonElement>(`${workspaceSelector} .configuration-workspace__navigation-toggle`);
  if (toggle.getBoundingClientRect().height > 0 && toggle.getAttribute("aria-expanded") !== "true") {
    await click(`${workspaceSelector} .configuration-workspace__navigation-toggle`);
  }
  await click(navigationButton(id));
  check(select(navigationButton(id)).getAttribute("aria-current") === "page", `Navigation did not select ${id}`);
  const visible = [...fixture.querySelectorAll<HTMLElement>("[data-maintenance-section]")].filter((section) => !section.hidden);
  check(visible.length === 1 && visible[0].dataset.maintenanceSection === id,
    `Maintenance must display only the selected functional section: ${id}`);
  check(visible[0].getBoundingClientRect().height > 0, `Selected section is not rendered: ${id}`);
  verifyHorizontalFit(select(".server-detail-scroll--maintenance"), `Maintenance section ${id}`);
  // Backup policy and save import live in toolbar chip popovers; open them so their controls can be measured and used.
  if (id === "backups" && select(policyChip).getAttribute("aria-expanded") !== "true") await click(policyChip);
  if (id === "saves" && select(importChip).getAttribute("aria-expanded") !== "true") await click(importChip);
}
function verifyWorkspaceBounds() {
  const outer = select<HTMLElement>(".server-detail-scroll--maintenance");
  check(outer.scrollHeight <= outer.clientHeight + 1, "Maintenance content scrolls the outer workbench");
  check(document.documentElement.scrollHeight <= innerHeight + 1, "Maintenance scrolls the entire window");
  verifyHorizontalFit(outer, "Maintenance workbench");
}
async function pressKey(key: "Tab" | "Enter" | "Escape") {
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/${key}`, { method: "POST" });
    check(response.ok, `Native keyboard command failed: ${key}`);
  });
}
function localScrollOwner(element: HTMLElement, outer: HTMLElement): HTMLElement | null {
  for (let parent = element.parentElement; parent && parent !== outer; parent = parent.parentElement) {
    if (["auto", "scroll"].includes(getComputedStyle(parent).overflowY) && parent.scrollHeight > parent.clientHeight + 1) return parent;
  }
  return null;
}

interface PolicyRowAlignment { toggle_top: number; toggle_bottom: number; input_top: number; input_bottom: number; columns: number }
let policyAlignment: PolicyRowAlignment | null = null;
// The stop-backup toggle and retention input are one row of 38px controls: equal tops and bottoms in
// the two-column grid, or the same column edges once the container collapses to one column.
function policyFooter() {
  return select<HTMLElement>(".server-backup-policy-card .server-backup-policy-footer");
}
function measureBackupPolicyRow(): PolicyRowAlignment {
  const toggle = select<HTMLElement>(".server-managed-backup-policy .server-backup-policy-toggle").getBoundingClientRect();
  const field = select<HTMLElement>(".server-managed-backup-policy input[type=number]").getBoundingClientRect();
  const columns = toggle.right <= field.left + 1 ? 2 : 1;
  const result = { toggle_top: toggle.top, toggle_bottom: toggle.bottom, input_top: field.top, input_bottom: field.bottom, columns };
  if (columns === 2) {
    check(Math.abs(toggle.top - field.top) <= 1 && Math.abs(toggle.bottom - field.bottom) <= 1,
      `Backup toggle and retention input must share one control row: ${JSON.stringify(result)}`);
  } else {
    check(toggle.bottom <= field.top && Math.abs(toggle.left - field.left) <= 1 && Math.abs(toggle.width - field.width) <= 1,
      `Single-column backup controls must stack on the same edges: ${JSON.stringify(result)}`);
  }
  return result;
}

async function run() {
  const fonts = await document.fonts.load('500 13px "Inter"', "LanGame 0123456789");
  check(fonts.length > 0 && fonts.every((face) => face.status === "loaded"), "Bundled Inter must load before layout checks");
  await document.fonts.ready;
  check((innerWidth === 1560 && innerHeight === 900) || (innerWidth === 960 && innerHeight === 600),
    "Browser fixture must run at a supported desktop viewport");
  fixture.style.height = `${innerHeight - 176}px`;
  const bootstrap = await bootstrapApp({ includeSystemSnapshot: false });
  const original = bootstrap.state.instances.find((instance) => instance.module_id === "dontstarve");
  check(original, "Browser mock must provide a DST instance");
  const instance = { ...original, name: "合作世界", status: "Stopped" as const, active_process_count: 0, autostart: false };
  const stored = await readInstanceDetails(instance.id);
  const settings = { ...JSON.parse(stored.settings_json), fixture_preserved_value: "keep",
    runtime_restart: { enabled: false, max_restarts: 3, backoff_ms: 5000, only_nonzero_exit: true } };
  const details = { ...stored, summary: instance, active_run: null, backup_uses_declared_saves_path: true,
    auto_backup_on_stop: true, backup_retention_count: 5, settings_json: JSON.stringify(settings) };
  const moduleDetails = await readModuleDetails(instance.module_id);
  const healthyReport = await readInstanceIsolation(instance.id);
  check(healthyReport.mode === "private" && !healthyReport.conflicts.length && !healthyReport.issues.length,
    "Default mock must describe independent instance directories");
  const backup: InstanceBackupResult = { backup_id: "maintenance-backup", instance_id: instance.id, backup_kind: "manual",
    display_name: "周末联机备份", created_at_unix_ms: 1_800_000_000_000,
    backup_path: "fixture/backups/maintenance-backup", saves_path: "fixture/saves", file_count: 5, total_bytes: 8192 };
  let props: ComponentProps<typeof ServersView> = {
    aiSettings: createDefaultAiSettings(), assistantCanRun: false, bindAddressCandidates: [], instances: [instance],
    moduleInstallations: { [instance.module_id]: { installState: "Installed", hasManagedInstallSource: true } },
    instanceLaunchPlans: {}, instanceLaunchFailures: {}, section: "overview", onWorkspaceSectionChange: noOperation,
    selectedInstanceId: instance.id, selectedDetails: details, selectedBackups: [backup], selectedModuleDetails: moduleDetails,
    runtime: null, runtimeWindows: null, launchPlan: null, launchPlanError: null,
    refreshIssue: null, onActivity: noOperation, onResumeAutoRefresh: noOperation,
    onSelectInstance: noOperation, onStart: noOperation, onStop: noOperation, onInstallModule: async () => {},
    onOpenModuleLibrary: noOperation, onCreateInstance: noOperation,
    onPickDirectory: async () => "fixture/import/Cluster_1",
    onImportDontStarveWorldData: async (id, source) => {
      check(id === instance.id, "Import targeted the wrong instance");
      importedSources.push(source);
      if (holdNextImport) {
        holdNextImport = false;
        return new Promise<DstWorldImportResult>((resolve) => { resolveImport = resolve; });
      }
      return { instance_id: id, source_cluster_path: source, target_cluster_path: "fixture/current/clusters/main",
        safeguard_path: "fixture/backups/pre-import", imported_master: true, imported_caves: true,
        copied_file_count: 5, copied_total_bytes: 8192 };
    },
    onOpenLocalPath: (path) => { openedPaths.push(path); },
    onSendRuntimeCommand: async () => null, onSuppressRuntimeWindows: async () => {},
    onCreateBackup: (id) => { backupWrites.push(id); }, onArchivesChanged: async () => {}, onArchiveInstance: async () => {}, onDeleteInstance: noOperation,
    onRestoreBackup: (id, backupId) => {
      check(id === instance.id, "Restore targeted the wrong instance");
      restoredBackups.push(backupId);
    }, onRenameBackup: async () => true, onDeleteBackup: noOperation,
    onSaveSettings: async (update) => {
      settingsSaveAttempts++;
      if (holdNextSettingsSave) {
        holdNextSettingsSave = false;
        return new Promise<never>((_resolve, reject) => { rejectSettingsSave = reject; });
      }
      savedSettings.push(update);
      const next = { ...props.selectedDetails!, settings_json: update.settings_json, ports: update.ports,
        auto_backup_on_stop: update.auto_backup_on_stop, backup_retention_count: update.backup_retention_count };
      props = { ...props, selectedDetails: next };
      render();
      return next;
    },
    onSaveAutostart: async (id, value) => {
      check(id === instance.id, "Autostart targeted the wrong instance");
      autostartWrites.push(value);
      const summary = { ...props.selectedDetails!.summary, autostart: value };
      props = { ...props, instances: [summary], selectedDetails: { ...props.selectedDetails!, summary } };
      render();
    },
    onApplyPlayerAccessMutation: async () => { throw new Error("Unexpected player mutation"); }
  };
  function render() {
    root.render(<StrictMode><I18nProvider><InstanceSettingsSaveProvider>
      <SaveCoordinatorProbe /><ServersView {...props} />
    </InstanceSettingsSaveProvider></I18nProvider></StrictMode>);
  }
  async function running(value: boolean) {
    await act(async () => {
      const summary = { ...props.selectedDetails!.summary, status: value ? "Running" as const : "Stopped" as const,
        active_process_count: value ? 1 : 0 };
      props = { ...props, instances: [summary], selectedDetails: { ...props.selectedDetails!, summary,
        active_run: value ? { run_id: 1, process_count: 1 } : null } };
      render();
    });
  }
  await act(async () => {
    await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]);
    render();
  });
  await waitFor(() => Boolean(fixture.querySelector('[role="tab"][id$="-maintenance"]')), "server tabs");
  const tabsBefore = fixture.querySelectorAll('[role="tab"]').length;
  const initialTab = fixture.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]');
  check(initialTab?.id && !initialTab.id.endsWith("-maintenance"), "Fixture must start outside maintenance");
  const initialTabSelector = `[role="tab"][id="${initialTab.id}"]`;
  await click('[role="tab"][id$="-maintenance"]');
  await waitFor(() => fixture.querySelector(".instance-isolation-panel")?.getAttribute("aria-busy") === "false", "directory diagnosis");
  check(tabsBefore === 6 && fixture.querySelectorAll('[role="tab"]').length === tabsBefore,
    "Maintenance must retain the six existing instance tabs");
  check(fixture.querySelectorAll('[role="tablist"]').length === 1
    && !select(workspaceSelector).querySelector('[role="tablist"]'), "Maintenance introduced nested tabs");
  checks++;

  // Runtime is one card: status header with program actions, a policy chip grid and the directory rows.
  const isolation = select<HTMLElement>(".instance-isolation-panel");
  const runtimeCard = isolation;
  let outer = select<HTMLElement>(".server-detail-scroll--maintenance");
  const initialHeight = outer.scrollHeight;
  await maintenanceSection("saves");
  const importPanel = select<HTMLElement>(".dst-world-import");
  await maintenanceSection("backups");
  const policyPanel = select<HTMLElement>(".server-backup-policy-card .maintenance-chip__panel");
  const navigation = select<HTMLElement>(`${workspaceSelector} .configuration-workspace__sidebar`);
  const content = select<HTMLElement>(`${workspaceSelector} .configuration-workspace__main`);
  check(select(navigationButton("backups")).getAttribute("aria-current") === "page", "Maintenance must keep the selected backups category");
  check(!fixture.querySelector('[data-maintenance-section="cluster"]'), "A DST instance exposes ARK cluster maintenance");
  if (innerWidth > 1100) {
    const navBox = navigation.getBoundingClientRect();
    const contentBox = content.getBoundingClientRect();
    check(Math.abs(navBox.width - 228) <= 1 && navBox.right <= contentBox.left + 1,
      "Desktop maintenance does not share configuration's category sidebar geometry");
  }
  for (const id of sectionIds) await maintenanceSection(id);
  verifyWorkspaceBounds();
  checks++;

  check(!fixture.querySelector('.server-file-panel--paths, .server-file-path-list'), "A second local directory panel remains");
  await maintenanceSection("runtime");
  const policyChips = [...select(".runtime-policies__grid").querySelectorAll<HTMLElement>(":scope > .maintenance-chip")];
  check(!isolation.querySelector("details") && policyChips.length === 5
    && policyChips.every((chip) => chip.querySelector(".maintenance-chip__value")?.textContent), "Runtime policies must collapse into chips that show their current value");
  const chipColumns = new Set(policyChips.map((chip) => Math.round(chip.getBoundingClientRect().left))).size;
  check(chipColumns === (select(".runtime-policies__grid").getBoundingClientRect().width > 560 ? 2 : 1), "Policy chips must form a balanced grid");
  check(isolation.querySelector(".runtime-status-tag")?.textContent === "已停止" && isolation.querySelectorAll(".maintenance-group__heading").length === 2,
    "Runtime header must show the instance status and group headings");
  const autostartCheckbox = select<HTMLInputElement>(".server-autostart-chip input[type=checkbox]");
  check(!autostartCheckbox.checked && autostartCheckbox.closest(".maintenance-chip__summary.settings-toggle-card")
    && !fixture.querySelector(".server-autostart-chip [aria-expanded]")
    && !fixture.querySelector(".server-autostart-chip .maintenance-chip__panel"), "Autostart must be a checkbox chip (shared toggle dot), not a popover");
  for (const chip of ["server-resource-policy", "server-runtime-recovery-chip", "server-daily-maintenance-chip", "server-program-policy"]) {
    await verifyAnchoredPopover(chip, ".runtime-policies");
  }
  checks++;
  await openChip("server-runtime-recovery-chip");
  for (const name of ["maxRestarts", "waitSeconds", "onlyNonzeroExit"]) {
    const control = select<HTMLInputElement>(`.server-runtime-recovery-editor input[name="${name}"]`);
    control.scrollIntoView({ block: "nearest" });
    check(!control.disabled && visibleInside(control, outer), `Disabled recovery must retain its editable ${name} control`);
  }
  checks++;

  await openChip("server-resource-policy");
  verifyHorizontalFit(runtimeCard, "Resource popover");
  const resourceControls = [...fixture.querySelectorAll<HTMLInputElement>(".server-resource-policy input")];
  check(resourceControls.length === 3 && resourceControls.every((control) => !control.disabled),
    "Stopped instance must expose editable CPU, memory and reserve controls");
  check(resourceControls.map((control) => control.name).join(",") === "cpu,memory,reserve", "Resource policy lost a supported limit");
  checks++;

  await maintenanceSection("saves");
  await verifyAnchoredPopover("server-save-policy-card", '[data-maintenance-section="saves"]');
  await maintenanceSection("backups");
  check(!fixture.querySelector(".server-backup-policy-card > .maintenance-chip__summary .maintenance-chip__value")
    && !fixture.querySelector(".server-save-policy-card > .maintenance-chip__summary .maintenance-chip__value"),
    "Header toolbar policy chips are label-only; their details live in the popover");
  await verifyAnchoredPopover("server-backup-policy-card", ".server-backup-history-card");
  await click(".server-backup-history-card .maintenance-toolbar > button.secondary-button");
  check(backupWrites.length === 1 && backupWrites[0] === instance.id, "Immediate backup lost its instance callback");
  checks++;

  // Save slots share the backup table's row layout, mark the slot in use and expand to their files.
  await maintenanceSection("saves");
  await waitFor(() => !!fixture.querySelector('[data-maintenance-section="saves"] .save-slot-table .server-file-backup-row'), "save slot rows");
  const slotRow = select<HTMLElement>('[data-maintenance-section="saves"] .save-slot-table .server-file-backup-row');
  const backupRowStyle = getComputedStyle(select('[data-maintenance-section="backups"] .server-file-backup-row'));
  const slotRowStyle = getComputedStyle(slotRow);
  check(slotRowStyle.display === backupRowStyle.display && slotRowStyle.borderRadius === backupRowStyle.borderRadius
    && slotRowStyle.paddingTop === backupRowStyle.paddingTop, "Save slot rows must match the backup table style");
  check(slotRow.dataset.current === "true" && slotRow.textContent?.includes("当前使用"), "The slot in use is not marked");
  check(!slotRow.querySelector(".inline-confirm-action") && !/恢复|删除|重命名/.test(slotRow.querySelector(".server-file-backup-actions")?.textContent ?? ""),
    "Save slots must stay read-only");
  const slotToggle = slotRow.querySelector<HTMLButtonElement>('button[aria-expanded]')!;
  await act(async () => { slotToggle.click(); });
  check(slotToggle.getAttribute("aria-expanded") === "true" && !!slotRow.querySelector(".save-file-browser__table"),
    "Expanding a save slot did not list its files");
  check(slotRow.querySelector(".save-file-browser__table")?.textContent?.includes("Master/world.dat"), "Slot file list lost its file");
  verifyHorizontalFit(select(".server-detail-scroll--maintenance"), "Expanded save slot");
  const openedBefore = openedPaths.length;
  await act(async () => { [...slotRow.querySelectorAll<HTMLButtonElement>(".server-file-backup-actions button")].at(-1)!.click(); });
  check(openedPaths.length === openedBefore + 1 && /Master$/.test(openedPaths.at(-1)!), "Slot folder action targeted another path");
  openedPaths.splice(openedBefore);
  await act(async () => { slotToggle.click(); });
  checks++;

  await maintenanceSection("saves");
  check(select<HTMLButtonElement>(".dst-world-import-actions .primary-button").disabled, "Import without a source is enabled");
  await click(".dst-world-import-source-row .secondary-button");
  check(importPanel.textContent?.includes("fixture/import/Cluster_1"), "Selected import source is not displayed");
  check(!select<HTMLButtonElement>(".dst-world-import-actions .primary-button").disabled, "Stopped import with a source is disabled");
  await running(true);
  check([...fixture.querySelectorAll<HTMLInputElement>(".server-resource-policy input")].every((control) => control.disabled),
    "Running instance allows resource policy edits");
  check(select<HTMLButtonElement>(".dst-world-import-actions .primary-button").disabled, "A running instance can replace its world");
  await click(".dst-world-import-actions .primary-button");
  check(importedSources.length === 0, "A disabled import dispatched its callback");
  await running(false);
  await click(".dst-world-import-actions .primary-button");
  check(importedSources.length === 1 && importedSources[0] === "fixture/import/Cluster_1", "Import did not use the selected source exactly once");
  check(select(".dst-world-import").querySelector('[role="status"]')?.textContent?.includes("fixture/backups/pre-import"), "Import success lost its recovery backup");
  verifyHorizontalFit(importPanel, "Import form");
  checks++;

  await maintenanceSection("backups");
  policyAlignment = measureBackupPolicyRow();
  check(!policyPanel.querySelector("button[type=submit]"), "Autosaved policy still offers a manual save button");
  await input(".server-backup-policy-card .server-save-policy-group input[type=number]", "7");
  check(savedSettings.length === 0, "Typed retention must wait for the autosave pause");
  await maintenanceSection("runtime");
  await maintenanceSection("saves");
  check(importPanel.textContent?.includes("fixture/import/Cluster_1"), "Navigation discarded the selected import source");
  await maintenanceSection("backups");
  check(select<HTMLInputElement>(".server-backup-policy-card .server-save-policy-group input[type=number]").value === "7",
    "Navigation discarded an unsaved backup-policy draft");
  await waitFor(() => savedSettings.length === 1, "debounced retention autosave");
  check(savedSettings[0].backup_retention_count === 7 && savedSettings[0].auto_backup_on_stop,
    "Backup strategy did not autosave the edited retention");
  check(JSON.parse(savedSettings[0].settings_json).fixture_preserved_value === "keep", "Backup strategy overwrote unrelated settings");
  await waitFor(() => policyFooter().dataset.saveState === "saved", "policy saved status");
  check(policyFooter().textContent?.includes("策略已保存"), "Autosave does not report the saved policy");
  await click(".server-managed-backup-policy input[type=checkbox]");
  await act(async () => { await new Promise<void>((resolve) => setTimeout(resolve, 100)); });
  check(savedSettings.length === 2 && savedSettings[1].auto_backup_on_stop === false && savedSettings[1].backup_retention_count === 7,
    "The stop-backup toggle must save immediately without losing retention");
  await input(".server-backup-policy-card .server-save-policy-group input[type=number]", "0");
  check(select(".server-backup-policy-card .server-save-policy-group input[type=number]").getAttribute("aria-invalid") === "true"
    && select(".server-backup-policy-validation").getBoundingClientRect().height > 0, "Invalid retention hides its error");
  await act(async () => { await new Promise<void>((resolve) => setTimeout(resolve, 900)); });
  check(savedSettings.length === 2 && policyFooter().dataset.saveState === "blocked", "Invalid retention reached storage");
  check(await coordinator.flush(instance.id).then(() => false, () => true), "Start ignored an invalid policy draft");
  await input(".server-backup-policy-card .server-save-policy-group input[type=number]", "7");
  await waitFor(() => policyFooter().dataset.saveState === "saved", "restored valid retention");
  check(savedSettings.length === 2, "Restoring the persisted retention wrote it again");
  verifyHorizontalFit(select<HTMLElement>(".server-backup-policy-card .maintenance-chip__panel"), "Save strategy form");
  checks++;

  await maintenanceSection("runtime");
  check(!fixture.querySelector(".server-runtime-recovery-editor button[type=submit], .server-resource-policy button[type=submit]"),
    "Autosaved runtime policies still offer manual save buttons");
  await openChip("server-runtime-recovery-chip");
  const recoveryFooter = () => select<HTMLElement>(".server-runtime-recovery-editor .server-backup-policy-footer");
  await input('.server-runtime-recovery-editor input[name="maxRestarts"]', "0");
  check(select('.server-runtime-recovery-editor input[name="maxRestarts"]').getAttribute("aria-invalid") === "true"
    && recoveryFooter().dataset.saveState === "blocked", "Invalid recovery limits are not blocked");
  await act(async () => { await new Promise<void>((resolve) => setTimeout(resolve, 900)); });
  check(savedSettings.length === 2, "Invalid recovery limits reached storage");
  await input('.server-runtime-recovery-editor input[name="maxRestarts"]', "4");
  await input('.server-runtime-recovery-editor input[name="waitSeconds"]', "7.25");
  check(savedSettings.length === 2, "Typed recovery limits skipped the autosave pause");
  // The toggle saves at once and carries the typed limits with it.
  await click('.server-runtime-recovery-editor input[name="enabled"]');
  check(!select<HTMLInputElement>('.server-runtime-recovery-editor input[name="waitSeconds"]').disabled, "Enabling recovery did not enable its limits");
  await waitFor(() => savedSettings.length === 3, "immediate recovery toggle autosave");
  const recoveryWrite = savedSettings.at(-1)!;
  const recoveredSettings = JSON.parse(recoveryWrite.settings_json);
  check(savedSettings.length === 3 && recoveredSettings.runtime_restart.enabled
    && recoveredSettings.runtime_restart.max_restarts === 4 && recoveredSettings.runtime_restart.backoff_ms === 7250,
  "Recovery save changed its limits or seconds-to-milliseconds contract");
  check(recoveryWrite.backup_retention_count === 7 && !recoveryWrite.auto_backup_on_stop && recoveredSettings.fixture_preserved_value === "keep",
    "Recovery save lost another maintenance policy");
  await waitFor(() => recoveryFooter().dataset.saveState === "saved", "recovery saved status");
  verifyHorizontalFit(runtimeCard, "Recovery controls");
  checks++;

  await input('.server-runtime-recovery-editor input[name="maxRestarts"]', "");
  await click('.server-runtime-recovery-editor input[name="enabled"]');
  const disabledRecoveryLimit = select<HTMLInputElement>('.server-runtime-recovery-editor input[name="maxRestarts"]');
  check(!disabledRecoveryLimit.disabled && disabledRecoveryLimit.value === "",
    "Disabling recovery locked its invalid parameter and prevented correction");
  await act(async () => { await new Promise<void>((resolve) => setTimeout(resolve, 100)); });
  check(recoveryFooter().dataset.saveState === "blocked" && savedSettings.length === 3,
    "Disabling recovery bypassed parameter validation");
  await input('.server-runtime-recovery-editor input[name="maxRestarts"]', "4");
  await waitFor(() => savedSettings.length === 4, "corrected disabled recovery autosave");
  check(savedSettings.length === 4 && JSON.parse(savedSettings[3].settings_json).runtime_restart.enabled === false,
    "Disabled recovery could not be persisted");
  // Another editor's write becomes the policy editor's baseline instead of a conflict or a second write.
  await act(async () => { await new Promise<void>((resolve) => setTimeout(resolve, 800)); });
  check(savedSettings.length === 4 && settingsSaveAttempts === 4, "A recovery write triggered a policy autosave");
  checks++;

  await maintenanceSection("backups");
  holdNextSettingsSave = true;
  rejectSettingsSave = null;
  await input(".server-backup-policy-card .server-save-policy-group input[type=number]", "9");
  await waitFor(() => rejectSettingsSave !== null, "policy autosave to reach the controlled storage boundary");
  check(select(".server-backup-policy-card .shell-activity-notice.is-info").textContent?.includes("保存"),
    "In-flight policy save does not show its progress");
  check(savedSettings.length === 4 && settingsSaveAttempts === 5, "Pending policy save was reported as persisted");
  let startSettled = false;
  const pendingStart = coordinator.flush(instance.id).then(
    () => { startSettled = true; return null; },
    (error: unknown) => { startSettled = true; return error; }
  );
  await act(async () => { await Promise.resolve(); });
  check(!startSettled, "Start ignored a pending policy write");
  await act(async () => { rejectSettingsSave!(new Error("Fixture policy storage unavailable")); });
  check(await pendingStart instanceof Error, "Failed policy persistence did not block start");
  const failedStart = await coordinator.flush(instance.id).then(() => null, (error: unknown) => error);
  check(failedStart instanceof Error, "A later start forgot the settled policy failure");
  const policyError = select<HTMLElement>(".server-backup-policy-card .shell-activity-notice.is-error");
  policyError.scrollIntoView({ block: "nearest" });
  check(policyError.textContent?.includes("Fixture policy storage unavailable") && policyError.getBoundingClientRect().height > 0,
    "Failed policy save hid its actual storage error");
  check(visibleInside(policyError, policyPanel) && visibleInside(policyError, outer), "Policy error cannot be reached by local scrolling");
  check(select<HTMLInputElement>(".server-backup-policy-card .server-save-policy-group input[type=number]").value === "9"
    && !select<HTMLButtonElement>(".server-backup-policy-footer .secondary-button").disabled,
    "Failed policy save lost its draft or its retry");
  await click(".server-backup-policy-footer .secondary-button");
  await waitFor(() => savedSettings.length === 5, "policy retry");
  check(settingsSaveAttempts === 6 && savedSettings[4].backup_retention_count === 9,
    "Retry did not persist the retained policy draft exactly once");
  await waitFor(() => policyFooter().dataset.saveState === "saved", "retried policy saved status");
  check(policyPanel.querySelector(".shell-activity-notice.is-success") && !policyPanel.querySelector(".shell-activity-notice.is-error"),
    "Successful policy retry left the failure status visible");
  await coordinator.flush(instance.id);
  holdNextSettingsSave = true;
  rejectSettingsSave = null;
  await input(".server-backup-policy-card .server-save-policy-group input[type=number]", "10");
  await waitFor(() => rejectSettingsSave !== null, "discarded policy write to reach storage");
  await act(async () => { rejectSettingsSave!(new Error("Fixture discarded policy write")); });
  const discardedFailure = await coordinator.flush(instance.id).then(() => null, (error: unknown) => error);
  check(discardedFailure instanceof Error, "The policy failure was not tracked before discarding");
  await click(".server-backup-policy-footer .ghost-button");
  await coordinator.flush(instance.id);
  check(select<HTMLInputElement>(".server-backup-policy-card .server-save-policy-group input[type=number]").value === "9"
    && savedSettings.length === 5, "Discard changed the persisted policy");
  checks++;

  await maintenanceSection("runtime");
  await click(".server-autostart-chip input[type=checkbox]");
  check(autostartWrites.length === 1 && autostartWrites[0] && select<HTMLInputElement>(".server-autostart-chip input[type=checkbox]").checked,
    "Autostart no longer persists independently from recovery");
  checks++;

  await maintenanceSection("runtime");
  const directoryRows = [...isolation.querySelectorAll<HTMLElement>(".instance-isolation-paths .server-file-backup-row")];
  const directoryButtons = [...isolation.querySelectorAll<HTMLButtonElement>(".instance-isolation-paths .instance-isolation-open")];
  check(directoryRows.length === 5 && directoryButtons.length === 5, "Unified directories must include program, data, config, saves and backups");
  check(directoryRows[0].textContent?.includes("程序位置") && directoryRows[1].textContent?.includes("数据位置"),
    "Program and instance data locations must be named explicitly");
  check(directoryRows.every((row) => row.getBoundingClientRect().height <= 44 && !row.querySelector("details")),
    "Directory rows must stay dense single lines");
  const copiedPaths: string[] = [];
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async (text: string) => { copiedPaths.push(text); } } });
  await act(async () => { directoryRows[2].querySelector<HTMLButtonElement>('button[aria-label="复制路径"]')!.click(); });
  check(copiedPaths.length === 1 && copiedPaths[0] === healthyReport.config_path, "Copying a directory did not copy its full path");
  for (const button of directoryButtons) await act(async () => { button.click(); });
  check(openedPaths.length === 5 && new Set(openedPaths).size === 5, "Directory actions are duplicated or target the same path");
  check(openedPaths.includes(healthyReport.runtime_path) && openedPaths.includes(healthyReport.data_path)
    && openedPaths.includes(healthyReport.config_path) && openedPaths.includes(healthyReport.saves_path)
    && openedPaths.some((path) => path.endsWith("/backups")), "Unified directories lost an existing path action");
  verifyHorizontalFit(isolation, "Directory controls");
  checks++;

  const damagedReport: InstanceIsolationReport = { ...healthyReport, mode: "damaged", issues: ["Fixture directory issue"] };
  let resolveDiagnosis!: (report: InstanceIsolationReport) => void;
  instancePanelReader.readIsolation = () => new Promise((resolve) => { resolveDiagnosis = resolve; });
  await click(".instance-isolation-refresh");
  check(isolation.getAttribute("aria-busy") === "true", "Directory refresh does not expose its busy state");
  await act(async () => { resolveDiagnosis(damagedReport); });
  check(isolation.textContent?.includes("Fixture directory issue"), "A directory diagnosis hid its issue");
  instancePanelReader.readIsolation = async () => { throw new Error("Fixture diagnosis unavailable"); };
  await click(".instance-isolation-refresh");
  const diagnosisError = select<HTMLElement>(".instance-isolation-panel .shell-activity-notice.is-error");
  check(diagnosisError.textContent?.includes("Fixture diagnosis unavailable") && diagnosisError.getBoundingClientRect().height > 0,
    "Directory read failure lost its visible cause");
  instancePanelReader.readIsolation = originalReadIsolation;
  await click(".instance-isolation-refresh");
  await waitFor(() => isolation.getAttribute("aria-busy") === "false", "healthy directory refresh");
  check(!isolation.querySelector(".shell-activity-notice.is-error, .instance-isolation-error"), "Successful diagnosis retry left the failure visible");
  checks++;

  const unsupportedModule = { ...moduleDetails, runtime: { ...moduleDetails.runtime, player_actions: [] } };
  const supportedModule = { ...moduleDetails, runtime: { ...moduleDetails.runtime,
    player_actions: [{ id: "broadcast", kind: "broadcast", label: "Broadcast", command_template: "say {{message}}",
      target_required: false, destructive: false }] } };
  await act(async () => { props = { ...props, selectedModuleDetails: unsupportedModule }; render(); });
  await maintenanceSection("broadcast");
  const unavailableBroadcast = select<HTMLElement>(".server-broadcast-panel");
  check(unavailableBroadcast.textContent?.includes("广播") && !unavailableBroadcast.querySelector("textarea[name=broadcast-intent]"),
    "Unsupported broadcast must keep its titled unavailable state without unusable controls");
  check(unavailableBroadcast.querySelector(".server-broadcast-unavailable")?.textContent?.trim(),
    "Unsupported broadcast lost the explanation of its capability limit");
  await act(async () => { props = { ...props, selectedModuleDetails: supportedModule }; render(); });
  await waitFor(() => !select<HTMLTextAreaElement>("textarea[name=broadcast-intent]").disabled, "broadcast policy loading");
  const broadcastDraft = select<HTMLTextAreaElement>("textarea[name=broadcast-intent]");
  broadcastDraft.scrollIntoView({ block: "nearest" });
  check(visibleInside(broadcastDraft, outer), "Supported broadcast has no reachable intent editor");
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(broadcastDraft, "保留广播草稿");
    broadcastDraft.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await maintenanceSection("backups");
  await maintenanceSection("broadcast");
  check(select<HTMLTextAreaElement>("textarea[name=broadcast-intent]").value === "保留广播草稿",
    "Category navigation discarded the broadcast draft");
  await maintenanceSection("saves");
  await click(".dst-world-import-source-row .secondary-button");
  verifyWorkspaceBounds();
  checks++;

  await maintenanceSection("backups");
  const manyBackups = Array.from({ length: 30 }, (_, index) => ({ ...backup, backup_id: `history-${index}`,
    display_name: `联机备份 ${index + 1}`, backup_path: `fixture/backups/history-${index}` }));
  await act(async () => { props = { ...props, selectedBackups: manyBackups }; render(); });
  const lastRow = select<HTMLElement>('[data-maintenance-section="backups"] .server-file-backup-row:last-child');
  const backupScroll = localScrollOwner(lastRow, outer);
  check(backupScroll && backupScroll.matches("[data-configuration-scroll-owner]"),
    "Long backup history must scroll inside the selected configuration-style content region");
  check(outer.scrollHeight <= outer.clientHeight + 1, "Long backup history forces the entire maintenance page to scroll");
  lastRow.scrollIntoView({ block: "nearest" });
  const lastRestore = lastRow.querySelector<HTMLButtonElement>(".server-file-backup-actions > button")!;
  check(visibleInside(lastRestore, backupScroll), "Scrolling the backup list cannot reach its final restore action");
  await act(async () => { lastRestore.click(); });
  check(restoredBackups.length === 1 && restoredBackups[0] === "history-29", "Last backup action targeted a different row");
  await act(async () => { lastRow.querySelector<HTMLButtonElement>(".inline-confirm-action > button")!.click(); });
  const lastReview = lastRow.querySelector<HTMLElement>(".inline-confirm-review")!;
  lastReview.scrollIntoView({ block: "nearest" });
  check(visibleInside(lastReview.querySelector(".inline-confirm-buttons")!, backupScroll), "Last backup confirmation is unreachable");
  await act(async () => { lastReview.querySelector<HTMLButtonElement>(".inline-confirm-buttons > button")!.click(); });
  await act(async () => { props = { ...props, selectedBackups: [backup] }; render(); });
  checks++;

  await maintenanceSection("saves");
  holdNextImport = true;
  await click(".dst-world-import-actions .primary-button");
  check(resolveImport && importedSources.length === 2, "Pending import did not reach its storage boundary");
  check([...fixture.querySelectorAll<HTMLButtonElement>(".dst-world-import-form button")].every((button) => button.disabled),
    "An in-flight import permits overlapping source selection or import");
  await click(".dst-world-import-actions .primary-button");
  check(importedSources.length === 2, "A repeated click dispatched a second import");
  await maintenanceSection("runtime");
  await maintenanceSection("saves");
  check(select<HTMLButtonElement>(".dst-world-import-actions .primary-button").disabled
    && importedSources.length === 2, "Navigation discarded an in-flight import or allowed duplicate dispatch");
  const firstDetails = props.selectedDetails!;
  const alternate: InstanceDetails = { ...firstDetails, summary: { ...firstDetails.summary, id: "maintenance-alternate", name: "另一世界" },
    config_file_path: "fixture/alternate/config/server.ini", saves_path: "fixture/alternate/saves" };
  instancePanelReader.readIsolation = async (id, signal) => id === alternate.summary.id ? {
    ...healthyReport, instance_id: id, runtime_path: "fixture/alternate/runtime", data_path: "fixture/alternate", config_path: "fixture/alternate/config",
    saves_path: "fixture/alternate/saves"
  } : originalReadIsolation.call(instancePanelReader, id, signal);
  await act(async () => {
    props = { ...props, instances: [firstDetails.summary, alternate.summary], selectedInstanceId: alternate.summary.id,
      selectedDetails: alternate, selectedBackups: [], selectedModuleDetails: unsupportedModule };
    render();
  });
  await waitFor(() => fixture.querySelector(".instance-isolation-panel")?.getAttribute("aria-busy") === "false", "alternate instance directories");
  await maintenanceSection("saves");
  check(select<HTMLButtonElement>(".dst-world-import-actions .primary-button").disabled
    && !select(".dst-world-import").textContent?.includes("fixture/import/Cluster_1"), "Changing instances leaked the previous import source");
  await act(async () => { resolveImport!({ instance_id: instance.id, source_cluster_path: "fixture/import/Cluster_1",
    target_cluster_path: "fixture/current", safeguard_path: "fixture/backups/stale-result", imported_master: true,
    imported_caves: true, copied_file_count: 5, copied_total_bytes: 8192 }); });
  check(!select(".dst-world-import").textContent?.includes("stale-result"), "A completed old import changed the newly selected instance");
  await maintenanceSection("runtime");
  await click(".instance-isolation-paths button");
  check(openedPaths.at(-1) === "fixture/alternate/runtime", "Instance switch retained the old directory action");
  check(!fixture.querySelector(".server-backup-policy-card .is-failed"), "Instance switch retained an old save error");
  instancePanelReader.readIsolation = originalReadIsolation;
  await act(async () => {
    props = { ...props, instances: [firstDetails.summary], selectedInstanceId: instance.id, selectedDetails: firstDetails,
      selectedBackups: [backup], selectedModuleDetails: supportedModule };
    render();
  });
  await waitFor(() => fixture.querySelector(".instance-isolation-panel")?.getAttribute("aria-busy") === "false"
    && !select<HTMLTextAreaElement>("textarea[name=broadcast-intent]").disabled, "original instance controls to reload");
  // Instance changes replace the keyed detail panel; geometry must use its current DOM owner.
  outer = select<HTMLElement>(".server-detail-scroll--maintenance");
  check(outer.isConnected, "Restored instance maintenance panel is detached");
  checks++;

  // Leaving the maintenance tab inside the typing pause persists the edit instead of discarding it.
  await maintenanceSection("backups");
  const writesBeforeTabSwitch = savedSettings.length;
  await input(".server-backup-policy-card .server-save-policy-group input[type=number]", "8");
  check(savedSettings.length === writesBeforeTabSwitch, "Typed retention skipped the autosave pause");
  await click(initialTabSelector);
  await waitFor(() => savedSettings.length === writesBeforeTabSwitch + 1, "tab switch to flush the pending policy edit");
  check(savedSettings.at(-1)!.backup_retention_count === 8, "Tab switch saved a different retention");
  await click('[role="tab"][id$="-maintenance"]');
  await waitFor(() => fixture.querySelector(".instance-isolation-panel")?.getAttribute("aria-busy") === "false", "maintenance remount");
  await maintenanceSection("backups");
  check(select<HTMLInputElement>(".server-backup-policy-card .server-save-policy-group input[type=number]").value === "8", "Returning to maintenance lost the saved retention");
  outer = select<HTMLElement>(".server-detail-scroll--maintenance");
  policyAlignment = measureBackupPolicyRow();
  checks++;

  const toggle = select<HTMLButtonElement>(`${workspaceSelector} .configuration-workspace__navigation-toggle`);
  if (innerWidth <= 1100) {
    check(toggle.getBoundingClientRect().height > 0 && toggle.getAttribute("aria-expanded") === "false",
      "Compact category navigation must start collapsed");
    toggle.focus();
    await pressKey("Enter");
    check(toggle.getAttribute("aria-expanded") === "true", "Enter did not open compact maintenance categories");
    await pressKey("Tab");
    check(select(`${workspaceSelector} .configuration-workspace__sidebar`).contains(document.activeElement), "Keyboard focus did not enter maintenance categories");
    await pressKey("Escape");
    check(toggle.getAttribute("aria-expanded") === "false" && document.activeElement === toggle,
      "Escape did not close categories and return focus to their toggle");
    await pressKey("Enter");
    select<HTMLButtonElement>(navigationButton("runtime")).focus();
    await pressKey("Enter");
    check(toggle.getAttribute("aria-expanded") === "false" && document.activeElement === toggle
      && select(navigationButton("runtime")).getAttribute("aria-current") === "page",
      "Keyboard category selection failed to collapse navigation and retain focus");
    await pressKey("Tab");
    check(document.activeElement?.closest("[data-maintenance-section]")?.getAttribute("data-maintenance-section") === "runtime",
      "Keyboard focus entered a hidden maintenance category");
  }
  for (const [id, selector] of [
    ["saves", ".dst-world-import-source-row .secondary-button"], ["backups", ".server-managed-backup-policy input[type=number]"],
    ["runtime:server-runtime-recovery-chip", '.server-runtime-recovery-editor input[name="enabled"]'],
    ["runtime:server-resource-policy", ".server-resource-policy input[name=cpu]"],
    ["runtime", ".instance-isolation-paths button"], ["broadcast", "textarea[name=broadcast-intent]"]
  ] as const) {
    const [section, chip] = id.split(":") as [MaintenanceSection, string | undefined];
    await maintenanceSection(section);
    if (chip) await openChip(chip);
    const control = select<HTMLElement>(selector);
    control.scrollIntoView({ block: "nearest", inline: "nearest" });
    check(visibleInside(control, outer), `Maintenance cannot reach ${selector} at ${innerWidth} x ${innerHeight}`);
  }
  await maintenanceSection("backups");
  await click(".server-backup-history-card .maintenance-toolbar > button.secondary-button");
  check(backupWrites.length === 2 && backupWrites[1] === instance.id, "Category navigation lost immediate backup");
  checks++;

  // Only delayed native receipts are controlled; the real forms, API envelope,
  // history refresh timer and instance lifecycle remain mounted in ServersView.
  const beforeBroadcastProps = props;
  const broadcastEvents = new Map<string, InstanceBroadcastEvent[]>();
  const broadcastHistoryStatuses: string[] = [];
  let broadcastSendCount = 0;
  let holdNextHistory = false;
  let resolveLateHistory: ((events: InstanceBroadcastEvent[]) => void) | null = null;
  Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args?: Record<string, unknown>) => {
    if (command === "send_instance_broadcast") {
      const request = args?.input as SendInstanceBroadcastInput;
      check(request.instanceId === instance.id && request.source === "manual", "Broadcast targeted the wrong instance or source");
      const event: InstanceBroadcastEvent = { event_id: `receipt-${++broadcastSendCount}`, instance_id: request.instanceId,
        module_id: "dontstarve", source: "manual", message: request.message, status: "pending",
        action_id: "broadcast", transport: "stdin", created_at_unix_ms: 1_800_000_000_000 + broadcastSendCount };
      broadcastEvents.set(request.instanceId, [event, ...(broadcastEvents.get(request.instanceId) ?? [])]);
      return { event: { ...event }, actionId: "broadcast", transport: "stdin", commandPreview: "fixture pending stdin receipt" };
    }
    if (command === "list_instance_broadcast_events") {
      const id = String(args?.instanceId);
      if (holdNextHistory && id === instance.id) {
        holdNextHistory = false;
        return new Promise<InstanceBroadcastEvent[]>((resolve) => { resolveLateHistory = resolve; });
      }
      return structuredClone(broadcastEvents.get(id) ?? []);
    }
    return invokeMock(command, args);
  } } });
  await running(true);
  await maintenanceSection("broadcast");
  const latestBroadcastStatus = () => select<HTMLElement>(".server-broadcast-event-status");
  for (const status of ["sent", "failed"] as const) {
    await input("textarea[name=broadcast-message]", `等待确认后${status === "sent" ? "发送" : "失败"}的广播`);
    await click(".server-broadcast-actions .primary-button");
    await waitFor(() => fixture.querySelector(".server-broadcast-event-status")?.textContent?.includes("待确认") === true,
      "pending broadcast receipt in history");
    broadcastHistoryStatuses.push(latestBroadcastStatus().textContent?.trim() ?? "");
    check(!latestBroadcastStatus().classList.contains("is-success")
      && select(".server-broadcast-panel").textContent?.includes("系统不会自动重发")
      && select<HTMLTextAreaElement>("textarea[name=broadcast-message]").value === "",
      "An accepted pending receipt must explain uncertainty, clear the submitted draft and never claim success");
    const sendsBeforeRefresh = broadcastSendCount;
    const events = broadcastEvents.get(instance.id)!;
    broadcastEvents.set(instance.id, events.map((event, index) => index === 0 ? { ...event, status,
      error_message: status === "failed" ? "Fixture stdin pipe closed before confirmation" : null } : event));
    await waitFor(() => fixture.querySelector(".server-broadcast-event-status")?.textContent?.includes(status === "sent" ? "已发送" : "失败") === true,
      `pending broadcast to refresh as ${status}`);
    broadcastHistoryStatuses.push(latestBroadcastStatus().textContent?.trim() ?? "");
    check(broadcastSendCount === sendsBeforeRefresh && fixture.querySelectorAll(".server-broadcast-event").length === sendsBeforeRefresh,
      "Receipt refresh must update the same history entry without resending or duplicating a broadcast");
    check(status === "sent" ? latestBroadcastStatus().classList.contains("is-success")
      : select(".server-broadcast-event .server-broadcast-error[role=alert]").textContent?.includes("Fixture stdin pipe closed"),
      "Settled broadcast history must preserve its confirmed status or original transport error");
    checks++;
  }
  await input("textarea[name=broadcast-message]", "旧实例待确认广播");
  await click(".server-broadcast-actions .primary-button");
  await waitFor(() => fixture.querySelector(".server-broadcast-event-status")?.textContent?.includes("待确认") === true,
    "old instance pending receipt");
  holdNextHistory = true;
  await waitFor(() => resolveLateHistory !== null, "pending receipt refresh to reach the delayed native boundary");
  instancePanelReader.readIsolation = async (id, signal) => id === alternate.summary.id
    ? { ...healthyReport, instance_id: id } : originalReadIsolation.call(instancePanelReader, id, signal);
  await act(async () => {
    props = { ...props, instances: [alternate.summary], selectedInstanceId: alternate.summary.id,
      selectedDetails: alternate, selectedBackups: [], selectedModuleDetails: supportedModule };
    render();
  });
  await maintenanceSection("broadcast");
  await waitFor(() => !select<HTMLTextAreaElement>("textarea[name=broadcast-intent]").disabled, "alternate broadcast policy");
  const lateEvent = { ...broadcastEvents.get(instance.id)![0], status: "sent", message: "LATE OLD INSTANCE HISTORY" };
  await act(async () => { resolveLateHistory!([lateEvent]); });
  const broadcastHistoryIsolated = !select(".server-broadcast-history").textContent?.includes("LATE OLD INSTANCE HISTORY")
    && fixture.querySelectorAll(".server-broadcast-event").length === 0 && broadcastSendCount === 3;
  check(broadcastHistoryIsolated,
    "Late pending-history refresh must not replace the new instance's history or resend old messages");
  checks++;
  broadcastEvents.set(instance.id, [lateEvent, ...broadcastEvents.get(instance.id)!.slice(1)]);
  instancePanelReader.readIsolation = originalReadIsolation;
  await act(async () => { props = beforeBroadcastProps; render(); });
  await waitFor(() => !select<HTMLTextAreaElement>("textarea[name=broadcast-intent]").disabled
    && fixture.querySelector(".instance-isolation-panel")?.getAttribute("aria-busy") === "false", "restored maintenance controls");
  // The keyed instance panel remounted; visit its lazy import section again before
  // checking the complete workspace's original disclosure contract.
  await maintenanceSection("saves");

  const controlMeasurements = await measureMaintenanceControls(fixture, maintenanceSection, click);
  // "saves", "saves:autosave", "saves:import", "backups" or "backups:policy": a section at rest or with one popover open.
  const [previewId, previewChip] = (document.documentElement.dataset.maintenancePreview ?? "").split(":");
  const previewSection = sectionIds.find((id) => id === previewId);
  if (previewSection) {
    await maintenanceSection(previewSection);
    for (const open of fixture.querySelectorAll<HTMLButtonElement>('.maintenance-chip__summary[aria-expanded="true"]')) await act(async () => open.click());
    if (previewChips[previewChip]) await click(previewChips[previewChip]);
  }
  check(fixture.querySelectorAll('[role="tab"]').length === tabsBefore
    && fixture.querySelectorAll('[role="tablist"]').length === 1, "Maintenance introduced additional navigation tabs");
  const disclosures = [...select(".server-detail-scroll--maintenance").querySelectorAll("details")];
  check(disclosures.length === 0, "Maintenance guidance lives in help bubbles and operations in popovers, never in disclosures");
  verifyWorkspaceBounds();
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  checks++;
  return { status: "passed", checks, browser_errors: errors, control_measurements: controlMeasurements,
    control_violations: controlContractViolations(controlMeasurements), tab_count: tabsBefore,
    viewport: { width: innerWidth, height: innerHeight }, policy_alignment: policyAlignment,
    initial_maintenance_height: initialHeight, available_tab_height: select<HTMLElement>(".server-detail-scroll--maintenance").clientHeight,
    navigation_width: select(`${workspaceSelector} .configuration-workspace__sidebar`).getBoundingClientRect().width, compact_navigation: innerWidth <= 1100,
    settings_writes: savedSettings.length, settings_save_attempts: settingsSaveAttempts,
    import_attempts: importedSources.length, backup_restore_attempts: restoredBackups.length,
    broadcast_send_count: broadcastSendCount, broadcast_history_statuses: broadcastHistoryStatuses,
    broadcast_history_instance_isolated: broadcastHistoryIsolated };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`Interaction stalled after ${checks} checks`)), 25_000);
})]).finally(() => {
  clearTimeout(watchdog);
  instancePanelReader.readIsolation = originalReadIsolation;
}).catch((error) => ({ status: "failed", checks,
  error: `After ${checks} checks: ${error instanceof Error ? error.stack : String(error)}`, browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
