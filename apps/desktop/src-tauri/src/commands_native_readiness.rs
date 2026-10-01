use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use app_core::InstanceDetails;
use app_modules::ModuleDescriptor;
use app_platform_win::WindowsPlatform;
use serde::Deserialize;

#[path = "commands_native_log_readiness.rs"]
mod native_logs;
use native_logs::{LogBaseline, new_log_contains};

#[path = "commands_native_ark_maps_readiness.rs"]
pub(super) mod ark_maps;

#[path = "commands_native_minecraft_readiness.rs"]
mod minecraft_status;
#[path = "commands_native_status_readiness.rs"]
mod native_status;
#[path = "commands_native_windrose_readiness.rs"]
mod windrose_world;

pub(super) struct ProbeBaselines {
    logs: HashMap<PathBuf, LogBaseline>,
    returntomoria: Option<native_status::Baseline>,
}

#[derive(Clone, Copy)]
enum ProbeScope<'a> {
    Disposable(&'a Path),
    ExistingInstance,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SmokeManifest {
    schema_version: u32,
    pub module_id: String,
    primary_process_key: String,
    readiness_timeout_ms: u64,
    poll_interval_ms: u64,
    stability_window_ms: u64,
    probe_mode: String,
    #[serde(default)]
    pub fixture: SmokeFixture,
    #[serde(default)]
    pub preconditions: Vec<SmokePrecondition>,
    probes: Vec<Probe>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SmokeFixture {
    #[serde(default)]
    pub settings: serde_json::Map<String, serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SmokePrecondition {
    pub id: String,
    pub kind: String,
    pub environment: Option<String>,
    pub expected: Option<String>,
    pub setting_key: Option<String>,
    pub reason_code: String,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Probe {
    ProcessEndpoint {
        id: String,
        process_key: String,
        port_name: String,
        protocol: String,
    },
    PlayerQuery {
        id: String,
        port_name: String,
    },
    HumanitzInfo {
        id: String,
        port_name: String,
    },
    SquadPlayers {
        id: String,
        port_name: String,
    },
    AstroneerPlayers {
        id: String,
        port_name: String,
    },
    AstroneerWorld {
        id: String,
        port_name: String,
    },
    ReturntomoriaStatus {
        id: String,
    },
    WindroseWorld {
        id: String,
    },
    TcpConnect {
        id: String,
        port_name: String,
    },
    LogMarker {
        id: String,
        sources: Vec<LogSource>,
        any_of: Vec<String>,
    },
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum LogSource {
    Process { process_key: String },
    Instance { path: PathBuf },
    Install { path: PathBuf },
}

impl Probe {
    fn id(&self) -> &str {
        match self {
            Self::ProcessEndpoint { id, .. }
            | Self::PlayerQuery { id, .. }
            | Self::HumanitzInfo { id, .. }
            | Self::SquadPlayers { id, .. }
            | Self::AstroneerPlayers { id, .. }
            | Self::AstroneerWorld { id, .. }
            | Self::ReturntomoriaStatus { id }
            | Self::WindroseWorld { id }
            | Self::TcpConnect { id, .. }
            | Self::LogMarker { id, .. } => id,
        }
    }
}

impl SmokeManifest {
    pub(super) fn readiness_budget(&self) -> Duration {
        Duration::from_millis(self.readiness_timeout_ms)
    }

    pub fn load(descriptor: &ModuleDescriptor) -> Result<Self, String> {
        let content = std::fs::read_to_string(descriptor.root.join("smoke.toml"))
            .map_err(|_| "module has no readable smoke.toml contract")?;
        let value: Self =
            toml::from_str(&content).map_err(|error| format!("invalid smoke contract: {error}"))?;
        if value.schema_version != 1
            || value.module_id != descriptor.summary.id
            || value.probes.is_empty()
            || value.probe_mode != "all"
            || !(1..=600_000).contains(&value.readiness_timeout_ms)
            || !(50..=5_000).contains(&value.poll_interval_ms)
            || value.stability_window_ms > value.readiness_timeout_ms
        {
            return Err("unsupported or unbounded smoke contract".into());
        }
        for probe in &value.probes {
            if let Probe::SquadPlayers { port_name, .. } = probe
                && (port_name != "rcon"
                    || super::squad_probe::validate_contract(descriptor).is_err())
            {
                return Err("Squad probe requires the declared read-only player action".into());
            }
            if matches!(probe, Probe::ReturntomoriaStatus { .. })
                && descriptor.summary.id != "returntomoria"
            {
                return Err("Return to Moria status probe requires its native module".into());
            }
            if matches!(probe, Probe::WindroseWorld { .. }) && descriptor.summary.id != "windrose" {
                return Err("Windrose world probe requires its native module".into());
            }
            if let Probe::AstroneerPlayers { port_name, .. }
            | Probe::AstroneerWorld { port_name, .. } = probe
                && (descriptor.summary.id != "astroneer"
                    || port_name != "console"
                    || !descriptor.runtime.player_list.as_ref().is_some_and(|list| {
                        list.source == app_core::ModulePlayerListSource::TcpConsole
                            && list.response_codec
                                == app_core::ModulePlayerListCodec::AstroneerPlayers
                    }))
            {
                return Err("ASTRONEER probe requires the declared TCP player console".into());
            }
            if let Probe::HumanitzInfo { port_name, .. } = probe
                && (descriptor.summary.id != "humanitz"
                    || !descriptor.runtime.player_actions.iter().any(|action| {
                        action.id == "list_online_players"
                            && action.transport == "humanitz_rcon"
                            && action.command_template == "info"
                            && action.port_name.as_ref() == Some(port_name)
                            && action.password_setting_key.as_deref() == Some("rcon_password")
                            && action.enabled_setting_key.as_deref() == Some("rcon_enabled")
                    }))
            {
                return Err(
                    "HumanitZ info probe requires the declared read-only player action".into(),
                );
            }
            match probe {
                Probe::ReturntomoriaStatus { .. } | Probe::WindroseWorld { .. } => {}
                Probe::LogMarker {
                    sources, any_of, ..
                } => {
                    if sources.is_empty()
                        || any_of.is_empty()
                        || any_of.iter().any(String::is_empty)
                    {
                        return Err(
                            "log readiness requires explicit sources and nonempty markers".into(),
                        );
                    }
                    for source in sources {
                        if let LogSource::Instance { path } | LogSource::Install { path } = source
                            && (path.as_os_str().is_empty()
                                || !path
                                    .components()
                                    .all(|part| matches!(part, Component::Normal(_))))
                        {
                            return Err(
                                "smoke log paths must stay inside the disposable package".into()
                            );
                        }
                    }
                }
                Probe::ProcessEndpoint { port_name, .. }
                | Probe::PlayerQuery { port_name, .. }
                | Probe::HumanitzInfo { port_name, .. }
                | Probe::SquadPlayers { port_name, .. }
                | Probe::AstroneerPlayers { port_name, .. }
                | Probe::AstroneerWorld { port_name, .. }
                | Probe::TcpConnect { port_name, .. } => {
                    if !descriptor
                        .default_ports
                        .iter()
                        .any(|port| &port.name == port_name)
                    {
                        return Err("smoke probe references an undeclared port".into());
                    }
                }
            }
        }
        Ok(value)
    }

    pub fn require_fresh_world(
        &self,
        install_root: &Path,
        fixture_root: &Path,
    ) -> Result<(), String> {
        if self
            .probes
            .iter()
            .any(|probe| matches!(probe, Probe::WindroseWorld { .. }))
        {
            windrose_world::require_fresh(install_root, fixture_root)?;
        }
        Ok(())
    }

    pub fn log_baselines(
        &self,
        instance: &InstanceDetails,
        install_root: &Path,
    ) -> Result<ProbeBaselines, String> {
        let mut result = ProbeBaselines {
            logs: HashMap::new(),
            returntomoria: None,
        };
        for probe in &self.probes {
            if matches!(probe, Probe::ReturntomoriaStatus { .. }) {
                result.returntomoria = Some(native_status::capture(install_root)?);
            }
            if let Probe::LogMarker { sources, .. } = probe {
                for source in sources {
                    if let Some(path) = log_path(source, instance, install_root)?
                        && let std::collections::hash_map::Entry::Vacant(entry) =
                            result.logs.entry(path)
                    {
                        let baseline = LogBaseline::capture(entry.key())?;
                        entry.insert(baseline);
                    }
                }
            }
        }
        ark_maps::capture_baselines(instance, &mut result)?;
        Ok(result)
    }

    pub fn evidence_log_paths(
        &self,
        instance: &InstanceDetails,
        install_root: &Path,
    ) -> Result<Vec<PathBuf>, String> {
        let mut paths = Vec::new();
        for probe in &self.probes {
            if let Probe::LogMarker { sources, .. } = probe {
                for source in sources {
                    if let Some(path) = log_path(source, instance, install_root)?
                        && !paths.contains(&path)
                    {
                        paths.push(path);
                    }
                }
            }
        }
        Ok(paths)
    }

    pub async fn wait_ready(
        &self,
        descriptor: &ModuleDescriptor,
        instance: &InstanceDetails,
        install_root: &Path,
        fixture_root: &Path,
        baselines: ProbeBaselines,
    ) -> Result<(), String> {
        self.wait_ready_in_scope(
            descriptor,
            instance,
            install_root,
            ProbeScope::Disposable(fixture_root),
            baselines,
        )
        .await
    }

    pub async fn wait_ready_existing(
        &self,
        descriptor: &ModuleDescriptor,
        instance: &InstanceDetails,
        install_root: &Path,
        baselines: ProbeBaselines,
    ) -> Result<(), String> {
        self.wait_ready_in_scope(
            descriptor,
            instance,
            install_root,
            ProbeScope::ExistingInstance,
            baselines,
        )
        .await
    }

    fn humanitz_info_required(
        &self,
        scope: ProbeScope<'_>,
        settings_json: &str,
    ) -> Result<bool, String> {
        let settings: serde_json::Value = serde_json::from_str(settings_json)
            .map_err(|_| "invalid HumanitZ instance settings")?;
        match settings
            .get("rcon_enabled")
            .and_then(serde_json::Value::as_bool)
        {
            Some(true) => Ok(true),
            Some(false) if matches!(scope, ProbeScope::ExistingInstance) => {
                // An existing server may intentionally disable remote control.
                // Its readiness still requires fresh native session output and
                // the managed process's game endpoint for the stability window.
                let owned_game = self.probes.iter().any(|probe| {
                    matches!(probe,
                    Probe::ProcessEndpoint { process_key, port_name, protocol, .. }
                    if process_key == "main" && port_name == "game" && protocol == "udp")
                });
                let fresh_session = self.probes.iter().any(|probe| matches!(probe,
                    Probe::LogMarker { sources, any_of, .. }
                    if matches!(sources.as_slice(), [LogSource::Process { process_key }] if process_key == "main")
                        && any_of.as_slice() == ["LogHZSuccess: Display: Success => Session created!"]));
                if self.module_id != "humanitz"
                    || self.primary_process_key != "main"
                    || self.probe_mode != "all"
                    || self.stability_window_ms < 3000
                    || !owned_game
                    || !fresh_session
                {
                    return Err("HumanitZ without RCON requires fresh session, owned game UDP and a 3-second stability window".into());
                }
                Ok(false)
            }
            Some(false) => Err("HumanitZ fixture info probe requires RCON enabled".into()),
            None => {
                Err("HumanitZ readiness requires an explicit boolean rcon_enabled setting".into())
            }
        }
    }

    async fn wait_ready_in_scope(
        &self,
        descriptor: &ModuleDescriptor,
        instance: &InstanceDetails,
        install_root: &Path,
        scope: ProbeScope<'_>,
        mut baselines: ProbeBaselines,
    ) -> Result<(), String> {
        let targets = crate::commands::commands_runtime_supervision::build_window_inspection_targets_from_instance(instance);
        if !targets
            .iter()
            .any(|target| target.process_key == self.primary_process_key)
        {
            return Err("declared primary process has no stable managed identity".into());
        }
        let deadline = Instant::now() + Duration::from_millis(self.readiness_timeout_ms);
        let use_minecraft_status = minecraft_status::required(
            matches!(scope, ProbeScope::ExistingInstance),
            &descriptor.summary.id,
            &instance.settings_json,
        )?;
        let use_humanitz_info = descriptor.summary.id != "humanitz"
            || self.humanitz_info_required(scope, &instance.settings_json)?;
        let mut stable_since = None;
        loop {
            if matches!(scope, ProbeScope::Disposable(_)) {
                super::diagnostics::report_astroneer_startup(instance, install_root);
            }
            for target in &targets {
                if app_runtime::inspect_process_identity(target.pid)
                    .map_err(|_| "process identity check failed")?
                    .as_ref()
                    != Some(&target.process_identity)
                {
                    return Err("managed process exited before native readiness completed".into());
                }
            }
            let inspection_targets = targets.clone();
            let ports = instance
                .ports
                .iter()
                .map(|port| port.port)
                .collect::<Vec<_>>();
            let endpoints = tokio::task::spawn_blocking(move || {
                WindowsPlatform::inspect_process_network_endpoints(&inspection_targets, &ports)
            })
            .await
            .map_err(|_| "endpoint inspection worker failed")??
            .endpoints;
            let mut pending = Vec::new();
            let mut query_diagnostics = Vec::new();
            for probe in &self.probes {
                let ready = match probe {
                    Probe::WindroseWorld { .. } => {
                        let install = install_root.to_owned();
                        let fixture = match scope {
                            ProbeScope::Disposable(root) => Some(root.to_owned()),
                            ProbeScope::ExistingInstance => None,
                        };
                        tokio::task::spawn_blocking(move || match fixture {
                            Some(root) => windrose_world::ready(&install, &root),
                            None => windrose_world::ready_existing(&install),
                        })
                        .await
                        .map_err(|_| "Windrose world probe worker failed")??
                    }
                    Probe::SquadPlayers { .. } => {
                        let response = match scope {
                            ProbeScope::Disposable(root) => {
                                super::squad_probe::query(descriptor, instance, install_root, root)
                                    .await
                            }
                            ProbeScope::ExistingInstance => {
                                super::squad_probe::query_existing(
                                    descriptor,
                                    instance,
                                    install_root,
                                )
                                .await
                            }
                        };
                        if let Err(reason) = &response {
                            query_diagnostics.push(format!("{}={reason}", probe.id()));
                        }
                        response.is_ok()
                    }
                    Probe::ReturntomoriaStatus { .. } => native_status::ready(
                        install_root,
                        baselines
                            .returntomoria
                            .as_ref()
                            .ok_or("missing native status baseline")?,
                    )?,
                    Probe::LogMarker {
                        sources, any_of, ..
                    } => {
                        let mut matched = false;
                        for source in sources {
                            if let Some(path) = log_path(source, instance, install_root)? {
                                let baseline = baselines.logs.entry(path.clone()).or_default();
                                matched |= new_log_contains(&path, baseline, any_of)?;
                            }
                        }
                        matched
                    }
                    Probe::ProcessEndpoint {
                        process_key,
                        port_name,
                        protocol,
                        ..
                    } => {
                        let port = port_number(instance, port_name)?;
                        endpoints.iter().any(|entry| {
                            entry.local_port == port
                                && entry.protocol.eq_ignore_ascii_case(protocol)
                                && &entry.process_key == process_key
                        })
                    }
                    Probe::PlayerQuery { .. } if use_minecraft_status => {
                        let port = port_number(instance, "game")?;
                        let source = LogSource::Process {
                            process_key: self.primary_process_key.clone(),
                        };
                        if !minecraft_status::owns_game_endpoint(&endpoints, port) {
                            false
                        } else if let Some(path) = log_path(&source, instance, install_root)? {
                            // Read the newly pinned run's managed log, never a
                            // latest.log left by an earlier Minecraft process.
                            let baseline = baselines.logs.entry(path.clone()).or_default();
                            if native_logs::new_log_line_matches(
                                &path,
                                baseline,
                                minecraft_status::done_line,
                            )? {
                                match minecraft_status::query_status(
                                    &instance.summary.bind_ip,
                                    port,
                                )
                                .await
                                {
                                    Ok(()) => true,
                                    Err(reason) => {
                                        query_diagnostics.push(reason.into());
                                        false
                                    }
                                }
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    }
                    Probe::PlayerQuery { port_name, .. } => {
                        let port = port_number(instance, port_name)?;
                        let owned = endpoints.iter().any(|entry| {
                            entry.local_port == port && entry.protocol.eq_ignore_ascii_case("udp")
                        });
                        let query = descriptor
                            .runtime
                            .player_query
                            .as_ref()
                            .ok_or("module has no player query contract")?;
                        if !query.port_names.contains(port_name)
                            || !app_storage::supports_live_player_query_protocol(&query.protocol)
                        {
                            return Err(
                                "smoke player query is not supported by the module contract".into(),
                            );
                        }
                        let protocol = query.protocol.clone();
                        let response = if owned {
                            tokio::task::spawn_blocking(move || {
                                super::diagnostics::query_player_count(&protocol, port)
                            })
                            .await
                            .map_err(|_| "player query worker failed")?
                        } else {
                            Err("no owned endpoint".into())
                        };
                        let answered = response.is_ok();
                        if let Err(reason) = response {
                            query_diagnostics.push(format!("{}:{port}/udp={reason}", probe.id(),));
                        }
                        answered
                    }
                    Probe::AstroneerWorld { port_name, .. } => {
                        let port = port_number(instance, port_name)?;
                        let owned = endpoints.iter().any(|entry| {
                            entry.local_port == port && entry.protocol.eq_ignore_ascii_case("tcp")
                        });
                        let ready = owned
                            && super::astroneer_readiness::active_world_selected(instance).await;
                        if std::env::var("LANGAME_NATIVE_LOG_SUCCESS").as_deref() == Ok("true") {
                            eprintln!(
                                "NATIVE_ASTRONEER_PROBE command=world_state owned_endpoint={owned} ready={ready}"
                            );
                        }
                        ready
                    }
                    Probe::AstroneerPlayers { port_name, .. } => {
                        let port = port_number(instance, port_name)?;
                        let owned = endpoints.iter().any(|entry| {
                            entry.local_port == port && entry.protocol.eq_ignore_ascii_case("tcp")
                        });
                        let observed_at = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_err(|_| "native probe clock is before Unix epoch")?
                            .as_millis();
                        let ready = owned
                            && crate::live_players::astroneer::collect_astroneer(
                                instance,
                                &instance.summary.id,
                                "native-readiness",
                                u64::try_from(observed_at)
                                    .map_err(|_| "native probe clock overflow")?,
                            )
                            .await
                            .is_ok();
                        if std::env::var("LANGAME_NATIVE_LOG_SUCCESS").as_deref() == Ok("true") {
                            eprintln!(
                                "NATIVE_ASTRONEER_PROBE command=DSListPlayers owned_endpoint={owned} ready={ready}"
                            );
                        }
                        ready
                    }
                    Probe::HumanitzInfo { .. } if !use_humanitz_info => true,
                    Probe::HumanitzInfo { port_name, .. } => {
                        let settings: serde_json::Value =
                            serde_json::from_str(&instance.settings_json)
                                .map_err(|_| "invalid HumanitZ fixture settings")?;
                        if settings
                            .get("rcon_enabled")
                            .and_then(serde_json::Value::as_bool)
                            != Some(true)
                        {
                            return Err("HumanitZ info probe requires RCON enabled".into());
                        }
                        let password = settings
                            .get("rcon_password")
                            .and_then(serde_json::Value::as_str)
                            .filter(|value| !value.is_empty())
                            .ok_or("HumanitZ info probe requires a generated RCON password")?
                            .to_owned();
                        let port = port_number(instance, port_name)?;
                        let owned = endpoints.iter().any(|entry| {
                            entry.local_port == port && entry.protocol.eq_ignore_ascii_case("tcp")
                        });
                        owned
                            && tokio::task::spawn_blocking(move || {
                                crate::runtime_transport_humanitz::humanitz_rcon_info(
                                    &format!("127.0.0.1:{port}"),
                                    &password,
                                    "info",
                                )
                                .is_ok()
                            })
                            .await
                            .map_err(|_| "HumanitZ info worker failed")?
                    }
                    Probe::TcpConnect { port_name, .. } => {
                        let port = port_number(instance, port_name)?;
                        let owned = endpoints.iter().any(|entry| {
                            entry.local_port == port && entry.protocol.eq_ignore_ascii_case("tcp")
                        });
                        owned
                            && tokio::time::timeout(
                                Duration::from_millis(500),
                                tokio::net::TcpStream::connect(("127.0.0.1", port)),
                            )
                            .await
                            .is_ok_and(|result| result.is_ok())
                    }
                };
                if !ready {
                    pending.push(
                        if use_minecraft_status && matches!(probe, Probe::PlayerQuery { .. }) {
                            "minecraft-status".to_owned()
                        } else {
                            probe.id().to_owned()
                        },
                    );
                }
            }
            pending
                .extend(ark_maps::pending(descriptor, instance, &endpoints, &mut baselines).await?);
            if pending.is_empty() {
                let since = stable_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= Duration::from_millis(self.stability_window_ms) {
                    if matches!(scope, ProbeScope::Disposable(_)) {
                        ark_maps::verify_layout(instance, install_root)?;
                    }
                    return Ok(());
                }
            } else {
                stable_since = None;
            }
            if Instant::now() >= deadline {
                if !self
                    .probes
                    .iter()
                    .any(|probe| matches!(probe, Probe::PlayerQuery { .. }))
                    && let Some(query) = descriptor.runtime.player_query.as_ref()
                    && app_storage::supports_live_player_query_protocol(&query.protocol)
                {
                    let owned_query_port = query.port_names.iter().find_map(|name| {
                        let port = port_number(instance, name).ok()?;
                        endpoints
                            .iter()
                            .any(|entry| {
                                entry.local_port == port
                                    && entry.protocol.eq_ignore_ascii_case("udp")
                            })
                            .then_some(port)
                    });
                    if let Some(port) = owned_query_port {
                        let protocol = query.protocol.clone();
                        let response = tokio::task::spawn_blocking(move || {
                            super::diagnostics::query_player_count(&protocol, port)
                        })
                        .await
                        .map_err(|_| "diagnostic player query worker failed")?;
                        query_diagnostics.push(format!(
                            "timeout-only:{port}/udp={}",
                            response.map_or_else(|reason| reason, |()| "accepted_reply".into())
                        ));
                    } else {
                        query_diagnostics.push("timeout-only:no_owned_endpoint".into());
                    }
                }
                let observed = endpoints
                    .iter()
                    .map(|entry| {
                        format!(
                            "{}:{}/{}",
                            entry.process_key, entry.local_port, entry.protocol
                        )
                    })
                    .collect::<Vec<_>>();
                return Err(format!(
                    "native readiness timed out; pending probes: {}; player query: [{}]; owned declared endpoints: [{}]",
                    pending.join(", "),
                    query_diagnostics.join(", "),
                    observed.join(", ")
                ));
            }
            tokio::time::sleep(Duration::from_millis(self.poll_interval_ms)).await;
        }
    }
}

fn port_number(instance: &InstanceDetails, name: &str) -> Result<u16, String> {
    instance
        .ports
        .iter()
        .find(|port| port.name == name)
        .map(|port| port.port)
        .ok_or_else(|| "missing actual probe port".into())
}

fn log_path(
    source: &LogSource,
    instance: &InstanceDetails,
    install_root: &Path,
) -> Result<Option<PathBuf>, String> {
    let instance_root = Path::new(&instance.config_file_path)
        .parent()
        .and_then(Path::parent)
        .ok_or("missing instance root")?;
    Ok(match source {
        LogSource::Install { path } => Some(install_root.join(path)),
        LogSource::Instance { path } => Some(instance_root.join(path)),
        LogSource::Process { process_key } => instance
            .active_run
            .as_ref()
            .and_then(|run| {
                run.processes
                    .iter()
                    .find(|process| &process.process_key == process_key)
            })
            .and_then(|process| process.log_path.as_ref())
            .map(PathBuf::from),
    })
}

#[path = "commands_native_readiness_tests.rs"]
mod tests;
