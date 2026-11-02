import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import React, { act, StrictMode, useState } from "react";
import { createRoot } from "react-dom/client";
import { invokeMock } from "../../src/api-mock";
import type { BootstrapResponse, InstanceDetails } from "../../src/types";
import { I18nProvider, useI18n, type LocaleCode } from "../../src/i18n";
import type { ArkSpawnInput, ArkSpawnResult, ArkToolsStatus, ArkToolsPreparationResult, ArkToolsRemovalResult } from "../../src/api-ark-tools";
import { queuedArkToolsInstallation, type ArkToolsInstallProgress } from "../../src/ark-tools-installation";
import type { ArkMapPlayer } from "../../src/ark-world-map";
import { ArkCreatureSpawner } from "../../src/views/servers/ArkCreatureSpawner";
import { ArkItemSpawnPanel } from "../../src/views/servers/ArkItemSpawnPanel";
import { ArkWorldToolsPanel } from "../../src/views/servers/ArkWorldToolsPanel";
import type { ArkMapToolContext } from "../../src/views/servers/ArkWorldMapWorkbench";
import { getGmToolCatalog, getInitialGmToolValues } from "../../src/views/servers/gm-tools";
import { ArkToolsInstallationProvider } from "../../src/components/ArkToolsInstallationContext";
import { useArkToolsActivity } from "../../src/components/ArkToolsInstallActivity";
import { ActivityBar } from "../../src/components/ActivityBar";
import { createActivityNoticeStore } from "../../src/components/activity-notice-store";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";
import "../../src/views/servers/ark-world-map.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const errors: string[] = [];
addEventListener("error", event => errors.push(event.message));
addEventListener("unhandledrejection", event => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
let checks = 0;
let baseDetails: InstanceDetails;
let railWidth: number | null = null;
let installed = false;
let connected = false;
let holdStatus = false;
const statusReplies: Array<(value: ArkToolsStatus) => void> = [];
let mode: "response" | "invalid" | "failure" | "deferred" = "response";
let resolveSpawn: ((result: ArkSpawnResult) => void) | undefined;
let installMode: "failure" | "deferred" = "failure";
let resolveInstall: ((result: ArkToolsPreparationResult) => void) | undefined;
let installProgress: ArkToolsInstallProgress | null = null;
let progressReadFailure = false;
const extensionStates = new Map<string, ArkToolsStatus>();
let removeMode: "failure" | "invalid" | "deferred" = "failure";
let resolveRemove: ((result: ArkToolsRemovalResult) => void) | undefined;
const removals: string[] = [];
const removed: ArkToolsRemovalResult = { installed: false, connected: false, canUninstall: false, issue: null, frameworkRetained: false };
const player: ArkMapPlayer = { playerId: 12345, name: "测试玩家", accountId: "fixture-account", team: 1000, x: 100, y: 200, z: 300 };
let props: React.ComponentProps<typeof ArkCreatureSpawner> = { instanceId: "ark-a", instanceName: "方舟测试服", moduleId: "arksurvivalascended", status: "Stopped", processKey: "main", runId: 7,
  selectedPlayer: null,
  settingsJson: JSON.stringify({ rcon_enabled: true, admin_password: "fixture-admin-password", mod_ids_csv: "123456" }) };
const spawnCalls: ArkSpawnInput[] = [];
const installs: Array<{ instanceId: string; operationId: string; locale: string; allowMatchingSymbolsDownload: boolean }> = [];
let clipboardCalls = 0;
let changeLocale: ((locale: LocaleCode) => void) | undefined;
function LocaleControl() { changeLocale = useI18n().setLocale; return null; }
function ActivityFooter() {
  const [store] = useState(createActivityNoticeStore);
  const activity = useArkToolsActivity();
  return <footer className="shell-activity-bar" role="status" aria-live="polite" aria-atomic="false">
    <span className="shell-activity-label">动态</span>
    <ActivityBar store={store} entries={activity.outcomes} progressEntries={activity.progressEntries} />
  </footer>;
}
function FixtureItems() {
  const tool = getGmToolCatalog(props.moduleId)?.tools.find(value => value.id === "ark_give_item_to_player");
  if (!tool) throw new Error("Missing ARK item tool definition");
  const [values, setValues] = useState(() => getInitialGmToolValues(tool)), [query, setQuery] = useState("");
  const [targetMode, setTargetMode] = useState<"player" | "manual">("player");
  const targetValues = targetMode === "player" ? { ...values, playerId: String(props.selectedPlayer?.playerId ?? "") } : values;
  return <ArkItemSpawnPanel tool={tool} values={targetValues} query={query} onQueryChange={setQuery}
    update={(field, value) => setValues(current => ({ ...current, [field.key]: value }))}
    selectedPlayer={props.selectedPlayer ?? null} targetMode={targetMode} onTargetModeChange={setTargetMode}
    busy={false} sending={false} issue={null} onSubmit={() => { throw new Error("Unexpected item dispatch"); }} feedback={null} result={null} />;
}

function result(input: ArkSpawnInput): ArkSpawnResult {
  return { instanceId: input.instanceId, processKey: input.processKey, runId: input.runId, requestId: input.requestId, creature: {
    id1: 123, id2: 456, className: "Rex_Character_BP_C", level: input.level, team: input.tamed ? 1000 : 1,
    x: input.x, y: input.y, z: input.z, tamed: input.tamed
  } };
}
// Only the native IPC boundary is controlled; the production form, API input,
// result validation and operation lifetime run unchanged in a real browser.
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: (command: string, args: { input: ArkSpawnInput & { operationId: string; locale: string; allowMatchingSymbolsDownload: boolean } }) => {
  if (command === "read_ark_tools_status") return holdStatus
    ? new Promise<ArkToolsStatus>(resolve => statusReplies.push(resolve))
    : Promise.resolve(extensionStates.get(args.input.instanceId) ?? { installed, connected, canUninstall: installed, issue: installed ? null : "Prepare the ARK server extension while this instance is stopped" });
  if (command === "read_ark_tools_install_progress") return progressReadFailure ? Promise.reject(new Error("progress pipe unavailable")) : Promise.resolve(installProgress?.instanceId === args.input.instanceId
    && (args.input.operationId == null || installProgress.operationId === args.input.operationId) ? { ...installProgress } : null);
  if (command === "prepare_ark_tools") {
    installs.push(args.input);
    installProgress = queuedArkToolsInstallation(args.input.instanceId, args.input.operationId);
    if (installMode === "failure") {
      installProgress = { ...installProgress, phase: "failed", active: false, elapsedSeconds: 21, error: "api.gitcode.com: connection reset; github.com: timed out" };
      return Promise.reject(installProgress.error);
    }
    return new Promise<ArkToolsPreparationResult>(resolve => { resolveInstall = resolve; });
  }
  if (command === "remove_ark_tools") {
    removals.push(args.input.instanceId);
    if (removeMode === "failure") return Promise.reject(new Error("ARK extension file is in use; stop the server and retry"));
    if (removeMode === "invalid") return Promise.resolve({ ...removed, installed: true });
    return new Promise<ArkToolsRemovalResult>(resolve => { resolveRemove = receipt => {
      extensionStates.set(args.input.instanceId, receipt); resolve(receipt);
    }; });
  }
  if (command !== "spawn_ark_creature") throw new Error(`Unexpected IPC ${command}`);
  spawnCalls.push(args.input);
  if (mode === "deferred") return new Promise<ArkSpawnResult>(resolve => { resolveSpawn = resolve; });
  if (mode === "failure") return Promise.reject(new Error("Native creature class could not be loaded"));
  return Promise.resolve(mode === "invalid" ? { ...result(args.input), requestId: "wrong-request" } : result(args.input));
} } });
Object.defineProperty(navigator, "clipboard", { configurable: true, value: {
  writeText: async () => { clipboardCalls++; throw new Error("Spawning must not use the clipboard"); }
} });

function check(value: unknown, message: string): asserts value { if (!value) throw new Error(message); checks++; }
function element<T extends HTMLElement = HTMLElement>(selector: string): T {
  const node = fixture.querySelector<T>(selector); if (!node) throw new Error(`Missing ${selector}`); return node;
}
function namedButton(name: string) {
  const button = Array.from(fixture.querySelectorAll<HTMLButtonElement>("button")).find(node => (node.getAttribute('aria-label') || node.textContent?.trim()) === name);
  if (!button) throw new Error(`Missing button ${name}; rendered: ${fixture.textContent}`); return button;
}
async function click(node: HTMLElement) { await act(async () => { node.click(); }); }
async function waitFor(checkReady: () => boolean, message: string) {
  const deadline = performance.now() + 5000;
  while (!checkReady()) {
    if (performance.now() > deadline) throw new Error(message);
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 15)); });
  }
}
async function render(next: Partial<typeof props> = {}, showSpawner = true, showItems = false, width = railWidth) {
  props = { ...props, ...next };
  railWidth = width;
  const running = props.runtimeRunning ?? props.status === "Running";
  const details: InstanceDetails = { ...baseDetails, summary: { ...baseDetails.summary, id: props.instanceId, name: props.instanceName,
    module_id: props.moduleId, status: props.status === "Running" ? "Running" : "Stopped", active_process_count: running ? 1 : 0 },
    settings_json: props.settingsJson, active_run: running && props.runId ? { run_id: props.runId } : null };
  const context: ArkMapToolContext = { scope: props.runId ? { instanceId: props.instanceId, processKey: props.processKey, runId: props.runId } : null,
    scopeKey: `${props.instanceId}:${props.processKey}:${props.runId}:${running}`, running, live: running, selectedPlayer: props.selectedPlayer ?? null, extension: props.extensionStatus ?? null,
    send: async () => { throw new Error("Unexpected map command"); } };
  await act(async () => root.render(<StrictMode><I18nProvider><ArkToolsInstallationProvider><LocaleControl />
    <div style={{ height: '100vh', display: 'flex', flexDirection: 'column' }}>
      <main style={{ padding: 24, width: '100%', maxWidth: railWidth === null ? 1000 : railWidth + 48, margin: "0 auto", flex: 1, minHeight: 0, boxSizing: 'border-box', display: 'flex' }}>
        {showSpawner || showItems ? <div className="ark-map-tools">
          <div className="ark-creature-tool" hidden={!showSpawner}><ArkCreatureSpawner {...props} /></div>
          {showItems ? <FixtureItems /> : null}
          <ArkWorldToolsPanel details={details} context={context} busy={false} sendingToolId={null}
            onSend={async () => { throw new Error("Unexpected world command"); }} result={null} />
        </div> : <span>其他页面</span>}
      </main><ActivityFooter />
    </div></ArkToolsInstallationProvider></I18nProvider></StrictMode>));
  if (!showSpawner) return;
  const deadline = performance.now() + 5000;
  while (!fixture.querySelector(".ark-creature-spawner")) {
    if (performance.now() >= deadline) throw new Error("ARK tool did not finish loading");
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 10)); });
  }
}
async function input(node: HTMLInputElement | HTMLSelectElement, value: string) {
  const proto = node instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
  await act(async () => {
    Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(node, value);
    node.dispatchEvent(new Event(node instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  });
}
async function locale(value: LocaleCode) {
  await act(async () => changeLocale!(value));
  const label = value === 'en-US' ? 'Spawn creature' : '生成生物';
  const deadline = performance.now() + 5000;
  while (!fixture.querySelector(`button[type="submit"][aria-label="${label}"]`)) {
    if (performance.now() >= deadline) throw new Error(`Spawner did not finish switching to ${value}`);
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 15)); });
  }
}
const submit = () => element<HTMLButtonElement>('.ark-creature-spawner button[type="submit"]');
function actionHelp(button: HTMLElement): string {
  const anchor = button.closest('.configuration-action-help');
  return document.getElementById(anchor?.getAttribute('aria-describedby') ?? '')?.textContent ?? '';
}
const resultSelector = ".ark-creature-spawner > .shell-activity-notice.is-success";
const removalResultSelector = ".ark-extension-feedback .shell-activity-notice.is-success";
async function coordinates() {
  const fields = fixture.querySelectorAll<HTMLInputElement>(".ark-creature-spawner__coordinates input");
  for (const [index, value] of ["100", "200", "300"].entries()) await input(fields[index], value);
}
async function removalChecks() {
  await render({ instanceId: "ark-a", status: "Running" }, true, false, 328);
  check(namedButton("安装服务端扩展").disabled && namedButton("卸载服务端扩展").disabled && actionHelp(namedButton("卸载服务端扩展")).includes("停止实例"),
    "An installed extension keeps both permanent actions disabled while running and explains the stop requirement");
  await click(namedButton("卸载服务端扩展"));
  check(removals.length === 0 && !fixture.querySelector(".inline-confirm-review"), "A running instance cannot open or dispatch uninstall");
  connected = false;
  await render({ status: "Stopped" });
  check(namedButton("安装服务端扩展").disabled && !namedButton("卸载服务端扩展").disabled,
    "A stopped installed extension disables installation and permits uninstall");
  const uninstall = namedButton("卸载服务端扩展");
  const uninstallBox = uninstall.getBoundingClientRect(), installBox = namedButton("安装服务端扩展").getBoundingClientRect();
  const timeBox = element('#gm-ark_set_time-time-input').closest('label')!.getBoundingClientRect();
  const toolbarBox = element(".ark-world-tool-grid").getBoundingClientRect();
  check(Math.abs(toolbarBox.width - 328) < 1 && [namedButton("改时间"), namedButton("清除野生龙"), namedButton("安装服务端扩展"), uninstall].every(button => {
    const box = button.getBoundingClientRect();
    return box.top >= timeBox.top && Math.abs(box.width - timeBox.width) < 1 && box.height === 46
      && box.left >= toolbarBox.left && box.right <= toolbarBox.right + 1
      && button.scrollWidth <= button.clientWidth + 1;
  }) && getComputedStyle(element('.ark-world-tool-grid')).gridTemplateColumns.split(' ').length === 4,
  "The real 328px map rail keeps all world controls in fixed-height, equal-width cells in DST's four-column grid");
  check(uninstall.classList.contains("secondary-button") && uninstall.classList.contains("ark-spawn-tool")
    && uninstall.textContent?.trim() === "卸载" && uninstall.querySelector("svg")
    && Math.abs(uninstallBox.width - installBox.width) < 1 && Math.abs(uninstallBox.height - installBox.height) < 1
    && Math.abs(uninstallBox.left - timeBox.left) < 1 && Math.abs(uninstallBox.top - installBox.bottom - 6) < 1,
    "Install and uninstall retain matching icon-button sizes when the four-column grid wraps");
  await click(namedButton("卸载服务端扩展"));
  const review = element(".inline-confirm-review");
  check(review.textContent?.includes("玩家地图和生物工具将停用") && review.textContent?.includes("游戏存档及已经生成的生物会保留")
    && review.textContent?.includes("其他 ArkApi 插件") && removals.length === 0,
    "Uninstall requires reviewing tool, save, creature and shared-framework effects before invoking native removal");
  const reviewBox = review.getBoundingClientRect();
  const parametersBox = element(".ark-world-tool-grid").getBoundingClientRect();
  check(reviewBox.left >= 0 && reviewBox.right <= innerWidth && reviewBox.bottom <= innerHeight
    && document.documentElement.scrollWidth <= innerWidth + 1 && Math.abs(reviewBox.width - parametersBox.width) < 1
    && reviewBox.top >= namedButton("改时间").getBoundingClientRect().bottom,
    "Uninstall confirmation spans the row below the world actions and remains inside the viewport without horizontal overflow");
  check(document.activeElement === namedButton("取消"), "The uninstall review initially focuses Cancel");
  await click(namedButton("取消"));
  check(removals.length === 0 && !fixture.querySelector(".inline-confirm-review"), "Cancelling uninstall sends no native operation");
  check(Math.abs(namedButton("卸载服务端扩展").getBoundingClientRect().top - uninstallBox.top) < 1,
    "Cancelling confirmation restores uninstall to its permanent grid position");
  await click(namedButton("卸载服务端扩展"));
  await click(namedButton("确认卸载"));
  check(removals.length === 1 && removals[0] === "ark-a" && element("[role=alert]").textContent?.includes("file is in use")
    && !namedButton("卸载服务端扩展").disabled && namedButton("安装服务端扩展").disabled,
    "A native removal failure preserves installed actions, exposes the cause and permits a deliberate retry");
  removeMode = "invalid";
  await click(namedButton("卸载服务端扩展"));
  await click(namedButton("确认卸载"));
  check(removals.length === 2 && element("[role=alert]").textContent?.includes("Invalid ARK removal completion receipt")
    && !fixture.querySelector(removalResultSelector), "An inconsistent native removal receipt cannot claim successful uninstall");
  removeMode = "deferred";
  await click(namedButton("卸载服务端扩展"));
  const confirm = namedButton("确认卸载");
  await click(confirm);
  await click(confirm);
  check(removals.length === 3 && confirm.disabled && namedButton("安装服务端扩展").disabled && submit().disabled,
    "Pending uninstall sends one request and blocks duplicate confirmation, installation and creature spawning");
  check(!fixture.querySelector('[role="progressbar"]'), "Removal never invents installation download progress");
  const requests = statusReplies.length;
  holdStatus = true;
  await render({ settingsJson: JSON.stringify({ ...JSON.parse(props.settingsJson), removal_revision: 1 }) });
  check(statusReplies.length === requests, "Settings changes cannot launch a stale installed-status read during removal");
  holdStatus = false;
  await render({}, false);
  await render();
  check(namedButton("安装服务端扩展").disabled && namedButton("卸载服务端扩展").disabled && removals.length === 3, "Leaving and returning to the tool page preserves the removal lock");
  await render({ instanceId: "ark-uninstall-other" });
  await act(async () => resolveRemove!({ ...removed, frameworkRetained: true }));
  check(!namedButton("卸载服务端扩展").disabled && !fixture.querySelector(removalResultSelector), "A late removal receipt does not change another selected instance");
  await render({ instanceId: "ark-a" });
  check(!namedButton("安装服务端扩展").disabled && namedButton("卸载服务端扩展").disabled
    && element(removalResultSelector).textContent?.includes("共享框架") && element(removalResultSelector).textContent?.includes("已保留"),
    "Successful uninstall enables installation, retains disabled uninstall and reports the framework preserved for other plugins");
  await render({}, false);
  await render();
  check(!namedButton("安装服务端扩展").disabled && namedButton("卸载服务端扩展").disabled && submit().disabled
    && actionHelp(submit()).includes("安装服务端扩展"),
    "Remounting cannot resurrect cached installation success after uninstall");

  const oldExtension = { installed: false, connected: false, canUninstall: true, issue: "Managed extension requires an update" };
  extensionStates.set("ark-old-install", oldExtension);
  await render({ instanceId: "ark-old-install" });
  check(!namedButton("卸载服务端扩展").disabled && !namedButton("安装服务端扩展").disabled,
    "An outdated managed extension remains uninstallable even when installed is false");
  await click(namedButton("安装服务端扩展"));
  check(namedButton("卸载服务端扩展").disabled, "Installing an update disables uninstall");
  await click(namedButton("卸载服务端扩展"));
  check(removals.length === 3, "No removal is dispatched while installation is active");
  const ready = { installed: true, connected: false, canUninstall: true, issue: null };
  extensionStates.set("ark-old-install", ready);
  installProgress = { ...installProgress!, active: false, phase: "ready", elapsedSeconds: 2 };
  await act(async () => resolveInstall!({ ...ready, progress: installProgress! }));
  extensionStates.set("ark-old-remove", oldExtension);
  await render({ instanceId: "ark-old-remove" });
  const beforeInstall = installs.length;
  await click(namedButton("卸载服务端扩展"));
  await click(namedButton("确认卸载"));
  check(namedButton("安装服务端扩展").disabled && removals.length === 4 && removals[3] === "ark-old-remove",
    "A stopped outdated extension can be removed while the install action is locked");
  await click(namedButton("安装服务端扩展"));
  check(installs.length === beforeInstall, "Removal and installation cannot run together");
  await act(async () => resolveRemove!(removed));
  check(!namedButton("安装服务端扩展").disabled && namedButton("卸载服务端扩展").disabled,
    "Removing an outdated extension returns to the clean installation state");
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  const bootstrap = await invokeMock<BootstrapResponse>("bootstrap", { includeSystemSnapshot: false });
  const instance = bootstrap.state.instances.find(value => value.module_id === "minecraft");
  if (!instance) throw new Error("Missing fixture instance");
  baseDetails = await invokeMock<InstanceDetails>("read_instance_details_from_storage", { instanceId: instance.id, instance_id: instance.id });
  await render();
  check(!namedButton("安装服务端扩展").disabled && namedButton("卸载服务端扩展").disabled,
    "A stopped uninstalled extension keeps installation enabled and uninstall visible but disabled");
  check(!fixture.querySelector('[data-ark-tools-action="check"]')
    && element(".ark-creature-spawner__parameters").querySelectorAll('input, select, button').length === 3
    && getComputedStyle(element(".ark-creature-spawner__parameters")).gridTemplateColumns.split(' ').length === 4,
    "The spawn row keeps level, state and spawn in the shared four-column grid, without a redundant connection button");
  const worldActions = element(".ark-world-tools");
  const permanentActions = [namedButton("改时间"), namedButton("清除野生龙"), namedButton("安装服务端扩展"), namedButton("卸载服务端扩展")];
  const worldRow = element('#gm-ark_set_time-time-input').closest('label')!.getBoundingClientRect();
  check(permanentActions.every((button, index) => worldActions.contains(button) && button.getClientRects().length
    && Math.abs(button.getBoundingClientRect().width - worldRow.width) < 1 && button.getBoundingClientRect().height === 46
    && Math.abs(button.getBoundingClientRect().top - worldRow.top - Math.floor((index + 1) / 4) * 52) < 1)
    && !element(".ark-creature-spawner").contains(namedButton("安装服务端扩展")),
    "Install and uninstall remain with time and world actions rather than inside the creature form");
  await render({}, false, true);
  check(element(".ark-item-spawn").getClientRects().length && element<HTMLElement>(".ark-creature-tool").hidden
    && element(".ark-world-tools") === worldActions && namedButton("安装服务端扩展").getClientRects().length
    && namedButton("卸载服务端扩展").getClientRects().length,
    "Switching to the real item panel preserves the same permanent extension actions");
  await render();
  check(!fixture.querySelector('.ark-creature-spawner__status, .ark-creature-spawner__note')
    && !fixture.textContent?.includes('Prepare the ARK server extension'), 'Routine extension state must not occupy a banner or expose the raw English setup message');
  check(submit().disabled && actionHelp(submit()).includes('安装服务端扩展'), 'The unavailable spawn action explains its missing extension prerequisite');
  check(!fixture.querySelector('.ark-creature-spawner__coordinates, .ark-map-use-player'),
    'Default player spawning requires neither coordinate fields nor a separate use-player click');
  check(!element<HTMLDetailsElement>('.ark-spawn-options').open && element('.ark-spawn-options summary').textContent?.includes('自定义生物'),
    'Optional manual position and custom creature controls start collapsed');
  await locale('en-US');
  check(actionHelp(submit()).includes('Install the server extension'), 'The missing extension reason follows the selected UI language');
  await locale('zh-CN');
  const installAnchor = namedButton('安装服务端扩展').closest<HTMLElement>('.configuration-action-help')!;
  await act(async () => installAnchor.focus());
  const tooltipUntil = performance.now() + 3000;
  while (!document.querySelector('.configuration-field-help-tooltip.is-visible')) {
    if (performance.now() > tooltipUntil) throw new Error('Install help did not open on keyboard focus');
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 15)); });
  }
  check(document.querySelector('.configuration-field-help-tooltip.is-visible')?.textContent === '安装本实例的玩家地图与生物工具扩展。',
    'The uninstalled extension keeps a concise install tooltip for both ARK editions');
  await act(async () => installAnchor.blur());
  await render({ status: 'Running' });
  check(namedButton('安装服务端扩展').disabled && namedButton('卸载服务端扩展').disabled,
    'Both permanent extension actions reject a running instance');
  check(actionHelp(submit()).includes('停止实例') && actionHelp(submit()).includes('安装服务端扩展'), 'Spawn explains that installing a missing extension requires stopping first');
  const runningAnchor = namedButton('安装服务端扩展').closest<HTMLElement>('.configuration-action-help')!;
  const reason = document.getElementById(runningAnchor.getAttribute('aria-describedby')!);
  check(reason?.textContent?.includes('请先停止实例'), 'The disabled action explains how to enable installation');
  await render({ status: 'Stopped' });
  await click(namedButton("安装服务端扩展"));
  check(element(".shell-activity-bar .shell-activity-notice.is-error").textContent?.includes("21 秒"), "Failed installation preserves its native final elapsed time in the global activity bar");
  await click(namedButton("消息详情"));
  check(element(".shell-activity-notice-diagnostics").textContent?.includes("connection reset"), "Immediate native installation failure keeps its concrete network cause in activity details");
  await render({ settingsJson: JSON.stringify({ ...JSON.parse(props.settingsJson), install_revision: 1 }) });
  check(element(".shell-activity-notice-diagnostics").textContent?.includes("connection reset"), "A background handshake must not erase installation diagnostics");
  await click(element('.shell-activity-panel-close'));
  installMode = "deferred";
  await click(namedButton("安装服务端扩展"));
  check(installs.length === 2 && installs[1].instanceId === "ark-a" && installs[1].allowMatchingSymbolsDownload
    && installs[1].locale === "zh-CN" && /^[a-f0-9]{32}$/.test(installs[1].operationId) && installs[0].operationId !== installs[1].operationId,
    "Installation passes consent, locale and a fresh operation identity");
  check(namedButton("安装服务端扩展").disabled && Boolean(fixture.querySelector('[role="progressbar"]')),
    "Pending installation shows immediate progress and blocks duplicate dispatch");
  check(!fixture.querySelector('.ark-creature-spawner [role="progressbar"]') && fixture.querySelector('.shell-activity-bar [role="progressbar"]'),
    "Extension installation progress belongs to the global activity bar");
  await click(namedButton("安装服务端扩展"));
  check(installs.length === 2, "A second install click cannot start another native operation");
  installProgress = { ...installProgress!, phase: "downloading", source: "api.gitcode.com", downloadedBytes: 524288, totalBytes: 1048576 };
  await waitFor(() => fixture.querySelector('[role="progressbar"]')?.getAttribute('aria-valuenow') === "50", "Real installation byte progress was not displayed");
  check(fixture.textContent?.includes("512 KB / 1 MB") && fixture.textContent?.includes("api.gitcode.com"), "The install progress exposes current bytes and download source");
  await render({ settingsJson: JSON.stringify({ ...JSON.parse(props.settingsJson), install_revision: 2 }) });
  check(namedButton("安装服务端扩展").disabled, "Settings refresh retains the active installation lock");
  installed = true;
  installProgress = { ...installProgress, active: false, phase: "ready", elapsedSeconds: 8 };
  await act(async () => resolveInstall!({ installed, connected, canUninstall: true, issue: null, progress: installProgress! }));
  check(!fixture.querySelector('[role="progressbar"]') && fixture.textContent?.includes("服务端扩展已安装"), "Verified installation closes progress with an inspectable result");
  check(element('.shell-activity-bar .shell-activity-notice-text').textContent?.includes('8 秒'),
    "The completion receipt replaces the last zero-second poll with the native eight-second duration");
  check(submit().disabled, "A stopped instance must not allow spawning");
  check(actionHelp(submit()).includes('启动当前地图'), 'An installed extension explains that the map must be started');
  connected = true;
  await render({ status: "Running" });
  check(submit().disabled && actionHelp(submit()).includes('选择'),
    'A connected extension still requires a selected player in default mode');
  await click(submit());
  check(spawnCalls.length === 0, 'No player must never silently spawn at zero coordinates');
  await render({ selectedPlayer: player });
  check(!submit().disabled, 'Selecting a live player immediately enables spawning');
  await render({ extensionStatus: { installed: true, connected: false, canUninstall: true, issue: null } });
  check(submit().disabled && actionHelp(submit()).includes('自动更新'),
    'An unavailable map handshake disables spawning while automatic connection checks continue');
  await render({ extensionStatus: { installed: true, connected: true, canUninstall: true, issue: null } });
  check(!submit().disabled && !fixture.querySelector('[data-ark-tools-action="check"]'),
    'A recovered map handshake re-enables spawning without a manual connection action');
  await render({ extensionStatus: undefined });
  await input(element<HTMLInputElement>('input[type="search"]'), "rex");
  check(Array.from(fixture.querySelectorAll(".gmt-ark-catalog-option")).some(node => node.textContent?.includes("霸王龙")),
    "Real creature search must show translated names");
  await click(element('.ark-spawn-options summary'));
  await input(element<HTMLInputElement>(".ark-creature-spawner__saved input"), "回归测试霸王龙");
  await click(namedButton("收藏生物"));
  check(JSON.parse(localStorage.getItem("lsgm.gmTools.arkCreatures.ark-a") ?? "[]")[0]?.label === "回归测试霸王龙",
    "Saved creature entries must preserve the existing per-instance storage contract");
  await click(element('.ark-spawn-options summary'));
  await click(submit());
  check(spawnCalls.length === 1 && spawnCalls[0].x === 100 && spawnCalls[0].y === 200 && spawnCalls[0].z === 300
    && spawnCalls[0].creature.endsWith("/Rex_Character_BP.Rex_Character_BP_C") && !spawnCalls[0].tamed,
    "Default wild spawning uses the selected player's position without editing coordinates");
  check(/^[a-f0-9]{32}$/.test(spawnCalls[0].requestId), "Spawn requests must have a unique correlation ID");
  check(element(resultSelector).textContent?.includes("ID 123:456"), "Native entity identity must be visible");
  check(clipboardCalls === 0, "One-click spawning must never copy an in-game command");
  await render({ selectedPlayer: { ...player, x: 150, y: 250, z: 350 } });
  await input(element<HTMLSelectElement>(".ark-creature-spawner__parameters select"), "true");
  check(!fixture.querySelector('.ark-creature-spawner__coordinates, input[max="4294967295"]'),
    'Default tamed spawning requires no manual coordinates or owner fields');
  await click(submit());
  check(spawnCalls.length === 2 && spawnCalls[1].tamed && spawnCalls[1].playerId === 12345
    && spawnCalls[1].x === 150 && spawnCalls[1].y === 250 && spawnCalls[1].z === 350,
    'Tamed spawning automatically uses the selected owner and their latest polled position');
  await render({ selectedPlayer: null });
  await click(submit());
  check(submit().disabled && spawnCalls.length === 2, 'Clearing or expiring the selected player prevents reuse of their old position');
  await render({ selectedPlayer: { ...player, y: null } });
  check(submit().disabled && actionHelp(submit()).includes('位置'), 'A player without complete character coordinates cannot be a spawn target');
  await render({ selectedPlayer: { ...player, playerId: 6789, name: '玩家乙', x: 400, y: 500, z: 600 } });
  mode = "invalid";
  await click(submit());
  check(spawnCalls[2].playerId === 6789 && spawnCalls[2].x === 400 && spawnCalls[2].y === 500 && spawnCalls[2].z === 600,
    'Changing the selected player immediately changes both spawn location and owner');
  check(!fixture.querySelector(resultSelector) && element("[role=alert]").textContent?.includes("Invalid ARK creature read-back"),
    "Mismatched native read-back must never display a verified creature");
  await click(element('.ark-spawn-options summary'));
  await input(element<HTMLSelectElement>('[aria-label="生成位置来源"]'), 'manual');
  await act(async () => { element<HTMLDetailsElement>('.ark-spawn-options').open = false; });
  await click(submit());
  check(spawnCalls.length === 3 && element<HTMLDetailsElement>('.ark-spawn-options').open,
    'Incomplete manual coordinates block dispatch and reveal the optional fields');
  await coordinates();
  await click(submit());
  check(spawnCalls.length === 3, 'Manual tamed spawning still requires an owner ID');
  await input(element<HTMLInputElement>('input[max="4294967295"]'), '777');
  await render({ selectedPlayer: { ...player, x: 800, y: 900, z: 1000 } });
  await act(async () => { element<HTMLDetailsElement>('.ark-spawn-options').open = false; });
  check(element('.ark-spawn-options summary').textContent?.includes('已启用'), 'The collapsed summary identifies an active manual location override');
  mode = "failure";
  await click(submit());
  check(spawnCalls[3].x === 100 && spawnCalls[3].y === 200 && spawnCalls[3].z === 300 && spawnCalls[3].playerId === 777,
    'Explicit manual coordinates and owner are not overwritten by subsequent player polling');
  check(element("[role=alert]").textContent?.includes("Native creature class could not be loaded"), "Native failure must remain visible without retry");
  mode = "deferred";
  await click(submit());
  const pending = spawnCalls[spawnCalls.length - 1];
  check(submit().disabled, "Pending spawn must prevent another click");
  await render({ settingsJson: JSON.stringify({ ...JSON.parse(props.settingsJson), ui_refresh: true }) });
  check(submit().disabled && spawnCalls.length === 5, "Settings refresh must not release a native spawn lock");
  await render({ instanceId: "ark-b" });
  check(submit().disabled && !fixture.querySelector(resultSelector), "Instance switch must keep the in-flight lock and clear prior results");
  await act(async () => resolveSpawn!(result(pending)));
  check(!submit().disabled && !fixture.querySelector(resultSelector) && !fixture.querySelector('.ark-creature-spawner__coordinates'),
    'Late results must not pollute the selected instance, which resets to default player targeting');
  holdStatus = true;
  await render({ settingsJson: JSON.stringify({ ...JSON.parse(props.settingsJson), status_revision: 1 }) });
  const oldReplies = statusReplies.slice();
  await render({ settingsJson: JSON.stringify({ ...JSON.parse(props.settingsJson), status_revision: 2 }) });
  const newReplies = statusReplies.slice(oldReplies.length);
  check(oldReplies.length === 2 && newReplies.length === 2, "Settings changes recheck both connection and extension controls");
  await act(async () => { newReplies.forEach(reply => reply({ installed: true, connected: false, canUninstall: true, issue: "new disconnected state" })); });
  await act(async () => { oldReplies.forEach(reply => reply({ installed: true, connected: true, canUninstall: true, issue: null })); });
  check(submit().disabled && actionHelp(submit()).includes("new disconnected state") && !fixture.textContent?.includes("服务端扩展已连接"),
    "An older handshake must not overwrite the current connection state");
  holdStatus = false;
  await render({ settingsJson: JSON.stringify({ ...JSON.parse(props.settingsJson), status_revision: 3 }) });
  mode = "response";
  await input(element<HTMLSelectElement>(".ark-creature-spawner__parameters select"), "false");
  await input(element<HTMLInputElement>('input[type="search"]'), "rex");
  await click(submit());
  check(spawnCalls[5].x === 800 && spawnCalls[5].y === 900 && spawnCalls[5].z === 1000,
    'The new instance uses the current player rather than the previous manual override');
  const feedback = element(resultSelector);
  await act(async () => { element<HTMLDetailsElement>('.ark-spawn-options').open = false; element<HTMLInputElement>('.gmt-ark-catalog-search').focus(); });
  const catalogBox = element('.gmt-ark-catalog-results').getBoundingClientRect();
  const actionBox = submit().getBoundingClientRect();
  check(catalogBox.height >= 52 && catalogBox.bottom <= actionBox.top && actionBox.bottom <= innerHeight,
    `Creature catalog and bottom actions fit independently: ${JSON.stringify({ catalog: catalogBox.toJSON(), action: actionBox.toJSON(), form: element('form').getBoundingClientRect().toJSON(), fieldset: element('fieldset').getBoundingClientRect().toJSON() })}`);
  check(feedback.textContent?.includes("已生成并读回确认") && feedback.textContent?.includes("ID 123:456"), "Verified result must remain inspectable");
  check(document.documentElement.scrollWidth <= innerWidth + 1, "ARK spawning must not overflow horizontally");
  check(element('.shell-activity-bar .shell-activity-notice-text').textContent?.includes('8 秒'), "Finished elapsed time remains frozen across page work");
  await click(element('.shell-activity-bar .shell-activity-notice-close'));
  await removalChecks();
  installed = false;
  connected = false;
  await render({ instanceId: "ark-install-preview", status: "Stopped", selectedPlayer: null });
  await click(namedButton("安装服务端扩展"));
  installProgress = { ...installProgress!, phase: "downloading", source: "api.gitcode.com", downloadedBytes: 524288, totalBytes: 1048576, elapsedSeconds: 90 };
  await waitFor(() => fixture.querySelector('[role="progressbar"]')?.getAttribute('aria-valuenow') === "50", "Installation preview did not show current bytes");
  const elapsed = () => Number(element('.shell-activity-progress [role="timer"]').textContent?.match(/\d+/)?.[0]);
  check(elapsed() >= 90, "An observed installation retains the native elapsed duration");
  progressReadFailure = true;
  await waitFor(() => elapsed() >= 91, "The elapsed clock stopped during progress-read failure");
  const elapsedBeforeNavigation = elapsed();
  const preparationsBeforeNavigation = installs.length;
  await render({}, false);
  check(!fixture.querySelector('.ark-creature-spawner') && Boolean(fixture.querySelector('.shell-activity-bar [role="progressbar"]')),
    "Leaving the tool page preserves the global installation progress");
  await click(namedButton('消息详情'));
  check(Array.from(fixture.querySelectorAll('.shell-activity-bar [role="timer"]')).every(timer => Number(timer.textContent?.match(/\d+/)?.[0]) >= elapsedBeforeNavigation),
    "Opening details uses the same elapsed clock instead of restarting from the old snapshot");
  await click(element('.shell-activity-panel-close'));
  await render();
  check(namedButton('安装服务端扩展').disabled && installs.length === preparationsBeforeNavigation && elapsed() >= elapsedBeforeNavigation,
    "Returning to the tool page preserves the operation lock and elapsed clock without installing again");
  const progressBox = element('.shell-activity-bar').getBoundingClientRect();
  check(progressBox.bottom <= innerHeight && document.documentElement.scrollWidth <= innerWidth + 1,
    "Installation progress remains visible without viewport overflow");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`);
  return { status: "passed", checks, browser_errors: errors, spawn_calls: spawnCalls.length, removal_calls: removals.length, clipboard_calls: clipboardCalls,
    viewport: { width: innerWidth, height: innerHeight } };
}
Object.assign(globalThis, { __reliabilityFixtureCleanup: async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  await act(async () => root.unmount());
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  return { browser_errors: errors, native_dialogs: 0 };
} });
void run().catch(error => ({ status: "failed", checks, error: error instanceof Error ? error.stack : String(error), browser_errors: errors }))
  .finally(() => Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false }))
  .then(report => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
