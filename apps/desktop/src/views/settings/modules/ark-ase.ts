import type { TranslateFn } from "../../../i18n";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type { GuidedFieldCopy, GuidedSettingsField, GuidedSettingsSection } from "../settings-schema";
import {
  buildArkFieldCopy,
  buildArkFieldGroups,
  buildArkSections
} from "./ark-definition-shared";
import { ARK_ASE_FOUNDATION_GROUPS } from "./ark-ase-groups-foundation";
import { ARK_ASE_RULE_GROUPS } from "./ark-ase-groups-rules";
import { arkComplexEditors } from "../ark-complex-editors";
import { ArkConfigurationTransfer } from "../ArkConfigurationTransfer";

const ARK_ASE_GROUPS = {
  ...ARK_ASE_FOUNDATION_GROUPS,
  ...ARK_ASE_RULE_GROUPS
};

export function buildArkAseFieldCopy(key: string, t: TranslateFn): GuidedFieldCopy | undefined {
  return buildArkFieldCopy("arksurvivalevolved", key, t);
}

export function buildArkAseSections(t: TranslateFn): GuidedSettingsSection[] {
  return buildArkSections(t, "arkse.settings.sections.modsDescription");
}

export function buildArkAseFieldGroups(
  sectionId: string,
  fields: GuidedSettingsField[],
  t: TranslateFn
): SettingsModuleFieldGroup[] {
  return buildArkFieldGroups(sectionId, fields, t, ARK_ASE_GROUPS);
}

export const arkSurvivalEvolvedSettingsDefinition: SettingsModuleDefinition = {
  id: "arksurvivalevolved",
  ...arkComplexEditors("arksurvivalevolved"),
  workspaceToolbar: ArkConfigurationTransfer,
  getSections: buildArkAseSections,
  buildFieldGroups: (sectionId, fields, _locale, t) => buildArkAseFieldGroups(sectionId, fields, t),
  getFieldCopy: (key, t) => buildArkAseFieldCopy(key, t)
};
