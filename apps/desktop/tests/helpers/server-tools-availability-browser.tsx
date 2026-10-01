import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, StrictMode, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { bootstrapApp, readInstanceDetails, readInstanceRuntime, readModuleDetails } from "../../src/api";
import { invokeMock } from "../../src/api-mock";
import type { ArkSpawnInput, ArkSpawnResult } from "../../src/api-ark-tools";
import { createDefaultAiSettings } from "../../src/ai-settings";
import { createInitialAppUpdateState } from "../../src/app-update-model";
import { AppShell } from "../../src/components/AppShell";
import { I18nProvider } from "../../src/i18n";
import { ServersView } from "../../src/views/ServersView";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
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
const supported = new Set([
  "arksurvivalascended", "arksurvivalevolved", "dontstarve"
]);
const unavailable = {
  "zh-CN": "该游戏暂未提供可用工具。",
  "en-US": "This game does not currently provide server tools."
};
let checks = 0;

function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(description);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const target = document.querySelector<T>(selector);
  check(target, `Missing ${selector}`);
  return target;
}
function tab(id: string) {
  return element<HTMLButtonElement>(`.server-detail-tabs [role="tab"][id$="-${id}"]`);
}
function selectedTab() {
  return element<HTMLButtonElement>('.server-detail-tabs [role="tab"][aria-selected="true"]');
}
async function settleUntil(predicate: () => boolean, description: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    check(performance.now() < deadline, description);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function nativeKey(key: "Tab" | "Enter" | "Escape") {
  await act(async () => {
    const response = await fetch(`/__reliability_key/${nonce}/${key}`, { method: "POST" });
    check(response.ok, `Native ${key} dispatch failed`);
  });
}
async function focus(target: HTMLElement) {
  await act(async () => {
    target.scrollIntoView({ block: "nearest", inline: "nearest" });
    target.focus();
  });
  check(document.activeElement === target, "Keyboard target must receive focus");
}
function assertFits(target: HTMLElement, description: string) {
  const rect = target.getBoundingClientRect();
  check(rect.width > 0 && rect.height > 0 && rect.left >= -1 && rect.right <= innerWidth + 1
    && rect.top >= -1 && rect.bottom <= innerHeight + 1, `${description} must fit the viewport: ${JSON.stringify(rect.toJSON())}`);
  check(target.scrollWidth <= target.clientWidth + 1, `${description} must not overflow horizontally`);
}
function noUnavailablePage() {
  const panel = element('[role="tabpanel"]');
  check(!panel.textContent?.includes(unavailable["zh-CN"]) && !panel.textContent?.includes(unavailable["en-US"]),
    "Unavailable explanation must not appear as a tool page");
  check(!panel.querySelector(".gmt-workbench"), "Unsupported game must not mount a tool workbench");
}
function helpTrigger() {
  const help = tab("gm").closest<HTMLElement>(".server-detail-tab-help");
  check(help?.tagName === "SPAN" && help.tabIndex === 0, "Disabled tools need a keyboard-focusable help span");
  check(help.getAttribute("aria-label") === tab("gm").textContent?.trim(), "Help must have the tools tab label");
  check(!help.hasAttribute("title"), "Help must not duplicate its bubble with a native title tooltip");
  return help;
}
async function visibleTooltip(text: string) {
  await settleUntil(() => {
    const tooltip = document.querySelector(".configuration-field-help-tooltip.is-visible");
    return Boolean(tooltip && Number(getComputedStyle(tooltip).opacity) === 1);
  }, "Help bubble did not finish appearing");
  const tooltip = element(".configuration-field-help-tooltip.is-visible");
  check(tooltip.textContent === text, `Help bubble text must match the current locale: ${tooltip.textContent}`);
  const help = helpTrigger();
  const describedBy = help.getAttribute("aria-describedby");
  check(describedBy, "Keyboard help trigger must describe its disabled state accessibly");
  const description = document.getElementById(describedBy);
  check(description?.getAttribute("role") === "tooltip" && description.textContent === text,
    "aria-describedby must resolve to the localized help text");
  assertFits(help, "Tools help trigger");
  assertFits(tooltip, "Tools help bubble");
}
async function hiddenTooltip() {
  await settleUntil(() => !document.querySelector(".configuration-field-help-tooltip"), "Help bubble did not leave after dismissal");
}

async function run() {
  // Only native/storage/network boundaries use the development mock. The shell,
  // real ServersView, detail tabs, tool forms and help interaction run unchanged.
  const bootstrap = await bootstrapApp({ includeSystemSnapshot: false });
  const original = bootstrap.state.instances.find((instance) => instance.module_id === "minecraft");
  check(original, "Development mock must include a Minecraft instance");
  const stored = await readInstanceDetails(original.id);
  const runtime = { ...await readInstanceRuntime(original.id), recent_runs: [], diagnostics: [] };
  const instance = { ...original, module_id: "valheim", name: "Valheim 工具可用性验证", status: "Stopped" as const, active_process_count: 0 };
  const details = { ...stored, summary: instance, active_run: null };
  const aiSettings = createDefaultAiSettings();
  const viewProps: ComponentProps<typeof ServersView> = {
    aiSettings, assistantCanRun: false, bindAddressCandidates: [], instances: [instance],
    moduleInstallations: {}, instanceLaunchPlans: {}, instanceLaunchFailures: {},
    section: "overview", onWorkspaceSectionChange: noOperation,
    selectedInstanceId: instance.id, selectedDetails: details, selectedBackups: [], selectedModuleDetails: null,
    runtime, runtimeWindows: null, launchPlan: null, launchPlanError: null, refreshIssue: null,
    onActivity: noOperation, onResumeAutoRefresh: noOperation, onSelectInstance: noOperation,
    onStart: noOperation, onStop: noOperation, onInstallModule: async () => {}, onOpenModuleLibrary: noOperation,
    onCreateInstance: noOperation, onPickDirectory: async () => null,
    onImportDontStarveWorldData: async () => { throw new Error("Unexpected world import"); },
    onOpenLocalPath: noOperation, onSendRuntimeCommand: async () => null, onSuppressRuntimeWindows: async () => {},
    onCreateBackup: noOperation, onArchivesChanged: async () => {}, onArchiveInstance: async () => {}, onDeleteInstance: async () => {}, onRestoreBackup: noOperation,
    onRenameBackup: async () => true, onDeleteBackup: async () => {}, onSaveSettings: async () => undefined,
    onSaveAutostart: async () => {}, onApplyPlayerAccessMutation: async () => { throw new Error("Unexpected player mutation"); }
  };
  const shellProps: Omit<ComponentProps<typeof AppShell>, "children"> = {
    activeView: "servers", theme: "dark", serverCount: 1, aiSettings, activityText: "界面已就绪",
    appUpdatesEnabled: true, appUpdateState: createInitialAppUpdateState("0.1.0"), assistant: { panelTitle: "助手", tone: "info" }, assistantDraft: "",
    assistantExecution: { status: "idle", promptLabel: null, result: null, error: null },
    assistantMessages: [], assistantConversations: [], assistantActiveConversationId: null,
    assistantInput: { aiSettings, locale: "zh-CN", activeJobsCount: 0, activeView: "servers", bootstrap,
      storageReady: true, libraryPage: "catalog", overlayNames: [], runtimeAutoRefreshPaused: false, runtimeRefreshIssue: null,
      selectedInstanceDetails: details, selectedInstanceId: instance.id, selectedModuleId: null, selectedInstanceModuleDetails: null,
      selectedLaunchPlan: null, selectedLaunchPlanError: null, selectedLogDocument: runtime.log_tail,
      selectedModuleDetails: null, selectedRuntime: runtime, serverWorkspaceSection: "overview", steamCmdStatus: null },
    jobs: [], steamCmdProgress: null, steamCmdMessage: "", steamCmdStopPending: false, steamCmdStopError: null,
    installationStopPendingIds: [], installationStopErrors: {}, onCancelInstallation: noOperation, onCancelSteamCmd: noOperation,
    runtimeRefreshIssue: null, runtimeAutoRefreshPaused: false, runtimePollIntervalMs: 5000, runtimeRefreshFailureLimit: 3,
    onResumeRuntimeAutoRefresh: noOperation, onAssistantAction: noOperation, onAssistantDeleteConversation: noOperation,
    onAssistantDraftChange: noOperation, onAssistantNewConversation: noOperation, onAssistantSelectConversation: noOperation,
    onAssistantRunPrompt: noOperation, onAssistantSendMessage: noOperation, onCheckAppUpdate: noOperation,
    onClearAiSecret: async (next) => next, onInstallAppUpdate: noOperation, onSaveAiSettings: async (next) => next,
    onSelectView: noOperation, onThemeChange: noOperation
  };
  function render() {
    root.render(<StrictMode><I18nProvider><InstanceSettingsSaveProvider>
      <AppShell {...shellProps}><ServersView {...viewProps} /></AppShell>
    </InstanceSettingsSaveProvider></I18nProvider></StrictMode>);
  }
  async function selectModule(moduleId: string, withModuleDetails = false) {
    await act(async () => {
      const summary = { ...instance, module_id: moduleId, name: `${moduleId} 工具可用性验证` };
      viewProps.selectedDetails = { ...details, summary };
      viewProps.instances = [summary];
      viewProps.selectedModuleDetails = withModuleDetails ? await readModuleDetails(moduleId) : null;
      render();
    });
  }
  await act(async () => { await prepareBrowserLocaleCatalogs(); render(); });
  await settleUntil(() => Boolean(fixture.querySelector('.server-detail-tabs [id$="-gm"]')), "Server tabs did not mount");
  check(tab("gm").disabled && tab("gm").getAttribute("aria-disabled") === "true", "Valheim tools tab must be natively disabled");
  await act(async () => { tab("gm").click(); });
  check(selectedTab() === tab("runtime"), "Clicking disabled tools must keep the runtime page");
  noUnavailablePage();
  checks++;

  await focus(tab("runtime"));
  await nativeKey("Tab");
  check(document.activeElement === helpTrigger(), "Tab must reach the disabled tools explanation from the active runtime tab");
  await visibleTooltip(unavailable["zh-CN"]);
  await nativeKey("Enter");
  check(selectedTab() === tab("runtime"), "Enter on disabled tools help must not navigate");
  noUnavailablePage();
  checks++;
  await nativeKey("Escape");
  await hiddenTooltip();
  check(document.activeElement === helpTrigger(), "Escape must dismiss only the help bubble");
  checks++;
  await focus(tab("runtime"));
  await focus(helpTrigger());
  await visibleTooltip(unavailable["zh-CN"]);
  await focus(tab("runtime"));
  await hiddenTooltip();
  checks++;

  await act(async () => {
    helpTrigger().dispatchEvent(new PointerEvent("pointerover", { bubbles: true, pointerType: "mouse" }));
  });
  await visibleTooltip(unavailable["zh-CN"]);
  await act(async () => {
    helpTrigger().dispatchEvent(new PointerEvent("pointerout", { bubbles: true, pointerType: "mouse", relatedTarget: document.body }));
  });
  await hiddenTooltip();
  checks++;

  await focus(tab("runtime"));
  await act(async () => { tab("runtime").dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true })); });
  check(selectedTab() === tab("maintenance") && document.activeElement === tab("maintenance"),
    "ArrowLeft must skip disabled tools when wrapping from runtime");
  await act(async () => { tab("maintenance").dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true })); });
  check(selectedTab() === tab("runtime") && document.activeElement === tab("runtime"),
    "ArrowRight must skip disabled tools when wrapping to runtime");
  checks++;

  for (const moduleId of ["dontstarve", "arksurvivalascended"]) {
    await selectModule(moduleId, true);
    check(!tab("gm").disabled, `${moduleId} must expose tools`);
    await focus(tab("gm"));
    await nativeKey("Enter");
    check(selectedTab() === tab("gm"), `${moduleId} tools must be keyboard operable`);
    check(Boolean(fixture.querySelector(".gmt-workbench .gmt-nav-btn"))
      && Boolean(fixture.querySelector(".gmt-workbench form")), `${moduleId} must mount its real tools and form`);
    await selectModule("valheim");
    check(selectedTab() === tab("runtime"), "Changing from tools to an unsupported game must select runtime");
    noUnavailablePage();
    await act(async () => {
      helpTrigger().dispatchEvent(new PointerEvent("pointerover", { bubbles: true, pointerType: "mouse" }));
    });
    await visibleTooltip(unavailable["zh-CN"]);
    await selectModule(moduleId, true);
    check(selectedTab() === tab("gm"), "Returning to a supported game restores the requested Tools workspace");
    check(!document.querySelector(".server-detail-tab-help") && !document.querySelector(".configuration-field-help-tooltip"),
      "Switching to a supported game must unmount an open disabled-tools bubble");
    checks++;
  }

  let supportedModules = 0;
  let unsupportedModules = 0;
  for (const module of bootstrap.state.modules) {
    await selectModule(module.id);
    check(tab("gm").disabled === !supported.has(module.id), `${module.id} tools availability differs from the supported game list`);
    if (supported.has(module.id)) supportedModules++;
    else { unsupportedModules++; helpTrigger(); noUnavailablePage(); }
  }
  check(supportedModules === 3 && unsupportedModules === 29, "Availability matrix must cover all 32 game modules");
  checks++;

  // Native IPC is the only replaced boundary. Keep the real top-level tabs,
  // workbench, spawning form and validated API response in this regression.
  let spawnInput: ArkSpawnInput | null = null;
  let resolveSpawn: ((result: ArkSpawnResult) => void) | undefined;
  Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: (command: string, args: { input?: ArkSpawnInput }) => {
    if (command === "read_ark_tools_status") return Promise.resolve({ installed: true, connected: true, issue: null });
    if (command === "spawn_ark_creature" && args.input) {
      spawnInput = args.input;
      return new Promise<ArkSpawnResult>(resolve => { resolveSpawn = resolve; });
    }
    return invokeMock(command, args);
  } } });
  await selectModule("arksurvivalascended", true);
  await act(async () => {
    const selected = viewProps.selectedDetails!;
    viewProps.selectedDetails = { ...selected, summary: { ...selected.summary, status: "Running", active_process_count: 1 },
      settings_json: JSON.stringify({ rcon_enabled: true, admin_password: "fixture-admin-password" }),
      active_run: { run_id: 1, processes: [{ process_key: "main", status: "Running" }] } };
    render();
  });
  await act(async () => { tab("gm").click(); });
  await settleUntil(() => Boolean(fixture.querySelector(".ark-creature-spawner")), "ARK spawning did not mount");
  const spawner = element(".ark-creature-spawner");
  const spawnButton = () => element<HTMLButtonElement>('.ark-creature-spawner button[type="submit"]');
  for (const [index, value] of ["100", "200", "300"].entries()) {
    const field = spawner.querySelectorAll<HTMLInputElement>(".ark-creature-spawner__coordinates input")[index];
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(field, value);
      field.dispatchEvent(new Event("input", { bubbles: true }));
    });
  }
  await settleUntil(() => !spawnButton().disabled, "ARK connection did not enable spawning");
  await act(async () => { spawnButton().click(); });
  check(spawnInput && spawnButton().disabled, "Spawning must remain pending until native read-back"); checks++;
  await act(async () => { tab("runtime").click(); });
  check(fixture.querySelector(".ark-creature-spawner") === spawner && spawner.getClientRects().length === 0,
    "Leaving Tools must hide the existing spawning operation, not discard it"); checks++;
  await act(async () => { tab("gm").click(); });
  check(spawnButton().disabled, "Returning to Tools must retain the pending native operation"); checks++;
  await act(async () => { tab("runtime").click(); });
  const request = spawnInput as ArkSpawnInput;
  await act(async () => resolveSpawn!({ instanceId: request.instanceId, requestId: request.requestId, creature: {
    id1: 501, id2: 902, className: "Rex_Character_BP_C", level: request.level, team: 1,
    x: request.x, y: request.y, z: request.z, tamed: request.tamed
  } }));
  await act(async () => { tab("gm").click(); });
  check(element(".ark-creature-spawner__result").textContent?.includes("ID 501:902"),
    "A native result received on another top-level tab must remain visible on return"); checks++;
  await act(async () => {
    viewProps.selectedDetails = { ...viewProps.selectedDetails!, summary: { ...viewProps.selectedDetails!.summary, id: "ark-other-instance" } };
    viewProps.selectedInstanceId = "ark-other-instance";
    render();
  });
  check(!fixture.querySelector(".ark-creature-spawner__result"), "Switching instances must not carry the previous creature result"); checks++;
  Object.assign(window, { isTauri: false });
  viewProps.selectedInstanceId = instance.id;

  await selectModule("valheim");
  await focus(element(".shell-locale-button"));
  await nativeKey("Enter");
  await settleUntil(() => tab("gm").textContent?.trim() === "Tools", "English interface did not load");
  await focus(helpTrigger());
  await visibleTooltip(unavailable["en-US"]);
  noUnavailablePage();
  checks++;
  await focus(element(".shell-locale-button"));
  await hiddenTooltip();
  await nativeKey("Enter");
  await settleUntil(() => tab("gm").textContent?.trim() === "工具", "Chinese interface did not restore");
  await focus(helpTrigger());
  await visibleTooltip(unavailable["zh-CN"]);
  checks++;

  check(document.documentElement.scrollWidth <= innerWidth + 1, "Window must not overflow horizontally");
  assertFits(element(".server-detail-subheader"), "Server tab strip");
  assertFits(element(".server-detail-tabs"), "Server tabs");
  noUnavailablePage();
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  checks++;
  return { status: "passed", checks, browser_errors: errors, viewport: { width: innerWidth, height: innerHeight },
    supported_modules: supportedModules, unsupported_modules: unsupportedModules };
}

let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => {
  watchdog = setTimeout(() => reject(new Error(`Tools interaction stalled after ${checks} checks`)), 25000);
})]).finally(() => {
  clearTimeout(watchdog);
  // Assertions are complete; the browser owner now captures and closes the page.
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
}).catch((error) => ({ status: "failed", checks,
  error: `After ${checks} checks: ${error instanceof Error ? error.stack : String(error)}`, browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
