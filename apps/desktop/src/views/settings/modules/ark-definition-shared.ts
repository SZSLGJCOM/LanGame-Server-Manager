import type { TranslateFn } from "../../../i18n";
import type { SettingsModuleFieldGroup } from "../module-types";
import type { GuidedFieldCopy, GuidedSettingsField, GuidedSettingsSection } from "../settings-schema";
import { buildArkSectionHierarchy, type ArkParentDomainId } from "./ark-section-hierarchy";

export type ArkGroupSpec = {
  id: string;
  titleKey: string;
  fallbackTitle: string;
  layoutClass: string;
  keys: readonly string[];
};

const ARK_SECTION_PARENTS: Readonly<Record<string, ArkParentDomainId>> = {
  operations: "runtime",
  join: "access",
  admin: "access",
  moderation: "access",
  transfer: "ark-cluster-transfer",
  gameplay: "ark-world-gameplay",
  world: "ark-world-gameplay",
  rates: "ark-rates-progression",
  balance: "ark-rates-progression",
  leveling: "ark-rates-progression",
  experience: "ark-rates-progression",
  building: "ark-building-resources",
  limits: "ark-building-resources",
  farming: "ark-building-resources",
  breeding: "ark-building-resources",
  spawns: "ark-content-rules",
  loot: "ark-content-rules",
  crafting: "ark-content-rules",
  engrams: "ark-content-rules",
  mods: "runtime",
  logs: "runtime",
  advanced: "runtime"
};

function readCatalogText(t: TranslateFn, key: string): string | undefined {
  const translated = t(key, undefined, "");
  return translated.trim().length > 0 ? translated : undefined;
}

export function buildArkFieldCopy(
  moduleId: string,
  key: string,
  t: TranslateFn
): GuidedFieldCopy | undefined {
  const schemaBaseKey = `settings.schema.${moduleId}.${key}`;
  const title = readCatalogText(t, `${schemaBaseKey}.title`);
  const description = readCatalogText(t, `${schemaBaseKey}.description`);
  return title || description ? { title: title ?? key, description } : undefined;
}

export function buildArkSections(
  t: TranslateFn,
  modsDescriptionKey: string
): GuidedSettingsSection[] {
  const childSections: GuidedSettingsSection[] = [
    {
      id: "operations",
      title: t("ark.settings.sections.operations", undefined, "Host Ops & Safety"),
      description: t(
        "ark.settings.sections.operationsDescription",
        undefined,
        "Autosave, idle-player cleanup, process performance, restart policy, and host safety controls."
      )
    },
    {
      id: "transfer",
      title: t("ark.settings.sections.transfer", undefined, "Transfers & Tribute"),
      description: t(
        "ark.settings.sections.transferDescription",
        undefined,
        "Cluster routing, upload/download guardrails, tribute caps, and cross-map transfer policy."
      )
    },
    {
      id: "join",
      title: t("ark.settings.sections.join", undefined, "Join Control"),
      description: t(
        "ark.settings.sections.joinDescription",
        undefined,
        "Platform admission, access policy, anti-cheat, and spectator credentials."
      )
    },
    {
      id: "admin",
      title: t("ark.settings.sections.admin", undefined, "Admin Control"),
      description: t(
        "ark.settings.sections.adminDescription",
        undefined,
        "Administrator credentials and remote permission sources."
      )
    },
    {
      id: "moderation",
      title: t("ark.settings.sections.moderation", undefined, "Moderation"),
      description: t(
        "ark.settings.sections.moderationDescription",
        undefined,
        "Chat and naming filters plus remote word-list sources."
      )
    },
    {
      id: "gameplay",
      title: t("ark.settings.sections.gameplay", undefined, "Gameplay"),
      description: t(
        "ark.settings.sections.gameplayDescription",
        undefined,
        "PvE/PvP posture, third person, map markers, voice, and visible room rules."
      )
    },
    {
      id: "building",
      title: t("ark.settings.sections.building", undefined, "Building & Offline Protection"),
      description: t(
        "ark.settings.sections.buildingDescription",
        undefined,
        "Structure decay, platform limits, cave building, and offline protection."
      )
    },
    {
      id: "limits",
      title: t("ark.settings.sections.limits", undefined, "Limits & Anti-Abuse"),
      description: t(
        "ark.settings.sections.limitsDescription",
        undefined,
        "Pickup windows, abandoned-base cleanup, turret caps, and tame soft-limit guardrails."
      )
    },
    {
      id: "world",
      title: t("ark.settings.sections.world", undefined, "World & Difficulty"),
      description: t(
        "ark.settings.sections.worldDescription",
        undefined,
        "Difficulty, day-night pacing, and seasonal events for the whole server."
      )
    },
    {
      id: "rates",
      title: t("ark.settings.sections.rates", undefined, "Rates & Progression"),
      description: t(
        "ark.settings.sections.ratesDescription",
        undefined,
        "XP, taming, harvesting, and food or water drain multipliers."
      )
    },
    {
      id: "balance",
      title: t("ark.settings.sections.balance", undefined, "Combat & Balance"),
      description: t(
        "ark.settings.sections.balanceDescription",
        undefined,
        "Resource respawn, stack size, disease posture, flyer rules, and combat multipliers."
      )
    },
    {
      id: "leveling",
      title: t("ark.settings.sections.leveling", undefined, "Character & Creature Stats"),
      description: t(
        "ark.settings.sections.levelingDescription",
        undefined,
        "Base attributes, per-level growth, movement-speed leveling, and respec rules."
      )
    },
    {
      id: "farming",
      title: t("ark.settings.sections.farming", undefined, "Farming & Resource Refresh"),
      description: t(
        "ark.settings.sections.farmingDescription",
        undefined,
        "Crop pacing and resource replenishment blockers near players or structures."
      )
    },
    {
      id: "breeding",
      title: t("ark.settings.sections.breeding", undefined, "Breeding & Imprint"),
      description: t(
        "ark.settings.sections.breedingDescription",
        undefined,
        "Breeding cadence, growth speed, and imprint rules."
      )
    },
    {
      id: "experience",
      title: t("ark.settings.sections.experience", undefined, "Experience Curves"),
      description: t(
        "ark.settings.sections.experienceDescription",
        undefined,
        "Experience curves, player and creature XP limits, and engram points."
      )
    },
    {
      id: "engrams",
      title: t("ark.settings.sections.engrams", undefined, "Engrams & Unlock Rules"),
      description: t(
        "ark.settings.sections.engramsDescription",
        undefined,
        "Automatic unlocks, learning restrictions, and indexed or named engram overrides."
      )
    },
    {
      id: "spawns",
      title: t("ark.settings.sections.spawns", undefined, "Spawns & Ecology"),
      description: t(
        "ark.settings.sections.spawnsDescription",
        undefined,
        "Spawn weights, replacements, and container-level spawn overrides."
      )
    },
    {
      id: "loot",
      title: t("ark.settings.sections.loot", undefined, "Loot & Supply Drops"),
      description: t(
        "ark.settings.sections.lootDescription",
        undefined,
        "Supply-drop quality, crate contents, spawn equipment, and corpse or dropped-item lifetime."
      )
    },
    {
      id: "crafting",
      title: t("ark.settings.sections.crafting", undefined, "Crafting & Item Rules"),
      description: t(
        "ark.settings.sections.craftingDescription",
        undefined,
        "Recipes, crafting bonuses, wireless crafting, item preservation, and item overrides."
      )
    },
    {
      id: "mods",
      title: t("ark.settings.sections.mods", undefined, "Mods"),
      description: t(
        modsDescriptionKey,
        undefined,
        "Mod IDs and install posture that LanGame renders into the game-specific mod entry point."
      )
    },
    {
      id: "logs",
      title: t("ark.settings.sections.logs", undefined, "Logs & Audit"),
      description: t(
        "ark.settings.sections.logsDescription",
        undefined,
        "Game log, tribe log, and admin command echo posture."
      )
    },
    {
      id: "advanced",
      title: t("ark.settings.sections.advanced", undefined, "Raw Overrides"),
      description: t(
        "ark.settings.sections.advancedDescription",
        undefined,
        "Raw file and launch overrides for options outside the structured controls."
      )
    }
  ];
  return buildArkSectionHierarchy(t, childSections, ARK_SECTION_PARENTS);
}

export function buildArkFieldGroups(
  sectionId: string,
  fields: GuidedSettingsField[],
  t: TranslateFn,
  groupCatalog: Readonly<Record<string, readonly ArkGroupSpec[]>>
): SettingsModuleFieldGroup[] {
  const fieldsByKey = new Map(fields.map((field) => [field.key, field]));
  const claimedKeys = new Set<string>();
  const groups = (groupCatalog[sectionId] ?? [])
    .map((spec) => {
      const groupedFields = spec.keys
        .map((key) => fieldsByKey.get(key))
        .filter((field): field is GuidedSettingsField => Boolean(field));
      groupedFields.forEach((field) => claimedKeys.add(field.key));
      return {
        id: spec.id,
        title: t(spec.titleKey, undefined, spec.fallbackTitle),
        layoutClass: spec.layoutClass,
        fields: groupedFields
      };
    })
    .filter((group) => group.fields.length > 0);

  const remainingFields = fields.filter((field) => !claimedKeys.has(field.key));
  if (remainingFields.length > 0) {
    groups.push({
      id: "additional",
      title: t("ark.settings.groups.additional.title", undefined, "Additional fields"),
      layoutClass: `${sectionId}-additional`,
      fields: remainingFields
    });
  }
  return groups.length > 0
    ? groups
    : [{ id: "default", layoutClass: `${sectionId}-default`, fields }];
}
