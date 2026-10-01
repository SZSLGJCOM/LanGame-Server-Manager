use super::*;
use crate::{InstanceStatus, InstanceSummary};
use serde_json::json;

fn map(id: &str, enabled: bool) -> Value {
    json!({ "id": id, "map_name": "ScorchedEarth_P", "name": "焦土", "enabled": enabled })
}

fn fixture(module_id: &str) -> InstanceDetails {
    let defaults = defaults(module_id);
    let settings = json!({ "server_name": "ARK Room", "additional_maps": [map("desert", true), map("paused", false)] });
    InstanceDetails {
        summary: InstanceSummary {
            id: "ark-owned".into(),
            name: "ARK Room".into(),
            module_id: module_id.into(),
            status: InstanceStatus::Stopped,
            active_process_count: 0,
            bind_ip: "0.0.0.0".into(),
            port_count: defaults.len() * 3,
            autostart: false,
        },
        config_file_path: "instances/ark-owned/config/settings.json".into(),
        saves_path: "instances/ark-owned/runtime/ShooterGame/Saved".into(),
        backup_uses_declared_saves_path: true,
        auto_backup_on_stop: false,
        backup_retention_count: 10,
        settings_json: settings.to_string(),
        ports: requested_ports(module_id, &settings, &defaults, &defaults, &[]).unwrap(),
        active_run: None,
    }
}

fn defaults(module_id: &str) -> Vec<PortBinding> {
    let mut ports = vec![PortBinding {
        name: "game".into(),
        port: 7777,
        protocol: "udp".into(),
    }];
    if module_id == "arksurvivalevolved" {
        ports.push(PortBinding {
            name: "peer".into(),
            port: 7778,
            protocol: "udp".into(),
        });
    }
    ports.extend([
        PortBinding {
            name: "query".into(),
            port: 27015,
            protocol: "udp".into(),
        },
        PortBinding {
            name: "rcon".into(),
            port: 27020,
            protocol: "tcp".into(),
        },
    ]);
    ports
}

#[test]
fn maps_have_stable_world_log_and_transport_identity_without_changing_ownership() {
    for edition in ["arksurvivalevolved", "arksurvivalascended"] {
        let instance = fixture(edition);
        let plans = processes(&instance).unwrap();
        assert_eq!(plans.len(), 2, "paused map must not spawn");
        assert_eq!(plans[0].save_directory, instance.summary.id);
        assert_eq!(plans[1].save_directory, "ark-owned-map-desert");
        assert_ne!(plans[0].native_log_path, plans[1].native_log_path);
        let extra = project_process(&instance, Some("map-desert")).unwrap();
        assert_eq!(extra.summary.id, instance.summary.id);
        assert_eq!(extra.config_file_path, instance.config_file_path);
        assert_eq!(extra.saves_path, instance.saves_path);
        assert_eq!(extra.ports.len(), defaults(edition).len());
        assert!(
            extra
                .ports
                .iter()
                .all(|port| !port.name.starts_with("map-"))
        );
        let settings: Value = serde_json::from_str(&extra.settings_json).unwrap();
        assert_eq!(settings["map_name"], "ScorchedEarth_P");
        assert_eq!(settings["server_name"], "ARK Room | 焦土");
        assert_eq!(settings["cluster_id"], "lgsm-ark-owned");
        assert_eq!(
            Path::new(settings["cluster_directory"].as_str().unwrap()),
            Path::new(&instance.saves_path).join("ark-owned/cluster")
        );
        assert!(project_process(&instance, Some("map-paused")).is_err());
        assert!(project_process(&instance, Some("missing")).is_err());
    }
}

#[test]
fn main_preserves_existing_save_and_cluster_directory() {
    let mut instance = fixture("arksurvivalevolved");
    instance.settings_json =
        json!({"map_name": "TheIsland", "cluster_id": "existing", "additional_maps": []})
            .to_string();
    instance.saves_path = "instances/ark-owned/runtime/ShooterGame/Saved/ark-owned".into();
    let main = project_process(&instance, None).unwrap();
    let settings: Value = serde_json::from_str(&main.settings_json).unwrap();
    assert_eq!(settings["_managed_ark_save_directory"], "ark-owned");
    assert_eq!(
        Path::new(settings["cluster_directory"].as_str().unwrap()),
        Path::new(&instance.saves_path).join("cluster")
    );
    assert_eq!(
        serde_json::to_value(main.ports).unwrap(),
        serde_json::to_value(defaults("arksurvivalevolved")).unwrap()
    );
}

#[test]
fn pauses_keep_reserved_ports_and_resume_identity() {
    let instance = fixture("arksurvivalevolved");
    let defaults = defaults("arksurvivalevolved");
    let paused = json!({"additional_maps": [map("desert", false), map("paused", true)]});
    let ports = requested_ports(
        "arksurvivalevolved",
        &paused,
        &defaults,
        &defaults,
        &instance.ports,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&ports).unwrap(),
        serde_json::to_value(&instance.ports).unwrap()
    );
    for id in ["desert", "paused"] {
        let game = ports
            .iter()
            .find(|port| port.name == port_name(id, "game"))
            .unwrap();
        let peer = ports
            .iter()
            .find(|port| port.name == port_name(id, "peer"))
            .unwrap();
        assert_eq!(peer.port, game.port + 1);
    }
    let removed = requested_ports(
        "arksurvivalevolved",
        &json!({"additional_maps": []}),
        &defaults,
        &instance.ports,
        &instance.ports,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(removed).unwrap(),
        serde_json::to_value(defaults).unwrap()
    );
}

#[test]
fn rejects_untrusted_ids_packages_names_duplicates_and_excess_maps() {
    for bad in [
        json!({"id": "../outside", "map_name": "TheIsland", "name": "Island", "enabled": true}),
        json!({"id": "island", "map_name": "TheIsland?Port=1", "name": "Island", "enabled": true}),
        json!({"id": "island", "map_name": "TheIsland", "name": "?Port=1", "enabled": true}),
        json!({"id": "island", "map_name": "TheIsland", "name": "Island", "enabled": "true"}),
        json!({"id": "island", "map_name": "TheIsland", "name": "Island", "enabled": true, "path": "elsewhere"}),
    ] {
        assert!(parse_additional_maps(&json!({"additional_maps": [bad]})).is_err());
    }
    assert!(
        parse_additional_maps(&json!({"additional_maps": [map("same", true), map("same", false)]}))
            .is_err()
    );
    let too_many: Vec<_> = (0..16)
        .map(|index| map(&format!("map-{index}"), true))
        .collect();
    assert!(parse_additional_maps(&json!({"additional_maps": too_many})).is_err());
}

#[test]
fn running_topology_and_saved_map_package_cannot_change() {
    let previous = json!({"additional_maps": [map("desert", true)]});
    let paused = json!({"additional_maps": [map("desert", false)]});
    assert!(validate_map_changes(&previous, &paused, true).is_err());
    assert!(validate_map_changes(&previous, &paused, false).is_ok());
    let changed_package = json!({"additional_maps": [{"id": "desert", "map_name": "Ragnarok", "name": "Desert", "enabled": true}]});
    assert!(validate_map_changes(&previous, &changed_package, false).is_err());
}

#[test]
fn overlapping_ids_cannot_expose_another_maps_ports() {
    let mut instance = fixture("arksurvivalevolved");
    let settings = json!({"additional_maps": [map("a", true), map("a-desert", false)]});
    let defaults = defaults("arksurvivalevolved");
    instance.ports =
        requested_ports("arksurvivalevolved", &settings, &defaults, &defaults, &[]).unwrap();
    instance.settings_json = settings.to_string();
    let projected = project_process(&instance, Some("map-a")).unwrap();
    assert_eq!(projected.ports.len(), defaults.len());
    assert!(
        projected
            .ports
            .iter()
            .all(|port| ["game", "peer", "query", "rcon"].contains(&port.name.as_str()))
    );
    assert_ne!(
        projected
            .ports
            .iter()
            .find(|port| port.name == "rcon")
            .unwrap()
            .port,
        instance
            .ports
            .iter()
            .find(|port| port.name == "map-a-desert-rcon")
            .unwrap()
            .port
    );
}

#[test]
fn ase_allocates_game_and_peer_as_a_free_pair() {
    let mut defaults = defaults("arksurvivalevolved");
    defaults
        .iter_mut()
        .find(|port| port.name == "query")
        .unwrap()
        .port = 7788;
    let ports = requested_ports(
        "arksurvivalevolved",
        &json!({"additional_maps": [map("desert", true)]}),
        &defaults,
        &defaults,
        &[],
    )
    .unwrap();
    let game = ports
        .iter()
        .find(|port| port.name == "map-desert-game")
        .unwrap();
    let peer = ports
        .iter()
        .find(|port| port.name == "map-desert-peer")
        .unwrap();
    assert_eq!(game.port, 7797);
    assert_eq!(peer.port, game.port + 1);
    let endpoints: HashSet<_> = ports
        .iter()
        .map(|port| (&port.protocol, port.port))
        .collect();
    assert_eq!(endpoints.len(), ports.len());
}

#[test]
fn new_maps_before_retained_maps_do_not_take_their_reserved_endpoints() {
    for edition in ["arksurvivalevolved", "arksurvivalascended"] {
        let defaults = defaults(edition);
        let previous = json!({"additional_maps": [map("retained", false)]});
        let current = requested_ports(edition, &previous, &defaults, &defaults, &[]).unwrap();
        let incoming = json!({"additional_maps": [map("new", true), map("retained", false)]});
        let ports = requested_ports(edition, &incoming, &defaults, &defaults, &current).unwrap();
        for binding in current
            .iter()
            .filter(|binding| binding.name.starts_with("map-retained-"))
        {
            let retained = ports.iter().find(|port| port.name == binding.name).unwrap();
            assert_eq!(
                retained.port, binding.port,
                "{edition}: {} must stay reserved",
                binding.name
            );
            assert_eq!(retained.protocol, binding.protocol);
        }
        let endpoints: HashSet<_> = ports
            .iter()
            .map(|port| (&port.protocol, port.port))
            .collect();
        assert_eq!(
            endpoints.len(),
            ports.len(),
            "{edition}: new map must use distinct endpoints"
        );
    }
}

#[test]
fn additional_map_zero_requests_are_rejected_without_changing_main_zero_bindings() {
    for edition in ["arksurvivalevolved", "arksurvivalascended"] {
        let defaults = defaults(edition);
        let settings = json!({"additional_maps": [map("desert", true)]});
        let current = requested_ports(edition, &settings, &defaults, &defaults, &[]).unwrap();
        let mut main_zero = defaults.clone();
        main_zero
            .iter_mut()
            .find(|port| port.name == "rcon")
            .unwrap()
            .port = 0;
        let requested =
            requested_ports(edition, &settings, &defaults, &main_zero, &current).unwrap();
        assert_eq!(
            requested
                .iter()
                .find(|port| port.name == "rcon")
                .unwrap()
                .port,
            0
        );
        assert!(
            requested
                .iter()
                .filter(|port| port.name.starts_with("map-"))
                .all(|port| port.port != 0)
        );
        for name in ["game", "query", "rcon"] {
            let mut zero = current.clone();
            zero.iter_mut()
                .find(|port| port.name == port_name("desert", name))
                .unwrap()
                .port = 0;
            assert!(
                requested_ports(edition, &settings, &defaults, &zero, &current).is_err(),
                "{edition}: additional {name} cannot leave an unregistered native endpoint"
            );
        }
        let mut all_zero = current.clone();
        for port in all_zero
            .iter_mut()
            .filter(|port| port.name.starts_with("map-"))
        {
            port.port = 0;
        }
        assert!(requested_ports(edition, &settings, &defaults, &all_zero, &current).is_err());
    }
}

#[test]
fn ase_single_peer_edit_keeps_group_inputs_for_storage_to_resolve() {
    let defaults = defaults("arksurvivalevolved");
    let settings = json!({"additional_maps": [map("desert", true)]});
    let current =
        requested_ports("arksurvivalevolved", &settings, &defaults, &defaults, &[]).unwrap();
    let current_game = current
        .iter()
        .find(|port| port.name == "map-desert-game")
        .unwrap()
        .port;
    let mut incoming = defaults.clone();
    incoming.push(PortBinding {
        name: "map-desert-peer".into(),
        protocol: "udp".into(),
        port: 7812,
    });
    let ports = requested_ports(
        "arksurvivalevolved",
        &settings,
        &defaults,
        &incoming,
        &current,
    )
    .unwrap();
    // Storage's fixed-offset allocator identifies the sole changed member and
    // moves the game base to peer - 1; expansion must not erase that edit.
    assert_eq!(
        ports
            .iter()
            .find(|port| port.name == "map-desert-game")
            .unwrap()
            .port,
        current_game
    );
    assert_eq!(
        ports
            .iter()
            .find(|port| port.name == "map-desert-peer")
            .unwrap()
            .port,
        7812
    );
    for name in ["query", "rcon"] {
        let binding_name = port_name("desert", name);
        assert_eq!(
            ports
                .iter()
                .find(|port| port.name == binding_name)
                .unwrap()
                .port,
            current
                .iter()
                .find(|port| port.name == binding_name)
                .unwrap()
                .port
        );
    }
}
