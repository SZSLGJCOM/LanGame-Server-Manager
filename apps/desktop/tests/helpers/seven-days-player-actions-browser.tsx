import React, { act, StrictMode, useState } from "react";
import { createRoot } from "react-dom/client";
import { buildMockModuleDetails } from "../../src/api-mock/module-details";
import { applyMockPlayerAccessMutation } from "../../src/api-mock/player-access";
import { createPlayerCenterState, derivePlayerCenterViewModel, reducePlayerCenterState } from "../../src/domain/live-player-state";
import { I18nProvider, useI18n } from "../../src/i18n";
import { OnlinePlayersView } from "../../src/views/servers/player-center/OnlinePlayersView";
import { ManualPlayerActions } from "../../src/views/servers/player-center/ManualPlayerActions";
import { readManualPlayerActions } from "../../src/views/servers/player-center/manual-player-action-model";
import { readPlayerAccessRosterCapabilities } from "../../src/views/servers/player-center/player-access-roster-model";
import { usePlayerAccess } from "../../src/views/servers/player-center/use-player-access";
import type { ExecuteInstancePlayerActionInput, ExecuteInstancePlayerActionResult, InstanceDetails, InstancePlayerAccessMutationInput, RuntimeLivePlayerEntry, RuntimeLivePlayerSnapshot } from "../../src/types";
import "../../src/app.css";
import "../../src/views/servers/workbench.css";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
localStorage.setItem("langame.locale", "zh-CN");
document.documentElement.dataset.theme = "dark";
const nonce = new URLSearchParams(location.search).get("nonce");
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
const originalError = console.error;
console.error = (...args) => { errors.push(args.map(String).join(" ")); originalError(...args); };
const moduleDetails = buildMockModuleDetails("sevendaystodie");
const moduleProperties = JSON.parse(moduleDetails.schema_json).properties;
const capabilities = readPlayerAccessRosterCapabilities(moduleDetails);
const manualActions = readManualPlayerActions(moduleDetails, capabilities);
const onlineActionIds = moduleDetails.runtime.player_list?.player_action_ids ?? [];
const mutations: InstancePlayerAccessMutationInput[] = [];
const liveTargets: string[] = [];
const nativeActions: ExecuteInstancePlayerActionInput[] = [];
const now = 1_000_000;
const steamId = "76561198000000001";
const storedBan = { platform: "Steam", userid: steamId, name: "Stored Alice", unbandate: "2036-02-29 23:59:58", reason: "Fixture reason" };
const syntheticBanReceipt = { ...storedBan, name: "Alice", unbandate: "2036-09-27 12:34:56", reason: "LanGame" };
// These explicit stable identities exercise UI capability handling, not the native collector.
const trustedPlayers: RuntimeLivePlayerEntry[] = [steamId, "76561198000000002"].map((value, index) => ({
  player_key: `trusted-${index}`, display_name: index ? "Bob" : "Alice", identifiers: [{ kind: "steam_id", value, stable: true }],
  available_action_ids: onlineActionIds, ping_ms: null, session_started_at_unix_ms: null, role: null, attributes: []
}));
const initialDetails: InstanceDetails = {
  summary: { id: "seven-days-actions", name: "7DTD actions", module_id: "sevendaystodie", status: "Stopped", active_process_count: 0,
    autostart: false, bind_ip: "127.0.0.1" }, ports: [], config_file_path: "", saves_path: "", backup_uses_declared_saves_path: false,
  auto_backup_on_stop: false, backup_retention_count: 3,
  settings_json: JSON.stringify({ admin_users: [], admin_groups: [], whitelist_users: [], whitelist_groups: [], blacklist_entries: [storedBan] })
};
function snapshot(entries: RuntimeLivePlayerEntry[] = [], stopped = false): RuntimeLivePlayerSnapshot {
  return { snapshot_id: `seven-days-${stopped}-${entries.map((row) => row.player_key).join("-")}`, instance_id: "seven-days-actions",
    status: stopped ? "stopped" : "ready", source: "runtime_action", observed_at_unix_ms: now, expires_at_unix_ms: now + 60_000,
    complete: true, truncated: false, stale: false, current_players: entries.length, max_players: 8, entries, issue: null };
}
let changeSnapshot: (value: RuntimeLivePlayerSnapshot) => void;
let changeBanDeclaration: (available: boolean) => void;
let completeNativeReceipt: (() => void) | null = null;
let checks = 0;
function Fixture() {
  const { locale, t } = useI18n();
  const [state, setState] = useState(() => createPlayerCenterState({ instanceId: "seven-days-actions", snapshot: snapshot([], true) }));
  const [details, setDetails] = useState(initialDetails);
  const [banDeclared, setBanDeclared] = useState(true);
  changeBanDeclaration = setBanDeclared;
  changeSnapshot = (value) => setState((current) => reducePlayerCenterState(current, { type: "snapshot-received", snapshot: value }));
  const access = usePlayerAccess({ details, moduleDetails, onApplyPlayerAccessMutation: async (input) => {
    const settings = JSON.parse(details.settings_json);
    const result = applyMockPlayerAccessMutation(moduleProperties[input.fieldKey], settings[input.fieldKey], input,
      state.snapshot?.status === "ready");
    mutations.push(input);
    liveTargets.push(result.liveTarget);
    setDetails((current) => ({ ...current, settings_json: JSON.stringify({ ...settings, [input.fieldKey]: result.value }) }));
    return { instanceId: input.instanceId, fieldKey: input.fieldKey, operation: input.operation,
      persistentStatus: result.changed ? "updated" : "unchanged", liveStatus: result.liveStatus, verificationStatus: result.verificationStatus };
  } });
  const presentation = derivePlayerCenterViewModel(state, now).livePlayers;
  async function executeWithSyntheticReceipt(input: ExecuteInstancePlayerActionInput): Promise<ExecuteInstancePlayerActionResult> {
    check(input.instance_id === details.summary.id && input.snapshot_id === state.snapshot?.snapshot_id
      && input.player_key === trustedPlayers[0].player_key && input.action_id === "ban_player", "Online ban lost its authorized snapshot references");
    nativeActions.push(input);
    // Substitute only the native command/readback boundary. The receipt updates the
    // owning details state just as the App's authoritative panel refresh does.
    return new Promise((resolve) => {
      completeNativeReceipt = () => {
        setDetails((current) => ({ ...current, settings_json: JSON.stringify({ ...JSON.parse(current.settings_json), blacklist_entries: [syntheticBanReceipt] }) }));
        resolve({ action_id: input.action_id, status: "sent", executed_at_unix_ms: now, summary: "Synthetic native receipt" });
        completeNativeReceipt = null;
      };
    });
  }
  return <main style={{ width: "1000px", height: "620px", margin: "24px auto", display: "grid", minHeight: 0 }}>
    <section className="server-detail-panel"><div className="detail-stack detail-stack--server">
      <div className="server-detail-subheader" style={{ minHeight: "42px" }}>7 Days to Die · 玩家</div>
      <div className="server-detail-scroll server-detail-scroll--players"><section className="player-access-workbench">
        <OnlinePlayersView moduleId="sevendaystodie" declaredActions={(moduleDetails.runtime.player_actions ?? []).filter((action) => banDeclared || action.id !== "ban_player")}
          playerActionIds={moduleDetails.runtime.player_list?.player_action_ids ?? []} access={access} error={null} loading={false}
          locale={locale} now={now} t={t} runtime={null} runtimeAvailable={state.snapshot?.status === "ready"}
          snapshot={state.snapshot} presentation={presentation} rosterCapabilities={capabilities} onRefresh={async () => {}}
          onExecutePlayerAction={executeWithSyntheticReceipt}
          onSelectPlayer={(playerKey) => setState((current) => reducePlayerCenterState(current, { type: "player-selected", playerKey }))}
          manualActions={(active) => <ManualPlayerActions moduleId="sevendaystodie" actions={manualActions} instanceId="seven-days-actions" locale={locale} t={t}
            disabled={!active || state.snapshot?.status !== "ready"} />} />
      </section></div>
    </div></section>
  </main>;
}
function check(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
function select<T extends Element>(selector: string): T {
  const value = document.querySelector<T>(selector); check(value, `Missing element: ${selector}`); return value;
}
async function waitFor(condition: () => boolean, label: string) {
  const deadline = performance.now() + 5000;
  while (!condition()) {
    check(performance.now() < deadline, `Timed out waiting for ${label}`);
    await act(async () => { await new Promise<void>((resolve) => setTimeout(resolve, 10)); });
  }
}
async function click(selector: string) {
  const target = select<HTMLButtonElement>(selector);
  check(target.getClientRects().length > 0 && !target.disabled, `Unavailable action: ${selector}`);
  target.scrollIntoView({ block: "nearest" }); await act(async () => { target.click(); });
}
async function input(selector: string, value: string) {
  const target = select<HTMLInputElement>(selector); check(!target.disabled, `Cannot edit locked identity: ${selector}`);
  await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(target, value);
    target.dispatchEvent(new Event("input", { bubbles: true })); });
}
async function state(value: RuntimeLivePlayerSnapshot) { await act(async () => { changeSnapshot(value); }); }
async function list(key: string) { await click(`.player-center-list-tabs [data-list-key="${key}"]`); }
const action = (field: string, operation = "add") => `.player-center-total-controls [data-roster-field="${field}"] [data-operation="${operation}"]`;
const nativeBanAction = ".player-center-member-action";
function banButton(): HTMLButtonElement | undefined {
  return Array.from(document.querySelectorAll<HTMLButtonElement>(nativeBanAction)).find((button) => button.textContent?.includes("封禁玩家（10年）"));
}
let initialActionInventory: string | null = null;
function fixedTitle() {
  const headers = document.querySelectorAll<HTMLElement>(".player-center-controls .player-center-member-header");
  check(headers.length === 1 && headers[0].parentElement?.matches(".player-center-controls")
    && headers[0].textContent?.trim() === "玩家操作", "Player action title is missing, duplicated, or replaced by a selected identity");
  const inventory = Array.from(document.querySelectorAll<HTMLElement>(".player-center-member-action, .player-access-selected-action, .player-access-edit-selected"))
    .map((button) => button.textContent?.trim()).join("|");
  initialActionInventory ??= inventory;
  check(inventory === initialActionInventory, "Changing list or target replaced the fixed total action inventory");
}
function personalActions(disabled: boolean) {
  for (const [field, label] of [["admin_users", "设为管理员"], ["whitelist_users", "加入白名单"]]) {
    const button = select<HTMLButtonElement>(action(field));
    check(button.disabled === disabled && button.textContent?.trim() === label, `${field} lacks its explicit action or correct disabled state`);
  }
  check(!document.querySelector(action("blacklist_entries")), "7DTD online controls duplicate native ban with a second persistent-add button");
  for (const field of ["admin_groups", "whitelist_groups"]) {
    check(select<HTMLButtonElement>(action(field)).disabled && select<HTMLButtonElement>(action(field, "remove")).disabled,
      "A personal online account is offered Steam group membership operations");
  }
  check(!document.querySelector(".player-center-total-controls")?.textContent?.includes("管理…"), "Controls still use generic management buttons");
}
async function run() {
  const root = createRoot(select<HTMLDivElement>("#fixture"));
  await act(async () => { await Promise.all([import("../../src/i18n-messages"), import("../../src/i18n-messages-zh-cn")]);
    root.render(<StrictMode><I18nProvider><Fixture /></I18nProvider></StrictMode>); });
  await waitFor(() => Boolean(document.querySelector('.player-center-list-tabs [data-list-key="online"]')), "localized player workspace mount");
  check(moduleDetails.summary.id === "sevendaystodie" && capabilities.length === 5, "Fixture is not using the real 7DTD roster declarations");
  fixedTitle(); personalActions(true);
  const stoppedActions = Array.from(document.querySelectorAll<HTMLButtonElement>(".player-center-member-action"));
  check(onlineActionIds.includes("kick_player") && onlineActionIds.includes("ban_player")
    && stoppedActions.length === onlineActionIds.length && stoppedActions.every((button) => button.disabled),
    "Stopped 7DTD did not retain its real declared online session actions in a disabled state");
  check(manualActions.every((item) => !onlineActionIds.includes(item.id)), "Manual entry duplicates an action declared for online selection"); checks++;
  await act(async () => { changeBanDeclaration(false); });
  check(select<HTMLButtonElement>(action("blacklist_entries")).disabled && !banButton(),
    "A module without the declared native ban lost its persistent blacklist add operation");
  await act(async () => { changeBanDeclaration(true); });
  personalActions(true); check(banButton()?.disabled, "Restoring the declared native ban lost its idle disabled button"); checks++;
  check(manualActions.length === 0 && !document.querySelector(".player-center-manual-actions"), "7DTD retains a redundant manual player-action list");
  await list("admin_groups"); fixedTitle(); await list("blacklist_entries"); fixedTitle();
  await click(".player-center-list-pane .player-access-roster-select"); fixedTitle();
  check(select<HTMLElement>(".player-center-member-heading").textContent?.includes(steamId),
    "Selecting the existing blacklist entry lost its offline identity");
  check(!select<HTMLButtonElement>(action("blacklist_entries", "remove")).disabled,
    "Stopped server cannot remove a persistent blacklist entry");
  check(!document.querySelector(".player-access-selected-editor"), "Selecting a roster entry opened an unrequested editor");
  await click(".player-access-edit-selected");
  check(select<HTMLInputElement>('.player-access-selected-editor input[id$="-unbandate"]').value === storedBan.unbandate,
    "Native blacklist expiry lost its time or seconds in the roster editor"); checks++;
  await list("online");
  const sessionOnly = { ...trustedPlayers[0], player_key: "native-session", available_action_ids: [], identifiers: [{ kind: "session_id" as const, value: "42", stable: false }] };
  await state(snapshot([sessionOnly])); fixedTitle(); personalActions(true);
  check(!document.querySelector(".player-center-player-table .player-center-player-select"), "A session-only row became a persistent account target"); checks++;
  await state(snapshot(trustedPlayers)); await click(".player-center-player-table tbody tr:first-child .player-center-player-select"); fixedTitle();
  check(!select<HTMLButtonElement>(action("blacklist_entries", "remove")).disabled, "Existing blacklist entry does not offer direct unban");
  check(banButton()?.disabled, "An already blacklisted online account is offered a second native ban");
  check(!document.querySelector(".player-access-selected-sync, .player-access-roster-action-timing"),
    "Player actions retain redundant persistent timing labels");
  await click(action("blacklist_entries", "remove"));
  check(mutations.length === 0 && !document.querySelector(".player-access-selected-editor")
    && select<HTMLElement>(".inline-confirm-review").textContent?.includes(steamId), "Unban opened a manager/editor or skipped target review");
  await click(".inline-confirm-submit");
  check(mutations.length === 1 && mutations[0].operation === "remove" && mutations[0].fieldKey === "blacklist_entries"
    && JSON.stringify(mutations[0].value) === JSON.stringify(storedBan), "Unban did not preserve the exact stored metadata"); checks++;
  check(liveTargets[0] === `Steam_${steamId}`, "Mock unban did not use the fully qualified 7DTD account target");
  check(!document.querySelector(action("blacklist_entries")) && banButton() && !banButton()!.disabled,
    "Removing a stored ban did not restore exactly one native online ban entry");
  await click(action("admin_users")); fixedTitle();
  check(select<HTMLElement>('.player-center-list-tabs [data-list-key="online"]').getAttribute("aria-selected") === "true", "Admin action navigated away from the online player");
  const identity = select<HTMLInputElement>(".player-access-selected-editor #player-access-quick-admin_users-userid");
  const platform = select<HTMLSelectElement>(".player-access-selected-editor #player-access-quick-admin_users-platform");
  check(identity.value === steamId && identity.disabled && platform.value === "Steam" && platform.disabled,
    "Trusted account identity was not automatically filled and locked");
  await input(".player-access-selected-editor #player-access-quick-admin_users-permission_level", "250"); checks++;
  await click(".player-center-player-table tbody tr:nth-child(2) .player-center-player-select");
  check(!document.querySelector(".player-access-selected-editor, .inline-confirm-review"), "Changing online target retained the previous account form");
  await click(action("admin_users"));
  check(select<HTMLInputElement>(".player-access-selected-editor #player-access-quick-admin_users-userid").value === trustedPlayers[1].identifiers[0].value
    && select<HTMLInputElement>(".player-access-selected-editor #player-access-quick-admin_users-permission_level").value === "0",
    "New target inherited the previous identity or permission draft");
  await list("whitelist_users"); await list("online");
  check(!document.querySelector(".player-access-selected-editor, .inline-confirm-review"), "Changing lists retained an online account draft"); checks++;
  await click(".player-center-player-table tbody tr:first-child .player-center-player-select");
  await click(action("admin_users"));
  await input(".player-access-selected-editor #player-access-quick-admin_users-permission_level", "250");
  await click(".player-access-selected-editor .player-access-roster-action-pair .inline-confirm-action button");
  check(mutations.length === 1 && select<HTMLElement>(".inline-confirm-review").textContent?.includes(steamId), "Admin permission save bypassed review");
  await click(".inline-confirm-submit");
  const admin = mutations[1]?.value as Record<string, unknown>;
  check(mutations.length === 2 && mutations[1].fieldKey === "admin_users" && admin.platform === "Steam"
    && admin.userid === steamId && admin.permission_level === 250, "Reviewed administrator parameters were not sent with the trusted identity");
  check(!document.querySelector(".player-access-selected-editor"), "Saved admin form did not close"); checks++;
  check(!select<HTMLButtonElement>(action("admin_users", "remove")).disabled && select<HTMLButtonElement>(action("admin_users")).disabled,
    "Existing administrator did not update its fixed add/remove availability");
  await click(action("admin_users", "remove")); await click(".inline-confirm-buttons button:first-child");
  check(mutations.length === 2, "Cancelling administrator removal changed the roster"); checks++;
  await list("admin_users"); fixedTitle();
  check(document.querySelectorAll(".player-center-list-pane .player-access-roster-entry").length === 1, "Saved admin did not appear in the left roster");
  await list("online"); fixedTitle();
  personalActions(true); checks++;
  await click(".player-center-player-table tbody tr:first-child .player-center-player-select");
  const ban = banButton(); check(ban && !ban.disabled, "Unlisted online player has no single active ban operation");
  await act(async () => { ban.click(); });
  check(nativeActions.length === 0 && select<HTMLElement>(".inline-confirm-review").textContent?.includes("封禁玩家（10年）"),
    "Native ban skipped review or lost its exact duration");
  await click(".inline-confirm-submit");
  check(nativeActions.length === 1 && completeNativeReceipt && mutations.length === 2,
    "Native ban was not routed once through the injected authoritative receipt boundary"); checks++;
  await act(async () => { completeNativeReceipt!(); });
  check(banButton()?.disabled && !select<HTMLButtonElement>(action("blacklist_entries", "remove")).disabled,
    "Authoritative ban receipt did not change the online operation to direct unban");
  await list("blacklist_entries"); fixedTitle();
  check(document.querySelectorAll(".player-center-list-pane .player-access-roster-entry").length === 1
    && select<HTMLElement>(".player-center-list-pane .player-access-roster-select").textContent?.includes(steamId),
    "Authoritative native ban did not appear in the sole left blacklist");
  check(!document.querySelector(".player-center-controls .player-access-roster-entries"), "Native ban created a second right-side roster"); checks++;
  const rosterEntry = select<HTMLButtonElement>(".player-center-list-pane .player-access-roster-select");
  if (rosterEntry.getAttribute("aria-pressed") !== "true") await click(".player-center-list-pane .player-access-roster-select");
  await click(".player-access-edit-selected");
  check(select<HTMLInputElement>('.player-access-selected-editor input[id$="-unbandate"]').value === syntheticBanReceipt.unbandate,
    "Native ban receipt expiry was truncated after switching to the blacklist");
  await click(".player-access-selected-editor .player-access-roster-action-pair > button");
  await click(action("blacklist_entries", "remove"));
  check(mutations.length === 2, "Left-list unban bypassed inline review");
  await click(".inline-confirm-submit");
  check(mutations.length === 3 && mutations[2].fieldKey === "blacklist_entries" && mutations[2].operation === "remove"
    && JSON.stringify(mutations[2].value) === JSON.stringify(syntheticBanReceipt) && liveTargets[2] === `Steam_${steamId}`
    && !document.querySelector(".player-center-list-pane .player-access-roster-entry"), "Left-list unban did not remove the exact native receipt and fully qualified target");
  await click(".player-access-roster-create");
  check(select<HTMLInputElement>('.player-access-selected-editor input[id$="-unbandate"]').value === "9999-12-31"
    && !select<HTMLElement>(".player-access-selected-editor .player-access-roster-editor").textContent?.includes("下次启动生效"),
    "Online action deduplication removed the left blacklist's independent persistent-add editor"); checks++;
  await list("online");
  await state(snapshot([], true)); fixedTitle(); personalActions(true);
  check(banButton()?.disabled, "Stopped unselected state lost the single disabled native ban action");
  check(!document.querySelector(".player-access-selected-editor, .inline-confirm-review"), "Stopped state retained an actionable target");
  check(errors.length === 0, `Browser errors: ${errors.join("; ")}`); checks++;
  return { status: "passed", checks, module_id: moduleDetails.summary.id,
    identity_fixture: "Synthetic stable Steam identities and native receipt; no native collector or live game server", browser_errors: errors };
}
let watchdog: ReturnType<typeof setTimeout>;
void Promise.race([run(), new Promise<never>((_, reject) => { watchdog = setTimeout(() => reject(new Error(`7DTD actions stalled after ${checks} checks`)), 25_000); })])
  .finally(() => clearTimeout(watchdog)).catch((error) => ({ status: "failed", checks, error: String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
