import { act, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import { MockLivePlayerStore, type MockLivePlayerContext } from "../../src/api-mock/live-players";
import { I18nProvider, useI18n, type LocaleCode } from "../../src/i18n";
import { createPlayerCenterState, derivePlayerCenterViewModel, reducePlayerCenterState } from "../../src/domain/live-player-state";
import { OnlinePlayersView } from "../../src/views/servers/player-center/OnlinePlayersView";
import { useLivePlayers } from "../../src/views/servers/player-center/use-live-players";
import type { DragonwildsBanEntry, DragonwildsBanRoster, ExecuteInstanceManualPlayerActionInput,
  ExecuteInstancePlayerActionInput, ExecuteInstancePlayerActionResult, InstanceDetails, RuntimeLivePlayerEntry } from "../../src/types";
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
  summary: { id: "dragonwilds-ban-roster", module_id: "runescapedragonwilds", name: "Dragonwilds bans",
    status: "Running", active_process_count: 1, autostart: false, bind_ip: "127.0.0.1" },
  active_run: { run_id: 1 }, ports: [], settings_json: "{}", config_file_path: "", saves_path: "",
  backup_uses_declared_saves_path: false, auto_backup_on_stop: false, backup_retention_count: 3
};
const accountA = "1".repeat(32), accountB = "2".repeat(32), accountC = "3".repeat(32);
const initialEntries: DragonwildsBanEntry[] = [{ account_id: accountA, display_name: "Same Name" },
  { account_id: accountB, display_name: "Same Name" }, { account_id: accountC, display_name: null }];
let entries: DragonwildsBanEntry[] = [], rows: RuntimeLivePlayerEntry[] = [], store: MockLivePlayerStore;
let readBehavior: "ready" | "failure" | "deferred" = "ready";
let actionBehavior: "complete" | "failure" | "deferred" | "unknown-effect" = "complete";
let running = true, liveFailure = false, liveStale = false, readCalls = 0, liveRefreshes = 0, mounts = 0;
let finishRead: (() => void) | null = null, finishUnban: (() => void) | null = null;
const unbanRequests: ExecuteInstanceManualPlayerActionInput[] = [], liveRequests: ExecuteInstancePlayerActionInput[] = [];
const context = (): MockLivePlayerContext => ({ instance_id: instance.summary.id, module_id: "runescapedragonwilds",
  running, player_list: moduleDetails.runtime.player_list ?? null, current_players: rows.length, max_players: null, settings: {} });

// Substitute only native IPC. The actual list view, roster hook, API envelope,
// online authorization model, selection and inline confirmation remain real.
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args: Record<string, unknown> = {}) => {
  if (command === "read_instance_dragonwilds_bans") {
    readCalls++;
    if (readBehavior === "failure") throw new Error("Synthetic persisted roster read failure");
    const result: DragonwildsBanRoster = { instance_id: instance.summary.id, observed_at_unix_ms: Date.now(), entries: structuredClone(entries) };
    if (readBehavior === "deferred") return new Promise<DragonwildsBanRoster>((resolve) => {
      finishRead = () => { finishRead = null; resolve(result); };
    });
    return result;
  }
  if (command === "read_instance_live_players" || command === "refresh_instance_live_players") {
    if (command === "refresh_instance_live_players") liveRefreshes++;
    if (liveFailure) throw new Error("Synthetic online snapshot unavailable");
    const snapshot = command === "refresh_instance_live_players" ? store.refresh(context()) : store.read(context());
    return { ...snapshot, stale: liveStale };
  }
  if (command === "execute_instance_manual_player_action") {
    const input = args.input as ExecuteInstanceManualPlayerActionInput;
    if (input.instance_id !== instance.summary.id || input.action_id !== "unban_player" || input.role
      || !/^[0-9a-f]{32}$/.test(input.target)) throw new Error("Unexpected EOS unban binding");
    unbanRequests.push(structuredClone(input));
    if (actionBehavior === "failure") throw new Error("Synthetic native unban failed");
    const apply = (): ExecuteInstancePlayerActionResult => {
      entries = entries.filter((entry) => entry.account_id !== input.target);
      return { action_id: input.action_id, status: "sent", executed_at_unix_ms: Date.now(), summary: "Synthetic unban confirmed" };
    };
    if (actionBehavior === "unknown-effect") { apply(); throw new Error("Synthetic unban receipt timed out"); }
    if (actionBehavior === "deferred") return new Promise<ExecuteInstancePlayerActionResult>((resolve) => {
      finishUnban = () => { finishUnban = null; resolve(apply()); };
    });
    return apply();
  }
  if (command === "execute_instance_player_action") {
    const input = args.input as ExecuteInstancePlayerActionInput;
    liveRequests.push(structuredClone(input));
    const selected = store.read(context()).entries.find((entry) => entry.player_key === input.player_key);
    const accountId = selected?.identifiers.find((identity) => identity.kind === "eos_id")?.value;
    if (!selected || !accountId) throw new Error("Synthetic live target missing");
    const result = store.execute(context(), input);
    if (input.action_id === "ban_player") entries.push({ account_id: accountId, display_name: selected.display_name });
    rows = rows.filter((entry) => entry.player_key !== selected.player_key);
    if (actionBehavior === "unknown-effect") throw new Error("Synthetic ban receipt timed out");
    return result;
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
async function click(selector: string) { await act(async () => { element<HTMLButtonElement>(selector).click(); }); }
const banTab = '[role="tab"][data-list-key="dragonwilds-banned"]';
const onlineTab = '[role="tab"][data-list-key="online"]';
const unban = ".dragonwilds-unban-action";
const actionInventory = ".player-center-member-action,.dragonwilds-unban-action";
const actionButtons = () => Array.from(fixture?.querySelectorAll<HTMLButtonElement>(actionInventory) ?? []);
const rosterCount = () => fixture?.querySelector(`${banTab} .player-center-list-count`)?.textContent;
const bannedRows = () => fixture?.querySelectorAll("[data-banned-account]").length ?? 0;
async function choose(accountId: string) { await click(`[data-banned-account="${accountId}"]`); }
async function confirm() { await click(".inline-confirm-submit"); }
async function refresh() { await click(".player-center-list-pane .player-center-refresh-button"); }
function preserveActionInventory(locale: LocaleCode) {
  const controls = element<HTMLElement>(".player-center-controls");
  const heading = element<HTMLElement>(".player-center-controls > .player-center-member-header");
  const buttons = actionButtons();
  const expectedLabels = (moduleDetails.runtime.player_actions ?? []).filter((action) =>
    moduleDetails.runtime.player_list?.player_action_ids?.includes(action.id))
    .map((action) => locale === "zh-CN" ? action.label_zh_cn : action.label);
  expectedLabels.push(locale === "zh-CN" ? "解除封禁" : "Unban");
  check(buttons.length === 3 && buttons.every((button, index) => button.textContent?.trim() === expectedLabels[index]),
    `${locale}: kick ban and unban have one fixed declared order`);
  return (phase: string, enabled: boolean[]) => {
    const current = actionButtons();
    check(element(".player-center-controls") === controls
      && element(".player-center-controls > .player-center-member-header") === heading
      && current.length === buttons.length && current.every((button, index) => button === buttons[index]
        && button.isConnected && button.getClientRects().length > 0 && button.disabled !== enabled[index]),
      `${locale}: ${phase} preserves every action DOM object and expected permission`);
  };
}
function NativeBanWorkspace({ readOnly }: { readOnly?: boolean }) {
  const { locale, t } = useI18n();
  const live = useLivePlayers(instance.summary.id, !readOnly, running ? "running" : "stopped");
  const [state, setState] = useState(() => createPlayerCenterState({ instanceId: instance.summary.id }));
  useEffect(() => {
    const snapshot = live.snapshot;
    if (!readOnly && snapshot) setState((current) => reducePlayerCenterState(current, {
      type: "snapshot-received", snapshot
    }));
  }, [live.snapshot, readOnly]);
  const presentation = derivePlayerCenterViewModel(readOnly
    ? createPlayerCenterState({ instanceId: instance.summary.id }) : state, Date.now()).livePlayers;
  const details = running ? instance : { ...instance,
    summary: { ...instance.summary, status: "Stopped", active_process_count: 0 }, active_run: null };
  return <section className="player-access-workbench"><OnlinePlayersView
    instanceDetails={details} moduleId="runescapedragonwilds" readOnly={readOnly}
    declaredActions={moduleDetails.runtime.player_actions ?? []}
    playerActionIds={moduleDetails.runtime.player_list?.player_action_ids ?? []}
    error={readOnly ? null : live.error} loading={!readOnly && live.loading}
    locale={locale} now={Date.now()} onRefresh={live.refresh} runtime={null} runtimeAvailable={running}
    snapshot={readOnly ? null : state.snapshot} presentation={presentation} t={t}
    onSelectPlayer={(playerKey) => setState((current) => reducePlayerCenterState(current, { type: "player-selected", playerKey }))}
  /></section>;
}
async function draw(locale: LocaleCode, options: { entries?: DragonwildsBanEntry[]; read?: typeof readBehavior;
  readOnly?: boolean; stopped?: boolean; liveFailure?: boolean } = {}) {
  if (finishRead || finishUnban) throw new Error("Fixture cannot abandon native requests");
  entries = structuredClone(options.entries ?? initialEntries);
  running = !options.stopped; liveFailure = Boolean(options.liveFailure); liveStale = false; readBehavior = options.read ?? "ready";
  actionBehavior = "complete";
  const seed = new MockLivePlayerStore().refresh({ ...context(), running: true, current_players: 2 }).entries;
  rows = seed;
  store = new MockLivePlayerStore({ collector: () => ({ status: "ready", complete: true, truncated: false,
    entries: structuredClone(rows), current_players: rows.length, issue: null }) });
  store.refresh(context());
  mounts++;
  await act(async () => {
    localStorage.setItem("langame.locale", locale);
    root.render(<I18nProvider key={mounts}><main style={{ height: "calc(100vh - 48px)", margin: 24,
      display: "grid", minHeight: 0 }}><section className="server-detail-panel"><div className="detail-stack detail-stack--server">
      <div className="server-detail-subheader">Dragonwilds · {locale === "zh-CN" ? "玩家" : "Players"}</div>
      <div className="server-detail-scroll server-detail-scroll--players"><NativeBanWorkspace readOnly={options.readOnly} /></div>
    </div></section></main></I18nProvider>);
  });
  await settle(() => Boolean(fixture?.querySelector(banTab)), "Ban tab did not mount");
}
async function run() {
  await act(prepareBrowserLocaleCatalogs);
  for (const locale of ["zh-CN", "en-US"] as const) {
    const label = (text: string) => `${locale}: ${text}`;
    await draw(locale);
    await settle(() => Boolean(fixture?.querySelector(".player-center-player-table .player-center-player-select")), "Live rows did not load");
    const verifyInventory = preserveActionInventory(locale);
    verifyInventory("no selection", [false, false, false]);
    await click(".player-center-player-table .player-center-player-select");
    verifyInventory("online selection", [true, true, false]);
    await click(banTab);
    verifyInventory("banned list without selection", [false, false, false]);
    await choose(accountB);
    verifyInventory("offline banned selection", [false, false, true]);
    const selectedBanRow = element<HTMLElement>(`[data-banned-account="${accountB}"][aria-pressed="true"]`);
    const rightControls = element<HTMLElement>(".player-center-controls");
    check(selectedBanRow.textContent?.includes("Same Name") && selectedBanRow.textContent.includes(accountB)
      && !rightControls.textContent?.includes(accountB) && !rightControls.querySelector(".player-center-details"),
      label("selected banned row shows the full EOS without duplicate details or fabricated duration"));
    await choose(accountA);
    verifyInventory("another same-name banned account", [false, false, true]);
    await click(onlineTab);
    verifyInventory("returning online without selection", [false, false, false]);
    await click(".player-center-player-table .player-center-player-select");
    verifyInventory("online reselection", [true, true, false]);
    await click(".player-center-player-table .player-center-player-select");
    verifyInventory("online deselection", [false, false, false]);
    await click(".player-center-player-table .player-center-player-select");
    liveStale = true; await refresh();
    verifyInventory("stale online snapshot", [false, false, false]);
    await click(banTab); await choose(accountB);
    verifyInventory("fresh persisted account despite stale online snapshot", [false, false, true]);

    // Inline review intentionally replaces its trigger. Test its cancellation
    // separately from navigation, which must preserve the idle action buttons.
    await draw(locale);
    await click(".player-center-player-table .player-center-player-select");
    const beforeCancelled = liveRequests.length + unbanRequests.length;
    await click(".player-center-member-action"); await click(banTab);
    check(!fixture?.querySelector(".inline-confirm-review") && liveRequests.length + unbanRequests.length === beforeCancelled,
      label("switching lists cancels a live confirmation without dispatch"));
    await choose(accountB); await click(unban); await choose(accountA);
    check(!fixture?.querySelector(".inline-confirm-review") && liveRequests.length + unbanRequests.length === beforeCancelled,
      label("changing banned accounts cancels the earlier confirmation without dispatch"));
    await click(unban); await click(onlineTab);
    check(!fixture?.querySelector(".inline-confirm-review") && liveRequests.length + unbanRequests.length === beforeCancelled,
      label("switching lists cancels an unban confirmation without dispatch"));

    await draw(locale, { read: "deferred", liveFailure: true });
    await click(banTab);
    preserveActionInventory(locale)("initial roster loading", [false, false, false]);
    check(!rosterCount() && !bannedRows() && element<HTMLButtonElement>(unban).disabled
      && !fixture?.textContent?.includes(locale === "zh-CN" ? "暂无被封禁玩家" : "No banned players"),
      label("initial loading has no fabricated count or empty state"));
    readBehavior = "ready";
    await act(async () => { finishRead?.(); });
    await settle(() => rosterCount() === "3", "Persisted bans did not load");
    check(bannedRows() === 3 && fixture?.querySelectorAll('[data-banned-account] .server-player-name')[0]?.textContent === "Same Name"
      && fixture?.querySelectorAll('[data-banned-account] .server-player-name')[1]?.textContent === "Same Name"
      && element<HTMLButtonElement>(`[data-banned-account="${accountC}"]`).textContent?.includes(accountC),
      label("same-name accounts and unnamed accounts retain complete EOS identities"));
    check(actionButtons().length === 3 && actionButtons().every((button) => button.disabled)
      && !Array.from(fixture?.querySelectorAll<HTMLElement>(".player-access-roster-create,.player-access-edit-selected,.player-center-manual-actions") ?? [])
      .some((control) => control.getClientRects().length > 0),
      label("ban tab retains the fixed action inventory without settings editors"));
    const readsBeforeTabs = readCalls;
    for (const [key, target] of [["ArrowLeft", onlineTab], ["ArrowRight", banTab], ["Home", onlineTab], ["End", banTab]]) {
      await act(async () => { element<HTMLButtonElement>('[role="tab"][aria-selected="true"]').dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true })); });
      check(element<HTMLButtonElement>(target).getAttribute("aria-selected") === "true" && document.activeElement === element(target), `${locale}: ${key} selects and focuses the expected tab`);
    }
    check(readCalls === readsBeforeTabs, label("tab keyboard navigation uses the shared tablist without extra reads"));
    await choose(accountB);
    check(!element<HTMLButtonElement>(unban).disabled
      && element<HTMLElement>(`[data-banned-account="${accountB}"][aria-pressed="true"]`).textContent?.includes(accountB)
      && actionButtons().slice(0, 2).every((button) => button.disabled),
      label("offline account unban needs no online snapshot"));
    const before = unbanRequests.length;
    await click(unban);
    check(unbanRequests.length === before && element<HTMLElement>(".inline-confirm-message").textContent?.includes(accountB)
      && !element<HTMLElement>(".inline-confirm-message").textContent?.includes(accountA),
      label("confirmation includes the exact selected EOS and sends nothing early"));
    actionBehavior = "deferred";
    await act(async () => { const submit = element<HTMLButtonElement>(".inline-confirm-submit"); submit.click(); submit.click(); });
    check(unbanRequests.length === before + 1 && unbanRequests.at(-1)?.target === accountB
      && element<HTMLButtonElement>(".inline-confirm-submit").disabled
      && Array.from(fixture?.querySelectorAll<HTMLButtonElement>('[role="tab"],[data-banned-account]') ?? []).every((button) => button.disabled),
      label("pending unban blocks duplicate requests and selection changes"));
    const beforeRead = readCalls, beforeLive = liveRefreshes;
    liveFailure = false;
    await act(async () => { finishUnban?.(); });
    await settle(() => rosterCount() === "2", "Unban did not refresh persisted roster");
    check(readCalls === beforeRead + 1 && liveRefreshes === beforeLive + 1 && !fixture?.querySelector(`[data-banned-account="${accountB}"]`)
      && !fixture?.querySelector('.player-center-details') && Boolean(fixture?.querySelector(".shell-activity-notice.is-success")),
      label("confirmed unban refreshes both lists and clears selection"));
    actionBehavior = "failure";
    await choose(accountA); await click(unban); await confirm();
    check(Boolean(fixture?.querySelector(`[data-banned-account="${accountA}"][aria-pressed="true"]`))
      && element<HTMLButtonElement>(unban).disabled && fixture?.textContent?.includes("Synthetic native unban failed"),
      label("native failure retains the exact entry and requires a fresh read"));
    readBehavior = "failure"; await refresh();
    check(bannedRows() === 2 && !rosterCount() && element<HTMLButtonElement>(unban).disabled
      && fixture?.textContent?.includes("Synthetic persisted roster read failure"),
      label("read failure preserves old entries without a fresh count or enabled unban"));
    readBehavior = "deferred";
    const beforeConcurrent = readCalls;
    await act(async () => { const button = element<HTMLButtonElement>(".player-center-refresh-button"); button.click(); button.click(); });
    readBehavior = "ready";
    await act(async () => { finishRead?.(); });
    check(readCalls === beforeConcurrent + 1 && rosterCount() === "2" && !element<HTMLButtonElement>(unban).disabled,
      label("explicit refresh coalesces concurrent reads and restores authority"));

    await draw(locale, { entries: [] }); await click(banTab);
    readBehavior = "deferred"; await refresh();
    await click(onlineTab); await click(".player-center-player-table .player-center-player-select");
    actionBehavior = "unknown-effect";
    await act(async () => { fixture?.querySelectorAll<HTMLButtonElement>(".player-center-member-action")[1]?.click(); });
    await confirm();
    const originalFailureVisible = fixture?.textContent?.includes("Synthetic ban receipt timed out");
    const dispatchedBans = liveRequests.length;
    readBehavior = "ready";
    await act(async () => { finishRead?.(); });
    await click(banTab);
    check(originalFailureVisible && bannedRows() === 0 && !rosterCount() && element<HTMLButtonElement>(unban).disabled,
      label("unknown ban results cannot be made fresh by an earlier read"));
    await refresh();
    check(rosterCount() === "1" && bannedRows() === 1 && liveRequests.length === dispatchedBans,
      `${locale}: explicit read resolves an unknown effect without repeating native ban`);

    await draw(locale, { readOnly: true }); await click(banTab); await choose(accountA);
    preserveActionInventory(locale)("read-only banned selection", [false, false, false]);
    const beforeReadonly = unbanRequests.length;
    await refresh();
    check(rosterCount() === "3" && element<HTMLButtonElement>(unban).disabled && unbanRequests.length === beforeReadonly
      && fixture?.textContent?.includes(locale === "zh-CN" ? "只读" : "read-only"),
      label("read-only workspace permits inspection and refresh but never unban"));
    await draw(locale, { stopped: true }); await click(banTab); await choose(accountC);
    preserveActionInventory(locale)("stopped banned selection", [false, false, false]);
    check(rosterCount() === "3" && element<HTMLButtonElement>(unban).disabled
      && !element<HTMLButtonElement>(".player-center-refresh-button").disabled
      && fixture?.textContent?.includes(locale === "zh-CN" ? "启动服务器" : "Start the server"),
      label("stopped server retains persisted bans and explains disabled unban"));
    await draw(locale, { read: "failure" }); await click(banTab);
    check(!rosterCount() && !bannedRows() && Boolean(fixture?.querySelector('[role="alert"]'))
      && !fixture?.textContent?.includes(locale === "zh-CN" ? "暂无被封禁玩家" : "No banned players"),
      label("initial read failure is distinct from a confirmed empty roster"));
    await draw(locale, { entries: [] }); await click(banTab);
    check(rosterCount() === "0" && fixture?.textContent?.includes(locale === "zh-CN" ? "暂无被封禁玩家" : "No banned players"), `${locale}: successful empty roster has an explicit zero count`);
    await click(onlineTab);
    await settle(() => Boolean(fixture?.querySelector(".player-center-player-table .player-center-player-select")), "Live rows did not load");
    await click(".player-center-player-table .player-center-player-select");
    const beforeBanRead = readCalls;
    await act(async () => { fixture?.querySelectorAll<HTMLButtonElement>(".player-center-member-action")[1]?.click(); });
    await confirm();
    await settle(() => readCalls > beforeBanRead && entries.length === 1, "Online ban did not reread roster");
    const bannedOnline = entries[0].account_id;
    await click(banTab);
    check(rosterCount() === "1" && Boolean(fixture?.querySelector(`[data-banned-account="${bannedOnline}"]`)),
      label("online native ban refreshes the independent persisted roster"));
    await choose(bannedOnline);
    const beforeUnbanRead = readCalls, beforeUnbanLive = liveRefreshes;
    await click(unban); await confirm();
    await settle(() => readCalls > beforeUnbanRead && entries.length === 0, "Selected unban did not refresh roster");
    check(rosterCount() === "0" && liveRefreshes === beforeUnbanLive + 1 && unbanRequests.at(-1)?.target === bannedOnline,
      label("disconnected online account unban refreshes the persisted roster and online list"));

    await draw(locale, { entries: [] }); await click(banTab);
    readBehavior = "deferred"; await refresh();
    await click(onlineTab); await click(".player-center-player-table .player-center-player-select");
    await act(async () => { fixture?.querySelectorAll<HTMLButtonElement>(".player-center-member-action")[1]?.click(); });
    await confirm();
    const beforeFresh = readCalls;
    readBehavior = "ready";
    await act(async () => { finishRead?.(); });
    await settle(() => readCalls === beforeFresh + 1, "Post-mutation read was incorrectly coalesced with earlier read");
    await click(banTab);
    check(rosterCount() === "1" && bannedRows() === 1,
      label("a read started before moderation cannot certify its resulting roster"));
  }
  await draw("zh-CN"); await click(banTab); await choose(accountB);
  await settle(() => rosterCount() === "3" && !element<HTMLButtonElement>(unban).disabled,
    "Final selected account did not become available for unban");
  const pane = element<HTMLElement>(".player-center-controls");
  // Capture the settled enabled style, after the actual CSS transitions finish.
  await act(async () => { await Promise.all(pane.getAnimations({ subtree: true }).map((animation) => animation.finished)); });
  check(pane.scrollWidth <= pane.clientWidth + 1, "Ban roster controls fit the desktop viewport");
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
