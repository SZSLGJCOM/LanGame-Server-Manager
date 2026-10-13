//! ASA map choices come from installed author metadata, never package titles.
use super::*;
use std::collections::BTreeMap;
use std::time::Instant;

const MAX_METADATA_BYTES: u64 = 64 * 1024;
const MAX_ENTRY_MAP_BYTES: u64 = 4 * 1024;
const MAX_SCAN_ENTRIES: usize = 50_000;
const MAX_MAPS: usize = 8192;
const MAX_TOTAL_METADATA_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct InstalledArkMap {
    pub map_name: String,
    pub mod_id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstanceArkMapAvailability {
    pub instance_id: String,
    pub module_id: String,
    pub maps: Vec<InstalledArkMap>,
}

#[tauri::command]
pub async fn read_instance_ark_map_availability(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<InstanceArkMapAvailability, String> {
    let operation = state.begin_storage_context_operation("installed map inspection")?;
    let storage = state.bootstrap_storage()?;
    let target =
        super::commands_mods::resolve_manual_mod_target(&storage, instance_id, false).await?;
    if target.module_id != "arksurvivalascended" {
        return Err("Installed map discovery is supported for ARK: Survival Ascended.".into());
    }
    spawn_blocking_storage_context_task(&operation, move || {
        Ok(InstanceArkMapAvailability {
            instance_id: target.instance_id,
            module_id: target.module_id,
            maps: discover_installed_maps(&target.target_path)?,
        })
    })
    .await
    .map_err(|error| format!("Installed map inspection worker failed: {error}"))?
}

/// A plain missing map without any Mod source is actionable before spawning
/// the group. Configured Mods retain native first-start download behavior;
/// custom package paths remain native-controlled rather than being guessed.
pub(super) async fn ensure_asa_map_content(
    instance: &InstanceDetails,
    module: &ModuleDetails,
    launch_plans: &[ProcessLaunchPlan],
) -> Result<(), String> {
    if module.summary.id != "arksurvivalascended" {
        return Ok(());
    }
    let Some(schema_json) = module.schema_json.clone() else {
        return Ok(());
    };
    let maps = launch_plans
        .iter()
        .filter_map(|plan| plan.launch_plan.args.first())
        .map(|argument| argument.split('?').next().unwrap_or(argument).to_owned())
        .collect::<Vec<_>>();
    let allows_native_content = launch_plans
        .iter()
        .any(|plan| has_mod_content_source(&plan.launch_plan.args));
    let Some(primary) = launch_plans.first() else {
        return Ok(());
    };
    let Some(staging) = module
        .mods
        .as_ref()
        .and_then(|mods| mods.manual_staging.as_ref())
    else {
        return Ok(());
    };
    let config_dir = Path::new(&instance.config_file_path)
        .parent()
        .ok_or("Instance configuration has no directory.")?;
    let instance_root = config_dir
        .parent()
        .ok_or("Instance configuration has no instance root.")?;
    let install_root = Path::new(&primary.launch_plan.install_root);
    let settings = serde_json::from_str::<Value>(&instance.settings_json)
        .map_err(|error| error.to_string())?;
    let root = super::commands_mods::resolve_manual_mod_target_template(
        &staging.target_template,
        &super::commands_mods::ManualModTargetContext {
            install_root,
            instance_root,
            config_dir,
            data_dir: &instance_root.join("data"),
            logs_dir: &instance_root.join("logs"),
            saves_dir: Path::new(&instance.saves_path),
            instance,
            settings_json: &settings,
        },
    )?;
    tokio::task::spawn_blocking(move || {
        check_map_content(&schema_json, &maps, &root, allows_native_content)
    })
    .await
    .map_err(|error| format!("Map content inspection worker failed: {error}"))?
}

fn has_mod_content_source(args: &[String]) -> bool {
    let mut args = args.iter().peekable();
    while let Some(argument) = args.next() {
        let (key, value) = argument.split_once('=').map_or_else(
            || (argument.as_str(), args.peek().map(|value| value.as_str())),
            |(key, value)| (key, Some(value)),
        );
        if ["-mods", "-passivemods", "-MapModID"]
            .iter()
            .any(|flag| key.eq_ignore_ascii_case(flag))
            && value.is_some_and(|value| !value.trim().is_empty() && !value.starts_with('-'))
        {
            return true;
        }
    }
    false
}

fn check_map_content(
    schema_json: &str,
    maps: &[String],
    root: &Path,
    allows_native_content: bool,
) -> Result<(), String> {
    if allows_native_content {
        return Ok(());
    }
    let schema: Value = serde_json::from_str(schema_json).map_err(|error| error.to_string())?;
    let Some(suggestions) = schema
        .pointer("/properties/map_name/x-lsgm-suggestions")
        .and_then(Value::as_array)
    else {
        return Ok(());
    };
    let official = suggestions
        .iter()
        .filter_map(|entry| entry.get("value").and_then(Value::as_str))
        .collect::<Vec<_>>();
    if official.is_empty() {
        return Ok(());
    }
    let missing = maps
        .iter()
        .filter(|map| {
            !map.is_empty()
                && map.len() <= 128
                && map
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                && !official.iter().any(|known| known.eq_ignore_ascii_case(map))
        })
        .collect::<Vec<_>>();
    let Some(map) = missing.first() else {
        return Ok(());
    };
    let installed = discover_installed_maps(root)?;
    if let Some(known) = installed
        .iter()
        .find(|known| known.map_name.eq_ignore_ascii_case(map))
    {
        Err(format!(
            "Map `{map}` is installed from Mod {}, but that Mod is not enabled in the native launch arguments. Enable its Mod ID in Mods before starting. Existing map settings and worlds were preserved.",
            known.mod_id
        ))
    } else {
        Err(format!(
            "Map `{map}` is not in the official map catalog and no installed map package declares this entry. Install its map Mod and add the author's Mod ID in Mods before starting, or select an official map. Existing map settings and worlds were preserved."
        ))
    }
}

struct ScanBudget {
    entries: usize,
    metadata_bytes: u64,
    deadline: Instant,
}

impl ScanBudget {
    fn check(&mut self) -> Result<(), String> {
        self.entries = self
            .entries
            .checked_sub(1)
            .ok_or("Installed map inspection exceeded its directory entry budget.")?;
        if Instant::now() >= self.deadline {
            return Err("Installed map inspection exceeded its time budget.".into());
        }
        Ok(())
    }
}

pub(super) fn discover_installed_maps(root: &Path) -> Result<Vec<InstalledArkMap>, String> {
    discover_with_budget(
        root,
        ScanBudget {
            entries: MAX_SCAN_ENTRIES,
            metadata_bytes: MAX_TOTAL_METADATA_BYTES,
            deadline: Instant::now() + Duration::from_secs(10),
        },
    )
}

fn discover_with_budget(
    root: &Path,
    mut budget: ScanBudget,
) -> Result<Vec<InstalledArkMap>, String> {
    super::commands_mods::validate_mod_inventory_root_chain(root)?;
    if !root.try_exists().map_err(|error| error.to_string())? {
        return Ok(Vec::new());
    }
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut maps = BTreeMap::<String, InstalledArkMap>::new();
    while let Some((directory, depth)) = pending.pop() {
        budget.check()?;
        if let Some(map) = inspect_plugin(&directory, &mut budget)? {
            if let Some(existing) = maps.get(&map.map_name) {
                if existing.mod_id != map.mod_id {
                    return Err(format!(
                        "Installed map `{}` is declared by multiple Mods ({}, {}); resolve the duplicate map packages before selecting it.",
                        map.map_name, existing.mod_id, map.mod_id
                    ));
                }
            } else {
                maps.insert(map.map_name.clone(), map);
            }
            if maps.len() > MAX_MAPS {
                return Err(
                    "Installed map inspection exceeded the ASA Mod membership limit.".into(),
                );
            }
            continue;
        }
        // Native SDK packages wrap one plugin in <modId>_<fileId>; manual
        // archives may have another outer directory. Never walk cooked assets.
        if depth >= 3 {
            continue;
        }
        let entries = fs::read_dir(&directory).map_err(|error| {
            format!(
                "Read installed Mod directory {}: {error}",
                directory.display()
            )
        })?;
        for entry in entries {
            budget.check()?;
            let entry = entry.map_err(|error| error.to_string())?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.')
                || name.eq_ignore_ascii_case("content")
                || name.eq_ignore_ascii_case("preview")
            {
                continue;
            }
            super::commands_mods::validate_mod_inventory_root_chain(&entry.path())?;
            if entry
                .file_type()
                .map_err(|error| error.to_string())?
                .is_dir()
            {
                pending.push((entry.path(), depth + 1));
            }
        }
    }
    Ok(maps.into_values().collect())
}

fn bounded_text(
    path: &Path,
    limit: u64,
    budget: &mut ScanBudget,
) -> Result<Option<String>, String> {
    budget.check()?;
    super::commands_mods::validate_mod_inventory_root_chain(path)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Inspect installed map metadata {}: {error}",
                path.display()
            ));
        }
    };
    if !metadata.is_file() || metadata.len() > limit {
        return Ok(None);
    }
    if metadata.len() > budget.metadata_bytes {
        return Err("Installed map inspection exceeded its metadata byte budget.".into());
    }
    let mut text = String::new();
    fs::File::open(path)
        .and_then(|file| file.take(limit + 1).read_to_string(&mut text))
        .map_err(|error| format!("Read installed map metadata {}: {error}", path.display()))?;
    budget.metadata_bytes = budget
        .metadata_bytes
        .checked_sub(text.len() as u64)
        .ok_or("Installed map inspection exceeded its metadata byte budget.")?;
    if text.len() as u64 > limit {
        return Ok(None);
    }
    Ok(Some(text.trim_start_matches('\u{feff}').to_owned()))
}

fn inspect_plugin(
    directory: &Path,
    budget: &mut ScanBudget,
) -> Result<Option<InstalledArkMap>, String> {
    // Wildcard's UGC Settings contract publishes the author's UMAP basename in
    // Preview/entry_map.txt. Verified with public WindowsServer file 6581527.
    let Some(entry) = bounded_text(
        &directory.join("Preview/entry_map.txt"),
        MAX_ENTRY_MAP_BYTES,
        budget,
    )?
    else {
        return Ok(None);
    };
    let map_name = entry.trim();
    if map_name.is_empty()
        || map_name.len() > 128
        || !map_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Ok(None);
    }
    let mut descriptor = None;
    let mut descriptor_count = 0;
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        budget.check()?;
        let path = entry.map_err(|error| error.to_string())?.path();
        if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("uplugin"))
        {
            descriptor_count += 1;
            if descriptor_count > 1 {
                return Ok(None);
            }
            descriptor = bounded_text(&path, MAX_METADATA_BYTES, budget)?;
        }
    }
    let Some(descriptor) = descriptor.and_then(|text| serde_json::from_str::<Value>(&text).ok())
    else {
        return Ok(None);
    };
    if descriptor.get("CanContainContent").and_then(Value::as_bool) != Some(true)
        || descriptor.get("Category").and_then(Value::as_str) != Some("UGC")
    {
        return Ok(None);
    }
    let Some(mod_id) = descriptor
        .get("Description")
        .and_then(Value::as_str)
        .and_then(ugc_id)
    else {
        return Ok(None);
    };
    let payload = directory.join("Content/Paks/WindowsServer");
    super::commands_mods::validate_mod_inventory_root_chain(&payload)?;
    if !payload.try_exists().map_err(|error| error.to_string())? {
        return Ok(None);
    }
    let mut packages = BTreeMap::<String, u8>::new();
    for entry in fs::read_dir(&payload).map_err(|error| error.to_string())? {
        budget.check()?;
        let path = entry.map_err(|error| error.to_string())?.path();
        let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        let kind = match extension.to_ascii_lowercase().as_str() {
            "pak" => 1,
            "utoc" => 2,
            "ucas" => 4,
            _ => continue,
        };
        super::commands_mods::validate_mod_inventory_root_chain(&path)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.is_file() && metadata.len() > 0 {
            let stem = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            *packages.entry(stem).or_default() |= kind;
        }
    }
    if !packages.values().any(|kind| *kind == 1 || *kind == 7) {
        return Ok(None);
    }
    // Installed=false is an Unreal descriptor flag, not CF installation state:
    // the real 928708 package retains it after successful native installation.
    let label = descriptor
        .get("FriendlyName")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(map_name)
        .chars()
        .filter(|character| !character.is_control())
        .take(128)
        .collect::<String>();
    let label = if label.trim().is_empty() {
        map_name.to_owned()
    } else {
        label.trim().to_owned()
    };
    Ok(Some(InstalledArkMap {
        map_name: map_name.into(),
        mod_id,
        label,
    }))
}

fn ugc_id(description: &str) -> Option<String> {
    let (_, value) = description.split_once("cf_ugcID=")?;
    let digits = value
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    digits
        .parse::<u64>()
        .ok()
        .filter(|id| *id > 0)
        .map(|id| id.to_string())
}

#[cfg(test)]
#[path = "commands_ark_map_availability_tests.rs"]
mod tests;
