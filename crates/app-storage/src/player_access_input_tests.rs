use super::*;
use crate::settings_validation::{SettingsValidationPhase, collect_settings_schema_diagnostics};
use serde_json::json;

fn settings(value: Value) -> Map<String, Value> {
    value.as_object().expect("settings object").clone()
}

#[test]
fn rust_raw_managed_directives_are_rejected_without_changing_rosters() {
    let mut settings = settings(json!({
        "owner_entries": "76561198000000001|Existing|Kept",
        "moderator_entries": "",
        "skip_queue_entries": "",
        "banned_entries": "76561198000000005|Existing ban",
        "users_cfg_extra": "server.description custom\nownerid 76561198000000001 \"Duplicate\" \"Ignored\"\nmoderatorid 76561198000000002 \"Mod\" \"Weekend\"\nglobal.skipqueueid 76561198000000003 \"Friend\" \"Event\"",
        "bans_cfg_extra": "banid 76561198000000004 \"Raider\" \"Griefing\" 7d\nserver.hostname custom"
    }));
    let before = settings.clone();
    assert!(!normalize_module_player_access_settings("rust", &mut settings).unwrap());
    assert_eq!(settings, before);
    let schema: Value =
        serde_json::from_str(include_str!("../../../modules/rust/schema.json")).unwrap();
    let diagnostics =
        collect_settings_schema_diagnostics(&schema, &settings, SettingsValidationPhase::Complete);
    assert!(
        diagnostics
            .iter()
            .any(|item| item.code == "managed_directive")
    );
    assert_eq!(settings, before);
}

#[test]
fn unrepresentable_raw_directives_are_rejected_without_rewriting_input() {
    let mut settings = settings(json!({
        "owner_entries": "",
        "users_cfg_extra": "ownerid 76561198000000001 \"Name|Alias\" \"Reason\""
    }));
    let before = settings.clone();
    assert!(!normalize_module_player_access_settings("rust", &mut settings).unwrap());
    assert_eq!(settings, before);
    let schema: Value =
        serde_json::from_str(include_str!("../../../modules/rust/schema.json")).unwrap();
    let diagnostics =
        collect_settings_schema_diagnostics(&schema, &settings, SettingsValidationPhase::Complete);
    assert!(
        diagnostics
            .iter()
            .any(|item| item.code == "managed_directive")
    );
    assert_eq!(settings, before);
}

#[test]
fn squad_raw_admin_assignments_are_rejected_without_promoting_privileges() {
    let mut settings = settings(json!({
        "admin_steam_ids": "76561198000000001",
        "priority_join_steam_ids": "",
        "admins_cfg": "Group=ClanAdmin:kick\nAdmin=76561198000000001:LanGameAdmin\nAdmin=76561198000000002:LanGameAdmin\nAdmin=76561198000000003:LanGameReserved\nAdmin=76561198000000004:ClanAdmin"
    }));
    let before = settings.clone();
    assert!(!normalize_module_player_access_settings("squad", &mut settings).unwrap());
    assert_eq!(settings, before);
    let schema: Value =
        serde_json::from_str(include_str!("../../../modules/squad/schema.json")).unwrap();
    let diagnostics =
        collect_settings_schema_diagnostics(&schema, &settings, SettingsValidationPhase::Complete);
    assert!(
        diagnostics
            .iter()
            .any(|item| item.code == "managed_directive")
    );
    assert_eq!(settings, before);
}
