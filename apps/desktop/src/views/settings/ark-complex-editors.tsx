import type { ComponentType } from "react";
import type { ConfigurationSpecializedRendererProps, SettingsModuleDefinition, SettingsModuleSpecializedRenderer } from "./module-types";
import type { ConfigurationFieldPresentationOverride } from "./settings-schema";
import { ArkStatsEditor } from "./ArkStatsEditor";
import { ArkEngramPointsEditor, ArkLevelsEditor } from "./ArkLevelsEditor";
import { ArkRulesEditor } from "./ArkRulesEditor";
import { arkText, type ArkFieldEditorProps } from "./ArkEditorFrame";
import { ARK_RULE_FIELDS } from "./ark-rule-fields";
import { validateArkComplexField } from "./ark-complex-validation";
import { ArkClusterMapsEditor } from "./ArkClusterMapsEditor";
import { getArkMapSettingsValidationIssues } from "./ark-cluster-maps";

export function arkComplexEditors(moduleId: "arksurvivalascended" | "arksurvivalevolved"): Pick<SettingsModuleDefinition, "fieldPresentationOverrides" | "specializedRenderers" | "getFieldValidationMessage" | "getSettingsValidationIssues"> {
  const presentation: Record<string, ConfigurationFieldPresentationOverride> = {};
  const renderers: Record<string, SettingsModuleSpecializedRenderer> = {};
  function register(key: string, sectionId: string, Editor: ComponentType<ArkFieldEditorProps>) {
    const rendererId = `ark-structured-${key}`;
    presentation[key] = { state: "specialized", owner: "configuration", sectionId, rendererId };
    const Renderer = (props: ConfigurationSpecializedRendererProps) => <Editor {...props} settingKey={key} />;
    renderers[rendererId] = { kind: "module-addon", sectionId, Renderer };
  }
  for (const key of ["per_level_stats_multiplier_player_integer", "per_level_stats_multiplier_dino_wild_integer", "per_level_stats_multiplier_dino_tamed_type_integer"]) register(key, "leveling", ArkStatsEditor);
  if (moduleId === "arksurvivalevolved") register("player_base_stat_multipliers_attribute", "leveling", ArkStatsEditor);
  register("level_experience_ramp_overrides", "experience", ArkLevelsEditor);
  register("override_player_level_engram_points", "experience", ArkEngramPointsEditor);
  for (const [key, field] of Object.entries(ARK_RULE_FIELDS)) {
    if (key !== "override_engram_entries" || moduleId === "arksurvivalevolved") register(key, field.section, ArkRulesEditor);
  }
  presentation.additional_maps = { state: "specialized", owner: "configuration", sectionId: "transfer", rendererId: "ark-cluster-maps" };
  renderers["ark-cluster-maps"] = { kind: "module-addon", placement: "before-fields", sectionId: "transfer", fieldKey: "additional_maps", Renderer: ArkClusterMapsEditor };
  return {
    fieldPresentationOverrides: presentation,
    specializedRenderers: renderers,
    getFieldValidationMessage: ({ field, value, t }) => validateArkComplexField(field.key, value) ? arkText(t, "invalid") : undefined,
    getSettingsValidationIssues: (settings, { t }) => getArkMapSettingsValidationIssues(settings, t)
  };
}
