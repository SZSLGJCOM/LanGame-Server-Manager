import { useEffect, useId, useMemo, useState } from "react";
import { ShellIcon } from "../../components/ShellIcon";
import { selectLocaleText, useI18n } from "../../i18n";
import type { BindAddressCandidate, InstanceDetails } from "../../types";
import { buildShareEndpoints, valheimCrossplayJoinGuidance } from "../../view-models";
import { resolveSelectedJoinEndpoint } from "./instance-connectivity-selection";
import { useConfigurationFieldHelp } from "./ConfigurationFieldHelp";

export interface PlayerJoinAddressSelection {
  value: string | null;
  options: readonly { address: string; label: string }[];
  inputId: string;
  disabled: boolean;
  refreshing?: boolean;
  onChange: (address: string | null) => void;
  onRefresh?: () => void;
}

interface PlayerJoinAddressSelectProps {
  details: InstanceDetails;
  candidates: BindAddressCandidate[];
  copyDisabled?: boolean;
  copyDisabledHint?: string;
  selection?: PlayerJoinAddressSelection;
}

const STORAGE_PREFIX = "langame.join-address";
const CUSTOM_ADDRESS = "__custom__";

function readStoredAddress(instanceId: string): string {
  try {
    return window.localStorage.getItem(`${STORAGE_PREFIX}.${instanceId}`) ?? "";
  } catch {
    return "";
  }
}

function storeAddress(instanceId: string, address: string) {
  try {
    window.localStorage.setItem(`${STORAGE_PREFIX}.${instanceId}`, address);
  } catch {
    // The current selection remains usable when browser storage is unavailable.
  }
}

export function PlayerJoinAddressSelect(props: PlayerJoinAddressSelectProps) {
  const { locale, t } = useI18n();
  const instanceId = props.details.summary.id;
  const crossplay = valheimCrossplayJoinGuidance(props.details, t);
  const endpoints = useMemo(
    () => buildShareEndpoints(props.details, props.candidates, locale, t),
    [locale, props.candidates, props.details, t]
  );
  const [selectedAddress, setSelectedAddress] = useState(() => readStoredAddress(instanceId));
  const [customEditing, setCustomEditing] = useState(false);
  const selectedEndpoint = useMemo(
    () => props.selection ? endpoints.find((endpoint) => endpoint.address === props.selection!.value)
      : resolveSelectedJoinEndpoint(endpoints, selectedAddress),
    [endpoints, props.selection, selectedAddress]
  );
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");
  const endpointPending = selectedEndpoint?.endpoint.includes(t("messages.pendingPort")) ?? true;
  const copyDisabled = endpointPending || Boolean(props.copyDisabled) || !selectedEndpoint;
  const copyLabel = selectLocaleText(locale, "复制入服地址", "Copy join address");
  const emptyAstroneer = props.details.summary.module_id === "astroneer" && endpoints.length === 0;
  const selection = props.selection;
  const custom = Boolean(selection && (customEditing || (selection.value !== null
    && !selection.options.some((option) => option.address === selection.value))));
  const joinLabel = t("settings.network.joinAddress", undefined, "Player join address");
  const refreshLabel = selectLocaleText(locale, "刷新地址", "Refresh addresses");
  const help = useConfigurationFieldHelp(useId(), props.copyDisabled ? props.copyDisabledHint
    : emptyAstroneer ? t("astroneer.network.share.pending") : undefined,
    undefined, undefined, "instructions");

  useEffect(() => {
    if (!selection && selectedEndpoint && selectedAddress !== selectedEndpoint.address) {
      setSelectedAddress(selectedEndpoint.address);
    }
  }, [selectedAddress, selectedEndpoint, selection]);

  useEffect(() => {
    setCopyState("idle");
  }, [copyDisabled, selectedEndpoint?.endpoint]);

  function selectAddress(address: string) {
    if (selection) {
      if (selection.disabled) return;
      setCustomEditing(address === CUSTOM_ADDRESS);
      if (address !== CUSTOM_ADDRESS) selection.onChange(address || null);
      else if (selection.value === null) selection.onChange("");
      return;
    }
    setSelectedAddress(address);
    storeAddress(instanceId, address);
  }

  async function copyJoinEndpoint() {
    if (copyDisabled || !selectedEndpoint) {
      return;
    }

    try {
      await navigator.clipboard.writeText(selectedEndpoint.endpoint);
      setCopyState("copied");
    } catch {
      setCopyState("failed");
    }
  }

  if (crossplay) {
    return <p className="player-join-address__instructions" role="note">{crossplay.instructions}</p>;
  }

  return (
    <div className="player-join-address" ref={help.anchorRef} {...help.interactionProps}
      tabIndex={copyDisabled && help.descriptionId ? 0 : undefined}
      role={help.descriptionId ? "group" : undefined}
      aria-label={help.descriptionId ? copyLabel : undefined} aria-describedby={help.descriptionId}>
      {help.helpNode}
      <select
        id={selection?.inputId}
        aria-label={joinLabel}
        className="settings-schema-input settings-schema-select player-join-address__select"
        value={selection ? custom ? CUSTOM_ADDRESS : selection.value ?? "" : selectedEndpoint?.address ?? ""}
        disabled={selection ? selection.disabled : endpoints.length === 0}
        onChange={(event) => selectAddress(event.target.value)}
      >
        {selection ? <>
          <option value="">{t("astroneer.network.address.automatic")}</option>
          {selection.options.map((option) => <option key={option.address} value={option.address}>{option.label}</option>)}
          <option value={CUSTOM_ADDRESS}>{selectLocaleText(locale, "输入其它 IPv4 地址…", "Enter another IPv4 address…")}</option>
        </> : <>
        {emptyAstroneer ? <option value="">{t("astroneer.network.share.pending")}</option> : null}
        {endpoints.map((endpoint) => (
          <option key={`${endpoint.kind}-${endpoint.address}`} value={endpoint.address}>
            {endpoint.label} · {endpoint.endpoint}
          </option>
        ))}</>}
      </select>
      <div className="player-join-address__actions">
      {selection?.onRefresh ? <button type="button" className="player-join-address__refresh"
        disabled={selection.disabled || selection.refreshing} aria-label={refreshLabel} title={refreshLabel}
        onClick={selection.onRefresh}>
        <ShellIcon name="refresh" className="player-join-address__copy-icon" />
      </button> : null}
      <button
        type="button"
        className="player-join-address__copy"
        disabled={copyDisabled}
        onClick={() => void copyJoinEndpoint()}
        aria-label={copyLabel} aria-describedby={help.descriptionId}
      >
        <ShellIcon name={copyState === "copied" ? "check" : "copy"} className="player-join-address__copy-icon" />
        <span>
          {copyState === "copied"
            ? selectLocaleText(locale, "已复制", "Copied")
            : copyState === "failed"
              ? selectLocaleText(locale, "失败", "Failed")
              : selectLocaleText(locale, "复制", "Copy")}
        </span>
      </button>
      </div>
      {selection && custom ? <input className="settings-schema-input player-join-address__custom"
        value={selection.value ?? ""} disabled={selection.disabled}
        aria-label={selectLocaleText(locale, "玩家连接 IPv4 地址", "Player join IPv4 address")}
        autoComplete="off" spellCheck={false} inputMode="decimal" placeholder="192.168.1.10"
        onChange={(event) => { if (!selection.disabled) selection.onChange(event.target.value); }} /> : null}
    </div>
  );
}
