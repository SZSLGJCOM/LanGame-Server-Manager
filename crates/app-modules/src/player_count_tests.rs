use super::*;

#[test]
fn player_count_source_defaults_to_the_declared_player_query() {
    let runtime = runtime_spec_from_toml(None, None).expect("default runtime");
    assert_eq!(
        runtime.player_count_source,
        ModulePlayerCountSource::PlayerQuery
    );
    let runtime: ModuleTomlRuntime =
        toml::from_str("[player_query]\nprotocol = 'a2s_info'\nport_names = ['query']\n")
            .expect("query contract");
    let runtime = runtime_spec_from_toml(Some(runtime), None).expect("runtime");
    assert_eq!(
        runtime.player_count_source,
        ModulePlayerCountSource::PlayerQuery
    );
    assert_eq!(runtime.player_query.unwrap().protocol, "a2s_info");
}

#[test]
fn player_count_source_requires_a_valid_player_list() {
    let runtime = toml::from_str("player_count_source = 'player_list'\n").unwrap();
    let error = runtime_spec_from_toml(Some(runtime), None).unwrap_err();
    assert!(error.contains("requires an online player_list"));
    assert!(toml::from_str::<ModuleTomlRuntime>("player_count_source = 'fallback'\n").is_err());
}

#[test]
fn player_count_source_squad_and_humanitz_use_lists_without_removing_native_query_ports() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules");
    let modules = discover_modules(root).expect("discover production contracts");
    for (id, query_port, query_argument) in [
        ("squad", 27165, "QUERYPORT={{ports.query.port}}"),
        ("humanitz", 27015, "-queryport={{ports.query.port}}"),
    ] {
        let module = modules
            .iter()
            .find(|module| module.summary.id == id)
            .unwrap();
        assert_eq!(
            module.runtime.player_count_source,
            ModulePlayerCountSource::PlayerList
        );
        let query = module.runtime.player_query.as_ref().unwrap();
        assert_eq!(query.protocol, "none");
        assert!(query.port_names.is_empty());
        assert!(module.default_ports.iter().any(|port| {
            port.name == "query" && port.protocol == "udp" && port.port == query_port
        }));
        assert!(
            module
                .process
                .as_ref()
                .unwrap()
                .args_template
                .iter()
                .any(|argument| argument == query_argument)
        );
        let list = module.runtime.player_list.as_ref().unwrap();
        assert_eq!(list.source, ModulePlayerListSource::RuntimeAction);
        let action = module
            .runtime
            .player_actions
            .iter()
            .find(|action| Some(&action.id) == list.action_id.as_ref())
            .unwrap();
        assert!(!action.target_required && !action.destructive);
        assert_eq!(
            action.enabled_setting_key.as_deref(),
            if id == "humanitz" {
                Some("rcon_enabled")
            } else {
                None
            }
        );
    }
    let rust = modules
        .iter()
        .find(|module| module.summary.id == "rust")
        .unwrap();
    assert_eq!(
        rust.runtime.player_count_source,
        ModulePlayerCountSource::PlayerQuery
    );
    assert_eq!(
        rust.runtime.player_query.as_ref().unwrap().protocol,
        "a2s_info"
    );
}
