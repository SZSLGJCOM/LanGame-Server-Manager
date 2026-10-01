import { useEffect, useId, useState, type ReactNode } from "react";
import { ShellIcon } from "../../../components/ShellIcon";
import type { LivePlayerPresentation } from "../../../domain/live-player-state";
import type { LocaleCode, TranslateFn } from "../../../i18n";
import type {
  ExecuteInstancePlayerActionInput,
  ExecuteInstancePlayerActionResult,
  InstanceRuntimeOverview,
  ModulePlayerActionDetails,
  RuntimeLivePlayerSnapshot
} from "../../../types";
import { LivePlayerActionPanel } from "./LivePlayerActionPanel";
import { LivePlayerState } from "./LivePlayerState";
import { LivePlayerTable } from "./LivePlayerTable";
import type { PlayerAccessRosterCapability } from "./player-access-roster-model";
import { PlayerAccessRosterList } from "./PlayerAccessRosterList";
import { SelectedPlayerRosterActions } from "./SelectedPlayerRosterActions";
import { resolveSelectedRosterIdentity } from "./player-access-selected-target";
import { matchRosterLivePlayer, rosterEntryPlayer } from "./player-center-selection";
import type { PlayerAccessState } from "./use-player-access";

interface OnlinePlayersViewProps {
  readOnly?: boolean;
  access?: PlayerAccessState;
  declaredActions: ModulePlayerActionDetails[];
  playerActionIds: string[];
  error: string | null;
  loading: boolean;
  locale: LocaleCode;
  moduleId?: string;
  manualActions?: (active: boolean) => ReactNode;
  now: number;
  onOpenSettings?: () => void;
  onRefresh: () => Promise<void>;
  onExecutePlayerAction?: (input: ExecuteInstancePlayerActionInput) => Promise<ExecuteInstancePlayerActionResult>;
  onSelectPlayer: (playerKey: string | null) => void;
  presentation: LivePlayerPresentation;
  rosterCapabilities?: PlayerAccessRosterCapability[];
  runtime: InstanceRuntimeOverview | null;
  runtimeAvailable: boolean;
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
  const fields = props.access?.fields ?? [];
  const activeField = fields.find((field) => field.key === activeListKey);
  const online = !activeField;
  const activeKey = activeField?.key ?? "online";
  const saving = Boolean(props.access?.busyFieldKey);
  const tabs = [{ key: "online", title: props.t("servers.playerCenter.online.title", undefined, "Online members") },
    ...fields.map((field) => ({ key: field.key, title: field.title }))];
  function selectList(key: string) {
    if (!saving) { clearSelection(); setActiveListKey(key); }
  }
  function clearSelection() {
    setRosterSelection(null);
    props.onSelectPlayer(null);
    setCreateFieldKey(undefined);
    setSelectionVersion((current) => current + 1);
  }
  useEffect(() => { clearSelection(); }, [props.snapshot?.instance_id]);
  const selectedField = fields.find((field) => field.key === rosterSelection?.fieldKey);
  const selectedEntry = selectedField?.entries.find((entry) => entry.key === rosterSelection?.entryKey);
  const selectedRoster = selectedField && selectedEntry ? { field: selectedField, entry: selectedEntry } : null;
  const selectedPlayer = rosterSelection ? rosterEntryPlayer(selectedRoster) : props.presentation.selectedPlayer;
  const matchedLivePlayer = rosterSelection ? matchRosterLivePlayer(selectedRoster, props.snapshot, props.now) : props.presentation.selectedPlayer;
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
  // 7DTD's native ban also records its exact result in this persistent roster.
  const nativeBanField = props.moduleId === "sevendaystodie" && declaredPlayerActionIds.includes("ban_player")
    ? fields.find((field) => field.key === "blacklist_entries") : undefined;
  const selectedBanIdentity = nativeBanField
    ? resolveSelectedRosterIdentity(selectedPlayer, nativeBanField)?.identity : null;
  const selectedPlayerBanned = selectedBanIdentity && nativeBanField?.entries.some((entry) => entry.key === selectedBanIdentity);
  const actionIds = declaredPlayerActionIds;
  const runtimePlayer = matchedLivePlayer && selectedPlayerBanned
    ? { ...matchedLivePlayer, available_action_ids: matchedLivePlayer.available_action_ids.filter((id) => id !== "ban_player") } : matchedLivePlayer;
  const showPlayerPanel = actionIds.length > 0;
  const hasControls = Boolean(showPlayerPanel || props.manualActions || fields.length > 0);
  const primaryIdentity = selectedPlayer?.identifiers.find((identifier) => identifier.stable)
    ?? selectedPlayer?.identifiers[0];

  return (
    <div className={`player-center-member-layout${hasControls ? "" : " is-read-only"}`}>
      <section className="player-center-list-pane" aria-label={props.t("servers.playerCenter.lists.label", undefined, "Player lists")} aria-busy={online && props.loading}>
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
              const count = tab.key === "online" ? countLabel : fields.find((field) => field.key === tab.key)?.entries.length;
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
          </div> : <button type="button" className="secondary-button player-access-roster-create"
            disabled={saving || props.access?.disabled || activeField.property.readOnly === true} onClick={() => { clearSelection(); setCreateFieldKey(activeField.key); }}>
            <ShellIcon name="plus" />{props.t("servers.playerCenter.lists.add", undefined, "Add entry")}
          </button>}
        </header>
        <div id={`${tabId}-panel`} role="tabpanel" aria-labelledby={`${tabId}-${activeKey}`} className="player-center-list-content">
        {activeField ? <PlayerAccessRosterList field={activeField} locale={props.locale}
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

      {hasControls ? <div className="player-center-controls">
        <header className="player-center-member-header">
          <span className="player-center-member-avatar" aria-hidden="true"><ShellIcon name="user" /></span>
          <strong>{props.t("servers.playerCenter.member.ariaLabel", undefined, "Player actions")}</strong>
        </header>
        <div className="player-center-total-controls">
        {selectedPlayer || selectedRoster ? <div className="player-center-member-heading">
          <strong title={selectedRoster?.entry.label ?? selectedPlayer?.display_name}>{selectedRoster?.entry.label ?? selectedPlayer?.display_name}</strong>
          {primaryIdentity ? <span title={primaryIdentity.value}>{primaryIdentity.value}</span> : null}
        </div> : null}
        {showPlayerPanel ? (
          <LivePlayerActionPanel
            actionIds={actionIds}
            actionsEnabled={props.runtimeAvailable && !props.error && (rosterSelection ? Boolean(runtimePlayer) : props.presentation.actionsEnabled)}
            declaredActions={props.declaredActions}
            locale={props.locale}
            onActionCompleted={props.onRefresh}
            onExecute={props.onExecutePlayerAction}
            player={runtimePlayer}
            snapshot={props.snapshot}
            t={props.t}
          />
        ) : null}
        {props.access ? <SelectedPlayerRosterActions key={JSON.stringify([activeKey, selectionVersion, selectedPlayer?.player_key, selectedPlayer?.identifiers])}
          selectedPlayer={selectedPlayer} selectedRoster={selectedRoster} createFieldKey={createFieldKey} onClearSelection={clearSelection}
          suppressedAddFieldKeys={nativeBanField ? [nativeBanField.key] : undefined}
          fields={fields} busyFieldKey={props.access.busyFieldKey}
          disabled={props.access.disabled} locale={props.locale} onMutate={props.access.onMutate}
          /> : null}
        {props.manualActions?.(true)}
        </div>
        {props.access?.feedback}
      </div> : null}
    </div>
  );
}
