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
import { isEnshroudedNativeBanRoster, resolveSelectedRosterIdentity } from "./player-access-selected-target";
import { dragonwildsAccountId, dragonwildsBanPlayer, matchDragonwildsBanLivePlayer, matchRosterLivePlayer, rosterEntryPlayer } from "./player-center-selection";
import type { PlayerAccessState } from "./use-player-access";
import { DragonwildsBanRosterActions, DragonwildsBanRosterList } from "./DragonwildsBanRoster";
import { useDragonwildsBanRoster } from "./use-dragonwilds-ban-roster";

interface OnlinePlayersViewProps {
  readOnly?: boolean;
  access?: PlayerAccessState;
  declaredActions: ModulePlayerActionDetails[];
  playerActionIds: string[];
  error: string | null;
  loading: boolean;
  locale: LocaleCode;
  moduleId?: string;
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
  const [rosterSelection, setRosterSelection] = useState<{ fieldKey: string; entryKey: string } | null>(null);
  const [createFieldKey, setCreateFieldKey] = useState<string | undefined>();
  const [selectionVersion, setSelectionVersion] = useState(0);
  const [selectedBannedAccountId, setSelectedBannedAccountId] = useState<string | null>(null);
  const [nativeUnbanBusy, setNativeUnbanBusy] = useState(false);
  const [manualLookupHost, setManualLookupHost] = useState<HTMLElement | null>(null);
  const [manualActionsHost, setManualActionsHost] = useState<HTMLElement | null>(null);
  const [manualTarget, setManualTarget] = useState("");
  const dragonwilds = props.moduleId === "runescapedragonwilds";
  const dragonwildsUnban = dragonwilds && props.declaredActions.some((action) => action.id === "unban_player");
  // Dragonwilds has one native unban action shared by list and manual selections.
  // Its existing lookup still reports a target; it must not portal a duplicate action.
  const manualHosts = useMemo(() => ({ lookup: manualLookupHost, actions: dragonwildsUnban ? null : manualActionsHost, onTarget: setManualTarget }),
    [manualLookupHost, manualActionsHost, dragonwildsUnban]);
  const bans = useDragonwildsBanRoster(dragonwilds ? props.instanceDetails?.summary.id ?? null : null);
  const bannedTab = dragonwilds && activeListKey === "dragonwilds-banned";
  const selectedBan = bans.roster?.entries.find((entry) => entry.account_id === selectedBannedAccountId) ?? null;
  const fields = props.access?.fields ?? [];
  const activeField = fields.find((field) => field.key === activeListKey);
  const manualTab = Boolean(props.manualActions) && activeListKey === "manual";
  const online = !activeField && !bannedTab && !manualTab;
  const activeKey = manualTab ? "manual" : bannedTab ? "dragonwilds-banned" : activeField?.key ?? "online";
  const saving = Boolean(props.access?.busyFieldKey) || nativeUnbanBusy;
  const tabs = [{ key: "online", title: props.t("servers.playerCenter.online.title", undefined, "Online members") },
    ...fields.map((field) => ({ key: field.key, title: field.title })),
    ...(dragonwilds ? [{ key: "dragonwilds-banned", title: selectBanText("被封禁", "Banned") }] : []),
    // 手动查询: the ID lookup is its own category; its actions use the shared right-side controls.
    ...(props.manualActions ? [{ key: "manual", title: props.t("servers.playerCenter.manual.tab", undefined, "Manual lookup") }] : [])];
  function selectBanText(zh: string, en: string) { return props.locale === "zh-CN" ? zh : en; }
  function selectList(key: string) {
    if (!saving) { clearSelection(); setActiveListKey(key); }
  }
  function clearSelection() {
    setRosterSelection(null);
    setSelectedBannedAccountId(null);
    props.onSelectPlayer(null);
    setCreateFieldKey(undefined);
    setSelectionVersion((current) => current + 1);
  }
  useEffect(() => { clearSelection(); }, [props.snapshot?.instance_id]);
  useEffect(() => { setNativeUnbanBusy(false); }, [props.instanceDetails?.summary.id]);
  useEffect(() => {
    if (!bans.stale && bans.roster && selectedBannedAccountId && !selectedBan) setSelectedBannedAccountId(null);
  }, [bans.roster, bans.stale, selectedBannedAccountId, selectedBan]);
  async function refreshAfterModeration() {
    const results = await Promise.allSettled([props.onRefresh(), ...(dragonwilds ? [bans.refreshAfterMutation()] : [])]);
    const failed = results.find((result) => result.status === "rejected");
    if (failed?.status === "rejected") throw failed.reason;
    if (results.some((result) => result.status === "fulfilled" && result.value === false)) {
      throw new Error(selectBanText("封禁名单刷新失败，请重试。", "Could not refresh the ban roster. Retry the refresh."));
    }
  }
  async function executeLiveAction(input: ExecuteInstancePlayerActionInput) {
    try { return await (props.onExecutePlayerAction ?? executeInstancePlayerAction)(input); }
    catch (cause) { if (dragonwilds && input.action_id === "ban_player") bans.invalidate(); throw cause; }
  }
  const selectedField = fields.find((field) => field.key === rosterSelection?.fieldKey);
  const selectedEntry = selectedField?.entries.find((entry) => entry.key === rosterSelection?.entryKey);
  const selectedRoster = selectedField && selectedEntry ? { field: selectedField, entry: selectedEntry } : null;
  const selectedPlayer = bannedTab ? dragonwildsBanPlayer(selectedBan)
    : manualTab ? null : rosterSelection ? rosterEntryPlayer(selectedRoster) : props.presentation.selectedPlayer;
  const matchedLivePlayer = bannedTab ? !bans.stale && !bans.loading && !bans.error
    ? matchDragonwildsBanLivePlayer(selectedBan, props.snapshot, props.now) : null
    : manualTab ? null : rosterSelection ? matchRosterLivePlayer(selectedRoster, props.snapshot, props.now) : props.presentation.selectedPlayer;
  const unbanAccountId = bannedTab ? selectedBan?.account_id : manualTab ? manualTarget
    : props.presentation.authoritative && !props.error ? dragonwildsAccountId(selectedPlayer) : null;
  const unbanEntry = bans.roster?.entries.find((entry) => entry.account_id === unbanAccountId) ?? null;
  const selectedDragonwildsBanned = dragonwilds && Boolean(unbanEntry);
  const currentPlayers = props.snapshot?.current_players ?? props.runtime?.players.current_players ?? null;
  const maxPlayers = props.snapshot?.max_players ?? props.runtime?.players.max_players ?? null;
  const countLabel = currentPlayers === null
    ? null
    : maxPlayers === null
      ? props.t("servers.playerCenter.online.count", { count: currentPlayers }, "{count} online")
      : `${currentPlayers} / ${maxPlayers}`;
  const observedAt = formatObservedAt(props.locale, props.snapshot?.observed_at_unix_ms ?? null);
  const declaredPlayerActionIds = [...new Set(props.playerActionIds)]
    .filter((actionId) => props.declaredActions.some((action) => action.id === actionId));
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
  const missingEnshroudedExtensionIdentity = props.moduleId === "enshrouded"
    && !hasEnshroudedExtensionIdentity(matchedLivePlayer);
  const enshroudedModerationProtected = props.moduleId === "enshrouded"
    && matchedLivePlayer?.attributes.some((attribute) => attribute.key === "moderation_immune" && attribute.value === "true");
  const protectedPlayerReason = props.t("servers.playerCenter.enshrouded.moderationProtected", undefined,
    "The server protects this player; native permissions do not allow kicking or banning them.");
  const disabledActionReasons = {
    ...(selectedPlayerBanned || selectedDragonwildsBanned ? { ban_player: props.t("servers.playerCenter.actions.alreadyBanned", undefined, "This player is already in the ban roster.") } : {}),
    ...(enshroudedModerationProtected ? { kick_player: protectedPlayerReason, ban_player: protectedPlayerReason } : {})
  };
  const runtimePlayer = matchedLivePlayer && (selectedPlayerBanned || selectedDragonwildsBanned || missingEnshroudedExtensionIdentity || enshroudedModerationProtected)
    ? { ...matchedLivePlayer, available_action_ids: matchedLivePlayer.available_action_ids.filter((id) =>
      !missingEnshroudedExtensionIdentity && (!(selectedPlayerBanned || selectedDragonwildsBanned) || id !== "ban_player")
      && (!enshroudedModerationProtected || (id !== "kick_player" && id !== "ban_player"))) } : matchedLivePlayer;
  const showPlayerPanel = actionIds.length > 0;
  const hasControls = Boolean(showPlayerPanel || props.manualActions || fields.length > 0 || dragonwilds);
  const showMemberPane = hasControls || props.presentation.tableVisible;
  const runtimeUnavailableReason = props.readOnly
    ? props.t("servers.playerCenter.actions.readOnly", undefined, "This workspace is read-only.")
    : nativeUnbanBusy
      ? props.t("servers.playerCenter.actions.waitForPending", undefined, "A player action is in progress. Wait for it to finish.")
    : !props.runtimeAvailable
      ? props.t("servers.playerCenter.actions.serverStopped", undefined, "The server is not running; live actions are unavailable.")
      : props.error
        ? props.t("servers.playerCenter.actions.listReadFailed", undefined, "The player list could not be read. Refresh it before retrying.")
        : !props.presentation.authoritative
          ? props.t("servers.playerCenter.actions.snapshotNotCurrent", undefined, "The player snapshot is not complete and current. Refresh it before retrying.")
          : (rosterSelection || bannedTab) && !runtimePlayer
            ? props.t("servers.playerCenter.actions.noUniqueOnlineMatch", undefined, "This entry does not uniquely match a current online player.")
            : missingEnshroudedExtensionIdentity && inspectedPlayer
              ? props.t("servers.playerCenter.enshrouded.extensionIdentityUnavailable", undefined,
                "Verified native player identity is unavailable from the Enshrouded extension. Live kick and ban are disabled; the player list remains read-only.")
              : null;

  return (
    <ManualPlayerHostContext.Provider value={manualHosts}>
    <div className={`player-center-member-layout${showMemberPane ? "" : " is-read-only"}`}>
      <section className="player-center-list-pane" aria-label={props.t("servers.playerCenter.lists.label", undefined, "Player lists")} aria-busy={online ? props.loading : bannedTab && bans.loading}>
        <header className="player-center-pane-header">
          <div className="player-center-list-tabs" role="tablist" aria-label={props.t("servers.playerCenter.lists.label", undefined, "Player lists")}
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
              const count = tab.key === "online" ? countLabel : tab.key === "manual" ? null : tab.key === "dragonwilds-banned"
                ? !bans.stale && !bans.loading && !bans.error ? bans.roster?.entries.length : null
                : fields.find((field) => field.key === tab.key)?.entries.length;
              return <button key={tab.key} id={`${tabId}-${tab.key}`} type="button" role="tab"
                data-list-key={tab.key} aria-selected={activeKey === tab.key} aria-controls={`${tabId}-panel`}
                tabIndex={activeKey === tab.key ? 0 : -1} disabled={saving}
                className="player-center-list-tab" onClick={() => selectList(tab.key)}>
                <span>{tab.title}</span>{count !== null && count !== undefined ? <span className="player-center-list-count">{count}</span> : null}
              </button>;
            })}
          </div>
          {online ? <div className="player-center-pane-tools">
            {props.snapshot?.stale ? <span className="page-chip">{props.t("servers.playerCenter.online.stale", undefined, "Stale")}</span> : null}
            {props.snapshot?.truncated ? <span className="page-chip">{props.t("servers.playerCenter.online.truncated", undefined, "Truncated")}</span> : null}
            {observedAt ? <span className="page-chip" title={props.t("servers.playerCenter.online.observedAt", undefined, "Observed at")}>{observedAt}</span> : null}
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
          </button> : manualTab ? null : activeField ? <button type="button" className="secondary-button player-access-roster-create"
            disabled={saving || props.access?.disabled || activeField.property.readOnly === true} onClick={() => { clearSelection(); setCreateFieldKey(activeField.key); }}>
            <ShellIcon name="plus" />{props.t("servers.playerCenter.lists.add", undefined, "Add entry")}
          </button> : null}
        </header>
        <div id={`${tabId}-panel`} role="tabpanel" aria-labelledby={`${tabId}-${activeKey}`} className="player-center-list-content">
        {/* 手动查询 page host. ManualPlayerActions stays mounted below, so a typed ID survives category switches. */}
        {manualTab ? <div className="player-center-manual-slot" ref={setManualLookupHost} /> : null}
        {manualTab ? null : bannedTab ? <DragonwildsBanRosterList state={bans} selectedAccountId={selectedBannedAccountId}
          disabled={saving} locale={props.locale} onSelect={(accountId) => {
            const next = selectedBannedAccountId === accountId ? null : accountId;
            clearSelection(); setSelectedBannedAccountId(next);
          }} /> : activeField ? <PlayerAccessRosterList field={activeField} locale={props.locale}
          disabled={saving} selectedEntryKey={selectedRoster?.field.key === activeField.key ? selectedRoster.entry.key : null}
          onSelect={(entry) => { clearSelection(); setRosterSelection(rosterSelection?.fieldKey === activeField.key && rosterSelection.entryKey === entry.key
            ? null : { fieldKey: activeField.key, entryKey: entry.key }); }} /> : <>
        {!hasControls ? <p className="player-center-capability-note" role="status">
          {props.t("servers.playerCenter.actions.empty", undefined, "Player information is read-only for this server.")}
        </p> : null}

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
          <strong>{props.t("servers.playerCenter.member.detailsAndActions", undefined, "Player details and actions")}</strong>
        </header>
        <div className="player-center-total-controls">
        {/* The selected left row already shows name, primary ID, session, ping, role and status; only extras appear here. */}
        {selectedPlayer && !selectedRoster ? <SelectedPlayerExtraDetails locale={props.locale} player={selectedPlayer}
            sessionUnknownReason={props.moduleId === "enshrouded" && props.snapshot?.source === "server_query" ? props.t("servers.playerCenter.enshrouded.sessionUnknown", undefined,
              "Anonymous query durations cannot be reliably matched to player names, and this roster has no usable join time for this player.") : undefined} />
          : manualTab && manualTarget ? <p className="player-center-target-line">
            {props.t("servers.playerCenter.manual.targetLine", { target: manualTarget }, "Target: {target}")}</p> : null}
        {props.moduleId === "enshrouded" ? <p className="player-center-capability-note">
          {!enshroudedNativeActionsDeclared
            ? props.t("servers.playerCenter.enshrouded.managementUnavailable", undefined,
              "Enshrouded has no verified live administration interface connected. Kick and immediate ban are unavailable. Its ban roster uses native accountId values; stop the server before editing, then start it to apply changes.")
            : !enshroudedExtensionIdentityAvailable
              ? props.t("servers.playerCenter.enshrouded.extensionIdentityUnavailable", undefined,
                "Verified native player identity is unavailable from the Enshrouded extension. Live kick and ban are disabled; the player list remains read-only.")
              : props.t("servers.playerCenter.enshrouded.managementConnected", undefined,
                "The native extension is connected. Bans and unbans can apply immediately. Stop the server before manually adding native accountId values to the persistent roster.")}
        </p> : null}
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
            disabledActionReasons={disabledActionReasons}
            locale={props.locale}
            onActionCompleted={dragonwilds ? refreshAfterModeration : props.onRefresh}
            onExecute={executeLiveAction}
            player={runtimePlayer}
            snapshot={props.snapshot}
            t={props.t}
          />
        ) : null}
        {props.access && fields.length > 0 ? <SelectedPlayerRosterActions key={JSON.stringify([activeKey, selectionVersion, selectedPlayer?.player_key, selectedPlayer?.identifiers])}
          selectedPlayer={selectedPlayer} selectedRoster={selectedRoster} createFieldKey={createFieldKey} onClearSelection={clearSelection}
          suppressedAddFieldKeys={nativeBanField ? [nativeBanField.key] : undefined}
          fields={fields} busyFieldKey={props.access.busyFieldKey}
          disabled={props.access.disabled || nativeUnbanBusy} locale={props.locale} onMutate={props.access.onMutate}
          disabledReason={props.access.disabledReason}
          /> : null}
        {dragonwildsUnban && props.instanceDetails ? <DragonwildsBanRosterActions key={props.instanceDetails.summary.id} instanceId={props.instanceDetails.summary.id}
          entry={unbanEntry} hasSelection={Boolean(selectedPlayer || manualTab && manualTarget)} state={bans}
          readOnly={Boolean(props.readOnly)} running={props.runtimeAvailable} locale={props.locale}
          onBusyChange={setNativeUnbanBusy} onCompleted={async () => {
            setSelectedBannedAccountId(null); await refreshAfterModeration();
          }} /> : null}
        {/* Identity actions sit with every other action; they need a 手动查询 target and stay disabled otherwise. */}
        {props.manualActions && !dragonwildsUnban ? <div className="player-center-manual-host" ref={setManualActionsHost} /> : null}
        </div>
        {props.access?.feedback}
      </div> : null}
      {props.manualActions?.(manualTab, dragonwilds ? refreshAfterModeration : undefined, dragonwilds ? bans.invalidate : undefined)}
    </div>
    </ManualPlayerHostContext.Provider>
  );
}
