import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { invokeMock } from "../../src/api-mock";
import { I18nProvider } from "../../src/i18n";
import { LibraryServerActions } from "../../src/views/library/LibraryServerActions";
import type { ModuleDetails, SteamCmdStatus } from "../../src/types";
import type { ModuleProgramInventory } from "../../src/storage-management-types";
import "../../src/app.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
let installedInventory = false;
// Program ownership is an external native boundary; no production files or
// running service are inspected. Inventory follows the mounted install state.
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: (command: string, args: Record<string, unknown> = {}) => {
  if (command === "inspect_module_programs") return Promise.resolve({ requires_archive_inventory: false,
    installations: installedInventory ? [{ id: 1, install_root: "fixture/library", scope: "library",
      install_state: "Installed", pending_removal: false, current_version: null, used_by: [],
      modification_state: "verified_original", size_bytes: null }] : [],
    creation: { can_create: false, action: "independent_install", program_path: "", additional_bytes: null,
      reason: "请先安装服务器文件。" } } satisfies ModuleProgramInventory);
  return invokeMock(command, args);
} } });
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const nonce = new URLSearchParams(location.search).get("nonce");
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
let checks = 0;
let calls = 0;
function check(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
  checks++;
}
async function settle(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(message);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function render(details: ModuleDetails, status: SteamCmdStatus | null, busy = false, installed = false) {
  installedInventory = installed;
  const selected = { ...details.summary, install_state: installed ? "Installed" : "NotInstalled" };
  await act(async () => {
    root.render(<StrictMode><I18nProvider><main className="library-detail-page" style={{ padding: 48, width: 440 }}>
      <LibraryServerActions selected={selected} selectedModuleDetails={{ ...details, summary: selected }}
        steamCmdStatus={status} steamCmdBusy={busy} installLabel={installed ? "已安装" : "未安装"}
        installBusy={false} installStatusClass="" creating={false} instanceName="依赖回归实例"
        onInstall={() => { calls++; }} onUninstall={() => {}} onCreateServer={async () => {}}
        onInstanceNameChange={() => {}} />
    </main></I18nProvider></StrictMode>);
  });
  await settle(() => Boolean(fixture.querySelector(".library-install-state-button")), "Library actions did not render");
  await settle(() => fixture.querySelector(".library-program-inventory")?.getAttribute("aria-busy") === "false",
    "Program inventory did not settle");
}
function installButton() { return fixture.querySelector<HTMLButtonElement>(".library-install-state-button")!; }
function validateButton() { return fixture.querySelectorAll<HTMLButtonElement>(".library-sidebar-maintenance-row button")[1]; }
async function tooltip(expected: string) {
  const wrapper = installButton().parentElement!;
  await act(async () => { wrapper.focus(); });
  check(document.activeElement === wrapper, "Disabled action must remain keyboard discoverable");
  await settle(() => Boolean(document.querySelector('.configuration-field-help-tooltip.is-visible')?.textContent?.includes(expected)), "Dependency tooltip is missing");
  const node = document.querySelector<HTMLElement>('.configuration-field-help-tooltip.is-visible')!;
  const rect = node.getBoundingClientRect();
  check(rect.width > 0 && rect.height > 0 && rect.left >= 0 && rect.right <= innerWidth && rect.bottom <= innerHeight,
    "Tooltip must be visible inside the viewport");
  check(document.getElementById(wrapper.getAttribute("aria-describedby") ?? "")?.textContent?.includes(expected),
    "Dependency explanation must be associated with the control");
}
async function rejectMock(command: string, moduleId: string) {
  let error: unknown;
  try { await invokeMock(command, { moduleId }); } catch (cause) { error = cause; }
  check(error instanceof Error && JSON.parse(error.message).code === "steamcmd_not_ready", "Preview must reject missing SteamCMD");
}
async function run() {
  await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]);
  const steam = await invokeMock<ModuleDetails>("read_module_details", { moduleId: "palworld" });
  const absent = await invokeMock<SteamCmdStatus>("uninstall_steamcmd");
  await render(steam, absent);
  check(installButton().disabled && validateButton().disabled, "Steam actions must be disabled after uninstall");
  await act(async () => { installButton().click(); validateButton().click(); });
  check(calls === 0, "Disabled actions must not submit installation");
  await tooltip("尚未安装 SteamCMD");
  await rejectMock("install_module_game", steam.summary.id);
  await rejectMock("validate_module_game", steam.summary.id);
  check(!(await invokeMock<SteamCmdStatus>("probe_steamcmd_status")).ready, "Server acquisition must not restore the dependency");

  await render(steam, absent, true);
  check(installButton().disabled, "Dependency changes must keep installation blocked");
  await render(steam, { ...absent, executable_exists: true });
  check(installButton().disabled, "An unverified executable is insufficient");
  await render(steam, null);
  check(installButton().disabled, "Unknown readiness must fail closed");

  const ready = await invokeMock<SteamCmdStatus>("ensure_steamcmd_ready", { operationId: "dependency-fixture" });
  await render(steam, ready);
  check(!installButton().disabled, "Explicit preparation must re-enable installation");
  await act(async () => { installButton().click(); });
  check(calls === 1, "Ready installation must submit exactly once");
  await invokeMock("install_module_game", { moduleId: steam.summary.id });
  await render(steam, ready, false, true);
  check(!validateButton().disabled, "Ready installed servers must allow validation");
  await render(steam, absent, false, true);
  check(validateButton().disabled, "Uninstall must also disable updates and validation");
  const uninstall = fixture.querySelector<HTMLButtonElement>(".inline-confirm-action > button")!;
  check(!uninstall.disabled, "Removing server files does not require SteamCMD");

  await invokeMock("uninstall_steamcmd");
  for (const moduleId of ["minecraft", "rimworld"]) {
    const independent = await invokeMock<ModuleDetails>("read_module_details", { moduleId });
    check(independent.install?.source === "minecraft_java" || independent.install?.download_url_windows,
      "Independent test module must use a real non-Steam acquisition source");
    await render(independent, absent);
    check(!installButton().disabled, `${moduleId} must not require SteamCMD`);
    await invokeMock("install_module_game", { moduleId });
    check(!(await invokeMock<SteamCmdStatus>("probe_steamcmd_status")).ready, "Independent install must not mark SteamCMD ready");
  }
  await render(steam, absent);
  await tooltip("尚未安装 SteamCMD");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, browser_errors: errors };
}
void run().catch((error) => ({ status: "failed", checks, error: String(error instanceof Error ? error.stack : error), browser_errors: errors }))
  .then((report) => {
    // Assertions are complete. The screenshot runner and browser shutdown may
    // subsequently move focus; those events are outside the React test scope.
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
    return fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) });
  });
