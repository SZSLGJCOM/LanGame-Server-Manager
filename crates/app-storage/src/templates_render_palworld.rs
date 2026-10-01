use super::*;

#[cfg(test)]
#[path = "templates_render_palworld_tests.rs"]
mod tests;

pub(super) fn render_palworld_option_settings(
    settings: &Map<String, Value>,
    ports: &[PortBinding],
) -> String {
    let mut entries = Vec::new();
    let public_port_default = lookup_port_number_text(ports, "game", "8211");
    let rcon_port = lookup_port_number_text(ports, "rcon", "25575");
    let rest_api_port = lookup_port_number_text(ports, "rest_api", "8212");

    push_palworld_raw_option(
        &mut entries,
        lookup_template_setting(settings, "randomizer_type")
            .unwrap_or_else(|| String::from("None")),
        "RandomizerType",
    );
    push_palworld_string_option(
        &mut entries,
        settings,
        "randomizer_seed",
        "RandomizerSeed",
        "",
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "is_randomizer_pal_level_random",
        "bIsRandomizerPalLevelRandom",
        false,
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "day_time_speed_rate",
        "DayTimeSpeedRate",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "night_time_speed_rate",
        "NightTimeSpeedRate",
        "1.0",
    );
    push_palworld_number_option(&mut entries, settings, "exp_rate", "ExpRate", "1.0");
    push_palworld_number_option(
        &mut entries,
        settings,
        "pal_capture_rate",
        "PalCaptureRate",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "pal_spawn_num_rate",
        "PalSpawnNumRate",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "pal_damage_rate_attack",
        "PalDamageRateAttack",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "pal_damage_rate_defense",
        "PalDamageRateDefense",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "player_damage_rate_attack",
        "PlayerDamageRateAttack",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "player_damage_rate_defense",
        "PlayerDamageRateDefense",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "player_stomach_decreace_rate",
        "PlayerStomachDecreaceRate",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "player_stamina_decreace_rate",
        "PlayerStaminaDecreaceRate",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "player_auto_hp_regene_rate",
        "PlayerAutoHPRegeneRate",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "player_auto_hp_regene_rate_in_sleep",
        "PlayerAutoHpRegeneRateInSleep",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "pal_stomach_decreace_rate",
        "PalStomachDecreaceRate",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "pal_stamina_decreace_rate",
        "PalStaminaDecreaceRate",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "pal_auto_hp_regene_rate",
        "PalAutoHPRegeneRate",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "pal_auto_hp_regene_rate_in_sleep",
        "PalAutoHpRegeneRateInSleep",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "build_object_hp_rate",
        "BuildObjectHpRate",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "build_object_damage_rate",
        "BuildObjectDamageRate",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "build_object_deterioration_damage_rate",
        "BuildObjectDeteriorationDamageRate",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "collection_drop_rate",
        "CollectionDropRate",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "collection_object_hp_rate",
        "CollectionObjectHpRate",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "collection_object_respawn_speed_rate",
        "CollectionObjectRespawnSpeedRate",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "enemy_drop_item_rate",
        "EnemyDropItemRate",
        "1.0",
    );
    push_palworld_enum_option(
        &mut entries,
        settings,
        "death_penalty",
        "DeathPenalty",
        "Item",
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "enable_player_to_player_damage",
        "bEnablePlayerToPlayerDamage",
        false,
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "enable_friendly_fire",
        "bEnableFriendlyFire",
        false,
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "enable_invader_enemy",
        "bEnableInvaderEnemy",
        true,
    );
    push_palworld_bool_option(&mut entries, settings, "active_unko", "bActiveUNKO", false);
    push_palworld_bool_option(
        &mut entries,
        settings,
        "enable_aim_assist_pad",
        "bEnableAimAssistPad",
        true,
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "enable_aim_assist_keyboard",
        "bEnableAimAssistKeyboard",
        false,
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "drop_item_max_num",
        "DropItemMaxNum",
        "3000",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "physics_active_drop_item_max_num",
        "PhysicsActiveDropItemMaxNum",
        "-1",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "drop_item_max_num_unko",
        "DropItemMaxNum_UNKO",
        "100",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "base_camp_max_num",
        "BaseCampMaxNum",
        "128",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "base_camp_max_num_in_guild",
        "BaseCampMaxNumInGuild",
        "4",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "base_camp_worker_max_num",
        "BaseCampWorkerMaxNum",
        "15",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "drop_item_alive_max_hours",
        "DropItemAliveMaxHours",
        "1.0",
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "auto_reset_guild_no_online_players",
        "bAutoResetGuildNoOnlinePlayers",
        false,
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "auto_reset_guild_time_no_online_players",
        "AutoResetGuildTimeNoOnlinePlayers",
        "72.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "guild_player_max_num",
        "GuildPlayerMaxNum",
        "20",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "pal_egg_default_hatching_time",
        "PalEggDefaultHatchingTime",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "work_speed_rate",
        "WorkSpeedRate",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "auto_save_span",
        "AutoSaveSpan",
        "30.0",
    );
    push_palworld_bool_option(&mut entries, settings, "is_pvp", "bIsPvP", false);
    push_palworld_bool_option(
        &mut entries,
        settings,
        "can_pickup_other_guild_death_penalty_drop",
        "bCanPickupOtherGuildDeathPenaltyDrop",
        false,
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "enable_fast_travel",
        "bEnableFastTravel",
        true,
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "enable_fast_travel_only_base_camp",
        "bEnableFastTravelOnlyBaseCamp",
        false,
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "is_start_location_select_by_map",
        "bIsStartLocationSelectByMap",
        false,
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "exist_player_after_logout",
        "bExistPlayerAfterLogout",
        false,
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "max_players",
        "ServerPlayerMaxNum",
        "32",
    );
    push_palworld_string_option(
        &mut entries,
        settings,
        "server_name",
        "ServerName",
        "Default Palworld Server",
    );
    push_palworld_string_option(
        &mut entries,
        settings,
        "server_description",
        "ServerDescription",
        "",
    );
    push_palworld_string_option(
        &mut entries,
        settings,
        "admin_password",
        "AdminPassword",
        "",
    );
    push_palworld_string_option(
        &mut entries,
        settings,
        "server_password",
        "ServerPassword",
        "",
    );
    push_palworld_string_option(&mut entries, settings, "public_ip", "PublicIP", "");
    push_palworld_number_option(
        &mut entries,
        settings,
        "public_port",
        "PublicPort",
        &public_port_default,
    );
    push_palworld_bool_option(&mut entries, settings, "rcon_enabled", "RCONEnabled", false);
    entries.push(format!("RCONPort={rcon_port}"));
    push_palworld_string_option(&mut entries, settings, "region", "Region", "");
    push_palworld_bool_option(&mut entries, settings, "use_auth", "bUseAuth", true);
    push_palworld_string_option(
        &mut entries,
        settings,
        "ban_list_url",
        "BanListURL",
        "https://b.palworldgame.com/api/banlist.txt",
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "rest_api_enabled",
        "RESTAPIEnabled",
        false,
    );
    entries.push(format!("RESTAPIPort={rest_api_port}"));
    push_palworld_bool_option(
        &mut entries,
        settings,
        "show_player_list",
        "bShowPlayerList",
        false,
    );
    push_palworld_raw_option(
        &mut entries,
        render_palworld_crossplay_platforms(settings),
        "CrossplayPlatforms",
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "use_backup_save_data",
        "bIsUseBackupSaveData",
        true,
    );
    push_palworld_raw_option(
        &mut entries,
        render_palworld_log_format(settings),
        "LogFormatType",
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "allow_client_mod",
        "bAllowClientMod",
        true,
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "join_left_message",
        "bIsShowJoinLeftMessage",
        true,
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "chat_post_limit_per_minute",
        "ChatPostLimitPerMinute",
        "30",
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "allow_enhance_stat_attack",
        "bAllowEnhanceStat_Attack",
        true,
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "allow_enhance_stat_health",
        "bAllowEnhanceStat_Health",
        true,
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "allow_enhance_stat_stamina",
        "bAllowEnhanceStat_Stamina",
        true,
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "allow_enhance_stat_weight",
        "bAllowEnhanceStat_Weight",
        true,
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "allow_enhance_stat_work_speed",
        "bAllowEnhanceStat_WorkSpeed",
        true,
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "allow_global_palbox_export",
        "bAllowGlobalPalboxExport",
        true,
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "allow_global_palbox_import",
        "bAllowGlobalPalboxImport",
        false,
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "build_area_limit",
        "bBuildAreaLimit",
        false,
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "character_recreate_in_hardcore",
        "bCharacterRecreateInHardcore",
        false,
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "display_pvp_item_num_on_world_map_base_camp",
        "bDisplayPvPItemNumOnWorldMap_BaseCamp",
        false,
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "display_pvp_item_num_on_world_map_player",
        "bDisplayPvPItemNumOnWorldMap_Player",
        false,
    );
    push_palworld_bool_option(&mut entries, settings, "hardcore", "bHardcore", false);
    push_palworld_number_option(
        &mut entries,
        settings,
        "block_respawn_time",
        "BlockRespawnTime",
        "5.0",
    );
    push_palworld_bool_option(&mut entries, settings, "pal_lost", "bPalLost", false);
    push_palworld_bool_option(
        &mut entries,
        settings,
        "invisible_other_guild_base_camp_area_fx",
        "bInvisibleOtherGuildBaseCampAreaFX",
        false,
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "item_weight_rate",
        "ItemWeightRate",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "equipment_durability_damage_rate",
        "EquipmentDurabilityDamageRate",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "item_corruption_multiplier",
        "ItemCorruptionMultiplier",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "monster_farm_action_speed_rate",
        "MonsterFarmActionSpeedRate",
        "1.0",
    );
    push_palworld_raw_option(
        &mut entries,
        render_palworld_deny_technology_list(settings),
        "DenyTechnologyList",
    );
    if let Some(difficulty) = settings
        .get("fishing_difficulty_rate")
        .and_then(Value::as_f64)
    {
        entries.push(format!("FishingDifficultyRate={difficulty}"));
    }
    // The publisher does not specify defaults for these settings; omission preserves game defaults.
    match settings
        .get("enemy_camp_spawn_near_base")
        .and_then(Value::as_str)
    {
        Some("allow") => entries.push(String::from("bAllowEnemyCampSpawnNearBaseCamp=True")),
        Some("prevent") => entries.push(String::from("bAllowEnemyCampSpawnNearBaseCamp=False")),
        _ => {}
    }
    push_palworld_number_option(
        &mut entries,
        settings,
        "supply_drop_span",
        "SupplyDropSpan",
        "180.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "guild_rejoin_cooldown_minutes",
        "GuildRejoinCooldownMinutes",
        "0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "auto_transfer_master_check_interval_seconds",
        "AutoTransferMasterCheckIntervalSeconds",
        "3600.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "auto_transfer_master_threshold_days",
        "AutoTransferMasterThresholdDays",
        "14",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "max_guilds_per_frame",
        "MaxGuildsPerFrame",
        "10",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "max_building_limit_num",
        "MaxBuildingLimitNum",
        "0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "max_building_limit_num_per_player",
        "MaxBuildingLimitNumPerPlayer",
        "0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "server_replicate_pawn_cull_distance",
        "ServerReplicatePawnCullDistance",
        "15000",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "item_container_force_mark_dirty_interval",
        "ItemContainerForceMarkDirtyInterval",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "player_data_pal_storage_update_check_tick_interval",
        "PlayerDataPalStorageUpdateCheckTickInterval",
        "1.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "respawn_penalty_duration_threshold",
        "RespawnPenaltyDurationThreshold",
        "0.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "respawn_penalty_time_scale",
        "RespawnPenaltyTimeScale",
        "2.0",
    );
    push_palworld_string_option(
        &mut entries,
        settings,
        "additional_drop_item_when_player_killing_in_pvp_mode",
        "AdditionalDropItemWhenPlayerKillingInPvPMode",
        "PlayerDropItem",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "additional_drop_item_num_when_player_killing_in_pvp_mode",
        "AdditionalDropItemNumWhenPlayerKillingInPvPMode",
        "1",
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "additional_drop_item_when_player_killing_in_pvp_mode_enabled",
        "bAdditionalDropItemWhenPlayerKillingInPvPMode",
        false,
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "enable_voice_chat",
        "bEnableVoiceChat",
        false,
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "voice_chat_max_volume_distance",
        "VoiceChatMaxVolumeDistance",
        "3000.0",
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "voice_chat_zero_volume_distance",
        "VoiceChatZeroVolumeDistance",
        "15000.0",
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "enable_predator_boss_pal",
        "EnablePredatorBossPal",
        true,
    );
    push_palworld_bool_option(
        &mut entries,
        settings,
        "enable_building_player_uid_display",
        "bEnableBuildingPlayerUIdDisplay",
        false,
    );
    push_palworld_number_option(
        &mut entries,
        settings,
        "building_name_display_cache_ttl_seconds",
        "BuildingNameDisplayCacheTTLSeconds",
        "60",
    );

    entries.join(",")
}

pub(super) fn render_palworld_mod_settings_ini(settings: &Map<String, Value>) -> String {
    let package_names = normalize_palworld_mod_package_names(settings);
    let mut lines = vec![
        String::from("[PalModSettings]"),
        format!(
            "bGlobalEnableMod={}",
            if package_names.is_empty() {
                "false"
            } else {
                "true"
            }
        ),
    ];
    lines.extend(
        package_names
            .into_iter()
            .map(|package_name| format!("ActiveModList={package_name}")),
    );
    lines.join("\n")
}

fn normalize_palworld_mod_package_names(settings: &Map<String, Value>) -> Vec<String> {
    let raw = lookup_palworld_string_text(settings, "mod_package_names", "");
    let mut seen = std::collections::HashSet::new();
    let mut package_names = Vec::new();
    for candidate in raw
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split(['\n', ',', ';'])
        .map(str::trim)
        .filter(|candidate| is_palworld_package_name(candidate))
    {
        if seen.insert(candidate.to_ascii_lowercase()) {
            package_names.push(candidate.to_string());
        }
    }
    package_names
}

fn is_palworld_package_name(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && trimmed != "."
        && trimmed != ".."
        && trimmed.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
        })
}

pub(super) fn push_palworld_raw_option(entries: &mut Vec<String>, value: String, output_key: &str) {
    entries.push(format!("{output_key}={value}"));
}

pub(super) fn push_palworld_string_option(
    entries: &mut Vec<String>,
    settings: &Map<String, Value>,
    settings_key: &str,
    output_key: &str,
    default_value: &str,
) {
    let value = lookup_palworld_string_text(settings, settings_key, default_value);
    entries.push(format!(
        "{output_key}=\"{}\"",
        escape_palworld_ini_string(&value)
    ));
}

pub(super) fn push_palworld_enum_option(
    entries: &mut Vec<String>,
    settings: &Map<String, Value>,
    settings_key: &str,
    output_key: &str,
    default_value: &str,
) {
    let value = lookup_palworld_string_text(settings, settings_key, default_value);
    push_palworld_raw_option(entries, value.trim().to_string(), output_key);
}

pub(super) fn push_palworld_number_option(
    entries: &mut Vec<String>,
    settings: &Map<String, Value>,
    settings_key: &str,
    output_key: &str,
    default_value: &str,
) {
    let value = lookup_palworld_number_text(settings, settings_key, default_value);
    entries.push(format!("{output_key}={value}"));
}

pub(super) fn push_palworld_bool_option(
    entries: &mut Vec<String>,
    settings: &Map<String, Value>,
    settings_key: &str,
    output_key: &str,
    default_value: bool,
) {
    let value = lookup_ini_bool_text(settings, settings_key, default_value);
    entries.push(format!("{output_key}={value}"));
}

pub(super) fn lookup_palworld_string_text(
    settings: &Map<String, Value>,
    key: &str,
    default_value: &str,
) -> String {
    settings
        .get(key)
        .and_then(|value| match value {
            Value::Null => None,
            Value::String(text) => Some(text.clone()),
            other => Some(other.to_string()),
        })
        .unwrap_or_else(|| String::from(default_value))
}

pub(super) fn lookup_palworld_number_text(
    settings: &Map<String, Value>,
    key: &str,
    default_value: &str,
) -> String {
    settings
        .get(key)
        .and_then(|value| match value {
            Value::Number(number) => Some(format_palworld_number_text(number, default_value)),
            Value::String(text) => {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(String::from(trimmed))
                }
            }
            Value::Bool(boolean) => Some(if *boolean {
                String::from("1")
            } else {
                String::from("0")
            }),
            _ => None,
        })
        .unwrap_or_else(|| String::from(default_value))
}

pub(super) fn format_palworld_number_text(
    number: &serde_json::Number,
    default_value: &str,
) -> String {
    let rendered = number.to_string();
    if default_value.contains('.') && !rendered.contains('.') && !rendered.contains('e') {
        format!("{rendered}.0")
    } else {
        rendered
    }
}

pub(super) fn lookup_ini_bool_text(
    settings: &Map<String, Value>,
    key: &str,
    default_value: bool,
) -> String {
    let boolean = settings
        .get(key)
        .and_then(|value| match value {
            Value::Bool(boolean) => Some(*boolean),
            Value::Number(number) => number.as_i64().map(|numeric| numeric != 0),
            Value::String(text) => {
                let trimmed = text.trim();
                if trimmed.eq_ignore_ascii_case("true")
                    || trimmed.eq_ignore_ascii_case("1")
                    || trimmed.eq_ignore_ascii_case("yes")
                    || trimmed.eq_ignore_ascii_case("on")
                {
                    Some(true)
                } else if trimmed.eq_ignore_ascii_case("false")
                    || trimmed.eq_ignore_ascii_case("0")
                    || trimmed.eq_ignore_ascii_case("no")
                    || trimmed.eq_ignore_ascii_case("off")
                {
                    Some(false)
                } else {
                    None
                }
            }
            _ => None,
        })
        .unwrap_or(default_value);

    if boolean {
        String::from("True")
    } else {
        String::from("False")
    }
}
