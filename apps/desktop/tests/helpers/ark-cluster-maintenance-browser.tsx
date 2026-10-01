import React, { act, StrictMode, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import { bootstrapApp, readInstanceDetails, readInstanceIsolation } from "../../src/api";
import { invokeMock } from "../../src/api-mock";
import { createDefaultAiSettings } from "../../src/ai-settings";
import { I18nProvider } from "../../src/i18n";
import { ServersView } from "../../src/views/ServersView";
import { ArkClusterMaintenance } from "../../src/views/servers/ArkClusterMaintenance";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { ArkClusterReport } from "../../src/ark-clusters";
import type { ArkClusterBackupSummary, ArkClusterRecoveryResult, ArkClusterRestoreResult, PendingArkClusterRestore } from "../../src/ark-cluster-backups";
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
let root = createRoot(fixture);
let checks = 0;
const writes: Array<{ command: string; input: Record<string, unknown> }> = [];
const reportFailures = new Set<string>();
const pending = new Map<string, PendingArkClusterRestore>();
let recoverFailure = false;
let restoreFlight: ReturnType<typeof deferred<ArkClusterRestoreResult>> | null = null;
let recoverFlight: ReturnType<typeof deferred<ArkClusterRecoveryResult>> | null = null;
let inspectFlight: { id: string; result: ReturnType<typeof deferred<PendingArkClusterRestore | null>> } | null = null;
const noOperation = () => {};
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (error: Error) => void;
  const promise = new Promise<T>((done, failed) => { resolve = done; reject = failed; }); return { promise, resolve, reject }; }
function check(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
function select<T extends Element>(selector: string): T { const value = fixture.querySelector<T>(selector); check(value, `Missing ${selector}`); return value; }
function button(text: string, scope: ParentNode = fixture): HTMLButtonElement {
  const value = [...scope.querySelectorAll<HTMLButtonElement>("button")].find((item) => item.textContent?.trim() === text);
  check(value, `Missing button: ${text}`); return value;
}
async function click(value: HTMLElement) {
  value.scrollIntoView({ block: "nearest", inline: "nearest" });
  const bounds = value.getBoundingClientRect();
  check(bounds.width > 0 && bounds.height > 0 && !value.closest("[hidden]"), `Cannot click a hidden control: ${value.textContent}`);
  await act(async () => value.click());
}
async function settleUntil(predicate: () => boolean, label: string) {
  const deadline = performance.now() + 6000;
  while (!predicate()) { check(performance.now() < deadline, `Timed out: ${label}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); }); }
}
const panelText = () => select(".ark-cluster-panel").textContent ?? "";
async function run() {
  await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]);
  const bootstrap = await bootstrapApp({ includeSystemSnapshot: false });
  const original = bootstrap.state.instances[0];
  check(original, "Mock instance is required");
  const stored = await readInstanceDetails(original.id);
  const isolation = await readInstanceIsolation(original.id);
  const summary = { ...original, name: "孤岛世界", module_id: "arksurvivalevolved", status: "Stopped" as const, active_process_count: 0, autostart: false };
  function report(id: string): ArkClusterReport {
    const identity = { module_id: "arksurvivalevolved", cluster_id: `friends-${id}`, directory_key: `d:/fixture/transfer/${id}`, member_ids: [id, `${id}-center`] };
    return { instance_id: id, identity, cluster_directory: identity.directory_key, related_instances: [], issues: [], start_blocked: false,
      members: ["TheIsland", "TheCenter"].map((map, index) => ({ summary: { ...summary, id: identity.member_ids[index], name: index ? "中心岛世界" : "孤岛世界" },
        map_name: map, cluster_id: identity.cluster_id, cluster_directory: identity.directory_key, explicit_shared_directory: true,
        config_file_path: `fixture/${index}/GameUserSettings.ini`, saves_path: `fixture/${index}/Saved`,
        ports: [{ name: "game", port: 7777 + index * 10, protocol: "udp" }] })) };
  }
  function backup(id: string): ArkClusterBackupSummary {
    const source = report(id);
    return { backup_id: `snapshot-${id}`, backup_kind: "manual", created_at_unix_ms: 1800000000000,
      identity: source.identity!, backup_path: `fixture/backups/${id}`, file_count: 7, total_bytes: 1024 * 1024,
      members: source.members.map((member) => ({ instance_id: member.summary.id, instance_name: member.summary.name, map_name: member.map_name })) };
  }
  function pendingFor(id: string): PendingArkClusterRestore {
    return { identity: report(id).identity!, backup_id: backup(id).backup_id,
      safeguard_backup: { ...backup(id), backup_kind: "pre_restore", backup_id: `safeguard-${id}` } };
  }
  let props: ComponentProps<typeof ServersView> = {
    // Sidebar artwork is outside this native IPC fixture. The selected instance
    // retains its actual ARK module, while a synthetic list cover avoids CDN I/O.
    aiSettings: createDefaultAiSettings(), assistantCanRun: false, bindAddressCandidates: [], instances: [{ ...summary, module_id: "fixture-art" }],
    moduleInstallations: { [summary.module_id]: { installState: "Installed", hasManagedInstallSource: true } },
    instanceLaunchPlans: {}, instanceLaunchFailures: {}, section: "overview", onWorkspaceSectionChange: noOperation,
    selectedInstanceId: summary.id, selectedDetails: { ...stored, summary, active_run: null, settings_json: "{}" },
    selectedBackups: [], selectedModuleDetails: null, runtime: null, runtimeWindows: null, launchPlan: null, launchPlanError: null,
    refreshIssue: null, onActivity: noOperation, onResumeAutoRefresh: noOperation,
    onSelectInstance: noOperation, onStart: noOperation, onStop: noOperation, onInstallModule: async () => {},
    onOpenModuleLibrary: noOperation, onCreateInstance: noOperation, onPickDirectory: async () => null,
    onImportDontStarveWorldData: async () => { throw new Error("Unexpected import"); }, onOpenLocalPath: noOperation,
    onSendRuntimeCommand: async () => null, onSuppressRuntimeWindows: async () => {}, onCreateBackup: noOperation,
    onArchivesChanged: async () => {}, onArchiveInstance: async () => {}, onDeleteInstance: async () => {}, onRestoreBackup: noOperation, onRenameBackup: async () => true,
    onDeleteBackup: async () => {}, onSaveSettings: async () => undefined, onSaveAutostart: async () => {},
    onApplyPlayerAccessMutation: async () => { throw new Error("Unexpected player mutation"); }
  };
  Object.assign(globalThis, { isTauri: true });
  mockIPC(async (command, args) => {
    const input = (args as { input?: Record<string, unknown> }).input ?? {};
    const id = String(input.instance_id ?? "");
    if (command === "read_ark_cluster") { if (reportFailures.has(id)) throw new Error("fixture member paths need recovery"); return report(id); }
    if (command === "read_pending_ark_cluster_restore") {
      if (inspectFlight?.id === id) return inspectFlight.result.promise;
      return pending.get(id) ?? null;
    }
    if (command === "list_ark_cluster_backups") return [backup(id)];
    if (["create_ark_cluster_backup", "restore_ark_cluster_backup", "recover_ark_cluster_restore", "operate_ark_cluster"].includes(command)) {
      writes.push({ command, input });
      if (command === "create_ark_cluster_backup") return backup(id);
      if (command === "restore_ark_cluster_backup") {
        check(restoreFlight, "Restore must have an owned pending request"); return restoreFlight.promise;
      }
      if (command === "recover_ark_cluster_restore") {
        if (recoverFailure) throw new Error("fixture recovery rejected; retry is possible");
        check(recoverFlight, "Recovery must have an owned pending request"); return recoverFlight.promise;
      }
      return { action: input.action, members: [] };
    }
    if (command === "read_instance_isolation") return isolation;
    return invokeMock(command, args as Record<string, unknown>);
  });
  const render = () => root.render(<StrictMode><I18nProvider><InstanceSettingsSaveProvider><ServersView {...props} /></InstanceSettingsSaveProvider></I18nProvider></StrictMode>);
  async function open() {
    await act(async () => { render(); });
    await settleUntil(() => !!fixture.querySelector('[role="tab"][id$="-maintenance"]'), "server navigation");
    await click(select<HTMLButtonElement>('[role="tab"][id$="-maintenance"]'));
    await click(button("ARK 集群"));
    check(!select<HTMLElement>('[data-maintenance-section="cluster"]').hidden, "Cluster maintenance category did not open");
  }
  async function changeInstance(id: string) {
    const next = { ...summary, id, name: `实例 ${id}` };
    props = { ...props, selectedInstanceId: id, instances: [{ ...next, module_id: "fixture-art" }], selectedDetails: { ...props.selectedDetails!, summary: next } };
    await act(async () => { render(); });
    await click(button("ARK 集群"));
    check(!select<HTMLElement>('[data-maintenance-section="cluster"]').hidden, "Instance switch did not open the cluster maintenance category");
  }
  function verifyFit() {
    const outer = select<HTMLElement>(".server-detail-scroll--maintenance");
    check(outer.scrollWidth <= outer.clientWidth + 1, "Maintenance has horizontal overflow");
    check(select<HTMLElement>(".ark-cluster-table-wrap").getBoundingClientRect().height >= 90,
      "Bounded maintenance card collapsed the cluster member table");
    verifyControlsFit([".ark-cluster-panel", ".ark-cluster-backups"]);
  }
  function verifyControlsFit(selectors: string[]) {
    for (const selector of selectors) {
      const section = select<HTMLElement>(selector);
      check(section.clientWidth >= 240 && section.scrollWidth <= section.clientWidth + 1, `${selector} is clipped horizontally`);
      for (const element of section.querySelectorAll<HTMLElement>("button, input")) {
        const box = element.getBoundingClientRect(); const bounds = section.getBoundingClientRect();
        check(box.left >= bounds.left - 1 && box.right <= bounds.right + 1, `${selector} clips a control`);
      }
    }
  }
  await open();
  await settleUntil(() => !!fixture.querySelector(".ark-cluster-backup-list"), "initial cluster backup list");
  await click(button("运行策略"));
  check(!select<HTMLElement>('[data-maintenance-section="runtime"]').hidden, "Runtime maintenance category did not open");
  verifyControlsFit([".server-resource-policy"]);
  await click(button("ARK 集群"));
  verifyFit(); check(writes.length === 0, "Reading reports or pending state performed a mutation"); checks++;

  await click(select<HTMLInputElement>(".ark-cluster-backups input[type=checkbox]"));
  await click(button("恢复…"));
  check(panelText().includes("TheIsland") && panelText().includes("TheCenter"), "Restore confirmation omitted member maps");
  restoreFlight = deferred();
  const restore = button("恢复完整集群");
  await act(async () => { restore.click(); restore.click(); });
  check(writes.length === 1 && restore.disabled, "Restore was submitted twice or stayed enabled");
  await act(async () => { restoreFlight!.resolve({ backup: backup(summary.id), safeguard_backup: pendingFor(summary.id).safeguard_backup,
    restored_at_unix_ms: 1800000000010, cleanup_warnings: ["fixture cleanup warning remains visible"] }); });
  await settleUntil(() => panelText().includes("fixture cleanup warning remains visible"), "cleanup warning");
  await click(button("刷新", select(".ark-cluster-heading")));
  await settleUntil(() => !button("刷新", select(".ark-cluster-heading")).disabled, "stable report refresh");
  check(panelText().includes("fixture cleanup warning remains visible") && panelText().includes("已恢复完整集群"), "Report/list refresh erased restore outcome");
  const cleanupWarnings = [...fixture.querySelectorAll(".shell-activity-notice")]
    .filter((node) => node.textContent?.includes("fixture cleanup warning remains visible"));
  check(cleanupWarnings.length === 1, "Cleanup warning is missing or duplicated");
  check(cleanupWarnings[0].classList.contains("is-warning") && cleanupWarnings[0].getAttribute("role") === "status",
    "Cleanup warning lost its warning presentation or accessible status"); checks++;

  const damaged = "damaged-group";
  pending.set(damaged, pendingFor(damaged)); reportFailures.add(damaged);
  await changeInstance(damaged);
  await settleUntil(() => panelText().includes("查看处理范围"), "recovery despite report=null");
  check(panelText().includes("fixture member paths need recovery") && !fixture.querySelector(".ark-cluster-backups"), "Unreadable report hid recovery or exposed ordinary writes");
  check(writes.length === 1, "Inspecting pending recovery performed a mutation");
  await click(button("查看处理范围…"));
  check(button("确认处理整个集群").disabled, "Recovery bypassed explicit exclusivity acknowledgement");
  check(panelText().includes(`safeguard-${damaged}`) && panelText().includes(`d:/fixture/transfer/${damaged}`), "Recovery scope omitted directory or safeguard"); checks++;

  await click(select<HTMLInputElement>(".ark-cluster-restore-confirm input[type=checkbox]"));
  recoverFailure = true;
  await click(button("确认处理整个集群"));
  await settleUntil(() => panelText().includes("fixture recovery rejected"), "failed recovery feedback");
  await act(async () => { render(); });
  check(select<HTMLInputElement>(".ark-cluster-restore-confirm input[type=checkbox]").checked && !button("确认处理整个集群").disabled,
    "Failed recovery or a parent render erased reviewed scope or prevented retry"); checks++;

  recoverFailure = false; recoverFlight = deferred();
  const recovery = button("确认处理整个集群");
  await act(async () => { recovery.click(); recovery.click(); });
  check(writes.filter((write) => write.command === "recover_ark_cluster_restore").length === 2, "Recovery retry was submitted twice");
  pending.delete(damaged); reportFailures.delete(damaged);
  await act(async () => { recoverFlight!.resolve({ outcome: "rolled_back", backup: backup(damaged), cleanup_warnings: [] }); });
  await settleUntil(() => !!fixture.querySelector(".ark-cluster-backup-list"), "normal maintenance after recovery");
  check(panelText().includes("已将整个集群回滚到恢复操作之前"), "Successful recovery message was cleared by its report refresh"); checks++;

  inspectFlight = { id: "late-inspect", result: deferred() };
  await changeInstance("late-inspect");
  await changeInstance("replacement");
  await settleUntil(() => !!fixture.querySelector(".ark-cluster-backup-list"), "replacement report");
  await act(async () => { inspectFlight!.result.resolve(pendingFor("late-inspect")); });
  inspectFlight = null;
  check(!panelText().includes("safeguard-late-inspect") && !panelText().includes("处理上次中断的恢复"), "Old inspection contaminated the selected instance"); checks++;

  pending.set("late-operation", pendingFor("late-operation"));
  await changeInstance("late-operation");
  await settleUntil(() => panelText().includes("查看处理范围"), "pending old operation");
  await click(button("查看处理范围…")); await click(select<HTMLInputElement>(".ark-cluster-restore-confirm input[type=checkbox]"));
  recoverFlight = deferred(); await click(button("确认处理整个集群"));
  await changeInstance("replacement");
  await settleUntil(() => !!fixture.querySelector(".ark-cluster-backup-list"), "switch away during recovery");
  await act(async () => { recoverFlight!.resolve({ outcome: "completed", backup: backup("late-operation"), cleanup_warnings: ["STALE RECOVERY RESULT"] }); });
  check(!panelText().includes("STALE RECOVERY RESULT") && !panelText().includes("此前的整组恢复已经完成"), "Late recovery applied an old result to another instance"); checks++;

  await click(select<HTMLInputElement>(".ark-cluster-backups input[type=checkbox]"));
  await click(button("恢复…")); restoreFlight = deferred();
  await click(button("恢复完整集群"));
  pending.set("replacement", pendingFor("replacement")); reportFailures.add("replacement");
  const beforeFailure = writes.length;
  await act(async () => { restoreFlight!.reject(new Error("fixture interrupted restore")); });
  await settleUntil(() => panelText().includes("查看处理范围"), "failed restore exposes explicit recovery");
  check(writes.length === beforeFailure, "Failed restore status refresh automatically mutated recovery data");
  check(panelText().includes("fixture member paths need recovery"), "Failed restore did not refresh the unreadable member report"); checks++;

  await changeInstance("final-layout");
  await settleUntil(() => !!fixture.querySelector(".ark-cluster-backup-list"), "final healthy cluster");
  verifyFit();
  await act(async () => { root.unmount(); });
  root = createRoot(fixture); await open();
  await settleUntil(() => !!fixture.querySelector(".ark-cluster-backup-list"), "final readback after remount");
  check(!panelText().includes("STALE RECOVERY RESULT"), "Remount retained stale operation UI");
  verifyFit(); checks++;
  await act(async () => { root.unmount(); });
  root = createRoot(fixture);
  Object.assign(globalThis, { isTauri: false });
  sessionStorage.setItem("langameLanToken", "fixture-local-browser-token");
  const originalFetch = globalThis.fetch;
  let lanRequests = 0;
  globalThis.fetch = async (input, init) => {
    if (String(input).includes("/__langame/api")) { lanRequests++; throw new Error("LAN cluster backup endpoints must never be requested"); }
    return originalFetch(input, init);
  };
  try {
    for (const source of [report("lan-cluster"), null]) {
      await act(async () => { root.render(<StrictMode><I18nProvider><ArkClusterMaintenance instanceId="lan-cluster"
        report={source} busy={false} onChanged={async () => { throw new Error("LAN maintenance cannot mutate"); }}
        onBusyChange={noOperation} /></I18nProvider></StrictMode>); });
      check(fixture.textContent?.includes("需要在本机 LanGame 桌面端操作"), "LAN view omitted the local desktop requirement");
      check(!fixture.querySelector("button, input"), "LAN view exposed a backup or recovery control");
    }
    check(lanRequests === 0, "LAN mount inspected or mutated local-only cluster backup data");
  } finally {
    globalThis.fetch = originalFetch;
    sessionStorage.removeItem("langameLanToken");
    Object.assign(globalThis, { isTauri: true });
  }
  checks++;
  await act(async () => { root.unmount(); });
  root = createRoot(fixture); await open();
  await settleUntil(() => !!fixture.querySelector(".ark-cluster-backup-list"), "desktop reconnect after LAN gate");
  const clusterPanel = select<HTMLElement>(".ark-cluster-panel");
  const backupPanel = select<HTMLElement>(".ark-cluster-backups");
  const scrollOwner = clusterPanel.closest<HTMLElement>("[data-configuration-scroll-owner]");
  check(scrollOwner, "Cluster maintenance must have a bounded workspace scroll owner");
  scrollOwner.scrollTop += backupPanel.getBoundingClientRect().top - scrollOwner.getBoundingClientRect().top - 10;
  const createBounds = button("创建集群快照").getBoundingClientRect();
  const scrollBounds = scrollOwner.getBoundingClientRect();
  check(createBounds.top >= scrollBounds.top - 1 && createBounds.bottom <= scrollBounds.bottom + 1,
    "Cluster backup controls cannot be reached within the maintenance workspace scroll area");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, writes: writes.length, browser_errors: errors };
}
run().catch((error) => ({ status: "failed", checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
