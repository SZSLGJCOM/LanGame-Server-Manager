//! Read explicit Rust options back through the production, generation-bound RCON path.
use super::{InstanceDetails, NativeRuntime, Path, Value};
use crate::commands::commands_assistant_ops::send_assistant_runtime_command;

const CONTROLS: [(&str, &str); 4] = [
    ("network_position_lod", "net.position_lod"),
    ("network_position_lod_players", "net.position_lod_players"),
    ("batch_packets", "server.batchpackets"),
    ("player_update_jobs", "server.useplayerupdatejobs"),
];

pub(super) async fn verify<R: tauri::Runtime>(
    runtime: &NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    cycle: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if running.summary.module_id != "rust"
        || !CONTROLS
            .iter()
            .any(|(key, _)| runtime.settings.contains_key(*key))
    {
        return Ok(());
    }
    require_owned_endpoint(runtime, running)?;
    let saved: serde_json::Map<String, Value> = serde_json::from_str(&running.settings_json)?;
    let transport = if saved.get("rcon_web").and_then(Value::as_bool) == Some(false) {
        "source_rcon"
    } else {
        "websocket_rcon"
    };
    for (setting, command) in CONTROLS {
        let Some(expected) = runtime.settings.get(setting) else {
            continue;
        };
        if saved.get(setting) != Some(expected) {
            return Err(format!("Native Rust fixture lost its explicit {setting} setting").into());
        }
        let result = send_assistant_runtime_command(
            runtime.state.clone(),
            serde_json::from_value(serde_json::json!({
                "instanceId": running.summary.id,
                "command": command,
                "transport": transport,
                "portName": "rcon",
                "passwordSettingKey": "rcon_password"
            }))?,
            running,
        )
        .await?;
        if result.instance_id != running.summary.id || result.command != command {
            return Err("Native Rust query returned a different target or command".into());
        }
        let response = result.response_text.as_deref().unwrap_or_default();
        verify_response(command, expected, response)?;
        println!(
            "NATIVE_RUST_CONFIGURATION cycle={cycle} convar={command} effective={expected} transport={transport} evidence=native_rcon_readback same_run=true"
        );
    }
    Ok(())
}

fn require_owned_endpoint<R: tauri::Runtime>(
    runtime: &NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = runtime.package.root.canonicalize()?;
    for path in [
        runtime.effective_install_root,
        Path::new(&running.config_file_path),
    ] {
        if !path.canonicalize()?.starts_with(&root) {
            return Err("Native Rust query target is outside the owned fixture".into());
        }
    }
    let targets = crate::commands::commands_runtime_supervision::build_window_inspection_targets_from_instance(running);
    if targets.is_empty() {
        return Err("Native Rust query has no owned process".into());
    }
    for target in &targets {
        if !Path::new(&target.process_identity.image_path)
            .canonicalize()?
            .starts_with(&root)
            || app_runtime::inspect_process_identity(target.pid)?.as_ref()
                != Some(&target.process_identity)
        {
            return Err("Native Rust query process ownership changed".into());
        }
    }
    let port = running
        .ports
        .iter()
        .find(|port| port.name == "rcon" && port.protocol.eq_ignore_ascii_case("tcp"))
        .ok_or("Native Rust query has no RCON binding")?;
    let endpoints = app_platform_win::WindowsPlatform::inspect_process_network_endpoints(
        &targets,
        &[port.port],
    )?;
    if !endpoints.endpoints.iter().any(|endpoint| {
        endpoint.local_port == port.port && endpoint.protocol.eq_ignore_ascii_case("tcp")
    }) {
        return Err("Native Rust RCON endpoint is not owned by the fixture".into());
    }
    Ok(())
}

fn verify_response(command: &str, expected: &Value, response: &str) -> Result<(), String> {
    // ConsoleSystem.Internal returns FullName + ": " + QuoteSafe(getterString).
    // A no-argument query never includes help text or a pending value.
    let value = response
        .trim()
        .split_once(": ")
        .filter(|(name, _)| name.eq_ignore_ascii_case(command))
        .and_then(|(_, value)| value.strip_prefix('"'))
        .and_then(|value| value.strip_suffix('"'))
        .ok_or_else(|| format!("Native Rust response omitted the exact {command} value"))?;
    let matches = match expected {
        Value::Bool(expected) => {
            value.eq_ignore_ascii_case(if *expected { "true" } else { "false" })
        }
        Value::Number(expected) => expected
            .as_i64()
            .is_some_and(|expected| value.parse::<i64>() == Ok(expected)),
        _ => false,
    };
    if !matches {
        return Err(format!(
            "Native Rust effective {command} differs from its explicit override"
        ));
    }
    Ok(())
}

#[test]
fn rust_configuration_readback_requires_exact_convar_and_typed_value() {
    assert!(
        verify_response(
            "net.position_lod",
            &serde_json::json!(2),
            "net.position_lod: \"2\""
        )
        .is_ok()
    );
    assert!(
        verify_response(
            "server.batchpackets",
            &serde_json::json!(false),
            "server.batchpackets: \"False\""
        )
        .is_ok()
    );
    for response in [
        "net.position_lod_players: \"2\"",
        "net.position_lod: \"0\"",
        "net.position_lod: \"2\" pending",
        "net.position_lod: 2",
        "net.position_lod: \"2\"\nnet.position_lod: \"0\"",
        "Unknown command: net.position_lod",
        "",
    ] {
        assert!(verify_response("net.position_lod", &serde_json::json!(2), response).is_err());
    }
    assert!(
        verify_response(
            "server.batchpackets",
            &serde_json::json!(false),
            "server.batchpackets: \"True\""
        )
        .is_err()
    );
    assert!(
        verify_response(
            "server.useplayerupdatejobs",
            &serde_json::json!("3"),
            "server.useplayerupdatejobs: \"3\""
        )
        .is_err()
    );
}
