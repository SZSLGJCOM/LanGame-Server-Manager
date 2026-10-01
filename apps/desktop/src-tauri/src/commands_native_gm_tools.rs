use super::*;
use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};

#[path = "commands_native_ark_creatures.rs"]
mod ark_creatures;
#[path = "commands_native_gm_dst.rs"]
mod dst;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ToolCase {
    module_id: String,
    tool_id: String,
    commands: Vec<String>,
    process_key: Option<String>,
    dispatch_options: serde_json::Map<String, Value>,
    #[serde(default)]
    values: BTreeMap<String, String>,
}

pub(super) async fn verify<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    cycle: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if cycle != "initial" || std::env::var("LANGAME_NATIVE_GM_TOOLS").as_deref() != Ok("true") {
        return Ok(());
    }
    let source = PathBuf::from(
        std::env::var_os("LANGAME_NATIVE_GM_COMMANDS_FILE")
            .ok_or("native GM probes require commands exported by the frontend builder")?,
    );
    if fs::metadata(&source)?.len() > 256 * 1024 {
        return Err("native GM command fixture exceeds 256 KiB".into());
    }
    let cases: Vec<ToolCase> = serde_json::from_slice(&fs::read(source)?)?;
    let cases = cases
        .iter()
        .filter(|case| case.module_id == running.summary.module_id)
        .collect::<Vec<_>>();
    if cases.is_empty() || cases.len() > 32 {
        return Err("native GM fixture must contain 1..=32 cases for this module".into());
    }
    require_fixture(runtime, running)?;
    if matches!(
        running.summary.module_id.as_str(),
        "arksurvivalevolved" | "arksurvivalascended"
    ) {
        let port = running
            .ports
            .iter()
            .find(|port| port.name == "rcon" && port.protocol.eq_ignore_ascii_case("tcp"))
            .ok_or("ARK RCON binding is missing")?;
        let targets = crate::commands::commands_runtime_supervision::build_window_inspection_targets_from_instance(running);
        let endpoints = app_platform_win::WindowsPlatform::inspect_process_network_endpoints(
            &targets,
            &[port.port],
        )?;
        if !endpoints.endpoints.iter().any(|endpoint| {
            endpoint.local_port == port.port && endpoint.protocol.eq_ignore_ascii_case("tcp")
        }) {
            return Err("ARK RCON endpoint is not owned by this isolated instance".into());
        }
        println!(
            "NATIVE_GM module={} phase=rcon_endpoint ownership=verified",
            running.summary.module_id
        );
        let mut input = cases[0].dispatch_options.clone();
        input.insert("instanceId".into(), json!(running.summary.id));
        input.insert("command".into(), json!("ListPlayers"));
        let started = Instant::now();
        match crate::commands::send_instance_gm_command(
            runtime.state.clone(),
            serde_json::from_value(Value::Object(input))?,
        )
        .await
        {
            Ok(result) => {
                println!(
                    "NATIVE_GM module={} phase=read_only_rcon_preflight elapsed_ms={} outcome=passed",
                    running.summary.module_id,
                    started.elapsed().as_millis()
                );
                runtime::print_native_failure(
                    "gm_preflight_response",
                    result.response_text.as_deref().unwrap_or(""),
                    &[],
                    runtime.settings,
                    &runtime.package.root,
                );
            }
            Err(error) => {
                report_failure(
                    runtime,
                    running,
                    &format!(
                        "read-only RCON preflight failed after {} ms: {error}",
                        started.elapsed().as_millis()
                    ),
                );
                return Err(
                    "native GM read-only RCON preflight failed before tool dispatch".into(),
                );
            }
        }
    }
    if running.summary.module_id == "dontstarve" {
        dst::verify_bounded_targets(&cases)?;
        dst::prepare(runtime, running).await?;
    }
    let mut failures = Vec::new();
    for case in cases {
        println!(
            "NATIVE_GM module={} tool={} phase=begin",
            case.module_id, case.tool_id
        );
        if let Err(error) = verify_case(runtime, running, case).await {
            report_failure(runtime, running, &error.to_string());
            failures.push(case.tool_id.clone());
            println!(
                "NATIVE_GM module={} tool={} outcome=failed",
                case.module_id, case.tool_id
            );
        }
    }
    if std::env::var("LANGAME_NATIVE_ARK_CREATURES").as_deref() == Ok("true")
        && matches!(
            running.summary.module_id.as_str(),
            "arksurvivalevolved" | "arksurvivalascended"
        )
    {
        ark_creatures::verify(runtime, running).await?;
    }
    if running.summary.module_id == "dontstarve" {
        dst::cleanup(runtime, running).await?;
    }
    require_fixture(runtime, running)?;
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!("native GM verification failed: {}", failures.join(", ")).into())
    }
}

fn report_failure<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    error: &str,
) {
    let mut logs = match diagnostics::failed_run_logs(
        running,
        runtime.effective_install_root,
        &runtime.package.root,
    ) {
        Ok(logs) => logs,
        Err(error) => {
            runtime::print_native_failure(
                "gm_log_read",
                error,
                &[],
                runtime.settings,
                &runtime.package.root,
            );
            Vec::new()
        }
    };
    let native_log = match running.summary.module_id.as_str() {
        "arksurvivalascended" => Some("ark-ascended-server.log"),
        "arksurvivalevolved" => Some("ark-server.log"),
        _ => None,
    };
    if let Some(name) = native_log
        && let Some(root) = Path::new(&running.config_file_path)
            .parent()
            .and_then(Path::parent)
        && let Ok(path) = root.join("logs").join(name).canonicalize()
        && let Ok(fixture) = runtime.package.root.canonicalize()
        && path.starts_with(fixture)
    {
        logs.push(app_storage::read_log_path_snapshot(
            path.to_string_lossy().into_owned(),
            80,
        ));
    }
    runtime::print_native_failure(
        "gm_tool",
        error,
        &logs,
        runtime.settings,
        &runtime.package.root,
    );
}

async fn verify_case<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    case: &ToolCase,
) -> Result<(), Box<dyn std::error::Error>> {
    if case.commands.is_empty() || case.commands.len() > 16 {
        return Err("native GM case must contain 1..=16 frontend commands".into());
    }
    let logs = log_baselines(running)?;
    let saves_before = save_metadata(Path::new(&running.saves_path))?;
    let dst_before = if case.module_id == "dontstarve" {
        Some(dst::before(runtime, running, case).await?)
    } else {
        None
    };
    let mut responses = Vec::new();
    for command in &case.commands {
        let mut input = case.dispatch_options.clone();
        input.insert("instanceId".into(), json!(running.summary.id));
        input.insert("command".into(), json!(command));
        input.insert("processKey".into(), json!(case.process_key));
        let input = serde_json::from_value(Value::Object(input))?;
        let result = if case.dispatch_options.contains_key("runtimeActionId") {
            crate::commands::send_instance_runtime_command(runtime.state.clone(), input).await?
        } else {
            crate::commands::send_instance_gm_command(runtime.state.clone(), input).await?
        };
        if result.instance_id != running.summary.id {
            return Err("native GM dispatcher returned a different instance".into());
        }
        if let Some(response) = result.response_text {
            runtime::print_native_failure(
                "gm_response",
                &response,
                &[],
                runtime.settings,
                &runtime.package.root,
            );
            responses.push(response);
        }
    }
    let response = responses.join("\n");
    if let Some(expected) = case.values.get("expectedError") {
        if case.module_id != "dontstarve" {
            return Err("native GM negative response verifier only supports DST".into());
        }
        let suffix = format!(": {expected}");
        wait_new_log(&logs, |text| {
            text.lines().any(|line| line.trim_end().ends_with(&suffix))
        })
        .await?;
        dst::verify_rejected_effect(runtime, running, case, dst_before.as_deref().unwrap_or(""))
            .await?;
        println!(
            "NATIVE_GM module={} tool={} outcome=passed evidence=native_lua_rejection_inventory_and_entities_preserved expected_error={expected}",
            case.module_id, case.tool_id
        );
        return Ok(());
    }
    let evidence = match case.tool_id.as_str() {
        "dst_give_item_to_player" | "dst_set_season" | "dst_set_rain" | "dst_revive_player" => {
            dst::after(runtime, running, case, dst_before.as_deref().unwrap_or("")).await?;
            "native_world_state"
        }
        "terraria_save_world" => {
            // Terraria 1.4.5.8's English native resource is LegacyWorldGen.49:
            // "Saving world data:". It does not promise a "World saved" line.
            wait_new_log(&logs, |text| text.contains("Saving world data:")).await?;
            wait_terraria_save(Path::new(&running.saves_path), &saves_before).await?;
            "native_save_log_updated_world_file_and_writer_closed"
        }
        "terraria_broadcast" => {
            let message = case
                .values
                .get("message")
                .ok_or("broadcast probe has no source message")?;
            wait_new_log(&logs, |text| {
                text.lines()
                    .any(|line| line.contains(message) && !line.trim_start().starts_with("say "))
            })
            .await?;
            "native_console_broadcast"
        }
        "minecraft_save_world" => {
            if !response.to_ascii_lowercase().contains("saved") {
                return Err("Minecraft did not acknowledge completed save".into());
            }
            wait_save_change(Path::new(&running.saves_path), &saves_before).await?;
            "native_response_and_persisted_save"
        }
        "zomboid_save_world" => {
            if !response.to_ascii_lowercase().contains("saved") {
                return Err("Project Zomboid did not acknowledge completed save".into());
            }
            wait_save_change(Path::new(&running.saves_path), &saves_before).await?;
            "native_response_and_persisted_save"
        }
        "palworld_save_world" => {
            if !response.contains("200") && !response.to_ascii_lowercase().contains("save") {
                return Err("Palworld did not acknowledge save API success".into());
            }
            if saves_before.is_empty() {
                println!(
                    "NATIVE_GM module=palworld tool=palworld_save_world persistence=unavailable_without_joined_client"
                );
            } else {
                wait_save_change(Path::new(&running.saves_path), &saves_before).await?;
            }
            "native_rest_response"
        }
        "palworld_broadcast" => {
            if !response.contains("200") && !response.to_ascii_lowercase().contains("announce") {
                return Err("Palworld did not acknowledge announce API success".into());
            }
            "native_rest_response_no_client_delivery_assertion"
        }
        "ark_set_time" | "ark_destroy_wild_dinos" => {
            if response.trim().is_empty()
                || ["error", "unknown", "not recognized"]
                    .iter()
                    .any(|marker| response.to_ascii_lowercase().contains(marker))
            {
                return Err("ARK world command lacks a successful native response".into());
            }
            "native_rcon_response_no_client_world_observation"
        }
        _ => return Err("this native GM tool still needs an independent effect verifier".into()),
    };
    println!(
        "NATIVE_GM module={} tool={} outcome=passed evidence={evidence}",
        case.module_id, case.tool_id
    );
    Ok(())
}

fn require_fixture<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = runtime.package.root.canonicalize()?;
    for path in [
        runtime.effective_install_root,
        Path::new(&running.config_file_path),
    ] {
        if !path.canonicalize()?.starts_with(&root) {
            return Err("native GM probe target is outside the owned fixture".into());
        }
    }
    let targets = crate::commands::commands_runtime_supervision::build_window_inspection_targets_from_instance(running);
    if targets.is_empty() {
        return Err("native GM fixture has no owned processes".into());
    }
    for target in targets {
        if !Path::new(&target.process_identity.image_path)
            .canonicalize()?
            .starts_with(&root)
            || app_runtime::inspect_process_identity(target.pid)?.as_ref()
                != Some(&target.process_identity)
        {
            return Err("native GM fixture process ownership changed".into());
        }
    }
    Ok(())
}

type LogBaselines = Vec<(PathBuf, u64)>;

fn log_baselines(running: &InstanceDetails) -> Result<LogBaselines, Box<dyn std::error::Error>> {
    running
        .active_run
        .as_ref()
        .ok_or("native GM fixture has no active run")?
        .processes
        .iter()
        .filter_map(|process| process.log_path.as_ref())
        .map(|path| Ok((PathBuf::from(path), fs::metadata(path)?.len())))
        .collect()
}

fn new_logs(logs: &LogBaselines) -> Result<String, Box<dyn std::error::Error>> {
    let mut result = String::new();
    for (path, offset) in logs {
        let mut file = fs::File::open(path)?;
        let length = file.metadata()?.len();
        if length < *offset || length - offset > 1024 * 1024 {
            return Err("native GM log rolled over or exceeded capture limit".into());
        }
        file.seek(SeekFrom::Start(*offset))?;
        let mut bytes = Vec::new();
        file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
        result.push_str(&String::from_utf8_lossy(&bytes));
    }
    Ok(result)
}

async fn wait_new_log(
    logs: &LogBaselines,
    predicate: impl Fn(&str) -> bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let text = new_logs(logs)?;
        if predicate(&text) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            let mut lines = text.lines().rev().take(60).collect::<Vec<_>>();
            lines.reverse();
            // The owning caller routes this error through print_native_failure,
            // which strips credentials, runtime identities and fixture paths.
            return Err(format!(
                "native GM effect not present in fresh logs; captured tail:\n{}",
                lines.join("\n")
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

type SaveMetadata = BTreeMap<PathBuf, (u64, std::time::SystemTime)>;

async fn wait_terraria_save(
    root: &Path,
    before: &SaveMetadata,
) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let after = save_metadata(root)?;
        for (path, state) in &after {
            if !path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("wld"))
                || state.0 == 0
                || before.get(path) == Some(state)
            {
                continue;
            }
            let mut options = fs::OpenOptions::new();
            options.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                // A changed backup or an in-progress world writer is not
                // evidence that the primary world finished saving.
                options.share_mode(0);
            }
            if let Ok(file) = options.open(path)
                && file.metadata()?.len() == state.0
            {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err(
                "Terraria did not finish updating and closing its primary .wld save".into(),
            );
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn save_metadata(root: &Path) -> Result<SaveMetadata, Box<dyn std::error::Error>> {
    let mut result = BTreeMap::new();
    let mut pending = if root.exists() {
        vec![root.to_owned()]
    } else {
        Vec::new()
    };
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err("native GM save contains link".into());
        }
        if metadata.is_dir() {
            for entry in fs::read_dir(path)? {
                pending.push(entry?.path());
            }
        } else if metadata.is_file() {
            result.insert(path, (metadata.len(), metadata.modified()?));
        }
        if pending.len() + result.len() > 100_000 {
            return Err("native GM save inventory too large".into());
        }
    }
    Ok(result)
}

async fn wait_save_change(
    root: &Path,
    before: &SaveMetadata,
) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let after = save_metadata(root)?;
        if !after.is_empty() && &after != before {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("native GM save did not update persisted files".into());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}
