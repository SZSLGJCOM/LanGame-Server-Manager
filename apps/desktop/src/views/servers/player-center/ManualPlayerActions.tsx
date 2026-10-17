import { ActivityNotice } from "../../../components/ActivityNotice";
import { createContext, useContext, useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { executeDeclaredRuntimePlayerAction } from "../../../api";
import { describeError } from "../../../app-state";
import { InlineConfirmAction } from "../../../components/InlineConfirmAction";
import { ShellIcon } from "../../../components/ShellIcon";
import { ConfigurationHelp } from "../../settings/ConfigurationFieldHelp";
import type { LocaleCode, TranslateFn } from "../../../i18n";
import type { ModulePlayerActionDetails } from "../../../types";
import { actionNeedsManualTarget, manualPlayerActionTargetError } from "./manual-player-action-model";

interface ManualPlayerActionsProps {
  actions: ModulePlayerActionDetails[];
  disabled: boolean;
  /** False while another player category is shown: the looked-up ID is then not the current target. */
  active?: boolean;
  instanceId: string;
  moduleId: string;
  locale: LocaleCode;
  t: TranslateFn;
  onActionCompleted?: () => Promise<void>;
  onActionFailed?: () => void;
}

/**
 * Hosts provided by the players workspace: the ID lookup renders in the 手动查询 category page and the
 * actions render in the shared right-side control area, exactly like a player picked from a list.
 */
export interface ManualPlayerHosts { lookup: HTMLElement | null; actions: HTMLElement | null; onTarget?: (target: string) => void }
export const ManualPlayerHostContext = createContext<ManualPlayerHosts | null>(null);

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
  const [lookedUp, setLookedUp] = useState("");
  const hosts = useContext(ManualPlayerHostContext);
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
  // Module-specific identity formats live in the tooltip instead of a permanent paragraph.
  const formatHint = action ? manualPlayerActionTargetError(props.moduleId, action, "", props.locale) : null;

  useEffect(() => {
    setActionId(action?.id ?? "");
    setTarget("");
    setLookedUp("");
    setRole("");
    setFeedback(null);
  }, [props.instanceId, action?.id]);
  // Actions only act on the ID that was explicitly looked up; editing the field deselects it.
  const active = props.active !== false;
  const selectedTarget = active && lookedUp && lookedUp === target.trim() ? lookedUp : "";
  const onTarget = hosts?.onTarget;
  useEffect(() => { if (!active) setLookedUp(""); }, [active]);
  useEffect(() => { onTarget?.(selectedTarget); }, [onTarget, selectedTarget]);
  async function execute() {
    if (!action || props.disabled || busy || Boolean(targetError) || (needsTarget && !selectedTarget)) {
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
      try {
        await props.onActionCompleted?.();
      } catch (refreshError) {
        setFeedback({ tone: "error", text: props.t("servers.playerCenter.member.refreshFailed",
          { error: describeError(refreshError) }, "Action sent, but refreshing the player list failed: {error}") });
      }
    } catch (error) {
      props.onActionFailed?.();
      setFeedback({ tone: "error", text: describeError(error) });
    } finally {
      setBusy(false);
    }
  }

  if (!action) {
    return null;
  }

  const blockedReason = props.disabled
    ? props.t("servers.playerCenter.manual.requiresRunning", undefined, "Available while the server is running.")
    : targetError
      ? targetError
      : needsTarget && !selectedTarget
        ? props.t("servers.playerCenter.manual.needsTarget", undefined, "Select or enter a player first.")
        : null;
  const actionDisabled = props.disabled || busy || Boolean(targetError) || (needsTarget && !selectedTarget);
  function lookUp() {
    const next = target.trim();
    if (!next || targetError || busy) return;
    setLookedUp(next);
    setFeedback(null);
  }

  const targetLabel = props.locale === "zh-CN" && action.target_label_zh_cn?.trim()
    ? action.target_label_zh_cn
    : action.target_label || props.t("servers.playerCenter.manual.playerId", undefined, "Player ID");
  const helpText = [props.t("servers.playerCenter.manual.description", undefined,
    "Enter an exact player identifier obtained from the server to run an action."), formatHint].filter(Boolean).join(" ");

  // 手动查询 category content: only the ID field and lookup; the actions stay in the shared control area.
  const lookup = needsTarget ? (
    <form className="settings-schema-grid player-center-manual-lookup" aria-label={props.t("servers.playerCenter.manual.tab", undefined, "Manual lookup")}
      onSubmit={(event) => { event.preventDefault(); lookUp(); }}>
      <div className="settings-schema-field">
        <ConfigurationHelp description={helpText}>{(help) =>
          <label className="detail-label" htmlFor={`manual-player-target-${props.instanceId}`} ref={help.anchorRef} {...help.interactionProps}
            aria-describedby={help.descriptionId}>
            {targetLabel}<ShellIcon name="alert-circle" className="maintenance-help-icon" />
          </label>}</ConfigurationHelp>
        <input
          id={`manual-player-target-${props.instanceId}`}
          type="text"
          className="settings-schema-input"
          value={target}
          aria-invalid={Boolean(targetError)}
          aria-describedby={targetError ? `manual-player-target-error-${props.instanceId}` : undefined}
          disabled={props.disabled || busy}
          placeholder={props.locale === "zh-CN" && action.target_placeholder_zh_cn?.trim()
            ? action.target_placeholder_zh_cn
            : action.target_placeholder || props.t("servers.playerCenter.manual.playerIdPlaceholder", undefined, "Exact player ID")}
          onChange={(event) => setTarget(event.target.value)}
        />
        {targetError ? <span className="form-note form-note--error" role="alert" id={`manual-player-target-error-${props.instanceId}`}>{targetError}</span> : null}
      </div>
      <div className="player-center-manual-lookup-action">
        <button type="submit" className="secondary-button player-center-manual-use"
          disabled={props.disabled || busy || !target.trim() || Boolean(targetError)}>
          <ShellIcon name="search" />{props.t("servers.playerCenter.manual.lookup", undefined, "Look up")}
        </button>
      </div>
    </form>
  ) : null;
  const label = localizedActionLabel(action, props.locale);
  const content = busy ? props.t("servers.playerCenter.actions.running", undefined, "Running...") : label;
  // Same action-group markup and button classes as the selected-player actions (LivePlayerActionPanel).
  const actions = (
    <section className="player-center-action-group player-center-manual-actions"
      aria-label={props.t("servers.playerCenter.manual.title", undefined, "Server player actions")} aria-busy={busy}>
      {eligibleActions.length > 1 ? (
        <label className="settings-schema-field" htmlFor={`manual-player-action-${props.instanceId}`}>
          <span className="detail-label">{props.t("servers.playerCenter.manual.action", undefined, "Action")}</span>
          <select
            id={`manual-player-action-${props.instanceId}`}
            className="settings-schema-input"
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
        </label>
      ) : null}
      {roleValues.length > 1 ? (
        <label className="settings-schema-field" htmlFor={`manual-player-role-${props.instanceId}`}>
          <span className="detail-label">{props.t("servers.playerCenter.manual.role", undefined, "Role")}</span>
          <select id={`manual-player-role-${props.instanceId}`} className="settings-schema-input" value={selectedRole} disabled={props.disabled || busy} onChange={(event) => setRole(event.target.value)}>
            {roleValues.map((roleValue) => <option key={roleValue} value={roleValue}>{roleValue}</option>)}
          </select>
        </label>
      ) : roleValues.length === 1 ? <span className="form-note player-center-manual-role">
        {props.t("servers.playerCenter.manual.role", undefined, "Role")}: {selectedRole}
      </span> : null}
      <div className="player-center-action-list">
        <ConfigurationHelp description={busy ? null : blockedReason}>{(help) =>
        <div className="player-access-total-action" ref={help.anchorRef} {...help.interactionProps}
          tabIndex={actionDisabled ? 0 : undefined} role={actionDisabled ? "group" : undefined} aria-label={actionDisabled ? label : undefined}
          aria-disabled={actionDisabled || undefined} aria-describedby={help.descriptionId}>
        {action.destructive ? (
          <InlineConfirmAction
            className="secondary-button player-center-action-button danger"
            disabled={actionDisabled}
            aria-describedby={help.descriptionId}
            scopeKey={JSON.stringify([props.instanceId, action.id, target.trim(), selectedRole])}
            confirmation={props.t(
              "servers.playerCenter.manual.confirm",
              { action: label, target: target.trim() },
              "Run \u201c{action}\u201d for known identity \u201c{target}\u201d?"
            )}
            onConfirm={execute}
          >
            {content}
          </InlineConfirmAction>
        ) : (
          <button
            type="button"
            className="secondary-button player-center-action-button"
            disabled={actionDisabled}
            aria-describedby={help.descriptionId}
            onClick={() => void execute()}
          >
            {content}
          </button>
        )}
        </div>}</ConfigurationHelp>
      </div>
      {feedback ? <ActivityNotice tone={feedback.tone} onDismiss={() => setFeedback(null)}>{feedback.text}</ActivityNotice> : null}
    </section>
  );
  if (hosts) {
    // Inside the players workspace: split into the category page and the shared control area.
    return <>
      {hosts.lookup ? createPortal(lookup, hosts.lookup) : null}
      {hosts.actions ? createPortal(actions, hosts.actions) : null}
    </>;
  }
  return <div className="player-center-manual-standalone">{lookup}{actions}</div>;
}