//! Product command registration, execution ownership and transport policy.
//! Parameter decoding stays in the transport adapters; service-control messages
//! are internal protocol operations and are not exposed as product commands.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Execution {
    Desktop,
    Runtime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LanAccess {
    Allowed,
    Denied,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Completion {
    Bounded,
    Owned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Command {
    pub(crate) execution: Execution,
    pub(crate) lan: LanAccess,
    pub(crate) completion: Completion,
}

// The same identifiers register Tauri handlers and select execution policy.
// A desktop-owned command can also have an authenticated LAN implementation.
macro_rules! register_commands {
    ($(($name:ident, [$($module:ident)::+], $execution:ident, $lan:ident, $completion:ident)),+ $(,)?) => {
        pub(crate) fn find(name: &str) -> Option<Command> {
            match name {
                $(stringify!($name) => Some(Command {
                    execution: Execution::$execution,
                    lan: LanAccess::$lan,
                    completion: Completion::$completion,
                }),)+
                _ => None,
            }
        }

        pub(crate) fn invoke_handler(invoke: tauri::ipc::Invoke<tauri::Wry>) -> bool {
            #[cfg(windows)]
            if !matches!(find(invoke.message.command()), Some(Command { execution: Execution::Desktop, .. })) {
                crate::runtime_service::forward(invoke);
                return true;
            }
            let handler: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool =
                tauri::generate_handler![$($($module)::+::$name),+];
            handler(invoke)
        }
    };
}

register_commands! {
    (set_tray_locale, [crate], Desktop, Denied, Bounded),
    (app_exit_status, [crate], Desktop, Denied, Bounded),
    (check_app_update, [crate::app_updates], Desktop, Denied, Bounded),
    (install_app_update, [crate::app_updates], Desktop, Denied, Bounded),
    (app_version, [crate::commands], Runtime, Allowed, Bounded),
    (bootstrap, [crate::commands], Runtime, Allowed, Bounded),
    (read_background_jobs, [crate::commands], Runtime, Allowed, Bounded),
    (refresh_modules, [crate::commands], Runtime, Allowed, Bounded),
    (read_module_details, [crate::commands], Runtime, Allowed, Bounded),
    (register_media_cache_source, [crate::media_cache], Desktop, Allowed, Bounded),
    (read_module_configuration_icons, [crate::commands::commands_configuration_icons], Runtime, Allowed, Bounded),
    (search_steam_workshop_items, [crate::commands], Runtime, Allowed, Bounded),
    (lookup_steam_workshop_items, [crate::commands], Runtime, Allowed, Bounded),
    (read_steam_workshop_item_details, [crate::commands], Runtime, Allowed, Bounded),
    (read_steam_workshop_installation_status, [crate::commands], Runtime, Allowed, Bounded),
    (read_dontstarve_mod_configuration_specs, [crate::commands], Runtime, Allowed, Bounded),
    (read_project_zomboid_workshop_mods_snapshot, [crate::commands], Runtime, Allowed, Bounded),
    (fetch_steam_news_for_app, [crate::commands], Runtime, Allowed, Bounded),
    (fetch_steam_store_about, [crate::commands], Runtime, Allowed, Bounded),
    (fetch_steam_review_summary, [crate::commands], Runtime, Allowed, Bounded),
    (open_external_url, [crate::commands], Desktop, Allowed, Bounded),
    (open_local_path, [crate::commands], Desktop, Allowed, Bounded),
    (probe_steamcmd_status, [crate::commands], Runtime, Allowed, Bounded),
    (ensure_steamcmd_ready, [crate::commands::commands_steamcmd_preparation], Runtime, Allowed, Bounded),
    (read_steamcmd_prepare_progress, [crate::commands::commands_steamcmd_preparation], Runtime, Allowed, Bounded),
    (cancel_steamcmd_preparation, [crate::commands::commands_steamcmd_preparation], Runtime, Allowed, Bounded),
    (cancel_installation_job, [crate::commands::commands_install_progress], Runtime, Allowed, Bounded),
    (uninstall_steamcmd, [crate::commands], Runtime, Denied, Bounded),
    (update_app_settings, [crate::commands], Runtime, Allowed, Bounded),
    (pick_directory_path, [crate::commands], Desktop, Allowed, Bounded),
    (install_module_game, [crate::commands], Runtime, Allowed, Bounded),
    (validate_module_game, [crate::commands], Runtime, Allowed, Bounded),
    (update_instance_program, [crate::commands::commands_program_storage], Runtime, Allowed, Bounded),
    (uninstall_module_game, [crate::commands], Runtime, Allowed, Bounded),
    (download_steam_workshop_items, [crate::commands], Runtime, Allowed, Bounded),
    (start_steam_workshop_account_login, [crate::commands::commands_steam_workshop_account], Runtime, Denied, Bounded),
    (read_steam_workshop_account_login, [crate::commands::commands_steam_workshop_account], Runtime, Denied, Bounded),
    (send_steam_workshop_account_input, [crate::commands::commands_steam_workshop_account], Runtime, Denied, Bounded),
    (cancel_steam_workshop_account_login, [crate::commands::commands_steam_workshop_account], Runtime, Denied, Bounded),
    (download_authenticated_steam_workshop_items, [crate::commands::commands_steam_workshop_account], Runtime, Denied, Owned),
    (install_authenticated_module_game, [crate::commands::commands_steam_app_account], Runtime, Denied, Owned),
    (update_authenticated_instance_program, [crate::commands::commands_steam_app_account], Runtime, Denied, Owned),
    (clear_steam_server_account_authorization, [crate::commands::commands_steam_app_account], Runtime, Denied, Bounded),
    (remove_instance_workshop_collection, [crate::commands::commands_workshop_collection_removal], Runtime, Allowed, Bounded),
    (stage_manual_mod_files, [crate::commands], Runtime, Allowed, Bounded),
    (install_manual_mod_references, [crate::commands], Runtime, Allowed, Owned),
    (nexus_account_status, [crate::commands::commands_nexus_mods], Runtime, Denied, Bounded),
    (nexus_store_api_key, [crate::commands::commands_nexus_mods], Runtime, Denied, Bounded),
    (nexus_clear_api_key, [crate::commands::commands_nexus_mods], Runtime, Denied, Bounded),
    (review_nexus_mod_references, [crate::commands::commands_nexus_mods], Runtime, Denied, Bounded),
    (install_nexus_mod_files, [crate::commands::commands_nexus_mods], Runtime, Denied, Owned),
    (read_manual_mod_inventory, [crate::commands], Runtime, Allowed, Bounded),
    (manage_manual_mod, [crate::commands::commands_file_mods], Runtime, Allowed, Owned),
    (read_instance_ark_map_availability, [crate::commands::commands_ark_map_availability], Runtime, Allowed, Bounded),
    (resolve_manual_mod_references, [crate::commands], Runtime, Allowed, Bounded),
    (preview_instance_launch, [crate::commands::commands_runtime_lifecycle], Runtime, Allowed, Bounded),
    (start_instance_process, [crate::commands::commands_runtime_lifecycle], Runtime, Allowed, Bounded),
    (stop_instance_process, [crate::commands::commands_runtime_lifecycle], Runtime, Allowed, Bounded),
    (send_instance_runtime_command, [crate::commands::commands_assistant_ops], Runtime, Allowed, Bounded),
    (send_instance_gm_command, [crate::commands::commands_assistant_ops], Runtime, Allowed, Bounded),
    (read_ark_tools_status, [crate::commands::commands_ark_tools], Runtime, Denied, Bounded),
    (prepare_ark_tools, [crate::commands::commands_ark_tools], Runtime, Denied, Bounded),
    (spawn_ark_creature, [crate::commands::commands_ark_tools], Runtime, Denied, Bounded),
    (read_instance_live_players, [crate::commands::commands_live_players], Runtime, Allowed, Bounded),
    (read_instance_dragonwilds_bans, [crate::commands::commands_dragonwilds_bans], Runtime, Allowed, Bounded),
    (read_instance_native_bans, [crate::commands::commands_native_bans], Runtime, Allowed, Bounded),
    (unban_instance_native_ban, [crate::commands::commands_native_bans], Runtime, Allowed, Owned),
    (refresh_instance_live_players, [crate::commands::commands_live_players], Runtime, Allowed, Bounded),
    (execute_instance_player_action, [crate::commands::commands_live_players], Runtime, Allowed, Bounded),
    (execute_instance_manual_player_action, [crate::commands::commands_manual_player_actions], Runtime, Allowed, Bounded),
    (suppress_instance_runtime_windows, [crate::commands::commands_storage], Runtime, Allowed, Bounded),
    (ensure_storage_ready, [crate::commands::commands_storage], Runtime, Allowed, Bounded),
    (sync_modules_to_storage, [crate::commands::commands_storage], Runtime, Allowed, Bounded),
    (list_instances_from_storage, [crate::commands::commands_storage], Runtime, Allowed, Bounded),
    (read_instance_connection_info_from_storage, [crate::commands::commands_instance_network], Runtime, Allowed, Bounded),
    (update_instance_autostart, [crate::commands::commands_autostart], Runtime, Allowed, Bounded),
    (read_instance_world_save_readiness, [crate::commands::commands_storage], Runtime, Allowed, Bounded),
    (read_instance_details_from_storage, [crate::commands::commands_storage], Runtime, Allowed, Bounded),
    (read_instance_isolation, [crate::commands::commands_instance_isolation], Runtime, Allowed, Bounded),
    (read_dragonwilds_world_settings, [crate::commands::commands_dragonwilds_world], Runtime, Allowed, Bounded),
    (write_dragonwilds_world_settings, [crate::commands::commands_dragonwilds_world], Runtime, Allowed, Bounded),
    (read_astroneer_save_catalog, [crate::commands::commands_astroneer_saves], Runtime, Allowed, Bounded),
    (read_valheim_world_rules, [crate::commands::commands_valheim_world], Runtime, Allowed, Bounded),
    (prepare_valheim_player_control, [crate::commands::commands_valheim_control], Runtime, Allowed, Owned),
    (read_satisfactory_world_settings, [crate::commands::commands_satisfactory_world], Runtime, Allowed, Bounded),
    (setup_satisfactory_server, [crate::commands::commands_satisfactory_world], Runtime, Allowed, Bounded),
    (authorize_satisfactory_server, [crate::commands::commands_satisfactory_world], Runtime, Allowed, Bounded),
    (read_satisfactory_admin_password, [crate::commands::commands_satisfactory_world], Runtime, Allowed, Bounded),
    (write_satisfactory_world_rules, [crate::commands::commands_satisfactory_world], Runtime, Allowed, Bounded),
    (create_satisfactory_world, [crate::commands::commands_satisfactory_world], Runtime, Allowed, Bounded),
    (write_satisfactory_room, [crate::commands::commands_satisfactory_world], Runtime, Allowed, Bounded),
    (load_satisfactory_save, [crate::commands::commands_satisfactory_world], Runtime, Allowed, Bounded),
    (read_ark_cluster, [crate::commands::commands_ark_clusters], Runtime, Allowed, Bounded),
    (operate_ark_cluster, [crate::commands::commands_ark_clusters], Runtime, Allowed, Owned),
    (list_ark_cluster_backups, [crate::commands::commands_ark_cluster_backups], Runtime, Denied, Owned),
    (create_ark_cluster_backup, [crate::commands::commands_ark_cluster_backups], Runtime, Denied, Owned),
    (restore_ark_cluster_backup, [crate::commands::commands_ark_cluster_backups], Runtime, Denied, Owned),
    (read_pending_ark_cluster_restore, [crate::commands::commands_ark_cluster_backups], Runtime, Denied, Owned),
    (recover_ark_cluster_restore, [crate::commands::commands_ark_cluster_backups], Runtime, Denied, Owned),
    (create_instance_backup, [crate::commands::commands_storage], Runtime, Allowed, Bounded),
    (list_instance_backups, [crate::commands::commands_storage], Runtime, Allowed, Bounded),
    (rename_instance_backup, [crate::commands::commands_storage], Runtime, Allowed, Bounded),
    (delete_instance_backup, [crate::commands::commands_storage], Runtime, Allowed, Bounded),
    (restore_instance_backup, [crate::commands::commands_storage], Runtime, Allowed, Bounded),
    (list_instance_save_files, [crate::commands::commands_save_files], Runtime, Denied, Bounded),
    (read_instance_save_import_support, [crate::commands::commands_save_import], Runtime, Denied, Bounded),
    (preview_instance_save_import, [crate::commands::commands_save_import], Runtime, Denied, Owned),
    (import_instance_save, [crate::commands::commands_save_import], Runtime, Denied, Owned),
    (read_instance_runtime_overview_from_storage, [crate::commands::commands_storage], Runtime, Allowed, Bounded),
    (read_instance_runtime_window_snapshot, [crate::commands::commands_storage], Runtime, Allowed, Bounded),
    (read_palworld_operator_snapshot, [crate::commands::commands_storage], Runtime, Allowed, Bounded),
    (read_sevendaystodie_operator_snapshot, [crate::commands::commands_storage], Runtime, Allowed, Bounded),
    (read_instance_log_document_from_storage, [crate::commands::commands_storage], Runtime, Allowed, Bounded),
    (create_instance_record, [crate::commands::commands_storage], Runtime, Allowed, Bounded),
    (log_frontend_event, [crate::commands::commands_runtime_supervision], Runtime, Allowed, Bounded),
    (update_instance_record_if_current, [crate::commands::commands_storage], Runtime, Allowed, Bounded),
    (apply_instance_player_access_mutation, [crate::commands::commands_player_access], Runtime, Allowed, Bounded),
    (archive_instance_record, [crate::commands::commands_instance_retirement], Runtime, Denied, Owned),
    (delete_instance_record, [crate::commands::commands_instance_retirement], Runtime, Allowed, Owned),
    (scan_storage_usage, [crate::commands::commands_storage_management], Runtime, Denied, Bounded),
    (inspect_module_programs, [crate::commands::commands_program_inventory], Runtime, Allowed, Bounded),
    (inspect_instance_removal, [crate::commands::commands_program_inventory], Runtime, Allowed, Bounded),
    (read_knowledge_status, [crate::commands_knowledge], Runtime, Denied, Bounded),
    (update_knowledge_settings, [crate::commands_knowledge], Runtime, Denied, Bounded),
    (start_knowledge_sync, [crate::commands_knowledge], Runtime, Denied, Bounded),
    (cancel_knowledge_sync, [crate::commands_knowledge], Runtime, Denied, Bounded),
    (cancel_storage_usage_scan, [crate::commands::commands_storage_management], Runtime, Denied, Bounded),
    (list_instance_archives, [crate::commands::commands_storage_management], Runtime, Denied, Owned),
    (read_instance_archive_details, [crate::commands::commands_storage_management], Runtime, Denied, Bounded),
    (restore_instance_archive, [crate::commands::commands_storage_management], Runtime, Denied, Owned),
    (purge_instance_archive, [crate::commands::commands_storage_management], Runtime, Denied, Owned),
    (import_dontstarve_world_data, [crate::commands::commands_storage], Runtime, Allowed, Bounded),
    (preview_dontstarve_world_start, [crate::commands::commands_dst_world_state], Runtime, Denied, Bounded),
    (regenerate_dontstarve_world, [crate::commands::commands_dst_regeneration], Runtime, Denied, Owned),
    (read_dontstarve_world_map, [crate::commands::commands_dst_map], Runtime, Allowed, Bounded),
    (read_instance_maintenance_schedule, [crate::commands::commands_maintenance_schedule], Runtime, Allowed, Bounded),
    (update_instance_maintenance_schedule, [crate::commands::commands_maintenance_schedule], Runtime, Allowed, Bounded),
    (read_instance_player_history, [crate::commands::commands_player_history], Runtime, Allowed, Bounded),
    (overlay_families, [crate::commands::commands_runtime_supervision], Runtime, Allowed, Bounded),
    (bind_address_candidates, [crate::commands::commands_runtime_supervision], Runtime, Allowed, Bounded),
    (assistant_secret_status, [crate::commands::commands_assistant_ops], Runtime, Allowed, Bounded),
    (assistant_store_secret, [crate::commands::commands_assistant_ops], Runtime, Allowed, Bounded),
    (assistant_clear_secret, [crate::commands::commands_assistant_ops], Runtime, Allowed, Bounded),
    (assistant_list_ollama_models, [crate::commands::commands_assistant_ops], Runtime, Allowed, Bounded),
    (assistant_check_connection, [crate::commands_assistant_connection], Runtime, Allowed, Bounded),
    (assistant_cancel_connection_check, [crate::commands_assistant_connection], Runtime, Allowed, Bounded),
    (assistant_run, [crate::commands::commands_assistant_ops], Runtime, Allowed, Bounded),
    (assistant_execute_operation, [crate::commands::commands_assistant_ops], Runtime, Allowed, Bounded),
    (assistant_confirm_operation, [crate::commands::commands_assistant_ops], Runtime, Allowed, Bounded),
    (assistant_create_conversation, [crate::commands::commands_assistant_ops], Runtime, Allowed, Bounded),
    (assistant_list_conversations, [crate::commands::commands_assistant_ops], Runtime, Allowed, Bounded),
    (assistant_cancel_turn, [crate::commands::commands_assistant_ops], Runtime, Allowed, Bounded),
    (assistant_delete_conversation, [crate::commands::commands_assistant_ops], Runtime, Allowed, Bounded),
    (assistant_resume_conversation, [crate::commands::commands_assistant_ops], Runtime, Allowed, Bounded),
    (assistant_get_conversation_state, [crate::commands::commands_assistant_ops], Runtime, Allowed, Bounded),
    (generate_instance_broadcast, [crate::commands::commands_broadcast], Runtime, Allowed, Bounded),
    (send_instance_broadcast, [crate::commands::commands_broadcast], Runtime, Allowed, Bounded),
    (read_instance_broadcast_policy, [crate::commands::commands_broadcast], Runtime, Allowed, Bounded),
    (update_instance_broadcast_policy, [crate::commands::commands_broadcast], Runtime, Allowed, Bounded),
    (list_instance_broadcast_events, [crate::commands::commands_broadcast], Runtime, Allowed, Bounded),
}

#[cfg(test)]
mod tests;
