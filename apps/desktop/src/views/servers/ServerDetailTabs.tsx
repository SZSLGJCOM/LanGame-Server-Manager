import type { KeyboardEvent, ReactNode } from "react";
import { ShellIcon, type ShellIconName } from "../../components/ShellIcon";
import { useConfigurationFieldHelp } from "../settings/ConfigurationFieldHelp";

export type ServerDetailTab = "runtime" | "players" | "settings" | "gm" | "mods" | "maintenance";

export interface ServerDetailTabSpec {
  id: ServerDetailTab;
  label: string;
  icon: ShellIconName;
  disabled?: boolean;
  disabledReason?: string;
}

interface ServerDetailTabsProps {
  tabs: readonly ServerDetailTabSpec[];
  activeTab: ServerDetailTab;
  panelId: string;
  label: string;
  onSelect(tab: ServerDetailTabSpec): void;
}

function DisabledTabHelp(props: { id: string; label: string; reason: string; children: ReactNode }) {
  const help = useConfigurationFieldHelp(props.id, props.reason, props.label, undefined, "instructions");
  return <span className="server-detail-tab-help" tabIndex={0}
    aria-label={props.label} aria-describedby={help.descriptionId}
    ref={help.anchorRef} {...help.interactionProps}>
    {props.children}
    {help.helpNode}
  </span>;
}

export function resolveDetailTabKey(
  tabs: readonly ServerDetailTabSpec[],
  currentId: ServerDetailTab,
  key: string
): ServerDetailTabSpec | null {
  const enabled = tabs.filter((tab) => !tab.disabled);
  if (enabled.length === 0) return null;
  if (key === "Home") return enabled[0];
  if (key === "End") return enabled[enabled.length - 1];
  if (key !== "ArrowLeft" && key !== "ArrowRight") return null;
  const currentIndex = enabled.findIndex((tab) => tab.id === currentId);
  if (currentIndex < 0) return enabled[0];
  const offset = key === "ArrowRight" ? 1 : -1;
  return enabled[(currentIndex + offset + enabled.length) % enabled.length];
}

export function ServerDetailTabs(props: ServerDetailTabsProps) {
  function handleKeyDown(event: KeyboardEvent<HTMLButtonElement>, currentId: ServerDetailTab) {
    if (event.altKey || event.ctrlKey || event.metaKey || event.nativeEvent.isComposing) return;
    const next = resolveDetailTabKey(props.tabs, currentId, event.key);
    if (!next) return;
    event.preventDefault();
    props.onSelect(next);
    event.currentTarget.ownerDocument.getElementById(`${props.panelId}-${next.id}`)?.focus();
  }

  return <div className="server-detail-tabs" role="tablist" aria-label={props.label}>
    {props.tabs.map((tab) => {
      const reason = tab.disabled ? tab.disabledReason : undefined;
      const button = <button
        key={tab.id}
        id={`${props.panelId}-${tab.id}`}
        type="button"
        role="tab"
        aria-selected={props.activeTab === tab.id}
        aria-controls={props.panelId}
        aria-disabled={tab.disabled || undefined}
        tabIndex={props.activeTab === tab.id ? 0 : -1}
        className={[
          "server-detail-tab",
          props.activeTab === tab.id ? "is-active" : "",
          tab.disabled ? "is-disabled" : ""
        ].filter(Boolean).join(" ")}
        disabled={tab.disabled}
        title={reason ? undefined : tab.label}
        onClick={() => props.onSelect(tab)}
        onKeyDown={(event) => handleKeyDown(event, tab.id)}>
        <ShellIcon name={tab.icon} className="server-detail-tab-icon" />
        <span className="server-detail-tab-label">{tab.label}</span>
      </button>;
      return reason ? <DisabledTabHelp key={tab.id} id={`${props.panelId}-${tab.id}-help`}
        label={tab.label} reason={reason}>{button}</DisabledTabHelp> : button;
    })}
  </div>;
}
