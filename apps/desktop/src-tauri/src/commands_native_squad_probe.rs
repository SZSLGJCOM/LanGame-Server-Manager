use std::path::Path;

use app_core::{InstanceDetails, ModulePlayerListCodec, ModulePlayerListSource, ProcessIdentity};
use app_modules::ModuleDescriptor;
use app_platform_win::{ProcessNetworkEndpoint, WindowInspectionTarget, WindowsPlatform};

pub(super) async fn report(
    descriptor: &ModuleDescriptor,
    instance: &InstanceDetails,
    install_root: &Path,
    fixture_root: &Path,
) {
    match query(descriptor, instance, install_root, fixture_root).await {
        Ok(report) => eprintln!("NATIVE_DIAGNOSTIC phase=readiness squad_rcon={report}"),
        Err(category) => {
            eprintln!("NATIVE_DIAGNOSTIC phase=readiness squad_rcon_error={category}");
        }
    }
}

pub(super) async fn query(
    descriptor: &ModuleDescriptor,
    instance: &InstanceDetails,
    install_root: &Path,
    fixture_root: &Path,
) -> Result<String, &'static str> {
    let descriptor = descriptor.clone();
    let instance = instance.clone();
    let install_root = install_root.to_owned();
    let fixture_root = fixture_root.to_owned();
    tokio::task::spawn_blocking(move || {
        inspect(&descriptor, &instance, &install_root, Some(&fixture_root))
    })
    .await
    .map_err(|_| "worker_failed")?
}

/// The selected existing instance is inspected only. Its program root and
/// configuration file are separately authorized by the pinned backend; neither
/// root is represented as a disposable fixture or eligible for cleanup.
pub(super) async fn query_existing(
    descriptor: &ModuleDescriptor,
    instance: &InstanceDetails,
    install_root: &Path,
) -> Result<String, &'static str> {
    let descriptor = descriptor.clone();
    let instance = instance.clone();
    let install_root = install_root.to_owned();
    tokio::task::spawn_blocking(move || inspect(&descriptor, &instance, &install_root, None))
        .await
        .map_err(|_| "worker_failed")?
}

fn inspect(
    descriptor: &ModuleDescriptor,
    instance: &InstanceDetails,
    install_root: &Path,
    fixture_root: Option<&Path>,
) -> Result<String, &'static str> {
    validate_contract(descriptor)?;
    if instance.summary.module_id != descriptor.summary.id {
        return Err("instance_module_mismatch");
    }
    let authorized_root = fixture_root
        .unwrap_or(install_root)
        .canonicalize()
        .map_err(|_| "authorized_program_root_unavailable")?;
    require_fixture_path(install_root, &authorized_root)?;
    if fixture_root.is_some() {
        require_fixture_path(Path::new(&instance.config_file_path), &authorized_root)?;
    } else if !Path::new(&instance.config_file_path).is_file() {
        return Err("existing_instance_configuration_unavailable");
    }
    let targets = crate::commands::commands_runtime_supervision::build_window_inspection_targets_from_instance(instance);
    if targets.is_empty() {
        return Err("managed_identity_missing");
    }
    for target in &targets {
        require_fixture_path(
            Path::new(&target.process_identity.image_path),
            &authorized_root,
        )?;
    }
    verify_targets(&targets)?;
    let port = instance
        .ports
        .iter()
        .find(|port| port.name == "rcon" && port.protocol.eq_ignore_ascii_case("tcp"))
        .map(|port| port.port)
        .filter(|port| *port != 0)
        .ok_or("rcon_binding_missing")?;
    let ports = instance
        .ports
        .iter()
        .map(|port| port.port)
        .collect::<Vec<_>>();
    let before = WindowsPlatform::inspect_process_network_endpoints(&targets, &ports)
        .map_err(|_| "endpoint_inspection_failed")?;
    let endpoint = rcon_endpoint(&before.endpoints, port)?;
    let owner = app_runtime::inspect_process_identity(endpoint.owning_pid)
        .map_err(|_| "rcon_identity_unavailable")?
        .ok_or("rcon_owner_exited")?;
    require_fixture_path(Path::new(&owner.image_path), &authorized_root)?;
    let settings: serde_json::Value =
        serde_json::from_str(&instance.settings_json).map_err(|_| "settings_invalid")?;
    let password = settings["rcon_password"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("rcon_password_missing")?;
    let response = crate::runtime_transport::player_list_rcon_exec(
        &format!("127.0.0.1:{port}"),
        password,
        "ListPlayers",
    );
    // A response is evidence only while the same managed process generation
    // still owns this listening endpoint. Never print transport response text.
    verify_targets(&targets)?;
    verify_owner(endpoint.owning_pid, &owner)?;
    let after = WindowsPlatform::inspect_process_network_endpoints(&targets, &ports)
        .map_err(|_| "endpoint_recheck_failed")?;
    if !after.endpoints.contains(endpoint) {
        return Err("rcon_endpoint_changed");
    }
    let response = response.map_err(|error| transport_category(&error))?;
    let roster = roster_summary(&response)?;
    let query_port = instance.ports.iter().find(|port| port.name == "query");
    let query_bindings = query_port
        .map(|port| {
            before
                .endpoints
                .iter()
                .filter(|endpoint| {
                    endpoint.protocol.eq_ignore_ascii_case("udp")
                        && endpoint.local_port == port.port
                })
                .take(8)
                .map(|endpoint| binding_kind(&endpoint.local_address))
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    Ok(format!("{roster} query_bindings=[{query_bindings}]"))
}

pub(super) fn validate_contract(descriptor: &ModuleDescriptor) -> Result<(), &'static str> {
    if descriptor.summary.id != "squad"
        || !descriptor.runtime.player_list.as_ref().is_some_and(|list| {
            list.source == ModulePlayerListSource::RuntimeAction
                && list.action_id.as_deref() == Some("list_players")
                && list.response_codec == ModulePlayerListCodec::SquadListPlayers
        })
        || !descriptor.runtime.player_actions.iter().any(|action| {
            action.id == "list_players"
                && action.transport == "source_rcon"
                && action.command_template == "ListPlayers"
                && action.port_name.as_deref() == Some("rcon")
                && action.password_setting_key.as_deref() == Some("rcon_password")
        })
    {
        return Err("read_only_contract_mismatch");
    }
    Ok(())
}

fn require_fixture_path(path: &Path, root: &Path) -> Result<(), &'static str> {
    let path = path
        .canonicalize()
        .map_err(|_| "fixture_path_unavailable")?;
    let path = path.to_string_lossy().to_ascii_lowercase();
    let root = root.to_string_lossy().to_ascii_lowercase();
    if !Path::new(&path).starts_with(Path::new(&root)) {
        return Err("path_outside_fixture");
    }
    Ok(())
}

fn verify_targets(targets: &[WindowInspectionTarget]) -> Result<(), &'static str> {
    for target in targets {
        verify_owner(target.pid, &target.process_identity)?;
    }
    Ok(())
}

fn verify_owner(pid: u32, identity: &ProcessIdentity) -> Result<(), &'static str> {
    if app_runtime::inspect_process_identity(pid)
        .map_err(|_| "identity_check_failed")?
        .as_ref()
        != Some(identity)
    {
        return Err("managed_process_generation_changed");
    }
    Ok(())
}

fn rcon_endpoint(
    endpoints: &[ProcessNetworkEndpoint],
    port: u16,
) -> Result<&ProcessNetworkEndpoint, &'static str> {
    let mut matches = endpoints.iter().filter(|endpoint| {
        endpoint.local_port == port
            && endpoint.protocol.eq_ignore_ascii_case("tcp")
            && matches!(endpoint.local_address.as_str(), "0.0.0.0" | "127.0.0.1")
    });
    let result = matches
        .next()
        .ok_or("managed_loopback_rcon_endpoint_missing")?;
    if matches.next().is_some() {
        return Err("managed_rcon_endpoint_ambiguous");
    }
    Ok(result)
}

fn binding_kind(address: &str) -> &'static str {
    match address.parse::<std::net::IpAddr>() {
        Ok(address) if address.is_unspecified() => "wildcard",
        Ok(address) if address.is_loopback() => "loopback",
        Ok(_) => "specific_local_interface",
        Err(_) => "invalid_address",
    }
}

fn transport_category(error: &str) -> &'static str {
    if error.contains("authentication") {
        "authentication_failed"
    } else if error.contains("connect") {
        "connection_failed"
    } else if error.contains("terminator") {
        "response_boundary_missing"
    } else if error.contains("packet") {
        "packet_exchange_failed"
    } else {
        "transport_failed"
    }
}

fn roster_summary(response: &str) -> Result<String, &'static str> {
    let roster = crate::live_players::response_codecs::parse(
        ModulePlayerListCodec::SquadListPlayers,
        response,
        "native-diagnostic",
        0,
        &[],
    )
    .map_err(|_| "roster_codec_rejected")?;
    if !roster.complete || roster.truncated {
        return Err("roster_incomplete");
    }
    Ok(format!("complete_roster players={}", roster.entries.len()))
}

#[test]
fn native_squad_diagnostic_accepts_only_complete_rosters_and_omits_response_text() {
    assert_eq!(
        roster_summary(
            "----- Active Players -----\n----- Recently Disconnected Players [Max of 15] -----\n"
        ),
        Ok("complete_roster players=0".into())
    );
    for invalid in ["", "----- Active Players -----", "private-player-identity"] {
        assert_eq!(roster_summary(invalid), Err("roster_codec_rejected"));
    }
}

#[test]
fn native_squad_diagnostic_requires_one_owned_loopback_listener() {
    let endpoint = ProcessNetworkEndpoint {
        protocol: "tcp".into(),
        local_address: "0.0.0.0".into(),
        local_port: 21114,
        owning_pid: 123,
        process_key: "main".into(),
        relation: "root".into(),
    };
    assert_eq!(
        rcon_endpoint(&[], 21114),
        Err("managed_loopback_rcon_endpoint_missing")
    );
    assert!(rcon_endpoint(std::slice::from_ref(&endpoint), 21114).is_ok());
    assert_eq!(
        rcon_endpoint(&[endpoint.clone(), endpoint], 21114),
        Err("managed_rcon_endpoint_ambiguous")
    );
    assert_eq!(binding_kind("192.0.2.1"), "specific_local_interface");
}
