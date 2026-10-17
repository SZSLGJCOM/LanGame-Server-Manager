import React, { act, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { invokeMock } from "../../src/api-mock";
import { canUseLocalDesktopApi, shouldUseLanApi } from "../../src/api-transport";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import { createDefaultAiSettings } from "../../src/ai-settings";
import { I18nProvider } from "../../src/i18n";
import { runtimeConsoleHistory } from "../../src/runtime-console-history";
import type { SteamWorkshopAccountLoginSnapshot, SteamWorkshopAccountPhase } from "../../src/steam-workshop-account-api";
import type { BootstrapResponse, InstanceDetails, ManualModInventoryResult } from "../../src/types";
import { ServersView } from "../../src/views/ServersView";
import { SteamWorkshopAccountConsole } from "../../src/views/servers/SteamWorkshopAccountConsole";
import { useSteamWorkshopAccount } from "../../src/views/servers/useSteamWorkshopAccount";
import { InstanceSettingsSaveProvider } from "../../src/views/settings/InstanceSettingsSaveContext";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import { createModWorkbenchFixtureIpc, workshopItem } from "./mod-workbench-browser-support";
import "../../src/app.css";

// Native IPC alone is replaced. The production instance navigation, Mod workspace,
// session owner and LanGameCMD render unchanged; all credentials are synthetic.
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture), checks: string[] = [], errors: string[] = [], activity: string[] = [], history: string[] = [];
const downloads: { command: string; accountName?: string }[] = [];
const credentials = ["Fixture_Password_Never_Record", "D3E4F", "A1B2C", "M4I7L"];
let starts = 0, cancellations = 0, writes = 0, serverCommands = 0;
let native: SteamWorkshopAccountLoginSnapshot | null = null, props: ComponentProps<typeof ServersView>;
let credentialReply: (() => void) | null = null;
const inputOrder: SteamWorkshopAccountPhase[] = [];
let accountPlaceholder = "";
let startFailure = false, cancelFailure = false, readFailure: "notReady" | "unavailable" | null = null;
let delayDownload = false, rejectDownload: ((reason: Error) => void) | null = null;
let boundaryProbe = false, boundaryInputCalls = 0;
let boundaryAccount: ReturnType<typeof useSteamWorkshopAccount> | null = null;
function PromptBoundaryHost({ instanceId }: { instanceId: string }) {
  boundaryAccount = useSteamWorkshopAccount({ kind: "instance", id: instanceId });
  return <SteamWorkshopAccountConsole account={boundaryAccount} />;
}
const originalHistoryWrite = runtimeConsoleHistory.write;
runtimeConsoleHistory.write = (scope, snapshot) => { history.push(JSON.stringify(snapshot)); originalHistoryWrite(scope, snapshot); };
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(`${label}: ${fixture.textContent}`);
  checks.push(label);
}
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const found = fixture.querySelector<T>(selector);
  if (!found) throw new Error(`Missing ${selector}: ${fixture.textContent}`);
  return found;
}
async function settle(predicate: () => boolean, label: string) {
  const deadline = performance.now() + 6000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(`${label}: ${fixture.textContent}`);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(node: HTMLElement) {
  assert(!(node instanceof HTMLButtonElement) || !node.disabled, "The selected control is enabled");
  await act(async () => { node.scrollIntoView({ block: "nearest" }); node.click(); });
}
function draw() { root.render(<I18nProvider><InstanceSettingsSaveProvider><ServersView {...props} /></InstanceSettingsSaveProvider></I18nProvider>); }
async function tab(id: string) {
  await settle(() => Boolean(fixture.querySelector(`.server-detail-tabs [id$="-${id}"]`)), "Instance detail navigation did not load");
  await click(element(`.server-detail-tabs [id$="-${id}"]`));
  await settle(() => Boolean(fixture.querySelector(id === "mods" ? ".mw-workbench" : ".server-runtime-console")), "The selected production workspace did not load");
}
function terminalInput() { return element<HTMLInputElement>(".server-steamcmd-console input"); }
async function enter(value: string) {
  const node = terminalInput();
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(node, value);
    node.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
async function submit() {
  await act(async () => { element<HTMLFormElement>(".server-steamcmd-console form").dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })); });
}
async function goSteamConsole() {
  await tab("runtime");
  if (!fixture.querySelector(".server-steamcmd-console")) await act(async () => {
    const source = element<HTMLSelectElement>(".server-steamcmd-source"); source.value = "steamcmd"; source.dispatchEvent(new Event("change", { bubbles: true }));
  });
  await settle(() => Boolean(fixture.querySelector(".server-steamcmd-console")), "SteamCMD source did not open");
}
async function startLogin() {
  await goSteamConsole(); await click(element(".server-steamcmd-console .server-runtime-terminal-actions button"));
  await settle(() => !terminalInput().disabled, "The console login did not become ready");
}
function advance(phase: SteamWorkshopAccountPhase, mode: "command" | "secret" | "disabled") {
  assert(native, "A native session exists before a prompt changes");
  native = { ...native, revision: native.revision + 1, phase, input_mode: mode, events: [...native.events, phase] };
}
async function status(phase: SteamWorkshopAccountPhase) {
  await settle(() => Boolean(fixture.querySelector(`.server-steamcmd-status[data-phase="${phase}"]`)), `Current ${phase} status did not render`);
  const text = element(".server-steamcmd-status").textContent ?? "";
  assert(text.trim().length > 0 && !text.includes("servers.mods.steamAccount"), `Current ${phase} status is localized`);
  const bounds = element(".server-steamcmd-status").getBoundingClientRect(), actions = element(".server-steamcmd-console .server-runtime-terminal-actions").getBoundingClientRect();
  assert(bounds.right <= actions.left + 1 || bounds.bottom <= actions.top + 1 || bounds.top >= actions.bottom - 1,
    `Current ${phase} status does not overlap identity or cancellation controls`);
  if (["input_submitted", "preparing_steamcmd", "connecting", "authenticating", "verifying_cache"].includes(phase)) {
    assert(terminalInput().disabled && terminalInput().value === "", `${phase} has no enabled credential input or retained draft`);
  }
}
function noSecretEvidence() {
  const recorded = [fixture.textContent ?? "", ...activity, ...history, ...errors,
    ...Object.keys(localStorage).map((key) => localStorage.getItem(key) ?? ""),
    ...Object.keys(sessionStorage).map((key) => sessionStorage.getItem(key) ?? "")].join("\n");
  assert(credentials.every((secret) => !recorded.includes(secret)), "Passwords and Guard codes never enter terminal output, activity, history, storage or errors");
}
async function run() {
  const { locale, scenario } = await (await fetch("/__steam_workshop_account_locale")).json() as { locale: string; scenario: "direct" | "email" | "token" | "phone" };
  localStorage.setItem("langame.locale", locale);
  const bootstrap = await invokeMock<BootstrapResponse>("bootstrap", { includeSystemSnapshot: false });
  const source = bootstrap.state.instances[0];
  assert(source, "The repository supplies a complete isolated instance template");
  const original = await invokeMock<InstanceDetails>("read_instance_details_from_storage", { instanceId: source.id });
  const first: InstanceDetails = { ...original, summary: { ...source, module_id: "palworld", name: "SteamCMD fixture instance", status: "Stopped", active_process_count: 0 }, active_run: null, settings_json: "{}" };
  const second: InstanceDetails = { ...first, summary: { ...first.summary, id: `${first.summary.id}-other`, module_id: "minecraft", name: "Other fixture instance" } };
  const details = new Map([[first.summary.id, first], [second.summary.id, second]]);
  const moduleDetails = buildMockModuleDetails("palworld"), cached = new Set<string>();
  const catalog = ["3816899969", "3816899970"].map((id) => ({ ...workshopItem(id, `Workshop fixture Mod ${id}`), consumer_app_id: 1623730 }));
  let anonFault: "generic" | "network" | "permission" = "generic", deployed = false, secretSubmissions = 0, inputAttempts = 0;
  const accessError = JSON.stringify({ code: innerWidth < 1000 ? "steam_workshop_account_required" : "steam_workshop_decryption_key_missing",
    message: "Verified fixture account access failure", consumer_app_id: 1623730, output_excerpt: "Fixture: verified account access required" });
  const inventory = (): ManualModInventoryResult => ({ instance_id: props.selectedInstanceId!, module_id: "palworld", source_label: "Fixture", target_label: "Mods",
    target_path: "C:\\Fixture\\Mods", target_exists: true, items: deployed ? [{ name: "3816899969", path: "C:\\Fixture\\Mods\\3816899969",
      item_type: "directory", inferred_id: "FixturePackage", file_count: 1, total_bytes: 100 }] : [] });
  const noOperation = () => {};
  const forbidden = (): never => { writes++; throw new Error("Unexpected instance mutation in credential fixture"); };
  props = { aiSettings: createDefaultAiSettings(), assistantCanRun: false, bindAddressCandidates: [], instances: [first.summary, second.summary],
    moduleInstallations: { palworld: { installState: "Installed", hasManagedInstallSource: true } }, instanceLaunchPlans: {}, instanceLaunchFailures: {},
    section: "overview", onWorkspaceSectionChange: (section) => { props.section = section; draw(); }, selectedInstanceId: first.summary.id,
    selectedDetails: first, selectedBackups: [], selectedModuleDetails: moduleDetails, selectedModuleDetailsError: null, runtime: null, runtimeWindows: null,
    launchPlan: null, launchPlanError: null, refreshIssue: null, onActivity: (value) => activity.push(JSON.stringify(value)), onResumeAutoRefresh: noOperation,
    onSelectInstance: (id) => { props.selectedInstanceId = id; props.selectedDetails = details.get(id)!;
      props.selectedModuleDetails = buildMockModuleDetails(props.selectedDetails.summary.module_id); draw(); },
    onStart: forbidden, onStop: forbidden, onInstallModule: async () => forbidden(), onOpenModuleLibrary: noOperation, onCreateInstance: forbidden,
    onPickDirectory: async () => null, onImportDontStarveWorldData: async () => forbidden(), onOpenLocalPath: noOperation,
    onSendRuntimeCommand: async () => { serverCommands++; return null; }, onSuppressRuntimeWindows: async () => forbidden(), onCreateBackup: forbidden,
    onDeleteInstance: forbidden, onArchiveInstance: forbidden, onRestoreBackup: forbidden, onRenameBackup: async () => forbidden(), onDeleteBackup: forbidden,
    onSaveSettings: async (input) => {
      assert(input.id === first.summary.id, "The resumed installation saves only its original instance");
      first.settings_json = input.settings_json; props.selectedDetails = { ...first }; writes++; draw(); return props.selectedDetails;
    }, onSaveAutostart: async () => forbidden(), onApplyPlayerAccessMutation: async () => forbidden(), onArchivesChanged: async () => {} };
  const base = createModWorkbenchFixtureIpc(() => ({ details: props.selectedDetails!, moduleDetails, catalog, cached, inventory: inventory() }), {
    check: assert, save: () => forbidden(), download: () => {
      downloads.push({ command: "anonymous" });
      if (anonFault === "permission") throw new Error(accessError);
      throw new Error(JSON.stringify(anonFault === "network" ? { code: "steam_workshop_network_failed", message: "Fixture network failure",
        stage: "details", reason: "connection", origin: "https://steamcommunity.com" } : { code: "steamcmd_command_failed", message: "Fixture generic failure", output_excerpt: "Failure" }));
    }
  });
  mockWindows("steam-workshop-account");
  const notReady = JSON.stringify({ code: "steamcmd_not_ready", message: "SteamCMD is not ready.", path: "C:\\Fixture\\SteamCMD" });
  mockIPC(async (command, payload) => {
    const args = (payload ?? {}) as Record<string, unknown>;
    if (boundaryProbe && command.endsWith("steam_workshop_account_login")) {
      if (command === "start_steam_workshop_account_login") native = { session_id: String(args.sessionId), target: { kind: "instance", id: first.summary.id },
        revision: 1, accepted_input_revision: null, phase: "ready", input_mode: "command", events: ["ready"], account_name: null, steamcmd_root: null, error: null };
      return structuredClone(native);
    }
    if (boundaryProbe && command === "send_steam_workshop_account_input") {
      assert(native, "The isolated prompt-boundary probe has a native snapshot"); boundaryInputCalls++;
      assert(args.expectedRevision === native.revision, "Prompt-boundary IPC keeps the exact rendered revision");
      assert(native.phase === "ready" ? args.input === "fixture_account" : native.phase === "password" ? args.input === credentials[0] : native.phase === "email_code" && args.input === credentials[3],
        "A stale token is never rebound to a later password prompt");
      native.accepted_input_revision = Number(args.expectedRevision);
      const next = native.phase === "ready" ? "password" : native.phase === "password" ? "connecting" : "authenticated";
      advance(next, next === "password" ? "secret" : "disabled");
      if (native.phase === "authenticated") native = { ...native, account_name: "Fixture_Account", steamcmd_root: "C:\\Fixture\\SteamCMD" };
      return structuredClone(native);
    }
    if (command === "start_steam_workshop_account_login") {
      if (startFailure) { startFailure = false; throw new Error(notReady); }
      const target = args.target as { kind: string; id: string };
      assert(Object.keys(args).sort().join() === "sessionId,target" && target.kind === "instance" && target.id === first.summary.id && typeof args.sessionId === "string" &&
        /^steam-workshop-login-[0-9a-f-]{36}$/.test(args.sessionId), "Starting a console session sends only its instance and bounded operation identity");
      starts++;
      native = { session_id: String(args.sessionId), target: { kind: "instance", id: target.id }, revision: 1, accepted_input_revision: null, phase: "ready", input_mode: "command",
        events: ["ready"], account_name: null, steamcmd_root: null, error: null };
      return structuredClone(native);
    }
    if (["read_steam_workshop_account_login", "send_steam_workshop_account_input", "cancel_steam_workshop_account_login"].includes(command)) {
      if (command === "read_steam_workshop_account_login" && readFailure) {
        if (readFailure === "notReady") throw new Error(notReady);
        readFailure = null; native = null;
        throw new Error(JSON.stringify({ code: "steam_account_session_unavailable", message: "The SteamCMD login session is unavailable." }));
      }
      if (command === "cancel_steam_workshop_account_login" && cancelFailure) { cancelFailure = false; throw new Error(notReady); }
      const target = args.target as { kind: string; id: string };
      assert(native && args.sessionId === native.session_id && target.kind === native.target.kind && target.id === native.target.id, "Session calls stay scoped to their native instance and identity");
      if (command === "cancel_steam_workshop_account_login") { cancellations++; advance("cancelled", "disabled"); }
      if (command === "send_steam_workshop_account_input") {
        inputAttempts++;
        assert(args.expectedRevision === native.revision, "Input checks the exact prompt revision");
        if (native.phase === "ready") {
          if (args.input === "login fixture extra") throw new Error(JSON.stringify({ code: "steam_account_command_invalid", message: "Safe fixture rejection" }));
          assert(args.input === "login fixture_account" || args.input === "fixture_account", "The console accepts a bare account name or login ACCOUNT");
          if (starts === 1) inputOrder.push(native.phase);
          native.accepted_input_revision = Number(args.expectedRevision);
          advance(starts === 1 ? "input_submitted" : "password", starts === 1 ? "disabled" : "secret");
        } else if (native.phase === "password") {
          assert(args.input === credentials[0], "The dedicated secret IPC receives the exact synthetic password without trimming");
          if (starts === 1) inputOrder.push(native.phase);
          secretSubmissions++; native.accepted_input_revision = Number(args.expectedRevision);
          if (starts === 1) {
            advance("input_submitted", "disabled");
            await new Promise<void>((resolve) => { credentialReply = resolve; });
            advance("connecting", "disabled"); throw new Error("Synthetic accepted password response lost");
          }
          advance("connecting", "disabled");
        } else if (["guard_code", "email_code", "steam_guard"].includes(native.phase)) {
          const phase = native.phase;
          assert(args.input === credentials[phase === "guard_code" ? 2 : phase === "email_code" ? 3 : 1], "Only the requested native challenge code crosses its dedicated IPC");
          if (starts === 1) inputOrder.push(phase);
          secretSubmissions++; native.accepted_input_revision = Number(args.expectedRevision);
          if (starts === 1 && phase === "guard_code") {
            advance("input_submitted", "disabled");
            await new Promise<void>((resolve) => { credentialReply = resolve; });
            advance("preparing_steamcmd", "disabled"); throw new Error("Synthetic accepted token response lost");
          }
          advance("authenticating", "disabled");
          if (phase === "email_code") throw new Error("Synthetic accepted email response lost");
        } else throw new Error("Unexpected input phase");
      }
      return structuredClone(native);
    }
    if (command === "download_authenticated_steam_workshop_items") {
      assert(args.accountName === "Fixture_Account" && args.instanceId === first.summary.id, "Account downloads use only the backend-confirmed account");
      downloads.push({ command: "account", accountName: String(args.accountName) });
      if ((args.ids as string[])[0] === "3816899970") throw new Error(accessError);
      deployed = true;
      return { consumer_app_id: 1623730, install_root: "fixture", workshop_root: "fixture", items: [{ item_id: "3816899969", expected_path: "fixture/3816899969", expected_path_exists: true }] };
    }
    if (command === "clear_steam_server_account_authorization") {
      assert(Object.keys(args).join() === "target", "Restoring anonymous access sends only the bound target authorization to revoke");
      return;
    }
    if (command === "list_instance_archives") return { archives: [], pending_deletions: [], issues: [] };
    if (command === "read_instance_log_document_from_storage") return { source_path: null, lines: [], truncated: false, read_error: null };
    if (command === "download_steam_workshop_items" && delayDownload) {
      delayDownload = false; downloads.push({ command: "anonymous" });
      return new Promise<never>((_, reject) => { rejectDownload = reject; });
    }
    if (["read_instance_details_from_storage", "read_manual_mod_inventory", "read_background_jobs", "search_steam_workshop_items",
      "lookup_steam_workshop_items", "read_steam_workshop_item_details", "read_steam_workshop_installation_status", "download_steam_workshop_items"].includes(command)) return base(command, payload);
    return invokeMock(command, args);
  }, { shouldMockEvents: true });
  Object.assign(window, { isTauri: true });
  sessionStorage.setItem("langameLanToken", "Fixture_Historical_Lan_Token");
  assert(shouldUseLanApi() && canUseLocalDesktopApi(), "Historical browser credentials do not disable native account login or downloads");
  boundaryProbe = true;
  await act(async () => { await prepareBrowserLocaleCatalogs(); root.render(<I18nProvider><PromptBoundaryHost instanceId={first.summary.id} /></I18nProvider>); });
  await settle(() => Boolean(boundaryAccount), "The isolated production account hook did not mount");
  await act(async () => { await boundaryAccount!.start(); });
  accountPlaceholder = terminalInput().placeholder;
  await enter("fixture_account"); await submit();
  await settle(() => boundaryAccount?.snapshot?.phase === "password", "The prompt-boundary probe did not request password before token");
  await enter(credentials[0]); await submit();
  await settle(() => boundaryAccount?.snapshot?.phase === "connecting", "The password did not hand authentication to Steam");
  advance("guard_code", "secret");
  await settle(() => boundaryAccount?.snapshot?.phase === "guard_code", "The actual token challenge did not render");
  await act(async () => { await boundaryAccount!.send("", boundaryAccount!.snapshot!); });
  assert(boundaryInputCalls === 2 && boundaryAccount!.inputError === "steam_account_input_invalid", "Empty token input is rejected before any credential IPC");
  const oldPrompt = structuredClone(boundaryAccount!.snapshot!);
  await enter(credentials[2]); advance("email_code", "secret");
  await settle(() => boundaryAccount?.snapshot?.phase === "email_code" && terminalInput().value === "", "Changing secret prompts did not clear the token draft");
  await act(async () => { await boundaryAccount!.send(credentials[2], oldPrompt); });
  assert(boundaryInputCalls === 2 && boundaryAccount!.inputError === "steam_account_prompt_changed", "A stale rendered token prompt is rejected before any credential IPC");
  await act(async () => { await boundaryAccount!.send("", boundaryAccount!.snapshot!); });
  assert(boundaryInputCalls === 2 && boundaryAccount!.inputError === "steam_account_input_invalid", "Empty input is rejected for every actual code challenge");
  await act(async () => { await boundaryAccount!.send(credentials[0], { ...boundaryAccount!.snapshot!, session_id: "another-session" }); });
  assert(boundaryInputCalls === 2 && boundaryAccount!.inputError === "steam_account_prompt_changed", "A credential from another rendered session cannot bind to the current session");
  await enter(credentials[3]); await submit();
  await settle(() => boundaryAccount?.snapshot?.phase === "authenticated", "A freshly bound email code could not complete the isolated prompt-boundary probe");
  assert(boundaryInputCalls === 3, "Only the account, password and freshly bound email response cross the isolated credential boundary");
  await status("authenticated");
  assert(terminalInput().disabled && terminalInput().placeholder !== accountPlaceholder, "Authenticated input no longer invites another account command");
  noSecretEvidence();
  await act(async () => { root.render(<div />); }); boundaryProbe = false; native = null;
  await act(async () => { draw(); });
  await tab("mods"); await settle(() => Boolean(fixture.querySelector(".mw-mod-quick-action:not(:disabled)")), "Workshop actions did not become ready");
  await click(element(".mw-mod-quick-action")); await settle(() => downloads.length === 1 && !element<HTMLButtonElement>(".mw-mod-quick-action").disabled, "Anonymous fixture download did not finish");
  assert(starts === 0 && downloads[0].command === "anonymous", "Downloads default to anonymous and never auto-open login");
  assert(!fixture.querySelector(".mw-steam-account-control, .mw-install-help-button, .mw-install-help-dialog"), "Mod header has no account menu or installation-note entry");
  anonFault = "network";
  await click(element(".mw-mod-quick-action")); await settle(() => downloads.length === 2 && !element<HTMLButtonElement>(".mw-mod-quick-action").disabled, "Network failure did not complete");
  assert(starts === 0 && !fixture.querySelector(".server-steamcmd-console"), "Generic Failure and network errors never trigger account login");
  anonFault = "permission";
  await click(element(".mw-mod-quick-action"));
  await settle(() => Boolean(fixture.querySelector(".server-steamcmd-console")) && !terminalInput().disabled, "Verified anonymous access failure did not open LanGameCMD");
  assert(starts === 1 && fixture.textContent?.includes(locale === "zh-CN" ? "认证成功后会自动继续本次下载" : "continue automatically after authentication"), "The console explains the verified access failure and pending original download");
  await enter("login fixture extra"); await submit();
  await settle(() => Boolean(fixture.querySelector(".server-steamcmd-input-error")), "Invalid account command was not explained");
  assert(element(".server-steamcmd-input-error").textContent?.includes(locale === "zh-CN" ? "不要在命令中附加密码" : "Do not include a password"), "Invalid account input shows its specific actionable error");
  await enter(innerWidth < 1000 ? "fixture_account" : "login fixture_account"); await submit();
  await status("input_submitted");
  advance("preparing_steamcmd", "disabled"); await status("preparing_steamcmd");
  advance("connecting", "disabled"); await status("connecting");
  advance("password", "secret"); await status("password");
  assert(terminalInput().type === "password" && !fixture.querySelector(".server-steamcmd-auto-verify"), "Steam requests a fresh account password privately without verification preselection");
  await enter(credentials[0]);
  const copy = new ClipboardEvent("copy", { bubbles: true, cancelable: true }); terminalInput().dispatchEvent(copy);
  assert(copy.defaultPrevented && terminalInput().autocomplete === "off", "Password input disables copying and browser completion");
  await submit();
  await status("input_submitted");
  assert(terminalInput().value === "" && terminalInput().disabled, "Password submission clears the field before native delivery completes");
  noSecretEvidence();
  await act(async () => { credentialReply!(); credentialReply = null; });
  await status("connecting");
  assert(!fixture.querySelector(".server-steamcmd-auto-verify") && terminalInput().disabled, "Steam selects the verification path without a method-selection button or token preselection");
  await tab("mods");
  assert(element<HTMLButtonElement>(".mw-mod-quick-action").disabled && cancellations === 0, "Changing tabs retains login and blocks concurrent Mod mutations");
  await tab("runtime"); assert(Boolean(fixture.querySelector(".server-steamcmd-console")), "Returning to Runtime resumes the selected SteamCMD session");
  if (scenario === "direct") {
    assert(!native!.events.includes("guard_code") && !native!.events.includes("email_code"), "An account without 2FA proceeds without requesting a code");
  } else {
    const challenge = scenario === "email" ? "email_code" : "guard_code";
    advance(challenge, "secret"); await status(challenge);
    await settle(() => !terminalInput().disabled, "The actual Steam challenge did not accept private input");
    assert(terminalInput().type === "password" && !fixture.querySelector(".server-steamcmd-auto-verify"), "Only a real native challenge exposes hidden code input, without extra buttons");
    const emptyAttempts = inputAttempts; await submit();
    assert(inputAttempts === emptyAttempts, "Empty challenge Enter never sends an input or chooses a different login method");
    const code = credentials[scenario === "email" ? 3 : 2]; await enter(code);
    const codeCopy = new ClipboardEvent("copy", { bubbles: true, cancelable: true }); terminalInput().dispatchEvent(codeCopy);
    assert(codeCopy.defaultPrevented && terminalInput().autocomplete === "off", "Challenge input disables copying and browser completion");
    const inputBounds = terminalInput().getBoundingClientRect();
    assert(inputBounds.left >= 0 && inputBounds.right <= innerWidth + 1 && document.documentElement.scrollWidth <= innerWidth + 1, "The automatic challenge input fits the console without horizontal overflow");
    if (scenario === "phone") {
      const beforeApproval = inputAttempts;
      assert(fixture.textContent?.includes(locale === "zh-CN" ? "若手机 Steam 应用中出现登录请求" : "or approve a login request if one appears in the app"), "The mobile challenge explains token input and conditional external phone approval");
      advance("authenticating", "disabled"); await status("authenticating");
      assert(inputAttempts === beforeApproval && terminalInput().value === "", "External phone approval needs no extra credential IPC and clears an unsubmitted token draft");
    } else {
      await submit();
      if (scenario === "token") {
        await status("input_submitted"); noSecretEvidence();
        await act(async () => { credentialReply!(); credentialReply = null; });
        await status("preparing_steamcmd"); advance("connecting", "disabled"); await status("connecting");
        advance("steam_guard", "secret"); await status("steam_guard");
        await enter(credentials[1]); await submit();
      }
      assert(terminalInput().value === "", "Every submitted code clears immediately and is never automatically resent");
    }
  }
  advance("authenticating", "disabled"); await status("authenticating");
  advance("verifying_cache", "disabled"); await status("verifying_cache");
  noSecretEvidence();
  advance("authenticated", "disabled"); native = { ...native!, account_name: "Fixture_Account", steamcmd_root: "C:\\Fixture\\SteamCMD" };
  await settle(() => writes === 1 && element('.server-detail-tabs [id$="-mods"]').getAttribute("aria-selected") === "true", "Authentication did not automatically resume and finish the original Mod installation");
  assert(downloads.length === 4 && JSON.parse(first.settings_json).mod_package_names.includes("FixturePackage"), "The exact original item is downloaded and enabled once after login without another click");
  const initialSecretSubmissions = secretSubmissions;
  assert(inputOrder.filter((phase) => phase === "password").length === 1, "The frontend submits the password once while Steam determines the required challenge");
  await settle(() => fixture.querySelectorAll(".mw-mod-quick-action:not(:disabled)").length === 2, "Mod actions did not recover after automatic continuation");
  await click(fixture.querySelectorAll<HTMLElement>(".mw-mod-quick-action")[1]);
  await settle(() => downloads.length === 5 && !fixture.querySelectorAll<HTMLButtonElement>(".mw-mod-quick-action")[1].disabled, "Authenticated access rejection did not complete");
  assert(starts === 1, "An authenticated access failure never loops back into automatic login");
  await goSteamConsole();
  assert(element<HTMLSelectElement>(".server-steamcmd-identity").value === "Fixture_Account", "Download identity is managed only in LanGameCMD");
  await act(async () => { const select = element<HTMLSelectElement>(".server-steamcmd-identity"); select.value = ""; select.dispatchEvent(new Event("change", { bubbles: true })); });
  await tab("mods"); anonFault = "generic";
  await click(fixture.querySelectorAll<HTMLElement>(".mw-mod-quick-action")[1]);
  await settle(() => downloads.length === 6 && !fixture.querySelectorAll<HTMLButtonElement>(".mw-mod-quick-action")[1].disabled, "Restored anonymous download did not finish");
  assert(downloads.map((entry) => entry.command).join() === "anonymous,anonymous,anonymous,account,account,anonymous", "Console identity selection restores anonymous downloads");
  anonFault = "permission";
  await click(fixture.querySelectorAll<HTMLElement>(".mw-mod-quick-action")[1]);
  await settle(() => Boolean(fixture.querySelector(".server-steamcmd-console")) && !terminalInput().disabled, "A new anonymous permission failure did not open its own recovery");
  await enter("fixture_account"); await submit();
  await settle(() => native?.phase === "password", "The failed-login scenario did not request a password before token choice");
  await enter(credentials[0]);
  advance("failed", "disabled");
  native = { ...native!, error: JSON.stringify({ code: "steam_account_authentication_failed", message: "Fixture authentication failed", output_excerpt: "Login failed (InvalidPassword)." }) };
  await settle(() => fixture.textContent?.includes("InvalidPassword") === true, "The native login failure did not render");
  assert(terminalInput().value === "" && terminalInput().type === "text" && terminalInput().disabled && terminalInput().placeholder !== accountPlaceholder,
    "A failed secret prompt clears its draft and no longer invites account input");
  await status("failed");
  noSecretEvidence();
  assert(fixture.textContent?.includes(locale === "zh-CN" ? "Steam 账号登录未完成" : "Steam account login did not complete"),
    "Native login failures have a localized actionable retry without exposing secret input");
  await click(element(".server-steamcmd-console .server-runtime-terminal-actions button"));
  await settle(() => native?.phase === "ready", "A failed login could not be retried");
  await click(element(".server-steamcmd-console .server-runtime-terminal-actions button"));
  await settle(() => native?.phase === "cancelled", "Explicit cancellation did not finish");
  await status("cancelled");
  assert(terminalInput().disabled && terminalInput().placeholder !== accountPlaceholder, "Cancelled input no longer invites account commands");
  await tab("mods");
  await settle(() => !fixture.querySelectorAll<HTMLButtonElement>(".mw-mod-quick-action")[1].disabled, "Cancelled recovery did not release the original Mod operation");
  assert(writes === 1 && downloads.length === 7, "Cancelled account recovery does not retry or save its original Mod download");
  await click(fixture.querySelectorAll<HTMLElement>(".mw-mod-quick-action")[1]);
  await settle(() => Boolean(fixture.querySelector(".server-steamcmd-console")) && !terminalInput().disabled, "Session-loss recovery did not start");
  readFailure = "unavailable";
  await settle(() => Boolean(fixture.querySelector(".server-steamcmd-input-error")) && !element<HTMLButtonElement>(".server-steamcmd-console button").disabled, "A confirmed missing session did not release recovery and permit login again");
  assert(!fixture.textContent?.includes(locale === "zh-CN" ? "认证成功后会自动继续本次下载" : "continue automatically after authentication"), "An expired session ends the pending original download");
  startFailure = true;
  await click(element(".server-steamcmd-console .server-runtime-terminal-actions button"));
  await settle(() => Boolean(fixture.querySelector(".server-runtime-console-errors")), "A safe pre-input startup failure was not shown");
  const safeFailure = locale === "zh-CN" ? "SteamCMD 尚未就绪" : "SteamCMD is not ready";
  assert(element(".server-runtime-console-errors").textContent?.includes(safeFailure), "A login startup error keeps its actionable backend cause instead of becoming an undelivered-input error");
  await click(element(".server-steamcmd-console .server-runtime-terminal-actions button"));
  await settle(() => native?.phase === "ready", "A cancelled login could not be restarted");
  readFailure = "notReady";
  await settle(() => fixture.querySelector(".server-runtime-console-errors")?.textContent?.includes(safeFailure) === true, "A safe read failure did not retain its actual cause");
  readFailure = null;
  await settle(() => !fixture.querySelector(".server-runtime-console-errors"), "A successful snapshot did not clear the transient read failure");
  cancelFailure = true;
  await click(element(".server-steamcmd-console .server-runtime-terminal-actions button"));
  await settle(() => fixture.querySelector(".server-runtime-console-errors")?.textContent?.includes(safeFailure) === true, "A safe cancellation failure did not retain its actual cause");
  assert(native?.phase === "ready", "An unknown cancellation outcome retains the native session until cleanup is confirmed");
  await click(element(`.server-list-card[data-card-id="${second.summary.id}"] .server-list-card-hitarea`));
  await settle(() => cancellations === 2, "Changing instances did not cancel the prior login session");
  await tab("runtime");
  assert(!fixture.querySelector(".server-steamcmd-console, .server-steamcmd-source"), "A non-Steam Minecraft instance has no SteamCMD login source");
  await click(element(`.server-list-card[data-card-id="${first.summary.id}"] .server-list-card-hitarea`)); await tab("mods"); await startLogin();
  await act(async () => { root.render(<div>Workspace unmounted</div>); });
  await settle(() => cancellations === 3, "Leaving the workspace did not cancel its unfinished login");
  await act(async () => { draw(); }); await goSteamConsole();
  assert(element<HTMLSelectElement>(".server-steamcmd-identity").value === "", "A fresh workspace returns to anonymous after prior authentication");
  await tab("mods"); delayDownload = true;
  await click(fixture.querySelectorAll<HTMLElement>(".mw-mod-quick-action")[1]);
  await settle(() => Boolean(rejectDownload), "The isolated anonymous download was not pending before changing instances");
  await click(element(`.server-list-card[data-card-id="${second.summary.id}"] .server-list-card-hitarea`));
  await act(async () => { rejectDownload!(new Error(accessError)); rejectDownload = null; });
  await tab("runtime");
  assert(starts === 6 && !fixture.querySelector(".server-steamcmd-console, .server-steamcmd-source"), "A late access failure cannot start the previous instance's login or navigate the new instance");
  await click(element(`.server-list-card[data-card-id="${first.summary.id}"] .server-list-card-hitarea`)); await tab("mods");
  delayDownload = true; await click(fixture.querySelectorAll<HTMLElement>(".mw-mod-quick-action")[1]);
  await settle(() => Boolean(rejectDownload), "The ABA anonymous download did not become pending");
  await click(element(`.server-list-card[data-card-id="${second.summary.id}"] .server-list-card-hitarea`));
  await click(element(`.server-list-card[data-card-id="${first.summary.id}"] .server-list-card-hitarea`)); await tab("mods");
  await act(async () => { rejectDownload!(new Error(accessError)); rejectDownload = null; });
  assert(starts === 6 && !fixture.querySelector(".server-steamcmd-console") && element('.server-detail-tabs [id$="-mods"]').getAttribute("aria-selected") === "true",
    "Returning to the same instance cannot reactivate an old download owner's account recovery");
  delayDownload = true; await click(fixture.querySelectorAll<HTMLElement>(".mw-mod-quick-action")[1]);
  await settle(() => Boolean(rejectDownload), "The isolated anonymous download was not pending before unmount");
  await act(async () => { root.render(<div>Workspace unmounted</div>); });
  await act(async () => { rejectDownload!(new Error(accessError)); rejectDownload = null; });
  assert(starts === 6, "A late access failure cannot reopen a session after its owner unmounts");
  await act(async () => { draw(); }); await goSteamConsole();
  await click(element(".server-steamcmd-console .server-runtime-terminal-actions button"));
  await settle(() => !terminalInput().disabled, "Final console did not become ready");
  await enter("fixture_account"); await submit();
  await settle(() => native?.phase === "password", "Final login did not request its password first");
  await enter(credentials[0]); await submit();
  await status("connecting");
  if (scenario !== "direct") { advance(scenario === "email" ? "email_code" : "guard_code", "secret"); await status(scenario === "email" ? "email_code" : "guard_code"); }
  assert(!fixture.querySelector(".server-steamcmd-auto-verify"), "No verification-method button remains in the final console");
  noSecretEvidence();
  assert(serverCommands === 0 && writes === 1 && props.selectedDetails?.summary.status === "Stopped" && errors.length === 0,
    "SteamCMD authentication never routes secrets to game commands, writes any settings beyond the resumed installation or starts a stopped server");
  return { status: "passed", locale, checks, writes, starts, cancellations, downloads, server_commands: serverCommands,
    native_token_precedence: true, challenge_scenario: scenario, secret_submissions: secretSubmissions, initial_secret_submissions: initialSecretSubmissions,
    automatic_challenge: true, password_sent_once: true, rendered_prompt_bound: true, credential_order: inputOrder, current_status: true, terminal_placeholder: true,
    default_anonymous: true, secrets_absent: true, cross_tab_retained: true, failure_retry: true, auto_resume: true, generic_failures_do_not_login: true,
    authenticated_failure_no_loop: true, instance_change_cancelled: true, unmount_cancelled: true, stale_completion_safe: true,
    expired_session_recoverable: true, safe_lifecycle_errors: true, secret_draft_transition_clear: true, browser_errors: errors };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error("Steam account browser regression exceeded 45 seconds")), 45000); })])
  .catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .then((report) => fetch(`/__reliability_result/${new URLSearchParams(location.search).get("nonce")}`, { method: "POST", body: JSON.stringify(report) }));
