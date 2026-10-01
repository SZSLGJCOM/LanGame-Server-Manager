use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use app_core::{
    InstallSource, InstallSpec, InstallState, MinecraftDistributionSpec, MinecraftJavaInstallSpec,
    ModuleBindAddressMode, ModuleBindAddressSpec, ModuleJoinProfile, ModulePlayerActionSpec,
    ModulePlayerCountSource, ModulePlayerManagementSpec, ModulePlayerQuerySpec,
    ModulePortGroupSpec, ModulePortRole, ModulePortRoleSpec, ModuleRuntimeSpec,
    ModuleShutdownCommandSpec, ModuleShutdownSpec, ModuleSummary, PortBinding, ProcessHostSurface,
    ProcessSpec, ProcessWindowPolicy, RuntimePerformancePolicy, RuntimePriorityClass, WorkshopSpec,
};
#[cfg(test)]
use app_core::{ModulePlayerListScope, ModulePlayerListSource};
use serde::Deserialize;
use thiserror::Error;

mod player_list;
use player_list::player_list_spec_from_toml;

#[cfg(test)]
mod player_count_tests;

#[cfg(test)]
mod install_tests;

mod storage;
pub use storage::{ModuleProgramSharing, ModuleStorageSpec};
use storage::{ModuleTomlStorage, storage_spec_from_toml, validate_program_sharing};

mod dst_configuration_icons;
pub use dst_configuration_icons::{DstConfigurationIcons, load_dst_configuration_icons};

#[derive(Debug, Clone)]
pub struct ModuleDescriptor {
    pub root: PathBuf,
    pub manifest_toml: String,
    pub schema_json: Option<String>,
    pub default_ports: Vec<PortBinding>,
    pub install: Option<InstallSpec>,
    pub process: Option<ProcessSpec>,
    pub workshop: Option<WorkshopSpec>,
    pub runtime: ModuleRuntimeSpec,
    pub storage: ModuleStorageSpec,
    pub summary: ModuleSummary,
}

#[derive(Debug, Deserialize)]
struct ModuleToml {
    id: String,
    name: String,
    version: String,
    description: Option<String>,
    steam_app_id: Option<u32>,
    supported_platforms: Option<Vec<String>>,
    default_ports: Option<Vec<ModuleTomlPort>>,
    install: Option<ModuleTomlInstall>,
    process: Option<ModuleTomlProcess>,
    workshop: Option<ModuleTomlWorkshop>,
    runtime: Option<ModuleTomlRuntime>,
    player_management: Option<ModuleTomlPlayerManagement>,
    storage: Option<ModuleTomlStorage>,
    mods: Option<toml::Value>,
}

#[derive(Debug, Deserialize)]
struct ModuleTomlInstall {
    shared_game_dir: Option<String>,
    download_url_windows: Option<String>,
    download_integrity_windows: Option<app_core::DownloadIntegritySpec>,
    source: Option<InstallSource>,
    verification_path: Option<String>,
    minecraft: Option<ModuleTomlMinecraftJavaInstall>,
}

#[derive(Debug, Deserialize)]
struct ModuleTomlMinecraftJavaInstall {
    version: Option<String>,
    manifest_url: Option<String>,
    server_jar: Option<String>,
    java_policy: Option<String>,
    default_distribution: Option<String>,
    #[serde(default)]
    distributions: Vec<ModuleTomlMinecraftDistribution>,
}

#[derive(Debug, Deserialize)]
struct ModuleTomlMinecraftDistribution {
    id: String,
    label: String,
    server_jar: Option<String>,
    source: String,
    entrypoint_kind: Option<String>,
    #[serde(default)]
    supports_mods: bool,
    #[serde(default)]
    supports_plugins: bool,
    #[serde(default)]
    supports_datapacks: bool,
    #[serde(default)]
    supports_resource_packs: bool,
}

#[derive(Debug, Deserialize)]
struct ModuleTomlProcess {
    executable: String,
    args_template: Option<Vec<String>>,
    #[serde(default)]
    environment_template: BTreeMap<String, String>,
    working_directory_template: Option<String>,
    window_policy: Option<ProcessWindowPolicy>,
    host_surface: Option<ProcessHostSurface>,
    host_notes: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModuleTomlWorkshop {
    provider: String,
    consumer_app_id: Option<u32>,
    supports_collections: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ModuleTomlRuntime {
    player_count_source: Option<ModulePlayerCountSource>,
    bind_address: Option<ModuleBindAddressSpec>,
    port_roles: Option<Vec<ModulePortRoleSpec>>,
    port_groups: Option<Vec<ModuleTomlPortGroup>>,
    player_query: Option<ModuleTomlPlayerQuery>,
    join: Option<ModuleTomlJoinProfile>,
    player_actions: Option<Vec<ModuleTomlPlayerAction>>,
    player_list: Option<ModuleTomlPlayerList>,
    shutdown: Option<ModuleTomlRuntimeShutdown>,
    performance: Option<ModuleTomlRuntimePerformance>,
    requires_admin: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModuleTomlJoinProfile {
    kind: ModuleTomlJoinKind,
    client_app_id: u32,
    join_port_name: String,
    query_port_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ModuleTomlJoinKind {
    SteamConnect,
}

#[derive(Debug, Deserialize)]
struct ModuleTomlPlayerList {
    scope: Option<String>,
    source: Option<String>,
    action_id: Option<String>,
    player_action_ids: Option<Vec<String>>,
    response_codec: Option<String>,
    identity_kind: Option<String>,
    refresh_interval_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModuleTomlPortGroup {
    id: String,
    members: Vec<String>,
    member_offsets: Option<BTreeMap<String, u16>>,
}

#[derive(Debug, Deserialize)]
struct ModuleTomlPlayerQuery {
    protocol: String,
    port_names: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct ModuleTomlPlayerAction {
    id: String,
    kind: Option<String>,
    label: String,
    label_zh_cn: Option<String>,
    transport: Option<String>,
    command_template: String,
    target_label: Option<String>,
    target_label_zh_cn: Option<String>,
    target_placeholder: Option<String>,
    target_placeholder_zh_cn: Option<String>,
    target_required: Option<bool>,
    target_encoding: Option<String>,
    role_values: Option<Vec<String>>,
    process_key: Option<String>,
    port_name: Option<String>,
    password_setting_key: Option<String>,
    enabled_setting_key: Option<String>,
    destructive: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ModuleTomlPlayerManagement {
    status: String,
    planned_surface: String,
    reason: String,
    verification: String,
}

#[derive(Debug, Deserialize)]
struct ModuleTomlRuntimeShutdown {
    commands: Option<Vec<ModuleTomlShutdownCommand>>,
    grace_period_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ModuleTomlShutdownCommand {
    transport: Option<String>,
    fallback_transport: Option<String>,
    command: String,
    process_key: Option<String>,
    port_name: Option<String>,
    password_setting_key: Option<String>,
    enabled_setting_key: Option<String>,
    wait_after_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ModuleTomlRuntimePerformance {
    priority_class: Option<RuntimePriorityClass>,
    cpu_affinity_mask: Option<u64>,
    apply_to_child_processes: Option<bool>,
    startup_stagger_ms: Option<u64>,
    child_process_stagger_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ModuleTomlPort {
    name: Option<String>,
    protocol: String,
    port: u16,
}

#[derive(Debug, Error)]
pub enum ModuleDiscoveryError {
    #[error("module catalog directory does not exist: {path}")]
    MissingDirectory { path: PathBuf },
    #[error("module catalog directory contains no module manifests: {path}")]
    EmptyCatalog { path: PathBuf },
    #[error("failed to read module directory {path}: {source}")]
    ReadDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read module manifest {path}: {source}")]
    ReadManifest {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse module manifest {path}: {source}")]
    ParseManifest {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },
    #[error("failed to read module schema {path}: {source}")]
    ReadSchema {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("module manifest {path} has an invalid runtime port group: {message}")]
    InvalidPortGroup { path: PathBuf, message: String },
    #[error("module manifest {path} has invalid runtime port roles: {message}")]
    InvalidPortRole { path: PathBuf, message: String },
    #[error("module manifest {path} has an invalid runtime join profile: {message}")]
    InvalidJoinProfile { path: PathBuf, message: String },
    #[error("module manifest {path} has an invalid runtime bind address policy: {message}")]
    InvalidBindAddress { path: PathBuf, message: String },
    #[error("module manifest {path} has an invalid runtime player list contract: {message}")]
    InvalidPlayerList { path: PathBuf, message: String },
    #[error("module manifest {path} has an invalid storage contract: {message}")]
    InvalidStorage { path: PathBuf, message: String },
}

pub fn discover_modules(
    root: impl AsRef<Path>,
) -> Result<Vec<ModuleDescriptor>, ModuleDiscoveryError> {
    let root = root.as_ref();
    let mut descriptors = Vec::new();

    if !root.exists() {
        return Err(ModuleDiscoveryError::MissingDirectory {
            path: root.to_path_buf(),
        });
    }

    for entry in fs::read_dir(root).map_err(|source| ModuleDiscoveryError::ReadDirectory {
        path: root.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| ModuleDiscoveryError::ReadDirectory {
            path: root.to_path_buf(),
            source,
        })?;
        let module_root = entry.path();

        if !module_root.is_dir() {
            continue;
        }

        let manifest_path = module_root.join("module.toml");
        if !manifest_path.exists() {
            continue;
        }

        let manifest_text =
            strip_utf8_bom(fs::read_to_string(&manifest_path).map_err(|source| {
                ModuleDiscoveryError::ReadManifest {
                    path: manifest_path.clone(),
                    source,
                }
            })?);
        let manifest: ModuleToml = toml::from_str(&manifest_text).map_err(|source| {
            ModuleDiscoveryError::ParseManifest {
                path: manifest_path.clone(),
                source: Box::new(source),
            }
        })?;

        let schema_path = module_root.join("schema.json");
        let schema_json = if schema_path.exists() {
            Some(strip_utf8_bom(fs::read_to_string(&schema_path).map_err(
                |source| ModuleDiscoveryError::ReadSchema {
                    path: schema_path.clone(),
                    source,
                },
            )?))
        } else {
            None
        };

        let ModuleToml {
            id,
            name,
            version,
            description,
            steam_app_id,
            supported_platforms,
            default_ports,
            install,
            process,
            workshop,
            runtime,
            player_management,
            storage,
            mods,
        } = manifest;

        let default_ports = default_ports
            .unwrap_or_default()
            .into_iter()
            .map(|port| PortBinding {
                name: port
                    .name
                    .unwrap_or_else(|| format!("port-{}", port.port))
                    .trim()
                    .to_string(),
                protocol: port.protocol.trim().to_ascii_lowercase(),
                port: port.port,
            })
            .collect::<Vec<_>>();

        let install = install.and_then(|install| {
            let minecraft = install.minecraft.map(|minecraft| MinecraftJavaInstallSpec {
                version: minecraft
                    .version
                    .unwrap_or_else(|| String::from("latest_release")),
                manifest_url: minecraft.manifest_url,
                server_jar: minecraft
                    .server_jar
                    .unwrap_or_else(|| String::from("server.jar")),
                java_policy: minecraft
                    .java_policy
                    .unwrap_or_else(|| String::from("mojang_version_metadata")),
                default_distribution: minecraft
                    .default_distribution
                    .unwrap_or_else(|| String::from("vanilla")),
                distributions: minecraft
                    .distributions
                    .into_iter()
                    .map(|distribution| MinecraftDistributionSpec {
                        id: distribution.id,
                        label: distribution.label,
                        server_jar: distribution.server_jar.unwrap_or_default(),
                        source: distribution.source,
                        entrypoint_kind: distribution
                            .entrypoint_kind
                            .unwrap_or_else(|| String::from("jar")),
                        supports_mods: distribution.supports_mods,
                        supports_plugins: distribution.supports_plugins,
                        supports_datapacks: distribution.supports_datapacks,
                        supports_resource_packs: distribution.supports_resource_packs,
                    })
                    .collect(),
            });
            let source = install
                .source
                .or_else(|| minecraft.as_ref().map(|_| InstallSource::MinecraftJava));
            let verification_path = install
                .verification_path
                .or_else(|| minecraft.as_ref().map(|spec| spec.server_jar.clone()));

            install.shared_game_dir.map(|shared_game_dir| InstallSpec {
                shared_game_dir,
                download_url_windows: install.download_url_windows,
                download_integrity_windows: install.download_integrity_windows,
                source,
                verification_path,
                minecraft,
            })
        });

        let process = process.map(|process| {
            let window_policy = process.window_policy.unwrap_or_default();

            ProcessSpec {
                executable: process.executable,
                args_template: process.args_template.unwrap_or_default(),
                environment_template: process.environment_template,
                working_directory_template: process.working_directory_template,
                window_policy: window_policy.clone(),
                host_surface: process
                    .host_surface
                    .unwrap_or_else(|| default_host_surface_for_window_policy(&window_policy)),
                host_notes: process.host_notes,
            }
        });

        let workshop = workshop.map(|workshop| WorkshopSpec {
            provider: workshop.provider,
            consumer_app_id: workshop.consumer_app_id,
            supports_collections: workshop.supports_collections.unwrap_or(false),
        });

        let mut runtime =
            runtime_spec_from_toml(runtime, player_management).map_err(|message| {
                ModuleDiscoveryError::InvalidPlayerList {
                    path: manifest_path.clone(),
                    message,
                }
            })?;
        validate_port_roles(&manifest_path, &default_ports, &runtime.port_roles)?;
        validate_port_groups(&manifest_path, &default_ports, &runtime.port_groups)?;
        validate_bind_address_policy(&manifest_path, &default_ports, &runtime.bind_address)?;
        validate_join_profile(&manifest_path, &default_ports, &runtime)?;

        let storage = storage_spec_from_toml(storage).map_err(|message| {
            ModuleDiscoveryError::InvalidStorage {
                path: manifest_path.clone(),
                message,
            }
        })?;
        validate_program_sharing(&storage, install.as_ref(), process.as_ref(), mods.as_ref())
            .map_err(|message| ModuleDiscoveryError::InvalidStorage {
                path: manifest_path.clone(),
                message,
            })?;
        runtime.program_sharing = match storage.program_sharing {
            ModuleProgramSharing::Shared => app_core::InstanceProgramMode::Shared,
            ModuleProgramSharing::Independent => app_core::InstanceProgramMode::Independent,
        };

        descriptors.push(ModuleDescriptor {
            root: module_root,
            manifest_toml: manifest_text,
            schema_json,
            default_ports,
            install,
            process,
            workshop,
            runtime,
            storage,
            summary: ModuleSummary {
                id,
                name,
                version,
                description,
                steam_app_id,
                install_state: InstallState::NotInstalled,
                instance_program_count: 0,
                archived_program_count: 0,
                supported_platforms: supported_platforms
                    .unwrap_or_else(|| vec![String::from("windows")]),
            },
        });
    }

    if descriptors.is_empty() {
        return Err(ModuleDiscoveryError::EmptyCatalog {
            path: root.to_path_buf(),
        });
    }

    descriptors.sort_by(|left, right| left.summary.name.cmp(&right.summary.name));
    Ok(descriptors)
}

fn strip_utf8_bom(content: String) -> String {
    content
        .strip_prefix('\u{feff}')
        .map(str::to_owned)
        .unwrap_or(content)
}

fn default_host_surface_for_window_policy(
    window_policy: &ProcessWindowPolicy,
) -> ProcessHostSurface {
    match window_policy {
        ProcessWindowPolicy::Background => ProcessHostSurface::ManagedTerminal,
        ProcessWindowPolicy::External => ProcessHostSurface::ExternalWindow,
    }
}

fn runtime_spec_from_toml(
    runtime: Option<ModuleTomlRuntime>,
    player_management: Option<ModuleTomlPlayerManagement>,
) -> Result<ModuleRuntimeSpec, String> {
    let runtime = runtime.unwrap_or(ModuleTomlRuntime {
        player_count_source: None,
        bind_address: None,
        port_roles: None,
        port_groups: None,
        player_query: None,
        join: None,
        player_actions: None,
        player_list: None,
        shutdown: None,
        performance: None,
        requires_admin: None,
    });
    let player_actions = runtime
        .player_actions
        .unwrap_or_default()
        .into_iter()
        .map(|action| ModulePlayerActionSpec {
            id: action.id,
            kind: action.kind,
            label: action.label,
            label_zh_cn: action.label_zh_cn,
            transport: action.transport.unwrap_or_else(|| String::from("stdin")),
            command_template: action.command_template,
            target_label: action.target_label,
            target_label_zh_cn: action.target_label_zh_cn,
            target_placeholder: action.target_placeholder,
            target_placeholder_zh_cn: action.target_placeholder_zh_cn,
            target_required: action.target_required.unwrap_or(false),
            target_encoding: action.target_encoding,
            role_values: action.role_values.unwrap_or_default(),
            process_key: action.process_key,
            port_name: action.port_name,
            password_setting_key: action.password_setting_key,
            enabled_setting_key: action.enabled_setting_key,
            destructive: action.destructive.unwrap_or(false),
        })
        .collect::<Vec<_>>();
    let player_list = runtime
        .player_list
        .map(|player_list| player_list_spec_from_toml(player_list, &player_actions))
        .transpose()?;
    let player_count_source = runtime.player_count_source.unwrap_or_default();
    if player_count_source == ModulePlayerCountSource::PlayerList && player_list.is_none() {
        return Err(String::from(
            "player_count_source 'player_list' requires an online player_list contract",
        ));
    }

    Ok(ModuleRuntimeSpec {
        program_sharing: app_core::InstanceProgramMode::Independent,
        player_count_source,
        bind_address: runtime.bind_address.unwrap_or_default(),
        port_roles: runtime
            .port_roles
            .unwrap_or_default()
            .into_iter()
            .map(|role| ModulePortRoleSpec {
                role: role.role,
                port_names: role
                    .port_names
                    .into_iter()
                    .map(|port_name| port_name.trim().to_string())
                    .collect(),
            })
            .collect(),
        port_groups: runtime
            .port_groups
            .unwrap_or_default()
            .into_iter()
            .map(|group| ModulePortGroupSpec {
                id: group.id,
                members: group.members,
                member_offsets: group.member_offsets,
            })
            .collect(),
        player_query: runtime
            .player_query
            .map(|player_query| ModulePlayerQuerySpec {
                protocol: player_query.protocol,
                port_names: player_query.port_names.unwrap_or_default(),
            }),
        join: runtime.join.map(|profile| match profile.kind {
            ModuleTomlJoinKind::SteamConnect => ModuleJoinProfile::SteamConnect {
                client_app_id: profile.client_app_id,
                join_port_name: profile.join_port_name,
                query_port_name: profile.query_port_name,
            },
        }),
        player_actions,
        player_list,
        player_management: player_management.map(|spec| ModulePlayerManagementSpec {
            status: spec.status,
            planned_surface: spec.planned_surface,
            reason: spec.reason,
            verification: spec.verification,
        }),
        shutdown: runtime.shutdown.map(|shutdown| ModuleShutdownSpec {
            commands: shutdown
                .commands
                .unwrap_or_default()
                .into_iter()
                .map(|command| ModuleShutdownCommandSpec {
                    transport: command.transport.unwrap_or_else(|| String::from("stdin")),
                    fallback_transport: command.fallback_transport,
                    command: command.command,
                    process_key: command.process_key,
                    port_name: command.port_name,
                    password_setting_key: command.password_setting_key,
                    enabled_setting_key: command.enabled_setting_key,
                    wait_after_ms: command.wait_after_ms.unwrap_or(0),
                })
                .collect(),
            grace_period_ms: shutdown.grace_period_ms.unwrap_or(10_000),
        }),
        performance: runtime
            .performance
            .map(performance_policy_from_toml)
            .unwrap_or_default(),
        requires_admin: runtime.requires_admin.unwrap_or(false),
    })
}

fn validate_bind_address_policy(
    manifest_path: &Path,
    default_ports: &[PortBinding],
    policy: &ModuleBindAddressSpec,
) -> Result<(), ModuleDiscoveryError> {
    match policy.mode {
        ModuleBindAddressMode::Unsupported => {
            if policy.port_names.is_empty() && policy.required_setting_key.is_none() {
                return Ok(());
            }
            return Err(ModuleDiscoveryError::InvalidBindAddress {
                path: manifest_path.to_path_buf(),
                message: String::from(
                    "unsupported mode must not declare verification ports or a required setting",
                ),
            });
        }
        ModuleBindAddressMode::Strict => {}
    }

    if policy.port_names.is_empty() {
        return Err(ModuleDiscoveryError::InvalidBindAddress {
            path: manifest_path.to_path_buf(),
            message: String::from("strict mode requires at least one verification port name"),
        });
    }
    if !(1_000..=300_000).contains(&policy.startup_timeout_ms) {
        return Err(ModuleDiscoveryError::InvalidBindAddress {
            path: manifest_path.to_path_buf(),
            message: format!(
                "startup_timeout_ms must be between 1000 and 300000, got {}",
                policy.startup_timeout_ms
            ),
        });
    }

    let declared_ports = default_ports
        .iter()
        .map(|port| port.name.as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut verified_names = std::collections::HashSet::new();
    for port_name in &policy.port_names {
        let normalized = port_name.trim();
        if normalized.is_empty() || normalized != port_name || !verified_names.insert(normalized) {
            return Err(ModuleDiscoveryError::InvalidBindAddress {
                path: manifest_path.to_path_buf(),
                message: format!(
                    "verification port name `{port_name}` must be non-empty, unique, and free of surrounding whitespace"
                ),
            });
        }
        if !declared_ports.contains(normalized) {
            return Err(ModuleDiscoveryError::InvalidBindAddress {
                path: manifest_path.to_path_buf(),
                message: format!(
                    "verification port `{normalized}` is not declared in default_ports"
                ),
            });
        }
    }
    if policy
        .required_setting_key
        .as_deref()
        .is_some_and(|key| key.trim().is_empty() || key.trim() != key)
    {
        return Err(ModuleDiscoveryError::InvalidBindAddress {
            path: manifest_path.to_path_buf(),
            message: String::from(
                "required_setting_key must be non-empty and free of surrounding whitespace",
            ),
        });
    }

    Ok(())
}

fn validate_port_groups(
    manifest_path: &Path,
    default_ports: &[PortBinding],
    port_groups: &[ModulePortGroupSpec],
) -> Result<(), ModuleDiscoveryError> {
    let mut port_names = std::collections::HashMap::new();
    for port in default_ports {
        if port_names.insert(port.name.as_str(), port).is_some() {
            return Err(ModuleDiscoveryError::InvalidPortGroup {
                path: manifest_path.to_path_buf(),
                message: format!(
                    "default port name `{}` is declared more than once",
                    port.name
                ),
            });
        }
    }

    let mut group_ids = std::collections::HashSet::new();
    let mut grouped_members = std::collections::HashSet::new();
    for group in port_groups {
        if group.id.is_empty()
            || group.id.trim() != group.id
            || !group_ids.insert(group.id.as_str())
        {
            return Err(ModuleDiscoveryError::InvalidPortGroup {
                path: manifest_path.to_path_buf(),
                message: format!(
                    "group id `{}` must be non-empty, unique, and free of surrounding whitespace",
                    group.id
                ),
            });
        }
        if group.members.len() < 2 {
            return Err(ModuleDiscoveryError::InvalidPortGroup {
                path: manifest_path.to_path_buf(),
                message: format!("group `{}` must contain at least two port names", group.id),
            });
        }

        let mut member_names = std::collections::HashSet::new();
        let offsets = group.member_offsets.as_ref();
        if let Some(offsets) = offsets {
            for offset_member in offsets.keys() {
                if offset_member.is_empty() || offset_member.trim() != offset_member {
                    return Err(ModuleDiscoveryError::InvalidPortGroup {
                        path: manifest_path.to_path_buf(),
                        message: format!(
                            "group `{}` offset key `{offset_member}` must be non-empty and free of surrounding whitespace",
                            group.id
                        ),
                    });
                }
                if !group.members.iter().any(|member| member == offset_member) {
                    return Err(ModuleDiscoveryError::InvalidPortGroup {
                        path: manifest_path.to_path_buf(),
                        message: format!(
                            "group `{}` offset key `{offset_member}` is not one of its members",
                            group.id
                        ),
                    });
                }
            }
            if let Some(member) = group
                .members
                .iter()
                .find(|member| !offsets.contains_key(member.as_str()))
            {
                return Err(ModuleDiscoveryError::InvalidPortGroup {
                    path: manifest_path.to_path_buf(),
                    message: format!(
                        "group `{}` member `{member}` is missing from member_offsets",
                        group.id
                    ),
                });
            }
        }

        let mut protocol_offsets = std::collections::HashSet::new();
        let mut shared_base_port = None;
        let mut maximum_offset = 0_u16;
        for member in &group.members {
            if member.is_empty() || member.trim() != member || !member_names.insert(member.as_str())
            {
                return Err(ModuleDiscoveryError::InvalidPortGroup {
                    path: manifest_path.to_path_buf(),
                    message: format!(
                        "group `{}` members must be non-empty, unique, and free of surrounding whitespace",
                        group.id
                    ),
                });
            }
            if !grouped_members.insert(member.as_str()) {
                return Err(ModuleDiscoveryError::InvalidPortGroup {
                    path: manifest_path.to_path_buf(),
                    message: format!("port `{member}` belongs to more than one group"),
                });
            }
            let Some(port) = port_names.get(member.as_str()) else {
                return Err(ModuleDiscoveryError::InvalidPortGroup {
                    path: manifest_path.to_path_buf(),
                    message: format!("group `{}` references undeclared port `{member}`", group.id),
                });
            };
            let offset = offsets
                .and_then(|offsets| offsets.get(member))
                .copied()
                .unwrap_or(0);
            maximum_offset = maximum_offset.max(offset);
            if !protocol_offsets.insert((port.protocol.to_ascii_lowercase(), offset)) {
                return Err(ModuleDiscoveryError::InvalidPortGroup {
                    path: manifest_path.to_path_buf(),
                    message: format!(
                        "group `{}` cannot bind protocol `{}` more than once at offset {offset}",
                        group.id, port.protocol,
                    ),
                });
            }
            let Some(base_port) = port.port.checked_sub(offset) else {
                return Err(ModuleDiscoveryError::InvalidPortGroup {
                    path: manifest_path.to_path_buf(),
                    message: format!(
                        "group `{}` default port `{member}` ({}) is below its offset {offset}",
                        group.id, port.port
                    ),
                });
            };
            if shared_base_port
                .replace(base_port)
                .is_some_and(|value| value != base_port)
            {
                return Err(ModuleDiscoveryError::InvalidPortGroup {
                    path: manifest_path.to_path_buf(),
                    message: format!(
                        "group `{}` default ports and offsets must resolve to the same base port",
                        group.id
                    ),
                });
            }
        }

        if shared_base_port
            .and_then(|base_port| base_port.checked_add(maximum_offset))
            .is_none()
        {
            return Err(ModuleDiscoveryError::InvalidPortGroup {
                path: manifest_path.to_path_buf(),
                message: format!(
                    "group `{}` base port plus maximum member offset exceeds 65535",
                    group.id
                ),
            });
        }
    }

    Ok(())
}

fn validate_port_roles(
    manifest_path: &Path,
    default_ports: &[PortBinding],
    port_roles: &[ModulePortRoleSpec],
) -> Result<(), ModuleDiscoveryError> {
    if port_roles.is_empty() {
        return Ok(());
    }

    let declared_ports = default_ports
        .iter()
        .map(|port| port.name.as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut declared_roles = std::collections::HashSet::new();
    let mut classified_ports = std::collections::HashMap::new();

    for role_spec in port_roles {
        if !declared_roles.insert(role_spec.role) {
            return Err(ModuleDiscoveryError::InvalidPortRole {
                path: manifest_path.to_path_buf(),
                message: format!("role `{:?}` is declared more than once", role_spec.role)
                    .to_ascii_lowercase(),
            });
        }
        if role_spec.port_names.is_empty() {
            return Err(ModuleDiscoveryError::InvalidPortRole {
                path: manifest_path.to_path_buf(),
                message: format!(
                    "role `{:?}` must contain at least one port name",
                    role_spec.role
                )
                .to_ascii_lowercase(),
            });
        }

        for port_name in &role_spec.port_names {
            if port_name.is_empty() {
                return Err(ModuleDiscoveryError::InvalidPortRole {
                    path: manifest_path.to_path_buf(),
                    message: String::from("role port names must be non-empty"),
                });
            }
            if !declared_ports.contains(port_name.as_str()) {
                return Err(ModuleDiscoveryError::InvalidPortRole {
                    path: manifest_path.to_path_buf(),
                    message: format!("port `{port_name}` is not declared in default_ports"),
                });
            }
            if let Some(previous_role) = classified_ports.insert(port_name.as_str(), role_spec.role)
            {
                let message = if previous_role == role_spec.role {
                    format!("port `{port_name}` is classified more than once")
                } else {
                    format!("port `{port_name}` belongs to more than one role")
                };
                return Err(ModuleDiscoveryError::InvalidPortRole {
                    path: manifest_path.to_path_buf(),
                    message,
                });
            }
        }
    }

    let mut missing_ports = declared_ports
        .difference(&classified_ports.keys().copied().collect())
        .copied()
        .collect::<Vec<_>>();
    missing_ports.sort_unstable();
    if !missing_ports.is_empty() {
        return Err(ModuleDiscoveryError::InvalidPortRole {
            path: manifest_path.to_path_buf(),
            message: format!(
                "default ports {} must each belong to exactly one role",
                missing_ports.join(", ")
            ),
        });
    }

    Ok(())
}

fn validate_join_profile(
    manifest_path: &Path,
    default_ports: &[PortBinding],
    runtime: &ModuleRuntimeSpec,
) -> Result<(), ModuleDiscoveryError> {
    let Some(ModuleJoinProfile::SteamConnect {
        client_app_id,
        join_port_name,
        query_port_name,
    }) = runtime.join.as_ref()
    else {
        return Ok(());
    };

    let invalid = |message: String| ModuleDiscoveryError::InvalidJoinProfile {
        path: manifest_path.to_path_buf(),
        message,
    };
    if *client_app_id == 0 {
        return Err(invalid(String::from(
            "client_app_id must be greater than zero",
        )));
    }
    for (field, port_name) in [
        ("join_port_name", join_port_name),
        ("query_port_name", query_port_name),
    ] {
        if port_name.is_empty() || port_name.trim() != port_name {
            return Err(invalid(format!(
                "{field} must be non-empty and free of surrounding whitespace"
            )));
        }
        if !default_ports.iter().any(|port| port.name == *port_name) {
            return Err(invalid(format!(
                "{field} `{port_name}` is not declared in default_ports"
            )));
        }
    }

    let join_is_player_port = runtime.port_roles.iter().any(|role| {
        role.role == ModulePortRole::Player
            && role.port_names.iter().any(|name| name == join_port_name)
    });
    if !join_is_player_port {
        return Err(invalid(format!(
            "join_port_name `{join_port_name}` must belong to the runtime player port role"
        )));
    }

    let player_query = runtime
        .player_query
        .as_ref()
        .filter(|query| query.port_names.iter().any(|name| name == query_port_name));
    let Some(player_query) = player_query else {
        return Err(invalid(format!(
            "query_port_name `{query_port_name}` must be declared as a runtime player_query candidate"
        )));
    };
    if player_query.protocol != "a2s_info" {
        return Err(invalid(format!(
            "steam_connect requires runtime player_query protocol `a2s_info`, found `{}`",
            player_query.protocol
        )));
    }
    let query_port_is_udp = default_ports
        .iter()
        .any(|port| port.name == *query_port_name && port.protocol.eq_ignore_ascii_case("udp"));
    if !query_port_is_udp {
        return Err(invalid(format!(
            "query_port_name `{query_port_name}` must reference a UDP default port"
        )));
    }

    Ok(())
}

fn performance_policy_from_toml(
    performance: ModuleTomlRuntimePerformance,
) -> RuntimePerformancePolicy {
    let defaults = RuntimePerformancePolicy::default();
    RuntimePerformancePolicy {
        resource_limits: defaults.resource_limits,
        priority_class: performance
            .priority_class
            .unwrap_or(defaults.priority_class),
        cpu_affinity_mask: performance
            .cpu_affinity_mask
            .filter(|mask| *mask > 0)
            .or(defaults.cpu_affinity_mask),
        apply_to_child_processes: performance
            .apply_to_child_processes
            .unwrap_or(defaults.apply_to_child_processes),
        startup_stagger_ms: performance
            .startup_stagger_ms
            .unwrap_or(defaults.startup_stagger_ms),
        child_process_stagger_ms: performance
            .child_process_stagger_ms
            .unwrap_or(defaults.child_process_stagger_ms),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_core::{ModuleDetails, ModulePortRole, ProcessHostSurface, ProcessWindowPolicy};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_ROOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn discover_modules_reports_a_missing_catalog_directory() {
        let sequence = TEST_ROOT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let missing_root = std::env::temp_dir().join(format!(
            "langame-missing-modules-{}-{}-{sequence}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));

        let error = discover_modules(&missing_root).expect_err("missing catalog must fail");

        assert!(matches!(
            error,
            ModuleDiscoveryError::MissingDirectory { path } if path == missing_root
        ));
    }

    #[test]
    fn discover_modules_reports_an_empty_catalog_directory() {
        let empty_root = create_temp_dir();

        let error = discover_modules(&empty_root).expect_err("empty catalog must fail");

        assert!(matches!(
            error,
            ModuleDiscoveryError::EmptyCatalog { path } if path == empty_root
        ));
        let _ = fs::remove_dir_all(empty_root);
    }

    #[test]
    fn discover_modules_strips_utf8_bom_from_schema_json() {
        let temp_root = create_temp_dir();
        let module_root = temp_root.join("bom-module");
        fs::create_dir_all(&module_root).expect("create module root");
        fs::write(
            module_root.join("module.toml"),
            "id = \"bom\"\nname = \"BOM Module\"\nversion = \"1.0.0\"\n",
        )
        .expect("write manifest");
        fs::write(
            module_root.join("schema.json"),
            [
                0xEF, 0xBB, 0xBF, b'{', b'\"', b't', b'y', b'p', b'e', b'\"', b':', b'\"', b'o',
                b'b', b'j', b'e', b'c', b't', b'\"', b'}',
            ],
        )
        .expect("write schema");

        let modules = discover_modules(&temp_root).expect("discover modules");
        let schema_json = modules
            .first()
            .and_then(|module| module.schema_json.as_deref())
            .expect("schema text");

        assert!(schema_json.starts_with('{'));
        assert!(!schema_json.starts_with('\u{feff}'));

        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn discover_modules_strips_utf8_bom_from_manifest_toml() {
        let temp_root = create_temp_dir();
        let module_root = temp_root.join("bom-manifest-module");
        fs::create_dir_all(&module_root).expect("create module root");
        let mut manifest = vec![0xEF, 0xBB, 0xBF];
        manifest.extend_from_slice(
            b"id = \"bom-manifest\"\nname = \"BOM Manifest Module\"\nversion = \"1.0.0\"\n",
        );
        fs::write(module_root.join("module.toml"), manifest).expect("write manifest");

        let modules = discover_modules(&temp_root).expect("discover modules");
        let descriptor = modules.first().expect("descriptor");

        assert_eq!(descriptor.summary.id, "bom-manifest");
        assert_eq!(descriptor.summary.name, "BOM Manifest Module");

        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn discover_modules_reads_storage_save_path_template() {
        let temp_root = create_temp_dir();
        let module_root = temp_root.join("storage-module");
        fs::create_dir_all(&module_root).expect("create module root");
        fs::write(
            module_root.join("module.toml"),
            "id = \"storage\"\nname = \"Storage Module\"\nversion = \"1.0.0\"\n\n[storage]\nsaves_path_template = \"{{paths.config_dir}}/savegame\"\n",
        )
        .expect("write manifest");

        let modules = discover_modules(&temp_root).expect("discover modules");
        let descriptor = modules.first().expect("descriptor");

        assert_eq!(
            descriptor.storage.saves_path_template.as_deref(),
            Some("{{paths.config_dir}}/savegame")
        );

        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn discover_modules_reads_runtime_player_query_spec() {
        let temp_root = create_temp_dir();
        let module_root = temp_root.join("runtime-module");
        fs::create_dir_all(&module_root).expect("create module root");
        fs::write(
            module_root.join("module.toml"),
            concat!(
                "id = \"runtime\"\n",
                "name = \"Runtime Module\"\n",
                "version = \"1.0.0\"\n\n",
                "[runtime.player_query]\n",
                "protocol = \"a2s_info\"\n",
                "port_names = [\"steam_query\", \"query\"]\n",
            ),
        )
        .expect("write manifest");

        let modules = discover_modules(&temp_root).expect("discover modules");
        let descriptor = modules.first().expect("descriptor");
        let player_query = descriptor
            .runtime
            .player_query
            .as_ref()
            .expect("player query runtime spec");

        assert_eq!(player_query.protocol, "a2s_info");
        assert_eq!(player_query.port_names, vec!["steam_query", "query"]);

        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn discover_modules_reads_explicit_steam_connect_join_profile() {
        let temp_root = write_join_profile_fixture(concat!(
            "kind = \"steam_connect\"\n",
            "client_app_id = 252490\n",
            "join_port_name = \"game\"\n",
            "query_port_name = \"query\"\n",
        ));

        let modules = discover_modules(&temp_root).expect("discover module");

        assert_eq!(
            modules[0].runtime.join,
            Some(ModuleJoinProfile::SteamConnect {
                client_app_id: 252490,
                join_port_name: String::from("game"),
                query_port_name: String::from("query"),
            })
        );
        let _ = fs::remove_dir_all(temp_root);
    }

    #[test]
    fn discover_modules_reads_and_serializes_fixed_port_group_offsets() {
        let temp_root = write_port_group_fixture(
            27_015,
            "udp",
            27_016,
            "udp",
            concat!(
                "id = \"game_query\"\n",
                "members = [\"game\", \"query\"]\n",
                "member_offsets = { game = 0, query = 1 }\n",
            ),
        );

        let modules = discover_modules(&temp_root).expect("discover offset port group");
        let group = &modules[0].runtime.port_groups[0];
        assert_eq!(group.id, "game_query");
        assert_eq!(group.members, ["game", "query"]);
        assert_eq!(
            group.member_offsets,
            Some(BTreeMap::from([
                (String::from("game"), 0),
                (String::from("query"), 1),
            ]))
        );

        let serialized = toml::Value::try_from(modules[0].runtime.clone())
            .expect("serialize runtime port group");
        assert_eq!(
            serialized["port_groups"][0]["member_offsets"]["game"].as_integer(),
            Some(0)
        );
        assert_eq!(
            serialized["port_groups"][0]["member_offsets"]["query"].as_integer(),
            Some(1)
        );

        let _ = fs::remove_dir_all(temp_root);
    }

    #[test]
    fn discover_modules_preserves_shared_port_groups_without_offsets() {
        let temp_root = write_port_group_fixture(
            7_777,
            "udp",
            7_777,
            "tcp",
            concat!(
                "id = \"game_transport\"\n",
                "members = [\"game\", \"query\"]\n",
            ),
        );

        let modules = discover_modules(&temp_root).expect("discover shared port group");
        assert!(modules[0].runtime.port_groups[0].member_offsets.is_none());

        let _ = fs::remove_dir_all(temp_root);
    }

    #[test]
    fn discover_modules_rejects_invalid_fixed_port_group_offsets() {
        let cases = [
            (
                27_015,
                "udp",
                27_016,
                "udp",
                concat!(
                    "id = \"game_query\"\n",
                    "members = [\"game\", \"query\"]\n",
                    "member_offsets = { game = 0, query = 1, typo = 2 }\n",
                ),
                "offset key `typo`",
            ),
            (
                27_015,
                "udp",
                27_016,
                "udp",
                concat!(
                    "id = \"game_query\"\n",
                    "members = [\"game\", \"query\"]\n",
                    "member_offsets = { game = 0 }\n",
                ),
                "missing from member_offsets",
            ),
            (
                27_015,
                "udp",
                27_015,
                "udp",
                concat!(
                    "id = \"game_query\"\n",
                    "members = [\"game\", \"query\"]\n",
                    "member_offsets = { game = 0, query = 0 }\n",
                ),
                "more than once at offset 0",
            ),
            (
                27_015,
                "udp",
                27_017,
                "udp",
                concat!(
                    "id = \"game_query\"\n",
                    "members = [\"game\", \"query\"]\n",
                    "member_offsets = { game = 0, query = 1 }\n",
                ),
                "same base port",
            ),
            (
                1,
                "udp",
                0,
                "udp",
                concat!(
                    "id = \"game_query\"\n",
                    "members = [\"game\", \"query\"]\n",
                    "member_offsets = { game = 0, query = 1 }\n",
                ),
                "below its offset",
            ),
            (
                27_015,
                "udp",
                27_016,
                "udp",
                concat!(
                    "id = \"game_query\"\n",
                    "members = [\"game\", \" query \"]\n",
                    "member_offsets = { game = 0, \" query \" = 1 }\n",
                ),
                "surrounding whitespace",
            ),
        ];

        for (game_port, game_protocol, query_port, query_protocol, group, expected) in cases {
            let temp_root = write_port_group_fixture(
                game_port,
                game_protocol,
                query_port,
                query_protocol,
                group,
            );
            let error = discover_modules(&temp_root).expect_err("invalid offset group must fail");
            assert!(
                error.to_string().contains(expected),
                "expected `{expected}` in `{error}`"
            );
            let _ = fs::remove_dir_all(temp_root);
        }
    }

    #[test]
    fn discover_modules_rejects_unknown_group_fields_and_offsets_above_u16() {
        for group in [
            concat!(
                "id = \"game_query\"\n",
                "members = [\"game\", \"query\"]\n",
                "offsets = { game = 0, query = 1 }\n",
            ),
            concat!(
                "id = \"game_query\"\n",
                "members = [\"game\", \"query\"]\n",
                "member_offsets = { game = 0, query = 65536 }\n",
            ),
        ] {
            let temp_root = write_port_group_fixture(27_015, "udp", 27_016, "udp", group);
            let error = discover_modules(&temp_root).expect_err("closed group schema must fail");
            assert!(matches!(error, ModuleDiscoveryError::ParseManifest { .. }));
            let _ = fs::remove_dir_all(temp_root);
        }
    }

    #[test]
    fn runtime_join_profile_is_never_inferred_from_server_metadata() {
        let temp_root = create_temp_dir();
        let module_root = temp_root.join("no-join-module");
        fs::create_dir_all(&module_root).expect("create module root");
        fs::write(
            module_root.join("module.toml"),
            concat!(
                "id = \"no-join\"\n",
                "name = \"No Join\"\n",
                "version = \"1.0.0\"\n",
                "steam_app_id = 258550\n",
                "[[default_ports]]\n",
                "name = \"game\"\n",
                "protocol = \"udp\"\n",
                "port = 28015\n",
            ),
        )
        .expect("write manifest");

        let modules = discover_modules(&temp_root).expect("discover module");

        assert!(modules[0].runtime.join.is_none());
        let _ = fs::remove_dir_all(temp_root);
    }

    #[test]
    fn discover_modules_rejects_invalid_steam_connect_join_profiles() {
        let cases = [
            (
                concat!(
                    "kind = \"steam_connect\"\n",
                    "client_app_id = 0\n",
                    "join_port_name = \"game\"\n",
                    "query_port_name = \"query\"\n",
                ),
                "greater than zero",
            ),
            (
                concat!(
                    "kind = \"steam_connect\"\n",
                    "client_app_id = 252490\n",
                    "join_port_name = \"missing\"\n",
                    "query_port_name = \"query\"\n",
                ),
                "default_ports",
            ),
            (
                concat!(
                    "kind = \"steam_connect\"\n",
                    "client_app_id = 252490\n",
                    "join_port_name = \"rcon\"\n",
                    "query_port_name = \"query\"\n",
                ),
                "player port role",
            ),
            (
                concat!(
                    "kind = \"steam_connect\"\n",
                    "client_app_id = 252490\n",
                    "join_port_name = \"game\"\n",
                    "query_port_name = \"game\"\n",
                ),
                "player_query candidate",
            ),
            (
                concat!(
                    "kind = \"steam_connect\"\n",
                    "client_app_id = 252490\n",
                    "join_port_name = \" game \"\n",
                    "query_port_name = \"query\"\n",
                ),
                "surrounding whitespace",
            ),
        ];

        for (join_profile, expected) in cases {
            let temp_root = write_join_profile_fixture(join_profile);
            let error = discover_modules(&temp_root).expect_err("invalid join profile must fail");
            assert!(
                error.to_string().contains(expected),
                "expected `{expected}` in `{error}`"
            );
            let _ = fs::remove_dir_all(temp_root);
        }
    }

    #[test]
    fn discover_modules_rejects_unknown_join_kind_and_fields() {
        for join_profile in [
            concat!(
                "kind = \"direct_udp\"\n",
                "client_app_id = 252490\n",
                "join_port_name = \"game\"\n",
                "query_port_name = \"query\"\n",
            ),
            concat!(
                "kind = \"steam_connect\"\n",
                "client_app_id = 252490\n",
                "join_port_name = \"game\"\n",
                "query_port_name = \"query\"\n",
                "host = \"192.0.2.1\"\n",
            ),
        ] {
            let temp_root = write_join_profile_fixture(join_profile);
            let error = discover_modules(&temp_root).expect_err("closed join profile must fail");
            assert!(matches!(error, ModuleDiscoveryError::ParseManifest { .. }));
            let _ = fs::remove_dir_all(temp_root);
        }
    }

    #[test]
    fn discover_modules_requires_a_udp_join_query_port() {
        let temp_root = write_join_profile_fixture_with_query_protocol(
            concat!(
                "kind = \"steam_connect\"\n",
                "client_app_id = 252490\n",
                "join_port_name = \"game\"\n",
                "query_port_name = \"query\"\n",
            ),
            "tcp",
        );

        let error = discover_modules(&temp_root).expect_err("TCP query port must fail");

        assert!(error.to_string().contains("UDP default port"));
        let _ = fs::remove_dir_all(temp_root);
    }

    #[test]
    fn discover_modules_requires_a2s_info_for_steam_connect() {
        let temp_root = write_join_profile_fixture_with_protocols(
            concat!(
                "kind = \"steam_connect\"\n",
                "client_app_id = 252490\n",
                "join_port_name = \"game\"\n",
                "query_port_name = \"query\"\n",
            ),
            "udp",
            "custom_status",
        );

        let error = discover_modules(&temp_root).expect_err("non-A2S query adapter must fail");

        assert!(error.to_string().contains("protocol `a2s_info`"));
        let _ = fs::remove_dir_all(temp_root);
    }

    #[test]
    fn discover_modules_reads_and_serializes_runtime_port_roles() {
        let temp_root = write_port_role_fixture(concat!(
            "[[runtime.port_roles]]\n",
            "role = \"player\"\n",
            "port_names = [\"game\"]\n",
            "[[runtime.port_roles]]\n",
            "role = \"service\"\n",
            "port_names = [\"rcon\"]\n",
        ));

        let modules = discover_modules(&temp_root).expect("discover modules");
        let descriptor = modules.first().expect("descriptor");
        assert_eq!(descriptor.runtime.port_roles.len(), 2);
        assert_eq!(
            descriptor.runtime.port_roles[0].role,
            ModulePortRole::Player
        );
        assert_eq!(descriptor.runtime.port_roles[0].port_names, ["game"]);
        assert_eq!(
            descriptor.runtime.port_roles[1].role,
            ModulePortRole::Service
        );
        assert_eq!(descriptor.runtime.port_roles[1].port_names, ["rcon"]);

        let details = ModuleDetails {
            summary: descriptor.summary.clone(),
            schema_json: descriptor.schema_json.clone(),
            default_ports: descriptor.default_ports.clone(),
            install: descriptor.install.clone(),
            process: descriptor.process.clone(),
            workshop: descriptor.workshop.clone(),
            mods: None,
            runtime: descriptor.runtime.clone(),
        };
        let serialized = toml::Value::try_from(details).expect("serialize module details");
        let serialized_roles = serialized["runtime"]["port_roles"]
            .as_array()
            .expect("serialized runtime port roles");
        assert_eq!(serialized_roles[0]["role"].as_str(), Some("player"));
        assert_eq!(serialized_roles[0]["port_names"][0].as_str(), Some("game"));
        assert_eq!(serialized_roles[1]["role"].as_str(), Some("service"));
        assert_eq!(serialized_roles[1]["port_names"][0].as_str(), Some("rcon"));

        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn discover_modules_rejects_unknown_runtime_port_role_member() {
        let temp_root = write_port_role_fixture(concat!(
            "[[runtime.port_roles]]\n",
            "role = \"player\"\n",
            "port_names = [\"game\"]\n",
            "[[runtime.port_roles]]\n",
            "role = \"service\"\n",
            "port_names = [\"admin\"]\n",
        ));

        let error = discover_modules(&temp_root).expect_err("unknown role member must fail");
        assert!(error.to_string().contains("admin"));
        assert!(error.to_string().contains("default_ports"));

        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn discover_modules_rejects_duplicate_runtime_port_role() {
        let temp_root = write_port_role_fixture(concat!(
            "[[runtime.port_roles]]\n",
            "role = \"player\"\n",
            "port_names = [\"game\"]\n",
            "[[runtime.port_roles]]\n",
            "role = \"player\"\n",
            "port_names = [\"rcon\"]\n",
        ));

        let error = discover_modules(&temp_root).expect_err("duplicate role must fail");
        assert!(error.to_string().contains("player"));
        assert!(error.to_string().contains("more than once"));

        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn discover_modules_rejects_overlapping_runtime_port_roles() {
        let temp_root = write_port_role_fixture(concat!(
            "[[runtime.port_roles]]\n",
            "role = \"player\"\n",
            "port_names = [\"game\", \"rcon\"]\n",
            "[[runtime.port_roles]]\n",
            "role = \"service\"\n",
            "port_names = [\"rcon\"]\n",
        ));

        let error = discover_modules(&temp_root).expect_err("overlapping roles must fail");
        assert!(error.to_string().contains("rcon"));
        assert!(error.to_string().contains("more than one role"));

        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn discover_modules_rejects_empty_runtime_port_role_member() {
        let temp_root = write_port_role_fixture(concat!(
            "[[runtime.port_roles]]\n",
            "role = \"player\"\n",
            "port_names = [\"game\"]\n",
            "[[runtime.port_roles]]\n",
            "role = \"service\"\n",
            "port_names = [\"\"]\n",
        ));

        let error = discover_modules(&temp_root).expect_err("empty role member must fail");
        assert!(error.to_string().contains("non-empty"));

        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn discover_modules_rejects_incomplete_runtime_port_role_coverage() {
        let temp_root = write_port_role_fixture(concat!(
            "[[runtime.port_roles]]\n",
            "role = \"player\"\n",
            "port_names = [\"game\"]\n",
        ));

        let error = discover_modules(&temp_root).expect_err("incomplete roles must fail");
        assert!(error.to_string().contains("rcon"));
        assert!(error.to_string().contains("exactly one role"));

        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn discover_modules_reads_strict_bind_address_policy() {
        let temp_root = create_temp_dir();
        let module_root = temp_root.join("strict-bind-module");
        fs::create_dir_all(&module_root).expect("create module root");
        fs::write(
            module_root.join("module.toml"),
            concat!(
                "id = \"strict-bind\"\n",
                "name = \"Strict Bind\"\n",
                "version = \"1.0.0\"\n",
                "[[default_ports]]\n",
                "name = \"game\"\n",
                "protocol = \"udp\"\n",
                "port = 14159\n",
                "[runtime.bind_address]\n",
                "mode = \"strict\"\n",
                "port_names = [\"game\"]\n",
                "startup_timeout_ms = 90000\n",
            ),
        )
        .expect("write manifest");

        let modules = discover_modules(&temp_root).expect("discover modules");
        let bind = &modules.first().expect("descriptor").runtime.bind_address;
        assert_eq!(bind.mode, ModuleBindAddressMode::Strict);
        assert_eq!(bind.port_names, vec![String::from("game")]);
        assert_eq!(bind.startup_timeout_ms, 90_000);

        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn strict_bind_address_policy_rejects_unknown_verification_port() {
        let error = validate_bind_address_policy(
            Path::new("module.toml"),
            &[PortBinding {
                name: String::from("game"),
                protocol: String::from("udp"),
                port: 14159,
            }],
            &ModuleBindAddressSpec {
                mode: ModuleBindAddressMode::Strict,
                port_names: vec![String::from("query")],
                required_setting_key: None,
                startup_timeout_ms: 60_000,
            },
        )
        .expect_err("unknown verification port must fail");

        assert!(error.to_string().contains("query"));
        assert!(error.to_string().contains("default_ports"));
    }

    #[test]
    fn discover_modules_reads_runtime_performance_policy() {
        let temp_root = create_temp_dir();
        let module_root = temp_root.join("performance-module");
        fs::create_dir_all(&module_root).expect("create module root");
        fs::write(
            module_root.join("module.toml"),
            concat!(
                "id = \"performance\"\n",
                "name = \"Performance Module\"\n",
                "version = \"1.0.0\"\n\n",
                "[runtime.performance]\n",
                "priority_class = \"above_normal\"\n",
                "apply_to_child_processes = false\n",
                "startup_stagger_ms = 2500\n",
                "child_process_stagger_ms = 750\n",
            ),
        )
        .expect("write manifest");

        let modules = discover_modules(&temp_root).expect("discover modules");
        let descriptor = modules.first().expect("descriptor");
        let policy = &descriptor.runtime.performance;

        assert_eq!(policy.priority_class, RuntimePriorityClass::AboveNormal);
        assert!(!policy.apply_to_child_processes);
        assert_eq!(policy.startup_stagger_ms, 2500);
        assert_eq!(policy.child_process_stagger_ms, 750);

        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn discover_modules_reads_runtime_player_actions() {
        let temp_root = create_temp_dir();
        let module_root = temp_root.join("player-actions-module");
        fs::create_dir_all(&module_root).expect("create module root");
        fs::write(
            module_root.join("module.toml"),
            concat!(
                "id = \"player-actions\"\n",
                "name = \"Player Actions Module\"\n",
                "version = \"1.0.0\"\n\n",
                "[runtime.player_query]\n",
                "protocol = \"none\"\n\n",
                "[[runtime.player_actions]]\n",
                "id = \"ban_player\"\n",
                "label = \"Ban\"\n",
                "label_zh_cn = \"封禁\"\n",
                "transport = \"source_rcon\"\n",
                "command_template = \"ban {{target}}\"\n",
                "target_label = \"Player\"\n",
                "target_placeholder = \"Player name\"\n",
                "target_required = true\n",
                "target_encoding = \"quoted_string\"\n",
                "port_name = \"rcon\"\n",
                "password_setting_key = \"rcon_password\"\n",
                "enabled_setting_key = \"rcon_enabled\"\n",
                "destructive = true\n\n",
                "[player_management]\n",
                "status = \"pending_adapter\"\n",
                "planned_surface = \"source_rcon\"\n",
                "reason = \"test manifest pending reason\"\n",
                "verification = \"run test command\"\n\n",
                "[[runtime.player_actions]]\n",
                "id = \"set_role\"\n",
                "label = \"Set role\"\n",
                "command_template = \"role {{target}} {{role}}\"\n",
                "target_required = true\n",
                "role_values = [\"USER\", \"ADMIN\"]\n",
            ),
        )
        .expect("write manifest");

        let modules = discover_modules(&temp_root).expect("discover modules");
        let actions = &modules.first().expect("descriptor").runtime.player_actions;

        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].id, "ban_player");
        assert_eq!(actions[0].label_zh_cn.as_deref(), Some("封禁"));
        assert_eq!(actions[0].transport, "source_rcon");
        assert_eq!(actions[0].target_encoding.as_deref(), Some("quoted_string"));
        assert_eq!(actions[0].port_name.as_deref(), Some("rcon"));
        assert_eq!(
            actions[0].password_setting_key.as_deref(),
            Some("rcon_password")
        );
        assert_eq!(
            actions[0].enabled_setting_key.as_deref(),
            Some("rcon_enabled")
        );
        assert!(actions[0].target_required);
        assert!(actions[0].destructive);
        assert_eq!(
            actions[1].role_values,
            vec![String::from("USER"), String::from("ADMIN")]
        );
        let player_management = modules
            .first()
            .expect("descriptor")
            .runtime
            .player_management
            .as_ref()
            .expect("player management contract");
        assert_eq!(player_management.status, "pending_adapter");
        assert_eq!(player_management.planned_surface, "source_rcon");
        assert_eq!(player_management.reason, "test manifest pending reason");
        assert_eq!(player_management.verification, "run test command");

        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn discover_modules_reads_runtime_player_list_defaults() {
        let temp_root = write_player_list_fixture(concat!(
            "source = \"structured_log\"\n",
            "action_id = \"list_online_players\"\n",
            "player_action_ids = [\"kick_userid\"]\n",
            "response_codec = \"dst_client_table_v1\"\n",
            "identity_kind = \"klei_user_id\"\n",
        ));

        let modules = discover_modules(&temp_root).expect("discover module");
        let player_list = modules[0]
            .runtime
            .player_list
            .as_ref()
            .expect("player list spec");

        assert_eq!(player_list.scope, ModulePlayerListScope::Online);
        assert_eq!(player_list.source, ModulePlayerListSource::StructuredLog);
        assert_eq!(player_list.refresh_interval_ms, 30_000);
        assert_eq!(player_list.player_action_ids, ["kick_userid"]);

        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn discover_modules_rejects_invalid_runtime_player_list_contracts() {
        let cases = [
            (
                "scope = \"offline\"\nsource = \"structured_log\"\naction_id = \"list_online_players\"\nplayer_action_ids = [\"kick_userid\"]\nresponse_codec = \"dst_client_table_v1\"\nidentity_kind = \"klei_user_id\"\n",
                "scope",
            ),
            (
                "source = \"unknown\"\naction_id = \"list_online_players\"\nplayer_action_ids = [\"kick_userid\"]\nresponse_codec = \"dst_client_table_v1\"\nidentity_kind = \"klei_user_id\"\n",
                "source",
            ),
            (
                "source = \"structured_log\"\naction_id = \"list_online_players\"\nplayer_action_ids = [\"kick_userid\"]\nresponse_codec = \"unknown\"\nidentity_kind = \"klei_user_id\"\n",
                "response_codec",
            ),
            (
                "source = \"structured_log\"\naction_id = \"list_online_players\"\nplayer_action_ids = [\"kick_userid\"]\nresponse_codec = \"dst_client_table_v1\"\nidentity_kind = \"unknown\"\n",
                "identity_kind",
            ),
            (
                "source = \"structured_log\"\nplayer_action_ids = [\"kick_userid\"]\nresponse_codec = \"dst_client_table_v1\"\nidentity_kind = \"klei_user_id\"\n",
                "action_id",
            ),
            (
                "source = \"structured_log\"\naction_id = \"missing\"\nplayer_action_ids = [\"kick_userid\"]\nresponse_codec = \"dst_client_table_v1\"\nidentity_kind = \"klei_user_id\"\n",
                "action_id",
            ),
            (
                "source = \"structured_log\"\naction_id = \"kick_userid\"\nplayer_action_ids = [\"kick_userid\"]\nresponse_codec = \"dst_client_table_v1\"\nidentity_kind = \"klei_user_id\"\n",
                "target-free",
            ),
            (
                "source = \"structured_log\"\naction_id = \"list_destructive\"\nplayer_action_ids = [\"kick_userid\"]\nresponse_codec = \"dst_client_table_v1\"\nidentity_kind = \"klei_user_id\"\n",
                "non-destructive",
            ),
            (
                "source = \"structured_log\"\naction_id = \"list_non_stdin\"\nplayer_action_ids = [\"kick_userid\"]\nresponse_codec = \"dst_client_table_v1\"\nidentity_kind = \"klei_user_id\"\n",
                "stdin",
            ),
            (
                "source = \"structured_log\"\naction_id = \"list_without_request\"\nplayer_action_ids = [\"kick_userid\"]\nresponse_codec = \"dst_client_table_v1\"\nidentity_kind = \"klei_user_id\"\n",
                "request_id",
            ),
            (
                "source = \"structured_log\"\naction_id = \"list_with_role\"\nplayer_action_ids = [\"kick_userid\"]\nresponse_codec = \"dst_client_table_v1\"\nidentity_kind = \"klei_user_id\"\n",
                "request_id",
            ),
            (
                "source = \"structured_log\"\naction_id = \"list_with_unknown\"\nplayer_action_ids = [\"kick_userid\"]\nresponse_codec = \"dst_client_table_v1\"\nidentity_kind = \"klei_user_id\"\n",
                "request_id",
            ),
            (
                "source = \"runtime_action\"\naction_id = \"list_online_players\"\nplayer_action_ids = [\"kick_userid\"]\nresponse_codec = \"dst_client_table_v1\"\nidentity_kind = \"klei_user_id\"\n",
                "source",
            ),
            (
                "source = \"structured_log\"\naction_id = \"list_online_players\"\nplayer_action_ids = [\"kick_userid\"]\nresponse_codec = \"dst_client_table_v1\"\nidentity_kind = \"klei_user_id\"\nrefresh_interval_ms = 14999\n",
                "refresh_interval_ms",
            ),
            (
                "source = \"structured_log\"\naction_id = \"list_online_players\"\nplayer_action_ids = [\"kick_userid\"]\nresponse_codec = \"dst_client_table_v1\"\nidentity_kind = \"klei_user_id\"\nrefresh_interval_ms = 120001\n",
                "refresh_interval_ms",
            ),
        ];

        for (override_fields, expected_message) in cases {
            let temp_root = write_player_list_fixture(override_fields);
            let error = discover_modules(&temp_root).expect_err("invalid player list must fail");
            assert!(matches!(
                error,
                ModuleDiscoveryError::InvalidPlayerList { .. }
            ));
            assert!(
                error.to_string().contains(expected_message),
                "expected error to mention {expected_message}, got {error}"
            );
            let _ = fs::remove_dir_all(&temp_root);
        }
    }

    #[test]
    fn discover_modules_rejects_unknown_player_list_action() {
        let temp_root = write_player_list_fixture(
            "source = \"structured_log\"\naction_id = \"list_online_players\"\nplayer_action_ids = [\"missing\"]\nresponse_codec = \"dst_client_table_v1\"\nidentity_kind = \"klei_user_id\"\n",
        );

        let error = discover_modules(&temp_root).expect_err("unknown row action must fail");
        assert!(matches!(
            error,
            ModuleDiscoveryError::InvalidPlayerList { .. }
        ));
        assert!(error.to_string().contains("missing"));

        let _ = fs::remove_dir_all(&temp_root);
    }

    mod player_list {
        use super::*;

        #[test]
        fn discover_modules_rejects_invalid_player_row_action() {
            for (override_action, expected_message) in [
                ("target_required = false\n", "require a target"),
                ("command_template = \"kick player\"\n", "{{target}}"),
                (
                    "command_template = \"kick {{target}} {{request_id}}\"\n",
                    "request_id",
                ),
                ("command_template = \"kick {{target}} {{role}}\"\n", "role"),
                (
                    "command_template = \"kick {{target}} {{reason}}\"\n",
                    "reason",
                ),
                (
                    "player_action_ids = [\"kick_userid\", \"kick_userid\"]\n",
                    "duplicate",
                ),
            ] {
                let temp_root = write_player_list_fixture_with_row_action(override_action);
                let error = discover_modules(&temp_root).expect_err("invalid row action must fail");
                assert!(matches!(
                    error,
                    ModuleDiscoveryError::InvalidPlayerList { .. }
                ));
                assert!(
                    error.to_string().contains(expected_message),
                    "expected error to mention {expected_message}, got {error}"
                );
                let _ = fs::remove_dir_all(&temp_root);
            }
        }
    }

    #[test]
    fn discover_modules_reads_runtime_action_kind() {
        let temp_root = create_temp_dir();
        let module_root = temp_root.join("broadcast-action-module");
        fs::create_dir_all(&module_root).expect("create module root");
        fs::write(
            module_root.join("module.toml"),
            concat!(
                "id = \"broadcast-action\"\n",
                "name = \"Broadcast Action Module\"\n",
                "version = \"1.0.0\"\n\n",
                "[[runtime.player_actions]]\n",
                "id = \"broadcast\"\n",
                "kind = \"broadcast\"\n",
                "label = \"Broadcast\"\n",
                "transport = \"source_rcon\"\n",
                "command_template = \"say {{message}}\"\n",
                "port_name = \"rcon\"\n",
                "password_setting_key = \"rcon_password\"\n",
                "enabled_setting_key = \"rcon_enabled\"\n",
            ),
        )
        .expect("write manifest");

        let modules = discover_modules(&temp_root).expect("discover modules");
        let actions = &modules.first().expect("descriptor").runtime.player_actions;

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id, "broadcast");
        assert_eq!(actions[0].kind.as_deref(), Some("broadcast"));

        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn discover_modules_reads_runtime_shutdown_spec() {
        let temp_root = create_temp_dir();
        let module_root = temp_root.join("shutdown-module");
        fs::create_dir_all(&module_root).expect("create module root");
        fs::write(
            module_root.join("module.toml"),
            concat!(
                "id = \"shutdown\"\n",
                "name = \"Shutdown Module\"\n",
                "version = \"1.0.0\"\n\n",
                "[runtime.shutdown]\n",
                "grace_period_ms = 9000\n\n",
                "[[runtime.shutdown.commands]]\n",
                "transport = \"telnet\"\n",
                "fallback_transport = \"stdin\"\n",
                "command = \"saveworld\"\n",
                "wait_after_ms = 1500\n\n",
                "[[runtime.shutdown.commands]]\n",
                "transport = \"source_rcon\"\n",
                "command = \"quit\"\n",
                "port_name = \"rcon\"\n",
                "password_setting_key = \"rcon_password\"\n",
                "enabled_setting_key = \"rcon_enabled\"\n",
                "wait_after_ms = 3000\n",
            ),
        )
        .expect("write manifest");

        let modules = discover_modules(&temp_root).expect("discover modules");
        let shutdown = modules
            .first()
            .expect("descriptor")
            .runtime
            .shutdown
            .as_ref()
            .expect("shutdown spec");

        assert_eq!(shutdown.grace_period_ms, 9000);
        assert_eq!(shutdown.commands.len(), 2);
        assert_eq!(shutdown.commands[0].transport, "telnet");
        assert_eq!(
            shutdown.commands[0].fallback_transport.as_deref(),
            Some("stdin")
        );
        assert_eq!(shutdown.commands[0].command, "saveworld");
        assert_eq!(shutdown.commands[0].wait_after_ms, 1500);
        assert_eq!(shutdown.commands[1].transport, "source_rcon");
        assert_eq!(shutdown.commands[1].port_name.as_deref(), Some("rcon"));
        assert_eq!(
            shutdown.commands[1].password_setting_key.as_deref(),
            Some("rcon_password")
        );
        assert_eq!(
            shutdown.commands[1].enabled_setting_key.as_deref(),
            Some("rcon_enabled")
        );

        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn discover_modules_derives_default_host_surface_from_window_policy() {
        let temp_root = create_temp_dir();
        let module_root = temp_root.join("host-surface-module");
        fs::create_dir_all(&module_root).expect("create module root");
        fs::write(
            module_root.join("module.toml"),
            concat!(
                "id = \"host-surface\"\n",
                "name = \"Host Surface Module\"\n",
                "version = \"1.0.0\"\n\n",
                "[process]\n",
                "executable = \"server.exe\"\n",
                "window_policy = \"external\"\n",
            ),
        )
        .expect("write manifest");

        let modules = discover_modules(&temp_root).expect("discover modules");
        let descriptor = modules.first().expect("descriptor");
        let process = descriptor.process.as_ref().expect("process spec");

        assert_eq!(process.window_policy, ProcessWindowPolicy::External);
        assert_eq!(process.host_surface, ProcessHostSurface::ExternalWindow);

        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn repo_modules_keep_background_window_hosting_contracts() {
        let repo_modules_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules");
        let modules = discover_modules(&repo_modules_root).expect("discover repo modules");
        let managed_native_window_exceptions = ["sevendaystodie"];
        let managed_pseudo_console_exceptions = ["barotrauma", "unturned"];

        assert_eq!(
            modules.len(),
            32,
            "the reviewed repository catalog should contain all 32 game modules"
        );

        let mut managed_terminal_count = 0;
        let mut managed_native_window_modules = Vec::new();
        let mut managed_pseudo_console_modules = Vec::new();
        for descriptor in &modules {
            let module_id = descriptor.summary.id.as_str();
            let process = descriptor
                .process
                .as_ref()
                .unwrap_or_else(|| panic!("missing process spec for repo module: {module_id}"));

            assert_eq!(
                process.window_policy,
                ProcessWindowPolicy::Background,
                "{module_id} should stay background-hosted on Windows"
            );
            if managed_native_window_exceptions.contains(&module_id) {
                assert_eq!(
                    process.host_surface,
                    ProcessHostSurface::ManagedNativeWindow,
                    "{module_id} is the only reviewed managed-native-window exception"
                );
                managed_native_window_modules.push(module_id);
            } else if managed_pseudo_console_exceptions.contains(&module_id) {
                assert_eq!(
                    process.host_surface,
                    ProcessHostSurface::ManagedPseudoConsole,
                    "{module_id} requires its verified console input buffer"
                );
                managed_pseudo_console_modules.push(module_id);
            } else {
                assert_eq!(
                    process.host_surface,
                    ProcessHostSurface::ManagedTerminal,
                    "{module_id} should route its host surface through its verified managed terminal"
                );
                managed_terminal_count += 1;
            }

            // Native logging remains on the managed private desktop. Windrose
            // additionally needs FConsoleWindow for its game-thread quit command.
            assert_eq!(
                process
                    .args_template
                    .iter()
                    .filter(|arg| arg.eq_ignore_ascii_case("-newconsole"))
                    .count(),
                usize::from(module_id == "windrose"),
                "{module_id} must use only its declared native GUI command channel"
            );
            if module_id == "windrose" {
                let shutdown = descriptor
                    .runtime
                    .shutdown
                    .as_ref()
                    .expect("Windrose shutdown");
                assert_eq!(
                    shutdown
                        .commands
                        .iter()
                        .map(|command| (command.transport.as_str(), command.command.as_str()))
                        .collect::<Vec<_>>(),
                    [("unreal_console", "quit")]
                );
                assert!(shutdown.commands[0].fallback_transport.is_none());
            }
        }

        assert_eq!(managed_terminal_count, 29);
        assert_eq!(
            managed_pseudo_console_modules,
            managed_pseudo_console_exceptions
        );
        assert_eq!(
            managed_native_window_modules,
            managed_native_window_exceptions
        );

        let abioticfactor = modules
            .iter()
            .find(|descriptor| descriptor.summary.id == "abioticfactor")
            .expect("missing abioticfactor repo module");
        let abiotic_process = abioticfactor
            .process
            .as_ref()
            .expect("abioticfactor process spec");

        for flag in ["-log", "-stdout", "-FullStdOutLogOutput"] {
            assert_eq!(
                abiotic_process
                    .args_template
                    .iter()
                    .filter(|arg| arg.eq_ignore_ascii_case(flag))
                    .count(),
                1,
                "Abiotic Factor requires {flag} for its managed native console"
            );
        }

        let minecraft = modules
            .iter()
            .find(|descriptor| descriptor.summary.id == "minecraft")
            .expect("missing minecraft repo module");
        let minecraft_install = minecraft.install.as_ref().expect("minecraft install spec");
        assert_eq!(minecraft_install.source, Some(InstallSource::MinecraftJava));
        assert_eq!(
            minecraft_install.verification_path.as_deref(),
            Some("server.jar")
        );
        assert_eq!(
            minecraft_install
                .minecraft
                .as_ref()
                .map(|spec| spec.server_jar.as_str()),
            Some("server.jar")
        );
    }

    #[test]
    fn repo_modules_include_requested_survival_wave_install_chain() {
        let repo_modules_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules");
        let modules = discover_modules(&repo_modules_root).expect("discover repo modules");
        let expected_modules = [
            ("returntomoria", 3349480, "MoriaServer.exe"),
            ("astroneer", 728470, "AstroServer.exe"),
            ("nightingale", 3796810, "NWXServer.exe"),
            ("theforest", 556450, "TheForestDedicatedServer.exe"),
        ];

        for (module_id, steam_app_id, expected_executable) in expected_modules {
            let descriptor = modules
                .iter()
                .find(|descriptor| descriptor.summary.id == module_id)
                .unwrap_or_else(|| panic!("missing requested survival module: {module_id}"));
            let install = descriptor
                .install
                .as_ref()
                .unwrap_or_else(|| panic!("{module_id} should declare an install spec"));
            let process = descriptor
                .process
                .as_ref()
                .unwrap_or_else(|| panic!("{module_id} should declare a process spec"));
            let player_management = descriptor
                .runtime
                .player_management
                .as_ref()
                .unwrap_or_else(|| panic!("{module_id} should document player-management state"));

            assert_eq!(
                descriptor.summary.steam_app_id,
                Some(steam_app_id),
                "{module_id} should declare the reviewed Steam dedicated-server app id"
            );
            assert!(
                install.source.is_none(),
                "{module_id} should stay on the SteamCMD install path"
            );
            assert_eq!(
                install.verification_path.as_deref(),
                Some(expected_executable),
                "{module_id} should verify the executable LanGame starts"
            );
            assert_eq!(
                process.executable, expected_executable,
                "{module_id} should launch the verified dedicated-server executable"
            );
            assert_eq!(
                process.window_policy,
                ProcessWindowPolicy::Background,
                "{module_id} should be hosted as a managed background server"
            );
            assert_eq!(
                process.host_surface,
                ProcessHostSurface::ManagedTerminal,
                "{module_id} should route console output through the managed terminal"
            );
            assert!(
                process
                    .args_template
                    .iter()
                    .all(|arg| !arg.eq_ignore_ascii_case("-newconsole")),
                "{module_id} does not use the native GUI command channel"
            );
            assert_eq!(
                process
                    .args_template
                    .iter()
                    .filter(|arg| arg.eq_ignore_ascii_case("-log"))
                    .count(),
                usize::from(matches!(module_id, "astroneer" | "nightingale")),
                "{module_id} must preserve its declared native logging profile"
            );
            assert!(
                descriptor.runtime.shutdown.is_some(),
                "{module_id} should declare an app-exit shutdown strategy"
            );
            assert!(
                !descriptor.default_ports.is_empty(),
                "{module_id} should expose at least one default port for allocation"
            );
            assert_eq!(
                player_management.status, "pending_adapter",
                "{module_id} live player actions should remain gated until adapter validation"
            );
        }
    }

    #[test]
    fn repo_modules_include_runescape_dragonwilds_install_chain() {
        let repo_modules_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules");
        let modules = discover_modules(&repo_modules_root).expect("discover repo modules");
        let descriptor = modules
            .iter()
            .find(|descriptor| descriptor.summary.id == "runescapedragonwilds")
            .expect("missing RuneScape: Dragonwilds repo module");
        let install = descriptor
            .install
            .as_ref()
            .expect("runescapedragonwilds should declare an install spec");
        let process = descriptor
            .process
            .as_ref()
            .expect("runescapedragonwilds should declare a process spec");
        let player_management = descriptor
            .runtime
            .player_management
            .as_ref()
            .expect("runescapedragonwilds should document player-management state");

        assert_eq!(descriptor.summary.steam_app_id, Some(4019830));
        assert!(install.source.is_none());
        assert_eq!(
            install.verification_path.as_deref(),
            Some("RSDragonwildsServer.exe")
        );
        assert_eq!(process.executable, "RSDragonwildsServer.exe");
        assert_eq!(
            descriptor.storage.saves_path_template.as_deref(),
            Some("{{paths.install_root}}/RSDragonwilds/Saved/SaveGames")
        );
        assert!(
            process
                .args_template
                .iter()
                .any(|arg| arg == "-Port={{ports.game.port}}"),
            "runescapedragonwilds should pass the configured game UDP port to the server"
        );
        assert!(
            process
                .args_template
                .iter()
                .any(|arg| arg == "-QueryPort={{ports.query.port}}"),
            "runescapedragonwilds should pass the configured query UDP port to the server"
        );
        assert_eq!(process.window_policy, ProcessWindowPolicy::Background);
        assert_eq!(process.host_surface, ProcessHostSurface::ManagedTerminal);
        assert!(
            process
                .args_template
                .iter()
                .all(|arg| !arg.eq_ignore_ascii_case("-newconsole")),
            "runescapedragonwilds does not use the native GUI command channel"
        );
        assert_eq!(
            process
                .args_template
                .iter()
                .filter(|arg| arg.eq_ignore_ascii_case("-log"))
                .count(),
            1,
            "runescapedragonwilds requires the documented native logging flag"
        );
        let shutdown = descriptor
            .runtime
            .shutdown
            .as_ref()
            .expect("Dragonwilds shutdown");
        assert_eq!(
            shutdown
                .commands
                .iter()
                .map(|command| (command.transport.as_str(), command.command.as_str()))
                .collect::<Vec<_>>(),
            [("console_ctrl_c", "CTRL_C")]
        );
        assert!(
            descriptor
                .default_ports
                .iter()
                .any(|port| port.name == "game" && port.protocol == "udp" && port.port == 7777),
            "runescapedragonwilds should expose the documented UDP game port"
        );
        assert!(
            descriptor.default_ports.iter().any(|port| {
                port.name == "query" && port.protocol == "udp" && port.port == 27057
            }),
            "runescapedragonwilds should expose the smoke-verified UDP query port"
        );
        assert_eq!(player_management.status, "pending_adapter");
    }

    #[test]
    fn repo_modules_declare_shutdown_contracts() {
        let repo_modules_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules");
        let modules = discover_modules(&repo_modules_root).expect("discover repo modules");
        assert!(!modules.is_empty(), "repo modules should be discoverable");

        for descriptor in &modules {
            let shutdown = descriptor.runtime.shutdown.as_ref().unwrap_or_else(|| {
                panic!(
                    "repo module '{}' must declare [runtime.shutdown] so every stop has an owned stop/save strategy",
                    descriptor.summary.id
                )
            });

            assert!(
                !shutdown.commands.is_empty(),
                "repo module '{}' must declare at least one shutdown command or console interrupt",
                descriptor.summary.id
            );
        }
    }

    #[test]
    fn repo_modules_expose_client_join_only_for_rust() {
        let repo_modules_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules");
        let modules = discover_modules(&repo_modules_root).expect("discover repo modules");
        let join_profiles = modules
            .iter()
            .filter_map(|descriptor| {
                descriptor
                    .runtime
                    .join
                    .as_ref()
                    .map(|profile| (descriptor.summary.id.as_str(), profile))
            })
            .collect::<Vec<_>>();

        assert_eq!(
            join_profiles,
            vec![(
                "rust",
                &ModuleJoinProfile::SteamConnect {
                    client_app_id: 252490,
                    join_port_name: String::from("game"),
                    query_port_name: String::from("query"),
                }
            )]
        );
    }

    #[test]
    fn unturned_reserves_native_query_then_game_port_pair() {
        let repo_modules_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules");
        let modules = discover_modules(&repo_modules_root).expect("discover repo modules");
        let unturned = modules
            .iter()
            .find(|descriptor| descriptor.summary.id == "unturned")
            .expect("Unturned module");
        assert_eq!(
            unturned
                .default_ports
                .iter()
                .map(|port| (port.name.as_str(), port.port))
                .collect::<Vec<_>>(),
            vec![("game", 27_016), ("query", 27_015)]
        );
        assert_eq!(unturned.runtime.port_groups.len(), 1);
        assert_eq!(
            unturned.runtime.port_groups[0].member_offsets,
            Some(BTreeMap::from([
                (String::from("query"), 0),
                (String::from("game"), 1)
            ]))
        );
        assert_eq!(
            unturned.runtime.player_query.as_ref().unwrap().port_names,
            ["query"]
        );
        let arguments = &unturned.process.as_ref().unwrap().args_template;
        assert!(
            arguments
                .iter()
                .any(|arg| arg == "-Port/{{ports.query.port}}")
        );
        assert!(!arguments.iter().any(|arg| arg.starts_with("-QueryPort")));
    }

    #[test]
    fn core_keeper_uses_offset_a2s_query_without_publishing_a_join_profile() {
        let repo_modules_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules");
        let modules = discover_modules(&repo_modules_root).expect("discover repo modules");
        let core_keeper = modules
            .iter()
            .find(|descriptor| descriptor.summary.id == "corekeeper")
            .expect("Core Keeper module");

        let shutdown = core_keeper
            .runtime
            .shutdown
            .as_ref()
            .expect("Core Keeper shutdown");
        assert_eq!(shutdown.commands.len(), 1);
        assert_eq!(shutdown.commands[0].transport, "window_close");
        assert_eq!(shutdown.commands[0].command, "WM_CLOSE");
        assert!(shutdown.commands[0].fallback_transport.is_none());

        assert_eq!(
            core_keeper
                .default_ports
                .iter()
                .map(|port| (port.name.as_str(), port.protocol.as_str(), port.port))
                .collect::<Vec<_>>(),
            vec![("game", "udp", 27_015), ("query", "udp", 27_016)]
        );
        assert_eq!(core_keeper.runtime.port_groups.len(), 1);
        assert_eq!(
            core_keeper.runtime.port_groups[0].member_offsets,
            Some(BTreeMap::from([
                (String::from("game"), 0),
                (String::from("query"), 1),
            ]))
        );
        let player_query = core_keeper
            .runtime
            .player_query
            .as_ref()
            .expect("Core Keeper A2S query");
        assert_eq!(player_query.protocol, "a2s_info");
        assert_eq!(player_query.port_names, ["query"]);
        assert!(core_keeper.runtime.join.is_none());
        assert!(
            core_keeper
                .process
                .as_ref()
                .expect("Core Keeper process")
                .args_template
                .iter()
                .any(|argument| argument == "{{corekeeper.direct_port_value}}")
        );
        assert!(
            !core_keeper
                .process
                .as_ref()
                .expect("Core Keeper process")
                .args_template
                .iter()
                .any(|argument| argument.contains("query"))
        );
    }

    fn create_temp_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        let sequence = TEST_ROOT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "langame-app-modules-test-{}-{unique}-{sequence}",
            std::process::id(),
        ));
        fs::create_dir(&path).expect("create unique temp dir");
        path
    }

    fn write_port_role_fixture(port_roles: &str) -> PathBuf {
        let temp_root = create_temp_dir();
        let module_root = temp_root.join("port-role-module");
        fs::create_dir_all(&module_root).expect("create module root");
        fs::write(
            module_root.join("module.toml"),
            format!(
                concat!(
                    "id = \"port-role\"\n",
                    "name = \"Port Role Module\"\n",
                    "version = \"1.0.0\"\n",
                    "[[default_ports]]\n",
                    "name = \"game\"\n",
                    "protocol = \"udp\"\n",
                    "port = 27015\n",
                    "[[default_ports]]\n",
                    "name = \"rcon\"\n",
                    "protocol = \"tcp\"\n",
                    "port = 27020\n",
                    "{}",
                ),
                port_roles
            ),
        )
        .expect("write manifest");
        temp_root
    }

    fn write_port_group_fixture(
        game_port: u16,
        game_protocol: &str,
        query_port: u16,
        query_protocol: &str,
        port_group: &str,
    ) -> PathBuf {
        let temp_root = create_temp_dir();
        let module_root = temp_root.join("port-group-module");
        fs::create_dir_all(&module_root).expect("create module root");
        fs::write(
            module_root.join("module.toml"),
            format!(
                concat!(
                    "id = \"port-group\"\n",
                    "name = \"Port Group Module\"\n",
                    "version = \"1.0.0\"\n",
                    "[[default_ports]]\n",
                    "name = \"game\"\n",
                    "protocol = \"{}\"\n",
                    "port = {}\n",
                    "[[default_ports]]\n",
                    "name = \"query\"\n",
                    "protocol = \"{}\"\n",
                    "port = {}\n",
                    "[[runtime.port_groups]]\n",
                    "{}",
                ),
                game_protocol, game_port, query_protocol, query_port, port_group
            ),
        )
        .expect("write manifest");
        temp_root
    }

    fn write_join_profile_fixture(join_profile: &str) -> PathBuf {
        write_join_profile_fixture_with_query_protocol(join_profile, "udp")
    }

    fn write_join_profile_fixture_with_query_protocol(
        join_profile: &str,
        query_protocol: &str,
    ) -> PathBuf {
        write_join_profile_fixture_with_protocols(join_profile, query_protocol, "a2s_info")
    }

    fn write_join_profile_fixture_with_protocols(
        join_profile: &str,
        query_protocol: &str,
        player_query_protocol: &str,
    ) -> PathBuf {
        let temp_root = create_temp_dir();
        let module_root = temp_root.join("join-profile-module");
        fs::create_dir_all(&module_root).expect("create module root");
        fs::write(
            module_root.join("module.toml"),
            format!(
                concat!(
                    "id = \"join-profile\"\n",
                    "name = \"Join Profile\"\n",
                    "version = \"1.0.0\"\n",
                    "[[default_ports]]\n",
                    "name = \"game\"\n",
                    "protocol = \"udp\"\n",
                    "port = 28015\n",
                    "[[default_ports]]\n",
                    "name = \"query\"\n",
                    "protocol = \"{}\"\n",
                    "port = 28017\n",
                    "[[default_ports]]\n",
                    "name = \"rcon\"\n",
                    "protocol = \"tcp\"\n",
                    "port = 28016\n",
                    "[runtime.player_query]\n",
                    "protocol = \"{}\"\n",
                    "port_names = [\"query\"]\n",
                    "[runtime.join]\n",
                    "{}",
                    "[[runtime.port_roles]]\n",
                    "role = \"player\"\n",
                    "port_names = [\"game\", \"query\"]\n",
                    "[[runtime.port_roles]]\n",
                    "role = \"service\"\n",
                    "port_names = [\"rcon\"]\n",
                ),
                query_protocol, player_query_protocol, join_profile
            ),
        )
        .expect("write manifest");
        temp_root
    }

    fn write_player_list_fixture(player_list: &str) -> PathBuf {
        write_player_list_fixture_with_parts("", player_list)
    }

    fn write_player_list_fixture_with_row_action(override_action: &str) -> PathBuf {
        if override_action.starts_with("player_action_ids") {
            return write_player_list_fixture_with_parts(
                "",
                &format!(
                    concat!(
                        "source = \"structured_log\"\n",
                        "action_id = \"list_online_players\"\n",
                        "{}",
                        "response_codec = \"dst_client_table_v1\"\n",
                        "identity_kind = \"klei_user_id\"\n",
                    ),
                    override_action
                ),
            );
        }

        let row_action = if override_action.starts_with("target_required") {
            format!(
                concat!(
                    "label = \"Kick\"\n",
                    "transport = \"stdin\"\n",
                    "command_template = \"kick {{{{target}}}}\"\n",
                    "{}",
                    "destructive = true\n",
                ),
                override_action
            )
        } else {
            format!(
                concat!(
                    "label = \"Kick\"\n",
                    "transport = \"stdin\"\n",
                    "{}",
                    "target_required = true\n",
                    "destructive = true\n",
                ),
                override_action
            )
        };

        write_player_list_fixture_with_parts(
            &row_action,
            concat!(
                "source = \"structured_log\"\n",
                "action_id = \"list_online_players\"\n",
                "player_action_ids = [\"kick_userid\"]\n",
                "response_codec = \"dst_client_table_v1\"\n",
                "identity_kind = \"klei_user_id\"\n",
            ),
        )
    }

    fn write_player_list_fixture_with_parts(
        row_action_override: &str,
        player_list: &str,
    ) -> PathBuf {
        let temp_root = create_temp_dir();
        let module_root = temp_root.join("player-list-module");
        fs::create_dir_all(&module_root).expect("create module root");
        let row_action = if row_action_override.is_empty() {
            String::from(concat!(
                "label = \"Kick\"\n",
                "transport = \"stdin\"\n",
                "command_template = \"kick {{target}}\"\n",
                "target_required = true\n",
                "destructive = true\n",
            ))
        } else {
            String::from(row_action_override)
        };
        fs::write(
            module_root.join("module.toml"),
            format!(
                concat!(
                    "id = \"player-list\"\n",
                    "name = \"Player List Module\"\n",
                    "version = \"1.0.0\"\n\n",
                    "[[runtime.player_actions]]\n",
                    "id = \"list_online_players\"\n",
                    "label = \"Refresh\"\n",
                    "transport = \"stdin\"\n",
                    "command_template = \"print {{{{request_id}}}}\"\n\n",
                    "[[runtime.player_actions]]\n",
                    "id = \"list_non_stdin\"\n",
                    "label = \"Refresh non stdin\"\n",
                    "transport = \"telnet\"\n",
                    "command_template = \"print {{{{request_id}}}}\"\n\n",
                    "[[runtime.player_actions]]\n",
                    "id = \"list_without_request\"\n",
                    "label = \"Refresh without request\"\n",
                    "transport = \"stdin\"\n",
                    "command_template = \"print players\"\n\n",
                    "[[runtime.player_actions]]\n",
                    "id = \"list_with_role\"\n",
                    "label = \"Refresh with role\"\n",
                    "transport = \"stdin\"\n",
                    "command_template = \"print {{{{request_id}}}} {{{{role}}}}\"\n\n",
                    "[[runtime.player_actions]]\n",
                    "id = \"list_with_unknown\"\n",
                    "label = \"Refresh with unknown value\"\n",
                    "transport = \"stdin\"\n",
                    "command_template = \"print {{{{request_id}}}} {{{{unknown}}}}\"\n\n",
                    "[[runtime.player_actions]]\n",
                    "id = \"list_destructive\"\n",
                    "label = \"Destructive refresh\"\n",
                    "transport = \"stdin\"\n",
                    "command_template = \"print {{{{request_id}}}}\"\n",
                    "destructive = true\n\n",
                    "[[runtime.player_actions]]\n",
                    "id = \"kick_userid\"\n",
                    "{}\n",
                    "[runtime.player_list]\n{}",
                ),
                row_action, player_list
            ),
        )
        .expect("write manifest");
        temp_root
    }
}
