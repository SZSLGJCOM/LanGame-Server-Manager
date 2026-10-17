import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import { I18nProvider, useI18n, type LocaleCode } from "../../src/i18n";
import { createPlayerCenterState, derivePlayerCenterViewModel, reducePlayerCenterState } from "../../src/domain/live-player-state";
import { OnlinePlayersView } from "../../src/views/servers/player-center/OnlinePlayersView";
import { ManualPlayerActions } from "../../src/views/servers/player-center/ManualPlayerActions";
import { actionNeedsManualTarget, manualPlayerActionTargetError, readManualPlayerActions } from "../../src/views/servers/player-center/manual-player-action-model";
import { useNativeBanRoster } from "../../src/views/servers/player-center/use-native-ban-roster";
import type { ExecuteInstanceManualPlayerActionInput, ExecuteInstancePlayerActionInput, ExecuteInstancePlayerActionResult, InstanceDetails, NativeBanEntry,
  NativeBanRoster, RuntimeLivePlayerEntry, RuntimeLivePlayerSnapshot, RuntimePlayerIdentityKind } from "../../src/types";
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

const games: { id: string; kind: RuntimePlayerIdentityKind; accounts: [string, string] }[] = [
  { id: "conanexiles", kind: "conan_user_id", accounts: ["1".repeat(32), "2".repeat(32)] },
  { id: "palworld", kind: "palworld_user_id", accounts: ["steam_76561198000000001", "steam_76561198000000002"] },
  { id: "rimworld", kind: "player_name", accounts: ["ColonistA", "ColonistB"] },
  { id: "soulmask", kind: "steam_id", accounts: ["76561198000000001", "76561198000000002"] },
  { id: "arksurvivalascended", kind: "eos_id", accounts: ["1".repeat(32), "2".repeat(32)] },
  { id: "arksurvivalevolved", kind: "ark_account_id", accounts: ["1000000000000000001", "1000000000000000002"] },
  { id: "squad", kind: "eos_id", accounts: ["1".repeat(32), "2".repeat(32)] },
  { id: "necesse", kind: "necesse_ban_id", accounts: ["HostAlice", "76561198000000002"] },
  { id: "unturned", kind: "steam_id", accounts: ["76561198000000001", "76561198000000002"] }
];
let game = games[0], entries: NativeBanEntry[] = [], running = true, readOnly = false;
let readBehavior: "ready" | "failure" | "deferred" | "wrong-scope" = "ready";
let actionBehavior: "ready" | "failure" | "deferred" | "sent-only" = "ready";
let finishRead: (() => void) | null = null, finishUnban: (() => void) | null = null;
let reads = 0, liveRefreshes = 0, mounts = 0;
const unbanRequests: { instanceId: string; accountId: string }[] = [];
const liveRequests: ExecuteInstancePlayerActionInput[] = [];
const manualUnbanRequests: ExecuteInstanceManualPlayerActionInput[] = [];
const manualBanRequests: ExecuteInstanceManualPlayerActionInput[] = [];
let liveEntries: RuntimeLivePlayerEntry[] = [];
const instanceId = () => `native-bans-${game.id}`;
function snapshot(): RuntimeLivePlayerSnapshot {
  return { snapshot_id: `snapshot-${instanceId()}`, instance_id: instanceId(), status: running ? "ready" : "stopped", source: "runtime_action",
    observed_at_unix_ms: running ? Date.now() : null, expires_at_unix_ms: running ? Date.now() + 60000 : null,
    complete: running, truncated: false, stale: false,
    current_players: running ? liveEntries.length : null, max_players: 20, entries: running ? structuredClone(liveEntries) : [], issue: null };
}
function result(): ExecuteInstancePlayerActionResult {
  return { action_id: game.id === "rimworld" ? "unban_user" : "unban_player", status: "sent",
    executed_at_unix_ms: Date.now(), summary: "Synthetic native result" };
}
// Only IPC is substituted. The shared view, identity matching, roster hook,
// manual-action suppression and confirmation controls remain production code.
Object.assign(window, { isTauri: true, __TAURI_INTERNALS__: { invoke: async (command: string, args: Record<string, unknown> = {}) => {
  if (command === "read_instance_native_bans") {
    reads++;
    if (readBehavior === "failure") throw new Error("Synthetic native roster read failure");
    const roster: NativeBanRoster = { instance_id: instanceId(), module_id: game.id, observed_at_unix_ms: Date.now(), entries: structuredClone(entries) };
    if (readBehavior === "wrong-scope") return { ...roster, module_id: "other-module" };
    if (readBehavior === "deferred") return new Promise<NativeBanRoster>((resolve) => { finishRead = () => { finishRead = null; resolve(roster); }; });
    return roster;
  }
  if (command === "unban_instance_native_ban") {
    if (args.instanceId !== instanceId() || typeof args.accountId !== "string" || !entries.some((entry) => entry.account_id === args.accountId)) {
      throw new Error("Wrong exact native unban envelope");
    }
    unbanRequests.push({ instanceId: String(args.instanceId), accountId: args.accountId });
    if (actionBehavior === "failure") throw new Error("Synthetic native unban failure");
    if (actionBehavior === "sent-only") return result();
    const apply = () => { entries = entries.filter((entry) => entry.account_id !== args.accountId); return result(); };
    if (actionBehavior === "deferred") return new Promise<ExecuteInstancePlayerActionResult>((resolve) => { finishUnban = () => { finishUnban = null; resolve(apply()); }; });
    return apply();
  }
  if (command === "execute_instance_manual_player_action") {
    const input = args.input as ExecuteInstanceManualPlayerActionInput;
    const declared = buildMockModuleDetails(game.id).runtime.player_actions?.find((action) => action.id === input.action_id);
    const soulmaskBan = game.id === "soulmask" && input.action_id === "ban_player";
    if (input.instance_id !== instanceId() || !declared || !soulmaskBan && input.action_id !== (game.id === "rimworld" ? "unban_user" : "unban_player")
      || input.role !== null || manualPlayerActionTargetError(game.id, declared, input.target, "en-US")) throw new Error("Wrong manual native action envelope");
    if (soulmaskBan) manualBanRequests.push(structuredClone(input));
    else {
      manualUnbanRequests.push(structuredClone(input));
      entries = entries.filter((entry) => entry.account_id !== input.target);
    }
    return { ...result(), action_id: input.action_id };
  }
  if (command === "execute_instance_player_action") {
    const input = args.input as ExecuteInstancePlayerActionInput;
    liveRequests.push(input);
    const player = liveEntries.find((entry) => entry.player_key === input.player_key);
    if (!player || input.instance_id !== instanceId() || !player.available_action_ids.includes(input.action_id)) throw new Error("Wrong live target");
    if (input.action_id === "ban_player" || input.action_id === "ban_user") {
      entries.push({ account_id: game.accounts[0], display_name: player.display_name, identity_kind: game.kind });
      liveEntries = [];
    }
    return { ...result(), action_id: input.action_id };
  }
  throw new Error(`Unexpected native command ${command}`);
} } });

function element<T extends Element>(selector: string): T {
  const found = fixture?.querySelector<T>(selector);
  if (!found) throw new Error(`Missing ${selector}: ${fixture?.textContent}`);
  return found;
}
function check(value: unknown, description: string): asserts value {
  if (!value) throw new Error(`${description}: ${fixture?.textContent}`);
  checks.push(description);
}
async function click(selector: string) { await act(async () => { element<HTMLButtonElement>(selector).click(); }); }
async function settle(predicate: () => boolean) {
  const deadline = performance.now() + 5000;
  while (!predicate()) {
    if (performance.now() > deadline) throw new Error(`Fixture did not settle: ${fixture?.textContent}`);
    await act(async () => { await new Promise<void>((resolve) => requestAnimationFrame(() => resolve())); });
  }
}
const banTab = '[role="tab"][data-list-key="native-banned"]';
const onlineTab = '[role="tab"][data-list-key="online"]';
const unban = ".native-unban-action";
const count = () => fixture?.querySelector(`${banTab} .player-center-list-count`)?.textContent;
const choose = (account: string) => click(`[data-banned-account="${account}"]`);
const refresh = () => click(".player-center-list-pane .player-center-refresh-button");
async function lookup(target: string) {
  await act(async () => {
    const input = element<HTMLInputElement>('.player-center-manual-lookup input');
    const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
    if (!setValue) throw new Error("Native input setter missing");
    setValue.call(input, target); input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await click('.player-center-manual-use');
}

function Workspace() {
  const { locale, t } = useI18n();
  const module = buildMockModuleDetails(game.id);
  const [state, setState] = useState(() => reducePlayerCenterState(createPlayerCenterState({ instanceId: instanceId() }), {
    type: "snapshot-received", snapshot: snapshot()
  }));
  const presentation = derivePlayerCenterViewModel(state, Date.now()).livePlayers;
  const details: InstanceDetails = {
    summary: { id: instanceId(), module_id: game.id, name: game.id, status: running ? "Running" : "Stopped", active_process_count: running ? 1 : 0,
      autostart: false, bind_ip: "127.0.0.1" }, active_run: running ? { run_id: 1 } : null,
    ports: [], settings_json: "{}", config_file_path: "", saves_path: "", backup_uses_declared_saves_path: false,
    auto_backup_on_stop: false, backup_retention_count: 3
  };
  const manual = readManualPlayerActions(module, []);
  const targetActions = manual.filter(actionNeedsManualTarget);
  const manualTargetFormat = targetActions.length ? { hint: null, validate: (target: string) => {
    const errors = targetActions.map((action) => manualPlayerActionTargetError(game.id, action, target, locale));
    return errors.some((error) => !error) ? null : errors[0] ?? null;
  } } : undefined;
  return <section className="player-access-workbench"><OnlinePlayersView instanceDetails={details} moduleId={game.id}
    readOnly={readOnly} declaredActions={module.runtime.player_actions ?? []} playerActionIds={module.runtime.player_list?.player_action_ids ?? []}
    error={null} loading={false} locale={locale} now={Date.now()} runtime={null} runtimeAvailable={running}
    snapshot={state.snapshot} presentation={presentation} t={t}
    manualTargetFormat={manualTargetFormat}
    onRefresh={async () => { liveRefreshes++; setState((current) => reducePlayerCenterState(current, { type: "snapshot-received", snapshot: snapshot() })); }}
    onSelectPlayer={(playerKey) => setState((current) => reducePlayerCenterState(current, { type: "player-selected", playerKey }))}
    manualActions={manual.length ? (active, onActionCompleted, onActionFailed) => <ManualPlayerActions actions={manual}
      instanceId={instanceId()} moduleId={game.id} disabled={readOnly || !running} active={active} locale={locale} t={t}
      onActionCompleted={onActionCompleted} onActionFailed={onActionFailed} /> : undefined} /></section>;
}
function ScopeProbe({ id, moduleId }: { id: string; moduleId: string }) {
  const state = useNativeBanRoster(id, moduleId);
  return <div data-scope-instance={state.roster?.instance_id ?? ""} data-scope-module={state.roster?.module_id ?? ""}
    data-scope-count={state.roster?.entries.length ?? ""} data-scope-stale={String(state.stale)} />;
}
async function draw(locale: LocaleCode, options: { readonly?: boolean; stopped?: boolean; read?: typeof readBehavior; empty?: boolean } = {}) {
  running = !options.stopped; readOnly = Boolean(options.readonly); readBehavior = options.read ?? "ready"; actionBehavior = "ready";
  entries = options.empty ? [] : game.accounts.map((account_id) => ({ account_id, display_name: "Same Name", identity_kind: game.kind }));
  const module = buildMockModuleDetails(game.id);
  liveEntries = [{ player_key: "actual-live-row", display_name: "Current Player", identifiers: [{ kind: game.kind, value: game.accounts[0], stable: true }],
    available_action_ids: module.runtime.player_list?.player_action_ids ?? [], ping_ms: null, session_started_at_unix_ms: null, role: null, attributes: [] }];
  mounts++;
  await act(async () => {
    localStorage.setItem("langame.locale", locale);
    root.render(<I18nProvider key={mounts}><main style={{ height: "calc(100vh - 48px)", margin: 24, display: "grid", minHeight: 0 }}>
      <section className="server-detail-panel"><div className="detail-stack detail-stack--server"><div className="server-detail-subheader">{game.id} · {locale === "zh-CN" ? "玩家" : "Players"}</div>
        <div className="server-detail-scroll server-detail-scroll--players"><Workspace /></div></div></section></main></I18nProvider>);
  });
  await settle(() => Boolean(fixture?.querySelector(banTab)));
  await click(banTab);
}

async function run() {
  await act(prepareBrowserLocaleCatalogs);
  for (const locale of ["zh-CN", "en-US"] as const) for (const candidate of games) {
    game = candidate;
    const label = (description: string) => `${locale}/${game.id}: ${description}`;
    await draw(locale, { read: "deferred" });
    const tabs = Array.from(fixture?.querySelectorAll('[role="tab"]') ?? []);
    check(tabs[0] === element(onlineTab) && tabs[1] === element(banTab)
      && tabs[1].closest(".player-center-pane-header") === tabs[0].closest(".player-center-pane-header"), label("ban list is adjacent to online players in header tablist"));
    check(!count() && !fixture?.querySelector('[data-banned-account]') && element<HTMLButtonElement>(unban).disabled,
      label("loading does not fabricate entries or authority"));
    readBehavior = "ready";
    await act(async () => { finishRead?.(); });
    await choose(game.accounts[1]);
    check(count() === "2" && !element<HTMLButtonElement>(unban).disabled
      && Array.from(fixture?.querySelectorAll<HTMLButtonElement>(".player-center-member-action") ?? []).every((button) => button.disabled),
      label("offline native entry only enables exact unban"));
    check(fixture?.querySelectorAll(unban).length === 1, label("native unban is one shared action including RimWorld and Squad"));
    const before = unbanRequests.length;
    await click(unban); await choose(game.accounts[0]);
    check(!fixture?.querySelector(".inline-confirm-review") && unbanRequests.length === before, label("changing identity cancels confirmation"));
    await choose(game.accounts[1]); await click(unban);
    check(element(".inline-confirm-message").textContent?.includes(game.accounts[1]) && unbanRequests.length === before,
      label("confirmation retains complete exact account"));
    actionBehavior = "deferred";
    await act(async () => { const submit = element<HTMLButtonElement>(".inline-confirm-submit"); submit.click(); submit.click(); });
    check(unbanRequests.length === before + 1 && unbanRequests.at(-1)?.accountId === game.accounts[1]
      && Array.from(fixture?.querySelectorAll<HTMLButtonElement>('[role="tab"],[data-banned-account]') ?? []).every((button) => button.disabled),
      label("native unban envelope and duplicate submission guard are exact"));
    const beforeReads = reads, beforeLive = liveRefreshes;
    await act(async () => { finishUnban?.(); });
    await settle(() => count() === "1");
    check(reads === beforeReads + 1 && liveRefreshes === beforeLive + 1 && !fixture?.querySelector(`[data-banned-account="${game.accounts[1]}"]`),
      label("unban refreshes native and online rosters"));
    await choose(game.accounts[0]); actionBehavior = "failure"; await click(unban); await click(".inline-confirm-submit");
    check(element<HTMLButtonElement>(unban).disabled && !count() && Boolean(fixture?.querySelector('[data-banned-account][aria-pressed="true"]')),
      label("failed unban invalidates authority while retaining selection"));
    readBehavior = "failure"; await refresh();
    check(!count() && Boolean(fixture?.querySelector('[data-banned-account]')) && Boolean(fixture?.querySelector('[role="alert"]')),
      label("failed read preserves entries without fresh count"));
    readBehavior = "ready"; await refresh();
    check(count() === "1" && !element<HTMLButtonElement>(unban).disabled, label("explicit refresh restores native authority"));
    if (game.id !== "squad") {
      actionBehavior = "sent-only"; await click(unban); await click(".inline-confirm-submit");
      check(count() === "1" && Boolean(fixture?.querySelector('[data-banned-account][aria-pressed="true"]'))
        && Boolean(fixture?.querySelector(".shell-activity-notice.is-warning")) && !fixture?.querySelector(".shell-activity-notice.is-success")
        && fixture?.textContent?.includes(locale === "zh-CN" ? "结果尚未确认" : "Result unconfirmed"),
        label("submitted command is not success while native entry remains"));
    }
    await draw(locale, { readonly: true }); await choose(game.accounts[1]);
    check(count() === "2" && element<HTMLButtonElement>(unban).disabled && !element<HTMLButtonElement>(".player-center-refresh-button").disabled,
      label("readonly workspace can inspect and refresh but cannot unban"));
    await draw(locale, { stopped: true }); await choose(game.accounts[1]);
    if (game.id === "squad") {
      const beforeStopped = unbanRequests.length, beforeStoppedLive = liveRefreshes;
      check(count() === "2" && !element<HTMLButtonElement>(unban).disabled, label("stopped Squad can remove a persisted ban"));
      await click(unban);
      check(element(".inline-confirm-message").textContent?.includes(locale === "zh-CN" ? "重启" : "restart"),
        label("Squad confirmation requires restart to take effect"));
      await click(".inline-confirm-submit");
      check(count() === "1" && unbanRequests.length === beforeStopped + 1
        && element(".shell-activity-notice.is-warning").textContent?.includes(locale === "zh-CN" ? "下次启动服务器时生效" : "next server start")
        && !fixture?.querySelector(".shell-activity-notice.is-success"),
        label("Squad persistent removal reports pending restart without claiming immediate unban"));
      check(liveRefreshes === beforeStoppedLive, label("stopped Squad removal refreshes persistence without requesting online players"));
      await choose(game.accounts[0]); actionBehavior = "sent-only";
      await click(unban); await click(".inline-confirm-submit");
      check(count() === "1" && element(".shell-activity-notice.is-warning").textContent?.includes(locale === "zh-CN" ? "仍有此账号" : "still in the saved bans")
        && !element(".shell-activity-notice.is-warning").textContent?.includes(locale === "zh-CN" ? "持久封禁已移除" : "Saved ban removed")
        && !fixture?.querySelector(".shell-activity-notice.is-success"),
        label("Squad reban during removal remains unconfirmed when the saved entry reappears"));
      actionBehavior = "ready"; readBehavior = "failure";
      await click(unban); await click(".inline-confirm-submit");
      check(element(".shell-activity-notice.is-warning").textContent?.includes(locale === "zh-CN" ? "未确认" : "unconfirmed")
        && !element(".shell-activity-notice.is-warning").textContent?.includes(locale === "zh-CN" ? "持久封禁已移除" : "Saved ban removed")
        && !fixture?.querySelector(".shell-activity-notice.is-success"),
        label("Squad failed post-removal read cannot confirm persisted removal"));
      check(!fixture?.querySelector('[data-manual-action="reload_server_config"]'), label("Squad has no ineffective reload recovery action"));
    } else {
      check(count() === "2" && element<HTMLButtonElement>(unban).disabled, label("stopped server retains bans without native mutation"));
    }
    if (game.id === "palworld") {
      await draw(locale);
      const oldGuid = "abcdef0123456789abcdef0123456789";
      entries.push({ account_id: oldGuid, display_name: null, identity_kind: "palworld_player_uid" });
      await refresh(); await choose(oldGuid);
      const requestsBefore = unbanRequests.length;
      await click(unban);
      check(count() === "3" && Boolean(fixture?.querySelector(`[data-banned-account="${oldGuid}"][aria-pressed="true"]`))
        && element<HTMLButtonElement>(unban).disabled && unbanRequests.length === requestsBefore
        && fixture?.textContent?.includes(locale === "zh-CN" ? "缺少原生用户 ID" : "no native user ID"),
        label("historical player GUID remains visible without inventing a REST user identity or unban"));
      await click('[role="tab"][data-list-key="manual"]'); await lookup(oldGuid);
      check(element<HTMLButtonElement>(unban).disabled && element<HTMLInputElement>('.player-center-manual-lookup input').getAttribute('aria-invalid') === "true",
        label("manual player GUID cannot fall back to REST user unban"));
    }
    if (game.id === "soulmask") {
      await draw(locale);
      const epic = `${"ABCDEF0123456789".repeat(2)}|${"0123456789abcdef".repeat(2)}`;
      entries.push({ account_id: epic, display_name: "Current Player", identity_kind: "soulmask_account_id" });
      await refresh(); await choose(epic);
      check(count() === "3" && !element<HTMLButtonElement>(unban).disabled
        && Array.from(fixture?.querySelectorAll<HTMLButtonElement>(".player-center-member-action") ?? []).every((button) => button.disabled),
        label("Epic ban remains visible without matching a Steam player or name"));
      const beforeEpic = unbanRequests.length;
      await click(unban);
      check(element(".inline-confirm-message").textContent?.includes(epic), label("Epic ban confirmation preserves both complete identity components"));
      await click(".inline-confirm-submit");
      check(unbanRequests.length === beforeEpic + 1 && unbanRequests.at(-1)?.accountId === epic,
        label("Epic persisted unban submits the exact pair without normalization"));
      await draw(locale, { read: "failure" }); await click('[role="tab"][data-list-key="manual"]'); await lookup(epic);
      const beforeEpicManual = manualUnbanRequests.length;
      await click(unban);
      check(element(".inline-confirm-message").textContent?.includes(epic), label("Epic manual unban confirms the full looked-up identity"));
      await click(".inline-confirm-submit");
      check(manualUnbanRequests.length === beforeEpicManual + 1 && manualUnbanRequests.at(-1)?.target === epic,
        label("Epic manual unban retains the exact declared action envelope"));
      const beforeEpicBan = manualBanRequests.length;
      const manualBan = '[data-manual-action="ban_player"] button';
      await click(manualBan);
      check(element(".inline-confirm-message").textContent?.includes(epic), label("Epic manual ban confirms the complete exact identity"));
      await click(".inline-confirm-submit");
      check(manualBanRequests.length === beforeEpicBan + 1 && manualBanRequests.at(-1)?.target === epic
        && manualBanRequests.at(-1)?.action_id === "ban_player", label("Epic manual ban retains the exact declared action envelope"));
      await lookup(epic.split("|")[0]);
      check(element<HTMLButtonElement>(unban).disabled && element<HTMLButtonElement>(manualBan).disabled,
        label("incomplete Epic manual identity cannot authorize ban or unban"));
    }
    await draw(locale, { read: "failure" }); await click('[role="tab"][data-list-key="manual"]'); await lookup(game.accounts[1]);
    const beforeManual = manualUnbanRequests.length;
    if (game.id === "squad") {
      check(element<HTMLButtonElement>(unban).disabled && manualUnbanRequests.length === beforeManual,
        label("Squad cannot fall back to an undeclared manual unban"));
    } else {
      check(!element<HTMLButtonElement>(unban).disabled && fixture?.querySelectorAll(unban).length === 1,
        label("initial native read failure preserves one explicit manual unban"));
      await click(unban);
      check(element('.inline-confirm-message').textContent?.includes(game.accounts[1]) && manualUnbanRequests.length === beforeManual,
        label("manual unban confirms the exact looked-up target"));
      await click('.inline-confirm-submit');
      check(manualUnbanRequests.length === beforeManual + 1 && manualUnbanRequests.at(-1)?.target === game.accounts[1]
        && manualUnbanRequests.at(-1)?.action_id === (game.id === "rimworld" ? "unban_user" : "unban_player")
        && fixture?.textContent?.includes(locale === "zh-CN" ? "解封命令已发送" : "Unban command sent")
        && !fixture?.textContent?.includes(locale === "zh-CN" ? "已确认解除封禁" : "confirms unban"),
        label("manual unban retains declared IPC without treating unavailable persistence as confirmation"));
      await draw(locale, { empty: true }); await click('[role="tab"][data-list-key="manual"]'); await lookup(game.accounts[1]);
      await click(unban); await click('.inline-confirm-submit');
      check(count() === "0" || !count(), label("manual unban does not require saved membership"));
      check(fixture?.textContent?.includes(locale === "zh-CN" ? "解封命令已发送" : "Unban command sent")
        && !fixture?.textContent?.includes(locale === "zh-CN" ? "已确认解除封禁" : "confirms unban"),
        label("empty native list cannot confirm a manual unban"));
    }
    await draw(locale, { read: "wrong-scope" });
    check(!count() && !fixture?.querySelector('[data-banned-account]') && element<HTMLButtonElement>(unban).disabled
      && Boolean(fixture?.querySelector('[role="alert"]')), label("mismatched module response cannot authorize a roster"));
    await draw(locale, { empty: true });
    check(count() === "0" && fixture?.textContent?.includes(locale === "zh-CN" ? "暂无被封禁玩家" : "No banned players"), label("confirmed empty native list is explicit"));
    const module = buildMockModuleDetails(game.id);
    const ban = module.runtime.player_list?.player_action_ids.find((id) => id === "ban_player" || id === "ban_user");
    if (ban) {
      await click(onlineTab); await click(".player-center-player-table .player-center-player-select");
      const action = Array.from(fixture?.querySelectorAll<HTMLButtonElement>(".player-center-member-action") ?? [])
        .find((button) => button.textContent?.trim() === (locale === "zh-CN"
          ? module.runtime.player_actions?.find((entry) => entry.id === ban)?.label_zh_cn : module.runtime.player_actions?.find((entry) => entry.id === ban)?.label));
      if (!action) throw new Error("Missing declared ban control");
      const beforeBanReads = reads;
      await act(async () => { action.click(); }); await click(".inline-confirm-submit");
      await settle(() => reads > beforeBanReads);
      await click(banTab);
      check(count() === "1" && liveRequests.at(-1)?.action_id === ban, label("online native ban refreshes persistence including ban_user"));
    }
  }
  game = games[0]; readBehavior = "deferred";
  entries = [{ account_id: game.accounts[0], display_name: null, identity_kind: game.kind }];
  await act(async () => { root.render(<ScopeProbe id={instanceId()} moduleId={game.id} />); });
  await settle(() => finishRead !== null);
  game = games[1]; readBehavior = "ready"; entries = [];
  await act(async () => { root.render(<ScopeProbe id={instanceId()} moduleId={game.id} />); });
  await settle(() => element('[data-scope-instance]').getAttribute('data-scope-instance') === instanceId());
  await act(async () => { finishRead?.(); });
  check(element('[data-scope-instance]').getAttribute('data-scope-instance') === instanceId()
    && element('[data-scope-module]').getAttribute('data-scope-module') === game.id
    && element('[data-scope-count]').getAttribute('data-scope-count') === "0"
    && element('[data-scope-stale]').getAttribute('data-scope-stale') === "false", "Late previous-instance read cannot replace current native roster");
  game = games.find((entry) => entry.id === "squad") ?? games[0];
  await draw("zh-CN", { stopped: true }); await choose(game.accounts[1]);
  const controls = element<HTMLElement>(".player-center-controls");
  await act(async () => { await Promise.all(controls.getAnimations({ subtree: true }).map((animation) => animation.finished)); });
  check(controls.scrollWidth <= controls.clientWidth + 1, "Native ban controls fit desktop viewport");
  check(errors.length === 0, "Browser and React report no errors");
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: false });
  return { status: "passed", checks, browser_errors: errors };
}
Object.assign(window, { __reliabilityFixtureCleanup: async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  await act(async () => { root.unmount(); }); console.error = originalError;
  return { browser_errors: errors, native_dialogs: 0 };
} });
void run().catch((error) => ({ status: "failed", error: String(error?.stack ?? error), checks, browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
