pub(super) const VRISING_TOP_LEVEL_GAME_SETTINGS: &[(&str, &str)] = &[
    ("game_difficulty", "GameDifficulty"),
    ("game_mode_type", "GameModeType"),
    ("castle_damage_mode", "CastleDamageMode"),
    ("siege_weapon_health", "SiegeWeaponHealth"),
    ("player_damage_mode", "PlayerDamageMode"),
    ("castle_heart_damage_mode", "CastleHeartDamageMode"),
    ("pvp_protection_mode", "PvPProtectionMode"),
    ("death_container_permission", "DeathContainerPermission"),
    ("relic_spawn_type", "RelicSpawnType"),
    ("can_loot_enemy_containers", "CanLootEnemyContainers"),
    ("blood_bound_equipment", "BloodBoundEquipment"),
    ("teleport_bound_items", "TeleportBoundItems"),
    ("bat_bound_items", "BatBoundItems"),
    ("bat_bound_shards", "BatBoundShards"),
    ("allow_global_chat", "AllowGlobalChat"),
    ("all_waypoints_unlocked", "AllWaypointsUnlocked"),
    ("free_castle_raid", "FreeCastleRaid"),
    ("free_castle_claim", "FreeCastleClaim"),
    ("free_castle_destroy", "FreeCastleDestroy"),
    ("inactivity_kill_enabled", "InactivityKillEnabled"),
    ("inactivity_kill_time_min", "InactivityKillTimeMin"),
    ("inactivity_kill_time_max", "InactivityKillTimeMax"),
    (
        "inactivity_kill_safe_time_addition",
        "InactivityKillSafeTimeAddition",
    ),
    (
        "inactivity_kill_timer_max_item_level",
        "InactivityKillTimerMaxItemLevel",
    ),
    ("starting_progression_level", "StartingProgressionLevel"),
    ("weapon_slots", "WeaponSlots"),
    (
        "disable_disconnected_dead_enabled",
        "DisableDisconnectedDeadEnabled",
    ),
    (
        "disable_disconnected_dead_timer",
        "DisableDisconnectedDeadTimer",
    ),
    (
        "disconnected_sun_immunity_time",
        "DisconnectedSunImmunityTime",
    ),
    ("inventory_stacks_modifier", "InventoryStacksModifier"),
    (
        "material_yield_modifier_global",
        "MaterialYieldModifier_Global",
    ),
    ("blood_essence_yield_modifier", "BloodEssenceYieldModifier"),
    ("drop_table_modifier_general", "DropTableModifier_General"),
    ("drop_table_modifier_missions", "DropTableModifier_Missions"),
    (
        "drop_table_modifier_stygian_shards",
        "DropTableModifier_StygianShards",
    ),
    (
        "soul_shard_durability_loss_rate",
        "SoulShard_DurabilityLossRate",
    ),
    (
        "journal_vblood_source_unit_max_distance",
        "JournalVBloodSourceUnitMaxDistance",
    ),
    ("pvp_vampire_respawn_modifier", "PvPVampireRespawnModifier"),
    (
        "castle_minimum_distance_in_floors",
        "CastleMinimumDistanceInFloors",
    ),
    ("clan_size", "ClanSize"),
    ("blood_drain_modifier", "BloodDrainModifier"),
    ("durability_drain_modifier", "DurabilityDrainModifier"),
    (
        "garlic_area_strength_modifier",
        "GarlicAreaStrengthModifier",
    ),
    ("holy_area_strength_modifier", "HolyAreaStrengthModifier"),
    ("silver_strength_modifier", "SilverStrengthModifier"),
    ("sun_damage_modifier", "SunDamageModifier"),
    ("castle_decay_rate_modifier", "CastleDecayRateModifier"),
    (
        "castle_blood_essence_drain_modifier",
        "CastleBloodEssenceDrainModifier",
    ),
    ("castle_siege_timer", "CastleSiegeTimer"),
    ("castle_under_attack_timer", "CastleUnderAttackTimer"),
    ("castle_raid_timer", "CastleRaidTimer"),
    ("castle_raid_protection_time", "CastleRaidProtectionTime"),
    (
        "castle_exposed_free_claim_timer",
        "CastleExposedFreeClaimTimer",
    ),
    ("castle_relocation_cooldown", "CastleRelocationCooldown"),
    ("castle_relocation_enabled", "CastleRelocationEnabled"),
    ("announce_siege_weapon_spawn", "AnnounceSiegeWeaponSpawn"),
    ("show_siege_weapon_map_icon", "ShowSiegeWeaponMapIcon"),
    ("build_cost_modifier", "BuildCostModifier"),
    ("recipe_cost_modifier", "RecipeCostModifier"),
    ("craft_rate_modifier", "CraftRateModifier"),
    ("research_cost_modifier", "ResearchCostModifier"),
    ("refinement_cost_modifier", "RefinementCostModifier"),
    ("refinement_rate_modifier", "RefinementRateModifier"),
    ("research_time_modifier", "ResearchTimeModifier"),
    ("dismantle_resource_modifier", "DismantleResourceModifier"),
    (
        "servant_convert_rate_modifier",
        "ServantConvertRateModifier",
    ),
    ("repair_cost_modifier", "RepairCostModifier"),
    ("death_durability_factor_loss", "Death_DurabilityFactorLoss"),
    (
        "death_durability_loss_factor_as_resources",
        "Death_DurabilityLossFactorAsResources",
    ),
    ("starter_equipment_id", "StarterEquipmentId"),
    ("starter_resources_id", "StarterResourcesId"),
];

pub(super) const VRISING_GAME_TIME_SETTINGS: &[(&str, &str)] = &[
    ("day_duration_in_seconds", "DayDurationInSeconds"),
    ("day_start_hour", "DayStartHour"),
    ("day_start_minute", "DayStartMinute"),
    ("day_end_hour", "DayEndHour"),
    ("day_end_minute", "DayEndMinute"),
    ("blood_moon_frequency_min", "BloodMoonFrequency_Min"),
    ("blood_moon_frequency_max", "BloodMoonFrequency_Max"),
    ("blood_moon_buff", "BloodMoonBuff"),
];

pub(super) const VRISING_CASTLE_STAT_SETTINGS: &[(&str, &str)] = &[
    ("castle_tick_period", "TickPeriod"),
    ("safety_box_limit", "SafetyBoxLimit"),
    ("eye_structures_limit", "EyeStructuresLimit"),
    ("tomb_limit", "TombLimit"),
    ("vermin_nest_limit", "VerminNestLimit"),
    ("prison_cell_limit", "PrisonCellLimit"),
    ("castle_heart_limit_type", "CastleHeartLimitType"),
    ("castle_limit", "CastleLimit"),
    ("nether_gate_limit", "NetherGateLimit"),
    ("throne_of_darkness_limit", "ThroneOfDarknessLimit"),
    ("arena_station_limit", "ArenaStationLimit"),
    ("routing_station_limit", "RoutingStationLimit"),
];

pub(super) const VRISING_TRADER_MODIFIER_SETTINGS: &[(&str, &str)] = &[
    ("trader_stock_modifier", "StockModifier"),
    ("trader_price_modifier", "PriceModifier"),
    ("trader_restock_timer_modifier", "RestockTimerModifier"),
];

pub(super) const VRISING_WAR_EVENT_SETTINGS: &[(&str, &str)] = &[
    ("war_event_interval", "Interval"),
    ("war_event_major_duration", "MajorDuration"),
    ("war_event_minor_duration", "MinorDuration"),
];

pub(super) const VRISING_VAMPIRE_STAT_SETTINGS: &[(&str, &str)] = &[
    ("vampire_max_health_modifier", "MaxHealthModifier"),
    ("vampire_physical_power_modifier", "PhysicalPowerModifier"),
    ("vampire_spell_power_modifier", "SpellPowerModifier"),
    ("vampire_resource_power_modifier", "ResourcePowerModifier"),
    ("vampire_siege_power_modifier", "SiegePowerModifier"),
    ("vampire_damage_received_modifier", "DamageReceivedModifier"),
    ("vampire_revive_cancel_delay", "ReviveCancelDelay"),
];

pub(super) const VRISING_GLOBAL_UNIT_STAT_SETTINGS: &[(&str, &str)] = &[
    ("global_unit_max_health_modifier", "MaxHealthModifier"),
    ("global_unit_power_modifier", "PowerModifier"),
    ("global_unit_level_increase", "LevelIncrease"),
];

pub(super) const VRISING_VBLOOD_UNIT_STAT_SETTINGS: &[(&str, &str)] = &[
    ("vblood_unit_max_health_modifier", "MaxHealthModifier"),
    ("vblood_unit_power_modifier", "PowerModifier"),
    ("vblood_unit_level_increase", "LevelIncrease"),
];

pub(super) const VRISING_EQUIPMENT_STAT_SETTINGS: &[(&str, &str)] = &[
    ("global_equipment_max_health_modifier", "MaxHealthModifier"),
    (
        "global_equipment_resource_yield_modifier",
        "ResourceYieldModifier",
    ),
    (
        "global_equipment_physical_power_modifier",
        "PhysicalPowerModifier",
    ),
    (
        "global_equipment_spell_power_modifier",
        "SpellPowerModifier",
    ),
    (
        "global_equipment_siege_power_modifier",
        "SiegePowerModifier",
    ),
    (
        "global_equipment_movement_speed_modifier",
        "MovementSpeedModifier",
    ),
];

pub(super) const VRISING_CASTLE_HEART_LIMIT_SETTINGS: &[(&str, &str, &str)] = &[
    ("castle_heart_level_1_floor_limit", "Level1", "FloorLimit"),
    (
        "castle_heart_level_1_servant_limit",
        "Level1",
        "ServantLimit",
    ),
    ("castle_heart_level_1_height_limit", "Level1", "HeightLimit"),
    ("castle_heart_level_2_floor_limit", "Level2", "FloorLimit"),
    (
        "castle_heart_level_2_servant_limit",
        "Level2",
        "ServantLimit",
    ),
    ("castle_heart_level_2_height_limit", "Level2", "HeightLimit"),
    ("castle_heart_level_3_floor_limit", "Level3", "FloorLimit"),
    (
        "castle_heart_level_3_servant_limit",
        "Level3",
        "ServantLimit",
    ),
    ("castle_heart_level_3_height_limit", "Level3", "HeightLimit"),
    ("castle_heart_level_4_floor_limit", "Level4", "FloorLimit"),
    (
        "castle_heart_level_4_servant_limit",
        "Level4",
        "ServantLimit",
    ),
    ("castle_heart_level_4_height_limit", "Level4", "HeightLimit"),
    ("castle_heart_level_5_floor_limit", "Level5", "FloorLimit"),
    (
        "castle_heart_level_5_servant_limit",
        "Level5",
        "ServantLimit",
    ),
    ("castle_heart_level_5_height_limit", "Level5", "HeightLimit"),
];
