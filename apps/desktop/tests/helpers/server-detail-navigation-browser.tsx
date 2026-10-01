import React, { act, StrictMode, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { invokeMock } from "../../src/api-mock";
import { buildMockPorts, buildMockSavesPath, buildMockSettingsForModule } from "../../src/api-mock/module-settings";
import { createDefaultAiSettings } from "../../src/ai-settings";
import { I18nProvider } from "../../src/i18n";
import type { InstanceArchiveDetails, InstanceArchiveSummary } from "../../src/storage-management-types";
import type { BootstrapResponse, InstanceDetails, InstanceSummary, ModuleDetails } from "../../src/types";
import { ServersView } from "../../src/views/ServersView";
import type { ServerDetailTab } from "../../src/views/servers/ServerDetailTabs";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import { initializeScumSettings } from "../../src/views/settings/scum-server-settings-inventory";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const errors: string[] = [];
const unexpectedWrites: string[] = [];
const observedCommands: string[] = [];
const readOnlyQueries = new Set(["lookup_steam_workshop_items", "search_steam_workshop_items", "refresh_instance_live_players"]);
const isReadOnlyCommand = (command: string) => /^(read_|list_|inspect_)/.test(command) || readOnlyQueries.has(command);
const normalTabs: ServerDetailTab[] = ["runtime", "settings", "mods", "players", "maintenance", "gm"];
const archiveTabs: ServerDetailTab[] = ["runtime", "settings", "mods", "players", "maintenance"];
let checks = 0;
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };

function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
  checks++;
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const target = fixture.querySelector<T>(selector);
  if (!target) throw new Error(`Missing ${selector}`);
  return target;
}
function tab(id: ServerDetailTab) {
  return element<HTMLButtonElement>(`.server-detail-tabs [role="tab"][id$="-${id}"]`);
}
function assertTab(id: ServerDetailTab, context: string) {
  check(tab(id).getAttribute("aria-selected") === "true", `${context}: expected ${id}, selected ${fixture.querySelector('.server-detail-tabs [aria-selected="true"]')?.id}`);
  const panel = element('[role="tabpanel"]');
  check(panel.getAttribute("aria-labelledby") === tab(id).id, `${context}: selected tab must label the current detail panel`);
}
async function settleUntil(predicate: () => boolean, description: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(description);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
async function click(selector: string) {
  await act(async () => { element<HTMLButtonElement>(selector).click(); await frame(); });
}
async function chooseTab(id: ServerDetailTab) {
  check(!tab(id).disabled, `${id} must be enabled before user selection`);
  await act(async () => { tab(id).click(); await frame(); });
  assertTab(id, "User selection");
}
function unexpectedWrite(command: string): never {
  unexpectedWrites.push(command);
  throw new Error(`Navigation must not perform ${command}`);
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((yes) => { resolve = yes; });
  return { promise, resolve };
}
type ReadArgs = { input?: { archive_id?: string }; moduleId?: string; includePreservedProgramCounts?: boolean } & Record<string, unknown>;
const archiveReads: { id: string; operation: ReturnType<typeof deferred<InstanceArchiveDetails>> }[] = [];
const moduleReads: { id: string; operation: ReturnType<typeof deferred<ModuleDetails>> }[] = [];
let holdArchiveReads = false;
let holdModuleReads = false;
let archives: InstanceArchiveSummary[] = [];
const archiveSnapshots = new Map<string, InstanceArchiveDetails>();
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: (command: string, args: ReadArgs = {}) => {
  observedCommands.push(command);
  if (command === "list_instance_archives") return Promise.resolve({ archives, pending_deletions: [], issues: [] });
  if (command === "read_instance_archive_details") {
    const id = args.input?.archive_id ?? "";
    const details = archiveSnapshots.get(id);
    if (!details) return Promise.reject(new Error(`Unknown fixture archive ${id}`));
    if (!holdArchiveReads) return Promise.resolve(details);
    const operation = deferred<InstanceArchiveDetails>(); archiveReads.push({ id, operation }); return operation.promise;
  }
  if (command === "read_module_details" && args.includePreservedProgramCounts === false && holdModuleReads) {
    const operation = deferred<ModuleDetails>(); moduleReads.push({ id: args.moduleId ?? "", operation }); return operation.promise;
  }
  if (!isReadOnlyCommand(command)) {
    unexpectedWrites.push(command); return Promise.reject(new Error(`Navigation must not invoke ${command}`));
  }
  return invokeMock(command, args);
} } });

async function run() {
  // Only native IPC is replaced. Real cards, detail tabs and all six production workspaces render unchanged.
  const bootstrap = await invokeMock<BootstrapResponse>("bootstrap", { includeSystemSnapshot: false });
  const sourceA = bootstrap.state.instances.find((instance) => instance.module_id === "dontstarve");
  const sourceB = bootstrap.state.instances.find((instance) => instance.module_id === "minecraft");
  check(sourceA && sourceB, "Mock includes two independent instance identities");
  const originalA = await invokeMock<InstanceDetails>("read_instance_details_from_storage", { instanceId: sourceA.id });
  const a: InstanceSummary = { ...sourceA, name: "Navigation A", status: "Stopped", active_process_count: 0 };
  const b: InstanceSummary = { ...sourceB, name: "Navigation B", module_id: "dontstarve", status: "Stopped", active_process_count: 0 };
  const unsupported: InstanceSummary = { ...sourceB, name: "Unsupported navigation B", module_id: "scum", status: "Stopped", active_process_count: 0 };
  // Provide complete SCUM native data; opening its configuration must not synthesize missing sections or autosave.
  const scumSettings = initializeScumSettings(buildMockSettingsForModule("scum", unsupported.name, unsupported.id));
  const detailsFor = (summary: InstanceSummary): InstanceDetails => ({ ...originalA, summary, active_run: null,
    settings_json: summary.module_id === "scum" ? JSON.stringify(scumSettings, null, 2) : originalA.settings_json,
    ports: buildMockPorts(summary.module_id), saves_path: buildMockSavesPath(summary),
    config_file_path: `D:/fixture-only/${summary.id}/config/instance.json` });
  const normalDetails = new Map([[a.id, detailsFor(a)], [b.id, detailsFor(b)]]);
  const modules = new Map<string, ModuleDetails>();
  for (const moduleId of ["dontstarve", "scum", "minecraft"]) {
    modules.set(moduleId, await invokeMock<ModuleDetails>("read_module_details", { moduleId, includePreservedProgramCounts: false }));
  }
  for (const [id, summary] of [["archive-a", a], ["archive-b", b], ["archive-unsupported", unsupported]] as const) {
    const archive: InstanceArchiveSummary = { archive_id: id, instance_id: summary.id, instance_name: `Archived ${summary.name}`,
      module_id: summary.module_id, deleted_at_unix_ms: 1790553600000, archived_instance_root: `D:/fixture-only/archives/${id}`,
      previous_instance_root: `D:/fixture-only/${summary.id}`, preserved_external_saves_path: null, external_saves_backup_id: null,
      state: "archived", can_restore: true, can_purge: true, program_storage: "full", omitted_program_bytes: 0,
      omitted_program_files: 0, required_program_fingerprint: null, required_program_version: null, program_retention_reason: null, issues: [] };
    archives.push(archive);
    archiveSnapshots.set(id, { archive_id: id, instance: detailsFor({ ...summary, name: archive.instance_name! }),
      maintenance: { autostart: false, auto_backup_on_stop: false, backup_retention_count: 7, crash_restart_limit: 0, runtime_mode: "independent" },
      runs: { entries: [], total: 0, truncated: false }, log: { relative_path: null, text: `Retained ${id}`, truncated: false, issues: [] },
      backups: { entries: [], issues: [], truncated: false } });
  }
  const noOperation = () => {};
  const props: ComponentProps<typeof ServersView> = {
    aiSettings: createDefaultAiSettings(), assistantCanRun: false, bindAddressCandidates: [], instances: [a, b],
    moduleInstallations: { dontstarve: { installState: "Installed", hasManagedInstallSource: true } },
    instanceLaunchPlans: {}, instanceLaunchFailures: {}, section: "overview", onWorkspaceSectionChange: (section) => { props.section = section; render(); },
    selectedInstanceId: a.id, selectedDetails: normalDetails.get(a.id)!, selectedBackups: [], selectedModuleDetails: modules.get("dontstarve")!,
    selectedModuleDetailsError: null, runtime: null, runtimeWindows: null, launchPlan: null, launchPlanError: null, refreshIssue: null,
    onActivity: noOperation, onResumeAutoRefresh: noOperation,
    onSelectInstance: (id) => { props.selectedInstanceId = id; props.selectedDetails = null; props.selectedModuleDetails = null; render(); },
    onStart: () => unexpectedWrite("start"), onStop: () => unexpectedWrite("stop"), onInstallModule: async () => unexpectedWrite("install"),
    onOpenModuleLibrary: noOperation, onCreateInstance: () => unexpectedWrite("create instance"), onPickDirectory: async () => null,
    onImportDontStarveWorldData: async () => unexpectedWrite("import world"), onOpenLocalPath: () => unexpectedWrite("open path"),
    onSendRuntimeCommand: async () => unexpectedWrite("runtime command"), onSuppressRuntimeWindows: async () => unexpectedWrite("suppress windows"),
    onCreateBackup: () => unexpectedWrite("create backup"), onArchivesChanged: async () => unexpectedWrite("archive mutation refresh"),
    onArchiveInstance: async () => unexpectedWrite("archive instance"), onDeleteInstance: async () => unexpectedWrite("delete instance"),
    onRestoreBackup: () => unexpectedWrite("restore backup"), onRenameBackup: async () => unexpectedWrite("rename backup"),
    onDeleteBackup: async () => unexpectedWrite("delete backup"), onSaveSettings: async () => unexpectedWrite("save settings"),
    onSaveAutostart: async () => unexpectedWrite("save autostart"), onApplyPlayerAccessMutation: async () => unexpectedWrite("player mutation")
  };
  function render() {
    root.render(<StrictMode><I18nProvider><InstanceSettingsSaveProvider><ServersView {...props} /></InstanceSettingsSaveProvider></I18nProvider></StrictMode>);
  }
  async function finishSelection(summary: InstanceSummary, expected: ServerDetailTab) {
    await act(async () => { props.selectedDetails = detailsFor(summary); render(); await frame(); });
    assertTab(expected, "Details available before module definition");
    await act(async () => { props.selectedModuleDetails = modules.get(summary.module_id)!; render(); await frame(); });
    assertTab(expected, "Module definition available");
  }
  async function switchNormal(summary: InstanceSummary, expected: ServerDetailTab) {
    await click(`.server-list-card[data-card-id="${summary.id}"] .server-list-card-hitarea`);
    check(props.selectedInstanceId === summary.id && props.selectedDetails === null && props.selectedModuleDetails === null,
      "Selecting a real card enters the details and module loading transition");
    check(!fixture.querySelector(".server-detail-tabs"), "Pending details show the loading surface");
    await finishSelection(summary, expected);
  }
  async function changeMode(mode: "normal" | "archived") {
    await click(`[data-server-list-mode="${mode}"]`);
  }
  async function switchArchive(id: string, expected: ServerDetailTab) {
    const before = archiveReads.length;
    holdArchiveReads = true; holdModuleReads = true;
    await click(`.server-list-card[data-archive-id="${id}"] .server-list-card-hitarea`);
    await settleUntil(() => archiveReads.length > before, `Archive ${id} did not request its saved details`);
    assertTab(expected, "Archive details loading");
    const pending = archiveReads.at(-1)!;
    check(pending.id === id, "Archive reads must target the newly selected card");
    const beforeModules = moduleReads.length;
    await act(async () => { pending.operation.resolve(archiveSnapshots.get(id)!); await frame(); });
    await settleUntil(() => moduleReads.length > beforeModules, `Archive ${id} did not request its module`);
    assertTab(expected, "Archive module definition loading");
    const moduleRead = moduleReads.at(-1)!;
    await act(async () => { moduleRead.operation.resolve(modules.get(moduleRead.id)!); await frame(); });
    assertTab(expected, "Archive read complete");
    const workspace = element(".archived-instance-workspace");
    check(workspace.dataset.archiveId === id && workspace.getAttribute("aria-label") === archiveSnapshots.get(id)!.instance.summary.name,
      "Archive details must belong to the newly selected card");
    check(tab("gm").disabled, "Archived tools stay disabled without starting or querying a live server");
    holdArchiveReads = false; holdModuleReads = false;
  }
  await act(async () => { render(); await frame(); });
  await settleUntil(() => Boolean(fixture.querySelector(".server-detail-tabs")), "Normal detail tabs did not mount");
  check(fixture.querySelectorAll('.server-detail-tabs [role="tab"]').length === 6, "All six normal tabs must exist");
  // Run the known loading regression first so pre-fix execution fails at the target behavior.
  await chooseTab("mods");
  await switchNormal(b, "mods");
  for (const id of normalTabs) {
    await chooseTab(id);
    await switchNormal(props.selectedInstanceId === a.id ? b : a, id);
  }
  for (const id of ["mods", "gm"] as const) {
    await chooseTab(id);
    await act(async () => { props.instances = [a, unsupported]; render(); await frame(); });
    await switchNormal(unsupported, "runtime");
    check(tab(id).disabled, `Unsupported game disables ${id}`);
    await act(async () => { tab(id).click(); await frame(); });
    assertTab("runtime", "Disabled tab click");
    await switchNormal(a, id);
    await act(async () => { props.instances = [a, b]; render(); await frame(); });
  }
  // Explicitly choosing another available page replaces the remembered unavailable selection.
  await chooseTab("gm");
  await act(async () => { props.instances = [a, unsupported]; render(); await frame(); });
  await switchNormal(unsupported, "runtime");
  await chooseTab("settings");
  await switchNormal(a, "settings");
  await act(async () => { props.instances = [a, b]; render(); await frame(); });
  await changeMode("archived");
  await settleUntil(() => Boolean(fixture.querySelector('[data-archive-id="archive-a"]')), "Archives did not load");
  await switchArchive("archive-a", "runtime");
  check(fixture.querySelectorAll('.server-detail-tabs [role="tab"]').length === 6, "Archive workspace retains all six tab labels");
  for (const id of archiveTabs) {
    await chooseTab(id);
    const selectedArchive = element(".archived-instance-workspace").dataset.archiveId;
    await switchArchive(selectedArchive === "archive-a" ? "archive-b" : "archive-a", id);
  }
  await chooseTab("mods");
  await switchArchive("archive-unsupported", "runtime");
  check(tab("mods").disabled, "Unsupported archive disables mods");
  await switchArchive("archive-a", "mods");
  await chooseTab("players");
  await click('.server-list-card[data-archive-id="archive-a"] .server-list-card-hitarea');
  check(!fixture.querySelector(".archived-instance-workspace"), "Clicking the selected archive clears its workspace");
  await switchArchive("archive-b", "players");
  await changeMode("normal");
  assertTab("settings", "Returning to normal mode retains its independent selection");
  await chooseTab("maintenance");
  await changeMode("archived");
  if (!fixture.querySelector(".archived-instance-workspace")) await switchArchive("archive-a", "players");
  else assertTab("players", "Archive mode preserves its independent selection");
  await chooseTab("settings");
  await changeMode("normal");
  assertTab("maintenance", "Archive navigation does not replace normal selection");
  await changeMode("archived");
  if (!fixture.querySelector(".archived-instance-workspace")) await switchArchive("archive-b", "settings");
  else assertTab("settings", "Normal navigation does not replace archive selection");
  await act(async () => { tab("gm").click(); await frame(); });
  assertTab("settings", "Disabled archived tools do not change navigation");
  check(!fixture.querySelector(".gmt-workbench"), "Archives never mount live tools");
  check(unexpectedWrites.length === 0, `Card and tab navigation must perform no writes or runtime actions: ${unexpectedWrites.join(", ")}`);
  check(observedCommands.every(isReadOnlyCommand), "All observed IPC is read-only");
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  // Leave the selected archive and tab strip visible for the caller's screenshot.
  await act(async () => { window.scrollTo(0, 0); await frame(); });
  return { status: "passed", checks, normal_tabs: normalTabs, archive_tabs: archiveTabs, browser_errors: errors,
    unexpected_writes: unexpectedWrites, observed_commands: [...new Set(observedCommands)] };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`Navigation stalled after ${checks} checks`)), 30000);
})]).finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .catch((error) => ({ status: "failed", checks, error: `After ${checks} checks: ${error instanceof Error ? error.stack : String(error)}`,
    browser_errors: errors, unexpected_writes: unexpectedWrites }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
