import { useMemo } from "react";
import { useI18n } from "../../i18n";
import { ConfigurationField } from "./ConfigurationField";
import { buildConfigurationFieldCopy } from "./GuidedSettingsForm";
import { parseGuidedSettingsSchema, readGuidedFieldValue, writeGuidedFieldValue } from "./guided-settings";
import type { ConfigurationSpecializedRendererProps } from "./module-types";
import { useSatisfactoryWorldSettings } from "./SatisfactoryWorldSettingsContext";
import type { GuidedSettingsField } from "./settings-schema";

const NATIVE_KEYS: Readonly<Record<string, string>> = {
  auto_pause_when_empty: "FG.DSAutoPause", network_quality: "FG.NetworkQuality",
  weather_preset: "FG.WeatherPreset", send_gameplay_data: "FG.SendGameplayData"
};

export function readSatisfactoryNativeOption(field: GuidedSettingsField, raw: string | undefined): unknown {
  if (raw === undefined || !raw.trim()) return undefined;
  if (field.type === "boolean") {
    if (["true", "1"].includes(raw.toLowerCase())) return true;
    if (["false", "0"].includes(raw.toLowerCase())) return false;
    return undefined;
  }
  return field.enumOptions?.find((option) => String(option.value) === raw)?.value;
}

/** The existing INI control remains the single editor; API values are read-only context. */
export function SatisfactoryNativeOptionField(props: ConfigurationSpecializedRendererProps) {
  const { locale, t } = useI18n();
  const { snapshot } = useSatisfactoryWorldSettings();
  const schema = useMemo(() => parseGuidedSettingsSchema(props.moduleDetails, locale, t), [locale, props.moduleDetails, t]);
  const field = schema.fields.find((entry) => entry.key === props.fieldKey);
  if (!field) return <p className="form-note form-note--error" role="alert">{t("satisfactory.settings.native.fieldUnavailable")}</p>;
  const native = snapshot?.server_options[NATIVE_KEYS[field.key] ?? ""];
  const pending = snapshot?.pending_server_options[NATIVE_KEYS[field.key] ?? ""];
  const actual = readSatisfactoryNativeOption(field, native);
  const hasOverride = Object.prototype.hasOwnProperty.call(props.settings, field.key) && props.settings[field.key] !== undefined;
  const value = hasOverride ? readGuidedFieldValue(field, props.settings) : actual;
  let label: string | null = null;
  if (native !== undefined && native.trim() !== "") {
    if (field.type === "boolean" && ["True", "False", "1", "0"].includes(native)) {
      label = t(native === "True" || native === "1" ? "common.enabled" : "common.disabled");
    } else {
      label = field.enumOptions?.find((option) => String(option.value) === native)?.label ?? native;
    }
  }
  const copy = buildConfigurationFieldCopy(t);
  copy.preserveNativeWhenUnset = native?.trim() || t("satisfactory.settings.native.currentValueUnavailable");
  const resolved = value !== undefined && (field.type === "boolean" ? typeof value === "boolean" :
    field.enumOptions?.some((option) => option.value === value));
  return <div className="settings-editor-stack">
    <div className="settings-schema-grid configuration-field-grid">
      <ConfigurationField field={{ ...field, preserveNativeWhenUnset: !resolved,
        defaultValue: field.type === "boolean" ? false : field.defaultValue,
        presentation: { ...field.presentation, state: "editable", rendererId: undefined } }}
        value={value} settings={props.settings} copy={copy} t={t}
        idPrefix="configuration-satisfactory" disabled={props.disabled}
        onPatch={(patch) => {
          const written = writeGuidedFieldValue(props.settings, field, patch[field.key]);
          props.onPatch({ [field.key]: written[field.key] });
        }} />
    </div>
    {label !== null && (hasOverride && value !== actual || pending && pending !== native) ?
      <p className="form-note" role="status">{t("satisfactory.settings.native.currentValue", { value: label })}
        {pending && pending !== native ? ` ${t("satisfactory.settings.native.pendingValue")}` : ""}</p> : null}
  </div>;
}
