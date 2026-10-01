use std::collections::HashMap;
use std::env;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use app_core::{AppState, InstanceStatus, ModuleJoinProfile, PortBinding};
use app_modules::discover_modules;
use app_storage::{
    InstancePortProjection, MAX_INSTANCE_PORT_PROJECTION_INSTANCES, bootstrap_storage,
    read_instance_port_projections,
};
use serde::Serialize;
use tauri::Manager;

use crate::state::{DesktopState, LanDirectoryWorker};

mod diagnostics;

pub(crate) fn report_shutdown_warning(message: &str) {
    diagnostics::record("warning", "app.exit.lan_directory_panicked", message);
}
mod sender;
use sender::{DirectorySender, DirectorySenders};

// These v2 discriminators are wire-stable. Renaming their namespace requires a protocol version bump.
const DIRECTORY_NODE_SCHEMA: &str = "cn.langame.lgsm-directory.node.v2";
const DIRECTORY_SERVER_SCHEMA: &str = "cn.langame.lgsm-directory.server.v2";
const DIRECTORY_MULTICAST_GROUP: Ipv4Addr = Ipv4Addr::new(239, 255, 76, 71);
const DIRECTORY_MULTICAST_PORT: u16 = 47_671;
const DIRECTORY_MULTICAST_TTL: u32 = 1;
const DIRECTORY_EMIT_INTERVAL: Duration = Duration::from_secs(5);
const DIRECTORY_RETRY_INITIAL_INTERVAL: Duration = Duration::from_secs(1);
const DIRECTORY_RETRY_MAX_INTERVAL: Duration = Duration::from_secs(30);
const DIRECTORY_SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(100);
const MAX_RUNNING_INSTANCES_PER_CYCLE: usize = MAX_INSTANCE_PORT_PROJECTION_INSTANCES;
const MAX_DATAGRAM_BYTES: usize = 1_200;
const MAX_NODE_ID_BYTES: usize = 64;
const MAX_NODE_NAME_BYTES: usize = 128;
const MAX_INSTANCE_ID_BYTES: usize = 128;
const MAX_INSTANCE_NAME_BYTES: usize = 128;
const MAX_MODULE_ID_BYTES: usize = 128;
const MAX_MODULE_NAME_BYTES: usize = 128;
const MODULE_JOIN_PROFILE_CACHE_TTL: Duration = Duration::from_secs(30);

static DIRECTORY_SESSION_NODE_ID: OnceLock<String> = OnceLock::new();

#[derive(Debug, Clone)]
struct DirectoryNodeIdentity {
    node_id: String,
    node_name: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct DirectoryNodeEvent {
    schema: &'static str,
    node_id: String,
    node_name: String,
    emitted_at: u64,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct DirectoryServerEvent {
    schema: &'static str,
    node_id: String,
    instance_id: String,
    name: String,
    module_id: String,
    module_name: String,
    running: bool,
    join: Option<DirectoryJoinDescriptor>,
    emitted_at: u64,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum DirectoryJoinDescriptor {
    SteamConnect {
        client_app_id: u32,
        join_port: u16,
        query_port: u16,
    },
}

#[derive(Debug, PartialEq, Eq)]
struct DirectoryCycle {
    node: DirectoryNodeEvent,
    servers: Vec<DirectoryServerEvent>,
}

#[derive(Debug, Default)]
struct ModuleJoinProfileCache {
    modules_root: Option<PathBuf>,
    module_signature: Vec<(String, String)>,
    loaded_at: Option<Instant>,
    profiles: HashMap<String, ModuleJoinProfile>,
}

#[derive(Debug, Default)]
struct DirectoryJoinProjection {
    profiles: HashMap<String, ModuleJoinProfile>,
    ports: HashMap<String, Vec<PortBinding>>,
}

pub fn spawn_lan_directory_broadcaster(app_handle: tauri::AppHandle) -> Result<(), String> {
    let identity = local_node_identity();
    let cancel = Arc::new(AtomicBool::new(false));
    let thread_cancel = Arc::clone(&cancel);
    let thread_app_handle = app_handle.clone();
    let broadcaster_thread = thread::Builder::new()
        .name(String::from("langame-lan-directory"))
        .spawn(move || {
            run_lan_directory_broadcaster(thread_app_handle, identity, thread_cancel);
        })
        .map_err(|error| format!("failed to spawn LanGame LAN directory broadcaster: {error}"))?;

    let state = app_handle.state::<DesktopState>();
    let worker = LanDirectoryWorker::new(cancel, broadcaster_thread);
    if let Err((register_error, worker)) = state.register_lan_directory_worker(worker) {
        return match worker.cancel_and_take_thread().join() {
            Ok(()) => Err(register_error),
            Err(_) => Err(format!(
                "{register_error}; LanGame LAN directory thread panicked during cleanup"
            )),
        };
    }

    Ok(())
}

fn run_lan_directory_broadcaster(
    app_handle: tauri::AppHandle,
    identity: DirectoryNodeIdentity,
    local_cancel: Arc<AtomicBool>,
) {
    let mut senders = DirectorySenders::default();
    let mut last_error = None;
    let mut last_interface_error = None;
    let mut last_projection_error = None;
    let mut retry_interval = DIRECTORY_RETRY_INITIAL_INTERVAL;
    let mut join_profile_cache = ModuleJoinProfileCache::default();

    loop {
        if should_stop(&local_cancel) {
            return;
        }

        match senders.refresh(Instant::now()) {
            Ok(()) => {
                if last_interface_error.take().is_some() {
                    diagnostics::record(
                        "info",
                        "lan_directory.interfaces_recovered",
                        "LanGame LAN directory interface discovery recovered",
                    );
                }
            }
            Err(error) => {
                // Keep previously bound healthy senders if enumeration failed.
                // A failed interface cannot suppress other reachable networks.
                report_directory_failure(&mut last_interface_error, &error);
            }
        }
        let state = app_handle.state::<DesktopState>();
        let app_state_result = {
            let app_state = state
                .app_state
                .read()
                .map_err(|_| String::from("desktop state lock poisoned"));
            app_state.map(|app_state| AppState {
                modules: app_state.modules.clone(),
                instances: app_state.instances.clone(),
                ..AppState::default()
            })
        };
        let publish_result = app_state_result.map(|app_state| {
            let projection = match load_join_projection(&state, &app_state, &mut join_profile_cache)
            {
                Ok(projection) => {
                    if last_projection_error.take().is_some() {
                        diagnostics::record(
                            "info",
                            "lan_directory.projection_recovered",
                            "LanGame LAN directory join projection recovered",
                        );
                    }
                    projection
                }
                Err(error) => {
                    report_directory_projection_failure(&mut last_projection_error, &error);
                    DirectoryJoinProjection::default()
                }
            };
            let emitted_at = current_unix_ms();
            senders.publish(Instant::now(), |sender| {
                // Join eligibility belongs to the actual outgoing address,
                // while node identity stays stable across every interface.
                let cycle = build_directory_cycle(
                    &app_state,
                    &identity,
                    emitted_at,
                    sender.source_ip,
                    &projection,
                );
                if directory_cycle_is_publishable(&cycle) {
                    emit_directory_cycle(sender, &cycle)?;
                }
                Ok(())
            });
        });

        match publish_result {
            Ok(()) => {
                if last_error.take().is_some() {
                    diagnostics::record(
                        "info",
                        "lan_directory.state_recovered",
                        "LanGame LAN directory state snapshot recovered",
                    );
                }
                retry_interval = DIRECTORY_RETRY_INITIAL_INTERVAL;
                if wait_for_shutdown(&local_cancel, senders.next_delay(Instant::now())) {
                    return;
                }
            }
            Err(error) => {
                report_directory_failure(&mut last_error, &error);
                if wait_for_shutdown(&local_cancel, retry_interval) {
                    return;
                }
                retry_interval = next_retry_interval(retry_interval);
            }
        }
    }
}

fn directory_cycle_is_publishable(cycle: &DirectoryCycle) -> bool {
    !cycle.servers.is_empty()
}

fn report_directory_failure(last_error: &mut Option<String>, error: &str) {
    if last_error.as_deref() != Some(error) {
        diagnostics::record(
            "warning",
            "lan_directory.multicast_failed",
            &format!("LanGame LAN directory multicast failed: {error}"),
        );
    }
    *last_error = Some(error.to_owned());
}

fn report_directory_projection_failure(last_error: &mut Option<String>, error: &str) {
    if last_error.as_deref() != Some(error) {
        diagnostics::record(
            "warning",
            "lan_directory.projection_failed",
            &format!("LanGame LAN directory join projection failed: {error}"),
        );
    }
    *last_error = Some(error.to_owned());
}

fn next_retry_interval(current: Duration) -> Duration {
    current.saturating_mul(2).min(DIRECTORY_RETRY_MAX_INTERVAL)
}

fn emit_directory_cycle(sender: &DirectorySender, cycle: &DirectoryCycle) -> Result<(), String> {
    send_event(sender, &cycle.node)?;
    for server in &cycle.servers {
        send_event(sender, server)?;
    }
    Ok(())
}

fn send_event<T: Serialize>(sender: &DirectorySender, event: &T) -> Result<(), String> {
    let datagram = encode_datagram(event)?;
    let sent = sender
        .socket
        .send(&datagram)
        .map_err(|error| format!("failed to send LanGame LAN directory datagram: {error}"))?;
    if sent != datagram.len() {
        return Err(format!(
            "LanGame LAN directory datagram was truncated: sent {sent} of {} bytes",
            datagram.len()
        ));
    }
    Ok(())
}

fn encode_datagram<T: Serialize>(event: &T) -> Result<Vec<u8>, String> {
    let datagram = serde_json::to_vec(event)
        .map_err(|error| format!("failed to serialize LanGame LAN directory event: {error}"))?;
    if datagram.len() > MAX_DATAGRAM_BYTES {
        return Err(format!(
            "LanGame LAN directory datagram exceeds {MAX_DATAGRAM_BYTES} bytes"
        ));
    }
    Ok(datagram)
}

fn load_join_projection(
    state: &DesktopState,
    app_state: &AppState,
    cache: &mut ModuleJoinProfileCache,
) -> Result<DirectoryJoinProjection, String> {
    let running_instances = sorted_running_instances(app_state);
    if running_instances.is_empty() {
        return Ok(DirectoryJoinProjection::default());
    }

    let _storage_operation = state.begin_storage_context_operation("LAN directory projection")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let mut module_signature = app_state
        .modules
        .iter()
        .map(|module| (module.id.clone(), module.version.clone()))
        .collect::<Vec<_>>();
    module_signature.sort_unstable();
    cache.refresh(&storage.paths.modules_root, module_signature)?;

    let instance_ids = running_instances
        .into_iter()
        .filter(|instance| cache.profiles.contains_key(&instance.module_id))
        .map(|instance| instance.id.clone())
        .collect::<Vec<_>>();
    let projections = tauri::async_runtime::block_on(read_instance_port_projections(
        &storage.paths,
        &instance_ids,
    ))
    .map_err(|error| error.to_string())?;

    Ok(DirectoryJoinProjection {
        profiles: cache.profiles.clone(),
        ports: projections
            .into_iter()
            .map(
                |InstancePortProjection {
                     instance_id, ports, ..
                 }| (instance_id, ports),
            )
            .collect(),
    })
}

impl ModuleJoinProfileCache {
    fn refresh(
        &mut self,
        modules_root: &Path,
        module_signature: Vec<(String, String)>,
    ) -> Result<(), String> {
        let cache_is_fresh = self.modules_root.as_deref() == Some(modules_root)
            && self.module_signature == module_signature
            && self
                .loaded_at
                .is_some_and(|loaded_at| loaded_at.elapsed() < MODULE_JOIN_PROFILE_CACHE_TTL);
        if cache_is_fresh {
            return Ok(());
        }

        let descriptors = match discover_modules(modules_root) {
            Ok(descriptors) => descriptors,
            Err(error) => {
                self.invalidate();
                return Err(error.to_string());
            }
        };
        self.profiles = descriptors
            .into_iter()
            .filter_map(|descriptor| {
                descriptor
                    .runtime
                    .join
                    .map(|profile| (descriptor.summary.id, profile))
            })
            .collect();
        self.modules_root = Some(modules_root.to_path_buf());
        self.module_signature = module_signature;
        self.loaded_at = Some(Instant::now());
        Ok(())
    }

    fn invalidate(&mut self) {
        self.modules_root = None;
        self.module_signature.clear();
        self.loaded_at = None;
        self.profiles.clear();
    }
}

fn sorted_running_instances(app_state: &AppState) -> Vec<&app_core::InstanceSummary> {
    let mut instances = app_state
        .instances
        .iter()
        .filter(|instance| matches!(instance.status, InstanceStatus::Running))
        .collect::<Vec<_>>();
    instances.sort_by(|left, right| left.id.cmp(&right.id));
    instances.truncate(MAX_RUNNING_INSTANCES_PER_CYCLE);
    instances
}

fn build_directory_cycle(
    app_state: &AppState,
    identity: &DirectoryNodeIdentity,
    emitted_at: u64,
    source_ip: Ipv4Addr,
    projection: &DirectoryJoinProjection,
) -> DirectoryCycle {
    let module_names = app_state
        .modules
        .iter()
        .map(|module| (module.id.as_str(), module.name.as_str()))
        .collect::<HashMap<_, _>>();
    let servers = sorted_running_instances(app_state)
        .into_iter()
        .filter_map(|instance| {
            let instance_id = bounded_protocol_id(&instance.id, MAX_INSTANCE_ID_BYTES)?;
            let module_id = bounded_protocol_id(&instance.module_id, MAX_MODULE_ID_BYTES)?;
            let name = bounded_display_text(&instance.name, &instance_id, MAX_INSTANCE_NAME_BYTES);
            let module_name = bounded_display_text(
                module_names
                    .get(instance.module_id.as_str())
                    .copied()
                    .unwrap_or(module_id.as_str()),
                &module_id,
                MAX_MODULE_NAME_BYTES,
            );

            Some(DirectoryServerEvent {
                schema: DIRECTORY_SERVER_SCHEMA,
                node_id: identity.node_id.clone(),
                instance_id,
                name,
                module_id,
                module_name,
                running: true,
                join: resolve_join_descriptor(instance, source_ip, projection),
                emitted_at,
            })
        })
        .collect();

    DirectoryCycle {
        node: DirectoryNodeEvent {
            schema: DIRECTORY_NODE_SCHEMA,
            node_id: identity.node_id.clone(),
            node_name: identity.node_name.clone(),
            emitted_at,
        },
        servers,
    }
}

fn resolve_join_descriptor(
    instance: &app_core::InstanceSummary,
    source_ip: Ipv4Addr,
    projection: &DirectoryJoinProjection,
) -> Option<DirectoryJoinDescriptor> {
    let bind_ip = instance.bind_ip.trim().parse::<Ipv4Addr>().ok()?;
    if !bind_ip.is_unspecified() && bind_ip != source_ip {
        return None;
    }
    let profile = projection.profiles.get(&instance.module_id)?;
    let ports = projection.ports.get(&instance.id)?;

    match profile {
        ModuleJoinProfile::SteamConnect {
            client_app_id,
            join_port_name,
            query_port_name,
        } => Some(DirectoryJoinDescriptor::SteamConnect {
            client_app_id: *client_app_id,
            join_port: unique_nonzero_port(ports, join_port_name, None)?,
            query_port: unique_nonzero_port(ports, query_port_name, Some("udp"))?,
        }),
    }
}

fn unique_nonzero_port(
    ports: &[PortBinding],
    port_name: &str,
    required_protocol: Option<&str>,
) -> Option<u16> {
    let mut matches = ports.iter().filter(|port| port.name == port_name);
    let binding = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    if binding.port == 0 || required_protocol.is_some_and(|protocol| binding.protocol != protocol) {
        return None;
    }
    Some(binding.port)
}

fn local_node_identity() -> DirectoryNodeIdentity {
    let node_id = DIRECTORY_SESSION_NODE_ID
        .get_or_init(|| uuid::Uuid::new_v4().simple().to_string())
        .clone();
    debug_assert!(node_id.len() <= MAX_NODE_ID_BYTES);
    let node_name = bounded_display_text(
        env::var("COMPUTERNAME")
            .as_deref()
            .unwrap_or("LanGame Node"),
        "LanGame Node",
        MAX_NODE_NAME_BYTES,
    );
    DirectoryNodeIdentity { node_id, node_name }
}

fn bounded_protocol_id(value: &str, max_bytes: usize) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > max_bytes
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return None;
    }
    Some(value.to_owned())
}

fn bounded_display_text(value: &str, fallback: &str, max_bytes: usize) -> String {
    let sanitized = sanitize_display_text(value, max_bytes);
    if !sanitized.is_empty() {
        return sanitized;
    }

    let fallback = sanitize_display_text(fallback, max_bytes);
    if fallback.is_empty() {
        String::from("Unknown")
    } else {
        fallback
    }
}

fn sanitize_display_text(value: &str, max_bytes: usize) -> String {
    let mut output = String::with_capacity(value.len().min(max_bytes));
    let mut pending_space = false;

    for character in value.chars() {
        if is_bidi_control(character) {
            continue;
        }
        if character.is_whitespace() {
            pending_space = !output.is_empty();
            continue;
        }
        if character.is_control() {
            continue;
        }

        let separator_bytes = usize::from(pending_space);
        if output.len() + separator_bytes + character.len_utf8() > max_bytes {
            break;
        }
        if pending_space {
            output.push(' ');
            pending_space = false;
        }
        output.push(character);
    }

    output
}

fn is_bidi_control(character: char) -> bool {
    matches!(
        character,
        '\u{061c}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}'
    )
}

fn should_stop(cancel: &AtomicBool) -> bool {
    cancel.load(Ordering::SeqCst)
}

fn wait_for_shutdown(cancel: &AtomicBool, duration: Duration) -> bool {
    let mut remaining = duration;
    while !remaining.is_zero() {
        if should_stop(cancel) {
            return true;
        }
        let sleep_for = remaining.min(DIRECTORY_SHUTDOWN_POLL_INTERVAL);
        thread::sleep(sleep_for);
        remaining = remaining.saturating_sub(sleep_for);
    }
    should_stop(cancel)
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use app_core::{InstallState, InstanceSummary, ModuleSummary};
    use serde_json::Value;

    use super::*;

    fn identity() -> DirectoryNodeIdentity {
        DirectoryNodeIdentity {
            node_id: String::from("0123456789abcdef0123456789abcdef"),
            node_name: String::from("LanGame Test Node"),
        }
    }

    fn module(id: &str, name: &str) -> ModuleSummary {
        ModuleSummary {
            id: id.to_owned(),
            name: name.to_owned(),
            version: String::from("1"),
            description: None,
            steam_app_id: None,
            install_state: InstallState::Installed,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        }
    }

    fn instance(id: &str, status: InstanceStatus) -> InstanceSummary {
        InstanceSummary {
            id: id.to_owned(),
            name: format!("Server {id}"),
            module_id: String::from("minecraft"),
            active_process_count: usize::from(matches!(&status, InstanceStatus::Running)),
            status,
            bind_ip: String::from("0.0.0.0"),
            port_count: 2,
            autostart: false,
        }
    }

    fn empty_projection() -> DirectoryJoinProjection {
        DirectoryJoinProjection::default()
    }

    fn rust_projection(
        instance_id: &str,
        join_port: u16,
        query_port: u16,
    ) -> DirectoryJoinProjection {
        DirectoryJoinProjection {
            profiles: HashMap::from([(
                String::from("rust"),
                ModuleJoinProfile::SteamConnect {
                    client_app_id: 252490,
                    join_port_name: String::from("game"),
                    query_port_name: String::from("query"),
                },
            )]),
            ports: HashMap::from([(
                instance_id.to_owned(),
                vec![
                    PortBinding {
                        name: String::from("game"),
                        protocol: String::from("udp"),
                        port: join_port,
                    },
                    PortBinding {
                        name: String::from("query"),
                        protocol: String::from("udp"),
                        port: query_port,
                    },
                ],
            )]),
        }
    }

    fn build_test_cycle(
        app_state: &AppState,
        emitted_at: u64,
        source_ip: Ipv4Addr,
        projection: &DirectoryJoinProjection,
    ) -> DirectoryCycle {
        build_directory_cycle(app_state, &identity(), emitted_at, source_ip, projection)
    }

    #[test]
    fn node_event_uses_the_exact_v2_golden_datagram() {
        let cycle = build_test_cycle(
            &AppState::default(),
            1_700_000_000_000,
            Ipv4Addr::new(192, 168, 1, 44),
            &empty_projection(),
        );
        let datagram = encode_datagram(&cycle.node).expect("serialize node event");

        assert_eq!(
            String::from_utf8(datagram.clone()).expect("UTF-8 datagram"),
            r#"{"schema":"cn.langame.lgsm-directory.node.v2","node_id":"0123456789abcdef0123456789abcdef","node_name":"LanGame Test Node","emitted_at":1700000000000}"#
        );
        assert!(datagram.len() <= MAX_DATAGRAM_BYTES);
    }

    #[test]
    fn server_event_uses_actual_ports_in_the_exact_v2_golden_datagram() {
        let mut running = instance("rust-one", InstanceStatus::Running);
        running.module_id = String::from("rust");
        running.name = String::from("Rust One");
        let app_state = AppState {
            modules: vec![module("rust", "Rust Dedicated Server")],
            instances: vec![running],
            ..AppState::default()
        };
        let projection = rust_projection("rust-one", 31_015, 31_017);

        let cycle = build_test_cycle(
            &app_state,
            1_700_000_000_000,
            Ipv4Addr::new(192, 168, 1, 44),
            &projection,
        );
        let datagram = encode_datagram(&cycle.servers[0]).expect("serialize server event");

        assert_eq!(
            String::from_utf8(datagram.clone()).expect("UTF-8 datagram"),
            r#"{"schema":"cn.langame.lgsm-directory.server.v2","node_id":"0123456789abcdef0123456789abcdef","instance_id":"rust-one","name":"Rust One","module_id":"rust","module_name":"Rust Dedicated Server","running":true,"join":{"kind":"steam_connect","client_app_id":252490,"join_port":31015,"query_port":31017},"emitted_at":1700000000000}"#
        );
        assert!(datagram.len() <= MAX_DATAGRAM_BYTES);
    }

    #[test]
    fn node_identity_survives_broadcaster_recovery_within_the_process() {
        assert_eq!(local_node_identity().node_id, local_node_identity().node_id);
    }

    #[test]
    fn cycle_without_a_running_server_is_not_published() {
        let cycle = build_test_cycle(
            &AppState::default(),
            42,
            Ipv4Addr::LOCALHOST,
            &empty_projection(),
        );

        assert!(!directory_cycle_is_publishable(&cycle));
    }

    #[test]
    fn server_events_are_running_only_sorted_and_bounded() {
        let mut app_state = AppState {
            modules: vec![module("minecraft", "Minecraft")],
            ..AppState::default()
        };
        app_state
            .instances
            .push(instance("stopped", InstanceStatus::Stopped));
        for index in (0..130).rev() {
            app_state.instances.push(instance(
                &format!("instance-{index:03}"),
                InstanceStatus::Running,
            ));
        }

        let cycle = build_test_cycle(&app_state, 42, Ipv4Addr::LOCALHOST, &empty_projection());

        assert_eq!(cycle.servers.len(), MAX_RUNNING_INSTANCES_PER_CYCLE);
        assert_eq!(cycle.servers[0].instance_id, "instance-000");
        assert_eq!(cycle.servers[127].instance_id, "instance-127");
        assert!(cycle.servers.iter().all(|server| server.running));
        assert!(
            cycle
                .servers
                .iter()
                .all(|server| server.module_name == "Minecraft")
        );
        assert!(
            !cycle
                .servers
                .iter()
                .any(|server| server.instance_id == "stopped")
        );
    }

    #[test]
    fn server_event_has_only_public_bounded_fields() {
        let mut running = instance("instance-safe", InstanceStatus::Running);
        running.name = format!("  {}\nsecret  ", "\\\"".repeat(96));
        let app_state = AppState {
            modules: vec![module("minecraft", &"模".repeat(256))],
            instances: vec![running],
            ..AppState::default()
        };

        let cycle = build_test_cycle(&app_state, 77, Ipv4Addr::LOCALHOST, &empty_projection());
        let server = cycle.servers.first().expect("running server event");
        let datagram = encode_datagram(server).expect("serialize server event");
        let value = serde_json::from_slice::<Value>(&datagram).expect("parse server event");
        let keys = value
            .as_object()
            .expect("server object")
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();

        assert_eq!(
            keys,
            BTreeSet::from([
                "emitted_at",
                "instance_id",
                "join",
                "module_id",
                "module_name",
                "name",
                "node_id",
                "running",
                "schema",
            ])
        );
        assert!(server.name.len() <= MAX_INSTANCE_NAME_BYTES);
        assert!(server.module_name.len() <= MAX_MODULE_NAME_BYTES);
        assert!(!server.name.chars().any(char::is_control));
        assert!(value["join"].is_null());
        assert!(datagram.len() <= MAX_DATAGRAM_BYTES);
    }

    #[test]
    fn invalid_or_oversized_identifiers_are_not_advertised() {
        let app_state = AppState {
            modules: vec![module("minecraft", "Minecraft")],
            instances: vec![
                instance("unsafe/id", InstanceStatus::Running),
                instance(
                    &"x".repeat(MAX_INSTANCE_ID_BYTES + 1),
                    InstanceStatus::Running,
                ),
            ],
            ..AppState::default()
        };

        let cycle = build_test_cycle(&app_state, 12, Ipv4Addr::LOCALHOST, &empty_projection());

        assert!(cycle.servers.is_empty());
    }

    #[test]
    fn join_is_null_for_missing_ports_unsupported_modules_and_bind_mismatch() {
        let source_ip = Ipv4Addr::new(192, 168, 1, 44);
        let mut wildcard = instance("wildcard", InstanceStatus::Running);
        wildcard.module_id = String::from("rust");
        let mut exact = instance("exact", InstanceStatus::Running);
        exact.module_id = String::from("rust");
        exact.bind_ip = source_ip.to_string();
        let mut mismatch = instance("mismatch", InstanceStatus::Running);
        mismatch.module_id = String::from("rust");
        mismatch.bind_ip = String::from("192.168.1.45");
        let unsupported = instance("unsupported", InstanceStatus::Running);
        let mut projection = rust_projection("wildcard", 31_015, 31_017);
        projection.ports.insert(
            String::from("exact"),
            rust_projection("exact", 32_015, 32_017)
                .ports
                .remove("exact")
                .expect("exact ports"),
        );
        projection.ports.insert(
            String::from("mismatch"),
            rust_projection("mismatch", 33_015, 33_017)
                .ports
                .remove("mismatch")
                .expect("mismatch ports"),
        );
        projection.ports.insert(
            String::from("unsupported"),
            vec![
                PortBinding {
                    name: String::from("game"),
                    protocol: String::from("udp"),
                    port: 34_015,
                },
                PortBinding {
                    name: String::from("query"),
                    protocol: String::from("udp"),
                    port: 34_017,
                },
            ],
        );
        let app_state = AppState {
            modules: vec![module("rust", "Rust"), module("minecraft", "Minecraft")],
            instances: vec![wildcard, exact, mismatch, unsupported],
            ..AppState::default()
        };

        let cycle = build_test_cycle(&app_state, 90, source_ip, &projection);
        let joins = cycle
            .servers
            .iter()
            .map(|server| (server.instance_id.as_str(), server.join.as_ref()))
            .collect::<HashMap<_, _>>();

        assert!(joins["wildcard"].is_some());
        assert!(joins["exact"].is_some());
        assert!(joins["mismatch"].is_none());
        assert!(joins["unsupported"].is_none());
    }

    #[test]
    fn core_keeper_a2s_status_remains_visible_but_non_joinable() {
        let mut running = instance("corekeeper-one", InstanceStatus::Running);
        running.module_id = String::from("corekeeper");
        running.name = String::from("Core Keeper One");
        let app_state = AppState {
            modules: vec![module("corekeeper", "Core Keeper Dedicated Server")],
            instances: vec![running],
            ..AppState::default()
        };
        let mut projection = empty_projection();
        projection.ports.insert(
            String::from("corekeeper-one"),
            vec![
                PortBinding {
                    name: String::from("game"),
                    protocol: String::from("udp"),
                    port: 27_017,
                },
                PortBinding {
                    name: String::from("query"),
                    protocol: String::from("udp"),
                    port: 27_018,
                },
            ],
        );

        let cycle = build_test_cycle(
            &app_state,
            1_700_000_000_000,
            Ipv4Addr::new(192, 168, 1, 44),
            &projection,
        );
        let server = cycle.servers.first().expect("Core Keeper server event");
        assert!(server.join.is_none());
        let value: Value = serde_json::from_slice(
            &encode_datagram(server).expect("serialize Core Keeper server event"),
        )
        .expect("parse Core Keeper server event");
        assert!(value["join"].is_null());
        assert!(value.get("game_id").is_none());
        assert!(value.get("password").is_none());
    }

    #[test]
    fn join_is_null_when_an_actual_port_is_missing_zero_or_ambiguous() {
        let source_ip = Ipv4Addr::new(192, 168, 1, 44);
        let mut running = instance("rust-one", InstanceStatus::Running);
        running.module_id = String::from("rust");
        let app_state = AppState {
            modules: vec![module("rust", "Rust")],
            instances: vec![running],
            ..AppState::default()
        };
        let mut projection = rust_projection("rust-one", 31_015, 0);
        assert!(
            build_test_cycle(&app_state, 1, source_ip, &projection).servers[0]
                .join
                .is_none()
        );

        projection.ports.get_mut("rust-one").expect("ports")[1].port = 31_017;
        projection
            .ports
            .get_mut("rust-one")
            .expect("ports")
            .push(PortBinding {
                name: String::from("query"),
                protocol: String::from("udp"),
                port: 41_017,
            });
        assert!(
            build_test_cycle(&app_state, 2, source_ip, &projection).servers[0]
                .join
                .is_none()
        );

        projection.ports.remove("rust-one");
        assert!(
            build_test_cycle(&app_state, 3, source_ip, &projection).servers[0]
                .join
                .is_none()
        );
    }

    #[test]
    fn join_is_null_when_the_actual_query_binding_is_not_udp() {
        let source_ip = Ipv4Addr::new(192, 168, 1, 44);
        let mut running = instance("rust-one", InstanceStatus::Running);
        running.module_id = String::from("rust");
        let app_state = AppState {
            modules: vec![module("rust", "Rust")],
            instances: vec![running],
            ..AppState::default()
        };
        let mut projection = rust_projection("rust-one", 31_015, 31_017);
        projection.ports.get_mut("rust-one").expect("ports")[1].protocol = String::from("tcp");

        assert!(
            build_test_cycle(&app_state, 4, source_ip, &projection).servers[0]
                .join
                .is_none()
        );
    }

    #[test]
    fn multicast_retry_backoff_is_bounded() {
        assert_eq!(
            next_retry_interval(DIRECTORY_RETRY_INITIAL_INTERVAL),
            Duration::from_secs(2)
        );
        assert_eq!(
            next_retry_interval(DIRECTORY_RETRY_MAX_INTERVAL),
            DIRECTORY_RETRY_MAX_INTERVAL
        );
    }

    #[test]
    fn broadcaster_wait_observes_only_its_dedicated_cancel() {
        let cancel = AtomicBool::new(true);

        assert!(wait_for_shutdown(&cancel, Duration::from_secs(1)));
    }

    #[test]
    fn display_text_removes_every_v2_bidi_control() {
        let bidi_controls = [
            '\u{061c}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}',
            '\u{202e}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
        ]
        .into_iter()
        .collect::<String>();

        assert_eq!(
            sanitize_display_text(&format!("left{bidi_controls}right"), 128),
            "leftright"
        );
    }
}
