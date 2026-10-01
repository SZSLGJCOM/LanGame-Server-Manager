import React, { act, StrictMode, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { bootstrapApp, readInstanceDetails, readInstanceIsolation, readModuleDetails } from "../../src/api";
import { createDefaultAiSettings } from "../../src/ai-settings";
import { I18nProvider } from "../../src/i18n";
import { instancePanelReader } from "../../src/instance-panel-loader";
import { ServersView } from "../../src/views/ServersView";
import { InstanceSettingsSaveProvider, useInstanceSettingsSaveCoordinator } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceSettingsSaveCoordinator } from "../../src/views/settings/instance-settings-save-coordinator";
import type { DstWorldImportResult, InstanceBackupResult, InstanceDetails, InstanceIsolationReport, UpdateInstanceInput } from "../../src/types";
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
    const element = select<HTMLInputElement>(selector);
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
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
      `${label} clips a control or path: ${element.className || element.tagName}`);
  }
}
type MaintenanceSection = "backups" | "save-policy" | "runtime" | "storage" | "broadcast";
const sectionIds: MaintenanceSection[] = ["backups", "save-policy", "runtime", "storage", "broadcast"];
const workspaceSelector = ".server-detail-scroll--maintenance .configuration-workspace";
function navigationButton(id: MaintenanceSection) {
  return `${workspaceSelector} [data-configuration-section-id="${id}"] > button`;
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
  await click('[role="tab"][id$="-maintenance"]');
  await waitFor(() => fixture.querySelector(".instance-isolation-panel")?.getAttribute("aria-busy") === "false", "directory diagnosis");
  check(tabsBefore === 6 && fixture.querySelectorAll('[role="tab"]').length === tabsBefore,
    "Maintenance must retain the six existing instance tabs");
  check(fixture.querySelectorAll('[role="tablist"]').length === 1
    && !select(workspaceSelector).querySelector('[role="tablist"]'), "Maintenance introduced nested tabs");
  checks++;

  const runtimeCard = select<HTMLElement>(".server-runtime-policy-card");
  const isolation = select<HTMLElement>(".instance-isolation-panel");
  let outer = select<HTMLElement>(".server-detail-scroll--maintenance");
  const initialHeight = outer.scrollHeight;
  const importPanel = select<HTMLElement>(".dst-world-import");
  const policyPanel = select<HTMLElement>(".server-backup-policy-card");
  const navigation = select<HTMLElement>(`${workspaceSelector} .configuration-workspace__sidebar`);
  const content = select<HTMLElement>(`${workspaceSelector} .configuration-workspace__main`);
  check(select(navigationButton("backups")).getAttribute("aria-current") === "page", "Maintenance must open on backups");
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
  for (const name of ["maxRestarts", "waitSeconds", "onlyNonzeroExit"]) {
    const control = select<HTMLInputElement>(`.server-runtime-recovery-editor input[name="${name}"]`);
    control.scrollIntoView({ block: "nearest" });
    check(!control.disabled && visibleInside(control, outer), `Disabled recovery must retain its editable ${name} control`);
  }
  checks++;

  const resourceControls = [...fixture.querySelectorAll<HTMLInputElement>(".server-resource-policy input")];
  check(resourceControls.length === 3 && resourceControls.every((control) => !control.disabled),
    "Stopped instance must expose editable CPU, memory and reserve controls");
  check(resourceControls.map((control) => control.name).join(",") === "cpu,memory,reserve", "Resource policy lost a supported limit");
  checks++;

  await maintenanceSection("backups");
  await click(".server-file-backup-toolbar button");
  check(backupWrites.length === 1 && backupWrites[0] === instance.id, "Immediate backup lost its instance callback");
  checks++;

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

  await maintenanceSection("save-policy");
  await input(".server-backup-policy-card .server-save-policy-group input[type=number]", "7");
  await maintenanceSection("storage");
  await maintenanceSection("backups");
  check(importPanel.textContent?.includes("fixture/import/Cluster_1"), "Navigation discarded the selected import source");
  await maintenanceSection("save-policy");
  check(select<HTMLInputElement>(".server-backup-policy-card .server-save-policy-group input[type=number]").value === "7",
    "Navigation discarded an unsaved backup-policy draft");
  await click(".server-backup-policy-card button[type=submit]");
  check(savedSettings.length === 1 && savedSettings[0].backup_retention_count === 7, "Backup strategy did not save the edited retention");
  check(JSON.parse(savedSettings[0].settings_json).fixture_preserved_value === "keep", "Backup strategy overwrote unrelated settings");
  verifyHorizontalFit(policyPanel, "Save strategy form");
  checks++;

  await maintenanceSection("runtime");
  await click('.server-runtime-recovery-editor input[name="enabled"]');
  check(!select<HTMLInputElement>('.server-runtime-recovery-editor input[name="waitSeconds"]').disabled, "Enabling recovery did not enable its limits");
  await input('.server-runtime-recovery-editor input[name="maxRestarts"]', "0");
  check(select<HTMLButtonElement>(".server-runtime-recovery-editor button[type=submit]").disabled, "Invalid recovery limits can be saved");
  await input('.server-runtime-recovery-editor input[name="maxRestarts"]', "4");
  await input('.server-runtime-recovery-editor input[name="waitSeconds"]', "7.25");
  await click(".server-runtime-recovery-editor button[type=submit]");
  const recoveryWrite = savedSettings.at(-1)!;
  const recoveredSettings = JSON.parse(recoveryWrite.settings_json);
  check(savedSettings.length === 2 && recoveredSettings.runtime_restart.enabled
    && recoveredSettings.runtime_restart.max_restarts === 4 && recoveredSettings.runtime_restart.backoff_ms === 7250,
  "Recovery save changed its limits or seconds-to-milliseconds contract");
  check(recoveryWrite.backup_retention_count === 7 && recoveredSettings.fixture_preserved_value === "keep",
    "Recovery save lost another maintenance policy");
  verifyHorizontalFit(runtimeCard, "Recovery controls");
  checks++;

  await input('.server-runtime-recovery-editor input[name="maxRestarts"]', "");
  await click('.server-runtime-recovery-editor input[name="enabled"]');
  const disabledRecoveryLimit = select<HTMLInputElement>('.server-runtime-recovery-editor input[name="maxRestarts"]');
  check(!disabledRecoveryLimit.disabled && disabledRecoveryLimit.value === "",
    "Disabling recovery locked its invalid parameter and prevented correction");
  check(select<HTMLButtonElement>(".server-runtime-recovery-editor button[type=submit]").disabled,
    "Disabling recovery bypassed parameter validation");
  await input('.server-runtime-recovery-editor input[name="maxRestarts"]', "4");
  check(!select<HTMLButtonElement>(".server-runtime-recovery-editor button[type=submit]").disabled,
    "Correcting a disabled recovery parameter did not restore saving");
  await click(".server-runtime-recovery-editor button[type=submit]");
  check(savedSettings.length === 3 && JSON.parse(savedSettings[2].settings_json).runtime_restart.enabled === false,
    "Disabled recovery could not be persisted");
  checks++;

  await maintenanceSection("save-policy");
  await input(".server-backup-policy-card .server-save-policy-group input[type=number]", "9");
  holdNextSettingsSave = true;
  await click(".server-backup-policy-card button[type=submit]");
  check(rejectSettingsSave, "Policy save did not reach the controlled storage boundary");
  check(select(".server-backup-policy-card .shell-activity-notice.is-info").textContent?.includes("保存"),
    "In-flight policy save does not show its progress");
  check(savedSettings.length === 3 && settingsSaveAttempts === 4, "Pending policy save was reported as persisted");
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
    && !select<HTMLButtonElement>(".server-backup-policy-card button[type=submit]").disabled,
    "Failed policy save lost its draft or disabled retry");
  await click(".server-backup-policy-card button[type=submit]");
  check(settingsSaveAttempts === 5 && savedSettings.length === 4 && savedSettings[3].backup_retention_count === 9,
    "Retry did not persist the retained policy draft exactly once");
  check(policyPanel.querySelector(".shell-activity-notice.is-success") && !policyPanel.querySelector(".shell-activity-notice.is-error"),
    "Successful policy retry left the failure status visible");
  await coordinator.flush(instance.id);
  await input(".server-backup-policy-card .server-save-policy-group input[type=number]", "10");
  holdNextSettingsSave = true;
  await click(".server-backup-policy-card button[type=submit]");
  await act(async () => { rejectSettingsSave!(new Error("Fixture discarded policy write")); });
  const discardedFailure = await coordinator.flush(instance.id).then(() => null, (error: unknown) => error);
  check(discardedFailure instanceof Error, "The policy failure was not tracked before discarding");
  await click(".server-backup-policy-footer .ghost-button");
  await coordinator.flush(instance.id);
  check(select<HTMLInputElement>(".server-backup-policy-card .server-save-policy-group input[type=number]").value === "9"
    && savedSettings.length === 4, "Discard changed the persisted policy");
  checks++;

  await maintenanceSection("runtime");
  await click(".server-autostart-policy-editor input[type=checkbox]");
  check(autostartWrites.length === 1 && autostartWrites[0] && select<HTMLInputElement>(".server-autostart-policy-editor input").checked,
    "Autostart no longer persists independently from recovery");
  checks++;

  await maintenanceSection("storage");
  const directoryButtons = [...isolation.querySelectorAll<HTMLButtonElement>(".instance-isolation-paths button")];
  check(directoryButtons.length === 5, "Unified directories must include program, data, config, saves and backups");
  check(directoryButtons[0].textContent?.includes("程序位置") && directoryButtons[1].textContent?.includes("数据位置"),
    "Program and instance data locations must be named explicitly");
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
  await maintenanceSection("backups");
  await click(".dst-world-import-source-row .secondary-button");
  verifyWorkspaceBounds();
  checks++;

  const manyBackups = Array.from({ length: 30 }, (_, index) => ({ ...backup, backup_id: `history-${index}`,
    display_name: `联机备份 ${index + 1}`, backup_path: `fixture/backups/history-${index}` }));
  await act(async () => { props = { ...props, selectedBackups: manyBackups }; render(); });
  const lastRow = select<HTMLElement>(".server-file-backup-row:last-child");
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

  holdNextImport = true;
  await click(".dst-world-import-actions .primary-button");
  check(resolveImport && importedSources.length === 2, "Pending import did not reach its storage boundary");
  check([...fixture.querySelectorAll<HTMLButtonElement>(".dst-world-import button")].every((button) => button.disabled),
    "An in-flight import permits overlapping source selection or import");
  await click(".dst-world-import-actions .primary-button");
  check(importedSources.length === 2, "A repeated click dispatched a second import");
  await maintenanceSection("runtime");
  await maintenanceSection("backups");
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
  check(select<HTMLButtonElement>(".dst-world-import-actions .primary-button").disabled
    && !select(".dst-world-import").textContent?.includes("fixture/import/Cluster_1"), "Changing instances leaked the previous import source");
  await act(async () => { resolveImport!({ instance_id: instance.id, source_cluster_path: "fixture/import/Cluster_1",
    target_cluster_path: "fixture/current", safeguard_path: "fixture/backups/stale-result", imported_master: true,
    imported_caves: true, copied_file_count: 5, copied_total_bytes: 8192 }); });
  check(!select(".dst-world-import").textContent?.includes("stale-result"), "A completed old import changed the newly selected instance");
  await maintenanceSection("storage");
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
    ["backups", ".dst-world-import-source-row .secondary-button"], ["save-policy", ".server-backup-policy-card button[type=submit]"],
    ["runtime", '.server-runtime-recovery-editor input[name="enabled"]'], ["runtime", ".server-resource-policy input[name=cpu]"],
    ["storage", ".instance-isolation-paths button"], ["broadcast", "textarea[name=broadcast-intent]"]
  ] as const) {
    await maintenanceSection(id);
    const control = select<HTMLElement>(selector);
    control.scrollIntoView({ block: "nearest", inline: "nearest" });
    check(visibleInside(control, outer), `Maintenance cannot reach ${selector} at ${innerWidth} x ${innerHeight}`);
  }
  await maintenanceSection("backups");
  await click(".server-file-backup-toolbar button");
  check(backupWrites.length === 2 && backupWrites[1] === instance.id, "Category navigation lost immediate backup");
  checks++;

  const controlMeasurements = await measureMaintenanceControls(fixture, maintenanceSection, click);
  const previewSection = sectionIds.find((id) => id === document.documentElement.dataset.maintenancePreview);
  if (previewSection) await maintenanceSection(previewSection);
  check(fixture.querySelectorAll('[role="tab"]').length === tabsBefore
    && fixture.querySelectorAll('[role="tablist"]').length === 1, "Maintenance introduced additional navigation tabs");
  check(!select(".server-detail-scroll--maintenance").querySelector("details"), "Maintenance still contains a disclosure after interactions");
  verifyWorkspaceBounds();
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  checks++;
  return { status: "passed", checks, browser_errors: errors, control_measurements: controlMeasurements,
    control_violations: controlContractViolations(controlMeasurements), tab_count: tabsBefore,
    viewport: { width: innerWidth, height: innerHeight },
    initial_maintenance_height: initialHeight, available_tab_height: select<HTMLElement>(".server-detail-scroll--maintenance").clientHeight,
    navigation_width: select(`${workspaceSelector} .configuration-workspace__sidebar`).getBoundingClientRect().width, compact_navigation: innerWidth <= 1100,
    settings_writes: savedSettings.length, settings_save_attempts: settingsSaveAttempts,
    import_attempts: importedSources.length, backup_restore_attempts: restoredBackups.length };
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
