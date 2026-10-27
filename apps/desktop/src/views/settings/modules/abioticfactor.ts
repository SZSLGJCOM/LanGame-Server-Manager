import type { TranslateFn } from "../../../i18n";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type { GuidedFieldCopy, GuidedSettingsField, GuidedSettingsSection } from "../settings-schema";
import { validateAbioticModField } from "./abioticfactor-mods";
import { AbioticOfficialModSource } from "./AbioticOfficialModSource";

const STEAM64_PATTERN = /^\d{17}$/;
const NATIVE_LAUNCH_VALUE_KEYS = new Set(["server_name", "world_save_name", "server_password", "admin_password"]);
const INVALID_WORLD_SAVE_NAME_PATTERN = /[\x00-\x1f\x7f<>:"/\\|?*]|[. ]$/;
const RESERVED_WORLD_SAVE_NAME_PATTERN = /^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i;


function parseDelimitedEntries(value: unknown): string[] {
  if (typeof value !== "string" || value.trim().length === 0) {
    return [];
  }

  const seen = new Set<string>();
  const entries: string[] = [];

  for (const entry of value
    .replace(/\r\n/g, "\n")
    .replace(/\r/g, "\n")
    .split(/[\n,]+/)
    .map((item) => item.trim())
    .filter((item) => item.length > 0 && !item.startsWith("#") && !item.startsWith("//"))) {
    if (!seen.has(entry)) {
      seen.add(entry);
      entries.push(entry);
    }
  }

  return entries;
}

function buildInvalidEntryPreview(entries: string[]): string {
  const preview = entries.slice(0, 3).join(", ");
  return entries.length > 3 ? `${preview}, ...` : preview;
}

function groupFields(
  fieldsByKey: Map<string, GuidedSettingsField>,
  id: string,
  title: string,
  description: string,
  layoutClass: string,
  keys: string[]
): SettingsModuleFieldGroup | null {
  const fields = keys
    .map((key) => fieldsByKey.get(key))
    .filter((field): field is GuidedSettingsField => Boolean(field));

  if (fields.length === 0) {
    return null;
  }

  return { id, title, description, layoutClass, fields };
}

function buildAbioticSections(t: TranslateFn): GuidedSettingsSection[] {
  return [
    {
      id: "official-mods",
      title: t("abiotic.settings.sections.officialMods", undefined, "Official Mods"),
      description: t("abiotic.settings.sections.officialModsDescription", undefined,
        "Configure official mod.io Mods. The native server downloads them at startup; unofficial UE4SS Mods keep their existing Mods workspace.")
    },
    {
      id: "network",
      title: t("abiotic.settings.sections.network", undefined, "Network"),
      description: t(
        "abiotic.settings.sections.networkDescription",
        undefined,
        "LAN discovery, cross-platform connections, and listener binding."
      )
    },
    {
      id: "access",
      title: t("abiotic.settings.sections.access", undefined, "Access"),
      description: t("abiotic.settings.sections.accessDescription", undefined, "Join credentials and administrator permissions.")
    },
    {
      id: "world",
      title: t("abiotic.settings.sections.world", undefined, "World Rules"),
      description: t(
        "abiotic.settings.sections.worldDescription",
        undefined,
        "Difficulty, day cycle, weather, starter gear, and the world-level rules that shape every session."
      )
    },
    {
      id: "resources",
      title: t("abiotic.settings.sections.resources", undefined, "Resources & Economy"),
      description: t(
        "abiotic.settings.sections.resourcesDescription",
        undefined,
        "Respawn behavior, sinks, spoilage, refrigeration, and item economy tuning for long-running saves."
      )
    },
    {
      id: "player",
      title: t("abiotic.settings.sections.player", undefined, "Scientists"),
      description: t(
        "abiotic.settings.sections.playerDescription",
        undefined,
        "Needs, XP, PvP bleed, death handling, cooperation helpers, and recipe-sharing rules."
      )
    },
    {
      id: "combat",
      title: t("abiotic.settings.sections.combat", undefined, "Threat Pressure"),
      description: t(
        "abiotic.settings.sections.combatDescription",
        undefined,
        "Enemy pace, health, damage, detection, accuracy, and radiation pressure for the Facility."
      )
    },
    {
      id: "structure",
      title: t("abiotic.settings.sections.structure", undefined, "Power & Structures"),
      description: t(
        "abiotic.settings.sections.structureDescription",
        undefined,
        "Night-time power, furniture destruction, bridge support, and building stack limits."
      )
    },
    {
      id: "advanced",
      title: t("abiotic.settings.sections.advanced", undefined, "Launch Tuning"),
      description: t(
        "abiotic.settings.sections.advancedDescription",
        undefined,
        "CPU thread flags and other launch-time controls that should stay on the host side of the console."
      )
    }
  ];
}

function buildAbioticFieldGroups(
  sectionId: string,
  fields: GuidedSettingsField[],
  t: TranslateFn
): SettingsModuleFieldGroup[] {
  const fieldsByKey = new Map(fields.map((field) => [field.key, field]));

  const groupSpecs: Record<string, Array<Omit<SettingsModuleFieldGroup, "fields"> & { keys: string[] }>> = {
    "official-mods": [
      {
        id: "official-mod-list",
        title: t("abiotic.settings.groups.officialMods", undefined, "Official Mod selection"),
        description: t("abiotic.settings.groups.officialModsDescription", undefined,
          "Find numeric IDs on mod.io, then save their load order. Downloads and dependency resolution run in the game at the next startup."),
        layoutClass: "advanced-launch",
        keys: ["mod_ids", "modio_token", "allow_local_only_mods"]
      },
      {
        id: "official-mod-loading",
        title: t("abiotic.settings.groups.officialModLoading", undefined, "Download and local packages"),
        description: t("abiotic.settings.groups.officialModLoadingDescription", undefined,
          "Directories belong to this instance. Predownloaded native Mod packages and unofficial UE4SS payloads use separate locations."),
        layoutClass: "advanced-launch",
        keys: ["mods_directory", "mod_install_timeout_seconds", "override_mod_warnings", "sideload_mods", "sideload_mods_path"]
      }
    ],
    access: [
      {
        id: "credentials",
        title: t("abiotic.settings.groups.joinAndAdminLanes", undefined, "Administration"),
        description: t("abiotic.settings.groups.joinPasswordAdminPasswordAndSteamModeratorIDsShouldBeTreatedAsSeparateAccessLane", undefined, "Administrator credentials and platform admission rules."),
        layoutClass: "network-credentials",
        keys: ["admin_password", "moderator_steam_ids", "platform_limited"]
      }
    ],
    network: [
      {
        id: "binding",
        title: t("abiotic.settings.groups.discoveryAndBinding", undefined, "Local addressing"),
        description: t("abiotic.settings.groups.thisIsWhereTheWindowsHostDecidesWhetherTheRoomStaysLANOnlyRemainsCrossPlatformOr", undefined, "Choose whether connections use local IP addresses."),
        layoutClass: "network-binding",
        keys: ["use_local_ips"]
      }
    ],
    world: [
      {
        id: "difficulty",
        title: t("abiotic.settings.groups.worldPosture", undefined, "World posture"),
        description: t("abiotic.settings.groups.difficultyHardcoreAndHomeWorldsDefineWhetherThisRoomFeelsLikeARelaxedCoOpSpaceOr", undefined, "Difficulty, hardcore, and Home Worlds define whether this room feels like a relaxed co-op space or a high-pressure challenge save."),
        layoutClass: "world-posture",
        keys: ["game_difficulty", "hardcore_mode", "allow_iron_mode", "home_worlds", "allow_character_reset"]
      },
      {
        id: "tempo",
        title: t("abiotic.settings.groups.timeAndOnboarding", undefined, "Time and onboarding"),
        description: t("abiotic.settings.groups.dayCycleWeatherDefaultInventoryAndStartingWeaponStronglyShapeTheFirstNightAndThe", undefined, "Day cycle, weather, default inventory, and starting weapon strongly shape the first night and the first few hours."),
        layoutClass: "world-tempo",
        keys: [
          "day_night_cycle_state",
          "day_night_cycle_speed_multiplier",
          "weather_frequency",
          "first_time_starting_weapon",
          "base_inventory_size"
        ]
      }
    ],
    resources: [
      {
        id: "facility",
        title: t("abiotic.settings.groups.facilitySupplyLoop", undefined, "Facility supply loop"),
        description: t("abiotic.settings.groups.respawnsSinksSpoilageAndRefrigerationDecideWhetherALongRunningGroupSaveConstantl", undefined, "Respawns, sinks, spoilage, and refrigeration decide whether a long-running group save constantly starves for supplies."),
        layoutClass: "resources-facility",
        keys: [
          "loot_respawn_enabled",
          "sink_refill_rate",
          "food_spoil_speed_multiplier",
          "refrigeration_effectiveness_multiplier",
          "storage_by_tag",
          "tainted_sink_water"
        ]
      },
      {
        id: "inventory",
        title: t("abiotic.settings.groups.inventoryEconomy", undefined, "Inventory economy"),
        description: t("abiotic.settings.groups.stackSizeWeightAndDurabilityShapeCarryingPressureNotJustAbstractNumbersOnAPage", undefined, "Stack size, weight, and durability shape carrying pressure, not just abstract numbers on a page."),
        layoutClass: "resources-inventory",
        keys: ["item_stack_size_multiplier", "item_weight_multiplier", "item_durability_multiplier"]
      }
    ],
    player: [
      {
        id: "needs",
        title: t("abiotic.settings.groups.needsAndProgression", undefined, "Needs and progression"),
        description: t("abiotic.settings.groups.hungerThirstFatigueContinenceAndXPTuningDecideWhetherTheServerFeelsLikeHarshSurv", undefined, "Hunger, thirst, fatigue, continence, and XP tuning decide whether the server feels like harsh survival or lighter co-op."),
        layoutClass: "player-needs",
        keys: [
          "hunger_speed_multiplier",
          "thirst_speed_multiplier",
          "fatigue_speed_multiplier",
          "continence_speed_multiplier",
          "bonus_perk_points",
          "player_xp_gain_multiplier"
        ]
      },
      {
        id: "death",
        title: t("abiotic.settings.groups.deathAndCooperation", undefined, "Death and cooperation"),
        description: t("abiotic.settings.groups.friendlyFireItemLossDeathBagAccessAndRecipeSharingHaveTheBiggestImpactOnAFriendG", undefined, "Friendly fire, item loss, death-bag access, and recipe-sharing have the biggest impact on a friend group's day-to-day feel."),
        layoutClass: "player-death",
        keys: [
          "damage_to_allies_multiplier",
          "durability_loss_on_death_multiplier",
          "death_penalties",
          "host_access_player_corpses",
          "show_death_messages",
          "allow_recipe_sharing",
          "allow_pagers",
          "allow_transmog",
          "disable_research_minigame"
        ]
      }
    ],
    combat: [
      {
        id: "enemies",
        title: t("abiotic.settings.groups.enemyPressure", undefined, "Enemy pressure"),
        description: t("abiotic.settings.groups.respawnRateHealthDamageDetectionAndAccuracyTogetherDecideWhetherCombatFeelsOppre", undefined, "Respawn rate, health, damage, detection, and accuracy together decide whether combat feels oppressive or exploratory."),
        layoutClass: "combat-enemies",
        keys: [
          "enemy_spawn_rate",
          "enemy_health_multiplier",
          "enemy_player_damage_multiplier",
          "enemy_deployable_damage_multiplier",
          "detection_speed_multiplier",
          "enemy_accuracy",
          "apocalyptic_abilities",
          "maximize_enemy_spawns"
        ]
      },
      {
        id: "radiation",
        title: t("abiotic.settings.groups.radiationPosture", undefined, "Radiation posture"),
        description: t("abiotic.settings.groups.theseTogglesControlWhetherRadiationStaysAReadableUIHazardOrBecomesSomethingPlaye", undefined, "These toggles control whether radiation stays a readable UI hazard or becomes something players must detect with tools and experience."),
        layoutClass: "combat-radiation",
        keys: ["invisible_radiation", "radiation_deals_damage"]
      }
    ],
    structure: [
      {
        id: "power",
        title: t("abiotic.settings.groups.powerAndSupport", undefined, "Power and support"),
        description: t("abiotic.settings.groups.nightPowerLossBridgeSupportAndStructureLimitsDirectlyAffectWhetherSharedBasesSta", undefined, "Night power loss, bridge support, and structure limits directly affect whether shared bases stay stable over long sessions."),
        layoutClass: "structure-core",
        keys: [
          "power_sockets_off_at_night",
          "structural_support_limit",
          "bridge_supports",
          "player_furniture_destruction"
        ]
      }
    ],
    advanced: [
      {
        id: "launch",
        title: t("abiotic.settings.groups.launchFlags", undefined, "Launch flags"),
        description: t("abiotic.settings.groups.theseAreHostSideLaunchDecisionsRatherThanWorldRulesMostServersShouldOnlyRevisitT", undefined, "These are host-side launch decisions rather than world rules. Most servers should only revisit them for performance tuning or troubleshooting."),
        layoutClass: "advanced-launch",
        keys: ["use_perf_threads", "disable_async_loading_thread"]
      }
    ]
  };

  const groups = groupSpecs[sectionId] ?? [];
  return groups
    .map((group) =>
      groupFields(
        fieldsByKey,
        group.id,
        group.title ?? group.id,
        group.description ?? "",
        group.layoutClass ?? group.id,
        group.keys
      )
    )
    .filter((group): group is SettingsModuleFieldGroup => Boolean(group));
}

function readCatalogText(t: TranslateFn, key: string): string | undefined {
  const value = t(key, undefined, "");
  return value.trim() ? value : undefined;
}

function getAbioticFieldCopy(key: string, t: TranslateFn): GuidedFieldCopy | undefined {
  const title = readCatalogText(t, `settings.schema.abioticfactor.${key}.title`);
  const description = readCatalogText(t, `settings.schema.abioticfactor.${key}.description`);

  if (!title) {
    return undefined;
  }

  return { title, description };
}

function getAbioticEnumOptionLabel(fieldKey: string, value: unknown, t: TranslateFn): string | undefined {
  return readCatalogText(t, `settings.schema.abioticfactor.${fieldKey}.option.${String(value)}`);
}


export const abioticFactorSettingsDefinition: SettingsModuleDefinition = {
  id: "abioticfactor",
  fieldPresentationOverrides: {
    mod_ids: { behavior: "multiline" },
    modio_token: { behavior: "secret", resourceUrl: "https://mod.io/me/access#tokens" }
  },
  specializedRenderers: {
    "abiotic-official-mod-source": {
      kind: "module-addon",
      placement: "before-fields",
      sectionId: "official-mods",
      Renderer: AbioticOfficialModSource
    }
  },
  getSections: buildAbioticSections,
  buildFieldGroups(sectionId, fields, _locale, t) {
    return buildAbioticFieldGroups(sectionId, fields, t);
  },
  getFieldCopy: (key, t) => getAbioticFieldCopy(key, t),
  getEnumOptionLabel: (fieldKey, value, _locale, t) => getAbioticEnumOptionLabel(fieldKey, value, t),
  getFieldValidationMessage({ field, value, settings, t }) {
    if (NATIVE_LAUNCH_VALUE_KEYS.has(field.key) && typeof value === "string" && value.includes('"')) {
      return t("abiotic.settings.validation.launchDoubleQuote", undefined,
        "Double quotes cannot be passed to Abiotic Factor in this setting. Remove the double quote before starting.");
    }
    const modIssue = validateAbioticModField(field.key, value, settings, t);
    if (modIssue) return modIssue;

    if (field.key === "moderator_steam_ids") {
      const invalidEntries = parseDelimitedEntries(value).filter((entry) => !STEAM64_PATTERN.test(entry));
      if (invalidEntries.length === 0) {
        return undefined;
      }

      const preview = buildInvalidEntryPreview(invalidEntries);
      return t(
        "abiotic.settings.validation.moderatorSteamIds",
        { preview },
        "Use one 17-digit Steam64 ID per line. Lines starting with # or // are ignored. Invalid entries: {preview}"
      );
    }

    if (field.key === "world_save_name") {
      if (typeof value !== "string") return undefined;
      if (value.length > 0 && !INVALID_WORLD_SAVE_NAME_PATTERN.test(value) && !RESERVED_WORLD_SAVE_NAME_PATTERN.test(value)) {
        return undefined;
      }

      return t("abiotic.settings.validation.worldSaveName", undefined,
        "Use a single non-empty Windows folder name, such as the instance ID. Do not use path separators, reserved names, or a trailing dot or space.");
    }

    if (field.key === "admin_password") {
      const adminPassword = typeof value === "string" ? value : "";
      const serverPassword =
        typeof settings.server_password === "string" ? settings.server_password : "";

      if (adminPassword.length > 0 && adminPassword === serverPassword) {
        return t("abiotic.settings.groups.useADifferentAdminPasswordSoTheJoinGateAndTheOperatorLaneStaySeparate", undefined, "Use a different admin password so the join gate and the operator lane stay separate.");
      }
    }

    return undefined;
  }
};
