import { useRef } from "react";
import type { TranslateFn } from "../../i18n";
import { ConfigurationField } from "../settings/ConfigurationField";
import { buildConfigurationFieldCopy } from "../settings/GuidedSettingsForm";
import { readGuidedFieldValue } from "../settings/guided-settings";
import { NATIVE_SAVE_POLICIES } from "../settings/save-policy";
import type { GuidedSettingsField, GuidedSettingsValidationIssue, SettingsObject } from "../settings/settings-schema";

interface NativeSavePolicyFieldsProps {
  instanceId: string;
  moduleId: string;
  fields: GuidedSettingsField[];
  settings: SettingsObject;
  issues: GuidedSettingsValidationIssue[];
  disabled: boolean;
  readOnly?: boolean;
  t: TranslateFn;
  onChange(field: GuidedSettingsField, value: unknown): void;
}

export function NativeSavePolicyFields(props: NativeSavePolicyFieldsProps) {
  const policy = NATIVE_SAVE_POLICIES[props.moduleId];
  const copy = buildConfigurationFieldCopy(props.t);
  const managedField = props.fields.find((field) => field.key === "managed_save_interval_seconds");
  const managedValue = managedField ? props.readOnly ? props.settings[managedField.key]
    : readGuidedFieldValue(managedField, props.settings) : undefined;
  const rememberedInterval = useRef({ instanceId: props.instanceId, value: 300 });
  if (rememberedInterval.current.instanceId !== props.instanceId) {
    rememberedInterval.current = { instanceId: props.instanceId, value: 300 };
  }
  if (typeof managedValue === "number" && Number.isInteger(managedValue) && managedValue > 0
    && managedValue <= (managedField?.maximum ?? 86_400)) rememberedInterval.current.value = managedValue;
  const managedEnabled = props.readOnly ? managedValue !== undefined && managedValue !== null && managedValue !== 0 : managedValue !== 0;
  const frequency = props.t(`servers.savePolicy.frequency.${props.moduleId}`, undefined,
    props.t("servers.savePolicy.frequency.gameManaged"));
  const groups = [
    { key: "autosave", keys: policy?.autosave ?? [], title: props.t("servers.savePolicy.autosave") },
    { key: "backups", keys: policy?.backups ?? [], title: props.t("servers.savePolicy.nativeBackups") }
  ];
  return <>
    {groups.map((group) => {
      const fields = props.fields.filter((field) => group.keys.includes(field.key));
      if (fields.length === 0 && group.key !== "autosave") return null;
      return <fieldset className="server-save-policy-group" key={group.key} disabled={props.disabled}>
        <legend>{group.title}</legend>
        {group.key === "autosave" && fields.length === 0 && (group.keys.length > 0
          ? <p className="form-note" role="status">{props.t("servers.savePolicy.schemaUnavailable")}</p>
          : <>
            <dl className="server-autosave-mechanism">
              <div><dt>{props.t("servers.savePolicy.control")}</dt>
                <dd>{props.t(props.moduleId === "squad" ? "servers.savePolicy.notApplicable" : "servers.savePolicy.gameControlled")}</dd></div>
              <div><dt>{props.t("servers.savePolicy.frequency")}</dt><dd>{frequency}</dd></div>
            </dl>
            <p className="form-note">{props.t(`servers.savePolicy.mechanism.${props.moduleId}`,
              undefined, props.t("servers.savePolicy.gameManaged"))}</p>
          </>)}
        <div className="server-save-policy-fields">
          {group.key === "autosave" && managedField && (!props.readOnly || managedValue !== undefined && managedValue !== null) && <label className="settings-toggle-card server-backup-policy-toggle">
            <span className="settings-toggle-copy"><span className="settings-field-title">{props.t("servers.savePolicy.enabled")}</span></span>
            <input type="checkbox" checked={managedEnabled} disabled={props.disabled}
              onChange={(event) => {
                if (!props.readOnly && !props.disabled) props.onChange(managedField, event.target.checked ? rememberedInterval.current.value : 0);
              }} />
          </label>}
          {fields.map((field) => <ConfigurationField key={field.key} field={field} copy={copy}
            className={field.control === "textarea" ? "settings-schema-field--full" : undefined}
            disabled={props.disabled || (field === managedField && !managedEnabled)} readOnly={props.readOnly} idPrefix={`save-policy-${props.moduleId}`}
            settings={props.settings} value={props.readOnly ? props.settings[field.key] : field === managedField && !managedEnabled
              ? rememberedInterval.current.value : readGuidedFieldValue(field, props.settings)} t={props.t}
            validationMessage={props.issues.find((issue) => issue.fieldKey === field.key)?.message}
            onPatch={(patch) => {
              if (!props.readOnly && !props.disabled && Object.prototype.hasOwnProperty.call(patch, field.key)) props.onChange(field, patch[field.key]);
            }} />)}
        </div>
        {group.key === "autosave" && props.moduleId === "dontstarve" && <p className="form-note">
          {props.t("servers.savePolicy.frequency")}{": "}{frequency}
        </p>}
      </fieldset>;
    })}
  </>;
}
