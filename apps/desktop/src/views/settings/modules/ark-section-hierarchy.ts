import type { TranslateFn } from "../../../i18n";
import type { GuidedSectionId, GuidedSettingsSection } from "../settings-schema";

export const ARK_PARENT_DOMAIN_IDS = [
  "ark-cluster-transfer",
  "ark-world-gameplay",
  "ark-rates-progression",
  "ark-building-resources",
  "ark-content-rules"
] as const;

export type ArkParentDomainId = (typeof ARK_PARENT_DOMAIN_IDS)[number] | "access" | "runtime";

type ParentDomainSpec = {
  id: ArkParentDomainId;
  titleKey: string;
  descriptionKey: string;
  fallbackTitle: string;
  fallbackDescription: string;
};

const ARK_PARENT_DOMAINS: readonly ParentDomainSpec[] = [
  {
    id: "ark-cluster-transfer",
    titleKey: "ark.settings.parents.clusterTransfer",
    descriptionKey: "ark.settings.parents.clusterTransferDescription",
    fallbackTitle: "Cluster & Transfer",
    fallbackDescription: "Cluster identity, tribute storage, uploads, downloads, and transfer filters."
  },
  {
    id: "ark-world-gameplay",
    titleKey: "ark.settings.parents.worldGameplay",
    descriptionKey: "ark.settings.parents.worldGameplayDescription",
    fallbackTitle: "World & Gameplay",
    fallbackDescription: "Difficulty, time, PvE or PvP posture, player visibility, and core world rules."
  },
  {
    id: "ark-rates-progression",
    titleKey: "ark.settings.parents.ratesProgression",
    descriptionKey: "ark.settings.parents.ratesProgressionDescription",
    fallbackTitle: "Rates, Progression & Character Stats",
    fallbackDescription: "Rates, combat balance, experience curves, and per-level survivor or creature stats."
  },
  {
    id: "ark-building-resources",
    titleKey: "ark.settings.parents.buildingResources",
    descriptionKey: "ark.settings.parents.buildingResourcesDescription",
    fallbackTitle: "Building, Breeding, Farming & Resources",
    fallbackDescription: "Structures, anti-abuse limits, resource refresh, crops, breeding, and imprinting."
  },
  {
    id: "ark-content-rules",
    titleKey: "ark.settings.parents.contentRules",
    descriptionKey: "ark.settings.parents.contentRulesDescription",
    fallbackTitle: "Spawns, Loot, Crafting & Engrams",
    fallbackDescription: "Creature ecology, spawn containers, supply drops, recipes, item rules, and engrams."
  }
];

export function buildArkParentDomains(t: TranslateFn): GuidedSettingsSection[] {
  return ARK_PARENT_DOMAINS.map((domain, index) => ({
    id: domain.id,
    title: t(domain.titleKey, undefined, domain.fallbackTitle),
    description: t(domain.descriptionKey, undefined, domain.fallbackDescription),
    order: index * 100
  }));
}

export function buildArkSectionHierarchy(
  t: TranslateFn,
  childSections: readonly GuidedSettingsSection[],
  parentByChild: Readonly<Record<GuidedSectionId, ArkParentDomainId>>
): GuidedSettingsSection[] {
  const childOrderByParent = new Map<ArkParentDomainId, number>();
  const children = childSections.map((section) => {
    const parentId = parentByChild[section.id];
    if (!parentId) {
      throw new Error(`ARK section ${section.id} has no parent domain`);
    }
    const nextOrder = (childOrderByParent.get(parentId) ?? 0) + 10;
    childOrderByParent.set(parentId, nextOrder);
    return { ...section, parentId, order: nextOrder };
  });

  return [...buildArkParentDomains(t), ...children];
}
