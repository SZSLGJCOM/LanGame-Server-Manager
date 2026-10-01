import { useCallback, useEffect, useMemo, useState } from "react";
import { useI18n } from "../../i18n";
import type { BindAddressCandidate, InstanceDetails, ModuleDetails, PortBinding } from "../../types";
import { InstancePortFields } from "./InstancePortFields";
import { ListenAddressSelect } from "./ListenAddressSelect";
import { PlayerJoinAddressSelect } from "./PlayerJoinAddressSelect";
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

interface InstanceConnectionSettingsPanelProps {
  details: InstanceDetails;
  moduleDetails: ModuleDetails | null;
  bindAddressCandidates: BindAddressCandidate[];
  bindIp: string;
  ports: PortBinding[];
  defaultPorts: PortBinding[];
  disabled?: boolean;
  onPortsChange: (ports: PortBinding[]) => void;
  onBindIpChange: (value: string) => void;
  onValidationBlockedChange?: (blocked: boolean) => void;
}

export function InstanceConnectionSettingsPanel(props: InstanceConnectionSettingsPanelProps) {
  const { t } = useI18n();
  const [portDraftDirtyGroups, setPortDraftDirtyGroups] = useState({
    player: false,
    service: false,
    unclassified: false
  });
  const supportsStrictBindAddress = props.moduleDetails?.runtime?.bind_address?.mode === "strict";
  const runtimeOwnsNetworkSettings = instanceRuntimeOwnsNetworkSettings(props.details);
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
      {runtimeOwnsNetworkSettings ? (
        <p className="instance-connection-settings__runtime-lock" role="status">
          {t(
            "settings.network.runtimeLock",
            undefined,
            "Stop the server before changing its listen address or ports."
          )}
        </p>
      ) : null}
      <section className="settings-schema-section">
        <div className="settings-schema-grid instance-connection-settings__grid">
          <div className={supportsStrictBindAddress
            ? "instance-connection-settings__listener-row"
            : "instance-connection-settings__listener-row is-bind-fixed"}
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
              <span className="detail-label">{t("settings.network.playerPorts", undefined, "Player ports")}</span>
              <InstancePortFields
                ports={partitionedPorts.player}
                role="player"
                disabled={controlState.portsDisabled}
                onPortChange={handlePortChange}
                onPortDraftDirtyChange={handlePlayerPortDraftDirtyChange}
              />
            </div>
          </div>
          <div className="settings-schema-field">
            <span className="detail-label">{t("settings.network.joinAddress", undefined, "Player join address")}</span>
            <PlayerJoinAddressSelect
              key={props.details.summary.id}
              details={props.details}
              candidates={props.bindAddressCandidates}
              copyDisabled={!networkPersisted || localPortDraftDirty}
              copyDisabledHint={t(
                "settings.network.pendingSave",
                undefined,
                "Save network changes before copying the join address."
              )}
            />
          </div>
        </div>
      </section>

      {partitionedPorts.service.length > 0 ? (
        <section className="settings-schema-section instance-connection-settings__port-group">
          <span className="detail-label">{t("settings.network.servicePorts", undefined, "Management service ports")}</span>
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
          <span className="detail-label">{t("settings.network.otherPorts", undefined, "Other ports")}</span>
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
