import type {
  DstModConfigChoice,
  DstModConfigOptionSpec,
  DstModPrimitiveValue
} from "../../types";
import type { TranslateFn } from "../../i18n";
import { useConfigurationFieldHelp } from "../settings/ConfigurationFieldHelp";

export type DstModOptionPrimitiveValue = string | number | boolean;

interface DstModOptionFieldProps {
  boolFalse: string;
  boolTrue: string;
  currentValue: (value: DstModOptionPrimitiveValue) => string;
  defaultChoice: string;
  editable: boolean;
  explicitValue?: DstModOptionPrimitiveValue;
  hasExplicit: boolean;
  idPrefix: string;
  onChoiceChange: (value: DstModPrimitiveValue) => void;
  onNumberChange: (value: string) => void;
  onTextChange: (value: string) => void;
  option: DstModConfigOptionSpec;
  t: TranslateFn;
}

export function DstModOptionField(props: DstModOptionFieldProps) {
  const declaredChoices = buildSelectableChoices(
    props.option,
    props.defaultChoice,
    props.boolTrue,
    props.boolFalse
  );
  const effectiveValue = resolveEffectiveSpecValue(props.option, props.explicitValue);
  const controlValue = serializeSpecValue(effectiveValue);
  const selectChoices = declaredChoices.length > 0 && props.hasExplicit && props.explicitValue !== undefined && !declaredChoices.some(
    (choice) => serializeSpecValue(choice.value) === controlValue
  )
    ? [{ label: props.currentValue(props.explicitValue), value: effectiveValue }, ...declaredChoices]
    : declaredChoices;
  const valueKind = resolveSpecValueKind(props.option);
  const inputId = `${props.idPrefix}-input`;
  const help = useConfigurationFieldHelp(
    `${props.idPrefix}-description`,
    props.option.hover,
    props.option.label,
    props.t
  );
  const accessibility = {
    "aria-describedby": help.descriptionId,
    id: inputId
  };

  return (
    <div className="dst-mod-spec-row" ref={help.anchorRef} {...help.interactionProps}>
      <label className="dst-mod-spec-copy" htmlFor={inputId}>
        <span className="dst-mod-spec-label">{props.option.label}</span>
      </label>

      {selectChoices.length > 0 ? (
        <select
          {...accessibility}
          className="settings-schema-input"
          value={controlValue}
          disabled={!props.editable}
          onChange={(event) => {
            const selectedChoice = selectChoices.find(
              (choice) => serializeSpecValue(choice.value) === event.target.value
            );
            if (selectedChoice) props.onChoiceChange(selectedChoice.value);
          }}
        >
          {selectChoices.map((choice) => (
            <option
              key={`${props.option.name}:${serializeSpecValue(choice.value)}`}
              value={serializeSpecValue(choice.value)}
            >
              {choice.label}
            </option>
          ))}
        </select>
      ) : valueKind === "number" ? (
        <input
          {...accessibility}
          className="settings-schema-input"
          type="number"
          step="any"
          key={`${inputId}:${controlValue}`}
          defaultValue={typeof props.explicitValue === "number" ? String(props.explicitValue) : ""}
          placeholder={formatDefaultPlaceholder(props.option.default_value)}
          disabled={!props.editable}
          onBlur={(event) => {
            const value = event.target.value;
            const previous = typeof props.explicitValue === "number" ? String(props.explicitValue) : "";
            if (props.editable && value !== previous && event.target.validity.valid) props.onNumberChange(value);
          }}
          onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); }}
        />
      ) : (
        <input
          {...accessibility}
          className="settings-schema-input"
          type="text"
          key={`${inputId}:${controlValue}`}
          defaultValue={typeof props.explicitValue === "string" ? props.explicitValue : ""}
          placeholder={formatDefaultPlaceholder(props.option.default_value)}
          disabled={!props.editable}
          onBlur={(event) => {
            const value = event.target.value;
            const previous = typeof props.explicitValue === "string" ? props.explicitValue : "";
            if (props.editable && value !== previous) props.onTextChange(value);
          }}
          onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); }}
        />
      )}
      {help.helpNode}
    </div>
  );
}

function buildSelectableChoices(
  option: DstModConfigOptionSpec,
  defaultLabel: string,
  boolTrue: string,
  boolFalse: string
): DstModConfigChoice[] {
  const choices = [...option.options];
  if (choices.some((choice) => choice.value.kind === "default")) return choices;

  const kind = resolveSpecValueKind(option);
  if (kind === "boolean" && choices.length === 0) {
    return [
      { label: defaultLabel, value: { kind: "default" } },
      { label: boolTrue, value: { kind: "boolean", value: true } },
      { label: boolFalse, value: { kind: "boolean", value: false } }
    ];
  }
  return choices.length > 0
    ? [{ label: defaultLabel, value: { kind: "default" } }, ...choices]
    : [];
}

function resolveSpecValueKind(option: DstModConfigOptionSpec): "string" | "number" | "boolean" {
  for (const choice of option.options) {
    if (choice.value.kind === "string" || choice.value.kind === "number" || choice.value.kind === "boolean") {
      return choice.value.kind;
    }
  }
  const kind = option.default_value?.kind;
  return kind === "string" || kind === "number" || kind === "boolean" ? kind : "string";
}

function resolveEffectiveSpecValue(
  option: DstModConfigOptionSpec,
  explicitValue?: DstModOptionPrimitiveValue
): DstModPrimitiveValue {
  if (explicitValue !== undefined) return primitiveToSpecValue(explicitValue);
  return option.default_value ?? option.options[0]?.value ?? { kind: "default" };
}

function primitiveToSpecValue(value: DstModOptionPrimitiveValue): DstModPrimitiveValue {
  if (typeof value === "boolean") return { kind: "boolean", value };
  if (typeof value === "number") return { kind: "number", value };
  return { kind: "string", value };
}

function serializeSpecValue(value: DstModPrimitiveValue): string {
  if (value.kind === "default") return "default";
  return `${value.kind}:${String(value.value)}`;
}

function formatDefaultPlaceholder(value?: DstModPrimitiveValue | null): string {
  return !value || value.kind === "default" ? "" : String(value.value);
}
