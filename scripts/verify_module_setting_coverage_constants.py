from __future__ import annotations

import re

NEW_MODULE_IDS = {
    "astroneer",
    "barotrauma",
    "humanitz",
    "nightingale",
    "returntomoria",
    "rimworld",
    "romestead",
    "runescapedragonwilds",
    "scum",
    "sonsoftheforest",
    "soulmask",
    "squad",
    "theforest",
    "windrose",
}

STRICT_MODULE_IDS = NEW_MODULE_IDS | {
    "abioticfactor",
    "arksurvivalascended",
    "arksurvivalevolved",
    "unturned",
    "conanexiles",
    "corekeeper",
    "dontstarve",
    "enshrouded",
    "humanitz",
    "minecraft",
    "necesse",
    "projectzomboid",
    "palworld",
    "rust",
    "satisfactory",
    "sevendaystodie",
    "terraria",
    "valheim",
    "vrising",
}

SCHEMA_COVERAGE_MODULE_IDS = STRICT_MODULE_IDS

TARGET_MODULE_IDS = {
    "abioticfactor",
    "arksurvivalascended",
    "arksurvivalevolved",
    "astroneer",
    "barotrauma",
    "conanexiles",
    "corekeeper",
    "dontstarve",
    "enshrouded",
    "humanitz",
    "minecraft",
    "necesse",
    "nightingale",
    "palworld",
    "projectzomboid",
    "returntomoria",
    "rimworld",
    "romestead",
    "runescapedragonwilds",
    "rust",
    "satisfactory",
    "scum",
    "sevendaystodie",
    "sonsoftheforest",
    "soulmask",
    "squad",
    "terraria",
    "theforest",
    "unturned",
    "valheim",
    "vrising",
    "windrose",
}

SETTING_TOKEN_RE = re.compile(
    r"(?:^|[\s\"'=])(?:(?:json|xml)\.)?settings\.([A-Za-z0-9_]+)(?:$|[\s\"'])"
)
PORT_TOKEN_RE = re.compile(r"(?:^|[\s\"'=])ports\.([A-Za-z0-9_]+)\.port(?:$|[\s\"'])")
TEMPLATE_TOKEN_RE = re.compile(r"\{\{\s*([^}]+?)\s*\}\}")
GENERIC_SCHEMA_SECTIONS = {
    "room",
    "basics",
    "network",
    "runtime",
    "world",
    "access",
    "admin",
    "performance",
    "advanced",
}
FRONTEND_SECTION_ORDER_OVERRIDE_MARKERS = (
    "FIELD_SECTION_MAP",
    "FIELD_ORDER",
    "resolveFieldSectionId",
    "resolveFieldSortWeight",
)

MODULE_SETTINGS_FILE_NAMES = {
    "arksurvivalascended": "ark-asa",
    "arksurvivalevolved": "ark-ase",
}

MODULE_SETTINGS_ADDITIONAL_SOURCES = {
    "conanexiles": (
        "views/settings/modules/conanexiles-exact-groups.ts",
    ),
    "arksurvivalascended": (
        "views/settings/modules/ark-definition-shared.ts",
        "views/settings/modules/ark-asa-groups-foundation.ts",
        "views/settings/modules/ark-asa-groups-rules.ts",
        "views/settings/modules/ark-asa-official-inventory.ts",
    ),
    "arksurvivalevolved": (
        "views/settings/modules/ark-definition-shared.ts",
        "views/settings/modules/ark-ase-groups-foundation.ts",
        "views/settings/modules/ark-ase-groups-rules.ts",
        "views/settings/modules/ark-ase-official-inventory.ts",
    ),
    "soulmask": (
        "views/settings/modules/soulmask-groups.ts",
    ),
}

MODULE_SCHEMA_USAGE_ADDITIONAL_SOURCES = {
    "arksurvivalevolved": ("crates/app-core/src/ark_maps.rs",),
    "arksurvivalascended": ("crates/app-core/src/ark_maps.rs",),
    "unturned": ("apps/desktop/src-tauri/src/commands_managed_save.rs",),
}

MODULE_TOKEN_PREFIX_ALIASES = {
    "arksurvivalascended": {"ark", "arksa"},
    "arksurvivalevolved": {"ark", "arkse"},
    "dontstarve": {"dst"},
}

DERIVED_TOKEN_SETTING_COVERAGE: dict[tuple[str, str], set[str]] = {
    ("astroneer", "console_port"): {"console_password"},
    ("abioticfactor", "admin_ini_flag"): set(),
    ("abioticfactor", "admin_password_flag"): {"admin_password"},
    ("abioticfactor", "lan_only_flag"): {"lan_only"},
    ("abioticfactor", "moderator_lines"): {"moderator_steam_ids"},
    ("abioticfactor", "multihome_flag"): set(),
    ("abioticfactor", "no_async_loading_thread_flag"): {"disable_async_loading_thread"},
    ("abioticfactor", "platform_limited_flag"): {"platform_limited"},
    ("abioticfactor", "sandbox_ini_flag"): set(),
    ("abioticfactor", "server_password_flag"): {"server_password"},
    ("abioticfactor", "use_local_ips_flag"): {"use_local_ips"},
    ("abioticfactor", "use_perf_threads_flag"): {"use_perf_threads"},
    ("ark", "config_add_npc_spawn_entries_container_lines"): {
        "config_add_npc_spawn_entries_container"
    },
    ("ark", "config_override_item_crafting_costs_lines"): {
        "config_override_item_crafting_costs"
    },
    ("ark", "config_override_item_max_quantity_lines"): {"config_override_item_max_quantity"},
    ("ark", "config_override_npc_spawn_entries_container_lines"): {
        "config_override_npc_spawn_entries_container"
    },
    ("ark", "config_override_supply_crate_items_lines"): {
        "config_override_supply_crate_items"
    },
    ("dontstarve", "launch_args"): {
        "allow_ioopenwrite_sandbox_escape",
        "backup_log_count",
        "backup_log_period",
        "disable_data_collection",
        "friends_only",
    },
    ("ark", "config_subtract_npc_spawn_entries_container_lines"): {
        "config_subtract_npc_spawn_entries_container"
    },
    ("ark", "dino_class_damage_multipliers_lines"): {"dino_class_damage_multipliers"},
    ("ark", "dino_class_resistance_multipliers_lines"): {
        "dino_class_resistance_multipliers"
    },
    ("ark", "dino_spawn_weight_multipliers_lines"): {"dino_spawn_weight_multipliers"},
    ("ark", "engram_entry_auto_unlock_lines"): {"engram_entry_auto_unlocks"},
    ("ark", "level_experience_ramp_overrides_lines"): {"level_experience_ramp_overrides"},
    ("ark", "npc_replacements_lines"): {"npc_replacements"},
    ("ark", "override_named_engram_entries_lines"): {"override_named_engram_entries"},
    ("ark", "override_player_level_engram_points_lines"): {
        "override_player_level_engram_points"
    },
    ("ark", "prevent_transfer_for_class_names_lines"): {"prevent_transfer_for_class_names"},
    ("ark", "account_id_lines admin_account_ids"): {"admin_account_ids"},
    ("ark", "account_id_lines exclusive_join_list"): {"exclusive_join_list"},
    ("ark", "account_id_lines priority_join_list"): {"priority_join_list"},
    ("ark", "steam64_lines admin_account_ids"): {"admin_account_ids"},
    ("ark", "steam64_lines exclusive_join_list"): {"exclusive_join_list"},
    ("ark", "steam64_lines priority_join_list"): {"priority_join_list"},
    ("ark", "tamed_dino_class_damage_multipliers_lines"): {
        "tamed_dino_class_damage_multipliers"
    },
    ("ark", "tamed_dino_class_resistance_multipliers_lines"): {
        "tamed_dino_class_resistance_multipliers"
    },
    ("arksa", "cluster_dir_override_flag"): {"cluster_id", "cluster_directory"},
    ("arksa", "custom_launch_flags"): {"custom_launch_flags"},
    ("arksa", "mod_ids_flag"): {"mod_ids_csv"},
    ("arksa", "multihome_flag"): set(),
    ("arksa", "native_log_path"): set(),
    ("arksa", "server_url"): {
        "admin_password",
        "map_name",
        "max_players",
        "rcon_enabled",
        "server_name",
        "server_password",
    },
    ("arkse", "cluster_dir_override_flag"): {"cluster_id", "cluster_directory"},
    ("arkse", "custom_launch_flags"): {"custom_launch_flags"},
    ("arkse", "multihome_flag"): set(),
    ("arkse", "native_log_path"): set(),
    ("arkse", "server_url"): {
        "event_colors_chance_override",
        "map_name",
        "new_year1_utc",
        "new_year2_utc",
    },
    ("barotrauma", "client_permissions_xml"): {"admin_entries"},
    ("conanexiles", "custom_launch_flags"): {"custom_launch_flags"},
    ("conanexiles", "multihome_flag"): set(),
    ("corekeeper", "admins_document_json"): {"admin_list"},
    ("corekeeper", "bans_document_json"): {"ban_list"},
    ("corekeeper", "direct_allowed_platform_flag"): {"direct_connection_enabled"},
    ("corekeeper", "direct_allowed_platform_value"): {
        "allowed_platform_code",
        "direct_connection_enabled",
    },
    ("corekeeper", "direct_ip_flag"): {"direct_connection_enabled"},
    ("corekeeper", "direct_ip_value"): {"direct_connection_enabled"},
    ("corekeeper", "direct_password_flag"): {"direct_connection_enabled"},
    ("corekeeper", "direct_password_value"): {
        "direct_connection_enabled",
        "join_password",
    },
    ("corekeeper", "direct_port_flag"): {"direct_connection_enabled"},
    ("corekeeper", "direct_port_value"): {"direct_connection_enabled"},
    ("corekeeper", "effective_game_id_json"): {"game_id"},
    ("corekeeper", "log_path"): set(),
    ("dst", "admin_list_lines"): {"admin_list"},
    ("dst", "blocklist_lines"): {"blocklist"},
    ("dst", "whitelist_lines"): {"whitelist"},
    ("enshrouded", "bans_json"): {"banned_player_ids"},
    ("enshrouded", "day_time_ns"): {"day_time_minutes"},
    ("enshrouded", "extra_user_groups_json_entries"): {"custom_user_groups_json"},
    ("enshrouded", "hunger_to_starving_ns"): {"hunger_to_starving_minutes"},
    ("enshrouded", "night_time_ns"): {"night_time_minutes"},
    ("enshrouded", "tags_json"): {"server_tags"},
    ("humanitz", "admin_list_lines"): {"admin_steam_ids"},
    ("humanitz", "reserved_player_lines"): {"reserved_player_steam_ids"},
    ("humanitz", "banned_player_lines"): {"banned_player_steam_ids"},
    ("humanitz", "settings_extra_lines"): {"settings_extra"},
    ("sonsoftheforest", "owner_whitelist_lines"): {"owner_whitelist_steam_ids"},
    ("theforest", "allow_cheats_on_off"): {"allow_cheats"},
    ("theforest", "building_destruction_on_off"): {"building_destruction"},
    ("theforest", "enemies_in_creative_on_off"): {"enemies_in_creative"},
    ("theforest", "realistic_player_damage_on_off"): {"realistic_player_damage"},
    ("theforest", "reset_holes_on_load_on_off"): {"reset_holes_on_load"},
    ("theforest", "show_logs_on_off"): {"show_logs"},
    ("theforest", "tree_regrowth_on_off"): {"tree_regrowth"},
    ("theforest", "vac_enabled_on_off"): {"vac_enabled"},
    ("theforest", "vegan_mode_on_off"): {"vegan_mode"},
    ("theforest", "vegetarian_mode_on_off"): {"vegetarian_mode"},
    ("minecraft", "banned_ips_json"): {"banned_ip_entries"},
    ("minecraft", "banned_players_json"): {"banned_player_entries"},
    ("minecraft", "extra_properties_lines"): {"extra_properties"},
    ("minecraft", "ops_json"): {"operator_entries"},
    ("minecraft", "server_ip"): set(),
    ("minecraft", "whitelist_json"): {"whitelist_entries"},
    ("necesse", "custom_launch_flags"): {"custom_launch_flags"},
    ("necesse", "ignore_seasons_flag"): {"ignore_seasons"},
    ("necesse", "logging_enabled_value"): {"logging_enabled"},
    ("necesse", "owner_args"): {"owner_name"},
    ("necesse", "pause_when_empty_value"): {"pause_when_empty"},
    ("necesse", "strict_server_authority_value"): {"strict_server_authority"},
    ("necesse", "zip_saves_value"): {"zip_saves"},
    ("palworld", "no_async_loading_thread_flag"): {"launch_perf_threads"},
    ("palworld", "public_ip_flag"): {"community_server", "public_ip"},
    ("palworld", "public_lobby_flag"): {"community_server"},
    ("palworld", "public_port_flag"): {"community_server", "public_port"},
    ("palworld", "use_multithread_for_ds_flag"): {"launch_perf_threads"},
    ("palworld", "use_perf_threads_flag"): {"launch_perf_threads"},
    ("palworld", "gamedata_api_flag"): {"gamedata_api_enabled"},
    ("palworld", "worker_thread_count_flag"): {
        "launch_worker_threads_enabled",
        "worker_thread_count",
    },
    ("projectzomboid", "classpath"): set(),
    ("projectzomboid", "seed_line"): {"world_seed"},
    ("projectzomboid", "map_list"): {"map_name"},
    ("projectzomboid", "mods"): {"mods"},
    ("projectzomboid", "welcome_message"): {"welcome_message"},
    ("projectzomboid", "workshop_items"): {"workshop_items"},
    ("romestead", "world_seed_json"): {"auto_create_world_seed"},
    ("astroneer", "extra_launch_args"): {"extra_launch_args"},
    ("nightingale", "extra_launch_args"): {"extra_launch_args"},
    ("romestead", "extra_launch_args"): {"extra_launch_args"},
    ("runescapedragonwilds", "extra_launch_args"): {"extra_launch_args"},
    ("runescapedragonwilds", "owner_id"): {"owner_id"},
    ("runescapedragonwilds", "platform_policy_line"): {"platform_policy"},
    ("rust", "app_listen_ip_line"): {"app_listen_ip"},
    ("rust", "app_port_line"): {"app_port"},
    ("rust", "app_public_ip_line"): {"app_public_ip"},
    ("rust", "ban_lines"): {"banned_entries"},
    ("rust", "bans_cfg_extra_lines"): {"bans_cfg_extra"},
    ("rust", "bans_server_endpoint_line"): {"bans_server_endpoint"},
    ("rust", "custom_launch_flags"): {"custom_launch_flags"},
    ("rust", "favorites_endpoint_line"): {"favorites_endpoint"},
    ("rust", "level_url_line"): {"level_url"},
    ("rust", "logo_image_line"): {"logo_image_url"},
    ("rust", "moderator_lines"): {"moderator_entries"},
    ("rust", "owner_lines"): {"owner_entries"},
    ("rust", "reports_server_endpoint_key_line"): {"reports_server_endpoint_key"},
    ("rust", "reports_server_endpoint_line"): {"reports_server_endpoint"},
    ("rust", "seed_line"): {"level_url", "seed"},
    ("rust", "server_gamemode_line"): {"server_gamemode"},
    ("rust", "skip_queue_lines"): {"skip_queue_entries"},
    ("rust", "insecure_flag"): {"secure"},
    ("rust", "use_new_navmesh_flag"): {"use_new_navmesh"},
    ("rust", "users_cfg_extra_lines"): {"users_cfg_extra"},
    ("rust", "wipe_cron_override_line"): {"wipe_cron_override"},
    ("rust", "wipe_timezone_line"): {"wipe_timezone"},
    ("rust", "wipe_unix_timestamp_override_line"): {"wipe_unix_timestamp_override"},
    ("rust", "world_configfile_args"): {"world_config_json"},
    ("rust", "world_size_line"): {"level_url", "world_size"},
    ("nightingale", "enable_cheats_flag"): {"enable_cheats"},
    ("nightingale", "json_logging_args"): {"json_logging"},
    ("nightingale", "status_endpoint_args"): {"status_endpoint_enabled"},
    ("satisfactory", "crash_reporting_value"): {"disable_crash_reporting"},
    ("satisfactory", "custom_launch_flags"): {"custom_launch_flags"},
    ("satisfactory", "disable_packet_routing_flag"): {"disable_packet_routing"},
    ("satisfactory", "disable_seasonal_events_flag"): {"disable_seasonal_events"},
    ("satisfactory", "external_reliable_port_flag"): {"external_reliable_port"},
    ("satisfactory", "insecure_local_api_flag"): {"allow_insecure_local_api"},
    ("soulmask", "workshop_mods_arg"): {"mod_workshop_ids"},
    ("terraria", "launch_args"): {"server_runtime", "tmodloader_runtime_dir"},
    ("terraria", "server_executable"): {"server_runtime", "tmodloader_runtime_dir"},
    ("terraria", "working_directory"): {"server_runtime", "tmodloader_runtime_dir"},
    ("scum", "admin_steam_ids_lines"): {"admin_steam_ids"},
    ("scum", "server_settings_ini"): {
        "server_general",
        "server_world",
        "server_features",
        "server_respawn",
        "server_vehicles",
        "server_damage",
    },
    ("scum", "economy_override_json"): {"economy_override"},
    ("scum", "raid_times_json"): {"raid_times"},
    ("scum", "notifications_json"): {"notifications"},
    ("squad", "admins_cfg"): {
        "admin_permissions",
        "admin_steam_ids",
        "admins_cfg",
        "priority_join_steam_ids",
    },
    ("sevendaystodie", "admin_group_lines"): {"admin_groups"},
    ("sevendaystodie", "admin_user_lines"): {"admin_users"},
    ("sevendaystodie", "blacklist_lines"): {"blacklist_entries"},
    ("sevendaystodie", "permission_lines"): {"command_permissions"},
    ("sevendaystodie", "whitelist_group_lines"): {"whitelist_groups"},
    ("sevendaystodie", "whitelist_user_lines"): {"whitelist_users"},
    ("terraria", "announcementboxrange_line"): {"announcementboxrange"},
    ("terraria", "banlist_lines"): {"banlist_entries"},
    ("terraria", "disableannouncementbox_line"): {"disableannouncementbox"},
    ("terraria", "lobby_line"): {"lobby", "steam"},
    ("terraria", "password_line"): {"password"},
    ("terraria", "secure_line"): {"secure"},
    ("terraria", "seed_line"): {"seed"},
    ("terraria", "slowliquids_line"): {"slowliquids"},
    ("terraria", "special_seed_line"): {"special_seed"},
    ("terraria", "steam_line"): {"steam"},
    ("terraria", "upnp_line"): {"upnp"},
    ("unturned", "admin_lines"): {"admin_steam_ids"},
    ("unturned", "cheats_line"): {"cheats"},
    ("unturned", "custom_launch_flags"): {"custom_launch_flags"},
    ("unturned", "gameplay_config_no_empty_values_flag"): {
        "gameplay_config_no_empty_values"
    },
    ("unturned", "gameplay_config_no_generated_comments_flag"): {
        "gameplay_config_no_generated_comments"
    },
    ("unturned", "hide_admins_line"): {"hide_admins"},
    ("unturned", "log_gameplay_config_flag"): {"log_gameplay_config"},
    ("unturned", "no_level_config_overrides_flag"): {"no_level_config_overrides"},
    ("unturned", "owner_line"): {"owner_steam_id"},
    ("unturned", "password_line"): {"password"},
    ("unturned", "pve_line"): {"pve"},
    ("unturned", "server_launch_mode"): {"internet_server"},
    ("unturned", "whitelist_line"): {"whitelist_enabled"},
    ("unturned", "workshop_file_ids_json"): {"workshop_file_ids"},
    ("unturned", "workshop_ignore_children_file_ids_json"): {
        "workshop_ignore_children_file_ids"
    },
    ("valheim", "admin_list_lines"): {"admin_list"},
    ("valheim", "banned_list_lines"): {"banned_list"},
    ("valheim", "crossplay_flag"): {"crossplay_enabled"},
    ("valheim", "custom_launch_flags"): {"custom_launch_flags"},
    ("valheim", "instance_id_args"): {"instance_id"},
    ("valheim", "log_file_args"): {"log_file"},
    ("valheim", "permitted_list_lines"): {"permitted_list"},
    ("valheim", "world_modifier_args"): {"world_modifiers"},
    ("valheim", "world_preset_args"): {"world_preset"},
    ("valheim", "world_setkey_args"): {"world_set_keys"},
    ("vrising", "admin_list_lines"): {"admin_list"},
    ("vrising", "ban_list_lines"): {"ban_list"},
    ("vrising", "bind_address_flag"): set(),
    ("vrising", "bind_address_value"): set(),
    ("windrose", "direct_connection_proxy_address_json"): {"direct_connection_proxy_address"},
    ("windrose", "is_password_protected"): {"server_password"},
    ("windrose", "p2p_proxy_address_json"): {"p2p_proxy_address"},
}

DERIVED_TOKEN_SCHEMA_REMAINDER_EXCLUSIONS: dict[tuple[str, str], set[str]] = {
    ("palworld", "option_settings"): {
        "community_server",
        "launch_perf_threads",
        "launch_worker_threads_enabled",
        "mod_package_names",
        "worker_thread_count",
    },
    ("vrising", "server_game_settings_json"): {
        "admin_list",
        "admin_only_debug_events",
        "api_enabled",
        "autosave_count",
        "autosave_interval_seconds",
        "autosave_smart_keep",
        "ban_list",
        "compress_save_files",
        "disable_debug_events",
        "game_difficulty_preset",
        "game_settings_preset",
        "hide_ip_address",
        "list_on_eos",
        "list_on_steam",
        "max_admins",
        "max_players",
        "rcon_enabled",
        "rcon_password",
        "save_name",
        "secure_mode",
        "server_description",
        "server_fps",
        "server_name",
        "server_password",
    },
}

DERIVED_TOKEN_RUST_FUNCTION_COVERAGE = {
    ("vrising", "optional_host_settings_members"): (
        "render_vrising_optional_host_settings_members", ()
    ),
    ("dst", "cluster_intention_line"): (
        "render_dst_cluster_intention_line",
        ("cluster_",),
    ),
}

DERIVED_TOKEN_PORT_COVERAGE: dict[tuple[str, str], set[str]] = {
    ("astroneer", "console_port"): {"console"},
    ("palworld", "option_settings"): {"game", "rcon", "rest_api"},
}

MATERIALIZATION_RUST_FUNCTION_COVERAGE = {
    "satisfactory": (("plan_user_settings", ()),),
    "rimworld": (("render_rimworld_password_json", ()), ("rimworld_native_setting_specs", ())),
    "necesse": (("render_necesse_server_settings", ()),),
    "barotrauma": (("materialize_barotrauma_workshop_mods", ()),),
    "conanexiles": (("materialize_conan_modlist", ()),),
    "dontstarve": (("render_dst_mod_setup", ("shared_",)),),
    "palworld": (
        ("render_palworld_mod_settings_ini", ()),
        ("normalize_palworld_mod_package_names", ()),
    ),
    "terraria": (
        ("render_terraria_tmodloader_install_txt", ()),
        ("parse_terraria_tmodloader_enabled_mod_names", ()),
    ),
    "windrose": (("render_world_description", ()),),
}

DST_ROSTER_TOKEN_COVERAGE = {
    "admin_list": "admin_list_lines",
    "whitelist": "whitelist_lines",
    "blocklist": "blocklist_lines",
}

DST_ROSTER_GUARD_MARKERS = (
    '"admin_list_lines" => Some(render_dst_klei_user_id_lines(settings, "admin_list"))',
    '"whitelist_lines" => Some(render_dst_klei_user_id_lines(settings, "whitelist"))',
    '"blocklist_lines" => Some(render_dst_klei_user_id_lines(settings, "blocklist"))',
    "fn normalize_dst_klei_id(raw: &str) -> Option<String>",
    "fn render_dst_klei_user_id_lines(settings: &Map<String, Value>, key: &str) -> String",
    "fn dst_roster_lists_filter_to_klei_user_ids()",
)

SEVENDAYSTODIE_SERVERADMIN_OBJECT_ROSTER_RENDERERS = {
    "admin_users": "render_sevendaystodie_admin_user_lines",
    "admin_groups": "render_sevendaystodie_admin_group_lines",
    "whitelist_users": "render_sevendaystodie_whitelist_user_lines",
    "whitelist_groups": "render_sevendaystodie_whitelist_group_lines",
    "blacklist_entries": "render_sevendaystodie_blacklist_lines",
}

MINECRAFT_NATIVE_JSON_ROSTER_RENDERERS = {
    "operator_entries": ("render_minecraft_ops_json", "normalize_minecraft_uuid"),
    "whitelist_entries": ("render_minecraft_named_uuid_json", "normalize_minecraft_uuid"),
    "banned_player_entries": (
        "render_minecraft_banned_players_json",
        "normalize_minecraft_uuid",
    ),
    "banned_ip_entries": ("render_minecraft_banned_ips_json", "normalize_minecraft_banned_ip"),
}

RUST_CFG_ROSTER_RENDERERS = {
    "owner_entries": "render_rust_user_lines",
    "moderator_entries": "render_rust_user_lines",
    "skip_queue_entries": "render_rust_skip_queue_lines",
    "banned_entries": "render_rust_ban_lines",
}

UNTURNED_COMMANDS_DAT_ROSTER_GUARD_MARKERS = (
    '"owner_line" => Some(render_unturned_owner_line(settings)),',
    "fn render_unturned_owner_line(settings: &Map<String, Value>) -> String",
    'parse_steam64_lines(settings, "admin_steam_ids")',
    "fn unturned_tokens_render_admin_lines_and_browser_fallbacks()",
)

STEAM64_TEXT_ROSTER_GUARD_MARKERS = (
    "fn render_abioticfactor_moderator_lines(settings: &Map<String, Value>) -> String",
    "fn abioticfactor_moderator_lines_deduplicate_comments_and_noop_invalid_entries()",
    'parse_steam64_lines(settings, "owner_whitelist_steam_ids").join("\\n")',
    'parse_steam64_lines(settings, "admin_steam_ids").join("\\n")',
    "fn sonsoftheforest_tokens_render_deduplicated_owner_steam64_lines()",
    "fn scum_tokens_render_deduplicated_admin_ids()",
)

JSON_ACCOUNT_ROSTER_GUARD_MARKERS = (
    "fn parse_steam64_values_from_text(raw: &str) -> Vec<String>",
    "fn normalize_corekeeper_identifier_list(raw: &str) -> Vec<String>",
    "parse_steam64_values_from_text(raw)",
    "fn corekeeper_identifier_list_deduplicates_and_filters_invalid_entries()",
    "fn enshrouded_banned_accounts_render_exact_native_uint64_hashes()",
    "fn enshrouded_invalid_account_hashes_fail_rendering_without_changing_files()",
)

VALHEIM_ROSTER_TOKEN_COVERAGE = {
    "admin_list": "admin_list_lines",
    "banned_list": "banned_list_lines",
    "permitted_list": "permitted_list_lines",
}

SQUAD_ADMINS_CFG_GUARD_MARKERS = (
    "fn normalize_squad_group_name(value: &str) -> Option<String>",
    "fn normalize_squad_extra_admin_line(line: &str) -> Option<String>",
    "let (group_name, permission_blob) = body.split_once(':')?;",
    "filter_map(|line| normalize_squad_extra_admin_line(&line))",
    "seen.insert(line.to_ascii_lowercase())",
    "fn squad_admins_cfg_renders_structured_admins_and_reserved_users()",
)

TERRARIA_BANLIST_GUARD_MARKERS = (
    "fn render_terraria_banlist_lines(settings: &Map<String, Value>) -> String",
    "fn normalize_terraria_banlist_entry(raw: &str) -> Option<String>",
    "fn terraria_banlist_lines_filter_comments_duplicates_and_template_tokens()",
)

VRISING_ROSTER_GUARD_MARKERS = (
    '"admin_list_lines" => Some(render_vrising_steam64_lines(settings, "admin_list"))',
    '"ban_list_lines" => Some(render_vrising_steam64_lines(settings, "ban_list"))',
    "fn render_vrising_steam64_lines(settings: &Map<String, Value>, key: &str) -> String",
    "fn vrising_roster_lists_filter_to_steam64_lines()",
)

BAROTRAUMA_CLIENT_PERMISSIONS_GUARD_MARKERS = (
    '"client_permissions_xml" => Some(render_barotrauma_client_permissions_xml(settings))',
    "fn normalize_barotrauma_account(raw: &str) -> Option<String>",
    "fn render_barotrauma_client_permissions_xml(settings: &Map<String, Value>) -> String",
    "fn barotrauma_client_permissions_render_admin_entries()",
)

PLAYER_ACCESS_KINDS = {"admin", "allow", "block", "priority"}
PLAYER_ACCESS_MANAGEMENT_KINDS = {"admin", "allow", "block"}
PLAYER_ROSTER_IDENTITY_PROPERTY_PRIORITY = (
    "steam_id",
    "steamid",
    "steam64_id",
    "steam64",
    "account_id",
    "user_id",
    "userid",
    "player_id",
    "playerid",
    "uuid",
    "xuid",
    "id",
    "name",
)
PLAYER_ROSTER_STEAM_IDENTITY_KEYS = {"steam_id", "steamid", "steam64_id", "steam64"}
PLAYER_MANAGEMENT_STATUSES = {"pending_adapter", "persistent_roster", "runtime_actions"}
PLAYER_MANAGEMENT_PENDING_GAP_RE = re.compile(
    r"(cannot|do not|does not|missing|no |not |pending|unverified|without|待|未|没有|不能|不可|缺)",
    re.I,
)
PLAYER_MANAGEMENT_PENDING_GATE_RE = re.compile(
    r"(before (adding|declaring|enabling|exposing)|keep .*pending|only enable|only expose|until|不得|才能|之前)",
    re.I,
)
PLAYER_MANAGEMENT_ROSTER_SURFACE_RE = re.compile(
    r"(admin|allow|ban|block|cfg|cluster|commands|config|file|ini|json|launch|list|owner|permission|roster|settings|txt|user|whitelist|xml)",
    re.I,
)
PLAYER_MANAGEMENT_ROSTER_MATERIALIZATION_RE = re.compile(
    r"(copy|file|json|launch|load|materiali[sz]e|read|render|schema-backed|sync|write|written|writes|cfg|ini|txt|xml|复制|文件|加载|启动|读取|渲染|物化|同步|写入)",
    re.I,
)
PLAYER_MANAGEMENT_RUNTIME_SURFACE_RE = re.compile(
    r"(BattlEye|command|console|port|RCON|RCon|stdin|Telnet|transport|WebRCON|WebSocket)",
    re.I,
)
PLAYER_MANAGEMENT_RUNTIME_READ_RE = re.compile(
    r"(clientlist|list|lp|online|output|players?|playing|show|snapshot|status|users?)",
    re.I,
)
PLAYER_MANAGEMENT_RUNTIME_EFFECT_RE = re.compile(
    r"(access|admin|allow|ban|block|deop|kick|moderator|op|owner|permission|permit|side effects|unadmin|unban|unpermit|whitelist)",
    re.I,
)
PLAYER_MANAGEMENT_PERSISTENT_FILE_RE = re.compile(
    r"(\.cfg|\.ini|\.json|\.txt|\.xml|Admin.ini|Admins.json|PlayerBans.json|serveradmin|serverconfig|whitelist)",
    re.I,
)
PLAYER_MANAGEMENT_PERSISTENT_MATERIALIZATION_RE = re.compile(
    r"(confirm|contains|copy|file|materiali[sz]e|read|render|schema-backed|sync|write|written|writes)",
    re.I,
)
PLAYER_MANAGEMENT_PERSISTENT_EFFECT_RE = re.compile(
    r"(access|admin|ban|block|can join|grant|owner|permission|permit|privileges|role|whitelist)",
    re.I,
)
PLAYER_ACTION_TARGET_ENCODINGS = {"raw", "quoted_string"}
PLAYER_ACTION_TEMPLATE_TOKENS = {"target", "role"}
PLAYER_ACTION_TRANSPORT_METADATA_KEYS = (
    "process_key",
    "port_name",
    "password_setting_key",
    "enabled_setting_key",
)
PLAYER_ACTION_REMOTE_TRANSPORTS = {"source_rcon", "websocket_rcon", "battleye_rcon", "humanitz_rcon", "telnet", "palworld_rest"}
PLAYER_ACTION_RCON_TRANSPORTS = {"source_rcon", "websocket_rcon", "battleye_rcon", "humanitz_rcon"}
PLAYER_ACTION_TRANSPORT_SURFACE_TOKENS = {
    "stdin": ("stdin", "console", "terminal"),
    "source_rcon": ("source_rcon",),
    "websocket_rcon": ("websocket_rcon",),
    "battleye_rcon": ("battleye_rcon",),
    "humanitz_rcon": ("humanitz_rcon",),
    "telnet": ("telnet",),
    "palworld_rest": ("rest",),
}
PLAYER_ACTION_TARGET_IDENTITY_RE = re.compile(
    r"(account|auth|ban|character|client|entity|guid|id|ip|klei|ku_|name|number|output|player|steam|user|userid|username)",
    re.I,
)
PLAYER_ACTION_RAW_NAME_TARGET_SOURCE_RE = re.compile(
    r"(auth|from|guid|id|list|number|output|players?|playing|status|steam|user\s*id|userid|username|uuid|xuid)",
    re.I,
)
PLAYER_ACTION_QUOTED_TEXT_TARGET_RE = re.compile(
    r"(account|auth|character|exact|guid|ip|name|or|platform|slg|username)",
    re.I,
)
PLAYER_ACTION_RAW_NAME_TARGET_RE = re.compile(
    r"(character name|player name|exact [a-z ]*name)",
    re.I,
)
PLAYER_ACTION_SINGLE_TOKEN_TARGET_RE = re.compile(
    r"(single[- ]token|no[- ]whitespace|username|单\s*token|用户名|不含\s*空格|无\s*空格|单个?词)",
    re.I,
)
PLAYER_ACTION_ZH_NAME_PLACEHOLDER_RE = re.compile(r"(玩家名|玩家名称|角色名|角色名称)")
PLAYER_ACTION_ROLE_VALUE_RE = re.compile(r"[A-Za-z0-9_.:-]+")
PLAYER_ROSTER_MATERIALIZATION_DESCRIPTION_RE = re.compile(
    r"(copy|file|json|materiali[sz]e|render|sync|write|written|writes|写|复制|渲染|物化|同步|文件)",
    re.I,
)
PLAYER_ROSTER_TARGET_DESCRIPTION_RE = re.compile(
    r"(\.(?:cfg|dat|ini|json|lua|properties|txt|xml)\b|-owner\b|"
    r"bannedAccounts|bans\s*(?:array|数组)|Commands\.dat|serveradmin|serverconfig)",
    re.I,
)
REMOTE_ACTION_SETTING_MATERIALIZATION_RE = re.compile(
    r"(\.cfg\b|\.ini\b|\.json\b|\.properties\b|\.xml\b|\bcfg\b|\bini\b|\bjson\b|\bproperties\b|\bxml\b|"
    r"\blaunch\b|\bmateriali[sz]e\b|\bpass(?:ed|es)?\b|\brender(?:ed|s)?\b|\bwritten\b|\bwrites\b|"
    r"BEServer|GameServerSettings|OptionSettings|PalWorldSettings|Rcon\.cfg|"
    r"ServerHostSettings|server\.cfg|server\.ini|server\.properties|serverconfig)",
    re.I,
)
REMOTE_ACTION_SETTING_EFFECT_RE = re.compile(
    r"(admin|authenticated|BattlEye|ban|command|console|kick|list|listener|moderation|player|RCON|RCon|remote|Telnet|WebSocket)",
    re.I,
)
PLAYER_ACTION_READ_PREFIXES = ("list_", "show_")
PLAYER_ACTION_READ_IDS = {
    "status",
    "players",
    "users",
    "banlistex",
    "check_permission",
}
PLAYER_ACTION_NON_MUTATING_IDS = PLAYER_ACTION_READ_IDS
PLAYER_ACTION_TARGETLESS_MUTATING_IDS = {"reload_banlist", "writecfg", "save_world"}
PLAYER_ACTION_MUTATING_PREFIXES = (
    "add_",
    "allow_",
    "ban_",
    "deop_",
    "demote_",
    "disallow_",
    "grant_",
    "kick_",
    "moderator",
    "op_",
    "owner",
    "permit_",
    "promote_",
    "remove",
    "revoke_",
    "set_",
    "unban_",
    "unpermit_",
    "whitelist_",
)
__all__ = [
    "NEW_MODULE_IDS",
    "STRICT_MODULE_IDS",
    "SCHEMA_COVERAGE_MODULE_IDS",
    "TARGET_MODULE_IDS",
    "SETTING_TOKEN_RE",
    "PORT_TOKEN_RE",
    "TEMPLATE_TOKEN_RE",
    "GENERIC_SCHEMA_SECTIONS",
    "FRONTEND_SECTION_ORDER_OVERRIDE_MARKERS",
    "MODULE_SETTINGS_FILE_NAMES",
    "MODULE_SETTINGS_ADDITIONAL_SOURCES",
    "MODULE_SCHEMA_USAGE_ADDITIONAL_SOURCES",
    "MODULE_TOKEN_PREFIX_ALIASES",
    "DERIVED_TOKEN_SETTING_COVERAGE",
    "DERIVED_TOKEN_SCHEMA_REMAINDER_EXCLUSIONS",
    "DERIVED_TOKEN_RUST_FUNCTION_COVERAGE",
    "DERIVED_TOKEN_PORT_COVERAGE",
    "MATERIALIZATION_RUST_FUNCTION_COVERAGE",
    "DST_ROSTER_TOKEN_COVERAGE",
    "DST_ROSTER_GUARD_MARKERS",
    "SEVENDAYSTODIE_SERVERADMIN_OBJECT_ROSTER_RENDERERS",
    "MINECRAFT_NATIVE_JSON_ROSTER_RENDERERS",
    "RUST_CFG_ROSTER_RENDERERS",
    "UNTURNED_COMMANDS_DAT_ROSTER_GUARD_MARKERS",
    "STEAM64_TEXT_ROSTER_GUARD_MARKERS",
    "JSON_ACCOUNT_ROSTER_GUARD_MARKERS",
    "VALHEIM_ROSTER_TOKEN_COVERAGE",
    "SQUAD_ADMINS_CFG_GUARD_MARKERS",
    "TERRARIA_BANLIST_GUARD_MARKERS",
    "VRISING_ROSTER_GUARD_MARKERS",
    "BAROTRAUMA_CLIENT_PERMISSIONS_GUARD_MARKERS",
    "PLAYER_ACCESS_KINDS",
    "PLAYER_ACCESS_MANAGEMENT_KINDS",
    "PLAYER_ROSTER_IDENTITY_PROPERTY_PRIORITY",
    "PLAYER_ROSTER_STEAM_IDENTITY_KEYS",
    "PLAYER_MANAGEMENT_STATUSES",
    "PLAYER_MANAGEMENT_PENDING_GAP_RE",
    "PLAYER_MANAGEMENT_PENDING_GATE_RE",
    "PLAYER_MANAGEMENT_ROSTER_SURFACE_RE",
    "PLAYER_MANAGEMENT_ROSTER_MATERIALIZATION_RE",
    "PLAYER_MANAGEMENT_RUNTIME_SURFACE_RE",
    "PLAYER_MANAGEMENT_RUNTIME_READ_RE",
    "PLAYER_MANAGEMENT_RUNTIME_EFFECT_RE",
    "PLAYER_MANAGEMENT_PERSISTENT_FILE_RE",
    "PLAYER_MANAGEMENT_PERSISTENT_MATERIALIZATION_RE",
    "PLAYER_MANAGEMENT_PERSISTENT_EFFECT_RE",
    "PLAYER_ACTION_TARGET_ENCODINGS",
    "PLAYER_ACTION_TEMPLATE_TOKENS",
    "PLAYER_ACTION_TRANSPORT_METADATA_KEYS",
    "PLAYER_ACTION_REMOTE_TRANSPORTS",
    "PLAYER_ACTION_RCON_TRANSPORTS",
    "PLAYER_ACTION_TRANSPORT_SURFACE_TOKENS",
    "PLAYER_ACTION_TARGET_IDENTITY_RE",
    "PLAYER_ACTION_RAW_NAME_TARGET_SOURCE_RE",
    "PLAYER_ACTION_QUOTED_TEXT_TARGET_RE",
    "PLAYER_ACTION_RAW_NAME_TARGET_RE",
    "PLAYER_ACTION_SINGLE_TOKEN_TARGET_RE",
    "PLAYER_ACTION_ZH_NAME_PLACEHOLDER_RE",
    "PLAYER_ACTION_ROLE_VALUE_RE",
    "PLAYER_ROSTER_MATERIALIZATION_DESCRIPTION_RE",
    "PLAYER_ROSTER_TARGET_DESCRIPTION_RE",
    "REMOTE_ACTION_SETTING_MATERIALIZATION_RE",
    "REMOTE_ACTION_SETTING_EFFECT_RE",
    "PLAYER_ACTION_READ_PREFIXES",
    "PLAYER_ACTION_READ_IDS",
    "PLAYER_ACTION_NON_MUTATING_IDS",
    "PLAYER_ACTION_TARGETLESS_MUTATING_IDS",
    "PLAYER_ACTION_MUTATING_PREFIXES",
]
