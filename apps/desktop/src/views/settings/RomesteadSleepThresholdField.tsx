import { useEffect, useMemo, useRef } from "react";
import { useI18n } from "../../i18n";
import {
  ConfigurationField,
  type ConfigurationSpecializedRendererProps as FieldRendererProps
} from "./ConfigurationField";
import { buildConfigurationFieldCopy } from "./GuidedSettingsForm";
import { parseGuidedSettingsSchema, readGuidedFieldValue, validateGuidedSettingsObject, writeGuidedFieldValue } from "./guided-settings";
import { resolveSettingsModuleDefinition } from "./module-registry";
import type { ConfigurationSpecializedRendererProps } from "./module-types";

function SleepThresholdControl(props: FieldRendererProps) {
  const { t } = useI18n();
  const enabled = Number(props.value) !== -1;
  const rememberedThreshold = useRef<unknown>(props.validationMessage ? 10 : enabled ? props.value : 10);
  useEffect(() => {
    if (enabled && !props.validationMessage) rememberedThreshold.current = props.value;
  }, [enabled, props.validationMessage, props.value]);
  const threshold = enabled ? props.value : rememberedThreshold.current;
  const thresholdId = `${props.inputId}-threshold`;
  const thresholdTitle = t("romestead.settings.sleep.threshold", undefined, "Sleep threshold (ms)");
  const thresholdDisabled = props.disabled || !enabled;
  const accessibility = {
    "aria-describedby": props.descriptionId,
    "aria-invalid": enabled && props.validationMessage ? true as const : undefined,
    "aria-errormessage": enabled ? props.errorId : undefined
  };
  function patchThreshold(raw: unknown) {
    const normalized = writeGuidedFieldValue(props.settings, props.field, raw);
    props.onPatch({ [props.field.key]: normalized[props.field.key] });
  }

  return <div className="settings-editor-stack">
    <label className="settings-toggle-card" htmlFor={props.inputId}>
      <span className="settings-toggle-copy">
        {t(enabled ? "common.enabled" : "common.disabled", undefined, enabled ? "Enabled" : "Disabled")}
      </span>
      <input id={props.inputId} type="checkbox" checked={enabled} disabled={props.disabled}
        aria-describedby={props.descriptionId} onChange={(event) => {
          if (props.disabled) return;
          if (event.target.checked) {
            patchThreshold(rememberedThreshold.current);
          } else {
            if (!props.validationMessage) rememberedThreshold.current = props.value;
            patchThreshold(-1);
          }
        }} />
    </label>
    <label className="detail-label settings-field-label" htmlFor={thresholdId}>{thresholdTitle}</label>
    <input id={thresholdId} className="settings-schema-input" type="text" inputMode="decimal"
      value={String(threshold ?? "")} disabled={thresholdDisabled} {...accessibility}
      onChange={(event) => {
        if (!thresholdDisabled) patchThreshold(event.target.value);
      }} />
    {!props.validationMessage ? <input type="range" min={1} max={16.5} step="any"
      value={Number(threshold)} disabled={thresholdDisabled} aria-label={thresholdTitle}
      aria-describedby={props.descriptionId}
      style={{ width: "100%", margin: 0, minWidth: 0, accentColor: "var(--accent)" }}
      onChange={(event) => {
        if (!thresholdDisabled) patchThreshold(event.target.value);
      }} /> : null}
  </div>;
}

export function RomesteadSleepThresholdField(props: ConfigurationSpecializedRendererProps) {
  const { locale, t } = useI18n();
  const schema = useMemo(() => parseGuidedSettingsSchema(props.moduleDetails, locale, t),
    [locale, props.moduleDetails, t]);
  const field = schema.fields.find((candidate) => candidate.key === "sleep_threshold_ms");
  if (!field) return null;
  const value = readGuidedFieldValue(field, props.settings);
  const validationMessage = validateGuidedSettingsObject(schema, props.settings, undefined, t)
    .find((issue) => issue.fieldKey === field.key)?.message
    ?? resolveSettingsModuleDefinition("romestead")?.getFieldValidationMessage?.({
      field, value, settings: props.settings, locale, t
    });
  return <div className="settings-schema-grid configuration-field-grid">
    <ConfigurationField key={props.details.summary.id} field={field} value={value} settings={props.settings}
      copy={buildConfigurationFieldCopy(t)} disabled={props.disabled} onPatch={props.onPatch}
      idPrefix="configuration-romestead" renderSpecialized={SleepThresholdControl}
      validationMessage={validationMessage} t={t} />
  </div>;
}
