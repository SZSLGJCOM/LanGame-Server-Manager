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
fn every_map_uses_its_complete_server_name_without_a_shared_prefix() {
    for edition in ["arksurvivalevolved", "arksurvivalascended"] {
        let mut instance = fixture(edition);
        let mut settings: Value = serde_json::from_str(&instance.settings_json).unwrap();
        settings["server_name"] = json!("Independent primary server");
        settings["additional_maps"][0]["name"] = json!("Another community | 焦土");
        instance.settings_json = settings.to_string();

        for (key, expected) in [
            ("main", "Independent primary server"),
            ("map-desert", "Another community | 焦土"),
        ] {
            let projected = project_process(&instance, Some(key)).unwrap();
            let projected_settings: Value = serde_json::from_str(&projected.settings_json).unwrap();
            assert_eq!(projected_settings["server_name"], expected);
            assert_eq!(projected.summary.id, instance.summary.id);
        }
        let plans = processes(&instance).unwrap();
        assert_eq!(plans[0].display_name, "Independent primary server");
        assert_eq!(plans[1].display_name, "Another community | 焦土");
        assert_eq!(plans[0].save_directory, instance.summary.id);
        assert_eq!(plans[1].save_directory, "ark-owned-map-desert");

        settings["server_name"] = json!("Renamed primary only");
        instance.settings_json = settings.to_string();
        let extra = project_process(&instance, Some("map-desert")).unwrap();
        let extra_settings: Value = serde_json::from_str(&extra.settings_json).unwrap();
        assert_eq!(extra_settings["server_name"], "Another community | 焦土");
    }
}

#[test]
fn mod_scopes_project_each_map_and_attach_owned_native_ini_paths() {
    let mut instance = fixture("arksurvivalascended");
    let mut settings: Value = serde_json::from_str(&instance.settings_json).unwrap();
    settings["mod_ids_csv"] = json!("111");
    settings["ark_mod_map_overrides"] =
        json!({"map-desert": {"mods": {"111": {"active": false}, "222": {"active": true}}}});
    settings["_managed_ark_ini_directory"] = json!("untrusted-other-instance");
    instance.settings_json = settings.to_string();
    for (key, ids) in [("main", "111"), ("map-desert", "222")] {
        let projected = project_process(&instance, Some(key)).unwrap();
        let projected: Value = serde_json::from_str(&projected.settings_json).unwrap();
        assert_eq!(projected["mod_ids_csv"], ids);
        let expected = Path::new(&instance.config_file_path)
            .parent()
            .unwrap()
            .join("ark-maps")
            .join(key);
        assert_eq!(
            projected["_managed_ark_ini_directory"],
            expected.to_string_lossy().as_ref()
        );
    }
    settings["ark_mod_map_overrides"] = json!({});
    instance.settings_json = settings.to_string();
    let projected = project_process(&instance, None).unwrap();
    let projected: Value = serde_json::from_str(&projected.settings_json).unwrap();
    assert!(projected.get("_managed_ark_ini_directory").is_none());
}

#[test]
fn complete_server_names_keep_lengths_previously_allowed_by_the_primary_name() {
    let mut instance = fixture("arksurvivalascended");
    let complete_name = format!("{} | {}", "界".repeat(100), "焦土");
    let mut settings: Value = serde_json::from_str(&instance.settings_json).unwrap();
    settings["additional_maps"][0]["name"] = json!(complete_name);
    instance.settings_json = settings.to_string();
    let extra = project_process(&instance, Some("map-desert")).unwrap();
    let projected: Value = serde_json::from_str(&extra.settings_json).unwrap();
    assert_eq!(projected["server_name"], complete_name);
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
        assert_eq!(settings["server_name"], "焦土");
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

#[cfg(windows)]
#[test]
fn canonical_save_roots_produce_valid_default_cluster_paths() {
    for edition in ["arksurvivalevolved", "arksurvivalascended"] {
        let mut instance = fixture(edition);
        instance.saves_path = r"\\?\C:\ARK Worlds\ShooterGame\Saved".into();
        let original_settings = instance.settings_json.clone();
        for key in ["main", "map-desert"] {
            let projected = project_process(&instance, Some(key)).unwrap();
            let settings: Value = serde_json::from_str(&projected.settings_json).unwrap();
            let directory = settings["cluster_directory"].as_str().unwrap();
            let resolved = crate::ark_cluster::resolve_cluster_directory(
                settings["cluster_id"].as_str().unwrap(),
                directory,
                Path::new(&instance.saves_path),
            )
            .expect("an owned canonical save root must not fail launch validation");
            assert_eq!(
                resolved.unwrap(),
                Path::new(r"C:\ARK Worlds\ShooterGame\Saved\ark-owned\cluster")
            );
            assert_eq!(projected.saves_path, instance.saves_path);
        }
        assert_eq!(instance.settings_json, original_settings);

        let mut settings: Value = serde_json::from_str(&instance.settings_json).unwrap();
        settings["cluster_directory"] = json!(r"\\?\C:\operator cluster");
        instance.settings_json = settings.to_string();
        let projected = project_process(&instance, None).unwrap();
        let settings: Value = serde_json::from_str(&projected.settings_json).unwrap();
        let directory = settings["cluster_directory"].as_str().unwrap();
        assert_eq!(directory, r"\\?\C:\operator cluster");
        assert!(
            crate::ark_cluster::resolve_cluster_directory(
                settings["cluster_id"].as_str().unwrap(),
                directory,
                Path::new(&instance.saves_path)
            )
            .is_err(),
            "explicit device paths must retain the existing rejection"
        );
    }
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
fn maximum_enabled_maps_have_distinct_processes_and_owned_native_ports() {
    for edition in ["arksurvivalevolved", "arksurvivalascended"] {
        let additions = (0..MAX_ADDITIONAL_MAPS)
            .map(|index| {
                json!({"id":format!("limit-{index}"), "name":format!("Limit map {index}"),
                    "map_name":if edition == "arksurvivalascended" {"ScorchedEarth_WP"} else {"ScorchedEarth_P"},
                    "enabled":true})
            })
            .collect::<Vec<_>>();
        let settings = json!({"additional_maps": additions});
        assert_eq!(
            parse_additional_maps(&settings).unwrap().len(),
            MAX_ADDITIONAL_MAPS
        );
        let mut native_ports = defaults(edition);
        // Both module catalogs declare all four roles, including ASA's peer.
        if edition == "arksurvivalascended" {
            native_ports.insert(
                1,
                PortBinding {
                    name: "peer".into(),
                    port: 7778,
                    protocol: "udp".into(),
                },
            );
        }
        let mut instance = fixture(edition);
        instance.settings_json = settings.to_string();
        instance.ports =
            requested_ports(edition, &settings, &native_ports, &native_ports, &[]).unwrap();
        instance.summary.port_count = instance.ports.len();
        let expected_process_count = MAX_ADDITIONAL_MAPS + 1;
        assert_eq!(instance.ports.len(), expected_process_count * 4);
        let plans = processes(&instance).unwrap();
        assert_eq!(plans.len(), expected_process_count);
        assert_eq!(
            plans
                .iter()
                .map(|plan| &plan.process_key)
                .collect::<HashSet<_>>()
                .len(),
            expected_process_count
        );
        let mut projected_endpoints = HashSet::new();
        for plan in &plans {
            let projected = project_process(&instance, Some(&plan.process_key)).unwrap();
            assert_eq!(projected.ports.len(), 4);
            for (role, protocol) in [
                ("game", "udp"),
                ("peer", "udp"),
                ("query", "udp"),
                ("rcon", "tcp"),
            ] {
                let binding = projected
                    .ports
                    .iter()
                    .find(|port| port.name == role)
                    .unwrap();
                let owned_name = if plan.process_key == "main" {
                    role.to_owned()
                } else {
                    format!("{}-{role}", plan.process_key)
                };
                let registered = instance
                    .ports
                    .iter()
                    .find(|port| port.name == owned_name)
                    .unwrap();
                assert_eq!(binding.protocol, registered.protocol);
                assert_eq!(binding.protocol, protocol);
                assert_eq!(binding.port, registered.port);
                assert_ne!(binding.port, 0);
                assert!(
                    projected_endpoints.insert((binding.protocol.clone(), binding.port)),
                    "{edition}: {} must own an independent {role} endpoint",
                    plan.process_key
                );
            }
        }
        assert_eq!(projected_endpoints.len(), expected_process_count * 4);
    }
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
