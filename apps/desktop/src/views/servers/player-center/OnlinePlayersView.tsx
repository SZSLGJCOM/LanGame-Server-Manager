import { useEffect, useId, useMemo, useState, type ReactNode } from "react";
import { executeInstancePlayerAction } from "../../../api";
import { ShellIcon } from "../../../components/ShellIcon";
import type { LivePlayerPresentation } from "../../../domain/live-player-state";
import type { LocaleCode, TranslateFn } from "../../../i18n";
import type {
  ExecuteInstancePlayerActionInput,
  ExecuteInstancePlayerActionResult,
  InstanceRuntimeOverview,
  InstanceDetails,
  ModulePlayerActionDetails,
  RuntimeLivePlayerEntry,
  RuntimeLivePlayerSnapshot
} from "../../../types";
import { LivePlayerActionPanel } from "./LivePlayerActionPanel";
import { LivePlayerState } from "./LivePlayerState";
import { LivePlayerTable } from "./LivePlayerTable";
import type { PlayerAccessRosterCapability } from "./player-access-roster-model";
import { PlayerAccessRosterList } from "./PlayerAccessRosterList";
import { SelectedPlayerRosterActions } from "./SelectedPlayerRosterActions";
import { SelectedPlayerExtraDetails } from "./SelectedPlayerDetails";
import { ValheimAccountControlSetup } from "./ValheimAccountControlSetup";
import { ManualPlayerHostContext } from "./ManualPlayerActions";
import { isEnshroudedNativeBanRoster, isPlayerRosterCapability, resolveManualRosterTarget, resolveSelectedRosterIdentity } from "./player-access-selected-target";
import { ConfigurationHelp } from "../../settings/ConfigurationFieldHelp";
import { nativeBanEntryForPlayer, nativeBanPlayer, matchNativeBanLivePlayer, matchRosterLivePlayer, rosterEntryPlayer } from "./player-center-selection";
import type { PlayerAccessState } from "./use-player-access";
import { NativeBanRosterActions, NativeBanRosterList } from "./NativeBanRoster";
import { nativeBanRosterModules, useNativeBanRoster } from "./use-native-ban-roster";
import { manualPlayerActionTargetError } from "./manual-player-action-model";
import { useTabStripOverflow } from "./use-tab-strip-overflow";
import { enshroudedPlayerManagementUnavailableReason } from "./enshrouded-extension-status";

interface OnlinePlayersViewProps {
  history?: ReactNode;
  readOnly?: boolean;
  access?: PlayerAccessState;
  declaredActions: ModulePlayerActionDetails[];
  playerActionIds: string[];
  error: string | null;
  loading: boolean;
  locale: LocaleCode;
  moduleId?: string;
  /** Format check and hint for manual-ID actions; rosters validate the ID with their own codecs. */
  manualTargetFormat?: { validate: (target: string) => string | null; hint: string | null };
  /** Module-owned categories (e.g. Moria permissions) that live in the same left list instead of separate cards. */
  extraLists?: { key: string; title: string; content: ReactNode }[];
  manualActions?: (active: boolean, onActionCompleted?: () => Promise<void>, onActionFailed?: () => void) => ReactNode;
  now: number;
  onOpenSettings?: () => void;
  onRefresh: () => Promise<void>;
  onExecutePlayerAction?: (input: ExecuteInstancePlayerActionInput) => Promise<ExecuteInstancePlayerActionResult>;
  onSelectPlayer: (playerKey: string | null) => void;
  presentation: LivePlayerPresentation;
  rosterCapabilities?: PlayerAccessRosterCapability[];
  runtime: InstanceRuntimeOverview | null;
  runtimeAvailable: boolean;
  instanceDetails?: InstanceDetails;
  onPrepareValheimControl?: (instanceId: string) => Promise<InstanceDetails>;
  snapshot: RuntimeLivePlayerSnapshot | null;
  t: TranslateFn;
}

function formatObservedAt(locale: LocaleCode, observedAt: number | null): string | null {
  if (observedAt === null) {
    return null;
  }
  return new Intl.DateTimeFormat(locale === "zh-CN" ? "zh-CN" : "en-US", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit"
  }).format(new Date(observedAt));
}

export function OnlinePlayersView(props: OnlinePlayersViewProps) {
  const tabId = useId();
  const [activeListKey, setActiveListKey] = useState("online");
  const [historyVisible, setHistoryVisible] = useState(false);
  const [rosterSelection, setRosterSelection] = useState<{ fieldKey: string; entryKey: string } | null>(null);
  const [selectionVersion, setSelectionVersion] = useState(0);
  const [selectedBannedAccountId, setSelectedBannedAccountId] = useState<string | null>(null);
  const [nativeUnbanBusy, setNativeUnbanBusy] = useState(false);
  const [manualActionsHost, setManualActionsHost] = useState<HTMLElement | null>(null);
  const [manualDraft, setManualDraft] = useState("");
  const [manualLookedUp, setManualLookedUp] = useState("");
  // Actions apply to the ID that was explicitly looked up; editing the field deselects it.
  const manualTarget = manualLookedUp && manualLookedUp === manualDraft.trim() ? manualLookedUp : "";
  const dragonwilds = props.moduleId === "runescapedragonwilds";
  const nativeBans = nativeBanRosterModules.has(props.moduleId ?? "");
  const nativeUnban = nativeBans && (!dragonwilds || props.declaredActions.some((action) => action.id === "unban_player"));
  const bannedTabKey = dragonwilds ? "dragonwilds-banned" : "native-banned";
  // The same native unban serves list, online and manual selections. Other
  // manual actions keep their controls; only the duplicated unban is suppressed.
  const bans = useNativeBanRoster(nativeBans ? props.instanceDetails?.summary.id ?? null : null, nativeBans ? props.moduleId ?? null : null);
  const bannedTab = nativeBans && activeListKey === bannedTabKey;
  const selectedBan = bans.roster?.entries.find((entry) => entry.account_id === selectedBannedAccountId) ?? null;
  const fields = props.access?.fields ?? [];
  const activeField = fields.find((field) => field.key === activeListKey);
  // 手动查询 exists for manual-ID actions and for any writable roster (its add/remove buttons take the ID).
  const canManual = Boolean(props.manualActions) || fields.some((field) => isPlayerRosterCapability(field));
  const manualTab = canManual && activeListKey === "manual";
  // Enshrouded's native unban and its ban-roster removal are one operation: the backend unbans natively while the
  // server runs and edits the saved roster while it is stopped. One 解封 button replaces both.
  const mergedUnbanField = props.moduleId === "enshrouded" && props.declaredActions.some((action) => action.id === "unban_player")
    ? fields.find(isEnshroudedNativeBanRoster) : undefined;
  const mergedUnbanKey = mergedUnbanField?.key ?? null;
  const manualHosts = useMemo(() => ({ actions: dragonwilds && nativeUnban ? null : manualActionsHost, target: manualTab ? manualTarget : "",
    suppressedActionIds: nativeUnban ? ["unban_player", "unban_user"] : mergedUnbanKey ? ["unban_player"] : undefined }),
  [manualActionsHost, dragonwilds, nativeUnban, manualTab, manualTarget, mergedUnbanKey]);
  const manualDraftValue = manualDraft.trim();
  const manualDraftError = !manualDraftValue ? null
    : (props.manualTargetFormat && !props.manualTargetFormat.validate(manualDraftValue))
      || fields.some((field) => resolveManualRosterTarget(manualDraftValue, field)) ? null
      : props.manualTargetFormat?.validate(manualDraftValue)
        ?? props.t("servers.playerCenter.manual.noMatch", undefined, "This ID does not match any available action or list.");
  const extraList = !activeField && !manualTab && !bannedTab ? props.extraLists?.find((list) => list.key === activeListKey) : undefined;
  const online = !activeField && !bannedTab && !manualTab && !extraList;
  const activeKey = manualTab ? "manual" : bannedTab ? bannedTabKey : extraList?.key ?? activeField?.key ?? "online";
  const saving = Boolean(props.access?.busyFieldKey) || nativeUnbanBusy;
  // The lookup stays usable while any list can take the ID; live-only targets need a running, writable server.
  const manualInputDisabled = saving || ((props.readOnly || !props.runtimeAvailable || props.snapshot?.status === "stopped")
    && (Boolean(props.access?.disabled) || !fields.some((field) => isPlayerRosterCapability(field))));
  const tabs = [{ key: "online", title: props.t("servers.playerCenter.online.title", undefined, "Online members") },
    ...(nativeBans ? [{ key: bannedTabKey, title: selectBanText("封禁列表", "Ban list") }] : []),
    ...fields.map((field) => ({ key: field.key, title: field.title })),
    ...(props.extraLists ?? []).map((list) => ({ key: list.key, title: list.title })),
    // 手动查询: the ID lookup is its own category; its actions use the shared right-side controls.
    ...(canManual ? [{ key: "manual", title: props.t("servers.playerCenter.manual.tab", undefined, "Manual lookup") }] : [])];
  const tabStrip = useTabStripOverflow<HTMLDivElement>(activeKey, tabs.map((tab) => `${tab.key}:${tab.title}`).join("|"));
  function selectBanText(zh: string, en: string) { return props.locale === "zh-CN" ? zh : en; }
  function selectList(key: string) {
    if (!saving) { clearSelection(); setActiveListKey(key); }
  }
  function clearSelection() {
    setRosterSelection(null);
    setSelectedBannedAccountId(null);
    props.onSelectPlayer(null);
    setSelectionVersion((current) => current + 1);
  }
  useEffect(() => { clearSelection(); setManualDraft(""); setManualLookedUp(""); }, [props.instanceDetails?.summary.id, props.snapshot?.instance_id]);
  useEffect(() => { setNativeUnbanBusy(false); }, [props.instanceDetails?.summary.id]);
  useEffect(() => {
    if (!bans.stale && bans.roster && selectedBannedAccountId && !selectedBan) setSelectedBannedAccountId(null);
  }, [bans.roster, bans.stale, selectedBannedAccountId, selectedBan]);
  async function refreshAfterModeration() {
    const results = await Promise.allSettled([
      ...(props.moduleId === "squad" && !props.runtimeAvailable ? [] : [props.onRefresh()]),
      ...(nativeBans ? [bans.refreshAfterMutation()] : [])
    ]);
    const failed = results.find((result) => result.status === "rejected");
    if (failed?.status === "rejected") throw failed.reason;
    if (results.some((result) => result.status === "fulfilled" && result.value === false)) {
      throw new Error(selectBanText("封禁名单刷新失败，请重试。", "Could not refresh the ban roster. Retry the refresh."));
    }
  }
  async function executeLiveAction(input: ExecuteInstancePlayerActionInput) {
    try { return await (props.onExecutePlayerAction ?? executeInstancePlayerAction)(input); }
    catch (cause) { if (nativeBans && (input.action_id === "ban_player" || input.action_id === "ban_user")) bans.invalidate(); throw cause; }
  }
  const selectedField = fields.find((field) => field.key === rosterSelection?.fieldKey);
  const selectedEntry = selectedField?.entries.find((entry) => entry.key === rosterSelection?.entryKey);
  const selectedRoster = selectedField && selectedEntry ? { field: selectedField, entry: selectedEntry } : null;
  const selectedPlayer = bannedTab ? nativeBanPlayer(selectedBan)
    : manualTab ? null : rosterSelection ? rosterEntryPlayer(selectedRoster) : props.presentation.selectedPlayer;
  const matchedLivePlayer = bannedTab ? !bans.stale && !bans.loading && !bans.error
    ? matchNativeBanLivePlayer(selectedBan, props.snapshot, props.now) : null
    : manualTab ? null : rosterSelection ? matchRosterLivePlayer(selectedRoster, props.snapshot, props.now) : props.presentation.selectedPlayer;
  const unbanEntry = bannedTab ? selectedBan : manualTab ? bans.roster?.entries.find((entry) => entry.account_id === manualTarget) ?? null
    : props.presentation.authoritative && !props.error ? nativeBanEntryForPlayer(selectedPlayer, bans.roster?.entries ?? []) : null;
  const manualUnbanAction = manualTab && !dragonwilds ? props.declaredActions.find((action) =>
    action.id === (props.moduleId === "rimworld" ? "unban_user" : "unban_player")) : undefined;
  const manualUnban = manualUnbanAction && manualTarget ? { target: manualTarget, actionId: manualUnbanAction.id,
    error: manualPlayerActionTargetError(props.moduleId ?? "", manualUnbanAction, manualTarget, props.locale) } : undefined;
  const selectedNativeBanned = nativeBans && Boolean(unbanEntry);
  const currentPlayers = props.snapshot?.current_players ?? props.runtime?.players.current_players ?? null;
  const maxPlayers = props.snapshot?.max_players ?? props.runtime?.players.max_players ?? null;
  const countLabel = currentPlayers === null
    ? null
    : maxPlayers === null
      ? props.t("servers.playerCenter.online.count", { count: currentPlayers }, "{count} online")
      : `${currentPlayers} / ${maxPlayers}`;
  const observedAt = formatObservedAt(props.locale, props.snapshot?.observed_at_unix_ms ?? null);
  const declaredPlayerActionIds = [...new Set(props.playerActionIds)]
    .filter((actionId) => props.declaredActions.some((action) => action.id === actionId)
      && (!nativeUnban || actionId !== "unban_player" && actionId !== "unban_user"));
  // Native bans record their exact result in the corresponding persistent roster.
  const nativeBanField = !declaredPlayerActionIds.includes("ban_player") ? undefined
    : props.moduleId === "sevendaystodie" ? fields.find((field) => field.key === "blacklist_entries")
      : props.moduleId === "enshrouded" ? fields.find(isEnshroudedNativeBanRoster) : undefined;
  const enshroudedRoster = fields.find(isEnshroudedNativeBanRoster)
    ?? props.rosterCapabilities?.find(isEnshroudedNativeBanRoster);
  function hasEnshroudedExtensionIdentity(player: RuntimeLivePlayerEntry | null): boolean {
    return props.snapshot?.source === "file_ipc" && Boolean(enshroudedRoster
      && resolveSelectedRosterIdentity(player, enshroudedRoster));
  }
  const enshroudedNativeActionsDeclared = declaredPlayerActionIds.some((id) => id === "kick_player" || id === "ban_player");
  const inspectedPlayer = matchedLivePlayer ?? selectedPlayer;
  const enshroudedExtensionIdentityAvailable = inspectedPlayer ? hasEnshroudedExtensionIdentity(inspectedPlayer)
    : props.presentation.rows.some(hasEnshroudedExtensionIdentity)
      || (props.snapshot?.source === "file_ipc" && props.presentation.authoritative && props.presentation.rows.length === 0);
  const selectedBanIdentity = nativeBanField
    ? resolveSelectedRosterIdentity(selectedPlayer, nativeBanField)?.identity
      ?? (selectedRoster?.field.key === nativeBanField.key ? selectedRoster.entry.key : null) : null;
  const selectedPlayerBanned = selectedBanIdentity && nativeBanField?.entries.some((entry) => entry.key === selectedBanIdentity);
  const actionIds = declaredPlayerActionIds;
  const managementUnavailableReason = props.moduleId === "enshrouded"
    ? enshroudedPlayerManagementUnavailableReason(props.snapshot?.issue, props.locale, props.t) : null;
  const missingEnshroudedExtensionIdentity = props.moduleId === "enshrouded"
    && (Boolean(managementUnavailableReason) || !hasEnshroudedExtensionIdentity(matchedLivePlayer));
  const enshroudedModerationProtected = props.moduleId === "enshrouded"
    && matchedLivePlayer?.attributes.some((attribute) => attribute.key === "moderation_immune" && attribute.value === "true");
  const protectedPlayerReason = props.t("servers.playerCenter.enshrouded.moderationProtected", undefined,
    "The server protects this player; native permissions do not allow kicking or banning them.");
  const disabledActionReasons = {
    ...(selectedPlayerBanned || selectedNativeBanned ? { ban_player: props.t("servers.playerCenter.actions.alreadyBanned", undefined, "This player is already in the ban roster."),
      ban_user: props.t("servers.playerCenter.actions.alreadyBanned", undefined, "This player is already in the ban roster.") } : {}),
    ...(enshroudedModerationProtected ? { kick_player: protectedPlayerReason, ban_player: protectedPlayerReason } : {})
  };
  const runtimePlayer = matchedLivePlayer && (selectedPlayerBanned || selectedNativeBanned || missingEnshroudedExtensionIdentity || enshroudedModerationProtected)
    ? { ...matchedLivePlayer, available_action_ids: matchedLivePlayer.available_action_ids.filter((id) =>
      !missingEnshroudedExtensionIdentity && (!(selectedPlayerBanned || selectedNativeBanned) || id !== "ban_player" && id !== "ban_user")
      && (!enshroudedModerationProtected || (id !== "kick_player" && id !== "ban_player"))) } : matchedLivePlayer;
  const showPlayerPanel = actionIds.length > 0;
  const hasControls = Boolean(showPlayerPanel || canManual || fields.length > 0 || nativeBans);
  const showMemberPane = hasControls || props.presentation.tableVisible;
  // A persistent read-only condition is explained by a muted ⓘ on the 在线成员 category, not a standing notice.
  const onlineInfo = [props.t("servers.playerCenter.online.info", undefined, "Only members the server reliably confirms are shown."), !hasControls
    ? props.t("servers.playerCenter.actions.empty", undefined, "Player information is read-only for this server.")
    : props.moduleId === "enshrouded" && !enshroudedNativeActionsDeclared
      ? props.t("servers.playerCenter.enshrouded.managementUnavailable", undefined,
        "No native admin interface connected; kick and immediate ban are unavailable. Edit the ban roster while stopped; it applies on start.")
      : props.moduleId === "enshrouded" && !enshroudedExtensionIdentityAvailable
        ? managementUnavailableReason ?? props.t("servers.playerCenter.enshrouded.extensionIdentityUnavailable", undefined,
          "No verified player identity from the extension; kick and ban are unavailable and the list is view-only.")
        : null].filter(Boolean).join(" ");
  const runtimeUnavailableReason = props.readOnly
    ? props.t("servers.playerCenter.actions.readOnly", undefined, "This workspace is read-only.")
    : nativeUnbanBusy
      ? props.t("servers.playerCenter.actions.waitForPending", undefined, "A player action is in progress. Wait for it to finish.")
    : !props.runtimeAvailable
      ? props.t("servers.playerCenter.actions.serverStopped", undefined, "The server is not running; live actions are unavailable.")
      : props.error
        ? props.t("servers.playerCenter.actions.listReadFailed", undefined, "The player list could not be read. Refresh it before retrying.")
        : props.presentation.refreshReason === "expired"
          ? props.t("servers.playerCenter.actions.snapshotExpired", undefined, "The player list has expired. Refresh it before running an action.")
          : managementUnavailableReason
            ? managementUnavailableReason
            : !props.presentation.authoritative
              ? props.t("servers.playerCenter.actions.snapshotNotCurrent", undefined, "The player snapshot is not complete and current. Refresh it before retrying.")
              : (rosterSelection || bannedTab) && !runtimePlayer
                ? props.t("servers.playerCenter.actions.noUniqueOnlineMatch", undefined, "This entry does not uniquely match a current online player.")
                : missingEnshroudedExtensionIdentity && inspectedPlayer
                  ? props.t("servers.playerCenter.enshrouded.extensionIdentityUnavailable", undefined,
                    "No verified player identity from the extension; kick and ban are unavailable and the list is view-only.")
                  : null;

  return (
    <ManualPlayerHostContext.Provider value={manualHosts}>
    <div className={`player-center-member-layout${showMemberPane ? "" : " is-read-only"}`}>
      <section className="player-center-list-pane" aria-label={props.t("servers.playerCenter.lists.label", undefined, "Player lists")} aria-busy={online ? props.loading : bannedTab && bans.loading}>
        <header className="player-center-pane-header">
          <div ref={tabStrip} className="player-center-list-tabs" role="tablist" aria-label={props.t("servers.playerCenter.lists.label", undefined, "Player lists")}
            onKeyDown={(event) => {
              const index = tabs.findIndex((tab) => tab.key === activeKey);
              const nextIndex = event.key === "ArrowRight" ? (index + 1) % tabs.length
                : event.key === "ArrowLeft" ? (index + tabs.length - 1) % tabs.length
                  : event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1 : null;
              if (nextIndex === null || saving) return;
              event.preventDefault();
              selectList(tabs[nextIndex].key);
              event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="tab"]')[nextIndex]?.focus();
            }}>
            {tabs.map((tab) => {
              const count = tab.key === "online" ? countLabel : tab.key === "manual" ? null : tab.key === bannedTabKey
                ? !bans.stale && !bans.loading && !bans.error ? bans.roster?.entries.length : null
                : fields.find((field) => field.key === tab.key)?.entries.length;
              return <button key={tab.key} id={`${tabId}-${tab.key}`} type="button" role="tab"
                data-list-key={tab.key} aria-selected={activeKey === tab.key} aria-controls={`${tabId}-panel`}
                tabIndex={activeKey === tab.key ? 0 : -1} disabled={saving}
                className="player-center-list-tab" onClick={() => selectList(tab.key)}>
                <span>{tab.title}</span>{tab.key === "online" && onlineInfo ? <ConfigurationHelp description={onlineInfo}>{(help) =>
                  <span className="player-center-list-info" ref={help.anchorRef} {...help.interactionProps} aria-describedby={help.descriptionId}>
                    <ShellIcon name="alert-circle" className="maintenance-help-icon" /></span>}</ConfigurationHelp> : null}{count !== null && count !== undefined ? <span className="player-center-list-count">{count}</span> : null}
              </button>;
            })}
          </div>
          {online ? <div className="player-center-pane-tools">
            {props.snapshot?.stale ? <span className="page-chip" title={props.t("servers.playerCenter.online.stale", undefined, "Stale")}>{props.t("servers.playerCenter.online.stale", undefined, "Stale")}</span> : null}
            {props.snapshot?.truncated ? <span className="page-chip" title={props.t("servers.playerCenter.online.truncated", undefined, "Truncated")}>{props.t("servers.playerCenter.online.truncated", undefined, "Truncated")}</span> : null}
            {observedAt ? <span className="page-chip player-center-observed-chip" title={props.t("servers.playerCenter.online.observedAt", undefined, "Observed at")}>{observedAt}</span> : null}
            <button
              type="button"
              className="secondary-button player-center-refresh-button"
              disabled={props.readOnly || !props.runtimeAvailable || props.loading || props.snapshot?.status === "stopped"}
              onClick={() => void props.onRefresh()}
            >
              <ShellIcon name="refresh" className={props.loading ? "is-spinning" : ""} />
              {props.loading
                ? props.t("servers.playerCenter.online.refreshing", undefined, "Refreshing")
                : props.t("servers.playerCenter.online.refresh", undefined, "Refresh")}
            </button>
          </div> : bannedTab ? <button type="button" className="secondary-button player-center-refresh-button"
            disabled={saving || bans.loading} onClick={() => void bans.refresh()}>
            <ShellIcon name="refresh" className={bans.loading ? "is-spinning" : ""} />
            {bans.loading ? selectBanText("正在刷新", "Refreshing") : selectBanText("刷新", "Refresh")}
          </button> : null}
        </header>
        {props.history ? <div className="player-center-history-tabs" role="tablist" aria-label={selectBanText("玩家记录视图", "Player record view")}
          onKeyDown={(event) => {
            if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
            event.preventDefault();
            const next = event.key === "Home" ? false : event.key === "End" ? true : !historyVisible;
            setHistoryVisible(next);
            event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="tab"]')[Number(next)]?.focus();
          }}>
          {[false, true].map((history) => <button key={String(history)} type="button" role="tab" className="player-center-list-tab"
            id={`${tabId}-${history ? "history" : "current"}-tab`} aria-selected={historyVisible === history} tabIndex={historyVisible === history ? 0 : -1}
            aria-controls={`${tabId}-${history ? "history" : "panel"}`} onClick={() => setHistoryVisible(history)}>
            {history ? selectBanText("历史记录", "History") : selectBanText("玩家列表", "Player lists")}
          </button>)}
        </div> : null}
        {historyVisible && props.history ? <div id={`${tabId}-history`} role="tabpanel" aria-labelledby={`${tabId}-history-tab`} className="player-center-list-content">{props.history}</div> : null}
        <div id={`${tabId}-panel`} role="tabpanel" aria-labelledby={`${tabId}-${activeKey}`} className="player-center-list-content" style={historyVisible ? { display: "none" } : undefined}>
        {/* 手动查询: only the ID field and lookup; the looked-up ID becomes the target of the shared action stack. */}
        {manualTab ? <form className="settings-schema-grid player-center-manual-lookup" aria-label={props.t("servers.playerCenter.manual.tab", undefined, "Manual lookup")}
          onSubmit={(event) => { event.preventDefault(); if (manualDraftValue && !manualDraftError) setManualLookedUp(manualDraftValue); }}>
          <div className="settings-schema-field">
            <ConfigurationHelp description={[props.t("servers.playerCenter.manual.description", undefined,
              "Enter an exact player identifier obtained from the server to run an action."), props.manualTargetFormat?.hint].filter(Boolean).join(" ")}>{(help) =>
              <label className="detail-label" htmlFor={`manual-player-target-${props.instanceDetails?.summary.id ?? props.snapshot?.instance_id ?? "players"}`}
                ref={help.anchorRef} {...help.interactionProps} aria-describedby={help.descriptionId}>
                {props.t("servers.playerCenter.manual.playerId", undefined, "Player ID")}<ShellIcon name="alert-circle" className="maintenance-help-icon" />
              </label>}</ConfigurationHelp>
            <input id={`manual-player-target-${props.instanceDetails?.summary.id ?? props.snapshot?.instance_id ?? "players"}`} type="text"
              className="settings-schema-input" value={manualDraft} aria-invalid={Boolean(manualDraftError)}
              aria-describedby={manualDraftError ? `${tabId}-manual-error` : undefined} disabled={manualInputDisabled}
              placeholder={props.t("servers.playerCenter.manual.playerIdPlaceholder", undefined, "Exact player ID")}
              onChange={(event) => setManualDraft(event.target.value)} />
            {manualDraftError ? <span className="form-note form-note--error" role="alert" id={`${tabId}-manual-error`}>{manualDraftError}</span> : null}
          </div>
          <div className="player-center-manual-lookup-action">
            <button type="submit" className="secondary-button player-center-manual-use" disabled={manualInputDisabled || !manualDraftValue || Boolean(manualDraftError)}>
              <ShellIcon name="search" />{props.t("servers.playerCenter.manual.lookup", undefined, "Look up")}
            </button>
          </div>
        </form> : null}
        {extraList ? <div className="player-center-extra-list">{extraList.content}</div> : null}
        {manualTab || extraList ? null : bannedTab ? <NativeBanRosterList state={bans} selectedAccountId={selectedBannedAccountId}
          disabled={saving} locale={props.locale} onSelect={(accountId) => {
            const next = selectedBannedAccountId === accountId ? null : accountId;
            clearSelection(); setSelectedBannedAccountId(next);
          }} /> : activeField ? <PlayerAccessRosterList field={activeField} locale={props.locale}
          disabled={saving} selectedEntryKey={selectedRoster?.field.key === activeField.key ? selectedRoster.entry.key : null}
          onSelect={(entry) => { clearSelection(); setRosterSelection(rosterSelection?.fieldKey === activeField.key && rosterSelection.entryKey === entry.key
            ? null : { fieldKey: activeField.key, entryKey: entry.key }); }} /> : <>

        {props.presentation.tableVisible ? (
          <>
            {props.presentation.stateVisible || props.error ? (
              <LivePlayerState
                readOnly={props.readOnly}
                moduleId={props.moduleId}
                error={props.error}
                locale={props.locale}
                onOpenSettings={props.onOpenSettings}
                presentation={props.presentation}
                snapshot={props.snapshot}
              />
            ) : null}
            <LivePlayerTable
              locale={props.locale}
              now={props.now}
              onSelect={(playerKey) => { clearSelection(); props.onSelectPlayer(playerKey === props.presentation.selectedPlayerKey ? null : playerKey); }}
              rows={props.presentation.rows}
              rosterCapabilities={props.rosterCapabilities}
              selectedPlayerKey={props.presentation.selectedPlayerKey}
              stale={!props.presentation.authoritative || Boolean(props.error)}
            />
          </>
        ) : (
          <LivePlayerState
            readOnly={props.readOnly}
            moduleId={props.moduleId}
            error={props.error}
            locale={props.locale}
            onOpenSettings={props.onOpenSettings}
            presentation={props.presentation}
            snapshot={props.snapshot}
          />
        )}
        </>}
        </div>
      </section>

      {showMemberPane ? <div className="player-center-controls">
        <header className="player-center-member-header">
          <span className="player-center-member-avatar" aria-hidden="true"><ShellIcon name="user" /></span>
          <strong>{props.t("servers.playerCenter.member.detailsAndActions", undefined, "Player management")}</strong>
        </header>
        <div className="player-center-total-controls">
        {/* The selected left row already shows name, primary ID, session, ping, role and status; only extras appear here. */}
        {selectedPlayer && !selectedRoster ? <SelectedPlayerExtraDetails locale={props.locale} player={selectedPlayer}
            sessionUnknownReason={props.moduleId === "enshrouded" && props.snapshot?.source === "server_query" ? props.t("servers.playerCenter.enshrouded.sessionUnknown", undefined,
              "Anonymous query durations cannot be reliably matched to player names, and this roster has no usable join time for this player.") : undefined} />
          : manualTab && manualTarget ? <p className="player-center-target-line">
            {props.t("servers.playerCenter.manual.targetLine", { target: manualTarget }, "Target: {target}")}</p> : null}
        {props.moduleId === "valheim" && props.instanceDetails ? <ValheimAccountControlSetup key={props.instanceDetails.summary.id}
          details={props.instanceDetails} locale={props.locale} readOnly={props.readOnly} onPrepare={props.onPrepareValheimControl}
          connected={props.presentation.authoritative && (props.snapshot?.source === "file_ipc"
            || props.presentation.rows.some((player) => player.identifiers.some((identifier) => identifier.kind === "valheim_platform_id" && identifier.stable)))} /> : null}
        {showPlayerPanel ? (
          <LivePlayerActionPanel
            actionIds={actionIds}
            actionsEnabled={!props.readOnly && !nativeUnbanBusy && props.runtimeAvailable && !props.error
              && (rosterSelection || bannedTab ? Boolean(runtimePlayer) : props.presentation.actionsEnabled)}
            declaredActions={props.declaredActions}
            disabledReason={runtimeUnavailableReason}
            disabledNotice={props.moduleId === "enshrouded" && inspectedPlayer
              ? runtimeUnavailableReason ?? (enshroudedModerationProtected ? protectedPlayerReason : null) : null}
            disabledActionReasons={disabledActionReasons}
            locale={props.locale}
            onActionCompleted={nativeBans ? refreshAfterModeration : props.onRefresh}
            onExecute={executeLiveAction}
            player={runtimePlayer}
            snapshot={props.snapshot}
            t={props.t}
          />
        ) : null}
        {props.access && fields.length > 0 ? <SelectedPlayerRosterActions key={JSON.stringify([activeKey, selectionVersion, selectedPlayer?.player_key, selectedPlayer?.identifiers, manualTab ? manualTarget : null])}
          moduleId={props.moduleId}
          selectedPlayer={selectedPlayer} selectedRoster={selectedRoster} manualTarget={manualTab && manualTarget ? manualTarget : undefined} onClearSelection={clearSelection}
          suppressedAddFieldKeys={nativeBanField && !manualTab ? [nativeBanField.key] : undefined}
          fields={fields} busyFieldKey={props.access.busyFieldKey}
          disabled={props.access.disabled || nativeUnbanBusy} locale={props.locale} onMutate={props.access.onMutate}
          disabledReason={props.access.disabledReason}
          mergedUnban={mergedUnbanField ? { fieldKey: mergedUnbanField.key, live: props.runtimeAvailable && !props.readOnly,
            liveUnavailableReason: props.snapshot?.issue?.code === "extension_unavailable" ? managementUnavailableReason : null } : undefined}
          /> : null}
        {nativeUnban && props.instanceDetails && props.moduleId ? <NativeBanRosterActions key={props.instanceDetails.summary.id} instanceId={props.instanceDetails.summary.id} moduleId={props.moduleId}
          entry={unbanEntry} hasSelection={Boolean(selectedPlayer || manualTab && manualTarget)} state={bans}
          manualUnban={manualUnban}
          readOnly={Boolean(props.readOnly)} running={props.runtimeAvailable} locale={props.locale}
          onBusyChange={setNativeUnbanBusy} onCompleted={refreshAfterModeration} /> : null}
        {/* Identity actions sit with every other action; they need a 手动查询 target and stay disabled otherwise. */}
        {props.manualActions && !(dragonwilds && nativeUnban) ? <div className="player-center-manual-host" ref={setManualActionsHost} /> : null}
        </div>
        {props.access?.feedback}
      </div> : null}
      {props.manualActions?.(manualTab, nativeBans ? refreshAfterModeration : undefined, nativeBans ? bans.invalidate : undefined)}
    </div>
    </ManualPlayerHostContext.Provider>
  );
}
