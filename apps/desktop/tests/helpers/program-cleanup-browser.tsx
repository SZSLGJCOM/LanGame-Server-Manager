import React, { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { invokeMock } from "../../src/api-mock";
import { I18nProvider, useI18n } from "../../src/i18n";
import { useLibraryActions } from "../../src/hooks/useDesktopActions";
import { message, resolveUiMessage, type UiMessage } from "../../src/app-ui";
import { ActivityNoticeContent } from "../../src/components/ActivityNotice";
import { LibraryServerActions } from "../../src/views/library/LibraryServerActions";
import type { BootstrapResponse, ModuleDetails, ModuleUninstallResult } from "../../src/types";
import type { ModuleProgramInventory, ProgramInstallationSummary } from "../../src/storage-management-types";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", innerWidth < 1000 ? "zh-CN" : "en-US");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
fixture.style.cssText = "height:100vh;padding:24px;box-sizing:border-box;display:flex;flex-direction:column";
const root = createRoot(fixture);
const checks: string[] = [], errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
function check(value: unknown, description: string): asserts value {
  if (!value) throw new Error(description);
  checks.push(description);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const value = fixture.querySelector<T>(selector);
  check(value, `Element exists: ${selector}`);
  return value;
}
function deferred() {
  let resolve!: (value: ModuleUninstallResult) => void, reject!: (error: Error) => void;
  const promise = new Promise<ModuleUninstallResult>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
let module: ModuleDetails, bootstrap: BootstrapResponse;
let installations: ProgramInstallationSummary[] = [];
let inspections = 0;
const calls: ReturnType<typeof deferred>[] = [];
Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
  await act(async () => {
    root.unmount();
    for (const call of calls) call.reject(new Error("UNMOUNTED_CLEANUP_FIXTURE"));
  });
  return { browser_errors: errors, native_dialogs: 0 };
} });
let t: ReturnType<typeof useI18n>["t"];
const noop = () => {};
function Harness() {
  const i18n = useI18n();
  t = i18n.t;
  const [activity, setActivity] = useState<UiMessage>(message("activity.idle"));
  const actions = useLibraryActions({
    setActivity, setBootstrap: noop, setSelectedModuleId: noop, setSelectedInstanceId: noop,
    setLibraryTaskPolling: noop, setSteamCmdBusy: noop, setSteamCmdMessage: noop,
    setSteamCmdStatus: noop, setSteamCmdProgress: noop, onSteamCmdOperationStart: noop,
    refreshSteamCmdStatus: async () => {}, reloadBootstrap: async () => bootstrap,
    refreshInstallationState: async () => ({ modules: [module.summary], instanceId: null,
      preview: { launchPlan: null, launchPlanError: null } })
  });
  return <>
    <main style={{ flex: 1, minHeight: 0, overflowY: "auto" }}>
      <section className="library-detail-page" style={{ width: 320, maxWidth: "100%" }}>
        <LibraryServerActions selected={module.summary} selectedModuleDetails={module}
          steamCmdStatus={null} steamCmdBusy={false} installLabel={t("status.install.installed")}
          installBusy={false} installStatusClass="is-success" creating={false}
          instanceName="Clean program fixture" onInstanceNameChange={noop} onInstall={noop}
          onCreateServer={async () => {}} onUninstall={actions.handleUninstallModule} />
      </section>
    </main>
    <footer className="shell-activity-bar"><span className="shell-activity-label">{t("shell.activity")}</span>
      <div className="shell-activity-notices"><ActivityNoticeContent tone={activity.tone ?? "info"}
        dismissLabel={t("common.close")} onDismiss={noop}>{resolveUiMessage(t, activity)}</ActivityNoticeContent></div>
    </footer>
  </>;
}
async function draw() { await act(async () => { root.render(<I18nProvider><Harness /></I18nProvider>); }); }
async function click(selector: string) { await act(async () => { element<HTMLButtonElement>(selector).click(); }); }
async function settled() {
  const deadline = performance.now() + 5000;
  while (!fixture.querySelector('.library-program-inventory[aria-busy="false"]')) {
    check(performance.now() < deadline, "Program inventory settles");
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
const uninstall = ".library-sidebar-maintenance-row .inline-confirm-action > button";
function count(expected: number) {
  check(element(".library-program-metric").getAttribute("aria-label") === t("library.programs.count", { count: expected }),
    `Visible inventory count is ${expected}`);
}
async function confirm() {
  await click(uninstall);
  const confirmation = element(".inline-confirm-message").textContent;
  check(confirmation === t("library.detail.uninstallConfirm", { name: module.summary.name }), "The confirmation describes all safe library cleanup");
  await click(".inline-confirm-submit");
}
function result(removed: string[], retained: ModuleUninstallResult["cleanup"]["retained_installs"] = []): ModuleUninstallResult {
  return { module_id: module.summary.id, install_state: retained.length ? "Installed" : "NotInstalled",
    executable_exists: retained.length > 0,
    cleanup: { removed_install_roots: removed, preserved_data_paths: [], retained_installs: retained } };
}
async function run() {
  bootstrap = await invokeMock<BootstrapResponse>("bootstrap");
  module = await invokeMock<ModuleDetails>("read_module_details", { moduleId: "palworld" });
  module = { ...module, summary: { ...module.summary, install_state: "Installed", instance_program_count: 1, archived_program_count: 1 } };
  const installation = (id: number, scope: "library" | "instance", install_root: string): ProgramInstallationSummary => ({
    id, scope, install_root, install_state: "Installed", pending_removal: false, current_version: "fixture", used_by: [],
    modification_state: "unverified", size_bytes: 4096
  });
  const base = installation(1, "library", "fixture/library/base");
  const extra = installation(2, "library", "fixture/library/extra");
  const independent = installation(3, "instance", "fixture/instances/private/runtime");
  installations = [base, extra, independent];
  Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: (command: string) => {
    if (command === "read_steamcmd_prepare_progress") return Promise.resolve(null);
    if (command === "inspect_module_programs") {
      inspections++;
      const response: ModuleProgramInventory = { requires_archive_inventory: false, installations: structuredClone(installations),
        creation: { can_create: true, action: "independent_install", program_path: base.install_root, additional_bytes: 4096, reason: null } };
      return Promise.resolve(response);
    }
    if (command === "uninstall_module_game") { const call = deferred(); calls.push(call); return call.promise; }
    throw new Error(`Unexpected cleanup fixture command: ${command}`);
  } } });
  await draw(); await settled(); count(3);
  await click(uninstall); await click(".inline-confirm-buttons button");
  check(calls.length === 0, "Cancelling confirmation does not request cleanup");
  const before = inspections;
  await confirm();
  check(element<HTMLButtonElement>(".inline-confirm-submit").disabled, "Pending cleanup prevents duplicate confirmation");
  const partial = result([extra.install_root], [{ install_root: base.install_root, reason: "archive_dependency" }]);
  partial.cleanup.preserved_data_paths = ["fixture/preserved/operator-world"];
  installations = [base, independent];
  await act(async () => { calls[0].resolve(partial); }); await settled(); count(2);
  check(inspections > before, "Unchanged module summary and missed busy job still trigger a fresh inventory");
  const notice = element(".shell-activity-notice");
  check(notice.classList.contains("is-warning") && notice.textContent?.includes(base.install_root)
    && notice.textContent?.includes(t("programCleanup.reason.archive_dependency"))
    && notice.textContent?.includes("fixture/preserved/operator-world"), "Partial cleanup shows retained paths, translated reason and personal data");
  check(!notice.textContent?.includes("archive_dependency"), "User feedback does not expose stable reason codes");

  await confirm();
  installations = [independent];
  await act(async () => { calls[1].reject(new Error("CLEANUP_FAILURE_END")); }); await settled(); count(1);
  check(element(".shell-activity-notice").textContent?.includes("CLEANUP_FAILURE_END"), "Cleanup failure remains visible after state refresh");
  check(element<HTMLButtonElement>(uninstall).disabled, "A private installation alone cannot enable library removal");

  module = { ...module, summary: { ...module.summary, install_state: "NotInstalled" } };
  installations = [extra, independent];
  await draw(); await settled(); count(2);
  check(!element<HTMLButtonElement>(uninstall).disabled, "A remaining library enables cleanup when the primary summary says not installed");
  await confirm();
  installations = [{ ...extra, install_state: "NotInstalled" }, independent];
  await act(async () => { calls[2].resolve(result([extra.install_root])); }); await settled(); count(1);
  check(element<HTMLButtonElement>(uninstall).disabled, "Personal data in a not-installed library does not enable further removal or increase the program count");
  check(element(".shell-activity-notice").textContent?.trim() === t("activity.uninstallCompleted", { moduleId: module.summary.id, count: 1 }),
    "Ordinary completion stays concise");

  installations = [extra, independent];
  await act(async () => { module = { ...module, summary: { ...module.summary, install_state: "Installed" } }; });
  await draw(); await settled(); count(2);
  await confirm();
  module = { ...module, summary: { ...module.summary, install_state: "NotInstalled" } };
  installations = [{ ...extra, install_state: "NotInstalled", pending_removal: true }, independent];
  await act(async () => { calls[3].reject(new Error("COMMITTED_CLEANUP_RETRY")); });
  await draw(); await settled(); count(1);
  check(!element<HTMLButtonElement>(uninstall).disabled, "A committed pending removal remains retryable without increasing the installed count");
  check(element(".shell-activity-notice").textContent?.includes("COMMITTED_CLEANUP_RETRY"), "Committed cleanup failure stays visible for retry");
  await confirm();
  installations = [{ ...extra, install_state: "NotInstalled", pending_removal: false }, independent];
  await act(async () => { calls[4].resolve(result([extra.install_root])); }); await settled(); count(1);
  check(element<HTMLButtonElement>(uninstall).disabled, "Completed journal cleanup disables removal when only personal data remains");
  check(document.documentElement.scrollWidth <= innerWidth, "Cleanup controls remain within the viewport");
  await act(async () => {
    await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]);
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  });
  check(errors.length === 0, "No browser or React errors");
  return { status: "passed", checks, inspections, uninstall_requests: calls.length, browser_errors: errors };
}
void run().catch((error) => ({ status: "failed", error: String(error?.stack ?? error), checks, browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
