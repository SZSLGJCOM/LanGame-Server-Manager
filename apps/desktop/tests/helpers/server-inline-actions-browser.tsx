import React, { act, StrictMode, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider } from "../../src/i18n";
import { bootstrapApp, readInstanceDetails } from "../../src/api";
import { invokeMock } from "../../src/api-mock";
import { createDefaultAiSettings } from "../../src/ai-settings";
import { ServersView } from "../../src/views/ServersView";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { InstanceBackupResult } from "../../src/types";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
let nativeDialogs = 0;
const rejectNativeDialog = () => { nativeDialogs++; throw new Error("Native browser dialog called"); };
window.confirm = rejectNativeDialog;
window.prompt = rejectNativeDialog;
window.alert = rejectNativeDialog;
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const deletedInstances: string[] = [];
const archivedInstances: string[] = [];
let finishArchive: (() => void) | null = null;
const deletedBackups: string[] = [];
const renames: string[] = [];
let checks = 0;
const noOperation = () => {};

function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
}
async function click(element: HTMLElement | null) {
  check(element, "Expected a mounted control");
  await act(async () => { element.click(); });
}
async function settleUntil(predicate: () => boolean, description: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    check(performance.now() < deadline, description);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function key(name: "Escape" | "Enter") {
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/${name}`, { method: "POST" });
    check(response.ok, `Native ${name} dispatch failed`);
  });
}
function contained(element: Element, container: Element, label: string) {
  const box = element.getBoundingClientRect();
  const outer = container.getBoundingClientRect();
  check(box.width > 0 && box.height > 0, `${label} is invisible`);
  check(box.left >= outer.left - 1 && box.right <= outer.right + 1 && box.top >= outer.top - 1 && box.bottom <= outer.bottom + 1,
    `${label} is clipped: ${JSON.stringify({ box: box.toJSON(), outer: outer.toJSON() })}`);
}
async function run() {
  const bootstrap = await bootstrapApp({ includeSystemSnapshot: false });
  const original = bootstrap.state.instances[0];
  check(original, "Browser mock must provide an instance");
  const instance = { ...original, name: "合作世界", status: "Stopped" as const, active_process_count: 0 };
  const details = { ...await readInstanceDetails(instance.id), summary: instance, active_run: null };
  Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: (command: string, args?: Record<string, unknown>) => {
    if (command === "inspect_instance_removal") return Promise.resolve({
      program_path: "fixture/library", data_path: "fixture/instance", remove_program: false,
      preserved_program_path: "fixture/library", owned_data_paths: ["fixture/instance/saves", "fixture/instance/backups"],
      preserved_external_saves_path: null
    });
    return invokeMock(command, args);
  } } });
  const backup: InstanceBackupResult = { backup_id: "inline-backup", instance_id: instance.id, backup_kind: "manual",
    display_name: "周末联机备份", created_at_unix_ms: 1_800_000_000_000, backup_path: "fixture/backups/inline-backup",
    saves_path: "fixture/saves", file_count: 5, total_bytes: 8192 };
  const props: ComponentProps<typeof ServersView> = {
    aiSettings: createDefaultAiSettings(), assistantCanRun: false, bindAddressCandidates: [], instances: [instance],
    moduleInstallations: { [instance.module_id]: { installState: "Installed", hasManagedInstallSource: true } },
    instanceLaunchPlans: {}, instanceLaunchFailures: {}, section: "overview", onWorkspaceSectionChange: noOperation,
    selectedInstanceId: instance.id, selectedDetails: details, selectedBackups: [backup], selectedModuleDetails: null,
    runtime: null, runtimeWindows: null, launchPlan: null, launchPlanError: null,
    refreshIssue: null, onActivity: noOperation, onResumeAutoRefresh: noOperation,
    onSelectInstance: noOperation, onStart: noOperation, onStop: noOperation, onInstallModule: async () => {},
    onOpenModuleLibrary: noOperation, onCreateInstance: noOperation, onPickDirectory: async () => null,
    onImportDontStarveWorldData: async () => { throw new Error("Unexpected world import"); },
    onOpenLocalPath: noOperation, onSendRuntimeCommand: async () => null, onSuppressRuntimeWindows: async () => {},
    onCreateBackup: noOperation, onDeleteInstance: async (id) => { deletedInstances.push(id); },
    onArchivesChanged: async () => {}, onArchiveInstance: async (id) => { archivedInstances.push(id); await new Promise<void>((resolve) => { finishArchive = resolve; }); },
    onRestoreBackup: noOperation, onRenameBackup: async (id, selected, draft) => {
      check(id === instance.id && selected.backup_id === backup.backup_id, "Rename targeted the wrong backup");
      renames.push(draft);
      return true;
    },
    onDeleteBackup: async (id, selected) => {
      check(id === instance.id, "Delete targeted the wrong instance");
      deletedBackups.push(selected.backup_id);
    },
    onSaveSettings: async () => undefined, onSaveAutostart: async () => {},
    onApplyPlayerAccessMutation: async () => { throw new Error("Unexpected player mutation"); }
  };
  await act(async () => { root.render(<StrictMode><I18nProvider><InstanceSettingsSaveProvider>
    <ServersView {...props} />
  </InstanceSettingsSaveProvider></I18nProvider></StrictMode>); });
  const readyDeadline = performance.now() + 5000;
  while (!fixture.querySelector(".server-list-card")) {
    check(performance.now() < readyDeadline, `Server list did not mount: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }

  const card = fixture.querySelector<HTMLElement>(".server-list-card")!;
  const closedHeight = card.getBoundingClientRect().height;
  await click(card.querySelector(".server-list-card-icon-action"));
  let review = card.querySelector<HTMLElement>(".inline-confirm-review")!;
  check(review && deletedInstances.length === 0, "Opening instance deletion must only request confirmation");
  check(review.textContent?.includes("永久删除此实例") && review.textContent?.includes("fixture/instance/backups")
    && review.textContent?.includes("关联库程序") && review.textContent?.includes("fixture/library")
    && review.textContent?.includes("基础库、此实例之外的个人数据及归档依赖的程序会保留"),
    "Deletion confirmation names removed instance data and retained installation");
  contained(review, card, "Instance confirmation");
  contained(review.querySelector(".inline-confirm-buttons")!, card, "Instance confirmation buttons");
  check(Math.abs(card.getBoundingClientRect().height - closedHeight) < 1, "Instance confirmation must stay within its existing card");
  check(getComputedStyle(review).pointerEvents === "auto", "Instance confirmation must receive pointer events");
  checks++;
  await key("Escape");
  check(!card.querySelector(".inline-confirm-review") && deletedInstances.length === 0, "Escape must cancel instance deletion");
  check(Math.abs(card.getBoundingClientRect().height - closedHeight) < 1, "Cancel must restore compact card layout");
  checks++;
  await click(card.querySelector(".server-list-card-icon-action"));
  await click(card.querySelector(".inline-confirm-submit"));
  check(deletedInstances.length === 1 && deletedInstances[0] === instance.id, "Instance confirmation must delete its own target once");
  checks++;

  const archiveButton = () => card.querySelector<HTMLButtonElement>(".server-list-card-archive > button");
  await settleUntil(() => !archiveButton()?.disabled && !card.querySelector(".inline-confirm-review"), "Deletion must finish its inventory refresh before another operation");
  await click(archiveButton());
  review = card.querySelector<HTMLElement>(".inline-confirm-review")!;
  check(review.textContent?.includes("归档此实例") && review.textContent?.includes("保留数据") && review.textContent?.includes("还原"), "Archive confirmation explains recovery independently of deletion");
  contained(review, card, "Archive confirmation");
  contained(review.querySelector(".inline-confirm-buttons")!, card, "Archive confirmation buttons");
  await key("Escape");
  check(archivedInstances.length === 0 && deletedInstances.length === 1, "Cancelling archive triggers neither operation");
  checks++;
  await click(archiveButton());
  await click(card.querySelector(".inline-confirm-submit"));
  check(archivedInstances.length === 1 && archivedInstances[0] === instance.id && deletedInstances.length === 1,
    "Archive submits exactly its own instance without invoking deletion");
  check(card.querySelector<HTMLButtonElement>(".server-list-card-delete > button")?.disabled, "Pending archive prevents conflicting deletion");
  await act(async () => { finishArchive?.(); });
  await settleUntil(() => !card.querySelector(".inline-confirm-review") && !archiveButton()?.disabled, "Archive must finish its inventory refresh");
  check(!card.querySelector(".inline-confirm-review") && !archiveButton()?.disabled, "Completed archive clears pending confirmation");
  checks++;

  sessionStorage.setItem("langameLanToken", "isolated-fixture-token");
  Object.assign(window, { isTauri: false });
  await act(async () => { root.render(<StrictMode><I18nProvider><InstanceSettingsSaveProvider>
    <ServersView {...props} />
  </InstanceSettingsSaveProvider></I18nProvider></StrictMode>); });
  check(archiveButton()?.disabled && archiveButton()?.title.includes("服务器主机"), "LAN archive entry explains that it is available only on the host");
  check(!card.querySelector<HTMLButtonElement>(".server-list-card-delete > button")?.disabled, "Existing LAN deletion retains its separate availability");
  sessionStorage.removeItem("langameLanToken");
  Object.assign(window, { isTauri: true });
  await act(async () => { root.render(<StrictMode><I18nProvider><InstanceSettingsSaveProvider>
    <ServersView {...props} />
  </InstanceSettingsSaveProvider></I18nProvider></StrictMode>); });
  checks++;

  await click(fixture.querySelector('[role="tab"][id$="-maintenance"]'));
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 100)); });
  const row = fixture.querySelector<HTMLElement>(".server-file-backup-row")!;
  check(row, "Maintenance must show backup rows");
  row.scrollIntoView({ block: "center" });
  await click(row.querySelector(".inline-confirm-action > button"));
  review = row.querySelector<HTMLElement>(".inline-confirm-review")!;
  check(review && deletedBackups.length === 0, "Opening backup deletion must only request confirmation");
  contained(review, row, "Backup confirmation");
  contained(review.querySelector(".inline-confirm-buttons")!, row, "Backup confirmation buttons");
  check(row.scrollWidth <= row.clientWidth + 1, "Backup confirmation must not overflow its row");
  checks++;
  await click(review.querySelector(".inline-confirm-buttons > button"));
  check(deletedBackups.length === 0 && !row.querySelector(".inline-confirm-review"), "Cancel must preserve the backup");
  await click(row.querySelector(".inline-confirm-action > button"));
  await click(row.querySelector(".inline-confirm-submit"));
  check(deletedBackups.length === 1 && deletedBackups[0] === backup.backup_id, "Backup confirmation must delete its own target once");
  checks++;

  const renameButton = () => [...row.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "重命名")!;
  await click(renameButton());
  const input = row.querySelector<HTMLInputElement>(".backup-rename-action input")!;
  check(document.activeElement === input && input.value === backup.display_name, "Rename must focus the existing name");
  const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
  await act(async () => { setValue.call(input, "新的联机备份"); input.dispatchEvent(new Event("input", { bubbles: true })); });
  await key("Enter");
  check(renames.length === 1 && renames[0] === "新的联机备份" && !row.querySelector("form"), "Enter must save the inline draft once");
  checks++;
  await click(renameButton());
  contained(row.querySelector(".backup-rename-action")!, row, "Backup rename editor");
  check(row.scrollWidth <= row.clientWidth + 1, "Backup rename must not overflow its row");
  await click(card.querySelector(document.body.dataset.confirmation === "archive"
    ? ".server-list-card-archive > button" : ".server-list-card-delete > button"));
  row.scrollIntoView({ block: "center" });
  contained(card.querySelector(".inline-confirm-review")!, card, "Final instance confirmation");
  check(card.querySelector(".inline-confirm-message")?.textContent?.includes(document.body.dataset.confirmation === "archive"
    ? "归档此实例" : "永久删除"), "Final confirmation keeps the requested action explicit");
  checks++;
  check(nativeDialogs === 0 && errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, native_dialogs: nativeDialogs, browser_errors: errors,
    instance_card_width: card.clientWidth, backup_row_width: row.clientWidth };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`Interaction stalled after ${checks} checks`)), 20_000);
})]).finally(() => clearTimeout(watchdog))
  .catch((error) => ({ status: "failed", checks, error: `After ${checks} checks: ${error instanceof Error ? error.stack : String(error)}`, browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
