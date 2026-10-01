import { useEffect, useId, useMemo, useState } from "react";
import { ShellIcon } from "../../components/ShellIcon";
import { selectLocaleText, useI18n } from "../../i18n";
import type { BindAddressCandidate, InstanceDetails } from "../../types";
import { buildShareEndpoints } from "../../view-models";
import { resolveSelectedJoinEndpoint } from "./instance-connectivity-selection";
import { useConfigurationFieldHelp } from "./ConfigurationFieldHelp";

interface PlayerJoinAddressSelectProps {
  details: InstanceDetails;
  candidates: BindAddressCandidate[];
  copyDisabled?: boolean;
  copyDisabledHint?: string;
}

const STORAGE_PREFIX = "langame.join-address";

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
  const endpoints = useMemo(
    () => buildShareEndpoints(props.details, props.candidates, locale, t),
    [locale, props.candidates, props.details, t]
  );
  const [selectedAddress, setSelectedAddress] = useState(() => readStoredAddress(instanceId));
  const selectedEndpoint = useMemo(
    () => resolveSelectedJoinEndpoint(endpoints, selectedAddress),
    [endpoints, selectedAddress]
  );
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");
  const endpointPending = selectedEndpoint?.endpoint.includes(t("messages.pendingPort")) ?? true;
  const copyDisabled = endpointPending || Boolean(props.copyDisabled) || !selectedEndpoint;
  const copyLabel = selectLocaleText(locale, "复制入服地址", "Copy join address");
  const help = useConfigurationFieldHelp(useId(), props.copyDisabled ? props.copyDisabledHint : undefined,
    undefined, undefined, "instructions");

  useEffect(() => {
    if (selectedEndpoint && selectedAddress !== selectedEndpoint.address) {
      setSelectedAddress(selectedEndpoint.address);
    }
  }, [selectedAddress, selectedEndpoint]);

  useEffect(() => {
    setCopyState("idle");
  }, [copyDisabled, selectedEndpoint?.endpoint]);

  function selectAddress(address: string) {
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

  return (
    <div className="player-join-address" ref={help.anchorRef} {...help.interactionProps}
      tabIndex={copyDisabled && help.descriptionId ? 0 : undefined}
      role={help.descriptionId ? "group" : undefined}
      aria-label={help.descriptionId ? copyLabel : undefined} aria-describedby={help.descriptionId}>
      {help.helpNode}
      <select
        className="settings-schema-input settings-schema-select player-join-address__select"
        value={selectedEndpoint?.address ?? ""}
        disabled={endpoints.length === 0}
        onChange={(event) => selectAddress(event.target.value)}
      >
        {endpoints.map((endpoint) => (
          <option key={`${endpoint.kind}-${endpoint.address}`} value={endpoint.address}>
            {endpoint.label} · {endpoint.endpoint}
          </option>
        ))}
      </select>
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
  );
}
