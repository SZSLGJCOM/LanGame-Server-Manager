import type { TranslateFn } from "../../../i18n";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type { GuidedFieldCopy, GuidedSettingsField, GuidedSettingsSection } from "../settings-schema";
import {
  buildArkFieldCopy,
  buildArkFieldGroups,
  buildArkSections
} from "./ark-definition-shared";
import { ARK_ASA_FOUNDATION_GROUPS } from "./ark-asa-groups-foundation";
import { ARK_ASA_RULE_GROUPS } from "./ark-asa-groups-rules";
import { arkComplexEditors } from "../ark-complex-editors";
import { ArkConfigurationTransfer } from "../ArkConfigurationTransfer";

const ARK_ASA_GROUPS = {
  ...ARK_ASA_FOUNDATION_GROUPS,
  ...ARK_ASA_RULE_GROUPS
};

export function buildArkAsaFieldCopy(key: string, t: TranslateFn): GuidedFieldCopy | undefined {
  return buildArkFieldCopy("arksurvivalascended", key, t);
}

export function buildArkAsaSections(t: TranslateFn): GuidedSettingsSection[] {
  return buildArkSections(t, "ark.settings.sections.modsDescription").map((section) =>
    section.id === "loot" ? {
      ...section,
      description: t(
        "arksa.settings.sections.lootDescription",
        undefined,
        "Supply-drop quality, drop locations, crate contents, and dropped-item lifetime."
      )
    } : section
  );
}

export function buildArkAsaFieldGroups(
  sectionId: string,
  fields: GuidedSettingsField[],
  t: TranslateFn
): SettingsModuleFieldGroup[] {
  return buildArkFieldGroups(sectionId, fields, t, ARK_ASA_GROUPS);
}

export const arkSurvivalAscendedSettingsDefinition: SettingsModuleDefinition = {
  id: "arksurvivalascended",
  ...arkComplexEditors("arksurvivalascended"),
  workspaceToolbar: ArkConfigurationTransfer,
  getSections: buildArkAsaSections,
  buildFieldGroups: (sectionId, fields, _locale, t) => buildArkAsaFieldGroups(sectionId, fields, t),
  getFieldCopy: (key, t) => buildArkAsaFieldCopy(key, t)
};
