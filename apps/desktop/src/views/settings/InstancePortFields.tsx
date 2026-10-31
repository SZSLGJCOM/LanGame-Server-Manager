import { useEffect, useId, useMemo, useRef, useState } from "react";
import { selectLocaleText, useI18n } from "../../i18n";
import { ConfigurationHelp } from "./ConfigurationFieldHelp";
import { ActivityNotice } from "../../components/ActivityNotice";
import { focusConfigurationControl } from "./configuration-workspace-state";
import type { ModulePortRole, PortBinding } from "../../types";
import {
  formatInstancePortName,
  instancePortBindingKey,
  minimumInstancePortValue,
  orderInstancePortsForEditing,
  parseInstancePortDraft,
  reconcileInstancePortDrafts
} from "./instance-port-presentation";

interface InstancePortFieldsProps {
  ports: PortBinding[];
  role?: ModulePortRole;
  /** Group caption shown in the shared help tooltip when it adds context. */
  caption?: string;
  nonzeroPortNames?: readonly string[];
  disabled?: boolean;
  onPortChange: (port: number, binding: PortBinding) => void;
  onPortDraftDirtyChange?: (dirty: boolean) => void;
}

function buildPortDrafts(ports: PortBinding[]): Record<string, string> {
  return Object.fromEntries(ports.map((port) => [instancePortBindingKey(port), String(port.port)]));
}

export function InstancePortFields(props: InstancePortFieldsProps) {
  const { locale } = useI18n();
  const portsToEdit = useMemo(() => orderInstancePortsForEditing(props.ports), [props.ports]);
  const primaryPortKey = portsToEdit[0] ? instancePortBindingKey(portsToEdit[0]) : "";
  const validationId = useId();
  const previousPortsRef = useRef(portsToEdit);
  const [portDrafts, setPortDrafts] = useState<Record<string, string>>(() => buildPortDrafts(portsToEdit));
  const portsSignature = useMemo(
    () => JSON.stringify(portsToEdit.map((port) => ({ name: port.name, protocol: port.protocol, port: port.port }))),
    [portsToEdit]
  );

  useEffect(() => {
    const previousPorts = previousPortsRef.current;
    setPortDrafts((current) => reconcileInstancePortDrafts(current, previousPorts, portsToEdit));
    previousPortsRef.current = portsToEdit;
  }, [portsSignature]);

  const invalidPorts = portsToEdit.filter((port) => {
    const key = instancePortBindingKey(port);
    return parseInstancePortDraft(
      portDrafts[key] ?? String(port.port),
      minimumInstancePortValue(props.role ?? null, key === primaryPortKey, props.nonzeroPortNames?.includes(port.name))
    ) === null;
  });
  const hasInvalidDrafts = invalidPorts.length > 0;

  useEffect(() => {
    props.onPortDraftDirtyChange?.(hasInvalidDrafts);
    return () => props.onPortDraftDirtyChange?.(false);
  }, [hasInvalidDrafts, props.onPortDraftDirtyChange]);

  function changePortDraft(port: PortBinding, rawValue: string) {
    const key = instancePortBindingKey(port);
    setPortDrafts((current) => ({ ...current, [key]: rawValue }));

    const minimumPort = minimumInstancePortValue(props.role ?? null, key === primaryPortKey, props.nonzeroPortNames?.includes(port.name));
    const parsed = parseInstancePortDraft(rawValue, minimumPort);
    if (parsed !== null) props.onPortChange(parsed, port);
  }

  if (portsToEdit.length === 0) {
    return <div className="settings-schema-field instance-port-fields__item">{props.caption ? <span className="detail-label">{props.caption}</span> : null}<span className="instance-port-fields__empty">{selectLocaleText(locale, "未声明", "Not declared")}</span></div>;
  }

  return (
    <div className="settings-schema-grid instance-port-fields">
      {portsToEdit.map((port, index) => {
        const key = instancePortBindingKey(port);
        const isPrimaryPort = key === primaryPortKey;
        const isPrimaryPlayerPort = props.role === "player" && isPrimaryPort;
        const value = portDrafts[key] ?? String(port.port);
        const minimumPort = minimumInstancePortValue(props.role ?? null, isPrimaryPort, props.nonzeroPortNames?.includes(port.name));
        const invalid = invalidPorts.includes(port);

        // Tooltip only when the group caption adds information beyond the field label.
        const helpText = props.caption
          ? props.role === "player" && !isPrimaryPlayerPort
            ? `${props.caption} \u00B7 ${selectLocaleText(locale, "\u8865\u5145", "Extra")}`
            : props.caption
          : undefined;

        return (
          <ConfigurationHelp key={key} description={helpText}>
            {(help) => (
              <label ref={help.anchorRef} {...help.interactionProps} className={isPrimaryPlayerPort
                ? "settings-schema-field instance-port-fields__item is-primary"
                : "settings-schema-field instance-port-fields__item"}>
                <span className="detail-label instance-port-fields__name">
                  {formatInstancePortName(port.name)} / {String(port.protocol).toUpperCase()}
                </span>
                <input
                  id={`${validationId}-${index}`}
                  className="instance-port-fields__input settings-schema-input"
                  type="number"
                  min={minimumPort}
                  max={65535}
                  value={value}
                  aria-invalid={invalid || undefined}
                  aria-describedby={[invalid ? validationId : undefined, help.descriptionId].filter(Boolean).join(" ") || undefined}
                  disabled={props.disabled}
                  onChange={(event) => changePortDraft(port, event.target.value)}
                />
              </label>
            )}
          </ConfigurationHelp>
        );
      })}
      {hasInvalidDrafts ? <>
        <span id={validationId} className="configuration-field-error-a11y">
          {selectLocaleText(locale, "端口必须是有效范围内的整数。", "Ports must be whole numbers within the allowed range.")}
        </span>
        <ActivityNotice key={JSON.stringify(invalidPorts.map(instancePortBindingKey).sort())} tone="error" action={<>{portsToEdit.map((port, index) => {
          const key = instancePortBindingKey(port);
          if (!invalidPorts.includes(port)) return null;
          return <button key={key} type="button" className="secondary-button configuration-validation-target"
            aria-controls={`${validationId}-${index}`} onClick={() => focusConfigurationControl(`${validationId}-${index}`)}>
            {formatInstancePortName(port.name)} / {String(port.protocol).toUpperCase()}
          </button>;
        })}</>}>
          {selectLocaleText(locale, "端口必须是有效范围内的整数。", "Ports must be whole numbers within the allowed range.")}
        </ActivityNotice>
      </> : null}
    </div>
  );
}
