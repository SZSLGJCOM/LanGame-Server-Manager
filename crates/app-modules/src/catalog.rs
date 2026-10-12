use super::*;
use std::collections::VecDeque;
use std::fs;
use std::sync::{Mutex, OnceLock};

const MAX_CACHED_MANIFESTS: usize = 64;
const MAX_CACHED_SOURCE_BYTES: usize = 1024 * 1024;

struct CachedManifest {
    source: String,
    prototype: ModuleDescriptor,
}

#[derive(Default)]
struct ManifestCache {
    entries: VecDeque<CachedManifest>,
    source_bytes: usize,
}

impl ManifestCache {
    fn get(&self, root: &Path, source: &str) -> Option<ModuleDescriptor> {
        self.entries
            .iter()
            .find(|entry| entry.prototype.root == root && entry.source == source)
            .map(|entry| entry.prototype.clone())
    }

    fn insert(&mut self, source: String, prototype: ModuleDescriptor) {
        if source.len() > MAX_CACHED_SOURCE_BYTES {
            return;
        }
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.prototype.root == prototype.root)
            && let Some(previous) = self.entries.remove(index)
        {
            self.source_bytes -= previous.source.len();
        }
        while self.entries.len() >= MAX_CACHED_MANIFESTS
            || self.source_bytes > MAX_CACHED_SOURCE_BYTES - source.len()
        {
            if let Some(previous) = self.entries.pop_front() {
                self.source_bytes -= previous.source.len();
            }
        }
        self.source_bytes += source.len();
        self.entries.push_back(CachedManifest { source, prototype });
    }
}

static PARSED_MANIFESTS: OnceLock<Mutex<ManifestCache>> = OnceLock::new();

fn cached_manifest(root: &Path, source: &str) -> Option<ModuleDescriptor> {
    // Reuse is optional. A poisoned cache falls back to reading and validating
    // the current files, without changing discovery errors or accepted input.
    let cache = PARSED_MANIFESTS.get_or_init(Default::default).lock().ok()?;
    cache.get(root, source)
}

fn remember_manifest(source: String, descriptor: &ModuleDescriptor) {
    if source.len() > MAX_CACHED_SOURCE_BYTES {
        return;
    }
    let prototype = descriptor.clone();
    if let Ok(mut cache) = PARSED_MANIFESTS.get_or_init(Default::default).lock() {
        cache.insert(source, prototype);
    }
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
        let source = fs::read_to_string(&manifest_path).map_err(|source| {
            ModuleDiscoveryError::ReadManifest {
                path: manifest_path.clone(),
                source,
            }
        })?;
        // Match the exact source, including its BOM. Directory membership and
        // schema contents remain fresh observations on every discovery call.
        if let Some(mut descriptor) = cached_manifest(&module_root, &source) {
            descriptor.schema_json = read_schema(&module_root)?;
            descriptor.root = module_root;
            descriptors.push(descriptor);
            continue;
        }
        let manifest_text = strip_utf8_bom(source.clone());
        let manifest = toml::from_str(&manifest_text).map_err(|source| {
            ModuleDiscoveryError::ParseManifest {
                path: manifest_path,
                source: Box::new(source),
            }
        })?;
        // Preserve syntax errors before schema I/O, and schema I/O errors
        // before semantic validation, as in uncached discovery.
        let schema_json = read_schema(&module_root)?;
        let mut descriptor = descriptor_from_manifest(module_root, manifest_text, manifest)?;
        remember_manifest(source, &descriptor);
        descriptor.schema_json = schema_json;
        descriptors.push(descriptor);
    }
    if descriptors.is_empty() {
        return Err(ModuleDiscoveryError::EmptyCatalog {
            path: root.to_path_buf(),
        });
    }
    descriptors.sort_by(|left, right| left.summary.name.cmp(&right.summary.name));
    Ok(descriptors)
}

fn read_schema(module_root: &Path) -> Result<Option<String>, ModuleDiscoveryError> {
    let path = module_root.join("schema.json");
    if !path.exists() {
        return Ok(None);
    }
    fs::read_to_string(&path)
        .map(strip_utf8_bom)
        .map(Some)
        .map_err(|source| ModuleDiscoveryError::ReadSchema { path, source })
}

fn descriptor_from_manifest(
    module_root: PathBuf,
    manifest_text: String,
    manifest: ModuleToml,
) -> Result<ModuleDescriptor, ModuleDiscoveryError> {
    let manifest_path = module_root.join("module.toml");
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

    let mut runtime = runtime_spec_from_toml(runtime, player_management).map_err(|message| {
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
    validate_program_sharing(&storage, install.as_ref(), process.as_ref(), mods.as_ref()).map_err(
        |message| ModuleDiscoveryError::InvalidStorage {
            path: manifest_path.clone(),
            message,
        },
    )?;
    runtime.program_sharing = match storage.program_sharing {
        ModuleProgramSharing::Shared => app_core::InstanceProgramMode::Shared,
        ModuleProgramSharing::Independent => app_core::InstanceProgramMode::Independent,
    };

    Ok(ModuleDescriptor {
        root: module_root,
        manifest_toml: manifest_text,
        schema_json: None,
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
    })
}

fn strip_utf8_bom(content: String) -> String {
    content
        .strip_prefix('\u{feff}')
        .map(str::to_owned)
        .unwrap_or(content)
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
