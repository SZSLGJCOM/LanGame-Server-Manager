import { ActivityNotice } from "../../../components/ActivityNotice";
import { useEffect, useMemo, useState } from "react";
import { executeDeclaredRuntimePlayerAction } from "../../../api";
import { describeError } from "../../../app-state";
import { InlineConfirmAction } from "../../../components/InlineConfirmAction";
import type { LocaleCode, TranslateFn } from "../../../i18n";
import type { ModulePlayerActionDetails } from "../../../types";
import { actionNeedsManualTarget, manualPlayerActionTargetError } from "./manual-player-action-model";

interface ManualPlayerActionsProps {
  actions: ModulePlayerActionDetails[];
  disabled: boolean;
  instanceId: string;
  moduleId: string;
  locale: LocaleCode;
  t: TranslateFn;
}

function localizedActionLabel(action: ModulePlayerActionDetails, locale: LocaleCode): string {
  if (locale === "zh-CN" && action.label_zh_cn?.trim()) {
    return action.label_zh_cn.trim();
  }
  return action.label.trim() || action.id;
}

export function ManualPlayerActions(props: ManualPlayerActionsProps) {
  const eligibleActions = useMemo(
    () => props.actions.filter(actionNeedsManualTarget),
    [props.actions]
  );
  const [actionId, setActionId] = useState(eligibleActions[0]?.id ?? "");
  const [target, setTarget] = useState("");
  const [role, setRole] = useState("");
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<{ tone: "success" | "error"; text: string } | null>(null);
  const action = useMemo(
    () => eligibleActions.find((candidate) => candidate.id === actionId) ?? eligibleActions[0] ?? null,
    [actionId, eligibleActions]
  );
  const needsTarget = action ? actionNeedsManualTarget(action) : false;
  const targetError = action && target ? manualPlayerActionTargetError(props.moduleId, action, target, props.locale) : null;
  const roleValues = action?.role_values ?? [];
  const selectedRole = roleValues.includes(role) ? role : roleValues[0] ?? "";

  useEffect(() => {
    setActionId(action?.id ?? "");
    setTarget("");
    setRole("");
    setFeedback(null);
  }, [props.instanceId, action?.id]);

  async function execute() {
    if (!action || props.disabled || busy || Boolean(targetError) || (needsTarget && !target.trim())) {
      return;
    }
    setBusy(true);
    setFeedback(null);
    try {
      await executeDeclaredRuntimePlayerAction(
        props.instanceId,
        action.id,
        needsTarget ? target.trim() : "",
        selectedRole
      );
      setFeedback({
        tone: "success",
        text: props.t("servers.playerCenter.manual.accepted", undefined, "The server accepted the action.")
      });
    } catch (error) {
      setFeedback({ tone: "error", text: describeError(error) });
    } finally {
      setBusy(false);
    }
  }

  if (!action) {
    return null;
  }

  return (
    <details className="player-center-manual-actions" aria-busy={busy}>
      <summary className="player-center-manual-entry">{props.t("servers.playerCenter.manual.entry", undefined, "Manual player action")}</summary>
      <div className="player-center-manual-body">
        {eligibleActions.length > 1 ? (
          <>
            <label className="detail-label" htmlFor={`manual-player-action-${props.instanceId}`}>{props.t("servers.playerCenter.manual.action", undefined, "Action")}</label>
            <select
              id={`manual-player-action-${props.instanceId}`}
              className="text-input"
              value={action.id}
              disabled={props.disabled || busy}
              onChange={(event) => {
                const next = eligibleActions.find((candidate) => candidate.id === event.target.value) ?? null;
                setActionId(event.target.value);
                setTarget("");
                setRole(next?.role_values?.[0] ?? "");
                setFeedback(null);
              }}
            >
              {eligibleActions.map((candidate) => <option key={candidate.id} value={candidate.id}>{localizedActionLabel(candidate, props.locale)}</option>)}
            </select>
          </>
        ) : null}
        {needsTarget ? (
          <>
            <label className="detail-label" htmlFor={`manual-player-target-${props.instanceId}`}>
              {props.locale === "zh-CN" && action.target_label_zh_cn?.trim()
                ? action.target_label_zh_cn
                : action.target_label || props.t("servers.playerCenter.manual.playerId", undefined, "Player ID")}
            </label>
            <input
              id={`manual-player-target-${props.instanceId}`}
              type="text"
              className="text-input"
              value={target}
              aria-invalid={Boolean(targetError)}
              aria-describedby={targetError ? `manual-player-target-error-${props.instanceId}` : undefined}
              disabled={props.disabled || busy}
              placeholder={props.locale === "zh-CN" && action.target_placeholder_zh_cn?.trim()
                ? action.target_placeholder_zh_cn
                : action.target_placeholder || props.t("servers.playerCenter.manual.playerIdPlaceholder", undefined, "Exact player ID")}
              onChange={(event) => setTarget(event.target.value)}
            />
            {targetError ? <p className="form-note" role="alert" id={`manual-player-target-error-${props.instanceId}`}>{targetError}</p> : null}
          </>
        ) : null}
        {roleValues.length > 1 ? (
          <>
            <label className="detail-label" htmlFor={`manual-player-role-${props.instanceId}`}>{props.t("servers.playerCenter.manual.role", undefined, "Role")}</label>
            <select id={`manual-player-role-${props.instanceId}`} className="text-input" value={selectedRole} disabled={props.disabled || busy} onChange={(event) => setRole(event.target.value)}>
              {roleValues.map((roleValue) => <option key={roleValue} value={roleValue}>{roleValue}</option>)}
            </select>
          </>
        ) : roleValues.length === 1 ? <span className="form-note">
          {props.t("servers.playerCenter.manual.role", undefined, "Role")}: {selectedRole}
        </span> : null}
        {action.destructive ? (
          <InlineConfirmAction
            className="ghost-button danger"
            disabled={props.disabled || busy || Boolean(targetError) || (needsTarget && !target.trim())}
            scopeKey={JSON.stringify([props.instanceId, action.id, target.trim(), selectedRole])}
            confirmation={props.t(
              "servers.playerCenter.manual.confirm",
              { action: localizedActionLabel(action, props.locale), target: target.trim() },
              "Run “{action}” for known identity “{target}”?"
            )}
            onConfirm={execute}
          >
            {busy ? props.t("servers.playerCenter.actions.running", undefined, "Running...") : localizedActionLabel(action, props.locale)}
          </InlineConfirmAction>
        ) : (
          <button
            type="button"
            className="secondary-button"
            disabled={props.disabled || busy || Boolean(targetError) || (needsTarget && !target.trim())}
            onClick={() => void execute()}
          >
            {busy ? props.t("servers.playerCenter.actions.running", undefined, "Running...") : localizedActionLabel(action, props.locale)}
          </button>
        )}
        {props.disabled ? <div className="form-note">{props.t("servers.playerCenter.manual.requiresRunning", undefined, "Available while the server is running.")}</div> : null}
        {feedback ? <ActivityNotice tone={feedback.tone} onDismiss={() => setFeedback(null)}>{feedback.text}</ActivityNotice> : null}
      </div>
    </details>
  );
}
