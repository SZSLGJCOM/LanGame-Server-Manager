import { useEffect, useRef, useState, type ComponentType } from "react";
import type { TranslateFn } from "../../i18n";
import { findEnumOptionIndex, isOptionalBooleanOverride } from "./guided-setting-values";
import { StringListEditor } from "./StringListEditor";
import { WorkshopIdListEditor } from "./WorkshopIdListEditor";
import { useConfigurationFieldHelp } from "./ConfigurationFieldHelp";
import { ConfigurationFieldResourceLink } from "./ConfigurationFieldResourceLink";
import type { GuidedSettingsField, SettingsObject } from "./settings-schema";

export interface ConfigurationSpecializedRendererProps {
  descriptionId?: string;
  disabled?: boolean;
  errorId?: string;
  field: GuidedSettingsField;
  inputId: string;
  onPatch: (patch: SettingsObject) => void;
  settings: SettingsObject;
  t?: TranslateFn;
  validationMessage?: string;
  value: unknown;
}

export interface ConfigurationFieldCopy {
  concealSecret: string;
  revealSecret: string;
  restartScopes: Readonly<Record<"none" | "server" | "world" | "cluster", string>>;
  showSuggestions?: string;
  specializedUnavailable?: string;
  notSaved?: string;
  useGameDefault?: string;
  preserveNativeWhenUnset?: string;
  enabled?: string;
  disabled?: string;
}

export interface ConfigurationFieldProps {
  className?: string;
  copy: ConfigurationFieldCopy;
  disabled?: boolean;
  readOnly?: boolean;
  field: GuidedSettingsField;
  idPrefix?: string;
  onPatch: (patch: SettingsObject) => void;
  renderSpecialized?: ComponentType<ConfigurationSpecializedRendererProps>;
  settings: SettingsObject;
  t?: TranslateFn;
  validationMessage?: string;
  value: unknown;
}

export interface ConfigurationFieldIds {
  descriptionId: string;
  errorId: string;
  inputId: string;
}

interface AccessibleControlProps {
  "aria-labelledby"?: string;
  "aria-describedby"?: string;
  "aria-errormessage"?: string;
  "aria-invalid"?: true;
  id: string;
}

function sanitizeIdSegment(value: string): string {
  return value
    .normalize("NFKD")
    .replace(/[^a-z0-9]+/gi, "-")
    .replace(/^-+|-+$/g, "")
    .toLowerCase() || "field";
}

export function buildConfigurationFieldIds(
  fieldKey: string,
  idPrefix = "configuration",
  exactKey = false
): ConfigurationFieldIds {
  const key = exactKey ? Array.from({ length: fieldKey.length }, (_, index) =>
    fieldKey.charCodeAt(index).toString(16).padStart(4, "0")).join("") : sanitizeIdSegment(fieldKey);
  const stem = `${sanitizeIdSegment(idPrefix)}-${key}`;
  return {
    descriptionId: `${stem}-description`,
    errorId: `${stem}-error`,
    inputId: `${stem}-input`
  };
}

function ConfigurationFieldRestartScope(props: {
  copy: ConfigurationFieldCopy;
  field: GuidedSettingsField;
}) {
  const restartScope = props.field.presentation.restartScope;
  if (!restartScope) return null;
  return (
    <span className="configuration-field-restart-scope" data-restart-scope={restartScope}>
      {props.copy.restartScopes[restartScope]}
    </span>
  );
}

function parseDelimitedList(value: unknown): string[] {
  if (typeof value !== "string" || value.trim().length === 0) return [];
  return [...new Set(value.replace(/\r\n?/g, "\n").split(/[\n,]+/).map((item) => item.trim()).filter(Boolean))];
}

function textInputAttributes(field: GuidedSettingsField) {
  const isNumber = field.control === "number";
  return {
    // Number inputs normalize drafts such as `0.` before React receives them.
    inputMode: isNumber ? field.type === "integer" ? "numeric" as const : "decimal" as const : undefined,
    maxLength: isNumber ? undefined : field.maxLength,
    minLength: isNumber ? undefined : field.minLength,
    pattern: isNumber || field.control === "textarea" ? undefined : field.pattern
  };
}

interface SuggestionInputProps {
  accessibility: AccessibleControlProps;
  disabled?: boolean;
  field: GuidedSettingsField;
  onChange: (value: unknown) => void;
  toggleLabel: string;
  value: unknown;
}

function SuggestionInput(props: SuggestionInputProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(-1);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const inputValue = String(props.value ?? "");
  const suggestions = props.field.suggestions ?? [];
  const constraints = textInputAttributes(props.field);
  const listboxId = `${props.accessibility.id}-suggestions`;

  function openSuggestions() {
    setIsOpen(true);
  }

  function closeSuggestions() {
    setIsOpen(false);
    setActiveIndex(-1);
  }

  function selectSuggestion(index: number) {
    const option = suggestions[index];
    if (!option) return;
    props.onChange(option.value);
    closeSuggestions();
  }

  useEffect(() => {
    if (!isOpen) return;
    function closeOnOutsidePointer(event: PointerEvent) {
      if (event.target instanceof Node && !rootRef.current?.contains(event.target)) {
        setIsOpen(false);
        setActiveIndex(-1);
      }
    }
    document.addEventListener("pointerdown", closeOnOutsidePointer);
    return () => document.removeEventListener("pointerdown", closeOnOutsidePointer);
  }, [isOpen]);

  return (
    <div className="settings-suggestion-combobox" ref={rootRef}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) closeSuggestions();
      }}>
      <input
        {...props.accessibility}
        className="settings-schema-input"
        type="text"
        value={inputValue}
        {...constraints}
        disabled={props.disabled}
        role="combobox"
        aria-autocomplete="list"
        aria-controls={listboxId}
        aria-activedescendant={isOpen && activeIndex >= 0 ? `${listboxId}-option-${activeIndex}` : undefined}
        aria-haspopup="listbox"
        aria-expanded={isOpen}
        onFocus={() => openSuggestions()}
        onClick={() => openSuggestions()}
        onKeyDown={(event) => {
          if (event.nativeEvent.isComposing) return;
          if (event.key === "Escape") {
            closeSuggestions();
            return;
          }
          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            event.preventDefault();
            const direction = event.key === "ArrowDown" ? 1 : -1;
            openSuggestions();
            setActiveIndex((current) => suggestions.length === 0 ? -1
              : current < 0 ? direction > 0 ? 0 : suggestions.length - 1
              : (current + direction + suggestions.length) % suggestions.length);
            return;
          }
          if (isOpen && event.key === "Home") {
            event.preventDefault();
            setActiveIndex(suggestions.length > 0 ? 0 : -1);
          } else if (isOpen && event.key === "End") {
            event.preventDefault();
            setActiveIndex(suggestions.length - 1);
          } else if (isOpen && event.key === "Enter") {
            event.preventDefault();
            if (activeIndex >= 0) selectSuggestion(activeIndex);
            else closeSuggestions();
          }
        }}
        onChange={(event) => {
          props.onChange(event.target.value);
          setActiveIndex(-1);
          setIsOpen(true);
        }}
      />
      <button
        type="button"
        className="settings-suggestion-toggle"
        disabled={props.disabled}
        aria-label={props.toggleLabel}
        aria-controls={listboxId}
        aria-expanded={isOpen}
        onClick={() => {
          if (isOpen) {
            closeSuggestions();
          } else {
            openSuggestions();
          }
        }}
      />
      {isOpen && suggestions.length > 0 ? (
        <div id={listboxId} className="settings-suggestion-menu" role="listbox">
          {suggestions.map((option, index) => {
            const optionValue = String(option.value);
            return (
              <button
                type="button"
                id={`${listboxId}-option-${index}`}
                key={`${props.field.key}-suggestion-${index}`}
                className={`settings-suggestion-option${optionValue === inputValue ? " is-selected" : ""}${index === activeIndex ? " is-active" : ""}`}
                role="option"
                aria-selected={optionValue === inputValue}
                tabIndex={-1}
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => selectSuggestion(index)}
              >
                <span className="settings-suggestion-option-label">{option.label}</span>
                <span className="settings-suggestion-option-value">{optionValue}</span>
              </button>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}

export function ConfigurationField(props: ConfigurationFieldProps) {
  const [secretVisible, setSecretVisible] = useState(false);
  const ids = buildConfigurationFieldIds(props.field.key, props.idPrefix, props.readOnly);
  const behavior = props.field.presentation.behavior ?? "plain";
  const helpMode = (behavior === "multiline" || behavior === "raw") &&
    !props.field.editorVariant && props.field.presentation.state === "editable"
    ? "instructions"
    : "summary";
  const help = useConfigurationFieldHelp(
    ids.descriptionId, props.field.description, props.field.title, props.t, helpMode
  );
  const isInvalid = Boolean(props.validationMessage);
  const accessibility: AccessibleControlProps = {
    id: ids.inputId,
    "aria-labelledby": props.field.presentation.resourceUrl ? `${ids.inputId}-label` : undefined,
    "aria-describedby": help.descriptionId,
    "aria-invalid": isInvalid ? true : undefined,
    "aria-errormessage": isInvalid ? ids.errorId : undefined
  };
  const patchValue = (value: unknown) => {
    if (!props.readOnly) props.onPatch({ [props.field.key]: value });
  };
  const constraints = textInputAttributes(props.field);
  const SpecializedRenderer = props.readOnly ? undefined : props.renderSpecialized;
  const disabled = props.disabled || props.readOnly;
  const usesSecretInput = behavior === "secret" &&
    props.field.type === "string" &&
    !props.field.editorVariant &&
    (props.field.control === "text" ||
      props.field.control === "password" ||
      props.field.control === "textarea");

  let control;
  const state = props.field.presentation.state;
  const optionalBooleanOverride = isOptionalBooleanOverride(props.field);
  const nonInteractive = state !== "editable" && state !== "specialized";
  const specializedUnavailable = state === "specialized" && !props.field.editorVariant && !SpecializedRenderer;
  const savedRaw = props.readOnly && (props.value === undefined || props.value === null ||
    typeof props.value === "object" || props.field.editorVariant || nonInteractive || specializedUnavailable ||
    (props.field.control === "select" && findEnumOptionIndex(props.field, props.value) < 0) ||
    (props.field.control === "checkbox" && typeof props.value !== "boolean"));
  if (savedRaw) {
    const value = props.value === undefined ? props.copy.notSaved ?? "Not saved"
      : typeof props.value === "string" ? props.value : JSON.stringify(props.value, null, 2);
    control = <textarea {...accessibility} className="settings-schema-input settings-schema-textarea"
      rows={4} readOnly value={value} />;
  } else if (nonInteractive || specializedUnavailable) {
    control = <div {...accessibility} className="configuration-field-unavailable" role="status"
      aria-labelledby={`${ids.inputId}-label`}>
      {props.copy.specializedUnavailable ?? "This setting is managed elsewhere."}
    </div>;
  } else if (state === "specialized" && SpecializedRenderer) {
    control = <SpecializedRenderer field={props.field} value={props.value}
      settings={props.settings} disabled={disabled} validationMessage={props.validationMessage}
      onPatch={props.onPatch} inputId={ids.inputId} descriptionId={help.descriptionId}
      errorId={isInvalid ? ids.errorId : undefined} />;
  } else if (props.field.editorVariant === "workshop-id-list") {
    control = <WorkshopIdListEditor field={props.field} value={props.value} disabled={disabled}
      inputId={ids.inputId} ariaDescribedBy={accessibility["aria-describedby"]}
      ariaErrorMessage={accessibility["aria-errormessage"]} ariaInvalid={accessibility["aria-invalid"]}
      onChange={patchValue} />;
  } else if (props.field.editorVariant === "string-list") {
    control = <StringListEditor field={props.field} value={props.value} disabled={disabled}
      inputId={ids.inputId} ariaDescribedBy={accessibility["aria-describedby"]}
      ariaErrorMessage={accessibility["aria-errormessage"]} ariaInvalid={accessibility["aria-invalid"]}
      onChange={patchValue} />;
  } else if (props.field.editorVariant === "enum-check-list") {
    const selected = new Set(parseDelimitedList(props.value));
    const options = props.field.enumOptions ?? props.field.suggestions ?? [];
    control = <div className="settings-enum-checklist" {...accessibility} role="group"
      aria-labelledby={`${ids.inputId}-label`}>
      {options.map((option) => {
        const optionValue = String(option.value);
        return <label key={`${props.field.key}:${optionValue}`} className="settings-enum-checklist-option">
          <input type="checkbox" checked={selected.has(optionValue)} disabled={disabled}
            onChange={(event) => patchValue(options.map((candidate) => String(candidate.value)).filter((candidate) =>
              candidate === optionValue ? event.target.checked : selected.has(candidate)).join("\n"))} />
          <span>{option.label}</span>
        </label>;
      })}
    </div>;
  } else if (props.field.control === "checkbox" && optionalBooleanOverride) {
    control = <select {...accessibility} className="settings-schema-input settings-schema-select"
      value={typeof props.settings[props.field.key] === "boolean" ? String(props.settings[props.field.key]) : ""}
      disabled={disabled}
      onChange={(event) => patchValue(event.target.value === "" ? undefined : event.target.value === "true")}>
      <option value="">{props.field.preserveNativeWhenUnset
        ? props.copy.preserveNativeWhenUnset ?? "Keep native setting (current value not read)"
        : props.copy.useGameDefault ?? "Use game default"}</option>
      <option value="true">{props.copy.enabled ?? "Enabled"}</option>
      <option value="false">{props.copy.disabled ?? "Disabled"}</option>
    </select>;
  } else if (props.field.control === "checkbox") {
    control = <input {...accessibility} type="checkbox" checked={Boolean(props.value)}
      disabled={disabled} onChange={(event) => patchValue(event.target.checked)} />;
  } else if (props.field.control === "select") {
    const optionIndex = findEnumOptionIndex(props.field, props.value);
    control = <select {...accessibility} className="settings-schema-input settings-schema-select"
      value={props.field.preserveNativeWhenUnset && optionIndex < 0 ? "" : String(Math.max(optionIndex, 0))}
      disabled={disabled}
      onChange={(event) => {
        if (props.field.preserveNativeWhenUnset && event.target.value === "") {
          patchValue(undefined);
          return;
        }
        const option = props.field.enumOptions?.[Number(event.target.value)];
        if (option) patchValue(option.value);
      }}>
      {props.field.preserveNativeWhenUnset ? <option value="">
        {props.copy.preserveNativeWhenUnset ?? "Keep native setting (current value not read)"}
      </option> : null}
      {props.field.enumOptions?.map((option, index) => <option key={`${props.field.key}-${index}`} value={index}>{option.label}</option>)}
    </select>;
  } else if (usesSecretInput) {
    control = <input {...accessibility} className="settings-schema-input"
      type={secretVisible ? "text" : "password"} value={String(props.value ?? "")}
      {...constraints} readOnly={props.readOnly} disabled={props.disabled && !props.readOnly}
      onChange={(event) => patchValue(event.target.value)} />;
  } else if (behavior === "multiline" || behavior === "raw") {
    control = <textarea {...accessibility} className="settings-schema-input settings-schema-textarea" rows={4}
      data-configuration-behavior={behavior} value={String(props.value ?? "")} minLength={constraints.minLength}
      maxLength={constraints.maxLength} readOnly={props.readOnly} disabled={props.disabled && !props.readOnly}
      onChange={(event) => patchValue(event.target.value)} />;
  } else if (!props.readOnly && props.field.suggestions?.length) {
    control = <SuggestionInput accessibility={accessibility} field={props.field} value={props.value}
      disabled={disabled} toggleLabel={props.copy.showSuggestions ?? "Show suggestions"} onChange={patchValue} />;
  } else {
    control = <input {...accessibility} className="settings-schema-input"
      type="text" value={String(props.value ?? "")} {...constraints}
      readOnly={props.readOnly} disabled={props.disabled && !props.readOnly}
      onChange={(event) => patchValue(event.target.value)} />;
  }

  const usesGroupLabel = !savedRaw && (props.field.editorVariant === "enum-check-list" || nonInteractive || specializedUnavailable);
  const usesToggleCard = props.field.control === "checkbox" && !optionalBooleanOverride &&
    !nonInteractive && !specializedUnavailable && !savedRaw;
  const rootClassName = `configuration-field settings-schema-field${props.className ? ` ${props.className}` : ""}${isInvalid ? " is-invalid" : ""}`;
  return (
    <div className={rootClassName} ref={help.anchorRef} {...help.interactionProps}
      tabIndex={props.readOnly ? -1 : undefined}
      data-configuration-behavior={behavior} data-field-key={props.field.key}>
      {props.field.icon && !usesToggleCard ? <img className="settings-field-icon" src={props.field.icon} alt="" aria-hidden="true" loading="lazy" /> : null}
      {!usesToggleCard ? <div className="configuration-field-heading">
        {props.field.presentation.resourceUrl ? (
          <ConfigurationFieldResourceLink key={props.field.presentation.resourceUrl}
            id={`${ids.inputId}-label`} url={props.field.presentation.resourceUrl} t={props.t}>
            {props.field.title}{props.field.required ? " *" : ""}
          </ConfigurationFieldResourceLink>
        ) : usesGroupLabel ? (
          <span id={`${ids.inputId}-label`} className="detail-label settings-field-label">
            {props.field.title}{props.field.required ? " *" : ""}
          </span>
        ) : (
          <label className="detail-label settings-field-label" htmlFor={ids.inputId}>
            {props.field.title}{props.field.required ? " *" : ""}
          </label>
        )}
      </div> : null}
      {help.helpNode}
      {usesToggleCard ? (
        <label className="configuration-field-control settings-toggle-card" htmlFor={ids.inputId}>
          <span className="settings-toggle-copy"><span className="settings-field-title">
            {props.field.title}{props.field.required ? " *" : ""}
          </span></span>
          {control}
        </label>
      ) : <div className="configuration-field-control">
        {control}
        {usesSecretInput ? (
          <button type="button" className="configuration-secret-toggle" disabled={props.disabled && !props.readOnly}
            aria-controls={ids.inputId} aria-pressed={secretVisible}
            onClick={() => setSecretVisible((visible) => !visible)}>
            {secretVisible ? props.copy.concealSecret : props.copy.revealSecret}
          </button>
        ) : null}
      </div>}
      {props.field.presentation.restartScope || isInvalid ? <div className="configuration-field-feedback">
        <ConfigurationFieldRestartScope copy={props.copy} field={props.field} />
        {isInvalid ? <p id={ids.errorId} className="form-note form-note--error">{props.validationMessage}</p> : null}
      </div> : null}
    </div>
  );
}
