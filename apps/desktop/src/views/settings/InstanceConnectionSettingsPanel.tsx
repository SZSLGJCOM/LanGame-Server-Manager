import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { fetchBindAddressCandidates } from "../../api";
import { formatBindAddressCandidateLabel } from "../../bind-addresses";
import { astroneerJoinAddressCandidates, astroneerRadminCandidates, buildAstroneerJoinAddressPatch,
  readAstroneerAdvertisedAddress, readAstroneerConnectionMode, resolveAstroneerConnectionMode } from "../../astroneer-network-settings";
import type { BindAddressCandidate, InstanceDetails, ModuleDetails, PortBinding } from "../../types";
import { InstancePortFields } from "./InstancePortFields";
import { ListenAddressSelect } from "./ListenAddressSelect";
import { PlayerJoinAddressSelect } from "./PlayerJoinAddressSelect";
import { buildConfigurationFieldIds } from "./ConfigurationField";
import type { SettingsObject } from "./settings-schema";
import { isArkModule } from "../../ark-clusters";
import {
  instanceNetworkControlState,
  instanceRuntimeOwnsNetworkSettings,
  materializeInstancePortEdit,
  networkDraftMatchesPersisted,
  partitionInstancePorts,
  resolveVisibleInstancePorts
} from "./instance-port-presentation";
import "./InstanceConnectionSettingsPanel.css";
import { ActivityNotice } from "../../components/ActivityNotice";
import { AstroneerNetworkRequirements } from "../../components/AstroneerNetworkRequirements";

interface InstanceConnectionSettingsPanelProps {
  details: InstanceDetails;
  moduleDetails: ModuleDetails | null;
  bindAddressCandidates: BindAddressCandidate[];
  bindIp: string;
  ports: PortBinding[];
  defaultPorts: PortBinding[];
  disabled?: boolean;
  networkSettingsUnsaved?: boolean;
  joinAddressInputId?: string;
  settings?: Readonly<SettingsObject>;
  onSettingsPatch?: (patch: Readonly<SettingsObject>) => void;
  onPortsChange: (ports: PortBinding[]) => void;
  onBindIpChange: (value: string) => void;
  onValidationBlockedChange?: (blocked: boolean) => void;
}

export function InstanceConnectionSettingsPanel(props: InstanceConnectionSettingsPanelProps) {
  const { locale, t } = useI18n();
  const [portDraftDirtyGroups, setPortDraftDirtyGroups] = useState({
    player: false,
    service: false,
    unclassified: false
  });
  const supportsStrictBindAddress = props.moduleDetails?.runtime?.bind_address?.mode === "strict";
  const runtimeOwnsNetworkSettings = instanceRuntimeOwnsNetworkSettings(props.details);
  const addressDisabled = Boolean(props.disabled) || runtimeOwnsNetworkSettings || props.details.summary.active_process_count > 0;
  const [scan, setScan] = useState<{ baseline: BindAddressCandidate[]; candidates: BindAddressCandidate[] } | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [scanError, setScanError] = useState<string | null>(null);
  const scanRequest = useRef(0);
  const scanBusy = useRef(false);
  const candidates = scan?.baseline === props.bindAddressCandidates ? scan.candidates : props.bindAddressCandidates;
  const astroneer = props.details.summary.module_id === "astroneer";
  const joinCandidates = astroneerJoinAddressCandidates(candidates);
  const savedAddress = readAstroneerAdvertisedAddress(props.details.settings_json);
  const savedRadminAddress = readAstroneerConnectionMode(props.details.settings_json) === "radmin" ? savedAddress : "";
  const addressOptions = joinCandidates.map((candidate) => ({ address: candidate.address,
    label: formatBindAddressCandidateLabel(candidate, locale) }));
  if (savedAddress && !addressOptions.some((option) => option.address === savedAddress)) {
    addressOptions.unshift({ address: savedAddress, label: `${savedAddress} · ${
      savedRadminAddress
        ? t("astroneer.network.radmin.savedUnavailable", { address: "Radmin VPN" })
        : t("astroneer.network.share.advertised")}` });
  }
  const addressValue = typeof props.settings?.public_ip === "string" ? props.settings.public_ip : "";
  const addressAutomatic = props.settings && resolveAstroneerConnectionMode(props.settings) === "public" && !addressValue.trim();
  const radminMissing = astroneer && props.settings && resolveAstroneerConnectionMode(props.settings) === "radmin"
    && addressValue && !astroneerRadminCandidates(candidates).some((candidate) => candidate.address === addressValue);
  const controlState = instanceNetworkControlState(
    Boolean(props.disabled) || runtimeOwnsNetworkSettings,
    supportsStrictBindAddress
  );
  const visiblePorts = useMemo(
    () => resolveVisibleInstancePorts(props.ports, props.defaultPorts),
    [props.defaultPorts, props.ports]
  );
  const partitionedPorts = useMemo(
    () => partitionInstancePorts(visiblePorts, props.moduleDetails?.runtime?.port_roles ?? []),
    [props.moduleDetails?.runtime?.port_roles, visiblePorts]
  );
  const networkPersisted = networkDraftMatchesPersisted(props.details, props.bindIp, props.ports);
  const nonzeroMapPortNames = isArkModule(props.details.summary.module_id)
    ? visiblePorts.filter((port) => port.name.startsWith("map-")).map((port) => port.name)
    : [];
  const localPortDraftDirty = Object.values(portDraftDirtyGroups).some(Boolean);

  useEffect(() => () => { scanRequest.current++; }, []);

  async function refreshAddresses() {
    if (addressDisabled || scanBusy.current) return;
    scanBusy.current = true;
    const request = ++scanRequest.current;
    setRefreshing(true);
    setScanError(null);
    try {
      const found = await fetchBindAddressCandidates();
      if (request === scanRequest.current) setScan({ baseline: props.bindAddressCandidates, candidates: found });
    } catch {
      if (request === scanRequest.current) setScanError(t("astroneer.network.scanFailed"));
    } finally {
      scanBusy.current = false;
      if (request === scanRequest.current) setRefreshing(false);
    }
  }

  useEffect(() => {
    props.onValidationBlockedChange?.(localPortDraftDirty);
    return () => props.onValidationBlockedChange?.(false);
  }, [localPortDraftDirty, props.onValidationBlockedChange]);

  const handlePlayerPortDraftDirtyChange = useCallback((dirty: boolean) => {
    setPortDraftDirtyGroups((current) => current.player === dirty ? current : { ...current, player: dirty });
  }, []);
  const handleServicePortDraftDirtyChange = useCallback((dirty: boolean) => {
    setPortDraftDirtyGroups((current) => current.service === dirty ? current : { ...current, service: dirty });
  }, []);
  const handleUnclassifiedPortDraftDirtyChange = useCallback((dirty: boolean) => {
    setPortDraftDirtyGroups((current) => current.unclassified === dirty ? current : { ...current, unclassified: dirty });
  }, []);

  function handlePortChange(port: number, binding: PortBinding) {
    props.onPortsChange(materializeInstancePortEdit(props.ports, props.defaultPorts, binding, port));
  }

  return (
    <div className="instance-connection-settings">
      {runtimeOwnsNetworkSettings || (astroneer && props.details.summary.active_process_count > 0) ? (
        <ActivityNotice tone="warning">
          {astroneer ? t("astroneer.network.stopFirst") : t(
            "settings.network.runtimeLock",
            undefined,
            "Stop the server before changing its listen address or ports."
          )}
        </ActivityNotice>
      ) : null}
      <section className="settings-schema-section">
        <div className="instance-connection-settings__grid">
          <div className={supportsStrictBindAddress
            ? "settings-schema-grid instance-connection-settings__listener-row"
            : "settings-schema-grid instance-connection-settings__listener-row is-bind-fixed"}
          >
            {supportsStrictBindAddress ? (
              <label className="settings-schema-field">
                <span className="detail-label">{t("settings.details.bindIp", undefined, "Listen address")}</span>
                <ListenAddressSelect
                  value={props.bindIp}
                  candidates={props.bindAddressCandidates}
                  disabled={controlState.bindDisabled}
                  onChange={props.onBindIpChange}
                />
              </label>
            ) : null}
            <div className={partitionedPorts.player.length > 0
              ? "instance-connection-settings__ports"
              : "instance-connection-settings__ports is-pending"}
            >
              <InstancePortFields
                ports={partitionedPorts.player}
                role="player"
                caption={t("settings.network.playerPorts", undefined, "Player ports")}
                disabled={controlState.portsDisabled}
                onPortChange={handlePortChange}
                onPortDraftDirtyChange={handlePlayerPortDraftDirtyChange}
              />
            </div>
          </div>
          <div className="settings-schema-field" data-field-key={astroneer ? "public_ip" : undefined}>
            <div className="astroneer-network-requirements-row">
              <span className="detail-label">{t("settings.network.joinAddress", undefined, "Player join address")}</span>
              {props.details.summary.module_id === "astroneer" ? <AstroneerNetworkRequirements showReachability /> : null}
            </div>
            <PlayerJoinAddressSelect
              key={props.details.summary.id}
              details={props.details}
              candidates={props.bindAddressCandidates}
              selection={astroneer && props.settings && props.onSettingsPatch ? {
                value: addressAutomatic ? null : addressValue,
                options: addressOptions,
                inputId: props.joinAddressInputId ?? buildConfigurationFieldIds("public_ip", "configuration-astroneer").inputId,
                disabled: addressDisabled,
                refreshing,
                onRefresh: () => void refreshAddresses(),
                onChange: (address) => {
                  if (!addressDisabled) props.onSettingsPatch!(buildAstroneerJoinAddressPatch(address, candidates,
                    savedRadminAddress || (resolveAstroneerConnectionMode(props.settings!) === "radmin" ? addressValue : "")));
                }
              } : undefined}
              copyDisabled={!networkPersisted || localPortDraftDirty || props.networkSettingsUnsaved}
              copyDisabledHint={t(
                "settings.network.pendingSave",
                undefined,
                "Save network changes before copying the join address."
              )}
            />
            {scanError ? <ActivityNotice tone="error">{scanError}</ActivityNotice> : null}
            {radminMissing ? <ActivityNotice tone="warning">{t("astroneer.network.radmin.stale")}</ActivityNotice> : null}
          </div>
        </div>
      </section>

      {partitionedPorts.service.length > 0 ? (
        <section className="settings-schema-section instance-connection-settings__port-group">
          <h4 className="guided-field-group-title">{t("settings.network.servicePorts", undefined, "Management service ports")}</h4>
          <div className="instance-connection-settings__ports">
            <InstancePortFields
              ports={partitionedPorts.service}
              role="service"
              disabled={controlState.portsDisabled}
              onPortChange={handlePortChange}
              onPortDraftDirtyChange={handleServicePortDraftDirtyChange}
            />
          </div>
        </section>
      ) : null}

      {partitionedPorts.unclassified.length > 0 ? (
        <section className="settings-schema-section instance-connection-settings__port-group">
          <h4 className="guided-field-group-title">{t("settings.network.otherPorts", undefined, "Other ports")}</h4>
          <div className="instance-connection-settings__ports">
            <InstancePortFields
              ports={partitionedPorts.unclassified}
              nonzeroPortNames={nonzeroMapPortNames}
              disabled={controlState.portsDisabled}
              onPortChange={handlePortChange}
              onPortDraftDirtyChange={handleUnclassifiedPortDraftDirtyChange}
            />
          </div>
        </section>
      ) : null}
    </div>
  );
}
