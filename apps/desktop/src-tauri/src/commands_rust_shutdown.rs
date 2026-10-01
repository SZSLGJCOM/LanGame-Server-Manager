use std::borrow::Cow;

use app_core::{InstanceDetails, ModuleShutdownSpec};
use serde_json::Value;

pub(in crate::commands) fn resolve<'a>(
    details: &InstanceDetails,
    shutdown: &'a ModuleShutdownSpec,
) -> Result<Cow<'a, ModuleShutdownSpec>, String> {
    if details.summary.module_id != "rust"
        || !shutdown
            .commands
            .iter()
            .any(|command| command.transport.eq_ignore_ascii_case("websocket_rcon"))
    {
        return Ok(Cow::Borrowed(shutdown));
    }

    let settings: Value = serde_json::from_str(&details.settings_json)
        .map_err(|error| format!("failed to parse Rust shutdown settings: {error}"))?;
    let web = settings
        .get("rcon_web")
        .and_then(Value::as_bool)
        .ok_or_else(|| String::from("Rust shutdown requires a boolean `rcon_web` setting"))?;
    if web {
        return Ok(Cow::Borrowed(shutdown));
    }

    // The verified native build selects Source RCON when rcon.web is false.
    // Choose before dispatch; connection errors must never select a protocol.
    let mut resolved = shutdown.clone();
    for command in &mut resolved.commands {
        if command.transport.eq_ignore_ascii_case("websocket_rcon") {
            command.transport = String::from("source_rcon");
            if command.enabled_setting_key.as_deref() == Some("rcon_web") {
                command.enabled_setting_key = None;
            }
        }
    }
    Ok(Cow::Owned(resolved))
}
