import React, { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { invokeMock } from "../../src/api-mock";
import { I18nProvider } from "../../src/i18n";
import { immediateWorldSaveAction } from "../../src/runtime-save-capability";
import { ValheimAccountControlSetup } from "../../src/views/servers/player-center/ValheimAccountControlSetup";
import type { BootstrapResponse, InstanceDetails, InstanceRuntimeCommandResult, ModuleDetails } from "../../src/types";
import { ImmediateWorldSave } from "../../src/views/servers/ImmediateWorldSave";
import { ServerCardWorldSave } from "../../src/views/servers/ServerCardWorldSave";
import { MaintenanceWorkspace } from "../../src/views/servers/MaintenanceWorkspace";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const nonce = new URLSearchParams(location.search).get("nonce");
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const calls: Array<{ instanceId: string; command: string; runtimeActionId: string }> = [];
let responseMode: "requested" | "confirmed" | "unconfirmed" | "pending" | "failed" | "deferred" = "requested";
let resolveSave: ((value: InstanceRuntimeCommandResult) => void) | undefined;
let activeDetails: InstanceDetails;
let activeModule: ModuleDetails | null = null;
const cardNotices: Array<{ tone?: string; params?: Record<string, unknown> }> = [];
// Replace only the native IPC boundary. The production component, configuration
// guards and API envelope run against current module TOML declarations.
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args: {
  input: { instanceId: string; command: string; runtimeActionId: string }; instanceId?: string
}) => {
  if (command === "read_instance_details_from_storage") return activeDetails;
  if (command === "read_module_details") return activeModule;
  if (command !== "send_instance_runtime_command") throw new Error(`Unexpected native command: ${command}`);
  calls.push(args.input);
  if (responseMode === "failed") throw new Error("Native save connection lost");
  if (responseMode === "deferred") return new Promise<InstanceRuntimeCommandResult>(resolve => { resolveSave = resolve; });
  return { instance_id: args.input.instanceId, command: args.input.command, process_key: "main", display_name: "Save transport",
    pid: 1, response_text: "Native save response", write_confirmation_pending: responseMode === "pending",
    save_completion: responseMode === "confirmed" ? "confirmed" : responseMode === "requested" ? "requested" : "unconfirmed", submitted_at_unix_ms: Date.now() };
} } });
let checks = 0;
function check(condition: unknown, message: string): asserts condition { if (!condition) throw new Error(message); checks++; }
async function settle(predicate: () => boolean, message: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    if (performance.now() >= deadline) throw new Error(message);
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
  }
}
async function render(details: InstanceDetails, moduleDetails: ModuleDetails | null, active = true) {
  activeDetails = details; activeModule = moduleDetails;
  await act(async () => root.render(<StrictMode><I18nProvider><main style={{ padding: 24 }}>
    <MaintenanceWorkspace active={active} sections={[
      { id: "backups", title: "存档与备份", icon: "history", content: null,
        persistentContent: <ImmediateWorldSave details={details} moduleDetails={moduleDetails} /> },
      { id: "other", title: "其他维护", icon: "settings", content: <p className="maintenance-costly-reader">Other maintenance content</p> }
    ]} broadcast={<p>Broadcast boundary</p>} />
    <ServerCardWorldSave key={details.summary.id} instance={details.summary} supported={Boolean(immediateWorldSaveAction(moduleDetails))}
      onActivity={(notice) => cardNotices.push(notice)} />
    {details.summary.module_id === "valheim" ? <ValheimAccountControlSetup key={`control-${details.summary.id}`}
      details={details} connected={false} locale="zh-CN" /> : null}
    </main></I18nProvider></StrictMode>));
  await settle(() => Boolean(fixture.querySelector(".immediate-world-save")), "Save panel did not mount");
}
const button = () => fixture.querySelector<HTMLButtonElement>(".immediate-world-save button")!;
const cardSave = () => fixture.querySelector<HTMLButtonElement>(".server-list-card-save-action")!;
async function submit() { await act(async () => { button().click(); }); }
async function chooseSection(title: string) {
  const target = Array.from(fixture.querySelectorAll<HTMLButtonElement>(".configuration-workspace__sidebar button"))
    .find(node => node.textContent?.trim() === title);
  if (!target) throw new Error(`Missing maintenance section ${title}`);
  await act(async () => { target.click(); });
}
async function run() {
  await act(async () => {
    await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]);
  });
  const bootstrap = await invokeMock<BootstrapResponse>("bootstrap", { includeSystemSnapshot: false });
  const original = bootstrap.state.instances.find((entry) => entry.module_id === "minecraft")!;
  const stored = await invokeMock<InstanceDetails>("read_instance_details_from_storage", { instance_id: original.id });
  let last: InstanceDetails = stored;
  let lastModule: ModuleDetails | null = null;
  const maintenanceModules: string[] = [];
  const cardModules: string[] = [];
  const configurationGuards: string[] = [];
  const persistenceScopes: string[] = [];
  const startupPreparedModules: string[] = [];
  const savedFixtures = new Map<string, { details: InstanceDetails; module: ModuleDetails }>();
  for (const [moduleId, command] of [
    ["dontstarve", "c_save()"], ["minecraft", "save-all flush"], ["projectzomboid", "save"], ["terraria", "save"],
    ["unturned", "Save"], ["arksurvivalevolved", "saveworld"], ["arksurvivalascended", "saveworld"],
    ["sevendaystodie", "saveworld"], ["necesse", "save"], ["rust", "server.save"], ["romestead", "save"],
    ["theforest", "save"], ["humanitz", "save"], ["satisfactory", "save"], ["astroneer", "save"],
    ["valheim", "save"], ["runescapedragonwilds", "save"], ["corekeeper", "save"], ["vrising", "save"], ["sonsoftheforest", "save"], ["abioticfactor", "save"], ["returntomoria", "save"], ["windrose", "save"],
    ["enshrouded", "save"], ["rimworld", "save"], ["barotrauma", "save"], ["soulmask", "SaveWorld 1"], ["nightingale", "save"], ["conanexiles", "save"], ["scum", "save_persistent_state"], ["palworld", "save"]
  ]) {
    const module = await invokeMock<ModuleDetails>("read_module_details", { moduleId });
    const details: InstanceDetails = { ...stored, summary: { ...stored.summary, id: `${moduleId}-save-fixture`, module_id: moduleId,
      status: "Running", active_process_count: 1 }, active_run: { run_id: 1, processes: [{ process_key: moduleId === "dontstarve" ? "master" : "main", status: "Running" }] },
      settings_json: JSON.stringify({ enable_rcon: true, rcon_enabled: true, rcon_web: true,
        rcon_password: "fixture-server-password", rest_api_enabled: true, admin_password: "fixture-admin-password",
        telnet_enabled: true, telnet_password: "fixture-server-password", console_password: "fixture-server-password" }),
      ports: [{ name: "rcon", port: 25575, protocol: "tcp" }, { name: "rest_api", port: 8212, protocol: "tcp" },
        { name: "telnet", port: 8081, protocol: "tcp" }, { name: "console", port: 8778, protocol: "tcp" }] };
    await render(details, module);
    const scope = ({ rimworld: "离线客户端无法保存", barotrauma: "进行中的回合无法手动保存", scum: "全局状态、待保存的坐标变化和已注册的持久化实体" } as Record<string, string>)[moduleId];
    if (scope) {
      // Limits of what a save can reach stay visible as one short line; a plain scope description lives in the title bubble.
      if (moduleId === "scum") check(!fixture.querySelector(".immediate-world-save > .form-note")
        && fixture.querySelector(".immediate-world-save [role=tooltip]")?.textContent?.includes(scope), "SCUM save scope must live in the title help bubble");
      else check(fixture.querySelector(".immediate-world-save > .form-note")?.textContent?.includes(scope), `${moduleId} save limitation must be visible without a tooltip`);
      check(cardSave()?.title.includes(scope), `${moduleId} card tooltip must retain its scope`);
      if (moduleId === "scum") check(button().textContent === "保存存档" && cardSave().getAttribute("aria-label")?.startsWith("保存服务器状态"), "SCUM must describe the native persistent-state save scope");
      persistenceScopes.push(moduleId);
    }
    check(!button().disabled, `${moduleId} save must be available with its declared configuration`);
    if (moduleId === "valheim") {
      check(fixture.querySelector('[data-valheim-control-setup] button')?.textContent === "提前准备账号管理",
        "Valheim account preparation must remain an optional early action");
      check(fixture.textContent?.includes("启动时会自动检查、安装并配置 BepInEx 和管理扩展"),
        "Valheim must describe automatic loader preparation at startup");
      check(!fixture.querySelector(".inline-confirm-review"), "Automatic startup help must not ask for a separate confirmation");
    }
    const beforeMaintenanceSave = calls.length;
    await submit();
    check(calls.length === beforeMaintenanceSave + 1 && calls.at(-1)?.instanceId === details.summary.id
      && calls.at(-1)?.runtimeActionId === "save_world" && calls.at(-1)?.command === command,
      `${moduleId} must submit the declared save action`);
    maintenanceModules.push(moduleId);
    check(fixture.textContent?.includes("Native save response"), "Native result must remain visible");
    check(fixture.querySelector(".immediate-world-save .shell-activity-notice.is-info")
      && !fixture.querySelector(".immediate-world-save .shell-activity-notice.is-success"),
      "A processed save request must use the normal activity status");
    check(fixture.textContent?.includes("已发送保存请求，由游戏执行保存。")
      && !fixture.querySelector(".immediate-world-save")?.textContent?.includes("保存完成。"),
      "A processed native command must report request delivery without claiming disk completion");
    check(cardSave() && !cardSave().disabled, `${moduleId} must expose an available card shortcut`);
    const beforeCardSave = calls.length;
    const beforeCardNotice = cardNotices.length;
    await act(async () => cardSave().click());
    await settle(() => !cardSave().disabled && cardNotices.length === beforeCardNotice + 1,
      `${moduleId} card save did not settle`);
    check(calls.length === beforeCardSave + 1 && calls.at(-1)?.instanceId === details.summary.id
      && calls.at(-1)?.runtimeActionId === "save_world" && calls.at(-1)?.command === command,
      `${moduleId} card shortcut must submit its declared save operation exactly once`);
    check(cardNotices.at(-1)?.params?.message?.toString().includes("Native save response"),
      `${moduleId} card save must expose the native result`);
    cardModules.push(moduleId);
    savedFixtures.set(moduleId, { details, module });
    last = details; lastModule = module;
  }
  for (const [moduleId, enabledKey, passwordKey, portName, protocol] of [
    ["rust", "rcon_web", "rcon_password", "rcon", "RCON"],
    ["arksurvivalevolved", "rcon_enabled", "admin_password", "rcon", "RCON"],
    ["sevendaystodie", "telnet_enabled", "telnet_password", "telnet", "Telnet"],
    ["palworld", "rest_api_enabled", "admin_password", "rest_api", "REST API"],
    ["astroneer", null, "console_password", "console", "控制台"]
  ]) {
    const saved = savedFixtures.get(moduleId)!;
    const settings = JSON.parse(saved.details.settings_json) as Record<string, unknown>;
    for (const [reason, details] of [
      ...(enabledKey ? [["disabled", { ...saved.details, settings_json: JSON.stringify({ ...settings, [enabledKey]: false }) }]] as const : []),
      ["password", { ...saved.details, settings_json: JSON.stringify({ ...settings, [passwordKey]: "  " }) }],
      ["missing-port", { ...saved.details, ports: saved.details.ports.filter((entry) => entry.name !== portName) }],
      ["udp-port", { ...saved.details, ports: saved.details.ports.map((entry) => entry.name === portName ? { ...entry, protocol: "udp" } : entry) }]
    ] as const) {
      await render(details, saved.module);
      check(button().disabled && fixture.textContent?.includes(protocol), `${moduleId} ${reason} must explain its unavailable transport`);
      const before = calls.length;
      await submit();
      check(calls.length === before, `${moduleId} ${reason} must prevent maintenance dispatch`);
      const beforeNotice = cardNotices.length;
      await act(async () => cardSave().click());
      await settle(() => !cardSave().disabled && cardNotices.length === beforeNotice + 1,
        `${moduleId} ${reason} card validation did not settle`);
      check(calls.length === before && cardNotices.at(-1)?.tone === "error"
        && cardNotices.at(-1)?.params?.message?.toString().includes(protocol),
        `${moduleId} ${reason} card shortcut must validate fresh configuration before dispatch`);
      configurationGuards.push(`${moduleId}:${reason}`);
    }
  }
  const sevenDays = savedFixtures.get("sevendaystodie")!;
  const sevenDaysSettings = JSON.parse(sevenDays.details.settings_json) as Record<string, unknown>;
  await render({ ...sevenDays.details, settings_json: JSON.stringify({ ...sevenDaysSettings, eac_enabled: true }) }, sevenDays.module);
  check(!button().disabled, "7 Days native Telnet saving must remain available with EAC enabled");
  const beforeEacSave = calls.length;
  await submit();
  check(calls.length === beforeEacSave + 1, "EAC must not block the native maintenance command");
  const beforeEacNotice = cardNotices.length;
  await act(async () => cardSave().click());
  await settle(() => !cardSave().disabled && cardNotices.length === beforeEacNotice + 1, "EAC card validation did not settle");
  check(calls.length === beforeEacSave + 2 && cardNotices.at(-1)?.tone !== "error"
    && cardNotices.at(-1)?.params?.message?.toString().includes("已发送保存请求"),
    "The 7 Days card must preserve EAC and send the native command");
  configurationGuards.push("sevendaystodie:eac-preserved");
  const unsupportedModule = await invokeMock<ModuleDetails>("read_module_details", { moduleId: "squad" });
  await render({ ...last, summary: { ...last.summary, id: "squad-save-fixture", module_id: "squad" } }, unsupportedModule);
  check(button().disabled && fixture.textContent?.includes("没有可恢复的持久世界存档"),
    "Games without a declared save action must retain maintenance guidance");
  check(!cardSave(), "Games without a declared save action must not offer a card shortcut");
  const beforeUnsupportedSave = calls.length;
  await submit();
  check(calls.length === beforeUnsupportedSave, "Unsupported maintenance must never dispatch a save command");
  await render({ ...last, summary: { ...last.summary, status: "Stopped", active_process_count: 0 }, active_run: null }, lastModule);
  check(button().disabled && fixture.textContent?.includes("请先启动服务器"), "Stopped saves need an explanation");
  await render(last, null);
  check(button().disabled && fixture.textContent?.includes("保存能力尚未就绪"), "Unloaded save capability must remain safely unavailable");
  await render({ ...last, ports: [], settings_json: JSON.stringify({ rest_api_enabled: false, admin_password: "" }) }, lastModule);
  check(button().disabled && fixture.textContent?.includes("REST API"), "Palworld saving must explain its native REST configuration");
  await render(last, lastModule);
  const beforeCardSave = calls.length;
  await act(async () => cardSave().click());
  await settle(() => !cardSave().disabled && cardNotices.length > 0, "Card save did not settle");
  check(calls.length === beforeCardSave + 1 && calls.at(-1)?.runtimeActionId === "save_world",
    "Card shortcut must use the shared declared save operation exactly once");
  check(cardNotices.at(-1)?.params?.message?.toString().includes("Native save response"), "Card save must expose the native result");
  check(cardNotices.at(-1)?.tone !== "error"
    && cardNotices.at(-1)?.params?.message?.toString().includes("已发送保存请求，由游戏执行保存。"),
    "The card must finish a processed request without a verification error");
  responseMode = "confirmed"; await submit();
  check(fixture.textContent?.includes("保存完成。"), "Confirmed native completion must retain its distinct message");
  responseMode = "pending"; await submit();
  check(fixture.querySelector('[role="alert"]')?.textContent?.includes("保存结果未知"), "Missing native completion must fail verification");
  responseMode = "unconfirmed"; await submit();
  check(fixture.querySelector('[role="alert"]')?.textContent?.includes("保存结果未知")
    && !fixture.querySelector(".immediate-world-save")?.textContent?.includes("保存完成。"),
    "A confirmed transport write must not become a completed native save");
  await act(async () => cardSave().click());
  await settle(() => !cardSave().disabled, "Unconfirmed card result did not settle");
  check(cardNotices.at(-1)?.tone === "error" && cardNotices.at(-1)?.params?.message?.toString().includes("保存结果未知"),
    "The card must expose missing completion as a verification failure");
  responseMode = "failed"; await submit();
  check(fixture.querySelector('[role="alert"]')?.textContent?.includes("Native save connection lost"), "Native failure must remain visible");
  responseMode = "deferred"; await submit();
  const savePanel = fixture.querySelector(".immediate-world-save");
  check(button().disabled, "A pending save must prevent another submission");
  const beforeDuplicate = calls.length;
  await submit();
  check(calls.length === beforeDuplicate, "A duplicate click during a pending save must not dispatch another command");
  await chooseSection("其他维护");
  check(fixture.querySelector(".immediate-world-save") === savePanel && savePanel?.getClientRects().length === 0,
    "Changing maintenance categories must hide and preserve the save operation");
  await chooseSection("存档与备份");
  check(button().disabled, "Returning to save maintenance must retain the pending request");
  await render(last, lastModule, false);
  check(fixture.querySelector(".immediate-world-save") === savePanel && !fixture.querySelector(".maintenance-costly-reader"),
    "Leaving maintenance must retain only immediate save state, not costly readers");
  const request = calls[calls.length - 1];
  await act(async () => resolveSave!({ instance_id: request.instanceId, command: request.command, process_key: "main",
    display_name: "Save transport", pid: 1, response_text: "Native save response after leaving maintenance",
    write_confirmation_pending: false, save_completion: "confirmed", submitted_at_unix_ms: Date.now() }));
  await render(last, lastModule);
  check(!button().disabled && fixture.textContent?.includes("Native save response after leaving maintenance"),
    "A save response received on another tab must remain visible on return");
  const rect = fixture.querySelector(".immediate-world-save")!.getBoundingClientRect();
  check(rect.width > 0 && rect.left >= 0 && rect.right <= innerWidth, "Maintenance save must fit the viewport");
  check(document.documentElement.scrollWidth <= innerWidth + 1, "Save feedback must not overflow horizontally");
  // Saving never requires a separate preparation click in maintenance.
  for (const moduleId of ["corekeeper", "vrising", "sonsoftheforest", "abioticfactor", "returntomoria", "windrose", "rimworld", "barotrauma", "nightingale", "conanexiles", "scum", "unturned", "minecraft", "romestead", "necesse", "projectzomboid", "terraria", "sevendaystodie", "arksurvivalevolved"]) {
    const saved = savedFixtures.get(moduleId)!;
    check(!saved.module.runtime?.player_actions?.some((action) => action.id === "prepare_world_save"), `${moduleId} must not declare manual preparation`);
    await render(saved.details, saved.module);
    check(!fixture.textContent?.includes("准备保存扩展"), `${moduleId} must not offer a manual preparation control`);
    check(!button().disabled, `${moduleId} saving must remain available without a separate preparation click`);
    startupPreparedModules.push(moduleId);
  }
  const limitedSave = savedFixtures.get("barotrauma")!;
  await render(limitedSave.details, limitedSave.module);
  const scopeRect = fixture.querySelector(".immediate-world-save > .form-note")!.getBoundingClientRect();
  check(scopeRect.width > 0 && scopeRect.right <= innerWidth && scopeRect.bottom <= innerHeight,
    "The Barotrauma save limitation must remain fully readable in the delivered viewport");
  const finalSave = savedFixtures.get("minecraft")!;
  responseMode = innerWidth > 1000 ? "confirmed" : "requested";
  await render(finalSave.details, finalSave.module);
  await submit();
  await act(async () => cardSave().click());
  await settle(() => !cardSave().disabled, "Final save status did not settle");
  check(fixture.querySelector(".immediate-world-save")?.textContent?.includes(
    responseMode === "confirmed" ? "保存完成。" : "已发送保存请求，由游戏执行保存。"), "Delivered save status must remain visible");
  check(errors.length === 0, `Unexpected browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, maintenance_modules: maintenanceModules, card_modules: cardModules,
    startup_prepared_modules: startupPreparedModules, persistence_scopes: persistenceScopes,
    configuration_guards: configurationGuards, browser_errors: errors, viewport: { width: innerWidth, height: innerHeight } };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error("Save UI stalled")), 25000); })])
  .finally(() => { clearTimeout(watchdog); Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }); })
  .catch((error) => ({ status: "failed", checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
