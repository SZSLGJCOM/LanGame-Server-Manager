import type { ReactNode } from "react";
import { ActivityNotice } from "../../components/ActivityNotice";
import { useI18n } from "../../i18n";
import { satisfactoryRuleCopyKey, satisfactoryRuleId, type SatisfactoryRuleDefinition } from "../../satisfactory-world-settings";
import { buildConfigurationFieldIds, ConfigurationField } from "./ConfigurationField";
import { ConfigurationValidationNotice } from "./ConfigurationValidationNotice";
import { buildConfigurationFieldCopy } from "./GuidedSettingsForm";
import type { GuidedEnumOption, GuidedSectionId, GuidedSettingsField } from "./settings-schema";
import { useSatisfactoryWorldSettings } from "./SatisfactoryWorldSettingsContext";

export const SATISFACTORY_NATIVE_COPY = "satisfactory.settings.native";

export function SatisfactoryNativeField(props: {
  fieldKey: string; title: string; description?: string; sectionId: GuidedSectionId;
  kind?: "text" | "secret" | "boolean" | "integer" | "select";
  value: string | boolean; onChange(value: string | boolean): void;
  options?: GuidedEnumOption[]; disabled?: boolean; readOnly?: boolean;
  minimum?: number | null; maximum?: number | null; validation?: string;
}) {
  const { t } = useI18n();
  const kind = props.kind ?? "text";
  const field: GuidedSettingsField = {
    key: props.fieldKey, title: props.title, description: props.description, sectionId: props.sectionId,
    type: kind === "boolean" ? "boolean" : kind === "integer" ? "integer" : "string",
    control: kind === "boolean" ? "checkbox" : kind === "select" ? "select" : kind === "integer" ? "number" : "text",
    required: false, defaultValue: kind === "boolean" ? false : undefined,
    enumOptions: props.options, minimum: props.minimum ?? undefined, maximum: props.maximum ?? undefined,
    presentation: { state: "editable", owner: "configuration", sectionId: props.sectionId,
      behavior: kind === "secret" ? "secret" : "plain" }
  };
  return <>
    <ConfigurationValidationNotice message={props.validation} label={props.title}
      inputId={buildConfigurationFieldIds(field.key, "configuration-satisfactory", props.readOnly).inputId} />
    <ConfigurationField field={field} value={props.value} settings={{ [field.key]: props.value }} t={t}
      indexed={false}
      copy={buildConfigurationFieldCopy(t)} idPrefix="configuration-satisfactory" disabled={props.disabled}
      readOnly={props.readOnly} validationMessage={props.validation} validationDisplay="activity"
      onPatch={(patch) => {
        const value = patch[field.key];
        if (!props.disabled && !props.readOnly && (typeof value === "string" || typeof value === "boolean")) props.onChange(value);
      }} />
  </>;
}

export function SatisfactoryRuleField(props: {
  rule: SatisfactoryRuleDefinition; value: string; sectionId: string; disabled: boolean;
  onChange(value: string): void; validation?: string;
}) {
  const { t } = useI18n();
  const copy = `${SATISFACTORY_NATIVE_COPY}.rules.${satisfactoryRuleCopyKey(props.rule.key)}`;
  return <SatisfactoryNativeField fieldKey={satisfactoryRuleId(props.rule.key)} sectionId={props.sectionId}
    title={t(`${copy}.title`)} description={t(`${copy}.description`)} kind={props.rule.kind}
    minimum={props.rule.minimum} maximum={props.rule.maximum} validation={props.validation}
    disabled={props.disabled} value={props.rule.kind === "boolean" ? props.value === "True" : props.value}
    options={props.rule.options.map((option) => ({ value: option.value, label: t(`${SATISFACTORY_NATIVE_COPY}.options.${option.label_key}`) }))}
    onChange={(value) => props.onChange(typeof value === "boolean" ? value ? "True" : "False" : value)} />;
}

export function SatisfactoryNativePanel(props: {
  sectionId: string; children?: ReactNode; dirty?: boolean; onNavigateField?: (fieldKey: string) => void;
}) {
  const { t } = useI18n();
  const state = useSatisfactoryWorldSettings();
  return <section className="settings-editor-stack" data-satisfactory-native-panel={props.sectionId}
    id={`configuration-satisfactory-native-${props.sectionId.replace(/_/gu, "-")}-input`}
    data-field-key={`native_${props.sectionId}`} tabIndex={-1} aria-label={t(`${SATISFACTORY_NATIVE_COPY}.panels.${props.sectionId}`)}
    aria-busy={state.loading || state.busy}>
    <div className="panel-head panel-head--compact panel-head--spread">
      <h4 className="settings-section-title settings-field-label">{t(`${SATISFACTORY_NATIVE_COPY}.panels.${props.sectionId}`)}</h4>
      <button type="button" className="secondary-button" disabled={state.loading || state.busy}
        onClick={() => void state.refresh()}>{t(`${SATISFACTORY_NATIVE_COPY}.refresh`)}</button>
    </div>
    {state.loading ? <ActivityNotice>{t(`${SATISFACTORY_NATIVE_COPY}.loading`)}</ActivityNotice> : null}
    {state.error ? <ActivityNotice tone="error"
      details={props.dirty ? t(`${SATISFACTORY_NATIVE_COPY}.draftRetained`) : undefined}
      action={<button type="button" className="secondary-button" disabled={state.loading || state.busy}
        onClick={() => void state.refresh()}>{t(`${SATISFACTORY_NATIVE_COPY}.refresh`)}</button>}>
      {state.error}
    </ActivityNotice> : null}
    {state.snapshot?.connection_status === "stopped" ? <ActivityNotice>
      {t(`${SATISFACTORY_NATIVE_COPY}.startRequired`)}
    </ActivityNotice> : null}
    {state.snapshot?.connection_status === "authorization_required" && props.sectionId !== "access" ?
      <ActivityNotice action={props.onNavigateField ? <button type="button"
        className="secondary-button configuration-validation-target" data-satisfactory-navigation-field="native_access"
        aria-controls="configuration-satisfactory-native-access-input" onClick={() => props.onNavigateField?.("native_access")}>
        {t(`${SATISFACTORY_NATIVE_COPY}.panels.access`)}
      </button> : undefined}>{t(`${SATISFACTORY_NATIVE_COPY}.authorizationRequired`)}</ActivityNotice> : null}
    {state.snapshot?.connection_status === "unclaimed" && props.sectionId !== "room" ?
      <ActivityNotice action={props.onNavigateField ? <button type="button"
        className="secondary-button configuration-validation-target" data-satisfactory-navigation-field="native_room"
        aria-controls="configuration-satisfactory-native-room-input" onClick={() => props.onNavigateField?.("native_room")}>
        {t(`${SATISFACTORY_NATIVE_COPY}.panels.room`)}
      </button> : undefined}>{t(`${SATISFACTORY_NATIVE_COPY}.setupRequired`)}</ActivityNotice> : null}
    {props.children}
  </section>;
}
