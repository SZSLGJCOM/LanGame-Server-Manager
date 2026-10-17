import { act } from "react";
import { createRoot } from "react-dom/client";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import { MockLivePlayerStore, type MockLivePlayerContext } from "../../src/api-mock/live-players";
import { I18nProvider, type LocaleCode } from "../../src/i18n";
import { PlayerCenterWorkbench } from "../../src/views/servers/PlayerCenterWorkbench";
import type { DragonwildsBanEntry, ExecuteInstanceManualPlayerActionInput, ExecuteInstancePlayerActionInput, ExecuteInstancePlayerActionResult,
  InstanceDetails, RuntimeLivePlayerEntry, RuntimeLivePlayerSnapshot } from "../../src/types";
import { prepareBrowserLocaleCatalogs } from "./browser-locale-catalogs";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
document.documentElement.dataset.theme = "dark";
const fixture = document.getElementById("fixture");
if (!fixture) throw new Error("Fixture root missing");
const root = createRoot(fixture);
const nonce = new URLSearchParams(location.search).get("nonce");
const checks: string[] = [], errors: string[] = [];
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
window.alert = window.confirm = window.prompt = () => { throw new Error("Native dialog prohibited"); };
const moduleDetails = buildMockModuleDetails("runescapedragonwilds");
const instance: InstanceDetails = {
  summary: { id: "dragonwilds-controls", module_id: "runescapedragonwilds", name: "Dragonwilds controls",
    status: "Running", active_process_count: 1, autostart: false, bind_ip: "127.0.0.1" },
  active_run: { run_id: 1 }, ports: [], settings_json: "{}", config_file_path: "", saves_path: "",
  backup_uses_declared_saves_path: false, auto_backup_on_stop: false, backup_retention_count: 3
};
const requests: ExecuteInstancePlayerActionInput[] = [];
const manualRequests: ExecuteInstanceManualPlayerActionInput[] = [];
const knownBannedAccount = "00000000000000000000000000000001";
let savedBanEntries: DragonwildsBanEntry[] = [];
let mode: "ready" | "fallback" | "stale" | "stopped" = "ready";
let behavior: "complete" | "deferred" | "failure" = "complete";
let rows: RuntimeLivePlayerEntry[] = [];
let store: MockLivePlayerStore;
let readCalls = 0, refreshCalls = 0;
let finishAction: (() => void) | null = null;
const context = (): MockLivePlayerContext => ({ instance_id: instance.summary.id, module_id: "runescapedragonwilds",
  running: mode !== "stopped", player_list: moduleDetails.runtime.player_list ?? null,
  current_players: rows.length, max_players: null, settings: {} });
function snapshot(refresh: boolean): RuntimeLivePlayerSnapshot {
  if (refresh) refreshCalls++; else readCalls++;
  const result = refresh ? store.refresh(context()) : store.read(context());
  return mode === "stale" ? { ...result, stale: true, expires_at_unix_ms: Date.now() - 1,
    entries: result.entries.map((entry) => ({ ...entry, available_action_ids: [] })) } : result;
}
// Only substitute the native IPC boundary. The real PlayerCenterWorkbench,
// live polling hook, API envelope, action form and confirmation stay intact.
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args: Record<string, unknown> = {}) => {
  if (command === "read_instance_live_players") return snapshot(false);
  if (command === "read_instance_dragonwilds_bans") return { instance_id: instance.summary.id,
    observed_at_unix_ms: Date.now(), entries: structuredClone(savedBanEntries) };
  if (command === "refresh_instance_live_players") return snapshot(true);
  if (command === "execute_instance_player_action") {
    const input = args.input as ExecuteInstancePlayerActionInput;
    requests.push(structuredClone(input));
    if (behavior === "failure") throw new Error("Synthetic native effect could not be confirmed");
    const apply = () => {
      const selected = store.read(context()).entries.find((entry) => entry.player_key === input.player_key);
      if (!selected) throw new Error("Synthetic target not current");
      const account = selected.identifiers.find((identity) => identity.kind === "eos_id")?.value;
      const result = store.execute(context(), input);
      if (input.action_id === "ban_player" && account) savedBanEntries.push({ account_id: account, display_name: selected.display_name });
      rows = rows.filter((entry) => !entry.identifiers.some((identity) => identity.kind === "eos_id" && identity.value === account));
      return result;
    };
    if (behavior === "deferred") return new Promise<ExecuteInstancePlayerActionResult>((resolve) => { finishAction = () => {
      const result = apply(); finishAction = null; resolve(result);
    }; });
    return apply();
  }
  if (command === "execute_instance_manual_player_action") {
    const input = args.input as ExecuteInstanceManualPlayerActionInput;
    if (input.action_id !== "unban_player" || input.instance_id !== instance.summary.id
      || !/^[0-9a-f]{32}$/.test(input.target) || input.role) throw new Error("Unexpected manual native target");
    if (!savedBanEntries.some((entry) => entry.account_id === input.target)) throw new Error("Synthetic EOS is not in the persisted ban roster");
    manualRequests.push(structuredClone(input));
    savedBanEntries = savedBanEntries.filter((entry) => entry.account_id !== input.target);
    return { action_id: input.action_id, status: "sent", executed_at_unix_ms: Date.now(), summary: "Synthetic unban confirmed" };
  }
  throw new Error(`Unexpected native command ${command}`);
} } });

function check(condition: unknown, description: string): asserts condition {
  if (!condition) throw new Error(`${description}: ${fixture?.textContent}`);
  checks.push(description);
}
function element<T extends Element>(selector: string): T {
  const value = fixture?.querySelector<T>(selector);
  if (!value) throw new Error(`Missing ${selector}: ${fixture?.textContent}`);
  return value;
}
async function settle(predicate: () => boolean, description: string) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    if (performance.now() > deadline) throw new Error(`${description}: ${fixture?.textContent}`);
    await act(async () => { await new Promise<void>((resolve) => requestAnimationFrame(() => resolve())); });
  }
}
async function draw(locale: LocaleCode, nextMode: typeof mode = "ready", bannedAccountId?: string) {
  mode = nextMode; behavior = "complete";
  savedBanEntries = bannedAccountId ? [{ account_id: bannedAccountId, display_name: "Synthetic banned account" }] : [];
  const sample = new MockLivePlayerStore().refresh({ ...context(), current_players: 2, running: true }).entries;
  rows = mode === "fallback" ? sample.map((entry) => ({ ...entry,
    identifiers: entry.identifiers.filter((identity) => identity.kind === "session_id"), available_action_ids: [] })) : sample;
  store = new MockLivePlayerStore({ collector: () => ({ status: "ready", complete: true, truncated: false,
    entries: structuredClone(rows), current_players: rows.length, issue: null }) });
  store.refresh(context());
  await act(async () => {
    localStorage.setItem("langame.locale", locale);
    root.render(<I18nProvider key={`${locale}-${mode}-${readCalls}`}><main style={{ height: "calc(100vh - 48px)", margin: 24,
      display: "grid", minHeight: 0 }}><section className="server-detail-panel"><div className="detail-stack detail-stack--server">
      <div className="server-detail-subheader">Dragonwilds · {locale === "zh-CN" ? "玩家" : "Players"}</div>
      <div className="server-detail-scroll server-detail-scroll--players"><PlayerCenterWorkbench
        details={mode === "stopped" ? { ...instance, summary: { ...instance.summary, status: "Stopped", active_process_count: 0 }, active_run: null } : instance}
        moduleDetails={moduleDetails} runtime={null} /></div>
    </div></section></main></I18nProvider>);
  });
  await settle(() => Boolean(fixture?.querySelector(mode === "stopped" ? ".dragonwilds-unban-action" : ".player-center-player-select")), "Player workspace failed to mount");
}
function liveAction(index: number): HTMLButtonElement {
  const buttons = fixture?.querySelectorAll<HTMLButtonElement>(".player-center-member-action");
  if (!buttons?.[index]) throw new Error(`Missing live action ${index}`);
  return buttons[index];
}
async function selectFirst() {
  const button = element<HTMLButtonElement>(".player-center-player-select");
  button.focus();
  await act(async () => { button.click(); });
  check(button.getAttribute("aria-pressed") === "true", "Keyboard-focusable player selection is announced");
}
async function enterTarget(value: string) {
  const input = element<HTMLInputElement>('input[id^="manual-player-target-"]');
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  if (!setter) throw new Error("Input value setter missing");
  await act(async () => { setter.call(input, value); input.dispatchEvent(new Event("input", { bubbles: true })); });
  const lookup = element<HTMLButtonElement>(".player-center-manual-use");
  if (!lookup.disabled) await act(async () => { lookup.click(); });
}
async function openManual() {
  // 手动查询 category: the ID lookup fills the list area and its actions use the right-side controls.
  await act(async () => { element<HTMLButtonElement>('[role="tab"][data-list-key="manual"]').click(); });
}
async function confirm() {
  await act(async () => { element<HTMLButtonElement>(".inline-confirm-submit").click(); });
}
// The right panel no longer repeats the selected identity; the highlighted left row is the target.
function selectedRowText() {
  const pressed = document.querySelector('.player-center-list-pane [aria-pressed="true"]');
  return (pressed?.closest('tr, [role="listitem"]') ?? pressed)?.textContent ?? "";
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  for (const locale of ["zh-CN", "en-US"] as const) {
    await draw(locale);
    check(readCalls > 0, `${locale}: actual player workspace reads through the live-player API hook`);
    check([liveAction(0), liveAction(1)].every((button) => button.disabled), `${locale}: kick and ban require current player selection`);
    await selectFirst();
    check(!liveAction(0).disabled && !liveAction(1).disabled, `${locale}: verified native EOS and session enable kick and ban`);
    check(liveAction(0).textContent?.trim() === (locale === "zh-CN" ? "踢出玩家" : "Kick player")
      && liveAction(1).textContent?.trim() === (locale === "zh-CN" ? "封禁玩家" : "Ban player"), `${locale}: current manifest supplies localized action labels`);
    check(selectedRowText().includes("00000000000000000000000000000001"), `${locale}: native EOS identity remains inspectable`);
    const start = requests.length;
    await act(async () => { liveAction(0).click(); });
    check(requests.length === start && Boolean(fixture?.querySelector(".inline-confirm-review")), `${locale}: opening kick confirmation sends no command`);
    behavior = "deferred";
    await confirm();
    check(requests.length === start + 1 && Boolean(finishAction), `${locale}: confirmed kick uses the production action API`);
    check(element<HTMLButtonElement>(".inline-confirm-submit").disabled
      && Array.from(fixture?.querySelectorAll<HTMLButtonElement>(".player-center-member-pane button") ?? []).every((button) => button.disabled),
      `${locale}: one pending native action disables duplicate operations`);
    check(Object.keys(requests[start]).sort().join(",") === "action_id,instance_id,player_key,snapshot_id",
      `${locale}: live action sends only authorized snapshot references`);
    const refreshBefore = refreshCalls;
    await act(async () => { finishAction?.(); });
    await settle(() => fixture?.querySelectorAll(".player-center-player-select").length === 1, "Kicked row did not refresh away");
    check(refreshCalls > refreshBefore, `${locale}: confirmed effect refreshes the real player polling hook`);
    await draw(locale);
    await selectFirst();
    await act(async () => { liveAction(1).click(); });
    await confirm();
    await settle(() => fixture?.querySelectorAll(".player-center-player-select").length === 1, "Banned row did not refresh away");
    check(requests.at(-1)?.action_id === "ban_player", `${locale}: online ban uses native runtime action instead of a settings mutation`);
    await draw(locale);
    await selectFirst(); behavior = "failure";
    await act(async () => { liveAction(0).click(); }); await confirm();
    check(fixture?.textContent?.includes("Synthetic native effect could not be confirmed"), `${locale}: native failure is visible without success feedback`);
    await draw(locale, "fallback"); await selectFirst();
    check([liveAction(0), liveAction(1)].every((button) => button.disabled), `${locale}: query-only fallback never gains account operations`);
    check([liveAction(0), liveAction(1)].every((button) => Boolean(button.getAttribute("aria-describedby"))),
      `${locale}: unavailable identity exposes accessible disabled reasons`);
    await draw(locale, "stale"); await selectFirst();
    check([liveAction(0), liveAction(1)].every((button) => button.disabled), `${locale}: stale snapshot disables live moderation`);
    await draw(locale, "stopped");
    await openManual();
    check(element<HTMLInputElement>('input[id^="manual-player-target-"]').disabled,
      `${locale}: stopped server cannot accept manual native operations`);
    await draw(locale, "ready", knownBannedAccount);
    const commonUnban = element<HTMLButtonElement>(".dragonwilds-unban-action");
    await openManual();
    check(element(".dragonwilds-unban-action") === commonUnban
      && fixture?.querySelectorAll(".dragonwilds-unban-action").length === 1
      && !fixture?.querySelector(".player-center-manual-actions")
      && [liveAction(0), liveAction(1)].every((button) => button.disabled),
      `${locale}: manual lookup shares one persistent unban action without granting live authority`);
    await enterTarget("76561198000000001");
    check(element<HTMLInputElement>('input[id^="manual-player-target-"]').getAttribute("aria-invalid") === "true"
      && element<HTMLButtonElement>(".dragonwilds-unban-action").disabled,
      `${locale}: Steam64 and session-like targets cannot submit an EOS unban`);
    await enterTarget("00000000000000000000000000000002");
    check(commonUnban.disabled && [liveAction(0), liveAction(1)].every((button) => button.disabled),
      `${locale}: a valid unbanned EOS cannot authorize unban or live operations`);
    await enterTarget(knownBannedAccount);
    check(element(".dragonwilds-unban-action") === commonUnban && !commonUnban.disabled
      && [liveAction(0), liveAction(1)].every((button) => button.disabled),
      `${locale}: exact persisted EOS enables only the same common unban button`);
    const manualBefore = manualRequests.length;
    await act(async () => { commonUnban.click(); });
    check(manualRequests.length === manualBefore && element(".inline-confirm-message").textContent?.includes(knownBannedAccount),
      `${locale}: manual unban requires explicit confirmation`);
    await confirm();
    check(manualRequests.length === manualBefore + 1 && manualRequests.at(-1)?.action_id === "unban_player"
      && manualRequests.at(-1)?.target === knownBannedAccount && manualRequests.at(-1)?.role === null,
      `${locale}: manual unban dispatches only the exact EOS account and declared operation`);
    check(Boolean(fixture?.querySelector(".shell-activity-notice.is-success")), `${locale}: confirmed unban reports its result in accessible feedback`);
    check(savedBanEntries.length === 0 && element<HTMLButtonElement>(".dragonwilds-unban-action").disabled
      && fixture?.querySelectorAll(".dragonwilds-unban-action").length === 1,
      `${locale}: successful unban refresh removes its saved authorization without duplicating the action`);
  }
  await draw("zh-CN", "ready", knownBannedAccount); await selectFirst();
  await openManual();
  await enterTarget(knownBannedAccount);
  const panel = element<HTMLElement>(".player-center-controls");
  check(panel.scrollWidth <= panel.clientWidth + 1, "Native action pane fits the desktop viewport without horizontal overflow");
  check(errors.length === 0, "Browser and React report no errors");
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  return { status: "passed", checks, browser_errors: errors };
}
Object.assign(window, { __reliabilityFixtureCleanup: async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  await act(async () => { root.unmount(); });
  console.error = originalError;
  return { browser_errors: errors, native_dialogs: 0 };
} });
void run().catch((error) => ({ status: "failed", error: String(error?.stack ?? error), checks, browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
