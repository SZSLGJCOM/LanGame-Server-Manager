import { useMemo, type ReactNode } from "react";
import { useI18n, type TranslateFn } from "../../i18n";
import { buildConfigurationFieldIds } from "./ConfigurationField";
import { useConfigurationFieldHelp } from "./ConfigurationFieldHelp";
import type { ConfigurationSpecializedRendererProps } from "./module-types";
import { useValheimWorldRules } from "./ValheimWorldRulesContext";
import {
  VALHEIM_MODIFIER_NAMES, parseValheimRuleEntries, readValheimModifier,
  readValheimWorldRuleContract, replaceValheimModifier, replaceValheimWorldKey,
  isValheimModifierEntry, valheimPresetKeys, readSavedValheimModifier, readSavedValheimPreset, resolveSavedValheimKeys
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
  const native = useValheimWorldRules();
  const contractResult = useMemo(() => {
    try {
      if (typeof props.moduleDetails.schema_json !== "string") throw new Error("Missing Valheim configuration schema");
      return { contract: readValheimWorldRuleContract(props.moduleDetails.schema_json), error: "" };
    } catch (error) {
      return { contract: null, error: error instanceof Error ? error.message : String(error) };
    }
  }, [props.moduleDetails.schema_json]);
  const contract = contractResult.contract;
  if (!contract) return <p className="form-note form-note--error" role="alert" title={contractResult.error}>
    {t("valheim.settings.rules.unavailable", undefined, "World rule choices could not be loaded. Reload the configuration.")}
  </p>;

  const unknown = t("valheim.settings.rules.notRead", undefined, "Not read");
  const savedKeys = native?.state.status === "ready" && native.state.world.source === "saved"
    ? resolveSavedValheimKeys(native.state.world.saved_keys) : null;
  const newWorld = native?.state.status === "ready" && native.state.world.source === "new_world";
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
  const baseKeys = preset && !invalidPreset ? valheimPresetKeys(preset) : newWorld ? [] : savedKeys;
  const savedPreset = savedKeys ? readSavedValheimPreset(savedKeys) : null;
  const presetLabel = (value: string | null) => value === null ? unknown : value === "custom"
    ? t("valheim.settings.rules.custom", undefined, "Custom")
    : t(`settings.schema.valheim.world_preset.option.${value}`, undefined, value);
  const modifierLabel = (name: string, value: string | null) => value === null ? unknown : value === "custom"
    ? t("valheim.settings.rules.custom", undefined, "Custom")
    : value === "normal" ? t("valheim.settings.rules.normal", undefined, "Normal")
    : t(`valheim.settings.rules.${name}.option.${value}`, undefined, value);
  const keyLabel = (value: boolean | null) => value === null ? unknown : value
    ? t("valheim.settings.rules.enabled", undefined, "On") : t("valheim.settings.rules.disabled", undefined, "Off");
  const pending = (current: string, next: string) => current !== next
    ? <p className="form-note">{t("valheim.settings.rules.pending", { current, next },
      `Current: ${current} · After restart: ${next}`)}</p> : null;
  const repeatedModifiers = VALHEIM_MODIFIER_NAMES.some((name) =>
    modifiers.filter((entry) => entry.split(/\s+/)[0] === name).length > 1);

  function malformedNotice(fieldKey: string) {
    if (!malformed(fieldKey)) return null;
    return <div className="configuration-workspace__notice" role="alert">
      <span>{t("valheim.settings.rules.invalidStoredType", undefined,
        "This saved setting has an invalid format. Its original value is retained; clear it to use these controls.")}</span>
      <button type="button" className="ghost-button" disabled={props.disabled}
        onClick={() => props.onPatch({ [fieldKey]: "" })}>
        {t("valheim.settings.rules.clearInvalidStoredType", undefined, "Clear invalid setting")}
      </button>
    </div>;
  }

  function repairNotice(fieldKey: "world_modifiers" | "world_set_keys", invalid: string[], valid: string[]) {
    if (invalid.length === 0) return null;
    return <div className="configuration-workspace__notice" role="alert">
      <span>{t("valheim.settings.rules.unrecognized", { entries: invalid.join(", ") },
        `Unrecognized saved rules are retained: ${invalid.join(", ")}.`)}</span>
      <button type="button" className="ghost-button" disabled={props.disabled}
        onClick={() => props.onPatch({ [fieldKey]: valid.join("\n") })}>
        {t("valheim.settings.rules.removeUnrecognized", undefined, "Remove unrecognized rules")}
      </button>
    </div>;
  }

  return <div className="guided-field-groups">
    {show("world_preset") ? <section className="guided-field-group" data-valheim-rule-group="preset">
      {native?.state.status === "loading" ? <p className="form-note" role="status">
        {t("valheim.settings.rules.loading", undefined, "Reading saved world rules…")}
      </p> : native?.state.status === "error" ? <div className="settings-editor-stack">
        <p className="form-note form-note--error" role="alert">{t("valheim.settings.rules.readError", undefined,
          "Saved world rules could not be read.")}</p>
        <button type="button" className="secondary-button" disabled={props.disabled} onClick={() => void native.refresh()}>
          {t("common.retry", undefined, "Retry")}
        </button>
      </div> : native?.state.status === "ready" && native.state.world.source === "missing_metadata" ?
        <p className="form-note" role="status">{t("valheim.settings.rules.missingMetadata", undefined,
          "This world's rule metadata is missing. Current rules cannot be shown.")}</p> : null}
      {malformedNotice("world_preset")}
      <div className="settings-schema-grid configuration-field-grid">
        <Field fieldKey="world_preset" title={t("settings.schema.valheim.world_preset.title", undefined, "World Preset")}
          description={t("settings.schema.valheim.world_preset.description", undefined, "Leave empty to preserve the current world rules.")} t={t}>
          {(inputId, descriptionId) => <select id={inputId} className="settings-schema-input settings-schema-select"
            value={preset} disabled={props.disabled || malformed("world_preset")} aria-describedby={descriptionId} aria-invalid={invalidPreset || malformed("world_preset") || undefined}
            onChange={(event) => props.onPatch({ world_preset: event.target.value })}>
            {invalidPreset ? <option value={preset} disabled>{t("valheim.settings.rules.unrecognizedChoice", undefined, "Unrecognized saved choice")}</option> : null}
            {contract.presets.map((value) => <option key={value} value={value}>
              {value ? presetLabel(value) : newWorld ? t("valheim.settings.rules.newWorldPreset", undefined, "New world: Normal")
                : t("valheim.settings.rules.savedPreset", { value: presetLabel(savedPreset) },
                  `Saved rules: ${presetLabel(savedPreset)}`)}
            </option>)}
          </select>}
        </Field>
      </div>
      {preset && savedPreset ? pending(presetLabel(savedPreset), presetLabel(preset)) : null}
    </section> : null}
    {show("world_modifiers") ? <section className="guided-field-group" data-valheim-rule-group="modifiers" data-field-key="world_modifiers">
      <div className="guided-field-group-head"><h5 className="guided-field-group-title">
        {t("valheim.settings.rules.modifiersTitle", undefined, "Difficulty & Gameplay")}
      </h5></div>
      {malformedNotice("world_modifiers")}
      {repairNotice("world_modifiers", invalidModifiers, modifiers.filter((entry) => isValheimModifierEntry(entry, contract)))}
      {repeatedModifiers ? <p className="form-note" role="status">{t("valheim.settings.rules.duplicates", undefined,
        "Some rules are set more than once. The last choice is shown; choosing a new value merges that rule into one entry.")}</p> : null}
      <div className="settings-schema-grid configuration-field-grid">
        {VALHEIM_MODIFIER_NAMES.map((name) => {
          const value = readValheimModifier(modifierValue, name);
          const baseValue = baseKeys ? readSavedValheimModifier(baseKeys, name) : null;
          const current = savedKeys ? readSavedValheimModifier(savedKeys, name) : null;
          const actual = value || baseValue;
          const invalid = value !== "" && !contract.modifiers[name].includes(value);
          return <Field key={name} fieldKey={`valheim_modifier_${name}`} t={t}
            title={t(`valheim.settings.rules.${name}.title`, undefined, name)}
            description={t(`valheim.settings.rules.${name}.description`, undefined, "Applies at server startup.")}>
            {(inputId, descriptionId) => <div className="settings-editor-stack"><select id={inputId} className="settings-schema-input settings-schema-select"
              value={value} disabled={props.disabled || malformedModifiers} aria-describedby={descriptionId} aria-invalid={invalid || malformedModifiers || undefined}
              onChange={(event) => props.onPatch({ world_modifiers: replaceValheimModifier(props.settings.world_modifiers, name, event.target.value) })}>
              <option value="">{modifierLabel(name, baseValue)}</option>
              {invalid ? <option value={value} disabled>{t("valheim.settings.rules.unrecognizedChoice", undefined, "Unrecognized saved choice")}</option> : null}
              {contract.modifiers[name].map((choice) => <option key={choice} value={choice}>
                {t(`valheim.settings.rules.${name}.option.${choice}`, undefined, choice)}
              </option>)}
            </select>{current !== null && actual !== null ? pending(modifierLabel(name, current), modifierLabel(name, actual)) : null}</div>}
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
          const baseEnabled = baseKeys ? baseKeys.includes(key) : null;
          const explicit = keys.includes(key);
          const enabled = explicit || baseEnabled;
          const current = savedKeys ? savedKeys.includes(key) : null;
          const lockedNative = baseEnabled === true;
          const description = lockedNative ? t("valheim.settings.rules.enabledNativeHelp", undefined,
            "Enabled in the saved world or selected preset. The native launch options cannot turn this rule off individually.")
            : t(`valheim.settings.rules.${key}.description`, undefined, "Applies at server startup.");
          return <Field key={key} fieldKey={`valheim_key_${key}`} t={t}
          title={t(`valheim.settings.rules.${key}.title`, undefined, key)}
          description={description}>
          {(inputId, descriptionId) => <div className="settings-editor-stack"><label className="settings-toggle-card" htmlFor={inputId}>
            <span className="settings-toggle-copy"><span className="settings-field-title">{keyLabel(enabled)}</span></span>
            <input id={inputId} type="checkbox" checked={enabled === true}
              ref={(input) => { if (input) input.indeterminate = enabled === null; }}
              aria-checked={enabled === null ? "mixed" : enabled}
              disabled={props.disabled || malformedKeys || lockedNative || enabled === null}
              aria-describedby={descriptionId}
              onChange={(event) => props.onPatch({ world_set_keys: replaceValheimWorldKey(props.settings.world_set_keys, key, event.target.checked) })} />
          </label>{current !== null && enabled !== null ? pending(keyLabel(current), keyLabel(enabled)) : null}
          {lockedNative && explicit ? <button type="button" className="ghost-button" disabled={props.disabled || malformedKeys}
            onClick={() => props.onPatch({ world_set_keys: replaceValheimWorldKey(props.settings.world_set_keys, key, false) })}>
            {t("valheim.settings.rules.releaseOverride", undefined, "Remove startup override")}
          </button> : null}</div>}
        </Field>;
        })}
      </div>
    </section> : null}
  </div>;
}
