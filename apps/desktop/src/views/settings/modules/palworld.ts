import type { TranslateFn } from "../../../i18n";
import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "../module-types";
import type { GuidedFieldCopy, GuidedSettingsField, GuidedSettingsSection } from "../settings-schema";

function buildPalworldSections(t: TranslateFn, _locale?: string): GuidedSettingsSection[] {
  return [
    {
      id: "access",
      title: t("palworld.module.sections.access.title", undefined, "Access"),
      description: t("palworld.module.sections.access.description", undefined, "Join credentials, authentication, and cross-platform access.")
    },
    {
      id: "services",
      title: t("palworld.module.sections.services.title", undefined, "Services & Logs"),
      description: t("palworld.module.sections.services.description", undefined, "Server log output.")
    },
    {
      id: "world",
      title: t("palworld.module.sections.world.title", undefined, "World & Features"),
      description: t("palworld.module.sections.world.description", undefined, "Fast travel, logout behavior, build restrictions, global Palbox, stat growth, and randomizer toggles.")
    },
    {
      id: "pvp",
      title: t("palworld.module.sections.pvp.title", undefined, "PvP & Death"),
      description: t("palworld.module.sections.pvp.description", undefined, "PvP switches, death handling, logout penalties, and PvP-specific map visibility.")
    },
    {
      id: "rates",
      title: t("palworld.module.sections.rates.title", undefined, "Rates & Progression"),
      description: t("palworld.module.sections.rates.description", undefined, "Time, XP, capture, drops, combat, hunger, stamina, durability, and hatch pacing.")
    },
    {
      id: "performance",
      title: t("palworld.module.sections.performance.title", undefined, "Performance & Limits"),
      description: t("palworld.module.sections.performance.description", undefined, "Simulation budgets, replication distance, dropped-item limits, and launch-thread tuning.")
    },
    {
      id: "advanced",
      title: t("palworld.module.sections.advanced.title", undefined, "Advanced"),
      description: t("palworld.module.sections.advanced.description", undefined, "Advanced engine switches.")
    }
  ];
}

function readCatalogText(t: TranslateFn, key: string): string | undefined {
  const value = t(key, undefined, "");
  return value.trim() ? value : undefined;
}

function getPalworldFieldCopy(key: string, t: TranslateFn): GuidedFieldCopy | undefined {
  const title = readCatalogText(t, `settings.schema.palworld.${key}.title`);
  const description = readCatalogText(t, `settings.schema.palworld.${key}.description`);

  if (!title) {
    return undefined;
  }

  return { title, description };
}

function getPalworldEnumOptionLabel(fieldKey: string, value: unknown, t: TranslateFn): string | undefined {
  if (typeof value !== "string") {
    return undefined;
  }

  return readCatalogText(t, `settings.schema.palworld.${fieldKey}.option.${value}`);
}

function buildPalworldFieldGroups(sectionId: string, fields: GuidedSettingsField[], t: TranslateFn): SettingsModuleFieldGroup[] {
  const fieldsByKey = new Map(fields.map((field) => [field.key, field]));
  const claimedKeys = new Set<string>();

  const copy = {
    roomGateTitle: t("palworld.module.groups.joinGate", undefined, "Administration and admission"),
    roomGateBody: t("palworld.module.groups.keepPasswordsAuthAndCrossplayPolicyTogether", undefined, "Administrator credentials, authentication, platform admission, and client trust policies."),
    roomDiscoveryTitle: t("palworld.module.groups.discoveryPosture", undefined, "Server listing"),
    roomDiscoveryBody: t("palworld.module.groups.chooseBetweenAPublicListingAndADirectOrPrivateShareFlow", undefined, "Choose between a public listing and a direct or private share flow."),
    servicesVisibilityTitle: t("palworld.module.groups.logsAndHostVisibility", undefined, "Log output"),
    servicesVisibilityBody: t("palworld.module.groups.settleTheDailyHostObservabilitySurfaceFirst", undefined, "Format of the server's log output."),
    playerFeedbackTitle: t("palworld.module.groups.playerFeedback", undefined, "Player notifications and identity"),
    simulationTitle: t("palworld.module.groups.simulationBudget", undefined, "Guild processing budget"),
    servicesSaveTitle: t("palworld.module.groups.saveCadence", undefined, "Save cadence"),
    servicesSaveBody: t("palworld.module.groups.keepRollingBackupsAndAutosaveCadenceTogether", undefined, "Keep rolling backups and autosave cadence together."),
    servicesInterfacesTitle: t("palworld.module.groups.operatorInterfaces", undefined, "Operator interfaces"),
    voiceChatTitle: t("palworld.module.groups.voiceChat", undefined, "Voice chat"),
    servicesInterfacesBody: t("palworld.module.groups.operatorInterfacesDescription", undefined, "REST and game data APIs for server administration."),
    worldTravelTitle: t("palworld.module.groups.travelAndWorldEvents", undefined, "Travel and world events"),
    worldTravelBody: t("palworld.module.groups.travelRulesAndInvasionPostureShapeTheWorldSFeelImmediately", undefined, "Travel rules and invasion posture shape the world's feel immediately."),
    worldContinuityTitle: t("palworld.module.groups.continuityAndResets", undefined, "Continuity and resets"),
    worldContinuityBody: t("palworld.module.groups.offlineBehaviorGuildResetsAndHardcoreGuardrailsBelongTogether", undefined, "Offline behavior, guild resets, and hardcore guardrails belong together."),
    worldProgressionTitle: t("palworld.module.groups.globalProgressionRules", undefined, "Global progression rules"),
    worldProgressionBody: t("palworld.module.groups.palboxAndStatGrowthPermissionsReshapeProgressionBoundaries", undefined, "Palbox and stat-growth permissions reshape progression boundaries."),
    worldRandomizerTitle: t("palworld.module.groups.randomizerAndInputAssist", undefined, "Randomizer and input assist"),
    worldRandomizerBody: t("palworld.module.groups.keepAdvancedRandomizerAndAssistTogglesConcentrated", undefined, "Keep advanced randomizer and assist toggles concentrated."),
    pvpConflictTitle: t("palworld.module.groups.conflictSwitches", undefined, "Conflict switches"),
    pvpConflictBody: t("palworld.module.groups.decideWhetherPvpIsTrulyOnAndWhereDamageBoundariesSit", undefined, "Decide whether PvP is truly on and where damage boundaries sit."),
    pvpDeathTitle: t("palworld.module.groups.deathConsequences", undefined, "Death consequences"),
    pvpDeathBody: t("palworld.module.groups.deathPenaltyAndLootRulesAreTheMostContestedPvpSettings", undefined, "Death penalty and loot rules are the most contested PvP settings."),
    pvpIntelTitle: t("palworld.module.groups.mapIntelAndKillBonus", undefined, "Map intel and kill bonus"),
    pvpIntelBody: t("palworld.module.groups.mapExposureAndKillRewardsShouldStayInOneConflictLayer", undefined, "Map exposure and kill rewards should stay in one conflict layer."),
    ratesPaceTitle: t("palworld.module.groups.worldPace", undefined, "World pace"),
    ratesPaceBody: t("palworld.module.groups.timeXpCaptureAndHatchSpeedDefineTheServerSFirstImpression", undefined, "Time, XP, capture, and hatch speed define the server's first impression."),
    ratesResourceTitle: t("palworld.module.groups.gatheringAndBuilding", undefined, "Gathering and building"),
    ratesResourceBody: t("palworld.module.groups.resourceAndBuildingPressureShouldBeTunedTogether", undefined, "Resource and building pressure should be tuned together."),
    ratesSurvivalTitle: t("palworld.module.groups.playerAndPalSurvival", undefined, "Player and Pal survival"),
    ratesSurvivalBody: t("palworld.module.groups.combatHungerStaminaAndRegenAreTheCoreSurvivalLayer", undefined, "Combat, hunger, stamina, and regen are the core survival layer."),
    ratesLocksTitle: t("palworld.module.groups.progressionLocks", undefined, "Progression locks"),
    ratesLocksBody: t("palworld.module.groups.keepDisabledTechnologiesGroupedAsOneProgressionGate", undefined, "Keep disabled technologies grouped as one progression gate."),
    perfBaseTitle: t("palworld.module.groups.baseCapacity", undefined, "Base and guild limits"),
    perfBaseBody: t("palworld.module.groups.baseCountGuildSizeAndWorkersDriveAlwaysOnSimulationLoad", undefined, "Base counts, building limits, workers, and guild membership rules."),
    perfItemsTitle: t("palworld.module.groups.droppedItemsAndSync", undefined, "Dropped items and sync"),
    perfItemsBody: t("palworld.module.groups.droppedItemVolumeAndReplicationDistanceUsuallyHitHostLoadFirst", undefined, "Dropped-item volume and replication distance usually hit host load first."),
    perfThreadsTitle: t("palworld.module.groups.launchThreading", undefined, "Launch threading"),
    perfThreadsBody: t("palworld.module.groups.theseFieldsAreHostLaunchStrategyNotPlayerFacingRoomCopy", undefined, "These fields are host launch strategy, not player-facing room copy."),
    advancedTitle: t("palworld.module.groups.internalFlags", undefined, "Internal flags"),
    advancedBody: t("palworld.module.groups.leaveShippedInternalFlagsAtDefaultUnlessYouKnowWhyTheyAre", undefined, "Leave shipped internal flags at default unless you know why they are changing."),
    additionalTitle: t("palworld.module.groups.additional", undefined, "Additional"),
    additionalBody: t("palworld.module.groups.thisCollectsPalworldFieldsThatDoNotHaveADedicatedGroupYet", undefined, "This collects Palworld fields that do not have a dedicated group yet.")
  };

  const groupSpecMap: Record<string, Array<{ id: string; title: string; description: string; layoutClass: string; keys: string[] }>> = {
    access: [
      { id: "join-gate", title: copy.roomGateTitle, description: copy.roomGateBody, layoutClass: "access-gate", keys: ["admin_password", "use_auth", "crossplay_platforms", "ban_list_url", "allow_client_mod", "chat_post_limit_per_minute"] }
    ],
    network: [
      { id: "discovery", title: copy.roomDiscoveryTitle, description: copy.roomDiscoveryBody, layoutClass: "network-discovery", keys: ["public_ip", "public_port"] },
      { id: "operator-interfaces", title: copy.servicesInterfacesTitle, description: copy.servicesInterfacesBody, layoutClass: "network-interfaces", keys: ["rest_api_enabled", "gamedata_api_enabled"] },
      { id: "voice-chat", title: copy.voiceChatTitle, description: "", layoutClass: "network-voice", keys: ["enable_voice_chat", "voice_chat_max_volume_distance", "voice_chat_zero_volume_distance"] }
    ],
    services: [
      { id: "host-observability", title: copy.servicesVisibilityTitle, description: copy.servicesVisibilityBody, layoutClass: "services-observability", keys: ["log_format"] },
      { id: "save-cadence", title: copy.servicesSaveTitle, description: copy.servicesSaveBody, layoutClass: "services-saves", keys: ["use_backup_save_data", "auto_save_span"] }
    ],
    world: [
      { id: "base-capacity", title: copy.perfBaseTitle, description: copy.perfBaseBody, layoutClass: "world-bases", keys: ["base_camp_max_num", "base_camp_max_num_in_guild", "base_camp_worker_max_num", "guild_player_max_num", "guild_rejoin_cooldown_minutes", "max_building_limit_num", "max_building_limit_num_per_player"] },
      { id: "player-feedback", title: copy.playerFeedbackTitle, description: "", layoutClass: "world-feedback", keys: ["join_left_message", "enable_building_player_uid_display"] },
      { id: "travel-and-events", title: copy.worldTravelTitle, description: copy.worldTravelBody, layoutClass: "world-travel", keys: ["enable_fast_travel", "enable_fast_travel_only_base_camp", "enable_invader_enemy", "enemy_camp_spawn_near_base", "enable_predator_boss_pal", "build_area_limit", "invisible_other_guild_base_camp_area_fx"] },
      { id: "continuity-and-reset", title: copy.worldContinuityTitle, description: copy.worldContinuityBody, layoutClass: "world-continuity", keys: ["exist_player_after_logout", "is_start_location_select_by_map", "auto_reset_guild_no_online_players", "auto_reset_guild_time_no_online_players", "auto_transfer_master_threshold_days", "hardcore", "character_recreate_in_hardcore", "drop_item_alive_max_hours"] },
      { id: "global-progression", title: copy.worldProgressionTitle, description: copy.worldProgressionBody, layoutClass: "world-progression", keys: ["allow_global_palbox_export", "allow_global_palbox_import", "allow_enhance_stat_health", "allow_enhance_stat_attack", "allow_enhance_stat_stamina", "allow_enhance_stat_weight", "allow_enhance_stat_work_speed"] },
      { id: "randomizer-and-input", title: copy.worldRandomizerTitle, description: copy.worldRandomizerBody, layoutClass: "world-randomizer", keys: ["randomizer_type", "randomizer_seed", "is_randomizer_pal_level_random", "enable_aim_assist_pad", "enable_aim_assist_keyboard"] }
    ],
    pvp: [
      { id: "conflict-core", title: copy.pvpConflictTitle, description: copy.pvpConflictBody, layoutClass: "pvp-conflict", keys: ["is_pvp", "enable_player_to_player_damage", "enable_friendly_fire", ] },
      { id: "death-and-loss", title: copy.pvpDeathTitle, description: copy.pvpDeathBody, layoutClass: "pvp-death", keys: ["death_penalty", "pal_lost", "can_pickup_other_guild_death_penalty_drop", "block_respawn_time", "respawn_penalty_duration_threshold", "respawn_penalty_time_scale"] },
      { id: "map-intel-and-bonus", title: copy.pvpIntelTitle, description: copy.pvpIntelBody, layoutClass: "pvp-intel", keys: ["display_pvp_item_num_on_world_map_base_camp", "display_pvp_item_num_on_world_map_player", "additional_drop_item_when_player_killing_in_pvp_mode_enabled", "additional_drop_item_when_player_killing_in_pvp_mode", "additional_drop_item_num_when_player_killing_in_pvp_mode"] }
    ],
    rates: [
      { id: "world-pace", title: copy.ratesPaceTitle, description: copy.ratesPaceBody, layoutClass: "rates-pace", keys: ["day_time_speed_rate", "night_time_speed_rate", "exp_rate", "pal_capture_rate", "fishing_difficulty_rate", "pal_spawn_num_rate", "work_speed_rate", "supply_drop_span", "pal_egg_default_hatching_time"] },
      { id: "resource-and-building", title: copy.ratesResourceTitle, description: copy.ratesResourceBody, layoutClass: "rates-resource", keys: ["collection_drop_rate", "collection_object_hp_rate", "collection_object_respawn_speed_rate", "enemy_drop_item_rate", "monster_farm_action_speed_rate", "build_object_hp_rate", "build_object_damage_rate", "build_object_deterioration_damage_rate", "item_weight_rate", "equipment_durability_damage_rate", "item_corruption_multiplier"] },
      { id: "player-and-pal-combat", title: copy.ratesSurvivalTitle, description: copy.ratesSurvivalBody, layoutClass: "rates-survival", keys: ["player_damage_rate_attack", "player_damage_rate_defense", "player_stomach_decreace_rate", "player_stamina_decreace_rate", "player_auto_hp_regene_rate", "player_auto_hp_regene_rate_in_sleep", "pal_damage_rate_attack", "pal_damage_rate_defense", "pal_stomach_decreace_rate", "pal_stamina_decreace_rate", "pal_auto_hp_regene_rate", "pal_auto_hp_regene_rate_in_sleep"] },
      { id: "progression-locks", title: copy.ratesLocksTitle, description: copy.ratesLocksBody, layoutClass: "rates-locks", keys: ["deny_technology_list"] }
    ],
    performance: [
      { id: "guild-processing", title: copy.simulationTitle, description: "", layoutClass: "performance-guilds", keys: ["auto_transfer_master_check_interval_seconds", "max_guilds_per_frame"] },
      { id: "item-and-sync", title: copy.perfItemsTitle, description: copy.perfItemsBody, layoutClass: "performance-items", keys: ["server_replicate_pawn_cull_distance", "item_container_force_mark_dirty_interval", "player_data_pal_storage_update_check_tick_interval", "drop_item_max_num", "physics_active_drop_item_max_num", "drop_item_max_num_unko", "building_name_display_cache_ttl_seconds"] },
      { id: "launch-threads", title: copy.perfThreadsTitle, description: copy.perfThreadsBody, layoutClass: "performance-threads", keys: ["launch_perf_threads", "launch_worker_threads_enabled", "worker_thread_count"] }
    ],
    advanced: [
      { id: "internal-flags", title: copy.advancedTitle, description: copy.advancedBody, layoutClass: "advanced-internal", keys: ["active_unko"] }
    ]
  };

  const rawGroups: Array<SettingsModuleFieldGroup | null> = (groupSpecMap[sectionId] ?? []).map((spec) => {
    const groupFields = spec.keys
      .map((key) => fieldsByKey.get(key))
      .filter((field): field is GuidedSettingsField => Boolean(field));

    for (const field of groupFields) {
      claimedKeys.add(field.key);
    }

    return groupFields.length > 0
      ? {
          id: spec.id,
          title: spec.title,
          description: spec.description,
          layoutClass: spec.layoutClass,
          fields: groupFields
        }
      : null;
  });

  const groups = rawGroups.filter((group): group is SettingsModuleFieldGroup => group !== null);
  const remainingFields = fields.filter((field) => !claimedKeys.has(field.key));
  if (remainingFields.length > 0) {
    groups.push({
      id: "additional",
      title: copy.additionalTitle,
      description: copy.additionalBody,
      layoutClass: `${sectionId}-additional`,
      fields: remainingFields
    });
  }

  return groups.length > 0 ? groups : [{ id: "default", fields }];
}

export const palworldSettingsDefinition: SettingsModuleDefinition = {
  id: "palworld",
  getSections: buildPalworldSections,
  buildFieldGroups: (sectionId, fields, _locale, t) => buildPalworldFieldGroups(sectionId, fields, t),
  getFieldCopy: (key, t) => getPalworldFieldCopy(key, t),
  getEnumOptionLabel: (fieldKey, value, _locale, t) => getPalworldEnumOptionLabel(fieldKey, value, t)
};
