import { useEffect, useId, useMemo, useRef, useState } from "react";
import { selectLocaleText, useI18n } from "../../i18n";
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

  const hasInvalidDrafts = portsToEdit.some((port) => {
    const key = instancePortBindingKey(port);
    return parseInstancePortDraft(
      portDrafts[key] ?? String(port.port),
      minimumInstancePortValue(props.role ?? null, key === primaryPortKey, props.nonzeroPortNames?.includes(port.name))
    ) === null;
  });

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
    return <span className="instance-port-fields__empty">{selectLocaleText(locale, "未声明", "Not declared")}</span>;
  }

  return (
    <div className="instance-port-fields">
      {portsToEdit.map((port) => {
        const key = instancePortBindingKey(port);
        const isPrimaryPort = key === primaryPortKey;
        const isPrimaryPlayerPort = props.role === "player" && isPrimaryPort;
        const value = portDrafts[key] ?? String(port.port);
        const minimumPort = minimumInstancePortValue(props.role ?? null, isPrimaryPort, props.nonzeroPortNames?.includes(port.name));
        const invalid = parseInstancePortDraft(value, minimumPort) === null;

        return (
          <label key={key} className={isPrimaryPlayerPort ? "instance-port-fields__item is-primary" : "instance-port-fields__item"}>
            <span className="instance-port-fields__label">
              {isPrimaryPlayerPort
                ? selectLocaleText(locale, "端口", "Host Port")
                : props.role === "player"
                  ? selectLocaleText(locale, "补充", "Extra")
                  : selectLocaleText(locale, "端口", "Port")}
            </span>
            <input
              className="instance-port-fields__input settings-schema-input"
              type="number"
              min={minimumPort}
              max={65535}
              value={value}
              aria-invalid={invalid || undefined}
              aria-describedby={invalid ? validationId : undefined}
              disabled={props.disabled}
              onChange={(event) => changePortDraft(port, event.target.value)}
            />
            <span className="instance-port-fields__name">
              {formatInstancePortName(port.name)} / {String(port.protocol).toUpperCase()}
            </span>
          </label>
        );
      })}
      {hasInvalidDrafts ? <p id={validationId} className="instance-port-fields__error" role="alert">
        {selectLocaleText(locale, "端口必须是有效范围内的整数。", "Ports must be whole numbers within the allowed range.")}
      </p> : null}
    </div>
  );
}
