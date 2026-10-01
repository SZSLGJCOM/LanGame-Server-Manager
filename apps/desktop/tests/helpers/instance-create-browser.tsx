import React, { act, StrictMode, useState, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC } from "@tauri-apps/api/mocks";
import { I18nProvider } from "../../src/i18n";
import { invokeMock } from "../../src/api-mock";
import { useInstanceActions } from "../../src/hooks/useDesktopActions";
import { useLibraryJobPolling } from "../../src/hooks/useDesktopEffects";
import { LibraryServerActions } from "../../src/views/library/LibraryServerActions";
import { ActivityNoticeTarget } from "../../src/components/ActivityNotice";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import type { UiMessage } from "../../src/app-ui";
import type { BootstrapResponse, InstanceProvisioning } from "../../src/types";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const noOperation = () => {};
const opened: string[] = [];
const requests: Array<ReturnType<typeof deferred<InstanceProvisioning>>> = [];
let refresh = deferred<BootstrapResponse>();
let refreshCalls = 0;
let jobPolls = 0;
let actions: ReturnType<typeof useInstanceActions>;

function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
async function settleUntil(predicate: () => boolean, description: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    check(performance.now() < deadline, description);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
const submit = () => fixture.querySelector<HTMLButtonElement>('button[type="submit"]')!;
const input = () => fixture.querySelector<HTMLInputElement>(".text-input")!;
async function click(id: string) {
  await act(async () => { document.getElementById(id)!.click(); });
}
async function key(name: "Enter") {
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/${name}`, { method: "POST" });
    check(response.ok, `Native ${name} dispatch failed`);
  });
}

function Frame({ children }: { children: ReactNode }) {
  const [target, setTarget] = useState<HTMLDivElement | null>(null);
  return <ActivityNoticeTarget.Provider value={{ element: target, dismissLabel: "关闭" }}>
    <main style={{ width: 360, padding: 24 }}>{children}</main>
    <footer className="shell-activity-bar"><span className="shell-activity-label">活动</span>
      <div className="shell-activity-notices" ref={setTarget} /></footer>
  </ActivityNoticeTarget.Provider>;
}

async function run() {
  await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]);
  const bootstrap = await invokeMock<BootstrapResponse>("bootstrap");
  const selected = { ...bootstrap.state.modules[0], install_state: "Installed" as const };
  const provisioning: InstanceProvisioning = {
    summary: { ...bootstrap.state.instances[0], id: "fixture-created", module_id: selected.id, name: "合作世界" },
    ports: [], config_file_path: "fixture/config/instance.json"
  };
  // Only native creation is deferred. The real hook, API, controls, translations,
  // remount lifecycle, and application styles run unchanged.
  Object.assign(globalThis, { isTauri: true });
  mockIPC((command) => {
    if (command === "read_background_jobs") { jobPolls++; return []; }
    if (command === "inspect_module_programs") return {
      installations: [{ id: 1, install_root: "fixture/library", scope: "library", install_state: "Installed",
        current_version: "fixture-version", used_by: [], modification_state: "unverified", pending_removal: false, size_bytes: 100 * 1024 ** 3 }],
      creation: { can_create: true, action: "existing_install",
        program_path: "fixture/library", additional_bytes: 0, reason: null }
    };
    check(command === "create_instance_record", `Unexpected native command: ${command}`);
    const request = deferred<InstanceProvisioning>();
    requests.push(request);
    return request.promise;
  });
  function Harness() {
    const [visible, setVisible] = useState(true);
    const [activity, setActivity] = useState<UiMessage | null>(null);
    const [name, setName] = useState("合作世界");
    actions = useInstanceActions({
      bindAddressCandidates: [], cacheInstancePanel: noOperation, clearSelectedInstancePanel: noOperation,
      getCurrentInstanceId: () => null, instanceBackupsById: {}, instanceDetailsById: {},
      markRuntimeRefreshed: noOperation, modules: [selected],
      openInstanceView: (section, id) => { check(section === "settings", "Created instance must open settings"); opened.push(id); },
      replaceSelectedInstancePanel: noOperation, selectedInstanceId: null,
      setInstanceBackupsById: noOperation, setInstanceDetailsById: noOperation, setInstanceRuntimesById: noOperation,
      setSelectedInstanceBackups: noOperation, setSelectedInstanceDetails: noOperation, setActivity,
      setBootstrap: noOperation, setSelectedModuleId: noOperation, setSelectedInstanceId: noOperation,
      reloadBootstrap: () => { refreshCalls++; return refresh.promise; },
      refreshInstallationState: async () => ({ modules: [selected], instanceId: null, preview: { launchPlan: null, launchPlanError: null } })
    });
    useLibraryJobPolling({ enabled: visible || actions.creatingModuleIds.size > 0, onJobsSynced: noOperation });
    return <Frame>
      <button id="toggle" type="button" onClick={() => setVisible(!visible)}>切换页面</button>
      {visible && <div className="library-detail-page"><LibraryServerActions selected={selected}
        steamCmdStatus={null} steamCmdBusy={false}
        selectedModuleDetails={null} installLabel="已安装" installBusy={false} installStatusClass="is-success"
        creating={actions.creatingModuleIds?.has(selected.id) ?? false} instanceName={name}
        creationStartedAt={actions.creationStartedAtByModule.get(selected.id)}
        onInstall={noOperation} onUninstall={noOperation} onInstanceNameChange={setName}
        onCreateServer={actions.handleCreateServer} /></div>}
      <output id="activity">{activity?.key}</output>
    </Frame>;
  }
  await act(async () => { root.render(<StrictMode><I18nProvider><InstanceSettingsSaveProvider>
    <Harness />
  </InstanceSettingsSaveProvider></I18nProvider></StrictMode>); });
  await settleUntil(() => Boolean(submit()), "Creation controls did not render");
  await act(async () => { submit().click(); submit().click(); });
  check(requests.length === 1, `Repeated clicks started ${requests.length} native creations`);
  check(submit().disabled && input().disabled, "Pending creation must disable submit and instance name");
  check(fixture.querySelector("form")?.getAttribute("aria-busy") === "true", "Creation form must expose pending state");
  const hint = fixture.querySelector<HTMLElement>('.shell-activity-notice[role="status"]');
  check(hint?.textContent?.includes("正在校验原版服务器文件"), "Original creation explains verification while it is pending");
  check(hint?.textContent?.includes("需要时下载修复，完成后自动创建"), "Original creation explains conditional repair and automatic continuation");
  check(getComputedStyle(hint).whiteSpace === "normal" && hint.scrollWidth <= hint.clientWidth,
    "Creation explanation must wrap completely inside the sidebar");
  check(submit().textContent?.includes("正在"), "Create button must show pending text");
  const startedAt = actions.creationStartedAtByModule.get(selected.id);
  check(typeof startedAt === "number", "Pending creation must retain its real start time");
  await settleUntil(() => /已等待 [1-9]\d* 秒/.test(fixture.querySelector('.shell-activity-notice[role="status"]')?.textContent ?? ""),
    "Pending creation must display elapsed wall-clock seconds");
  await act(async () => { fixture.querySelector<HTMLButtonElement>(".shell-activity-notice-close")!.click(); });
  const dismissedAt = Date.now();
  await settleUntil(() => Date.now() - dismissedAt > 1100, "Wait for the next elapsed-time refresh after dismissing");
  check(!fixture.querySelector('.shell-activity-notice[role="status"]'), "Dismissed creation progress must not reopen on the next timer tick");
  check(submit().disabled && requests.length === 1, "Dismissing progress must not pretend to cancel the native request");
  await click("toggle");
  const pollsBeforeNavigation = jobPolls;
  await settleUntil(() => jobPolls > pollsBeforeNavigation, "Pending creation must keep repair jobs discoverable after leaving the library");
  await click("toggle");
  check(submit().disabled && input().disabled, "Returning to the library lost in-flight creation state");
  check(actions.creationStartedAtByModule.get(selected.id) === startedAt
    && /已等待 [1-9]\d* 秒/.test(fixture.querySelector('.shell-activity-notice[role="status"]')?.textContent ?? ""),
    "Returning to the library must preserve elapsed preparation time");
  let duplicateFinished = false;
  await act(async () => {
    void actions.handleCreateServer({ module_id: selected.id, name: "重复请求" }).then(() => { duplicateFinished = true; });
  });
  check(requests.length === 1 && !duplicateFinished, "Hook must coalesce creation after remount and retain its Promise");
  await act(async () => { requests[0].resolve(provisioning); });
  await settleUntil(() => refreshCalls === 1, "Creation did not refresh workspace");
  check(submit().disabled && opened.length === 0, "Controls reopened before the created instance was loaded");
  await act(async () => { refresh.resolve(bootstrap); });
  await settleUntil(() => !submit().disabled, "Successful creation did not release controls");
  check(opened.length === 1 && duplicateFinished && !input().disabled, "Successful creation lost navigation or waiter completion");
  check(!actions.creationStartedAtByModule.has(selected.id), "Completed creation must release its elapsed-time owner");

  input().focus();
  await key("Enter");
  await settleUntil(() => requests.length === 2, "Keyboard submit did not begin creation");
  await act(async () => { requests[1].reject(new Error("Fixture copy failed")); });
  await settleUntil(() => !submit().disabled, "Failed creation did not release controls");
  check(document.getElementById("activity")?.textContent === "activity.createServerFailed", "Creation failure must be surfaced");
  check(opened.length === 1 && !input().disabled, "Failure navigated or kept input disabled");

  await act(async () => { submit().click(); });
  await settleUntil(() => requests.length === 3, "Cancellation scenario did not begin creation");
  await act(async () => { requests[2].reject(new Error("installation_cancelled. See app log: D:/fixture/app.log")); });
  await settleUntil(() => !submit().disabled, "Cancelled creation did not release controls");
  check(document.getElementById("activity")?.textContent === "activity.serverCreationCancelled",
    "Cancelling automatic repair reports that creation was cancelled");
  check(opened.length === 1 && refreshCalls === 1 && !actions.creationStartedAtByModule.has(selected.id),
    "Cancellation must not create, navigate or retain an in-flight request");

  refresh = deferred<BootstrapResponse>();
  await act(async () => { submit().click(); });
  await settleUntil(() => requests.length === 4, "Cancelled creation could not be retried");
  await act(async () => { requests[3].resolve(provisioning); refresh.resolve(bootstrap); });
  await settleUntil(() => !submit().disabled, "Retry did not finish");
  check(opened.length === 2, "Successful retry did not navigate once");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  // Retain the real pending control presentation for visual review after all
  // deferred native requests have completed.
  await act(async () => { root.render(<I18nProvider><Frame><div className="library-detail-page">
    <LibraryServerActions selected={selected} selectedModuleDetails={null} installLabel="已安装"
      steamCmdStatus={null} steamCmdBusy={false}
      installBusy={false} installStatusClass="is-success" creating creationStartedAt={Date.now() - 5000} instanceName="合作世界"
      onInstall={noOperation} onUninstall={noOperation} onInstanceNameChange={noOperation} onCreateServer={async () => {}} />
  </div></Frame></I18nProvider>); });
  check(document.documentElement.scrollWidth <= innerWidth, "Creation status overflows the desktop viewport");
  return { status: "passed", requests: requests.length, opened: opened.length, browser_errors: errors };
}

void run().catch((error) => ({ status: "failed", error: String(error instanceof Error ? error.stack : error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
