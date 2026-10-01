//! The synthetic loopback process is the external firewall test boundary.
//! Only the opt-in fixture service owns this capability; ordinary hosts do not.
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path};

use app_core::{InstanceDetails, ProcessLaunchPlan};
use app_storage::StoragePaths;
use tauri::Manager;

use super::config::{Config, same_path, write_new};

pub(super) struct LoopbackFixture(pub Config);

pub(crate) async fn try_fixture_firewall_boundary(
    app: Option<&tauri::AppHandle>,
    paths: &StoragePaths,
    instance: &InstanceDetails,
    plans: &[ProcessLaunchPlan],
) -> Result<bool, String> {
    let Some(config) = app
        .and_then(|app| app.try_state::<LoopbackFixture>())
        .map(|capability| capability.0.clone())
    else {
        return Ok(false);
    };
    let paths = paths.clone();
    let instance = instance.clone();
    let plans = plans.to_vec();
    tokio::task::spawn_blocking(move || {
        validate_boundary(
            &config,
            &paths,
            &instance,
            &plans,
            &std::env::current_exe().map_err(|e| e.to_string())?,
        )?;
        write_new(
            &config.root.join("data/firewall-boundary.json"),
            &serde_json::json!({
                "mode":"synthetic_loopback_external_boundary", "native_firewall_exercised":false,
                "instance_id":instance.summary.id, "nonce":config.nonce,
            }),
        )?;
        Ok(true)
    })
    .await
    .map_err(|e| format!("Fixture firewall boundary worker failed: {e}"))?
}

fn validate_boundary(
    config: &Config,
    paths: &StoragePaths,
    instance: &InstanceDetails,
    plans: &[ProcessLaunchPlan],
    reference_executable: &Path,
) -> Result<(), String> {
    config.verify()?;
    let owner = config.root.parent().ok_or("Missing fixture owner")?;
    same_path(
        owner.parent().ok_or("Missing fixture TEMP parent")?,
        &std::env::temp_dir(),
    )?;
    if config.root.file_name().and_then(|s| s.to_str()) != Some("fixture")
        || !owner
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.starts_with("langame-runtime-service-"))
    {
        return Err(
            "Synthetic firewall boundary requires the dedicated TEMP fixture namespace".into(),
        );
    }
    for (actual, relative) in [
        (&paths.app_data_root, "localappdata/LanGame/ServerManager"),
        (&paths.instances_root, "runtime/instances"),
        (&paths.games_root, "runtime/games"),
        (&paths.steamcmd_root, "runtime/steamcmd"),
    ] {
        same_path(actual, &config.root.join(relative))?;
    }
    let id = &instance.summary.id;
    if Path::new(id).components().count() != 1
        || !matches!(
            Path::new(id).components().next(),
            Some(Component::Normal(_))
        )
        || instance.summary.module_id != "necesse"
        || instance.summary.bind_ip != "127.0.0.1"
        || instance.ports.len() != 1
        || instance.ports[0].name != "game"
        || instance.ports[0].protocol != "udp"
        || instance.ports[0].port == 0
        || plans.len() != 1
    {
        return Err(
            "Synthetic firewall boundary requires one owned loopback Necesse process and UDP port"
                .into(),
        );
    }
    let root = paths.instances_root.join(id);
    same_path(
        Path::new(&instance.config_file_path),
        &root.join("config/instance.json"),
    )?;
    same_path(Path::new(&instance.saves_path), &root.join("data/saves"))?;
    let process = &plans[0];
    let plan = &process.launch_plan;
    if process.process_key != "main"
        || plan.module_id != "necesse"
        || plan.instance_id != *id
        || !plan.uses_private_runtime
        || plan.uses_script_entrypoint
        || plan.requires_admin
        || !plan.environment.is_empty()
    {
        return Err("Synthetic firewall boundary received an unexpected launch plan".into());
    }
    let arg = |name: &str| -> Result<&str, String> {
        let matches: Vec<_> = plan
            .args
            .iter()
            .enumerate()
            .filter(|(_, item)| item.as_str() == name)
            .collect();
        if matches.len() != 1 {
            return Err(format!("Synthetic launch requires exactly one {name}"));
        }
        plan.args
            .get(matches[0].0 + 1)
            .map(String::as_str)
            .ok_or_else(|| format!("Missing synthetic {name} value"))
    };
    if arg("-ip")? != "127.0.0.1" || arg("-port")? != instance.ports[0].port.to_string() {
        return Err("Synthetic launch does not match its loopback endpoint".into());
    }
    same_path(Path::new(arg("-datadir")?), &root.join("data"))?;
    same_path(Path::new(&plan.working_directory), &root.join("runtime"))?;
    let executable = Path::new(&plan.executable_path);
    same_path(executable, &root.join("runtime/jre/bin/java.exe"))?;
    equal_executables(executable, reference_executable)
}

fn equal_executables(actual: &Path, reference: &Path) -> Result<(), String> {
    let mut actual = File::open(actual).map_err(|e| e.to_string())?;
    let mut reference = File::open(reference).map_err(|e| e.to_string())?;
    let size = actual.metadata().map_err(|e| e.to_string())?.len();
    if size == 0
        || size > 512 * 1024 * 1024
        || size != reference.metadata().map_err(|e| e.to_string())?.len()
    {
        return Err("Synthetic executable identity mismatch".into());
    }
    let mut left = [0; 64 * 1024];
    let mut right = [0; 64 * 1024];
    let mut remaining = size;
    while remaining != 0 {
        let count = remaining.min(left.len() as u64) as usize;
        actual
            .read_exact(&mut left[..count])
            .map_err(|e| e.to_string())?;
        reference
            .read_exact(&mut right[..count])
            .map_err(|e| e.to_string())?;
        if left[..count] != right[..count] {
            return Err("Synthetic executable identity mismatch".into());
        }
        remaining -= count as u64;
    }
    if actual.read(&mut left[..1]).map_err(|e| e.to_string())? != 0
        || reference.read(&mut right[..1]).map_err(|e| e.to_string())? != 0
    {
        return Err("Synthetic executable changed during verification".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "fixture_network_tests.rs"]
mod tests;
