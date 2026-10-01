use super::*;
use crate::atomic_file::read_optional_file_to_string;
use app_modules::{ModuleDescriptor, discover_modules};

pub(crate) fn load_module_descriptor(
    paths: &StoragePaths,
    module_id: &str,
) -> Result<Option<ModuleDescriptor>, StorageError> {
    Ok(discover_modules(&paths.modules_root)?
        .into_iter()
        .find(|descriptor| descriptor.summary.id.eq_ignore_ascii_case(module_id)))
}

pub(crate) fn module_declares_saves_path(descriptor: Option<&ModuleDescriptor>) -> bool {
    descriptor
        .and_then(|descriptor| descriptor.storage.saves_path_template.as_deref())
        .is_some()
}

pub struct InstanceSavePathContext<'a> {
    pub install_root: &'a Path,
    pub instance_root: &'a Path,
    pub config_dir: &'a Path,
    pub instance_id: &'a str,
    pub instance_name: &'a str,
    pub module_id: &'a str,
    pub settings: Option<&'a Map<String, Value>>,
}

pub fn plan_instance_saves_dir(
    descriptor: Option<&ModuleDescriptor>,
    context: &InstanceSavePathContext<'_>,
) -> Result<PathBuf, StorageError> {
    planned_instance_saves_dir(descriptor, context)
}

pub(crate) fn planned_instance_saves_dir(
    descriptor: Option<&ModuleDescriptor>,
    context: &InstanceSavePathContext<'_>,
) -> Result<PathBuf, StorageError> {
    if let Some(resolved) = resolve_declared_instance_saves_dir(descriptor, context)? {
        return Ok(resolved);
    }

    Ok(context.instance_root.join("saves"))
}

pub(crate) fn materialized_instance_saves_dir(
    descriptor: Option<&ModuleDescriptor>,
    context: &InstanceSavePathContext<'_>,
) -> Result<PathBuf, StorageError> {
    let saves_dir = planned_instance_saves_dir(descriptor, context)?;

    if !app_core::ark_maps::is_ark(context.module_id)
        && declared_saves_path_points_to_dynamic_leaf(descriptor)
    {
        return Ok(saves_dir
            .parent()
            .unwrap_or(saves_dir.as_path())
            .to_path_buf());
    }

    Ok(saves_dir)
}

pub(crate) fn effective_instance_saves_dir(
    descriptor: Option<&ModuleDescriptor>,
    install_root: &Path,
    record: &StoredInstanceRecord,
) -> Result<PathBuf, StorageError> {
    let instance_root = record
        .config_dir
        .parent()
        .unwrap_or(record.config_dir.as_path());

    let settings = load_instance_settings_map(&record.config_dir.join("instance.json"))?;

    let context = InstanceSavePathContext {
        install_root,
        instance_root,
        config_dir: &record.config_dir,
        instance_id: &record.summary.id,
        instance_name: &record.summary.name,
        module_id: &record.summary.module_id,
        settings: Some(&settings),
    };
    if let Some(resolved) = resolve_declared_instance_saves_dir(descriptor, &context)? {
        return Ok(resolved);
    }

    // An unavailable module cannot establish a new save location. Keep the
    // recorded path so backup and recovery still target the operator's data.
    Ok(record.saves_dir.clone())
}

fn resolve_declared_instance_saves_dir(
    descriptor: Option<&ModuleDescriptor>,
    context: &InstanceSavePathContext<'_>,
) -> Result<Option<PathBuf>, StorageError> {
    let Some(template) =
        descriptor.and_then(|descriptor| descriptor.storage.saves_path_template.as_deref())
    else {
        return Ok(None);
    };

    let resolved = resolve_saves_path_template(template, context);
    let trimmed = resolved.trim();
    if trimmed.is_empty() || trimmed.contains("{{") || trimmed.contains("}}") {
        return Err(StorageError::InvalidModuleSavePathTemplate {
            module_id: String::from(context.module_id),
            template: String::from(template),
        });
    }

    // Every ARK map writes beside the main world's directory. Keep the whole
    // native Saved tree in backups even after a map is paused or removed.
    let path = if app_core::ark_maps::is_ark(context.module_id) {
        context.install_root.join("ShooterGame/Saved")
    } else {
        PathBuf::from(normalize_local_path_template(trimmed))
    };
    if descriptor.is_some_and(|descriptor| {
        descriptor.storage.program_sharing == app_modules::ModuleProgramSharing::Shared
    }) {
        crate::program_runtime::ensure_private_data_path(context.instance_root, &path)?;
    }
    Ok(Some(path))
}

fn declared_saves_path_points_to_dynamic_leaf(descriptor: Option<&ModuleDescriptor>) -> bool {
    let Some(template) =
        descriptor.and_then(|descriptor| descriptor.storage.saves_path_template.as_deref())
    else {
        return false;
    };

    let trimmed = template.trim_end_matches(['/', '\\']);
    let Some(last_segment) = trimmed.rsplit(['/', '\\']).next() else {
        return false;
    };

    last_segment.contains("{{") && last_segment.contains("}}")
}

fn normalize_local_path_template(template: &str) -> String {
    if cfg!(windows) {
        template.replace('/', "\\")
    } else {
        template.replace('\\', "/")
    }
}

fn resolve_saves_path_template(template: &str, context: &InstanceSavePathContext<'_>) -> String {
    let data_dir = context.instance_root.join("data");
    let logs_dir = context.instance_root.join("logs");
    let mut resolved = String::new();
    let mut cursor = template;

    while let Some(open_index) = cursor.find("{{") {
        resolved.push_str(&cursor[..open_index]);
        let token_start = open_index + 2;

        if let Some(close_offset) = cursor[token_start..].find("}}") {
            let close_index = token_start + close_offset;
            let token = cursor[token_start..close_index].trim();
            let replacement = resolve_saves_path_token(token, context, &data_dir, &logs_dir)
                .unwrap_or_else(|| format!("{{{{{token}}}}}"));
            resolved.push_str(&replacement);
            cursor = &cursor[close_index + 2..];
        } else {
            resolved.push_str(&cursor[open_index..]);
            return resolved;
        }
    }

    resolved.push_str(cursor);
    resolved
}

fn resolve_saves_path_token(
    token: &str,
    context: &InstanceSavePathContext<'_>,
    data_dir: &Path,
    logs_dir: &Path,
) -> Option<String> {
    match token {
        "instance_id" | "instance.id" => Some(String::from(context.instance_id)),
        "instance_name" | "instance.name" => Some(String::from(context.instance_name)),
        "module_id" | "module.id" | "instance.module_id" => Some(String::from(context.module_id)),
        "paths.install_root" => Some(context.install_root.to_string_lossy().into_owned()),
        "paths.instance_root" => Some(context.instance_root.to_string_lossy().into_owned()),
        "paths.config_dir" => Some(context.config_dir.to_string_lossy().into_owned()),
        "paths.data_dir" => Some(data_dir.to_string_lossy().into_owned()),
        "paths.logs_dir" => Some(logs_dir.to_string_lossy().into_owned()),
        _ => {
            if let Some(path) = token.strip_prefix("settings.") {
                lookup_settings_path(context.settings, path)
            } else {
                None
            }
        }
    }
}

fn load_instance_settings_map(config_file_path: &Path) -> Result<Map<String, Value>, StorageError> {
    let Some(content) = read_optional_file_to_string(config_file_path).map_err(|source| {
        StorageError::ReadConfig {
            path: config_file_path.to_path_buf(),
            source,
        }
    })?
    else {
        return Ok(Map::new());
    };
    let document: Value = serde_json::from_str(&content)?;
    let settings = document
        .get("settings")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    Ok(settings)
}

fn lookup_settings_path(settings: Option<&Map<String, Value>>, path: &str) -> Option<String> {
    let mut current = settings?.get(path.split('.').next()?)?;

    for segment in path.split('.').skip(1) {
        current = current.get(segment)?;
    }

    match current {
        Value::Null => Some(String::new()),
        Value::Bool(boolean) => Some(boolean.to_string()),
        Value::Number(number) => Some(number.to_string()),
        Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_core::{InstallSpec, InstallState, ModuleSummary};

    #[test]
    fn missing_module_preserves_the_recorded_dontstarve_save_directory() {
        let root = std::env::temp_dir().join(format!("lgsm-save-path-{}", Uuid::new_v4()));
        let config_dir = root.join("config");
        let saves_dir = root.join("operator-selected-cluster");
        let record = StoredInstanceRecord {
            summary: app_core::InstanceSummary {
                id: String::from("dst-save-recovery"),
                name: String::from("DST save recovery"),
                module_id: String::from("dontstarve"),
                status: app_core::InstanceStatus::Stopped,
                active_process_count: 0,
                bind_ip: String::from("0.0.0.0"),
                port_count: 0,
                autostart: false,
            },
            config_dir,
            saves_dir: saves_dir.clone(),
            runtime_mode: String::from("independent"),
            program_install_root: Some(root.join("runtime")),
            auto_backup_on_stop: false,
            backup_retention_count: 10,
        };

        let effective = effective_instance_saves_dir(None, &root.join("game"), &record)
            .expect("recover the stored save path without a module");

        assert_eq!(effective, saves_dir);
    }

    fn make_descriptor(template: &str) -> ModuleDescriptor {
        ModuleDescriptor {
            root: PathBuf::from("modules/abioticfactor"),
            manifest_toml: String::new(),
            schema_json: None,
            default_ports: Vec::new(),
            install: Some(InstallSpec {
                shared_game_dir: String::from("abioticfactor"),
                download_url_windows: None,
                download_integrity_windows: None,
                source: None,
                verification_path: None,
                minecraft: None,
            }),
            process: None,
            workshop: None,
            runtime: app_core::ModuleRuntimeSpec::default(),
            storage: app_modules::ModuleStorageSpec {
                saves_path_template: Some(String::from(template)),
                ..Default::default()
            },
            summary: ModuleSummary {
                id: String::from("abioticfactor"),
                name: String::from("Abiotic Factor Dedicated Server"),
                version: String::from("0.1.0"),
                description: None,
                steam_app_id: Some(2857200),
                install_state: InstallState::NotInstalled,
                instance_program_count: 0,
                archived_program_count: 0,
                supported_platforms: vec![String::from("windows")],
            },
        }
    }

    #[test]
    fn planned_instance_saves_dir_supports_settings_tokens() {
        let descriptor = make_descriptor(
            "{{paths.install_root}}/AbioticFactor/Saved/SaveGames/Server/Worlds/{{settings.world_save_name}}",
        );
        let mut settings = Map::new();
        settings.insert(
            String::from("world_save_name"),
            Value::String(String::from("science-alpha")),
        );

        let context = InstanceSavePathContext {
            install_root: Path::new("D:/LanGame/server-files/abioticfactor"),
            instance_root: Path::new("D:/LanGame/instances/abiotic-alpha"),
            config_dir: Path::new("D:/LanGame/instances/abiotic-alpha/config"),
            instance_id: "abiotic-alpha",
            instance_name: "Abiotic Alpha",
            module_id: "abioticfactor",
            settings: Some(&settings),
        };
        let saves_dir =
            planned_instance_saves_dir(Some(&descriptor), &context).expect("resolve saves dir");

        assert!(
            saves_dir
                .to_string_lossy()
                .ends_with("AbioticFactor\\Saved\\SaveGames\\Server\\Worlds\\science-alpha")
        );
    }

    #[test]
    fn materialized_instance_saves_dir_uses_parent_for_dynamic_leaf_templates() {
        let descriptor = make_descriptor(
            "{{paths.install_root}}/AbioticFactor/Saved/SaveGames/Server/Worlds/{{settings.world_save_name}}",
        );
        let mut settings = Map::new();
        settings.insert(
            String::from("world_save_name"),
            Value::String(String::from("science-alpha")),
        );

        let context = InstanceSavePathContext {
            install_root: Path::new("D:/LanGame/server-files/abioticfactor"),
            instance_root: Path::new("D:/LanGame/instances/abiotic-alpha"),
            config_dir: Path::new("D:/LanGame/instances/abiotic-alpha/config"),
            instance_id: "abiotic-alpha",
            instance_name: "Abiotic Alpha",
            module_id: "abioticfactor",
            settings: Some(&settings),
        };
        let materialized_dir = materialized_instance_saves_dir(Some(&descriptor), &context)
            .expect("resolve materialized saves dir");

        assert!(
            materialized_dir
                .to_string_lossy()
                .ends_with("AbioticFactor\\Saved\\SaveGames\\Server\\Worlds")
        );
    }

    #[test]
    fn materialized_instance_saves_dir_keeps_stable_root_templates() {
        let descriptor = make_descriptor("{{paths.instance_root}}/Saves");

        let context = InstanceSavePathContext {
            install_root: Path::new("D:/LanGame/server-files/abioticfactor"),
            instance_root: Path::new("D:/LanGame/instances/abiotic-alpha"),
            config_dir: Path::new("D:/LanGame/instances/abiotic-alpha/config"),
            instance_id: "abiotic-alpha",
            instance_name: "Abiotic Alpha",
            module_id: "abioticfactor",
            settings: None,
        };
        let materialized_dir = materialized_instance_saves_dir(Some(&descriptor), &context)
            .expect("resolve stable root saves dir");

        assert!(
            materialized_dir
                .to_string_lossy()
                .ends_with("abiotic-alpha\\Saves")
        );
    }

    #[test]
    fn ark_save_scope_keeps_the_full_native_tree_without_reparenting_materialization() {
        let descriptor =
            make_descriptor("{{paths.install_root}}/ShooterGame/Saved/{{instance.id}}");
        for module_id in ["arksurvivalevolved", "arksurvivalascended"] {
            let context = InstanceSavePathContext {
                install_root: Path::new("D:/owned/ark/runtime"),
                instance_root: Path::new("D:/owned/ark"),
                config_dir: Path::new("D:/owned/ark/config"),
                instance_id: "main-world",
                instance_name: "ARK cluster",
                module_id,
                settings: None,
            };
            let saved = context.install_root.join("ShooterGame/Saved");
            assert_eq!(
                planned_instance_saves_dir(Some(&descriptor), &context).unwrap(),
                saved
            );
            assert_eq!(
                materialized_instance_saves_dir(Some(&descriptor), &context).unwrap(),
                saved
            );
        }
    }
}
