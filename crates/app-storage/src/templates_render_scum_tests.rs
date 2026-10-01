use super::*;
use crate::settings_validation::{SettingsValidationPhase, collect_settings_schema_diagnostics};
use serde_json::{Map, json};

#[test]
fn scum_renderer_writes_all_436_managed_v7_keys_once() {
    let rendered = render_scum_server_settings_ini(&Map::new()).expect("render SCUM settings");
    let assignments = rendered
        .lines()
        .filter(|line| line.starts_with("scum.") && line.contains('='))
        .collect::<Vec<_>>();

    assert_eq!(assignments.len(), 436);
    assert_eq!(
        assignments
            .iter()
            .map(|line| line.split_once('=').unwrap().0)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        436
    );
    assert!(!rendered.contains("scum.ServerSettingsVersion="));
    for section in [
        "General", "World", "Features", "Respawn", "Vehicles", "Damage",
    ] {
        assert_eq!(rendered.matches(&format!("[{section}]")).count(), 1);
    }
}

#[test]
fn scum_renderer_uses_structured_values_and_blocks_ini_line_injection() {
    let settings = Map::from_iter([
        (
            String::from("server_general"),
            json!({
                "server_name": "LanGame\n[Damage]\nscum.ZombieDamageMultiplier=99",
                "max_players": 128,
                "full_wipe": true
            }),
        ),
        (
            String::from("server_world"),
            json!({"door_lockability_garage": true}),
        ),
    ]);

    let rendered = render_scum_server_settings_ini(&settings).expect("render SCUM settings");

    assert!(rendered.contains("scum.ServerName=LanGame [Damage] scum.ZombieDamageMultiplier=99"));
    assert!(rendered.contains("scum.MaxPlayers=128"));
    assert!(rendered.contains("scum.FullWipe=True"));
    assert!(rendered.contains("scum.DoorLockability.Garage=True"));
    assert_eq!(
        rendered
            .lines()
            .filter(|line| line.starts_with("scum.ZombieDamageMultiplier="))
            .count(),
        1
    );
}

#[test]
fn scum_json_renderers_keep_native_root_shapes_and_row_order() {
    let settings = Map::from_iter([
        (
            String::from("economy_override"),
            json!({"economy-reset-time-hours":"24.0","traders":{"A_0_Armory":[
                {"tradeable-code":"First"}, {"tradeable-code":"First"}
            ]}}),
        ),
        (
            String::from("raid_times"),
            json!([{"day":"Weekend","time":"12:00-13:00","start-announcement-time":"30","end-announcement-time":"15"}]),
        ),
        (
            String::from("notifications"),
            json!([{"day":"Everyday","time":["10:00","20:00"],"duration":"10","color":"255-255-255","wait":"5","message":"Restart #RestartIn(00:15)"}]),
        ),
    ]);

    let economy: Value =
        serde_json::from_str(&render_scum_economy_override(&settings).expect("render economy"))
            .unwrap();
    assert_eq!(
        economy["economy-override"]["traders"]["A_0_Armory"][1]["tradeable-code"],
        "First"
    );
    let raid: Value =
        serde_json::from_str(&render_scum_raid_times(&settings).expect("render raid times"))
            .unwrap();
    assert_eq!(raid["raiding-times"][0]["day"], "Weekend");
    let notifications: Value =
        serde_json::from_str(&render_scum_notifications(&settings).expect("render notifications"))
            .unwrap();
    assert_eq!(notifications["Notifications"][0]["time"][1], "20:00");
}

#[test]
fn scum_nested_schema_rejects_invalid_native_leaf_values() {
    let schema: Value = serde_json::from_str(include_str!("../../../modules/scum/schema.json"))
        .expect("parse SCUM schema");
    let settings = json!({
        "server_general": {
            "server_name": "unsafe\nname",
            "max_players": 129
        },
        "server_world": {"are_animals_allowed_in_world": "False"}
    });

    let diagnostics = collect_settings_schema_diagnostics(
        &schema,
        settings.as_object().unwrap(),
        SettingsValidationPhase::Complete,
    );
    let fields = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.field.as_str())
        .collect::<Vec<_>>();
    assert!(fields.contains(&"server_general.server_name"));
    assert!(fields.contains(&"server_general.max_players"));
    assert!(fields.contains(&"server_world.are_animals_allowed_in_world"));
}

#[test]
fn scum_august_settings_render_in_their_native_sections() {
    let settings = Map::from_iter([
        (
            String::from("server_world"),
            json!({
                "max_allowed_apex_facility_keycards": 8,
                "max_allowed_apex_facility_keycards_police_station": 6,
                "max_allowed_apex_facility_keycards_radiation_zone": 2
            }),
        ),
        (
            String::from("server_respawn"),
            json!({"cloning_sickness_enabled": false}),
        ),
    ]);
    let rendered = render_scum_server_settings_ini(&settings).expect("render August SCUM settings");
    let world = rendered
        .split("[World]\n")
        .nth(1)
        .unwrap()
        .split("\n[")
        .next()
        .unwrap();
    assert!(world.contains("scum.MaxAllowedApexFacilityKeycards=8\n"));
    assert!(world.contains("scum.MaxAllowedApexFacilityKeycards_PoliceStation=6\n"));
    assert!(world.contains("scum.MaxAllowedApexFacilityKeycards_RadiationZone=2\n"));
    let respawn = rendered
        .split("[Respawn]\n")
        .nth(1)
        .unwrap()
        .split("\n[")
        .next()
        .unwrap();
    assert!(respawn.contains("scum.CloningSicknessEnabled=False\n"));
}
