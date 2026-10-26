use super::*;

#[test]
fn desktop_execution_and_lan_access_are_independent() {
    for name in [
        "set_tray_locale",
        "app_exit_status",
        "check_app_update",
        "install_app_update",
    ] {
        let command = find(name).unwrap();
        assert_eq!(command.execution, Execution::Desktop, "{name}");
        assert_eq!(command.lan, LanAccess::Denied, "{name}");
    }
    // These existing LAN operations execute in the desktop only when invoked
    // by its own WebView. Execution ownership must not silently revoke access.
    for name in [
        "register_media_cache_source",
        "pick_directory_path",
        "open_external_url",
        "open_local_path",
    ] {
        let command = find(name).unwrap();
        assert_eq!(command.execution, Execution::Desktop, "{name}");
        assert_eq!(command.lan, LanAccess::Allowed, "{name}");
    }
}

#[test]
fn current_user_maintenance_cannot_be_exposed_through_lan() {
    for name in [
        "uninstall_steamcmd",
        "preview_dontstarve_world_start",
        "regenerate_dontstarve_world",
        "read_ark_tools_status",
        "prepare_ark_tools",
        "spawn_ark_creature",
        "read_knowledge_status",
        "update_knowledge_settings",
        "start_knowledge_sync",
        "cancel_knowledge_sync",
        "scan_storage_usage",
        "cancel_storage_usage_scan",
        "archive_instance_record",
        "list_instance_archives",
        "read_instance_archive_details",
        "restore_instance_archive",
        "purge_instance_archive",
        "list_ark_cluster_backups",
        "create_ark_cluster_backup",
        "restore_ark_cluster_backup",
        "read_pending_ark_cluster_restore",
        "recover_ark_cluster_restore",
    ] {
        let command = find(name).unwrap();
        assert_eq!(command.execution, Execution::Runtime, "{name}");
        assert_eq!(command.lan, LanAccess::Denied, "{name}");
    }
    for (name, completion) in [
        ("start_instance_process", Completion::Owned),
        ("read_instance_details_from_storage", Completion::Bounded),
        ("read_dontstarve_world_map", Completion::Bounded),
    ] {
        let command = find(name).unwrap();
        assert_eq!(command.execution, Execution::Runtime, "{name}");
        assert_eq!(command.lan, LanAccess::Allowed, "{name}");
        assert_eq!(command.completion, completion, "{name}");
    }
}

#[test]
fn service_control_and_unknown_names_are_not_product_commands() {
    for name in [
        "runtime_service_status",
        "runtime_service_events",
        "runtime_service_shutdown",
        "runtime_service_tray_exit",
        "unknown_command",
        "START_INSTANCE_PROCESS",
        "import_dontstarve_world_map_artwork",
        "read_dontstarve_map_client_package",
    ] {
        assert_eq!(find(name), None, "{name}");
    }
}

#[test]
fn dst_world_regeneration_completes_under_runtime_ownership() {
    assert_eq!(
        find("regenerate_dontstarve_world").unwrap().completion,
        Completion::Owned
    );
}

#[test]
fn installation_and_save_mutations_keep_completion_ownership() {
    for name in [
        "ensure_steamcmd_ready",
        "create_instance_backup",
        "delete_instance_backup",
        "restore_instance_backup",
        "import_dontstarve_world_data",
    ] {
        let command = find(name).unwrap();
        assert_eq!(command.execution, Execution::Runtime, "{name}");
        assert_eq!(command.lan, LanAccess::Allowed, "{name}");
        assert_eq!(command.completion, Completion::Owned, "{name}");
    }
}
