use super::*;

#[test]
fn dragonwilds_readiness_requires_one_positive_native_record() {
    let modules = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../modules");
    let descriptor = app_modules::discover_modules(&modules)
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "runescapedragonwilds")
        .unwrap();
    let manifest = SmokeManifest::load(&descriptor).unwrap();
    let path =
        std::env::temp_dir().join(format!("lg-dragonwilds-ready-{}.log", uuid::Uuid::new_v4()));
    let negative = "LogNetSessionSettings: Setting [\"ReadyToJoin\"] written with key[x0] value[0]\n\
                    LogNetSessionSettings: Setting [\"OtherFlag\"] written with key[x0] value[1]\n";
    let positive =
        "LogNetSessionSettings: Setting [\"ReadyToJoin\"] written with key[x0] value[1]\n";
    for (text, expected) in [
        (negative.to_owned(), false),
        (format!("{negative}{positive}"), true),
    ] {
        std::fs::write(&path, "").unwrap();
        let mut baseline = LogBaseline::capture(&path).unwrap();
        std::fs::write(&path, text).unwrap();
        let ready = manifest.probes.iter().all(|probe| match probe {
            Probe::LogMarker { any_of, .. } => {
                new_log_contains(&path, &mut baseline, any_of).unwrap()
            }
            _ => panic!("unexpected Dragonwilds readiness probe"),
        });
        if ready != expected {
            std::fs::remove_file(&path).unwrap();
            assert_eq!(
                ready, expected,
                "unrelated true settings must not satisfy ReadyToJoin"
            );
        }
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn windrose_readiness_requires_a_native_world_in_addition_to_owned_transport() {
    let modules = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../modules");
    let descriptor = app_modules::discover_modules(&modules)
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "windrose")
        .unwrap();
    let manifest = SmokeManifest::load(&descriptor).unwrap();
    assert_eq!(manifest.probes.len(), 4);
    assert!(
        manifest
            .probes
            .iter()
            .any(|probe| matches!(probe, Probe::WindroseWorld { .. }))
    );
    for (port_name, protocol) in [("direct", "udp"), ("direct_tcp", "tcp")] {
        assert!(manifest.probes.iter().any(|probe| matches!(probe,
            Probe::ProcessEndpoint { process_key, port_name: name, protocol: transport, .. }
            if process_key == "main" && name == port_name && transport == protocol
        )));
    }
    assert!(manifest.probes.iter().any(|probe| matches!(probe,
        Probe::TcpConnect { port_name, .. } if port_name == "direct_tcp"
    )));
}

#[test]
fn squad_readiness_requires_fresh_world_session_and_authenticated_complete_roster() {
    let modules = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../modules");
    let mut descriptor = app_modules::discover_modules(&modules)
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "squad")
        .unwrap();
    let manifest = SmokeManifest::load(&descriptor).unwrap();
    assert_eq!(manifest.probes.len(), 4);
    assert!(manifest.probes.iter().any(|probe| matches!(probe,
        Probe::SquadPlayers { port_name, .. } if port_name == "rcon"
    )));
    assert!(manifest.probes.iter().any(|probe| matches!(probe,
        Probe::ProcessEndpoint { process_key, port_name, protocol, .. }
        if process_key == "main" && port_name == "game" && protocol == "udp"
    )));
    let markers = manifest
        .probes
        .iter()
        .filter_map(|probe| match probe {
            Probe::LogMarker {
                sources, any_of, ..
            } => {
                assert!(matches!(sources.as_slice(), [LogSource::Install { path }]
                if path == Path::new("SquadGame/Saved/Logs/SquadGame.log")));
                Some(any_of)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(markers.len(), 2);
    assert_eq!(
        markers[0],
        &["LogGameState: Match State Changed from EnteringMap to WaitingToStart"]
    );
    assert_eq!(
        markers[1],
        &[
            "LogSquadOnlineServices: Session created: Started USQOnlineServicesUpdateSessionManager updates"
        ]
    );
    let path = std::env::temp_dir().join(format!(
        "lg-squad-log-{}.log",
        uuid::Uuid::new_v4().simple()
    ));
    let previous = format!("{}\n{}\n", markers[0][0], markers[1][0]);
    std::fs::write(&path, &previous).unwrap();
    let mut baseline = LogBaseline::capture(&path).unwrap();
    for marker in &markers {
        assert!(!new_log_contains(&path, &mut baseline, marker).unwrap());
    }
    std::fs::write(&path, format!("{previous}{}\n", markers[0][0])).unwrap();
    assert!(new_log_contains(&path, &mut baseline, markers[0]).unwrap());
    assert!(!new_log_contains(&path, &mut baseline, markers[1]).unwrap());
    std::fs::write(&path, format!("{previous}{previous}")).unwrap();
    for marker in &markers {
        assert!(new_log_contains(&path, &mut baseline, marker).unwrap());
    }
    std::fs::remove_file(path).unwrap();
    descriptor
        .runtime
        .player_actions
        .iter_mut()
        .find(|action| action.id == "list_players")
        .unwrap()
        .command_template = "AdminBroadcast changed".into();
    assert!(matches!(SmokeManifest::load(&descriptor), Err(error)
        if error.contains("declared read-only player action")));
    descriptor
        .runtime
        .player_actions
        .iter_mut()
        .find(|action| action.id == "list_players")
        .unwrap()
        .command_template = "ListPlayers".into();
    descriptor
        .runtime
        .player_list
        .as_mut()
        .unwrap()
        .response_codec = app_core::ModulePlayerListCodec::A2sPlayers;
    assert!(SmokeManifest::load(&descriptor).is_err());
}

#[test]
fn returntomoria_readiness_pairs_fresh_status_with_owned_game_udp() {
    let modules = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../modules");
    let descriptor = app_modules::discover_modules(&modules)
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "returntomoria")
        .unwrap();
    let manifest = SmokeManifest::load(&descriptor).unwrap();
    assert_eq!(manifest.probes.len(), 2);
    assert!(
        manifest
            .probes
            .iter()
            .any(|probe| matches!(probe, Probe::ReturntomoriaStatus { .. }))
    );
    assert!(manifest.probes.iter().any(|probe| matches!(probe,
        Probe::ProcessEndpoint { process_key, port_name, protocol, .. }
        if process_key == "main" && port_name == "game" && protocol == "udp"
    )));
    let text = std::fs::read_to_string(descriptor.root.join("smoke.toml")).unwrap();
    let redirected = format!("{text}\npath = \"outside/Status.json\"\n");
    assert!(
        toml::from_str::<SmokeManifest>(&redirected).is_err(),
        "the status path is fixed, not fixture supplied"
    );
}

#[test]
fn astroneer_readiness_requires_its_declared_console_player_contract() {
    let modules = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../modules");
    let mut descriptor = app_modules::discover_modules(&modules)
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "astroneer")
        .unwrap();
    let root =
        std::env::temp_dir().join(format!("lg-astro-probe-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir(&root).unwrap();
    descriptor.root = root.clone();
    let manifest = r#"schema_version = 1
module_id = "astroneer"
primary_process_key = "main"
readiness_timeout_ms = 10000
poll_interval_ms = 100
stability_window_ms = 100
probe_mode = "all"
[[probes]]
id = "console-players"
kind = "astroneer_players"
port_name = "console"
"#;
    std::fs::write(root.join("smoke.toml"), manifest).unwrap();
    SmokeManifest::load(&descriptor).unwrap();
    std::fs::write(
        root.join("smoke.toml"),
        manifest.replace("astroneer_players", "astroneer_world"),
    )
    .unwrap();
    SmokeManifest::load(&descriptor).unwrap();
    std::fs::write(
        root.join("smoke.toml"),
        manifest.replace("port_name = \"console\"", "port_name = \"game\""),
    )
    .unwrap();
    assert!(SmokeManifest::load(&descriptor).is_err());
    std::fs::write(root.join("smoke.toml"), manifest).unwrap();
    descriptor
        .runtime
        .player_list
        .as_mut()
        .unwrap()
        .response_codec = app_core::ModulePlayerListCodec::A2sPlayers;
    assert!(SmokeManifest::load(&descriptor).is_err());
    descriptor
        .runtime
        .player_list
        .as_mut()
        .unwrap()
        .response_codec = app_core::ModulePlayerListCodec::AstroneerPlayers;
    descriptor.runtime.player_list.as_mut().unwrap().source =
        app_core::ModulePlayerListSource::RuntimeAction;
    assert!(SmokeManifest::load(&descriptor).is_err());
    descriptor.runtime.player_list.as_mut().unwrap().source =
        app_core::ModulePlayerListSource::TcpConsole;
    descriptor.summary.id = "valheim".into();
    std::fs::write(
        root.join("smoke.toml"),
        manifest.replace("module_id = \"astroneer\"", "module_id = \"valheim\""),
    )
    .unwrap();
    assert!(SmokeManifest::load(&descriptor).is_err());
    std::fs::remove_file(root.join("smoke.toml")).unwrap();
    std::fs::remove_dir(root).unwrap();
}

#[test]
fn humanitz_readiness_reuses_only_the_declared_read_only_info_action() {
    let modules = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../modules");
    let mut descriptor = app_modules::discover_modules(&modules)
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "humanitz")
        .unwrap();
    let manifest = SmokeManifest::load(&descriptor).unwrap();
    assert_eq!(
        manifest.fixture.settings.get("rcon_enabled"),
        Some(&serde_json::json!(true))
    );
    assert!(manifest.probes.iter().any(|probe| matches!(probe,
        Probe::HumanitzInfo { port_name, .. } if port_name == "rcon"
    )));
    assert_eq!(manifest.probes.len(), 3);
    assert!(manifest.probes.iter().any(|probe| matches!(probe,
        Probe::ProcessEndpoint { process_key, port_name, protocol, .. }
        if process_key == "main" && port_name == "game" && protocol == "udp"
    )));
    let markers = manifest.probes.iter().find_map(|probe| match probe {
        Probe::LogMarker { sources, any_of, .. }
        if matches!(sources.as_slice(), [LogSource::Process { process_key }] if process_key == "main") => Some(any_of),
        _ => None,
    }).expect("RCON mode also requires this run's EOS session creation");
    assert_eq!(
        markers,
        &["LogHZSuccess: Display: Success => Session created!"]
    );
    let path =
        std::env::temp_dir().join(format!("lg-hz-log-{}.log", uuid::Uuid::new_v4().simple()));
    std::fs::write(&path, format!("{}\n", markers[0])).unwrap();
    let mut baseline = LogBaseline::capture(&path).unwrap();
    assert!(!new_log_contains(&path, &mut baseline, markers).unwrap());
    std::fs::write(&path, format!("{}\n{}\n", markers[0], markers[0])).unwrap();
    assert!(new_log_contains(&path, &mut baseline, markers).unwrap());
    std::fs::remove_file(path).unwrap();
    descriptor
        .runtime
        .player_actions
        .iter_mut()
        .find(|action| action.id == "list_online_players")
        .unwrap()
        .command_template = "save".into();
    assert!(matches!(SmokeManifest::load(&descriptor), Err(error)
        if error.contains("declared read-only player action")));
}

#[test]
fn humanitz_existing_readiness_only_omits_explicitly_disabled_rcon_with_native_proof() {
    let mut manifest: SmokeManifest =
        toml::from_str(include_str!("../../../../modules/humanitz/smoke.toml")).unwrap();
    let existing = ProbeScope::ExistingInstance;
    let disposable = ProbeScope::Disposable(Path::new("fixture"));
    let disabled = r#"{"rcon_enabled":false}"#;
    let enabled = r#"{"rcon_enabled":true}"#;
    assert!(!manifest.humanitz_info_required(existing, disabled).unwrap());
    assert!(manifest.humanitz_info_required(existing, enabled).unwrap());
    assert!(
        manifest
            .humanitz_info_required(disposable, enabled)
            .unwrap()
    );
    assert!(
        manifest
            .humanitz_info_required(disposable, disabled)
            .is_err()
    );
    for invalid in [
        "{}",
        r#"{"rcon_enabled":null}"#,
        r#"{"rcon_enabled":"false"}"#,
        r#"{"rcon_enabled":0}"#,
        "invalid JSON",
    ] {
        assert!(manifest.humanitz_info_required(existing, invalid).is_err());
    }
    manifest.stability_window_ms = 2999;
    assert!(manifest.humanitz_info_required(existing, disabled).is_err());
    manifest.stability_window_ms = 3000;
    for index in 0..manifest.probes.len() {
        let probe = manifest.probes.remove(index);
        let needs_native_proof = !matches!(probe, Probe::HumanitzInfo { .. });
        assert_eq!(
            manifest.humanitz_info_required(existing, disabled).is_err(),
            needs_native_proof
        );
        manifest.probes.insert(index, probe);
    }
    manifest.probe_mode = "any".into();
    assert!(manifest.humanitz_info_required(existing, disabled).is_err());
}

#[test]
fn valheim_private_readiness_requires_both_owned_endpoints_and_fresh_opened_log() {
    let modules = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../modules");
    let descriptor = app_modules::discover_modules(&modules)
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "valheim")
        .unwrap();
    let manifest = SmokeManifest::load(&descriptor).unwrap();
    assert_eq!(
        manifest.fixture.settings.get("public_server"),
        Some(&serde_json::json!(0))
    );
    assert_eq!(
        manifest.fixture.settings.get("crossplay_enabled"),
        Some(&serde_json::json!(false))
    );
    assert_eq!(manifest.probes.len(), 3);
    for port in ["game", "query"] {
        assert!(manifest.probes.iter().any(|probe| matches!(probe,
            Probe::ProcessEndpoint { process_key, port_name, protocol, .. }
            if process_key == "main" && port_name == port && protocol == "udp"
        )));
    }
    let markers = manifest.probes.iter().find_map(|probe| match probe {
        Probe::LogMarker { sources, any_of, .. }
        if matches!(sources.as_slice(), [LogSource::Process { process_key }] if process_key == "main") => Some(any_of),
        _ => None,
    }).expect("private Steam mode requires the post-generation host startup marker");
    assert_eq!(markers, &["Opened Steam server"]);
    let path = std::env::temp_dir().join(format!(
        "lg-valheim-log-{}.log",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::write(&path, "Opened Steam server\n").unwrap();
    let mut baseline = LogBaseline::capture(&path).unwrap();
    std::fs::write(&path, "Opened Steam server\nGame server connected\n").unwrap();
    assert!(!new_log_contains(&path, &mut baseline, markers).unwrap());
    std::fs::write(
        &path,
        "Opened Steam server\nGame server connected\nOpened Steam server\n",
    )
    .unwrap();
    assert!(new_log_contains(&path, &mut baseline, markers).unwrap());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn native_readiness_contracts_cover_all_repo_modules() {
    let modules = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../modules");
    let descriptors = app_modules::discover_modules(&modules).unwrap();
    assert_eq!(descriptors.len(), 32);
    for descriptor in descriptors {
        SmokeManifest::load(&descriptor)
            .unwrap_or_else(|error| panic!("{}: {error}", descriptor.summary.id));
    }
}

#[test]
fn asa_readiness_rejects_early_startup_and_requires_fresh_advertising() {
    let modules = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../modules");
    let descriptor = app_modules::discover_modules(&modules)
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "arksurvivalascended")
        .unwrap();
    let manifest = SmokeManifest::load(&descriptor).unwrap();
    assert_eq!(manifest.readiness_timeout_ms, 300000);
    assert_eq!(manifest.stability_window_ms, 3000);
    let markers = manifest
        .probes
        .iter()
        .find_map(|probe| match probe {
            Probe::LogMarker {
                sources, any_of, ..
            } if matches!(sources.as_slice(), [LogSource::Instance { path }]
                if path == Path::new("logs/ark-ascended-server.log")) =>
            {
                Some(any_of)
            }
            _ => None,
        })
        .expect("ASA readiness requires its native startup log");
    let path =
        std::env::temp_dir().join(format!("lg-asa-log-{}.log", uuid::Uuid::new_v4().simple()));
    let advertised = "Server has completed startup and is now advertising for join. (4.00GB Mem)\n";
    std::fs::write(&path, advertised).unwrap();
    let mut baseline = LogBaseline::capture(&path).unwrap();
    assert!(!new_log_contains(&path, &mut baseline, markers).unwrap());
    let early_startup = concat!(
        "Server: \"fixture\" has successfully started!\n",
        "Steam Subsystem initialized: Success\n",
        "EOS Subsystem initialized: Success\n"
    );
    std::fs::write(&path, format!("{advertised}{early_startup}")).unwrap();
    assert!(
        !new_log_contains(&path, &mut baseline, markers).unwrap(),
        "native RCON authentication can still be unavailable at the early startup marker"
    );
    std::fs::write(&path, format!("{advertised}{early_startup}{advertised}")).unwrap();
    assert!(new_log_contains(&path, &mut baseline, markers).unwrap());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn native_readiness_does_not_accept_old_log_markers() {
    let path = std::env::temp_dir().join(format!(
        "lg-log-offset-{}.log",
        uuid::Uuid::new_v4().simple()
    ));
    let marker = vec!["READY".to_owned()];
    std::fs::write(&path, "READY previous run\n").unwrap();
    let mut baseline = LogBaseline::capture(&path).unwrap();
    assert!(!new_log_contains(&path, &mut baseline, &marker).unwrap());
    std::fs::write(&path, "READY previous run\nREADY current run\n").unwrap();
    assert!(new_log_contains(&path, &mut baseline, &marker).unwrap());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn barotrauma_private_readiness_requires_owned_game_endpoint_and_fresh_startup_log() {
    let modules = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../modules");
    let descriptor = app_modules::discover_modules(&modules)
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "barotrauma")
        .unwrap();
    let manifest = SmokeManifest::load(&descriptor).unwrap();
    assert_eq!(
        manifest.fixture.settings.get("public_server"),
        Some(&serde_json::json!(false))
    );
    assert_eq!(
        manifest.fixture.settings.get("enable_upnp"),
        Some(&serde_json::json!(false))
    );
    assert_eq!(manifest.probes.len(), 2);
    assert!(manifest.probes.iter().any(|probe| matches!(probe,
        Probe::ProcessEndpoint { process_key, port_name, protocol, .. }
        if process_key == "main" && port_name == "game" && protocol == "udp"
    )));
    let markers = manifest
        .probes
        .iter()
        .find_map(|probe| match probe {
            Probe::LogMarker {
                sources, any_of, ..
            } if matches!(sources.as_slice(),
            [LogSource::Process { process_key }] if process_key == "main") =>
            {
                Some(any_of)
            }
            _ => None,
        })
        .expect("private mode requires the managed process startup log");
    assert_eq!(markers, &["Server started"]);
    let path =
        std::env::temp_dir().join(format!("lg-baro-log-{}.log", uuid::Uuid::new_v4().simple()));
    std::fs::write(&path, "Server started\n").unwrap();
    let mut baseline = LogBaseline::capture(&path).unwrap();
    assert!(!new_log_contains(&path, &mut baseline, markers).unwrap());
    std::fs::write(&path, "Server started\nServer started\n").unwrap();
    assert!(new_log_contains(&path, &mut baseline, markers).unwrap());
    std::fs::remove_file(path).unwrap();
}
