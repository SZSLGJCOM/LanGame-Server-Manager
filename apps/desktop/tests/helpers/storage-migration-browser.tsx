import React, { act, StrictMode, useState } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider } from "../../src/i18n";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import { checkStoragePathLayout } from "./storage-path-layout";
import { useStorageMigration, type StorageMigrationModel } from "../../src/hooks/useStorageMigration";
import type { StorageMigrationApi, StorageMigrationPhase, StorageMigrationStatus } from "../../src/storage-migration-types";
import type { AppPathSettingsInput, AppSettings } from "../../src/types";
import { StoragePathsPanel } from "../../src/views/system/StoragePathsPanel";
import { StorageMigrationActivity } from "../../src/views/system/StorageMigrationReview";
import { ActivityProgressDetails } from "../../src/components/ActivityProgress";
import "../../src/app.css";
import "../../src/views/SystemView.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
document.documentElement.dataset.theme = "dark";
localStorage.setItem("langame.locale", "zh-CN");
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const errors: string[] = [];
addEventListener("error", event => errors.push(event.message));
addEventListener("unhandledrejection", event => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const initial: AppSettings = { games_root: "E:\\LanGame\\games", servers_root: "E:\\LanGame\\instances",
  archives_root: "E:\\LanGame\\instances\\.trash", steamcmd_root: "E:\\LanGame\\steamcmd", modules_root: "E:\\LanGame\\modules" };
let saved = { ...initial };
let backend: StorageMigrationStatus | null = null;
let selected: string | null = null;
let nextConflicts = 0;
let sequence = 0;
let previewCalls = 0;
let executeCalls = 0;
let cancelCalls = 0;
let refreshCalls = 0;
let failRefresh = false;
let failExecuteReceipt = false;
let failPreviewReceipt = false;
let failDismissalWrite = false;
const dismissalStorage = {
  getItem: (key: string) => localStorage.getItem(key),
  setItem: (key: string, value: string) => {
    if (failDismissalWrite) throw new Error("PREFERENCE_STORAGE_UNAVAILABLE");
    localStorage.setItem(key, value);
  }
};
let previewWait: Promise<void> | null = null;
let model: StorageMigrationModel;
let configurePaths: (paths: Partial<AppPathSettingsInput>) => void;
let checks = 0;
function check(value: unknown, message: string): asserts value { if (!value) throw new Error(message); checks++; }
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const value = fixture.querySelector<T>(selector); if (!value) throw new Error(`Missing ${selector}`); return value;
}
async function settle(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 8000;
  while (!predicate()) { if (performance.now() > deadline) throw new Error(message); await act(async () => { await new Promise(resolve => setTimeout(resolve, 10)); }); }
}
async function click(selector: string) {
  const target = element<HTMLButtonElement>(selector);
  check(!target.disabled, `${selector} unexpectedly disabled`);
  target.scrollIntoView({ block: "center" });
  const bounds = target.getBoundingClientRect();
  const x = bounds.left + bounds.width / 2, y = bounds.top + bounds.height / 2;
  check(target.contains(document.elementFromPoint(x, y)), `${selector} is covered by another control`);
  await act(async () => {
    const response = await fetch(`/__reliability_pointer/${nonce}`, { method: "POST",
      body: JSON.stringify({ type: "click", x, y }) });
    check(response.ok, "The real pointer event failed");
  });
}
const chooser = (key: keyof AppPathSettingsInput) => `.system-path-row[data-path-key="${key}"] .system-path-picker-button`;
const shown = (key: keyof AppPathSettingsInput) => element(`.system-path-row[data-path-key="${key}"] .system-runtime-path`).textContent;
function phase(value: StorageMigrationPhase, extra: Partial<StorageMigrationStatus> = {}) {
  check(backend, "A native operation must exist before reporting progress");
  backend = { ...backend, phase: value, can_cancel: !["applying", "cleaning", "completed", "cancelled", "failed"].includes(value), ...extra };
}
async function readPhase(value: StorageMigrationPhase, extra: Partial<StorageMigrationStatus> = {}) {
  phase(value, extra);
  await act(async () => { model.recheck(); });
  await settle(() => model.status?.phase === value, `Did not observe ${value}`);
}
async function readyToMove() {
  phase("ready");
  await act(async () => { model.recheck(); });
  await settle(() => model.status?.phase === "copying", "A checked selection did not automatically start migration");
}
function api(available: boolean): StorageMigrationApi {
  return { available: () => available, read: async () => structuredClone(backend),
    preview: async input => {
      previewCalls++;
      if (failPreviewReceipt) throw new Error("PREVIEW_REQUEST_REJECTED");
      if (previewWait) await previewWait;
      backend = { operation_id: `migration-${++sequence}`, phase: "checking", input: { ...input },
        roots: (Object.keys(input) as Array<keyof AppPathSettingsInput>).filter(key => saved[key] !== input[key])
          .map(key => ({ source: saved[key], target: input[key] })),
        total_files: 12, total_bytes: 4194304, completed_files: 0, completed_bytes: 0, reused_files: 0,
        conflict_count: nextConflicts, conflicts: Array.from({ length: nextConflicts }, (_, i) => ({
          source: `E:\\LanGame\\file-${i}.dat`, target: `D:\\LanGame\\file-${i}.dat`, message: "目标目录包含不安全的重解析点" })),
        can_cancel: true, recovery_required: false, error: null, cleanup_warnings: [], preserved_destinations: [] };
      return structuredClone(backend);
    },
    execute: async id => {
      check(backend?.operation_id === id && backend.phase === "ready" && backend.conflict_count === 0, "Execution bypassed the reviewed plan");
      executeCalls++; phase("copying");
      if (failExecuteReceipt) throw new Error("Runtime response timed out; the operation may still be running");
      return structuredClone(backend!);
    },
    cancel: async id => { check(backend?.operation_id === id, "Cancellation used another operation identity"); cancelCalls++; return structuredClone(backend!); }
  };
}
function Fixture({ available }: { available: boolean }) {
  const [settings, setSettings] = useState(saved);
  configurePaths = paths => { saved = { ...saved, ...paths }; setSettings(saved); };
  const [page, setPage] = useState("paths");
  const [transport] = useState(() => api(available));
  model = useStorageMigration(async status => {
    refreshCalls++;
    saved = { ...saved, ...status.input };
    if (failRefresh) throw new Error("BOOTSTRAP_REFRESH_FAILED");
    setSettings(saved);
  }, settings, transport, dismissalStorage);
  return <main className="system-dashboard-page" style={{ display: "block", height: "auto", overflow: "visible", padding: 20, minHeight: "100vh" }}>
    <nav style={{ display: "flex", gap: 12, marginBottom: 16 }}>
      <button className="secondary-button fixture-path-page" onClick={() => setPage("paths")}>路径设置</button>
      <button className="secondary-button fixture-other-page" onClick={() => setPage("other")}>其他页面</button>
    </nav>
    {page === "paths" ? <StoragePathsPanel appSettings={settings} steamCmdStatus={null} steamCmdBusy={false}
      steamCmdProgress={null} steamCmdMessage="" storageMigration={model}
      onPickDirectory={async () => selected} onEnsureSteamCmd={() => {}} onUninstallSteamCmd={() => {}} />
      : <div className="fixture-other-workspace">其他工作区</div>}
    <div className="fixture-background-progress"><ActivityProgressDetails.Provider value={true}>
      <StorageMigrationActivity migration={model} />
    </ActivityProgressDetails.Provider></div>
  </main>;
}
async function mount(available = true) {
  await act(async () => { root.render(<StrictMode><I18nProvider><Fixture key={String(available)} available={available} /></I18nProvider></StrictMode>); });
  await settle(() => Boolean(model && model.available === available && !model.recovering), "Migration recovery did not settle");
}
async function choose(key: Exclude<keyof AppPathSettingsInput, "archives_root">, value: string | null) { selected = value; await click(chooser(key)); }
async function run() {
  await prepareBrowserLocaleCatalogs();
  await mount(false);
  checkStoragePathLayout(fixture, check);
  check(element<HTMLButtonElement>(chooser("servers_root")).title.includes("浏览器预览不读取或移动本机文件"),
    "Browser preview must explain its local-file limitation");
  check([...fixture.querySelectorAll<HTMLButtonElement>(".system-path-picker-button")].every(button => button.disabled),
    "An unavailable host offers file mutation controls");
  check(previewCalls === 0 && executeCalls === 0, "The unavailable preview sent a native command");
  backend = { operation_id: "failed-overlapping-workspace", phase: "failed",
    input: { ...initial, servers_root: initial.games_root, archives_root: `${initial.games_root}\\.trash` },
    roots: [], total_files: 0, total_bytes: 0, completed_files: 0, completed_bytes: 0, reused_files: 0,
    conflicts: [], conflict_count: 0, can_cancel: false, recovery_required: false, cleanup_warnings: [], preserved_destinations: [],
    error: "目标目录与仍在使用的存储位置重叠" };
  await mount();
  check(shown("servers_root") === initial.servers_root && shown("archives_root") === initial.archives_root
    && !fixture.querySelector(".system-path-unapplied"), "A failed workspace draft must not replace the configured paths");
  await choose("games_root", initial.games_root);
  check(previewCalls === 0 && executeCalls === 0, "Selecting the existing games folder replayed an unrelated failed workspace draft");
  await choose("games_root", "D:\\Only programs");
  check(!fixture.querySelector(".shell-activity-notice.is-info"), "An explicit folder must not show the volume-root hint");
  check(model.status?.input.games_root === "D:\\Only programs" && model.status.input.servers_root === initial.servers_root
    && model.status.input.archives_root === initial.archives_root && model.status.input.steamcmd_root === initial.steamcmd_root,
    "Selecting games carried unrelated failed path selections into the next request");
  await readyToMove(); await readPhase("failed", { error: "TARGET_WRITE_FAILED" });
  check(shown("games_root") === initial.games_root, "A failed games migration replaced its configured path");
  await choose("steamcmd_root", "D:\\Only SteamCMD");
  check(model.status?.input.games_root === initial.games_root && model.status.input.servers_root === initial.servers_root
    && model.status.input.archives_root === initial.archives_root && model.status.input.steamcmd_root === "D:\\Only SteamCMD",
    "Selecting SteamCMD carried an unrelated failed migration into the next request");
  await readPhase("failed", { error: "TARGET_WRITE_FAILED" });
  await act(async () => { root.render(<div />); });
  check(backend, "The previous failure must be available for the older-preview fixture");
  const priorFailure = backend;
  for (const previousPhase of ["failed", "ready"] as const) {
    saved = { ...initial, archives_root: "E:\\Separate archives" };
    backend = { ...priorFailure, operation_id: `older-workspace-${previousPhase}`, phase: previousPhase,
      input: { ...saved, servers_root: "D:\\Retried instances" }, can_cancel: false,
      error: previousPhase === "failed" ? "ARCHIVE_VOLUME_MISMATCH" : null };
    const executionsBeforeRetry = executeCalls;
    await mount();
    check(executeCalls === executionsBeforeRetry, "Recovering an older preview must not execute it automatically");
    await click(".system-migration-retry");
    check(model.status?.input.servers_root === "D:\\Retried instances"
      && model.status.input.archives_root === "D:\\Retried instances\\.trash"
      && shown("archives_root") === "E:\\Separate archives",
      "Retry must prepare a new workspace/archive pair before changing the applied paths");
    await readyToMove(); await readPhase("cancelled");
    await act(async () => { root.render(<div />); });
  }
  const volumeTargets = [
    ["games_root", "D:\\LanGame\\server-files"],
    ["servers_root", "D:\\LanGame\\instances"],
    ["steamcmd_root", "D:\\LanGame\\cmd\\steamcmd"]
  ] as const;
  for (const [key, target] of volumeTargets) {
    saved = { ...initial, archives_root: "E:\\External archives" };
    backend = null;
    await mount();
    const previewsBeforeSelection = previewCalls;
    const executionsBeforeSelection = executeCalls;
    await choose(key, "D:\\");
    check(element(".shell-activity-notice.is-info").textContent?.includes(`已自动选择 LanGame 子目录: ${target}`),
      "A drive selection must explain its actual LanGame subfolder in a dismissible notice");
    await click(".shell-activity-notice.is-info .shell-activity-notice-close");
    check(!fixture.querySelector(".shell-activity-notice.is-info"), "The automatic subfolder notice could not be dismissed");
    check(model.status?.phase === "checking" && model.status.input[key] === target
      && previewCalls === previewsBeforeSelection + 1 && executeCalls === executionsBeforeSelection,
      "A drive selection must preview its managed subfolder before executing");
    check(shown(key) === initial[key]
      && model.status.input.archives_root === (key === "servers_root" ? `${target}\\.trash` : "E:\\External archives"),
      "A drive selection changed effective paths or failed to bind its workspace archives");
    for (const other of ["games_root", "servers_root", "steamcmd_root"] as const) {
      if (other !== key) check(model.status.input[other] === initial[other], "A drive selection moved an unselected role");
    }
    await readyToMove(); await readPhase("copying");
    check(executeCalls === executionsBeforeSelection + 1, "The resolved drive selection executed more than once");
    await readPhase("completed");
    await settle(() => !model.refreshing && shown(key) === target, "The resolved path was not published after completion");
    await choose(key, "d:/");
    check(previewCalls === previewsBeforeSelection + 1 && executeCalls === executionsBeforeSelection + 1,
      "Selecting the current role's drive caused a second migration");
    await act(async () => { root.render(<div />); });

    for (const previousPhase of ["failed", "ready"] as const) {
      saved = { ...initial, archives_root: "E:\\External archives" };
      backend = { ...priorFailure, operation_id: `volume-root-${key}-${previousPhase}`, phase: previousPhase,
        input: { ...saved, [key]: "D:\\" }, can_cancel: false,
        error: previousPhase === "failed" ? "storage migration cannot continue at D:\\: A migration target must be an absolute non-volume root." : null };
      const previewsBeforeRetry = previewCalls;
      const executionsBeforeRetry = executeCalls;
      await mount();
      check(executeCalls === executionsBeforeRetry, "Loading an old volume-root preview executed without a new choice");
      await click(".system-migration-retry");
      check(previewCalls === previewsBeforeRetry + 1 && model.status?.input[key] === target
        && model.status.input.archives_root === (key === "servers_root" ? `${target}\\.trash` : saved.archives_root),
        "Retry did not replace the old volume-root target with a complete managed destination");
      await readyToMove(); await readPhase("copying");
      check(executeCalls === executionsBeforeRetry + 1, "Retry repeated execution after the resolved target was checked");
      await readPhase("cancelled");
      await act(async () => { root.render(<div />); });
    }
  }
  saved = { ...initial };
  backend = null; previewCalls = 0; executeCalls = 0;
  await mount();
  check(fixture.querySelectorAll(".system-path-row").length === 4
    && !fixture.querySelector(".system-path-editor-footer,.system-path-save-button,.system-path-discard-button,.system-path-migration-review"),
    "Idle storage settings must contain only the path rows, without a permanent migration form");
  check(!fixture.querySelector(chooser("archives_root")) && fixture.querySelectorAll(".system-path-row .system-path-picker-button").length === 3,
    "Archives must not offer a separate chooser while the other three storage roles remain selectable");
  check(fixture.querySelectorAll('.system-path-row[data-path-key="archives_root"] button').length === 2
    && element('.system-path-row[data-path-key="archives_root"] .system-runtime-label').textContent === "实例归档区",
    "Archives must retain copy/open controls without the volume explanation");
  await choose("servers_root", " D:\\LanGame\\instances ");
  check(model.draft?.archives_root === "D:\\LanGame\\instances\\.trash" && previewCalls === 1,
    "Choosing a workspace must automatically check its linked default archives");
  check(shown("servers_root") === initial.servers_root && shown("archives_root") === initial.archives_root
    && executeCalls === 0 && !fixture.querySelector(".system-path-unapplied"), "Preflight must retain the configured paths without pending-path badges");
  check(!fixture.querySelector("dialog,.system-migration-apply,.system-path-migration-review"), "Selecting a directory opens a redundant confirmation");
  check(element<HTMLButtonElement>(chooser("servers_root")).disabled, "A scan permits concurrent target editing");
  check(element<HTMLButtonElement>('.system-path-row[data-path-key="servers_root"] .system-runtime-path-actions button:first-child').disabled,
    "An unapplied target can be copied as if it were configured");
  await readyToMove();
  check(model.status?.phase === "copying" && executeCalls === 1, "Execution did not use the checked operation");
  check([...fixture.querySelectorAll<HTMLButtonElement>(".system-path-row button")].every(button => button.disabled),
    "Running migration permits target edits");
  await click(".fixture-other-page");
  check(element(".fixture-background-progress").textContent?.includes("正在迁移文件"), "Leaving paths lost the active operation");
  await readPhase("verifying", { completed_bytes: 2097152, completed_files: 6 });
  check(element('[role="progressbar"]').getAttribute("aria-valuenow") === "50", "Verification progress did not use native bytes");
  await click(".fixture-path-page");
  check(shown("servers_root") === initial.servers_root && model.draft?.servers_root === "D:\\LanGame\\instances",
    "Returning to paths must retain configured paths and the separate submitted target");
  await click(".system-migration-activity .shell-task-stop");
  check(cancelCalls === 1 && model.cancelRequested && model.status?.phase === "verifying", "Cancel acknowledgement was reported as terminal cancellation");
  await readPhase("cancelled");
  check(shown("servers_root") === initial.servers_root && model.draft?.servers_root === "D:\\LanGame\\instances",
    "Cancellation lost the draft or displayed the target as configured");
  await click(".system-migration-retry");
  failExecuteReceipt = true; await readyToMove();
  await settle(() => model.status?.phase === "copying", "Lost receipt did not recover the existing operation");
  check(executeCalls === 2, "Recovery repeated execution");
  await readPhase("failed", { error: "TARGET_WRITE_FAILED" });
  check(shown("servers_root") === initial.servers_root && model.draft?.servers_root === "D:\\LanGame\\instances"
    && element(".fixture-background-progress").textContent?.includes("TARGET_WRITE_FAILED"), "Native failure lost the draft or diagnostic");
  check(!element(".system-runtime-panel").textContent?.includes("TARGET_WRITE_FAILED"), "The main panel repeats the activity error");
  failExecuteReceipt = false; await click(".system-migration-retry"); await readyToMove();
  await readPhase("applying");
  check(!fixture.querySelector(".system-migration-activity .shell-task-stop"), "Commit offers cancellation when the backend disallows it");
  failRefresh = true;
  await readPhase("completed", { completed_files: 12, completed_bytes: 4194304, cleanup_warnings: ["E:\\LanGame\\locked.dat 仍保留"] });
  await settle(() => Boolean(model.refreshError), "Refresh failure was not shown");
  check(model.status?.phase === "completed" && saved.servers_root.startsWith("D:") && shown("servers_root") === saved.servers_root,
    "Refresh failure hid committed paths or changed the migration outcome");
  check(fixture.textContent?.includes("无需再次迁移") && fixture.textContent.includes("locked.dat"), "Retained source cleanup warning is missing");
  check(!fixture.querySelector(".system-migration-apply"), "A completed migration offers execution again");
  await click(".system-migration-activity .shell-activity-notice-close");
  check(!fixture.querySelector(".system-migration-activity") && model.refreshError === "BOOTSTRAP_REFRESH_FAILED"
    && shown("servers_root") === saved.servers_root, "Closing refresh feedback lost committed path evidence");
  await act(async () => { await model.refresh(); });
  check(Boolean(fixture.querySelector(".system-migration-refresh")), "Explicit refresh did not show its failure again");
  const writesBeforeRefresh = executeCalls;
  const refreshCallsBeforeRetry = refreshCalls;
  failRefresh = false; await click(".system-migration-refresh");
  check(!model.refreshError && executeCalls === writesBeforeRefresh && refreshCalls === refreshCallsBeforeRetry + 1, "Status refresh repeated a migration");
  check(!fixture.querySelector(".system-path-unapplied,.system-path-migration-review"), "Completed paths retain a pending marker or permanent summary");
  await act(async () => { configurePaths({ archives_root: "D:\\Custom archives" }); });
  let finishPreview = () => {};
  previewWait = new Promise<void>(resolve => { finishPreview = resolve; });
  await choose("servers_root", "F:\\LanGame\\instances");
  check(shown("servers_root") === saved.servers_root && shown("archives_root") === "D:\\Custom archives"
    && model.draft?.servers_root === "F:\\LanGame\\instances" && model.draft.archives_root === "F:\\LanGame\\instances\\.trash"
    && model.pending === "preview", "The external archive must follow the pending workspace without changing the displayed effective paths");
  await act(async () => { finishPreview(); }); previewWait = null;
  await readyToMove();
  check(model.status?.input.archives_root === "F:\\LanGame\\instances\\.trash"
    && model.status.roots.some(root => root.source === "D:\\Custom archives" && root.target === "F:\\LanGame\\instances\\.trash"),
    "The same migration must include the external archive source and the new workspace trash destination");
  check(!fixture.querySelector(chooser("archives_root")) && !fixture.textContent?.includes("同盘"),
    "Automatic archive migration must not ask for another archive selection or a same-volume workspace");
  await readPhase("completed");
  check(saved.servers_root === "F:\\LanGame\\instances" && saved.archives_root === "F:\\LanGame\\instances\\.trash",
    "External archives were not published with the new workspace");
  await click(".fixture-other-page"); await click(".fixture-path-page");
  const previewsBeforeCancel = previewCalls;
  await choose("servers_root", null);
  check(shown("servers_root") === saved.servers_root && shown("archives_root") === saved.archives_root
    && model.draft === null && previewCalls === previewsBeforeCancel, "Cancelling a chooser changed the committed workspace or archives");
  await choose("servers_root", "D:\\Adjusted instances"); await readyToMove();
  check(model.status?.input.servers_root === "D:\\Adjusted instances" && model.status.input.archives_root === "D:\\Adjusted instances\\.trash",
    "Every later workspace selection must derive its own archive destination from the current configuration");
  await readPhase("completed");
  nextConflicts = 13; const beforeConflict = executeCalls;
  await choose("games_root", "G:\\LanGame\\games"); await readPhase("ready");
  check(executeCalls === beforeConflict && !fixture.querySelector(".system-migration-apply"), "Conflicts did not prevent automatic execution");
  check(fixture.querySelectorAll(".system-migration-conflicts li").length === 10
    && fixture.textContent?.includes("共 13 个冲突"), "Conflict details must be bounded while preserving the total count");
  await readPhase("ready", { conflict_count: 0, conflicts: [] });
  check(executeCalls === beforeConflict, "A later poll silently executed a previously conflicted selection");
  nextConflicts = 0; await click(".system-migration-retry"); await readyToMove(); await readPhase("completed");
  await choose("steamcmd_root", "G:\\LanGame\\steamcmd");
  const cancelBeforeScan = cancelCalls;
  const beforeCancelledScan = executeCalls;
  await click(".system-migration-activity .shell-task-stop");
  await settle(() => cancelCalls === cancelBeforeScan + 1, "Cancelling during the scan did not request cancellation");
  await readPhase("ready");
  check(executeCalls === beforeCancelledScan && model.cancelRequested, "Late ready after cancellation executed the migration");
  await readPhase("cancelled");
  await click(".system-migration-discard");
  check(shown("steamcmd_root") === saved.steamcmd_root && !fixture.querySelector(".system-path-unapplied,.system-migration-activity"), "Discard retained an unapplied target or its finished activity");
  await choose("steamcmd_root", "G:\\LanGame\\steamcmd"); await readyToMove(); await readPhase("completed");
  failPreviewReceipt = true;
  const beforeRejectedPreview = executeCalls;
  await choose("games_root", "H:\\LanGame\\games");
  await settle(() => !model.unconfirmed && !model.pending, "Rejected preview did not resolve the existing operation");
  check(model.status === null && shown("games_root") === saved.games_root && model.draft?.games_root === "H:\\LanGame\\games"
    && element(".fixture-background-progress").textContent?.includes("PREVIEW_REQUEST_REJECTED")
    && Boolean(fixture.querySelector(".system-migration-retry")), "An old completion hides the new preview failure or its recovery action");
  check(executeCalls === beforeRejectedPreview, "A rejected preview repeated the previous migration");
  failPreviewReceipt = false;
  await click(".system-migration-retry"); await readyToMove(); await readPhase("completed");
  await act(async () => { configurePaths({ archives_root: `${saved.servers_root}\\.trash` }); });
  await choose("servers_root", "D:\\Relocated instances");
  check(model.draft?.archives_root === "D:\\Relocated instances\\.trash" && shown("archives_root") === saved.archives_root,
    "Moving a workspace must carry its default archives without a separate archive chooser");
  await readyToMove();
  const backup = { original_path: "D:\\Relocated instances", backup_path: "D:\\Relocated instances.previous-contents" };
  await readPhase("completed", { preserved_destinations: [backup] });
  check(element(".system-migration-activity .shell-task-summary").textContent === "目录迁移已完成"
    && !fixture.querySelector(".system-migration-warnings,.system-migration-conflicts"), "Preserving a target folder is normal completion, not a warning or conflict");
  check(element(".system-migration-preserved code").textContent === backup.backup_path
    && !element(".system-runtime-panel").textContent?.includes(backup.backup_path), "Backup paths belong only in activity details");
  const openedBackups: string[] = [];
  let failBackupOpen = true;
  Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args: { path?: string } = {}) => {
    check(command === "open_local_path" && args.path === backup.backup_path, "The backup action must open the preserved folder, not the active destination");
    if (failBackupOpen) throw new Error("BACKUP_OPEN_FAILED");
    openedBackups.push(args.path);
  } } });
  const executionsBeforeOpen = executeCalls;
  await click(".system-migration-backup-open");
  check(element(".system-migration-backup-error").textContent?.includes("BACKUP_OPEN_FAILED")
    && model.status?.phase === "completed", "Opening a backup failure must not change the migration outcome");
  failBackupOpen = false; await click(".system-migration-backup-open");
  check(openedBackups[0] === backup.backup_path && !fixture.querySelector(".system-migration-backup-error")
    && executeCalls === executionsBeforeOpen, "Opening the retained target must not repeat migration or retain a resolved error");
  const writesBeforeDismiss = previewCalls + executeCalls + cancelCalls;
  failDismissalWrite = true;
  await click(".system-migration-activity .shell-activity-notice-close");
  check(model.hasActivity && fixture.textContent?.includes("无法更新消息关闭记录，请重试关闭")
    && fixture.textContent.includes("PREFERENCE_STORAGE_UNAVAILABLE"), "Failed acknowledgement was silently reported as dismissed");
  failDismissalWrite = false;
  await click(".system-migration-activity .shell-activity-notice-close");
  check(!model.hasActivity && !fixture.querySelector(".system-migration-activity") && model.status?.phase === "completed"
    && model.draft === null, "Closing a completion did not remove just its notification and draft");
  await act(async () => { model.recheck(); });
  check(!fixture.querySelector(".system-migration-activity"), "Polling restored a dismissed completion");
  await mount(false); await mount();
  check(!model.hasActivity && model.draft === null && !fixture.querySelector(".system-migration-activity"),
    "Reloading restored a dismissed operation or its draft");
  check(previewCalls + executeCalls + cancelCalls === writesBeforeDismiss, "Closing a notification sent a native mutation");
  for (const result of ["failed", "cancelled", "ready"] as const) {
    nextConflicts = result === "ready" ? 1 : 0;
    await choose("games_root", `J:\\${result}-programs`);
    check(model.hasActivity && !fixture.querySelector(".system-migration-activity .shell-activity-notice-close"),
      "A new running migration was hidden or offered dismissal");
    await readPhase(result, { error: result === "failed" ? "TARGET_WRITE_FAILED" : null });
    const writes = previewCalls + executeCalls + cancelCalls;
    await click(".system-migration-activity .shell-activity-notice-close");
    check(!model.hasActivity && model.draft === null && !fixture.querySelector(".system-migration-activity")
      && previewCalls + executeCalls + cancelCalls === writes, `Closing ${result} changed native work or left feedback`);
    await mount(false); await mount();
    check(!model.hasActivity && model.draft === null, `Reloading revived the dismissed ${result}`);
  }
  nextConflicts = 0;
  await choose("games_root", "K:\\completed-programs"); await readyToMove();
  await readPhase("completed", { error: null, cleanup_warnings: ["K:\\retained.dat 仍保留"] });
  await click(".system-migration-activity .shell-activity-notice-close");
  check(!model.hasActivity && model.status?.cleanup_warnings.length === 1, "Closing a cleanup warning lost the native result");
  failPreviewReceipt = true;
  await choose("games_root", "L:\\rejected-programs");
  await settle(() => !model.unconfirmed && !model.pending, "Rejected preview did not settle for dismissal");
  check(model.status === null && model.error === "PREVIEW_REQUEST_REJECTED", "Expected error-only preview feedback");
  await click(".system-migration-activity .shell-activity-notice-close");
  check(!model.hasActivity && model.error === null && model.draft === null, "Error-only preview feedback could not be closed");
  failPreviewReceipt = false;
  phase("failed", { operation_id: "restart-recovery", recovery_required: true, error: "请重启应用完成恢复" });
  await mount(false); await mount();
  check(model.hasActivity && !model.canDismiss && !fixture.querySelector(".system-migration-activity .shell-activity-notice-close")
    && !fixture.querySelector(".system-migration-retry,.system-migration-discard,[role=progressbar]")
    && fixture.textContent?.includes("迁移需要重启恢复"), "A required recovery was hidden or shown as dismissible progress");
  await readPhase("failed", { recovery_required: false });
  check(Boolean(fixture.querySelector(".system-migration-activity .shell-activity-notice-close")),
    "A resolved recovery barrier still blocks closing its finished feedback");
  await act(async () => { await new Promise<void>(resolve => requestAnimationFrame(() => resolve())); });
  check(!fixture.querySelector("dialog,.system-migration-apply,.system-path-editor-footer"), "Automatic migration retained a confirmation or permanent footer");
  check(document.documentElement.scrollWidth <= innerWidth + 1, "Migration review overflows horizontally");
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, browser_errors: errors };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error(`Migration checks stalled after ${checks}`)), 30000); })])
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .catch(error => ({ status: "failed", checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then(report => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
