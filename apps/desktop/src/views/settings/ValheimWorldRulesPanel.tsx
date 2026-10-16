import { useMemo, type ReactNode } from "react";
import { useI18n, type TranslateFn } from "../../i18n";
import { ActivityNotice } from "../../components/ActivityNotice";
import { buildConfigurationFieldIds } from "./ConfigurationField";
import { useConfigurationFieldHelp } from "./ConfigurationFieldHelp";
import type { ConfigurationSpecializedRendererProps } from "./module-types";
import {
  VALHEIM_MODIFIER_NAMES, parseValheimRuleEntries, readValheimModifier,
  readValheimWorldRuleContract, replaceValheimModifier, replaceValheimWorldKey,
  isValheimModifierEntry
} from "./valheim-world-rules";

function Field(props: {
  fieldKey: string; title: string; description: string; t: TranslateFn;
  children: (inputId: string, descriptionId?: string) => ReactNode;
}) {
  const ids = buildConfigurationFieldIds(props.fieldKey, "configuration-valheim-rules");
  const help = useConfigurationFieldHelp(ids.descriptionId, props.description, props.title, props.t);
  return <div className="configuration-field settings-schema-field"
    data-field-key={props.fieldKey === "world_preset" ? props.fieldKey : undefined}
    data-valheim-rule={props.fieldKey === "world_preset" ? undefined : props.fieldKey}
    ref={help.anchorRef} {...help.interactionProps}>
    <div className="configuration-field-heading">
      <label className="detail-label settings-field-label" htmlFor={ids.inputId}>{props.title}</label>
    </div>
    {help.helpNode}
    <div className="configuration-field-control">{props.children(ids.inputId, help.descriptionId)}</div>
  </div>;
}

export function ValheimWorldRulesPanel(props: ConfigurationSpecializedRendererProps) {
  const { t } = useI18n();
  const contractResult = useMemo(() => {
    try {
      if (typeof props.moduleDetails.schema_json !== "string") throw new Error("Missing Valheim configuration schema");
      return { contract: readValheimWorldRuleContract(props.moduleDetails.schema_json), error: "" };
    } catch (error) {
      return { contract: null, error: error instanceof Error ? error.message : String(error) };
    }
  }, [props.moduleDetails.schema_json]);
  const contract = contractResult.contract;
  if (!contract) return <ActivityNotice tone="error">
    {`${t("valheim.settings.rules.unavailable", undefined, "World rule choices could not be loaded. Reload the configuration.")} ${contractResult.error}`}
  </ActivityNotice>;
  const malformed = (fieldKey: string) => props.settings[fieldKey] !== undefined && typeof props.settings[fieldKey] !== "string";
  const malformedModifiers = malformed("world_modifiers");
  const malformedKeys = malformed("world_set_keys");
  const modifierValue = malformedModifiers ? undefined : props.settings.world_modifiers;
  const keyValue = malformedKeys ? undefined : props.settings.world_set_keys;
  const modifiers = parseValheimRuleEntries(modifierValue);
  const keys = parseValheimRuleEntries(keyValue);
  const invalidModifiers = modifiers.filter((entry) => !isValheimModifierEntry(entry, contract));
  const invalidKeys = keys.filter((entry) => !contract.keys.some((key) => key === entry));
  const invalidPreset = typeof props.settings.world_preset === "string" && !contract.presets.includes(props.settings.world_preset);
  const show = (fieldKey: string) => !props.fieldKey || props.fieldKey === fieldKey;
  const preset = typeof props.settings.world_preset === "string" ? props.settings.world_preset : "";
  const repeatedModifiers = VALHEIM_MODIFIER_NAMES.some((name) =>
    modifiers.filter((entry) => entry.split(/\s+/)[0] === name).length > 1);

  function malformedNotice(fieldKey: string) {
    if (!malformed(fieldKey)) return null;
    return <ActivityNotice tone="error" action={<button type="button" className="ghost-button" disabled={props.disabled}
        onClick={() => props.onPatch({ [fieldKey]: "" })}>
        {t("valheim.settings.rules.clearInvalidStoredType", undefined, "Clear invalid setting")}
      </button>}>
      {t("valheim.settings.rules.invalidStoredType", undefined,
        "This saved setting has an invalid format. Its original value is retained; clear it to use these controls.")}
    </ActivityNotice>;
  }

  function repairNotice(fieldKey: "world_modifiers" | "world_set_keys", invalid: string[], valid: string[]) {
    if (invalid.length === 0) return null;
    return <ActivityNotice tone="warning" action={<button type="button" className="ghost-button" disabled={props.disabled}
        onClick={() => props.onPatch({ [fieldKey]: valid.join("\n") })}>
        {t("valheim.settings.rules.removeUnrecognized", undefined, "Remove unrecognized rules")}
      </button>}>
      {t("valheim.settings.rules.unrecognized", { entries: invalid.join(", ") },
        `Unrecognized saved rules are retained: ${invalid.join(", ")}.`)}
    </ActivityNotice>;
  }

  return <div className="guided-field-groups">
    {show("world_preset") ? <section className="guided-field-group" data-valheim-rule-group="preset">
      {malformedNotice("world_preset")}
      <div className="settings-schema-grid configuration-field-grid">
        <Field fieldKey="world_preset" title={t("settings.schema.valheim.world_preset.title", undefined, "World Preset")}
          description={t("settings.schema.valheim.world_preset.description", undefined, "Leave empty to preserve the current world rules.")} t={t}>
          {(inputId, descriptionId) => <select id={inputId} className="settings-schema-input settings-schema-select"
            value={preset} disabled={props.disabled || malformed("world_preset")} aria-describedby={descriptionId} aria-invalid={invalidPreset || malformed("world_preset") || undefined}
            onChange={(event) => props.onPatch({ world_preset: event.target.value })}>
            {invalidPreset ? <option value={preset} disabled>{t("valheim.settings.rules.unrecognizedChoice", undefined, "Unrecognized saved choice")}</option> : null}
            {contract.presets.map((value) => <option key={value} value={value}>
              {value ? t(`settings.schema.valheim.world_preset.option.${value}`, undefined, value)
                : t("valheim.settings.rules.keepSaved", undefined, "Preserve saved rules")}
            </option>)}
          </select>}
        </Field>
      </div>
    </section> : null}
    {show("world_modifiers") ? <section className="guided-field-group" data-valheim-rule-group="modifiers" data-field-key="world_modifiers">
      <div className="guided-field-group-head"><h5 className="guided-field-group-title">
        {t("valheim.settings.rules.modifiersTitle", undefined, "Difficulty & Gameplay")}
      </h5></div>
      {malformedNotice("world_modifiers")}
      {repairNotice("world_modifiers", invalidModifiers, modifiers.filter((entry) => isValheimModifierEntry(entry, contract)))}
      {repeatedModifiers ? <ActivityNotice tone="warning">{t("valheim.settings.rules.duplicates", undefined,
        "Some rules are set more than once. The last choice is shown; choosing a new value merges that rule into one entry.")}</ActivityNotice> : null}
      <div className="settings-schema-grid configuration-field-grid">
        {VALHEIM_MODIFIER_NAMES.map((name) => {
          const value = readValheimModifier(modifierValue, name);
          const invalid = value !== "" && !contract.modifiers[name].includes(value);
          return <Field key={name} fieldKey={`valheim_modifier_${name}`} t={t}
            title={t(`valheim.settings.rules.${name}.title`, undefined, name)}
            description={t(`valheim.settings.rules.${name}.description`, undefined, "Applies at server startup.")}>
            {(inputId, descriptionId) => <div className="settings-editor-stack"><select id={inputId} className="settings-schema-input settings-schema-select"
              value={value} disabled={props.disabled || malformedModifiers} aria-describedby={descriptionId} aria-invalid={invalid || malformedModifiers || undefined}
              onChange={(event) => props.onPatch({ world_modifiers: replaceValheimModifier(props.settings.world_modifiers, name, event.target.value) })}>
              <option value="">{t("valheim.settings.rules.inherit", undefined, "No startup override")}</option>
              {invalid ? <option value={value} disabled>{t("valheim.settings.rules.unrecognizedChoice", undefined, "Unrecognized saved choice")}</option> : null}
              {contract.modifiers[name].map((choice) => <option key={choice} value={choice}>
                {t(`valheim.settings.rules.${name}.option.${choice}`, undefined, choice)}
              </option>)}
            </select></div>}
          </Field>;
        })}
      </div>
    </section> : null}
    {show("world_set_keys") ? <section className="guided-field-group" data-valheim-rule-group="keys" data-field-key="world_set_keys">
      <div className="guided-field-group-head"><h5 className="guided-field-group-title">
        {t("valheim.settings.rules.keysTitle", undefined, "Additional World Rules")}
      </h5></div>
      {malformedNotice("world_set_keys")}
      {repairNotice("world_set_keys", invalidKeys, keys.filter((entry) => contract.keys.some((key) => key === entry)))}
      <div className="settings-schema-grid configuration-field-grid">
        {contract.keys.map((key) => {
          const explicit = keys.includes(key);
          const description = `${t(`valheim.settings.rules.${key}.description`, undefined, "Applies at server startup.")} ${t(
            "valheim.settings.rules.keyOverrideHelp", undefined,
            "Clearing this choice only removes the startup override; it does not turn off a rule already enabled in the saved world or preset.")}`;
          return <Field key={key} fieldKey={`valheim_key_${key}`} t={t}
          title={t(`valheim.settings.rules.${key}.title`, undefined, key)}
          description={description}>
          {(inputId, descriptionId) => <div className="settings-editor-stack"><label className="settings-toggle-card" htmlFor={inputId}>
            <span className="settings-toggle-copy"><span className="settings-field-title">{explicit
              ? t("valheim.settings.rules.enableStartup", undefined, "Enable at startup")
              : t("valheim.settings.rules.inherit", undefined, "No startup override")}</span></span>
            <input id={inputId} type="checkbox" checked={explicit}
              disabled={props.disabled || malformedKeys}
              aria-describedby={descriptionId}
              onChange={(event) => props.onPatch({ world_set_keys: replaceValheimWorldKey(props.settings.world_set_keys, key, event.target.checked) })} />
          </label></div>}
        </Field>;
        })}
      </div>
    </section> : null}
  </div>;
}
