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
 * Provided by the players workspace: 手动查询 owns the typed/looked-up ID (`target`, empty unless that
 * category is open) and the actions render into the shared right-side stack (`actions`).
 */
/** `suppressedActionIds`: actions merged into a roster button of the same meaning (e.g. Enshrouded unban). */
export interface ManualPlayerHosts { actions: HTMLElement | null; target: string; suppressedActionIds?: readonly string[] }
export const ManualPlayerHostContext = createContext<ManualPlayerHosts | null>(null);

function localizedActionLabel(action: ModulePlayerActionDetails, locale: LocaleCode): string {
  if (locale === "zh-CN" && action.label_zh_cn?.trim()) {
    return action.label_zh_cn.trim();
  }
  return action.label.trim() || action.id;
}

export function ManualPlayerActions(props: ManualPlayerActionsProps) {
  const hosts = useContext(ManualPlayerHostContext);
  const suppressed = hosts?.suppressedActionIds;
  const eligibleActions = useMemo(
    () => props.actions.filter(actionNeedsManualTarget)
      .filter((candidate) => !suppressed?.includes(candidate.id)),
    [props.actions, suppressed]
  );
  const [ownTarget, setTarget] = useState("");
  const [lookedUp, setLookedUp] = useState("");
  // Inside the workspace the ID comes from 手动查询; standalone use keeps its own field and lookup.
  const target = hosts ? hosts.target : ownTarget;
  // Each action is its own stack button; a role choice expands in place of the button that needs it.
  const [roles, setRoles] = useState<Record<string, string>>({});
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const busy = busyId !== null;
  const [feedback, setFeedback] = useState<{ tone: "success" | "error"; text: string } | null>(null);

  useEffect(() => {
    setTarget("");
    setLookedUp("");
    setRoles({});
    setExpandedId(null);
    setFeedback(null);
  }, [props.instanceId]);
  // Actions only act on the ID that was explicitly looked up; editing the field deselects it.
  const active = props.active !== false;
  const selectedTarget = hosts ? active ? target.trim() : "" : active && lookedUp && lookedUp === target.trim() ? lookedUp : "";
  useEffect(() => { if (!active) { setLookedUp(""); setExpandedId(null); } }, [active]);

  // The first action supplies the lookup field's label, placeholder and format hint.
  const action = eligibleActions[0] ?? null;
  if (eligibleActions.length === 0) {
    return null;
  }
  // The typed ID is acceptable when any action accepts it; each button still checks its own format.
  const lookupError = action && target.trim() && eligibleActions.every((candidate) => manualPlayerActionTargetError(props.moduleId, candidate, target, props.locale))
    ? manualPlayerActionTargetError(props.moduleId, action, target, props.locale) : null;
  const formatHint = action ? manualPlayerActionTargetError(props.moduleId, action, "", props.locale) : null;

  async function execute(action: ModulePlayerActionDetails, role: string) {
    const targetError = target ? manualPlayerActionTargetError(props.moduleId, action, target, props.locale) : null;
    if (props.disabled || busy || Boolean(targetError) || !selectedTarget) {
      return;
    }
    setBusyId(action.id);
    setFeedback(null);
    try {
      await executeDeclaredRuntimePlayerAction(props.instanceId, action.id, target.trim(), role);
      setExpandedId(null);
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
      setBusyId(null);
    }
  }

  function lookUp() {
    const next = target.trim();
    if (!next || lookupError || busy) return;
    setLookedUp(next);
    setFeedback(null);
  }

  const targetLabel = props.locale === "zh-CN" && action?.target_label_zh_cn?.trim()
    ? action.target_label_zh_cn
    : action?.target_label || props.t("servers.playerCenter.manual.playerId", undefined, "Player ID");
  const helpText = [props.t("servers.playerCenter.manual.description", undefined,
    "Enter an exact player identifier obtained from the server to run an action."), formatHint].filter(Boolean).join(" ");

  // 手动查询 category content: only the ID field and lookup; the actions stay in the shared control area.
  const lookup = action ? (
    <form className="settings-schema-grid player-center-manual-lookup" aria-label={props.t("servers.playerCenter.manual.tab", undefined, "Manual lookup")}
      onSubmit={(event) => { event.preventDefault(); lookUp(); }}>
      <div className="settings-schema-field">
        <ConfigurationHelp description={helpText}>{(help) =>
          <label className="detail-label settings-field-label" htmlFor={`manual-player-target-${props.instanceId}`} ref={help.anchorRef} {...help.interactionProps}
            aria-describedby={help.descriptionId}>
            {targetLabel}<ShellIcon name="alert-circle" className="maintenance-help-icon" />
          </label>}</ConfigurationHelp>
        <input
          id={`manual-player-target-${props.instanceId}`}
          type="text"
          className="settings-schema-input"
          value={target}
          aria-invalid={Boolean(lookupError)}
          aria-describedby={lookupError ? `manual-player-target-error-${props.instanceId}` : undefined}
          disabled={props.disabled || busy}
          placeholder={props.locale === "zh-CN" && action.target_placeholder_zh_cn?.trim()
            ? action.target_placeholder_zh_cn
            : action.target_placeholder || props.t("servers.playerCenter.manual.playerIdPlaceholder", undefined, "Exact player ID")}
          onChange={(event) => setTarget(event.target.value)}
        />
        {lookupError ? <span className="form-note form-note--error" role="alert" id={`manual-player-target-error-${props.instanceId}`}>{lookupError}</span> : null}
      </div>
      <div className="player-center-manual-lookup-action">
        <button type="submit" className="secondary-button player-center-manual-use"
          disabled={props.disabled || busy || !target.trim() || Boolean(lookupError)}>
          <ShellIcon name="search" />{props.t("servers.playerCenter.manual.lookup", undefined, "Look up")}
        </button>
      </div>
    </form>
  ) : null;

  function renderAction(action: ModulePlayerActionDetails) {
    const roleValues = action?.role_values ?? [];
    const selectedRole = roleValues.includes(roles[action.id] ?? "") ? roles[action.id] : roleValues[0] ?? "";
    const targetError = target ? manualPlayerActionTargetError(props.moduleId, action, target, props.locale) : null;
    const blockedReason = props.disabled
      ? props.t("servers.playerCenter.manual.requiresRunning", undefined, "Available while the server is running.")
      : targetError ?? (!selectedTarget ? props.t("servers.playerCenter.manual.needsTarget", undefined, "Select or enter a player action.") : null);
    const actionDisabled = props.disabled || busy || Boolean(targetError) || !selectedTarget;
    const label = localizedActionLabel(action, props.locale);
    const content = busyId === action.id ? props.t("servers.playerCenter.actions.running", undefined, "Running...") : label;
    const expanded = roleValues.length > 1 && expandedId === action.id && !actionDisabled;
    const buttonClass = `secondary-button player-center-action-button${action.destructive ? " danger" : ""}`;
    const confirmation = props.t("servers.playerCenter.manual.confirm",
      { action: selectedRole ? `${label} (${selectedRole})` : label, target: target.trim() },
      "Run \u201c{action}\u201d for known identity \u201c{target}\u201d?");
    if (expanded) {
      const confirmLabel = props.locale === "zh-CN" ? `\u786e\u8ba4${label}` : `Confirm ${label}`;
      return <div key={action.id} className="player-access-selected-editor is-inline player-center-manual-action" data-manual-action={action.id}>
        <label className="settings-schema-field" htmlFor={`manual-player-role-${props.instanceId}-${action.id}`}>
          <span className="detail-label settings-field-label">{props.t("servers.playerCenter.manual.role", undefined, "Role")}</span>
          <select id={`manual-player-role-${props.instanceId}-${action.id}`} className="settings-schema-input settings-schema-select" value={selectedRole}
            disabled={props.disabled || busy} onChange={(event) => setRoles((current) => ({ ...current, [action.id]: event.target.value }))}>
            {roleValues.map((roleValue) => <option key={roleValue} value={roleValue}>{roleValue}</option>)}
          </select>
        </label>
        <div className="player-access-roster-action-pair">
          {action.destructive ? (
            <InlineConfirmAction className={buttonClass} disabled={actionDisabled}
              scopeKey={JSON.stringify([props.instanceId, action.id, target.trim(), selectedRole])}
              confirmation={confirmation} onConfirm={() => execute(action, selectedRole)}>
              {busyId === action.id ? content : confirmLabel}
            </InlineConfirmAction>
          ) : <button type="button" className={buttonClass} disabled={actionDisabled} onClick={() => void execute(action, selectedRole)}>
            {busyId === action.id ? content : confirmLabel}
          </button>}
          <button type="button" className="secondary-button player-center-action-button" disabled={busy} onClick={() => setExpandedId(null)}>
            {props.locale === "zh-CN" ? "\u53d6\u6d88" : "Cancel"}
          </button>
        </div>
      </div>;
    }
    return <ConfigurationHelp key={action.id} description={busy ? null : blockedReason}>{(help) =>
      <div className="player-access-total-action player-center-manual-action" data-manual-action={action.id} ref={help.anchorRef} {...help.interactionProps}
        tabIndex={actionDisabled ? 0 : undefined} role={actionDisabled ? "group" : undefined} aria-label={actionDisabled ? label : undefined}
        aria-disabled={actionDisabled || undefined} aria-describedby={help.descriptionId}>
        {roleValues.length > 1 ? (
          <button type="button" className={buttonClass} disabled={actionDisabled} aria-describedby={help.descriptionId}
            aria-expanded={false} onClick={() => setExpandedId(action.id)}>{content}</button>
        ) : action.destructive ? (
          <InlineConfirmAction
            className={buttonClass}
            disabled={actionDisabled}
            aria-describedby={help.descriptionId}
            scopeKey={JSON.stringify([props.instanceId, action.id, target.trim(), selectedRole])}
            confirmation={confirmation}
            onConfirm={() => execute(action, selectedRole)}
          >
            {content}
          </InlineConfirmAction>
        ) : (
          <button type="button" className={buttonClass} disabled={actionDisabled} aria-describedby={help.descriptionId}
            onClick={() => void execute(action, selectedRole)}>
            {content}
          </button>
        )}
      </div>}</ConfigurationHelp>;
  }

  // One stack button per action, same markup and classes as the selected-player actions; no headings or pickers.
  const actions = (
    <section className="player-center-action-group player-center-manual-actions"
      aria-label={props.t("servers.playerCenter.manual.title", undefined, "Server player actions")} aria-busy={busy}>
      <div className="player-center-action-list">{eligibleActions.map(renderAction)}</div>
      {feedback ? <ActivityNotice tone={feedback.tone} onDismiss={() => setFeedback(null)}>{feedback.text}</ActivityNotice> : null}
    </section>
  );
  if (hosts) {
    // Inside the players workspace: the actions join the shared control area.
    return <>
      {hosts.actions ? createPortal(actions, hosts.actions) : null}
    </>;
  }
  return <div className="player-center-manual-standalone">{lookup}{actions}</div>;
}
