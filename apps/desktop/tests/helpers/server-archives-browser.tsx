import React, { act, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider } from "../../src/i18n";
import { ServersView } from "../../src/views/ServersView";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import { createDefaultAiSettings } from "../../src/ai-settings";
import { invokeMock } from "../../src/api-mock";
import { listInstanceArchives } from "../../src/api-storage";
import type { BootstrapResponse, InstanceDetails, InstanceSummary } from "../../src/types";
import type { InstanceArchiveDetails, InstanceArchiveSummary, PendingInstanceDeletion } from "../../src/storage-management-types";
import type { LocaleCode } from "../../src/i18n-config";
import { ServerArchivesShell, createArchiveLayoutAssertions } from "./server-archives-shell";
import { assertNormalWorkspaceRestored, captureNormalWorkspace, runArchivePreviewAssertions, selectArchivePreviewSection, selectArchiveSettings } from "./server-archives-preview";
import { runArchiveWorkspaceAssertions } from "./server-archives-workspace";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "en-US");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const errors: string[] = [];
const checks: string[] = [];
const stableLayoutPhases: string[] = [];
const feedbackPhases: string[] = [];
const concurrencyPhases: string[] = [];
const previewPhases: string[] = [];
const workspacePhases: string[] = [];
const { layoutSnapshot, assertStableLayout, centeredIn, workspaceGeometry, unchangedWorkspace, notice }
  = createArchiveLayoutAssertions(fixture, check, stableLayoutPhases);
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
window.alert = window.confirm = window.prompt = () => { throw new Error("Unexpected native dialog"); };
function check(condition: unknown, description: string, diagnostics?: () => unknown): asserts condition {
  if (!condition) {
    const details = diagnostics?.();
    throw new Error(`${description}${details === undefined ? "" : `: ${JSON.stringify(details)}`}: ${fixture.textContent}`);
  }
  checks.push(description);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const result = fixture.querySelector<T>(selector);
  if (!result) throw new Error(`Missing ${selector}: ${fixture.textContent}`);
  return result;
}
function deferred() {
  let resolve!: (value: unknown) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
type Call = { command: string; args: { input?: { archive_id?: string }; instanceId?: string; instance_id?: string;
  moduleId?: string; module_id?: string; includePreservedProgramCounts?: boolean; include_preserved_program_counts?: boolean }; operation: ReturnType<typeof deferred> };
const calls: Call[] = [];
const observed: string[] = [];
const observedCalls: { command: string; args: Call["args"] }[] = [];
const moduleCalls: Call["args"][] = [];
const pendingModuleCalls: Call[] = [];
const controlled = new Set(["list_instance_archives", "read_instance_archive_details", "restore_instance_archive", "purge_instance_archive", "delete_instance_record"]);
let manualPreviewReads = false;
let failNextPreviewModuleRead = false;
let manualPreviewModuleReads = false;
let activeNativeRequests = 0;
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: (command: string, args: Call["args"] = {}) => {
  observed.push(command);
  observedCalls.push({ command, args });
  if (command === "read_module_details" && args.includePreservedProgramCounts === false) {
    moduleCalls.push(args);
    if (failNextPreviewModuleRead) { failNextPreviewModuleRead = false; return Promise.reject(new Error("Module definition unavailable MODULE_PREVIEW_FAILURE_END")); }
    if (manualPreviewModuleReads) {
      const operation = deferred(); pendingModuleCalls.push({ command, args, operation }); return operation.promise;
    }
  }
  if (!controlled.has(command)) return invokeMock(command, args);
  check(activeNativeRequests === 0, `Native storage requests remain serial before ${command}`); activeNativeRequests++;
  const operation = deferred(); calls.push({ command, args, operation });
  void operation.promise.then(() => { activeNativeRequests--; }, () => { activeNativeRequests--; });
  if (command === "read_instance_archive_details" && !manualPreviewReads) operation.resolve(archiveDetails(args.input?.archive_id ?? ""));
  return operation.promise;
} } });
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
async function click(selector: string) {
  await act(async () => {
    const target = element<HTMLButtonElement>(selector);
    target.focus({ preventScroll: true }); target.click(); await frame();
  });
}
async function nativeKey(name: "Tab" | "Escape" | "Enter") {
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/${name}`, { method: "POST" });
    check(response.ok, `Native ${name} dispatch succeeded`); await frame();
  });
}
async function dispatchKey(selector: string, key: string) {
  await act(async () => { element(selector).dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true })); await frame(); });
}
async function waitFor(selector: string) {
  const deadline = performance.now() + 5000;
  while (!fixture.querySelector(selector)) {
    if (performance.now() >= deadline) throw new Error(`Waiting for ${selector}: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
function last(command: string) {
  const result = calls.filter((entry) => entry.command === command).at(-1);
  if (!result) throw new Error(`Missing call ${command}`);
  return result;
}
async function resolve(call: Call, result: unknown) { await act(async () => { call.operation.resolve(result); await frame(); }); }
async function reject(call: Call, message: string) { await act(async () => { call.operation.reject(new Error(message)); await frame(); }); }
function archive(id: string, name: string, extra: Partial<InstanceArchiveSummary> = {}): InstanceArchiveSummary {
  return { archive_id: id, instance_id: `instance-${id}`, instance_name: name, module_id: "minecraft",
    deleted_at_unix_ms: 1790553600000, archived_instance_root: `D:/LanGame/archives/${id}`,
    previous_instance_root: `D:/LanGame/instances/instance-${id}`, preserved_external_saves_path: "E:/Worlds/Shared world",
    state: "archived", can_restore: true, can_purge: true, external_saves_backup_id: null,
    program_storage: "full", omitted_program_bytes: 0, omitted_program_files: 0,
    required_program_fingerprint: null, required_program_version: null, program_retention_reason: null,
    issues: [], ...extra };
}
const alpha = archive("a", "Alpha world", { program_storage: "reconstructable", omitted_program_bytes: 1024,
  omitted_program_files: 3, required_program_version: "1.21.8", required_program_fingerprint: "verified-library",
  archived_instance_root: `D:/LanGame/archives/${"retained-archive/".repeat(24)}ARCHIVE_PATH_END`,
  previous_instance_root: `D:/LanGame/instances/${"original-instance/".repeat(24)}ORIGINAL_PATH_END`,
  preserved_external_saves_path: `E:/Worlds/${"preserved-external-world/".repeat(24)}EXTERNAL_PATH_END`,
  program_retention_reason: "Modified program files retained PROGRAM_RETENTION_END",
  external_saves_backup_id: "archive-snapshot-a" });
const beta = archive("b", "Beta world", { module_id: "dontstarve", can_restore: false,
  issues: ["Archived port 25565 is reserved by another instance PORT_CONFLICT_END"] });
const historical = archive("c", "Historical world", { state: "missing_metadata", can_restore: false, issues: ["Archive metadata was not recorded."] });
const unsafe = archive("d", "Linked directory", { state: "unrecognized", can_restore: false, can_purge: false, issues: ["Archive root is a reparse point."] });
const missingProgram = archive("e", "Version mismatch", { module_id: "runescapedragonwilds", can_restore: false,
  issues: [`${"Exact archived program version unavailable. ".repeat(24)}PROGRAM_VERSION_END`] });
let archives = [alpha, beta, historical, unsafe, missingProgram];
const archivedFixturePasswords: Record<string, string> = {
  a: "fixture-server-password", b: "fixture-admin-password", c: "fixture-password",
  d: "fixture-pz-rcon-safe", e: "fixture-client-secret",
};
function archiveDetails(id: string): InstanceArchiveDetails {
  const source = archives.find((entry) => entry.archive_id === id);
  if (!source) throw new Error(`Missing archived configuration fixture ${id}`);
  return { archive_id: id, instance: {
    summary: { id: source.instance_id ?? `instance-${id}`, name: source.instance_name ?? "Unnamed archive", module_id: source.module_id ?? "minecraft",
      bind_ip: "192.168.1.42", status: "Stopped", active_process_count: 0, port_count: 1, autostart: true },
    config_file_path: `${source.archived_instance_root}/config/instance.json`, saves_path: source.preserved_external_saves_path ?? `${source.previous_instance_root}/saves`,
    backup_uses_declared_saves_path: true, auto_backup_on_stop: false, backup_retention_count: 7, active_run: null,
    ports: [{ name: "Game", protocol: "tcp", port: 25565 }],
    settings_json: JSON.stringify({ motd: `Archived ${source.instance_name}`, max_players: 8, archive_marker: `ARCHIVED_CONFIG_${id}`,
      rcon_password: archivedFixturePasswords[id],
      archived_false: false, archived_zero: 0, archived_empty: "", archived_null: null,
      foo_bar: "COLLISION_UNDERSCORE", "foo-bar": "COLLISION_HYPHEN", "测试甲": "CHINESE_COLLISION_FIRST", "测试乙": "CHINESE_COLLISION_SECOND",
      whitelist_entries: `00000000-0000-0000-0000-000000000001,ARCHIVE_PLAYER_${id}`,
      operator_entries: `00000000-0000-0000-0000-000000000002,ARCHIVE_ADMIN_${id},4,false`,
      runtime_performance: { resource_limits: { cpu_percent: 25, memory_limit_mib: 3072, host_memory_reserve_mib: 0 } },
      runtime_restart: { enabled: false, max_restarts: 2, backoff_ms: 0, only_nonzero_exit: true, restart_limit: 0 }, restart_policy: "on-failure", auto_restart_enabled: false,
      auto_restart_backoff_ms: 0, auto_restart_only_nonzero_exit: true,
      ...(source.module_id === "dontstarve" ? { shared_workshop_mod_ids: "22334455", master_enabled_workshop_mod_ids: "22334455",
        master_mod_configuration_options: { "22334455": { archive_note: `ARCHIVE_MOD_${id}` } } } : {}) }) },
    maintenance: { autostart: true, auto_backup_on_stop: false, backup_retention_count: 7, crash_restart_limit: 2, runtime_mode: "independent" },
    runs: { entries: [{ id: id === "a" ? 101 : 202, status: "stopped", started_at: "2026-09-28T07:50:00Z", stopped_at: "2026-09-28T08:00:00Z",
      exit_code: 0, crash_flag: false, display_name: `ARCHIVE_RUN_${id}` }], total: 1, truncated: false },
    log: { relative_path: `runtime/logs/retained-${id}.log`, text: `ARCHIVE_LOG_${id}\nSaved shutdown record ${id}`, truncated: false, issues: [] },
    backups: { entries: [{ backup_id: `archive-backup-${id}`, instance_id: source.instance_id ?? `instance-${id}`, backup_kind: "manual",
      display_name: `ARCHIVE_BACKUP_${id}`, created_at_unix_ms: 1790553600000,
      backup_path: `${source.archived_instance_root}/backups/archive-backup-${id}`, saves_path: `${source.archived_instance_root}/data/world`,
      file_count: 3, total_bytes: 4096 }], issues: [], truncated: false } };
}
const pending: PendingInstanceDeletion = { operation_id: "opaque-delete-operation", instance_id: "delete-target", instance_name: "Unfinished world",
  module_id: "minecraft", deleted_instance_root: "D:/LanGame/instances/delete-target", started_at_unix_ms: 1790553600000,
  can_retry: true, issues: ["A file is in use RETRY_REASON_END"] };
let pendingDeletions = [pending];
async function listed() { await resolve(last("list_instance_archives"), { archives, pending_deletions: pendingDeletions, issues: [] }); }
let props: ComponentProps<typeof ServersView>;
let globalInstances: InstanceSummary[] | null = null;
let viewMounted = true;
let changed = 0;
let created = 0;
let configurationSaves = 0;
let globalRefreshError: string | null = null;
let locale: LocaleCode = "en-US";
let bootstrap: BootstrapResponse;
let mount = 0;
const started: string[] = [];
const selected: string[] = [];
const retired: string[] = [];
const openedPaths: string[] = [];
const writeCallbacks: string[] = [];
const recordWrite = (name: string) => { writeCallbacks.push(name); };
function draw() {
  viewMounted = true;
  renderShell();
}
function renderShell() {
  root.render(<React.StrictMode><I18nProvider key={`${locale}:${mount}`}><InstanceSettingsSaveProvider>
    <ServerArchivesShell bootstrap={bootstrap} locale={locale} serverCount={props.instances.length}>
      {viewMounted ? <ServersView {...props} /> : <p>Closed servers page</p>}
    </ServerArchivesShell>
  </InstanceSettingsSaveProvider></I18nProvider></React.StrictMode>);
}
function hide() { viewMounted = false; renderShell(); }
async function setInput(selector: string, value: string) {
  await act(async () => {
    const input = element<HTMLInputElement>(selector);
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true })); await frame();
  });
}
async function search(value: string) { await setInput(".server-list-search", value); }
function contained(selector: string, parent: string) {
  const target = element(selector); target.scrollIntoView({ block: "nearest" });
  const rect = target.getBoundingClientRect(); const bounds = element(parent).getBoundingClientRect();
  check(rect.width > 0 && rect.height > 0 && rect.left >= bounds.left - 1 && rect.right <= bounds.right + 1
    && rect.top >= bounds.top - 1 && rect.bottom <= bounds.bottom + 1,
    `${selector} stays within ${parent}`, () => ({ rect: rect.toJSON(), bounds: bounds.toJSON() }));
}
async function initializeProps() {
  bootstrap = await invokeMock<BootstrapResponse>("bootstrap", { includeSystemSnapshot: false });
  const original = bootstrap.state.instances[0];
  check(original, "Server fixture includes an existing normal instance");
  const instance: InstanceSummary = { ...original, name: "Normal world", status: "Stopped", active_process_count: 0 };
  const details = { ...await invokeMock<InstanceDetails>("read_instance_details_from_storage", { instanceId: instance.id }), summary: instance, active_run: null };
  const noOperation = () => {};
  props = {
    aiSettings: createDefaultAiSettings(), assistantCanRun: false, bindAddressCandidates: [], instances: [instance],
    moduleInstallations: { [instance.module_id]: { installState: "Installed", hasManagedInstallSource: true } },
    instanceLaunchPlans: {}, instanceLaunchFailures: {}, section: "overview", onWorkspaceSectionChange: noOperation,
    selectedInstanceId: instance.id, selectedDetails: details, selectedBackups: [], selectedModuleDetails: null,
    runtime: null, runtimeWindows: null, launchPlan: null, launchPlanError: null, refreshIssue: null,
    onActivity: noOperation, onResumeAutoRefresh: noOperation, onSelectInstance: (id) => { selected.push(id); },
    onStart: (id) => { recordWrite("start"); started.push(id); }, onStop: () => { recordWrite("stop"); }, onInstallModule: async () => { recordWrite("install"); },
    onOpenModuleLibrary: noOperation, onCreateInstance: () => { created++; }, onPickDirectory: async () => null,
    onImportDontStarveWorldData: async () => { recordWrite("import world"); throw new Error("Unexpected world import"); },
    onOpenLocalPath: (path) => { openedPaths.push(path); }, onSendRuntimeCommand: async () => { recordWrite("console command"); return null; },
    onSuppressRuntimeWindows: async () => { recordWrite("suppress windows"); }, onCreateBackup: () => { recordWrite("create backup"); },
    onDeleteInstance: () => { recordWrite("delete instance"); }, onArchiveInstance: async (id) => { recordWrite("archive instance"); retired.push(id); },
    onArchivesChanged: async () => {
      check(activeNativeRequests === 0, "Global program statistics refresh waits for archive inventory to release its lock");
      changed++;
      if (globalRefreshError) throw new Error(globalRefreshError);
      if (globalInstances) { props = { ...props, instances: globalInstances }; if (viewMounted) draw(); }
    },
    onRestoreBackup: () => { recordWrite("restore backup"); }, onRenameBackup: async () => { recordWrite("rename backup"); return true; },
    onDeleteBackup: () => { recordWrite("delete backup"); }, onSaveSettings: async () => { recordWrite("save settings"); configurationSaves++; return undefined; },
    onSaveAutostart: async () => { recordWrite("save autostart"); },
    onApplyPlayerAccessMutation: async () => { recordWrite("player mutation"); throw new Error("Unexpected player mutation"); }
  };
  await act(async () => { draw(); await frame(); });
  await waitFor(".servers-page");
  return instance;
}
const mode = (value: "normal" | "archived") => `[data-server-list-mode="${value}"]`;
const card = (id: string) => `.server-list-card[data-archive-id="${id}"]`;
const primary = (id: string) => `${card(id)} .server-list-card-primary-action`;
const purge = (id: string) => `${card(id)} .server-list-card-delete > button`;
const openFolder = (id: string) => `${card(id)} .server-list-card-open-folder`;
const deletionCard = '.server-list-card[data-deletion-id="opaque-delete-operation"]';
const retry = `${deletionCard} .server-list-card-retry > button`;
const refreshNotice = ".shell-activity-notice.is-error .shell-activity-notice-actions button";
const busyError = "A storage scan or archive operation is in progress. Cancel the scan or wait for the operation to finish.";
async function changeMode(value: "normal" | "archived") { await click(mode(value)); await listed(); }
async function selectArchive(id: string) { await click(`${card(id)} .server-list-card-hitarea`); }
function alertContains(text: string) {
  return [...fixture.querySelectorAll('[role="alert"]')].some((node) => node.textContent?.includes(text));
}

async function run() {
  await document.fonts.load('500 13px "Inter"', "LanGame 0123456789"); await document.fonts.ready;
  const instance = await initializeProps();
  const originalDetails = props.selectedDetails;
  await waitFor(mode("archived"));
  check(element(mode("normal")).textContent === "Instances" && element(mode("archived")).textContent === "Archives",
    "English list names distinguish instances from archives");
  const normalHeight = element('.server-list-card[data-card-kind="instance"]').getBoundingClientRect().height;
  check(calls.length === 1 && calls[0].command === "list_instance_archives", "Server page lists archives without scanning storage");
  concurrencyPhases.push("StrictMode shared read");
  await click(mode("archived")); await click(mode("normal")); await click(mode("archived")); await click(mode("normal"));
  check(calls.length === 1, "Rapid list switches share the original pending native read"); concurrencyPhases.push("rapid switches");
  check(Boolean(fixture.querySelector(".server-detail-tabs")), "Normal instance retains its established detail tabs");
  const beforeListFailure = layoutSnapshot();
  const beforeFeedback = workspaceGeometry();
  await reject(last("list_instance_archives"), busyError);
  const busyNotice = notice(busyError, "error");
  unchangedWorkspace(beforeFeedback, "busy error shown"); feedbackPhases.push("busy error");
  assertStableLayout(beforeListFailure, "initial load failure");
  await act(async () => { busyNotice.querySelector<HTMLButtonElement>(".shell-activity-notice-close")!.focus(); });
  await nativeKey("Enter");
  check(!alertContains(busyError), "Closing the busy notification removes its visible alert");
  check(document.activeElement === element(".shell-activity-expand"), "Dismissing a focused notice keeps keyboard focus in the activity bar");
  unchangedWorkspace(beforeFeedback, "busy error dismissed"); feedbackPhases.push("dismissal");
  await click(mode("normal")); await reject(last("list_instance_archives"), busyError);
  notice(busyError, "error");
  const beforeRetryList = calls.length;
  await click(refreshNotice);
  check(calls.length === beforeRetryList + 1 && last("list_instance_archives") === calls.at(-1), "Bottom notification Refresh submits exactly one archive list request");
  await listed();
  check(!alertContains(busyError), "Successful refresh removes the previous busy failure");
  unchangedWorkspace(beforeFeedback, "busy retry complete"); feedbackPhases.push("retry");
  check(element(deletionCard).textContent?.includes("Unfinished world"), "Failed deletion remains reachable in the normal list after its database row disappears");
  contained(`${deletionCard} .server-list-card-copy`, `${deletionCard} .server-list-card-footer`);
  contained(`${deletionCard} .server-list-card-meta`, `${deletionCard} .server-list-card-footer`);
  contained(`${deletionCard} .server-list-card-primary-action`, `${deletionCard} .server-list-card-footer`);
  props = { ...props, instances: [instance, { ...instance, id: pending.instance_id, name: pending.instance_name }] };
  await act(async () => { draw(); await frame(); });
  check(fixture.querySelectorAll(deletionCard).length === 1 && !fixture.querySelector('.server-list-card[data-card-kind="instance"][data-card-id="delete-target"]'),
    "A retained database row does not duplicate its failed-deletion recovery card");
  props = { ...props, instances: [instance] }; await act(async () => { draw(); await frame(); });
  check(!fixture.querySelector('.server-list-card[data-card-kind="archive"]'), "Normal list does not mix recoverable archives into active instances");
  await click(".server-list-filter input");
  check(!fixture.querySelector('.server-list-card[data-card-kind="instance"]'), "Running-only filter hides stopped normal instances");
  const beforeToggle = layoutSnapshot();
  const normalIdentity = () => ({ selectedInstanceId: props.selectedInstanceId, detailsId: props.selectedDetails?.summary.id ?? null,
    moduleId: props.selectedDetails?.summary.module_id ?? null, settingsJson: props.selectedDetails?.settings_json ?? null });
  const normalSnapshot = captureNormalWorkspace(fixture, normalIdentity());
  await click(mode("archived"));
  assertStableLayout(beforeToggle, "archive switch loading");
  check(!fixture.querySelector(".server-detail-tabs") && !element(".server-detail-panel").textContent?.includes("Normal world"),
    "Archived mode never keeps another normal instance's workspace visible");
  await listed();
  assertStableLayout(beforeToggle, "archive switch complete");
  check(fixture.querySelectorAll('.server-list-card[data-card-kind="archive"]').length === 5, "Running-only filter does not hide archived instances");
  check(!fixture.querySelector('.server-list-card[data-card-kind="deletion-error"]'), "Archived list does not mislabel permanent deletion failures as recoverable archives");
  check(Math.abs(element(card("a")).getBoundingClientRect().height - normalHeight) <= 1, "Archived and normal instances share the same card dimensions");
  check(element(primary("a")).textContent?.includes("Restore") && !fixture.querySelector(`${card("a")} .server-card-connection`), "Archive primary action is Restore without a live connection control");
  const beforeArchiveSelection = { height: element(card("a")).getBoundingClientRect().height, scrollHeight: element(".server-list-panel > .table-list").scrollHeight };
  await runArchivePreviewAssertions({ fixture, check, click, setInput, selectArchive, changeMode, waitFor, resolve, reject,
    last, calls, moduleCalls, configuration: archiveDetails, setManualReads: (value) => { manualPreviewReads = value; },
    setManualModuleReads: (value) => { manualPreviewModuleReads = value; }, resolveModuleRead: async () => {
      const call = pendingModuleCalls.at(-1); check(call, "A module definition request is pending");
      await resolve(call, await invokeMock("read_module_details", call.args));
    },
    failNextModuleRead: () => { failNextPreviewModuleRead = true; }, originalModuleId: instance.module_id,
    runWorkspaceAssertions: async () => {
      manualPreviewReads = false;
      try { await runArchiveWorkspaceAssertions({ fixture, check, click, dispatchKey, waitFor, selectArchive,
        details: archiveDetails, writes: writeCallbacks, nativeCommands: observed, nativeCalls: observedCalls, normalSnapshot, phases: workspacePhases }); }
      finally { manualPreviewReads = true; }
    },
    normalSnapshot, normalIdentity, saves: () => configurationSaves, starts: () => started.length, phases: previewPhases });
  check(element(card("a")).classList.contains("is-active") && !fixture.querySelector(".server-archive-details"),
    "Archived selection highlights its card without expanding an archive detail panel");
  check(element(card("a")).getBoundingClientRect().height === beforeArchiveSelection.height
    && element(".server-list-panel > .table-list").scrollHeight === beforeArchiveSelection.scrollHeight,
    "Selecting an archive retains the card height and list scroll extent");
  assertStableLayout(beforeToggle, "archive selected");
  const archiveTooltip = element(`${card("a")} .server-list-card-hitarea`).title;
  check(archiveTooltip === element(`${card("a")} .server-list-card-meta`).title,
    "Archive card and metadata offer the same complete information tooltip");
  check(element(`${card("a")} .server-list-card-hitarea`).getAttribute("aria-description") === archiveTooltip,
    "Keyboard-focused archive cards expose the complete tooltip to assistive technology");
  check(["Archives preserve configuration, saves, mods and program changes", "Archived at", "Archive location", "Original location",
    "External saves retained", "Program files", "3 official files can be restored", "Required library version", "1.21.8",
    "matching verified program", "PROGRAM_RETENTION_END", "archive-snapshot-a", "does not start the server",
    new Date(alpha.deleted_at_unix_ms!).toLocaleString("en-US"), alpha.archived_instance_root, alpha.previous_instance_root!, alpha.preserved_external_saves_path!]
    .every((text) => archiveTooltip.includes(text)), "Archive tooltip preserves dates, complete long paths, program requirements, retention reason and external snapshot guidance");
  check(element(`${card("b")} .server-list-card-hitarea`).title.includes("Original program files retained"),
    "Full archives retain their original program storage explanation in the tooltip");
  check(!element(".server-list-panel").textContent?.includes("ARCHIVE_PATH_END")
    && !element(".server-list-panel").textContent?.includes("PROGRAM_RETENTION_END"), "Archive information stays in tooltips without adding a visible detail block");
  contained(openFolder("a"), card("a")); await click(openFolder("a"));
  check(openedPaths[0] === alpha.archived_instance_root, "Archive folder control opens the selected archive location");
  check(element(card("a")).classList.contains("is-active") && !fixture.querySelector(".server-archive-details"),
    "Opening an archive folder retains card selection without expanding details");
  check(selected.length === 0 && started.length === 0, "Selecting archived cards never selects or starts a normal runtime instance");
  await click(".server-archives-refresh"); await reject(last("list_instance_archives"), "Archive refresh unavailable ARCHIVE_REFRESH_FAILURE_END");
  notice("ARCHIVE_REFRESH_FAILURE_END", "error");
  assertStableLayout(beforeToggle, "archive refresh failure");
  await click(refreshNotice); await listed();
  await search("beta");
  check(fixture.querySelectorAll('.server-list-card[data-card-kind="archive"]').length === 1 && Boolean(fixture.querySelector(card("b"))), "Search filters archives by their instance name");
  await search("instance-a"); check(Boolean(fixture.querySelector(card("a"))), "Search also accepts the original instance ID");
  await search("minecraft"); check(fixture.querySelectorAll('.server-list-card[data-card-kind="archive"]').length === 3, "Search includes archive module IDs");
  await search("no matching archive"); check(!fixture.querySelector('.server-list-card[data-card-kind="archive"]'), "Archive search has a genuine empty state");
  await search(""); await changeMode("normal"); await click(".server-list-filter input");
  assertStableLayout(beforeToggle, "normal list restored");
  assertNormalWorkspaceRestored(fixture, check, normalSnapshot, normalIdentity());
  check(Boolean(fixture.querySelector(`.server-list-card[data-card-kind="instance"][data-card-id="${instance.id}"].is-active`)),
    "Returning to normal instances preserves the selected instance");
  await click(`${deletionCard} .server-list-card-hitarea`);
  assertStableLayout(beforeToggle, "failed deletion selected");
  await click('.server-list-card[data-card-kind="instance"] .server-list-card-hitarea');
  check(selected.at(-1) === instance.id && props.selectedInstanceId === instance.id, "Normal instance selection still targets the original workspace");
  const previousLists = calls.filter((call) => call.command === "list_instance_archives").length;
  await click('.server-list-card[data-card-kind="instance"] .server-list-card-archive > button');
  await click(".inline-confirm-submit");
  check(retired.length === 1 && retired[0] === instance.id && calls.filter((call) => call.command === "list_instance_archives").length === previousLists + 1,
    "Archiving a normal card refreshes the archive collection immediately");
  await listed();
  props = { ...props, instances: [], selectedInstanceId: null, selectedDetails: null };
  await act(async () => { draw(); await frame(); });
  check(Boolean(fixture.querySelector(mode("archived"))) && Boolean(fixture.querySelector(deletionCard)), "Zero normal instances preserve the archive switch and failed deletion recovery");
  check(!fixture.textContent?.includes("Create your first server") && !fixture.querySelector(".server-workspace-empty .primary-button"),
    "Existing archives do not show first-server onboarding when the normal list is empty");
  await search("Unfinished"); check(Boolean(fixture.querySelector(deletionCard)), "Normal-list search includes failed deletion entries"); await search("");
  await changeMode("archived"); await selectArchive("e");
  check(element<HTMLButtonElement>(primary("e")).disabled && element(primary("e")).title.includes(missingProgram.issues[0])
    && element(`${card("e")} .server-list-card-hitarea`).title.includes(missingProgram.issues[0])
    && element(`${card("e")} .server-list-card-meta`).title.includes("PROGRAM_VERSION_END")
    && !fixture.querySelector(".server-archive-details"), "Unavailable archived program version disables Restore and preserves its complete long error in tooltips");
  check(element(primary("e")).getAttribute("aria-description") === element(primary("e")).title,
    "Disabled Restore retains the complete recovery constraint as an accessible description");
  check(element<HTMLButtonElement>(primary("c")).disabled && !element<HTMLButtonElement>(purge("c")).disabled, "Historical metadata may be explicitly purged but cannot be restored");
  check(element<HTMLButtonElement>(primary("d")).disabled && element<HTMLButtonElement>(purge("d")).disabled, "Unrecognized linked archives expose no unsafe action");

  await selectArchive("a"); const beforeRestore = changed;
  await click(primary("a")); await click(primary("a"));
  check(calls.filter((call) => call.command === "restore_instance_archive").length === 1 && last("restore_instance_archive").args.input?.archive_id === "a",
    "Restore submits one request carrying the selected opaque archive ID");
  check(element<HTMLButtonElement>(primary("b")).disabled && element<HTMLButtonElement>(purge("b")).disabled, "One archive mutation owns the shared action gate");
  const listsBeforeMutationSwitch = calls.filter((call) => call.command === "list_instance_archives").length;
  await click(mode("normal")); await click(mode("archived")); await click(mode("normal")); await click(mode("archived"));
  check(calls.filter((call) => call.command === "list_instance_archives").length === listsBeforeMutationSwitch, "Switching lists during Restore does not send an overlapping native read");
  await reject(last("restore_instance_archive"), "Previous runtime directory occupied RESTORE_FAILURE_END");
  check(calls.filter((call) => call.command === "list_instance_archives").length === listsBeforeMutationSwitch + 1, "Restore completion and queued switches share exactly one fresh inventory request");
  check(changed === beforeRestore, "Global refresh does not race the post-restore inventory read");
  concurrencyPhases.push("mutation switches"); await listed();
  check(changed === beforeRestore + 1 && alertContains("RESTORE_FAILURE_END"), "Failed restore refreshes global instances and retains its failure reason");
  check(!element<HTMLButtonElement>(primary("a")).disabled, "Failed restore releases the gate for explicit retry");
  await click(primary("a")); archives = archives.filter((entry) => entry.archive_id !== "a");
  globalInstances = [{ ...instance, id: "instance-a", name: "Alpha world" }];
  await resolve(last("restore_instance_archive"), { archive_id: "a", instance_id: "instance-a", instance_name: "Alpha world", restored_instance_root: alpha.previous_instance_root,
    external_saves_restore_required: true, external_saves_backup_id: "archive-snapshot-a", preserved_external_saves_path: alpha.preserved_external_saves_path }); await listed();
  check(changed === beforeRestore + 2 && !fixture.querySelector(card("a")) && !alertContains("RESTORE_FAILURE_END"), "Successful restore removes the archive and clears the previous failure");
  check(!fixture.querySelector('.archived-instance-workspace[data-archive-id="a"]'), "Restoring the selected archive removes its obsolete retained-data workspace");
  previewPhases.push("restored archive removed");
  await click(".shell-activity-expand");
  check(element(".shell-activity-panel").getAttribute("role") === "dialog", "Concurrent restore feedback remains reachable in message details");
  check(fixture.textContent?.includes("External saves have not been restored") && fixture.textContent?.includes("archive-snapshot-a"), "Restore does not falsely claim the external world snapshot was restored");
  notice("Instance files restored. The server remains stopped.", "success"); feedbackPhases.push("success");
  const recoveryGeometry = workspaceGeometry();
  const externalWarning = notice("External saves have not been restored", "warning");
  notice("External saves remain at", "warning"); feedbackPhases.push("external warning");
  await act(async () => {
    const close = externalWarning.querySelector<HTMLButtonElement>(".shell-activity-notice-close")!;
    close.focus({ preventScroll: true }); close.click(); await frame();
  });
  unchangedWorkspace(recoveryGeometry, "external warning dismissed");
  await nativeKey("Escape");
  check(!fixture.querySelector(".shell-activity-panel"), "Message details close without leaving restore feedback over the workspace");
  await changeMode("normal");
  check(Boolean(fixture.querySelector('.server-list-card[data-card-kind="instance"][data-card-id="instance-a"]')) && started.length === 0,
    "Restored instance returns to the normal list after global refresh without automatically starting");
  globalInstances = []; props = { ...props, instances: [] }; await act(async () => { draw(); await frame(); });
  await changeMode("archived");
  const beforePurge = changed; await selectArchive("b"); await click(purge("b"));
  check(element(".inline-confirm-message").textContent?.includes("Beta world") && element(".inline-confirm-message").textContent?.includes("cannot be recovered"), "Purge confirmation names the exact archive and irreversible consequence");
  check(document.activeElement === element(".inline-confirm-buttons button"), "Destructive confirmation initially focuses Cancel");
  contained(".inline-confirm-review", card("b")); contained(".inline-confirm-buttons", card("b")); await nativeKey("Escape");
  check(!fixture.querySelector(".inline-confirm-review") && document.activeElement === element(purge("b")) && !calls.some((call) => call.command === "purge_instance_archive"),
    "Escape cancels purge without sending a command and restores the card trigger");
  await click(purge("b")); await click(".inline-confirm-submit");
  check(last("purge_instance_archive").args.input?.archive_id === "b", "Purge sends an opaque archive ID rather than a filesystem path");
  await reject(last("purge_instance_archive"), "File still in use PURGE_FAILURE_END");
  archives = archives.map((entry) => entry.archive_id === "b" ? { ...entry, state: "purging", can_restore: false, issues: ["Partial removal; retry to finish."] } : entry); await listed();
  check(alertContains("PURGE_FAILURE_END") && element<HTMLButtonElement>(primary("b")).disabled && !element<HTMLButtonElement>(purge("b")).disabled,
    "Partial purge exposes its original cause and permits retry without restoring incomplete files");
  await click(purge("b")); await click(".inline-confirm-submit"); await resolve(last("purge_instance_archive"), { archive_id: "b", purged: false }); await listed();
  check(changed === beforePurge + 2 && alertContains("did not complete") && !fixture.textContent?.includes("permanently removed"), "Incomplete purge refreshes state and never reports success");
  await click(purge("b")); await click(".inline-confirm-submit"); archives = archives.filter((entry) => entry.archive_id !== "b");
  await resolve(last("purge_instance_archive"), { archive_id: "b", purged: true }); await listed();
  check(changed === beforePurge + 3 && !fixture.querySelector(card("b")), "Successful purge refreshes both collections and removes only the selected archive");
  check(!fixture.querySelector('.archived-instance-workspace[data-archive-id="b"]'), "Purging the selected archive removes its obsolete retained-data workspace");
  previewPhases.push("purged archive removed");

  await changeMode("normal"); const emptyWorkspace = layoutSnapshot(); await click(`${deletionCard} .server-list-card-hitarea`);
  check(element(".server-list-panel").contains(element('.server-archive-details[data-deletion-id="opaque-delete-operation"]'))
    && element('.server-archive-details[data-deletion-id="opaque-delete-operation"]').textContent?.includes("RETRY_REASON_END"),
    "Failed deletion details expose the original reason inside the list");
  assertStableLayout(emptyWorkspace, "failed deletion with empty workspace");
  const beforeRetry = changed; await click(retry);
  check(element(".inline-confirm-message").textContent?.includes("Unfinished world"), "Deletion retry confirms the exact instance name");
  contained(".inline-confirm-buttons", deletionCard); await click(".inline-confirm-buttons button");
  check(!calls.some((call) => call.command === "delete_instance_record"), "Cancelling failed-deletion retry sends no mutation");
  await click(retry); await click(".inline-confirm-submit");
  check(last("delete_instance_record").args.instanceId === "delete-target", "Permanent deletion retry sends the instance ID instead of its operation or archive ID");
  await reject(last("delete_instance_record"), "Still locked DELETE_FAILURE_END"); await listed();
  check(changed === beforeRetry + 1 && alertContains("DELETE_FAILURE_END") && !element<HTMLButtonElement>(retry).disabled, "Failed retry refreshes instances, exposes its cause and remains retryable");
  await click(retry); await click(".inline-confirm-submit"); pendingDeletions = [];
  await resolve(last("delete_instance_record"), { instance_id: "delete-target", instance_name: "Unfinished world", module_id: "minecraft",
    deleted_at_unix_ms: 1790553600000, deleted_instance_root: pending.deleted_instance_root, preserved_external_saves_path: "E:/Worlds/Kept external world",
    program_cleanup: { removed_install_roots: [], preserved_data_paths: [], retained_installs: [] } }); await listed();
  await click(".shell-activity-expand");
  check(changed === beforeRetry + 2 && !fixture.querySelector(deletionCard) && fixture.textContent?.includes("External saves remain at E:/Worlds/Kept external world"),
    "Completed deletion removes the failed card while explicitly preserving external saves");
  notice("External saves remain at E:/Worlds/Kept external world", "warning");
  await nativeKey("Escape");
  check(Boolean(fixture.querySelector(mode("archived"))), "Empty normal list still retains navigation to archives");
  check(!fixture.textContent?.includes("Create your first server")
    && element(".server-workspace-empty").textContent?.includes("Restore an instance from the Archives list"),
    "An archive-only workspace offers recovery without first-server onboarding in either panel");
  check(Boolean(fixture.querySelector(".workspace-metrics")), "An archive-only workspace retains the top metrics bar");
  const archiveOnlyMetrics = [...fixture.querySelectorAll(".workspace-metric-value")].map((node) => node.textContent);
  check(archiveOnlyMetrics.join("|") === "0|0|Not selected|0",
    "Archived instances are not counted as normal, autostarting or registered-port instances");
  contained(".workspace-metrics", ".servers-page");
  check(element(".workspace-metrics").getBoundingClientRect().bottom <= element(".server-list-panel").getBoundingClientRect().top,
    "The archive-only metrics remain visible above the instance list");
  const archiveOnlyWorkspace = layoutSnapshot();
  centeredIn(".server-workspace-empty", ".server-detail-panel");
  await changeMode("archived");
  assertStableLayout(archiveOnlyWorkspace, "archive only switch");
  check(fixture.querySelectorAll('.server-list-card[data-card-kind="archive"]').length === 3, "Remaining archives stay available with no normal instances");
  await search("no matching archive");
  check(!fixture.querySelector('.server-list-card[data-card-kind="archive"]') && !fixture.textContent?.includes("Create your first server"),
    "An empty archive search result is not a new workspace");
  assertStableLayout(archiveOnlyWorkspace, "archive only search");
  await search("");
  await click(".server-archives-refresh");
  assertStableLayout(archiveOnlyWorkspace, "archive only refresh loading");
  await reject(last("list_instance_archives"), "Archive-only refresh unavailable");
  assertStableLayout(archiveOnlyWorkspace, "archive only refresh failure");
  await click(refreshNotice); await listed();

  await click(".server-archives-refresh"); await reject(last("list_instance_archives"), busyError); notice(busyError, "error");
  await act(async () => { hide(); });
  check(Boolean(fixture.querySelector(".shell-activity-bar")) && !fixture.querySelector(".shell-activity-notice"), "Leaving the server page clears its notifications while retaining the shell");
  feedbackPhases.push("unmount");
  await act(async () => { draw(); await frame(); });
  const abandonedList = last("list_instance_archives"), beforeRemountCalls = calls.length; await act(async () => { hide(); });
  mount++; archives = [alpha, beta]; await act(async () => { draw(); await frame(); });
  check(last("list_instance_archives") === abandonedList && calls.length === beforeRemountCalls, "Remount and StrictMode reuse the unfinished native read without reviving the unmounted view");
  await listed(); await changeMode("archived");
  check(last("list_instance_archives") !== abandonedList && Boolean(fixture.querySelector(card("a"))) && !fixture.querySelector(card("d")), "The remounted view receives shared data and its next refresh sends a new native read");
  concurrencyPhases.push("remount shared read");
  await click(primary("a")); const beforeUnmountedRestore = changed; await act(async () => { hide(); });
  await resolve(last("restore_instance_archive"), { archive_id: "a", instance_id: "instance-a", instance_name: "Alpha world", restored_instance_root: alpha.previous_instance_root,
    external_saves_restore_required: false, external_saves_backup_id: null, preserved_external_saves_path: null });
  check(changed === beforeUnmountedRestore + 1, "A restore completing after navigation still refreshes global instances");
  mount++; archives = []; pendingDeletions = [];
  await act(async () => { draw(); await frame(); });
  check(!fixture.textContent?.includes("Create your first server"), "Archive inventory must resolve before first-server onboarding appears");
  await reject(last("list_instance_archives"), "Archive inventory unavailable");
  check(!fixture.textContent?.includes("Create your first server"), "An inventory failure is not treated as an empty workspace");
  await click(refreshNotice);
  await resolve(last("list_instance_archives"), { archives: [], pending_deletions: [], issues: ["Archive directory inaccessible"] });
  await click(".shell-activity-expand");
  notice("Archive directory inaccessible", "warning"); feedbackPhases.push("inventory warning");
  await nativeKey("Escape");
  check(!fixture.textContent?.includes("Create your first server"), "Incomplete archive inventory is not treated as a new workspace");
  pendingDeletions = [pending]; await changeMode("normal");
  check(Boolean(fixture.querySelector(deletionCard)) && !fixture.textContent?.includes("Create your first server"),
    "A deletion recovery record prevents first-server onboarding even without archives");
  check(Boolean(fixture.querySelector(".workspace-metrics")), "Deletion-only workspaces retain the top metrics bar");
  pendingDeletions = []; await changeMode("normal");
  check(element(".server-workspace-empty h3").textContent === "Create your first server"
    && !element(".server-list-panel").textContent?.includes("Create your first server"),
    "A confirmed empty workspace keeps one first-server introduction instead of duplicating it in the list");
  check(!fixture.querySelector(".workspace-metrics"), "A confirmed empty workspace keeps its existing introduction layout");
  centeredIn(".server-workspace-empty", ".server-detail-panel");
  centeredIn(".server-list-notice", ".server-list-panel > .table-list");
  await click(".server-workspace-empty .primary-button");
  check(created === 1, "The truly empty workspace still opens server creation");
  await click(mode("archived"));
  check(!fixture.querySelector(".server-detail-tabs") && !fixture.querySelector(".server-workspace-empty .primary-button"),
    "An empty Archives list shows archive selection guidance while inventory refreshes");
  await listed();
  centeredIn(".server-list-notice", ".server-list-panel > .table-list");
  await click(".server-archives-refresh"); await reject(last("list_instance_archives"), "Empty inventory refresh failed");
  check(!fixture.textContent?.includes("Create your first server"), "A failed refresh does not present stale empty inventory as first-server onboarding");
  archives = [alpha]; await click(refreshNotice); await listed();
  globalRefreshError = "INSTANCE_REFRESH_FAILURE_END";
  await click(primary("a")); archives = [];
  await resolve(last("restore_instance_archive"), { archive_id: "a", instance_id: "instance-a", instance_name: "Alpha world",
    restored_instance_root: alpha.previous_instance_root, external_saves_restore_required: false,
    external_saves_backup_id: null, preserved_external_saves_path: null });
  check(!fixture.textContent?.includes("Create your first server"), "Restoring the last archive does not introduce first-server onboarding while refreshing instances");
  await listed();
  check(alertContains("INSTANCE_REFRESH_FAILURE_END") && !fixture.textContent?.includes("Create your first server"),
    "A restored instance with failed global refresh is not misrepresented as a new workspace");
  globalRefreshError = null;
  const originalArchiveAction = props.onArchiveInstance;
  props = { ...props, instances: [instance], selectedInstanceId: instance.id, selectedDetails: originalDetails,
    onArchiveInstance: async (id) => {
      retired.push(id); props = { ...props, instances: [], selectedInstanceId: null, selectedDetails: null }; draw();
    } };
  mount++; await act(async () => { draw(); await frame(); }); await listed();
  const beforeLastArchiveMetrics = element(".workspace-metrics");
  await click('.server-list-card[data-card-kind="instance"] .server-list-card-archive > button');
  await click(".inline-confirm-submit");
  check(!fixture.textContent?.includes("Create your first server"),
    "Archiving the last normal instance waits for archive inventory after normal instances have refreshed");
  check(fixture.querySelector(".workspace-metrics") === beforeLastArchiveMetrics,
    "The metrics bar stays mounted while the last normal instance moves to archives");
  archives = [alpha]; await listed();
  check(element(".server-workspace-empty").textContent?.includes("Restore an instance from the Archives list"),
    "The completed last-instance archive offers recovery rather than first-server onboarding");
  props = { ...props, onArchiveInstance: originalArchiveAction };
  await changeMode("archived"); await click(purge("a")); await click(".inline-confirm-submit");
  archives = []; await resolve(last("purge_instance_archive"), { archive_id: "a", purged: true });
  check(!fixture.textContent?.includes("Create your first server"), "Purging the last archive waits for refreshed inventory");
  await listed();
  check(!fixture.querySelector(".archived-instance-workspace[data-archive-id]") && !fixture.querySelector(".server-detail-tabs"),
    "Purging the last archive clears the archived configuration workspace");
  check(!fixture.querySelector(".workspace-metrics"), "The metrics bar hides after the last archive is purged and both inventories confirm empty");
  await changeMode("normal");
  check(element(".server-workspace-empty h3").textContent === "Create your first server",
    "The confirmed empty normal list restores first-server onboarding after the last archive is purged");
  Object.assign(window, { isTauri: false }); sessionStorage.setItem("langameLanToken", "isolated-fixture-token"); mount++;
  const beforeLan = calls.length; await act(async () => { draw(); await frame(); });
  check(element<HTMLButtonElement>(mode("archived")).disabled && element(mode("archived")).title.includes("server host")
    && !fixture.querySelector('.server-list-card[data-card-kind="archive"]'), "LAN disables archive access with its host-only explanation");
  try { await listInstanceArchives(); throw new Error("LAN unexpectedly allowed"); }
  catch (error) { check(String(error).includes("desktop host"), "Archive API also rejects LAN transport directly"); }
  check(calls.length === beforeLan, "LAN sends no storage management command");
  sessionStorage.removeItem("langameLanToken"); Object.assign(window, { isTauri: true });
  locale = "zh-CN"; localStorage.setItem("langame.locale", locale); mount++; archives = [alpha, beta, historical, unsafe, missingProgram];
  props = { ...props, instances: [instance], selectedInstanceId: instance.id, selectedDetails: originalDetails };
  await act(async () => { draw(); await frame(); }); await listed(); await changeMode("archived"); await selectArchive("a");
  check(element(mode("normal")).textContent === "实例" && element(mode("archived")).textContent === "归档", "Chinese mode switch clearly names the instance and archive lists");
  check(element(`${card("a")} .server-list-card-status`).textContent === "已归档", "Archive status remains distinct from the list name");
  check(element(primary("a")).textContent?.includes("还原"), "Chinese archive card replaces Start with Restore");
  const chineseTooltip = element(`${card("a")} .server-list-card-hitarea`).title;
  check(["归档保留配置、存档、模组和程序修改", "归档时间", "归档位置", "原始位置", "保留的外部存档", "程序文件",
    "3 个官方文件可由本地程序库还原", "所需程序库版本", "不会自动下载缺失文件", "不会自动启动", "不会自动覆盖原外部目录",
    "archive-snapshot-a", "ARCHIVE_PATH_END", "ORIGINAL_PATH_END", "EXTERNAL_PATH_END", "PROGRAM_RETENTION_END",
    new Date(alpha.deleted_at_unix_ms!).toLocaleString("zh-CN")]
    .every((text) => chineseTooltip.includes(text)) && chineseTooltip === element(`${card("a")} .server-list-card-meta`).title,
    "Chinese archive tooltip preserves every recovery detail and full long paths");
  check(!fixture.querySelector(".server-archive-details") && element(primary("e")).title.includes("PROGRAM_VERSION_END")
    && element(primary("e")).title.includes("无法恢复"), "Chinese disabled Restore explains the original cause without expanding archive details");
  await click(purge("a")); check(element(".inline-confirm-message").textContent?.includes("Alpha world") && element(".inline-confirm-message").textContent?.includes("无法通过 LanGame 恢复"),
    "Chinese purge confirmation preserves the named irreversible warning");
  check(getComputedStyle(element(openFolder("a"))).visibility === "hidden",
    "Purge confirmation hides the folder action while its review owns the card");
  contained(".inline-confirm-buttons", card("a")); await nativeKey("Escape");
  props = { ...props, instances: [], selectedInstanceId: null, selectedDetails: null };
  await act(async () => { draw(); await frame(); });
  check(!fixture.textContent?.includes("创建你的第一个服务器")
    && element('.archived-instance-workspace[data-archive-id="a"]').getAttribute("aria-label") === "Alpha world",
    "Chinese archive-only workspace keeps the selected archive configuration instead of first-server onboarding");
  await waitFor('.archived-instance-workspace[data-archive-id="a"][data-archive-configuration-state="ready"]');
  await selectArchiveSettings({ click, waitFor }, "a");
  await selectArchivePreviewSection({ fixture, click, waitFor }, "a", "saved_settings");
  check(element<HTMLInputElement>('.archived-instance-workspace[data-archive-id="a"] [data-field-key="archive_marker"] input').value === "ARCHIVED_CONFIG_a",
    "Final Chinese preview displays the selected archive's saved configuration without toggling selection off");
  contained(".workspace-metrics", ".servers-page");
  contained(".archived-instance-workspace", ".server-detail-panel");
  const finalGeometry = workspaceGeometry();
  await click(".server-archives-refresh"); await reject(last("list_instance_archives"), busyError);
  notice(busyError, "error"); unchangedWorkspace(finalGeometry, "final notification preview");
  contained(".shell-activity-bar", ".shell-frame");
  check(document.documentElement.scrollWidth <= innerWidth, "Archive page has no horizontal overflow");
  check(!observed.some((command) => command === "scan_storage_usage" || command === "cancel_storage_usage_scan"), "Server archive workflow never starts the removed storage-scan UI");
  check(activeNativeRequests === 0, "Every native storage request has settled at fixture completion");
  const screenshotTab = await (await fetch("/__server_archive_screenshot_tab")).text();
  check(["settings", "runtime", "maintenance"].includes(screenshotTab), "Visual capture uses an explicit retained-data tab");
  if (screenshotTab !== "settings") {
    await click(`.archived-instance-workspace[data-archive-id="a"] .server-detail-tab[id$="-${screenshotTab}"]`);
    await waitFor(`.archived-instance-workspace[data-archive-id="a"] [data-archive-workspace-tab="${screenshotTab}"]`);
    const marker = screenshotTab === "runtime" ? "ARCHIVE_LOG_a" : "ARCHIVE_BACKUP_a";
    check(element(`.archived-instance-workspace[data-archive-id="a"] [data-archive-workspace-tab="${screenshotTab}"]`).textContent?.includes(marker),
      "Visual capture displays retained data from the selected archive");
    contained(".archived-instance-workspace", ".server-detail-panel");
    check(document.documentElement.scrollWidth <= innerWidth, "Retained-data capture has no page-level horizontal overflow");
  }
  check(errors.length === 0, "No browser or React errors"); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  return { status: "passed", checks, stable_layout_phases: stableLayoutPhases, feedback_phases: feedbackPhases,
    concurrency_phases: concurrencyPhases, preview_phases: previewPhases, workspace_phases: workspacePhases, screenshot_tab: screenshotTab, browser_errors: errors };
}
void run().catch((error) => ({ status: "failed", error: String(error?.stack ?? error), checks, browser_errors: errors }))
  .then((result) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(result) }));
