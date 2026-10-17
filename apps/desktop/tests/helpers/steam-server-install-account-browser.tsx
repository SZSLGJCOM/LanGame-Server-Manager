import React, { act, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { invokeMock } from "../../src/api-mock";
import { canUseLocalDesktopApi, shouldUseLanApi } from "../../src/api-transport";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import { createDefaultAiSettings } from "../../src/ai-settings";
import { useLibraryActions } from "../../src/hooks/useDesktopActions";
import { I18nProvider } from "../../src/i18n";
import type { SteamWorkshopAccountLoginSnapshot, SteamWorkshopAccountPhase, SteamWorkshopAccountTarget } from "../../src/steam-workshop-account-api";
import type { BootstrapResponse, InstanceDetails, ModuleInstallResult } from "../../src/types";
import { LibraryView } from "../../src/views/LibraryView";
import { ServersView } from "../../src/views/ServersView";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import "../../src/app.css";

// Only native IPC and application refresh callbacks use disposable fixture data.
// Installation actions, account ownership, navigation and credential forms are production code.
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!, root = createRoot(fixture);
const checks: string[] = [], errors: string[] = [], activity: string[] = [];
const calls: { command: string; target?: SteamWorkshopAccountTarget; instanceId?: string; moduleId?: string; validate?: boolean; accountName?: string }[] = [];
const credential = "Fixture_Server_Secret_Never_Record";
const tokenCode = "T1K2N", emailCode = "M4I7L";
let scenario: "direct" | "email" | "token" | "phone";
const passwordSessions = new Set<string>();
let starts = 0, cancels = 0, revocations = 0, mutations = 0;
let native: SteamWorkshopAccountLoginSnapshot | null = null;
let fault: "generic" | "network" | "access" = "generic", accountDenied = false, revokeFailed = false;
let revokeReply: (() => void) | null = null, holdRevoke = false;
let downloadReject: ((reason: Error) => void) | null = null, delayDownload = false;
let page: "library" | "servers" = "library", libraryInstalled = false, instanceInstalled = false;
let libraryProps: ComponentProps<typeof LibraryView>, serverProps: ComponentProps<typeof ServersView>;
const noOperation = () => {};
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
function check(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(`${label}: ${fixture.textContent}`);
  checks.push(label);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const found = fixture.querySelector<T>(selector); if (!found) throw new Error(`Missing ${selector}: ${fixture.textContent}`); return found;
}
async function settle(predicate: () => boolean, label: string) {
  const deadline = performance.now() + 6000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(`${label}: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(node: HTMLElement) {
  check(!(node instanceof HTMLButtonElement) || !node.disabled, "The selected operation is enabled");
  await act(async () => { node.scrollIntoView({ block: "nearest" }); node.click(); });
}
function same(a: SteamWorkshopAccountTarget, b: SteamWorkshopAccountTarget) { return a.kind === b.kind && a.id === b.id; }
async function enter(value: string) {
  const node = element<HTMLInputElement>(".server-steamcmd-console input");
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(node, value);
    node.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
async function submit() { await act(async () => { element<HTMLFormElement>(".server-steamcmd-console form").dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })); }); }
function advance(phase: SteamWorkshopAccountPhase, inputMode: "command" | "secret" | "disabled" = "disabled") {
  check(native, "A server login exists before native progress changes");
  native = { ...native, revision: native.revision + 1, phase, input_mode: inputMode, events: [...native.events, phase] };
}
async function status(phase: SteamWorkshopAccountPhase) {
  await settle(() => Boolean(fixture.querySelector(`.server-steamcmd-status[data-phase="${phase}"]`)), `Server ${phase} current status did not render`);
  const text = element(".server-steamcmd-status").textContent ?? "";
  check(text.trim().length > 0 && !text.includes("servers.mods.steamAccount"), `Server ${phase} status is localized`);
  const bounds = element(".server-steamcmd-status").getBoundingClientRect(), actions = element(".server-steamcmd-console .server-runtime-terminal-actions").getBoundingClientRect();
  check(bounds.right <= actions.left + 1 || bounds.bottom <= actions.top + 1 || bounds.top >= actions.bottom - 1,
    `Server ${phase} status never overlaps login controls`);
  if (["input_submitted", "preparing_steamcmd", "connecting", "authenticating", "verifying_cache"].includes(phase)) {
    check(element<HTMLInputElement>(".server-steamcmd-console input").disabled, `Server ${phase} accepts no credential input`);
  }
}
async function reachNativeChallenge() {
  await enter("fixture_account"); await submit();
  await status("input_submitted");
  advance("preparing_steamcmd"); await status("preparing_steamcmd");
  advance("connecting"); await status("connecting");
  check(!fixture.querySelector(".server-steamcmd-auto-verify"), "Steam determines the verification method without an extra action");
  if (scenario === "direct") {
    check(!native!.events.includes("password") && !native!.events.includes("guard_code") && !native!.events.includes("email_code"),
      "A valid cached login proceeds without inventing a password or code prompt");
    return;
  }
  advance("password", "secret"); await status("password");
  check(element<HTMLInputElement>(".server-steamcmd-console input").type === "password", "A fresh server login requests its password privately");
  await enter(credential); await submit(); await status("connecting");
  advance(scenario === "email" ? "email_code" : "guard_code", "secret");
  await status(scenario === "email" ? "email_code" : "guard_code");
}
async function authenticate() {
  await settle(() => Boolean(fixture.querySelector(".server-steamcmd-console input:not(:disabled)")), "The login prompt did not open");
  await reachNativeChallenge();
  if (scenario === "phone") {
    await enter(tokenCode);
    advance("authenticating"); await status("authenticating");
    check(element<HTMLInputElement>(".server-steamcmd-console input").value === "", "External phone approval clears an unsubmitted token without any credential replay");
  } else if (scenario !== "direct") {
    const input = element<HTMLInputElement>(".server-steamcmd-console input");
    check(input.type === "password" && input.autocomplete === "off", "The actual email or token challenge uses hidden input without browser completion");
    await enter(scenario === "email" ? emailCode : tokenCode); await submit(); await status("connecting");
  }
  advance("authenticating"); await status("authenticating");
  advance("verifying_cache"); await status("verifying_cache");
  advance("authenticated"); native = { ...native!, account_name: "Confirmed_Account", steamcmd_root: "C:\\Fixture\\SteamCMD" };
  noSecrets();
}
async function tab(id: string) { await click(element(`.server-detail-tabs [id$="-${id}"]`)); }
async function steamConsole() {
  await tab("runtime");
  if (!fixture.querySelector(".server-steamcmd-console")) await act(async () => {
    const source = element<HTMLSelectElement>(".server-steamcmd-source"); source.value = "steamcmd"; source.dispatchEvent(new Event("change", { bubbles: true }));
  });
}
async function anonymous() {
  await steamConsole();
  await act(async () => { const select = element<HTMLSelectElement>(".server-steamcmd-identity"); select.value = ""; select.dispatchEvent(new Event("change", { bubbles: true })); });
}
async function maintenance() {
  await tab("maintenance");
  await settle(() => Boolean(fixture.querySelector('.maintenance-workspace [data-maintenance-section="storage"]')), "Maintenance did not load");
  const storage = fixture.querySelector<HTMLElement>('.maintenance-workspace [data-maintenance-section="storage"]')!;
  if (storage.hidden) {
    await click(element('.maintenance-workspace [data-configuration-section-id="storage"] > button'));
  }
  await settle(() => Boolean(storage.querySelector(".instance-isolation-content .inline-confirm-action")), "Program update actions did not load");
}
async function update(validate = false) {
  await maintenance();
  const buttons = fixture.querySelectorAll<HTMLButtonElement>('.instance-isolation-content .button-row > .inline-confirm-action > button');
  await click(buttons[validate ? 1 : 0]);
  await click(element(".instance-isolation-content .inline-confirm-submit"));
}
function noSecrets() {
  const recorded = [fixture.textContent ?? "", ...activity, ...errors, ...Object.keys(localStorage).map((key) => localStorage.getItem(key) ?? ""),
    ...Object.keys(sessionStorage).map((key) => sessionStorage.getItem(key) ?? "")].join("\n");
  check([credential, tokenCode, emailCode].every((secret) => !recorded.includes(secret)), "No server password or code enters logs, activity, storage or visible output");
}
async function run() {
  const options = await (await fetch("/__steam_server_account_locale")).json() as { locale: string; captureLibrary?: boolean; scenario: typeof scenario };
  const { locale, captureLibrary } = options; scenario = options.scenario;
  localStorage.setItem("langame.locale", locale);
  const bootstrap = await invokeMock<BootstrapResponse>("bootstrap", { includeSystemSnapshot: false });
  const source = bootstrap.state.instances[0], original = await invokeMock<InstanceDetails>("read_instance_details_from_storage", { instanceId: source.id });
  const first: InstanceDetails = { ...original, summary: { ...source, id: "fixture-steam-server-instance", module_id: "valheim", name: "Fixture Steam server", status: "Stopped", active_process_count: 0 }, active_run: null, settings_json: "{}" };
  const second: InstanceDetails = { ...first, summary: { ...first.summary, id: "fixture-minecraft-instance", module_id: "minecraft", name: "Fixture Minecraft" } };
  const moduleDetails = buildMockModuleDetails("valheim");
  const libraryDetails = { ...moduleDetails, summary: { ...moduleDetails.summary, id: "fixture-steam-library", name: "Fixture Steam library", install_state: "NotInstalled" } };
  const access = new Error(JSON.stringify({ code: "steam_app_account_required", message: "Fixture verified server access failure", app_id: 896660, output_excerpt: "Fixture server download: No subscription" }));
  const result = (moduleId: string, validate: boolean): ModuleInstallResult => ({ module_id: moduleId, steam_app_id: 896660,
    operation: validate ? "validate" : "install", install_root: "C:\\Fixture\\Program", executable_path: "C:\\Fixture\\Program\\server.exe",
    executable_exists: true, install_state: "Installed" });
  mockWindows("steam-server-account");
  mockIPC(async (command, payload) => {
    const args = (payload ?? {}) as Record<string, unknown>;
    if (command === "start_steam_workshop_account_login") {
      const target = args.target as SteamWorkshopAccountTarget;
      check(Object.keys(args).sort().join() === "sessionId,target" && ["instance", "module"].includes(target.kind), "Login sends a real instance or module scope without invented instance IDs");
      starts++; native = { session_id: String(args.sessionId), target, revision: 1, accepted_input_revision: null, phase: "ready", input_mode: "command", events: ["ready"], account_name: null, steamcmd_root: null, error: null };
      return structuredClone(native);
    }
    if (["read_steam_workshop_account_login", "send_steam_workshop_account_input", "cancel_steam_workshop_account_login"].includes(command)) {
      check(native && same(args.target as SteamWorkshopAccountTarget, native.target) && args.sessionId === native.session_id, "Session commands retain their exact login target");
      if (command === "cancel_steam_workshop_account_login") { cancels++; native = { ...native, revision: native.revision + 1, phase: "cancelled", input_mode: "disabled", events: [...native.events, "cancelled"] }; }
      if (command === "send_steam_workshop_account_input") {
        check(args.expectedRevision === native.revision, "Credential input owns the exact prompt revision");
        const phase = native.phase;
        check(phase === "ready" ? args.input === "fixture_account" : phase === "password" ? args.input === credential
          : phase === "email_code" ? args.input === emailCode : phase === "guard_code" && args.input === tokenCode,
          "The dedicated credential command accepts only the current native account, password or actual challenge");
        if (phase === "password") {
          check(!passwordSessions.has(native.session_id), "The frontend never resends a password within one login session"); passwordSessions.add(native.session_id);
        }
        const next = phase === "ready" ? "input_submitted" : "connecting";
        native = { ...native, revision: native.revision + 1, accepted_input_revision: Number(args.expectedRevision), phase: next,
          input_mode: "disabled", events: [...native.events, next], account_name: null, steamcmd_root: null };
        if (phase === "email_code" || phase === "guard_code") throw new Error("Synthetic accepted challenge response lost");
      }
      return structuredClone(native);
    }
    if (command === "clear_steam_server_account_authorization") {
      check(Object.keys(args).join() === "target", "Anonymous selection revokes only its target authorization without credentials"); revocations++;
      if (revokeFailed) { revokeFailed = false; throw new Error(JSON.stringify({ code: "steamcmd_not_ready", message: "SteamCMD is not ready.", path: "fixture" })); }
      if (holdRevoke) { holdRevoke = false; await new Promise<void>((resolve) => { revokeReply = resolve; }); }
      return;
    }
    if (["install_module_game", "validate_module_game", "update_instance_program", "install_authenticated_module_game", "update_authenticated_instance_program"].includes(command)) {
      const authenticated = command.includes("authenticated"), validate = command === "validate_module_game" || args.validate === true;
      calls.push({ command, instanceId: args.instanceId as string | undefined, moduleId: args.moduleId as string | undefined, validate,
        accountName: args.accountName as string | undefined, target: args.target as SteamWorkshopAccountTarget | undefined });
      if (authenticated) {
        check(args.accountName === "Confirmed_Account", "Server downloads pass only the successfully authenticated account name");
        if (command === "install_authenticated_module_game") check(native && same(args.target as SteamWorkshopAccountTarget, native.target), "A resumed shared installation stays bound to its original login target");
        if (accountDenied) throw access;
        if (args.moduleId === libraryDetails.summary.id) libraryInstalled = true; else instanceInstalled = true;
        return result(String(args.moduleId ?? "valheim"), validate);
      }
      if (delayDownload) { delayDownload = false; return new Promise<never>((_, reject) => { downloadReject = reject; }); }
      if (fault === "access") throw access;
      throw new Error(fault === "network" ? "Fixture connection reset" : "Fixture generic Failure");
    }
    if (command === "inspect_module_programs") return { requires_archive_inventory: false, installations: libraryInstalled ? [{ id: 1, install_root: "fixture/library", scope: "library", install_state: "Installed", current_version: "fixture", used_by: [], modification_state: "unverified", pending_removal: false, size_bytes: 100 }] : [],
      creation: { can_create: libraryInstalled, action: "existing_install", program_path: "fixture/library", additional_bytes: 0, reason: null } };
    if (command === "read_instance_isolation") return { instance_id: (args.input as { instance_id: string }).instance_id, mode: "independent", runtime_path: "fixture/program", data_path: "fixture/data", config_path: "fixture/config", saves_path: "fixture/saves", conflicts: [], issues: [] };
    if (command === "read_background_jobs") return [];
    if (command === "list_instance_archives") return { archives: [], pending_deletions: [], issues: [] };
    if (command === "read_instance_log_document_from_storage") return { source_path: null, lines: [], truncated: false, read_error: null };
    return invokeMock(command, args);
  }, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  sessionStorage.setItem("langameLanToken", "Fixture_Historical_Lan_Token");
  check(shouldUseLanApi() && canUseLocalDesktopApi(), "Historical browser credentials retain native server account login and download admission");
  const forbidden = (): never => { mutations++; throw new Error("Unexpected instance mutation in isolated installation fixture"); };
  serverProps = { aiSettings: createDefaultAiSettings(), assistantCanRun: false, bindAddressCandidates: [], instances: [first.summary, second.summary],
    moduleInstallations: { valheim: { installState: "NotInstalled", hasManagedInstallSource: true } }, instanceLaunchPlans: { [first.summary.id]: { ...await invokeMock("preview_instance_launch", { instanceId: source.id }), instance_id: first.summary.id, install_state: "NotInstalled", uses_private_runtime: false } } as ComponentProps<typeof ServersView>["instanceLaunchPlans"], instanceLaunchFailures: {},
    section: "overview", onWorkspaceSectionChange: noOperation, selectedInstanceId: second.summary.id, selectedDetails: second, selectedBackups: [], selectedModuleDetails: buildMockModuleDetails("minecraft"), selectedModuleDetailsError: null, runtime: null, runtimeWindows: null, launchPlan: null, launchPlanError: null, refreshIssue: null,
    onActivity: (value) => activity.push(JSON.stringify(value)), onResumeAutoRefresh: noOperation, onSelectInstance: (id) => { serverProps.selectedInstanceId = id; serverProps.selectedDetails = id === first.summary.id ? first : second; serverProps.selectedModuleDetails = buildMockModuleDetails(serverProps.selectedDetails.summary.module_id); draw(); },
    onStart: forbidden, onStop: forbidden, onInstallModule: async () => {}, onOpenModuleLibrary: noOperation, onCreateInstance: forbidden, onPickDirectory: async () => null, onImportDontStarveWorldData: async () => forbidden(), onOpenLocalPath: noOperation,
    onSendRuntimeCommand: async () => forbidden(), onSuppressRuntimeWindows: async () => forbidden(), onCreateBackup: forbidden, onDeleteInstance: forbidden, onArchiveInstance: forbidden, onRestoreBackup: forbidden, onRenameBackup: async () => forbidden(), onDeleteBackup: forbidden,
    onSaveSettings: async () => forbidden(), onSaveAutostart: async () => forbidden(), onApplyPlayerAccessMutation: async () => forbidden(), onArchivesChanged: async () => {} };
  libraryProps = { mode: "detail", modules: [libraryDetails.summary], instances: [], selectedModuleId: libraryDetails.summary.id, selectedModuleDetails: libraryDetails, steamCmdStatus: { ready: true, executable_exists: true } as ComponentProps<typeof LibraryView>["steamCmdStatus"], steamCmdBusy: false,
    search: "", catalogFocusId: libraryDetails.summary.id, catalogScrollLeft: 0, jobs: [], onSearchChange: noOperation, onCatalogFocusChange: noOperation, onCatalogScrollLeftChange: noOperation, onOpenModule: noOperation, onBackToCatalog: noOperation,
    onInstall: noOperation, onUninstall: noOperation, onCreateServer: async () => forbidden(), creatingModuleIds: new Set() };
  function Harness() {
    const actions = useLibraryActions({ setActivity: (value) => { activity.push(JSON.stringify(value)); }, setBootstrap: noOperation, setSelectedModuleId: noOperation, setSelectedInstanceId: noOperation,
      reloadBootstrap: async () => { libraryDetails.summary.install_state = libraryInstalled ? "Installed" : "NotInstalled"; serverProps.moduleInstallations.valheim!.installState = instanceInstalled ? "Installed" : "NotInstalled"; draw(); return bootstrap; },
      refreshInstallationState: async () => ({ modules: [libraryDetails.summary], instanceId: null, preview: { launchPlan: null, launchPlanError: null } }), setLibraryTaskPolling: noOperation, refreshSteamCmdStatus: async () => {},
      setSteamCmdBusy: noOperation, setSteamCmdMessage: noOperation, setSteamCmdStatus: noOperation, setSteamCmdProgress: noOperation });
    return page === "library" ? <LibraryView {...libraryProps} onInstall={actions.handleInstallModule} /> : <ServersView {...serverProps} onInstallModule={actions.handleInstallModule} />;
  }
  function draw() { root.render(<I18nProvider><InstanceSettingsSaveProvider><Harness /></InstanceSettingsSaveProvider></I18nProvider>); }
  await act(async () => { await prepareBrowserLocaleCatalogs(); draw(); });
  await settle(() => Boolean(fixture.querySelector(".library-install-state-button:not(:disabled)")), "The library install control did not become ready");
  await click(element(".library-install-state-button"));
  await settle(() => calls.length === 1 && !element<HTMLButtonElement>(".library-install-state-button").disabled, "Generic library failure did not settle");
  fault = "network"; await click(element(".library-install-state-button"));
  await settle(() => calls.length === 2 && !element<HTMLButtonElement>(".library-install-state-button").disabled, "Network library failure did not settle");
  check(starts === 0 && !fixture.querySelector(".server-steamcmd-console"), "Generic and network library failures never request account login");
  fault = "access"; await click(element(".library-install-state-button"));
  await settle(() => native?.target.kind === "module" && Boolean(fixture.querySelector(".server-steamcmd-console")), "First-time library installation did not open its module LanGameCMD");
  check(!fixture.querySelector(".server-steamcmd-source") && libraryProps.instances?.length === 0, "A library login does not invent or bind an unrelated instance");
  check(element(".server-steamcmd-console .server-runtime-console-title").textContent?.includes(libraryDetails.summary.name), "The library console identifies the module owning this login");
  const inputBounds = element<HTMLInputElement>(".server-steamcmd-console input").getBoundingClientRect();
  check(inputBounds.width > 0 && inputBounds.left >= 0 && inputBounds.right <= innerWidth + 1
    && inputBounds.top >= 0 && inputBounds.bottom <= innerHeight + 1 && document.documentElement.scrollWidth <= innerWidth + 1,
    "The first-download module console and input remain visible without horizontal overflow");
  if (captureLibrary) {
    await reachNativeChallenge();
    const inputBounds = element<HTMLInputElement>(".server-steamcmd-console input").getBoundingClientRect();
    check(inputBounds.left >= 0 && inputBounds.right <= innerWidth + 1 && inputBounds.bottom <= innerHeight + 1
      && document.documentElement.scrollWidth <= innerWidth + 1, "The module native challenge stays visible without horizontal overflow");
    check(!fixture.querySelector(".server-steamcmd-auto-verify"), "The first-install console has no verification-method button");
    return { status: "passed", locale, checks, scope: "module", library_no_instance: true, native_token_precedence: true,
      challenge_scenario: scenario, cached_login: scenario === "direct", automatic_challenge: true, current_status: true, browser_errors: errors };
  }
  await authenticate();
  await settle(() => libraryInstalled && Boolean(fixture.querySelector(".library-install-state-button")) && activity.some((value) => value.includes("activity.installCompleted")), "Module authentication did not resume the original library installation");
  check(calls[3].command === "install_authenticated_module_game" && calls[3].moduleId === libraryDetails.summary.id && calls[3].validate === false, "Library login continues the exact original first installation once");
  accountDenied = true;
  await click(element(".library-maintenance-icon[aria-label]"));
  await settle(() => calls.length === 5 && activity.at(-1)?.includes("activity.validationFailed") === true, "Authenticated library update rejection did not settle");
  check(starts === 1 && calls[4].validate === true, "An authenticated library access failure never loops into login"); accountDenied = false;
  libraryProps.mode = "catalog"; await act(async () => { draw(); }); libraryProps.mode = "detail"; await act(async () => { draw(); });
  await settle(() => Boolean(fixture.querySelector(".library-maintenance-icon:not(:disabled)")), "The fresh module scope did not return to anonymous");
  await click(element(".library-maintenance-icon[aria-label]"));
  await settle(() => native?.phase === "ready" && Boolean(fixture.querySelector(".server-steamcmd-console")), "Module validation login did not start");
  await click(element(".server-steamcmd-console .server-runtime-terminal-actions button"));
  await settle(() => Boolean(fixture.querySelector(".library-maintenance-icon:not(:disabled)")), "Cancelling module login did not release its original update");
  check(calls.length === 6 && cancels === 1, "Cancelled module recovery does not retry the original validation");
  await click(element(".library-maintenance-icon[aria-label]"));
  await settle(() => native?.phase === "ready" && Boolean(fixture.querySelector(".server-steamcmd-console")), "A second module login did not start");
  libraryProps.mode = "catalog"; await act(async () => { draw(); });
  await settle(() => cancels === 2, "Leaving the module detail did not cancel its native login");
  libraryProps.mode = "detail"; await act(async () => { draw(); });
  await settle(() => Boolean(fixture.querySelector(".library-maintenance-icon:not(:disabled)")), "The module detail did not recover after cancellation");
  delayDownload = true; await click(element(".library-maintenance-icon[aria-label]"));
  await settle(() => Boolean(downloadReject), "The delayed module validation did not become pending");
  libraryProps.mode = "catalog"; await act(async () => { draw(); }); libraryProps.mode = "detail"; await act(async () => { draw(); });
  await act(async () => { downloadReject!(access); downloadReject = null; });
  check(starts === 3 && !fixture.querySelector(".server-steamcmd-console"), "Returning to the same module cannot reactivate a cancelled download owner");
  page = "servers"; await act(async () => { draw(); });
  await settle(() => Boolean(fixture.querySelector(`.server-list-card[data-card-id="${first.summary.id}"] .server-list-card-primary-action`)), "Instance installation card did not load");
  await click(element(`.server-list-card[data-card-id="${first.summary.id}"] .server-list-card-primary-action`));
  await settle(() => native?.target.kind === "instance" && native.target.id === first.summary.id && Boolean(fixture.querySelector(".server-steamcmd-console")), "An unselected instance install did not recover in that instance's LanGameCMD");
  check(serverProps.selectedInstanceId === first.summary.id && moduleDetails.workshop?.provider !== "steam", "Server login supports an actual Steam install without Steam Workshop");
  await authenticate();
  await settle(() => instanceInstalled && calls.length === 10, "Instance authentication did not resume shared installation");
  check(calls[9].target?.kind === "instance" && calls[9].target?.id === first.summary.id, "Instance card installation binds the account to its own instance target");
  revokeFailed = true; await anonymous();
  await settle(() => Boolean(fixture.querySelector(".server-runtime-console-errors")), "Authorization revocation failure did not render");
  check(element<HTMLSelectElement>(".server-steamcmd-identity").value === "Confirmed_Account", "A failed authorization revocation does not falsely select anonymous");
  holdRevoke = true; await anonymous();
  await settle(() => Boolean(revokeReply), "The authorization revocation was not in flight");
  check(element<HTMLSelectElement>(".server-steamcmd-identity").disabled && element<HTMLSelectElement>(".server-steamcmd-identity").value === "Confirmed_Account", "Anonymous is selected only after revocation is confirmed");
  await act(async () => { revokeReply!(); revokeReply = null; });
  await settle(() => element<HTMLSelectElement>(".server-steamcmd-identity").value === "", "Confirmed authorization revocation did not restore anonymous");
  await update(false); await authenticate();
  await settle(() => calls.length === 12 && element('.server-detail-tabs [id$="-maintenance"]').getAttribute("aria-selected") === "true", "Program update did not return to its original maintenance operation");
  check(calls[11].command === "update_authenticated_instance_program" && calls[11].instanceId === first.summary.id && calls[11].validate === false, "Maintenance update retries its exact original instance and mode once");
  accountDenied = true; await update(true);
  await settle(() => calls.length === 13, "Authenticated validation did not execute");
  check(starts === 5 && calls[12].command === "update_authenticated_instance_program" && calls[12].validate === true, "Authenticated instance validation does not reopen login"); accountDenied = false;
  await anonymous(); await settle(() => element<HTMLSelectElement>(".server-steamcmd-identity").value === "", "Anonymous could not be restored for cancellation");
  await update(false); await settle(() => native?.phase === "ready" && Boolean(fixture.querySelector(".server-steamcmd-console")), "Cancellable update login did not start");
  await click(element(".server-steamcmd-console .server-runtime-terminal-actions button"));
  await maintenance();
  await settle(() => !element<HTMLButtonElement>(".instance-isolation-content .button-row button").disabled, "Cancelling login did not release the program update control");
  check(calls.length === 14, "Cancelling authentication does not retry the original program update");
  await update(false); await settle(() => native?.phase === "ready" && Boolean(fixture.querySelector(".server-steamcmd-console")), "Instance-change login did not start");
  await click(element(`.server-list-card[data-card-id="${second.summary.id}"] .server-list-card-hitarea`));
  await settle(() => cancels === 4, "Changing instances did not cancel the pending server login");
  await click(element(`.server-list-card[data-card-id="${first.summary.id}"] .server-list-card-hitarea`));
  delayDownload = true; await update(false); await settle(() => Boolean(downloadReject), "The delayed server update did not start");
  await click(element(`.server-list-card[data-card-id="${second.summary.id}"] .server-list-card-hitarea`));
  await click(element(`.server-list-card[data-card-id="${first.summary.id}"] .server-list-card-hitarea`));
  await act(async () => { downloadReject!(access); downloadReject = null; });
  check(starts === 7 && !fixture.querySelector(".server-steamcmd-console"), "A late server update permission error cannot reactivate an old owner after A-B-A selection");
  await update(false); await settle(() => Boolean(fixture.querySelector(".server-steamcmd-console")), "Final server recovery panel did not render");
  noSecrets();
  check(mutations === 0 && first.summary.status === "Stopped" && errors.length === 0, "Installation recovery never starts the server, mutates instance settings or emits browser errors");
  return { status: "passed", locale, checks, calls, starts, cancels, revocations, mutations, browser_errors: errors, native_token_precedence: true,
    library_no_instance: true, challenge_scenario: scenario, cached_login: scenario === "direct", automatic_challenge: true, password_not_replayed: true, current_status: true, instance_install: true, maintenance_update: true, no_loop: true, cancellation: true, stale_owner: true, anonymous_revocation: true, secrets_absent: true };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error("Server account regression exceeded 45 seconds")), 45000); })])
  .catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`, { method: "POST", body: JSON.stringify(report) }));
