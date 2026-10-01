import type { TranslateFn } from "../../i18n";
import { GUIDED_CORE_SECTION_IDS, type GuidedSettingsSection } from "./settings-schema";

const COMMON_SECTION_PARENTS: Readonly<Record<string, string>> = {
  admin: "access",
  join: "access",
  admin_role: "access",
  friend_role: "access",
  guest_role: "access",
  visitor_role: "access",
  advanced: "runtime",
  performance: "runtime",
  host: "runtime",
  raw: "runtime",
  services: "runtime"
};

export function withCommonSectionParent(section: GuidedSettingsSection): GuidedSettingsSection {
  const parentId = section.parentId ?? COMMON_SECTION_PARENTS[section.id];
  return parentId ? { ...section, parentId } : section;
}

export function buildCoreSettingsSections(t: TranslateFn): GuidedSettingsSection[] {
  return [
    {
      id: "room",
      title: t("settings.sections.room", undefined, "Room Settings"),
      description: t("settings.sections.roomDescription", undefined,
        "Server identity, join password, player capacity, listing, and world selection.")
    },
    {
      id: "network",
      showWhenEmpty: true,
      title: t("settings.sections.network", undefined, "Network"),
      description: t("settings.sections.networkDescription", undefined,
        "Listen and public addresses, ports, and remote console connections.")
    },
    {
      id: "access",
      title: t("settings.sections.access", undefined, "Administration & Permissions"),
      description: t("settings.sections.accessDescription", undefined,
        "Operator credentials, permission levels, and command or admission policies.")
    },
    {
      id: "runtime",
      title: t("settings.sections.runtime", undefined, "Runtime & Advanced"),
      description: t("settings.sections.runtimeDescription", undefined,
        "Performance, process behavior, launch arguments, and advanced native overrides.")
    }
  ].map((section, index) => ({ ...section, order: -400 + index * 100 }));
}

export function buildGuidedSections(
  sections: GuidedSettingsSection[],
  t: TranslateFn
): GuidedSettingsSection[] {
  const moduleSections: GuidedSettingsSection[] = [];
  const seen = new Set<string>();
  for (const section of sections) {
    if (seen.has(section.id)) {
      throw new Error(`duplicate guided settings section ${section.id}`);
    }
    seen.add(section.id);
    if (!GUIDED_CORE_SECTION_IDS.some((id) => id === section.id)) {
      moduleSections.push(withCommonSectionParent(section));
    }
  }
  return [...buildCoreSettingsSections(t), ...moduleSections];
}
