import { ActivityNotice } from "../../components/ActivityNotice";
import { useI18n } from "../../i18n";
import type { ConfigurationSpecializedRendererProps } from "./module-types";
import { useValheimWorldRules } from "./ValheimWorldRulesContext";
import {
  VALHEIM_MODIFIER_NAMES, parseValheimRuleEntries, projectValheimModifier,
  readValheimWorldRuleContract, readSavedValheimModifier, readSavedValheimPreset,
  resolveSavedValheimKeys, valheimPresetKeys
} from "./valheim-world-rules";
import "./valheim-world-rules.css";

export function ValheimWorldRulesReadback(props: ConfigurationSpecializedRendererProps) {
  const { t } = useI18n();
  const native = useValheimWorldRules();
  if (!native) return null;
  let contract;
  try { contract = readValheimWorldRuleContract(props.moduleDetails.schema_json ?? ""); }
  catch { return null; } // The configuration panel reports an unavailable schema.
  const world = native.state.status === "ready" ? native.state.world : null;
  const saved = world?.source === "saved" ? resolveSavedValheimKeys(world.saved_keys) : null;
  const savedPreset = saved ? readSavedValheimPreset(saved) : null;
  const baseline = world?.source === "new_world" ? [] : saved;
  const preset = props.settings.world_preset ?? "";
  const base = typeof preset === "string" && contract.presets.includes(preset)
    ? preset ? valheimPresetKeys(preset) : baseline : null;
  const unknown = t("valheim.settings.rules.notRead", undefined, "Not read");
  const modifierLabel = (name: string, value: string | null) => value === null ? unknown
    : value === "normal" ? t("valheim.settings.rules.normal", undefined, "Normal")
    : value === "custom" ? t("valheim.settings.rules.custom", undefined, "Custom")
    : t(`valheim.settings.rules.${name}.option.${value}`, undefined, value);
  const keyLabel = (value: boolean | null) => value === null ? unknown : value
    ? t("valheim.settings.rules.enabled", undefined, "On") : t("valheim.settings.rules.disabled", undefined, "Off");
  const modifiers = props.settings.world_modifiers;
  const keys = props.settings.world_set_keys;
  const validKeys = keys === undefined || typeof keys === "string";
  const explicitKeys = validKeys ? parseValheimRuleEntries(keys) : [];
  const rows = [
    ...VALHEIM_MODIFIER_NAMES.map((name) => {
      const next = projectValheimModifier(base, name, modifiers);
      return { name, title: t(`valheim.settings.rules.${name}.title`, undefined, name),
        current: modifierLabel(name, saved ? readSavedValheimModifier(saved, name) : null),
        next: modifierLabel(name, next) };
    }),
    ...contract.keys.map((name) => ({ name, title: t(`valheim.settings.rules.${name}.title`, undefined, name),
      current: keyLabel(saved ? saved.includes(name) : null),
      next: keyLabel(!validKeys ? null : explicitKeys.includes(name) ? true : base ? base.includes(name) : null) }))
  ];
  const pending = rows.some((row) => row.current !== unknown && row.next !== unknown && row.current !== row.next)
    || Boolean(preset && savedPreset && preset !== savedPreset);
  const message = native.state.status === "loading" ? t("valheim.settings.rules.loading", undefined, "Reading saved world rules…")
    : native.state.status === "error" ? t("valheim.settings.rules.readErrorDetail", { error: native.state.error },
      `Saved world rules could not be read. Configuration remains editable. ${native.state.error}`)
    : world?.source === "missing_metadata" ? t("valheim.settings.rules.missingMetadata", undefined,
      "This world's rule metadata is missing. Current rules cannot be shown.")
    : world?.source === "new_world" ? t("valheim.settings.rules.newWorldReadback", undefined,
      "The selected world has not been saved yet. Its initial rules will be Normal unless startup overrides are selected.")
    : pending ? t("valheim.settings.rules.pendingChanges", undefined,
      "Startup configuration differs from the saved world. Changes apply at the next server start.")
    : t("valheim.settings.rules.readComplete", undefined, "Saved world rules have been read.");
  return <section className="valheim-world-rules-readback" aria-label={t("valheim.settings.rules.readbackTitle", undefined, "Saved world readback")}>
    <ActivityNotice tone={native.state.status === "error" ? "error" : "info"} action={
      <button type="button" className="secondary-button" disabled={native.state.status === "loading"}
        onClick={() => void native.refresh()}>
        {native.state.status === "error" ? t("common.retry", undefined, "Retry")
          : t("valheim.settings.rules.refresh", undefined, "Refresh saved rules")}
      </button>}>
      {message}
    </ActivityNotice>
    <h5 className="guided-field-group-title">{t("valheim.settings.rules.readbackTitle", undefined, "Saved world readback")}</h5>
    {savedPreset ? <p className="valheim-world-rules-readback__preset">{t("valheim.settings.rules.savedPreset", {
      value: savedPreset === "custom" ? t("valheim.settings.rules.custom", undefined, "Custom")
        : t(`settings.schema.valheim.world_preset.option.${savedPreset}`, undefined, savedPreset)
    }, `Saved rules: ${savedPreset}`)}</p> : null}
    <table>
      <thead><tr><th scope="col">{t("valheim.settings.rules.ruleColumn", undefined, "Rule")}</th>
        <th scope="col">{t("valheim.settings.rules.savedColumn", undefined, "Saved world")}</th>
        <th scope="col">{t("valheim.settings.rules.nextColumn", undefined, "Next start")}</th></tr></thead>
      <tbody>{rows.map((row) => <tr key={row.name} data-valheim-readback-rule={row.name}>
        <th scope="row">{row.title}</th><td>{row.current}</td><td>{row.next}</td>
      </tr>)}</tbody>
    </table>
  </section>;
}
