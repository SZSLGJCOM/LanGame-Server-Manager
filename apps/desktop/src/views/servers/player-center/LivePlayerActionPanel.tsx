import { ActivityNotice } from "../../../components/ActivityNotice";
import { useLayoutEffect, useRef, useState } from "react";
import { executeInstancePlayerAction } from "../../../api";
import { describeError } from "../../../app-state";
import { InlineConfirmAction } from "../../../components/InlineConfirmAction";
import { ConfigurationHelp } from "../../settings/ConfigurationFieldHelp";
import type { LocaleCode, TranslateFn } from "../../../i18n";
import type {
  ExecuteInstancePlayerActionInput,
  ExecuteInstancePlayerActionResult,
  ModulePlayerActionDetails,
  RuntimeLivePlayerEntry,
  RuntimeLivePlayerSnapshot
} from "../../../types";

interface LivePlayerActionPanelProps {
  actionIds: string[];
  actionsEnabled: boolean;
  declaredActions: ModulePlayerActionDetails[];
  disabledReason?: string | null;
  disabledActionReasons?: Readonly<Record<string, string>>;
  locale: LocaleCode;
  onActionCompleted: () => Promise<void>;
  onExecute?: (input: ExecuteInstancePlayerActionInput) => Promise<ExecuteInstancePlayerActionResult>;
  player: RuntimeLivePlayerEntry | null;
  snapshot: RuntimeLivePlayerSnapshot | null;
  t: TranslateFn;
}

interface PlayerActionRequest {
  instanceId: string;
  playerKey: string;
  actionId: string;
}

interface PlayerActionFeedback {
  request: PlayerActionRequest;
  tone: "success" | "error";
  text: string;
}

function actionLabel(actionId: string, action: ModulePlayerActionDetails | undefined, locale: LocaleCode): string {
  if (locale === "zh-CN" && action?.label_zh_cn?.trim()) {
    return action.label_zh_cn.trim();
  }
  if (action?.label.trim()) {
    return action.label.trim();
  }
  return actionId.replace(/[_-]+/g, " ").replace(/\b\w/g, (letter) => letter.toUpperCase());
}

export function LivePlayerActionPanel(props: LivePlayerActionPanelProps) {
  const instanceId = props.snapshot?.instance_id;
  const scopeRef = useRef<object | null>(null);
  const inFlightRef = useRef<PlayerActionRequest | null>(null);
  const [pendingRequest, setPendingRequest] = useState<PlayerActionRequest | null>(null);
  const [actionFeedback, setFeedback] = useState<PlayerActionFeedback | null>(null);

  useLayoutEffect(() => {
    const scope = {};
    scopeRef.current = scope;
    inFlightRef.current = null;
    setPendingRequest(null);
    setFeedback(null);
    return () => {
      scopeRef.current = null;
      inFlightRef.current = null;
    };
  }, [instanceId]);

  const hasPendingRequest = pendingRequest !== null && pendingRequest.instanceId === instanceId;
  const busyActionId = pendingRequest && hasPendingRequest && pendingRequest.playerKey === props.player?.player_key
    ? pendingRequest.actionId : null;
  const feedback = actionFeedback && actionFeedback.request.instanceId === instanceId
    && actionFeedback.request.playerKey === props.player?.player_key ? actionFeedback : null;

  async function execute(actionId: string) {
    const snapshot = props.snapshot;
    const player = props.player;
    const scope = scopeRef.current;
    if (!snapshot || !player || !scope || !props.actionsEnabled || inFlightRef.current
      || !props.actionIds.includes(actionId) || !player.available_action_ids.includes(actionId)) {
      return;
    }
    const request: PlayerActionRequest = { instanceId: snapshot.instance_id, playerKey: player.player_key, actionId };
    // Selection and snapshot changes do not cancel a command already sent to the server.
    inFlightRef.current = request;
    setPendingRequest(request);
    setFeedback(null);
    const ownsRequest = () => scopeRef.current === scope && inFlightRef.current === request;
    try {
      await (props.onExecute ?? executeInstancePlayerAction)({
        instance_id: snapshot.instance_id,
        snapshot_id: snapshot.snapshot_id,
        player_key: player.player_key,
        action_id: actionId
      });
      if (!ownsRequest()) return;
      setFeedback({
        request,
        tone: "success",
        text: props.t("servers.playerCenter.member.sent", undefined, "Player action sent. Refreshing the member list.")
      });
      try {
        await props.onActionCompleted();
      } catch (error) {
        if (ownsRequest()) setFeedback({
          request,
          tone: "error",
          text: props.t("servers.playerCenter.member.refreshFailed", { error: describeError(error) },
            "Action sent, but refreshing the player list failed: {error}")
        });
      }
    } catch (error) {
      if (ownsRequest()) setFeedback({ request, tone: "error", text: describeError(error) });
    } finally {
      if (ownsRequest()) {
        inFlightRef.current = null;
        setPendingRequest(null);
      }
    }
  }

  return (
    <aside className="player-center-member-pane" aria-label={props.t("servers.playerCenter.member.ariaLabel", undefined, "Member actions")} aria-busy={Boolean(busyActionId)}>
      {props.actionIds.length > 0 ? <section className="player-center-action-group" aria-label={props.t("servers.playerCenter.member.currentSession", undefined, "Current session")}>
        <div className="player-center-action-list">
          {props.actionIds.map((actionId) => {
            const action = props.declaredActions.find((candidate) => candidate.id === actionId);
            const label = actionLabel(actionId, action, props.locale);
            const danger = action?.destructive !== false;
            const disabled = !props.actionsEnabled || hasPendingRequest || !props.player?.available_action_ids.includes(actionId);
            const unavailableReason = !disabled ? null : hasPendingRequest
              ? props.t("servers.playerCenter.actions.waitForPending", undefined, "A player action is in progress. Wait for it to finish.")
              : props.disabledReason ?? (!props.player
                ? props.t("servers.playerCenter.actions.selectOnlinePlayer", undefined, "Select a current online player first.")
                : props.disabledActionReasons?.[actionId] ?? (!props.player.available_action_ids.includes(actionId)
                  ? props.t("servers.playerCenter.actions.playerIdentityUnavailable", undefined, "The current player data does not provide the identity required for this action.")
                  : props.t("servers.playerCenter.actions.snapshotNotCurrent", undefined, "The player snapshot is not complete and current. Refresh it before retrying.")));
            const content = busyActionId === actionId ? props.t("servers.playerCenter.actions.running", undefined, "Running...") : label;
            return <ConfigurationHelp key={actionId} description={unavailableReason}>{(help) =>
              <div className="player-access-total-action" ref={help.anchorRef} {...help.interactionProps}
                tabIndex={disabled ? 0 : undefined} role={disabled ? "group" : undefined} aria-label={disabled ? label : undefined}
                aria-disabled={disabled || undefined} aria-describedby={help.descriptionId}>
              {danger ? (
              <InlineConfirmAction
                key={actionId}
                className="secondary-button player-center-action-button player-center-member-action danger"
                disabled={disabled}
                aria-describedby={help.descriptionId}
                scopeKey={JSON.stringify([props.snapshot?.instance_id, props.snapshot?.snapshot_id, props.player?.player_key, actionId])}
                confirmation={props.t(
                  "servers.playerCenter.member.confirm",
                  { action: label, player: props.player?.display_name ?? "" },
                  "Run “{action}” for “{player}”?"
                )}
                onConfirm={() => execute(actionId)}
              >
                {content}
              </InlineConfirmAction>
            ) : (
              <button
                key={actionId}
                type="button"
                className="secondary-button player-center-action-button player-center-member-action"
                disabled={disabled}
                aria-describedby={help.descriptionId}
                onClick={() => void execute(actionId)}
              >
                {content}
              </button>
            )}</div>}</ConfigurationHelp>;
          })}
        </div>
      </section> : null}
      {feedback ? (
        <ActivityNotice tone={feedback.tone} onDismiss={() => setFeedback(null)}>
          {feedback.text}
        </ActivityNotice>
      ) : null}
    </aside>
  );
}
