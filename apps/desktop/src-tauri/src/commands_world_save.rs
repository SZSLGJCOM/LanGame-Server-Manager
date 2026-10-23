//! Closed native save adapters. The module descriptor and this transport boundary
//! must agree before a native API receives a mutating request.
use super::*;

#[path = "commands_world_save_readiness.rs"]
pub(in crate::commands) mod readiness;

#[path = "commands_theforest_world_save.rs"]
pub(in crate::commands) mod theforest;

/// A successful native command is a request receipt, without a disk observer.
pub(super) fn network_save_completion(
    module: &str,
    transport: &str,
    action: Option<&str>,
    _command: &str,
    response: &str,
) -> Result<Option<WorldSaveCompletion>, String> {
    if action != Some("save_world") {
        return Ok(None);
    }
    for line in response.lines() {
        let line = line.trim().to_ascii_lowercase();
        let native_telnet_error =
            module == "sevendaystodie" && transport == "telnet" && line.starts_with("*** error:");
        if native_telnet_error
            || [
                "unknown command",
                "command not found",
                "not authorized",
                "permission denied",
                "saving failed",
                "save failed",
                "error executing command",
            ]
            .iter()
            .any(|prefix| line.starts_with(prefix))
        {
            return Err(
                "The game rejected the native save command. Check its console for details.".into(),
            );
        }
    }
    Ok(Some(WorldSaveCompletion::Requested))
}

pub(super) fn validate_action(module: &str, action: &ModulePlayerActionSpec) -> Result<(), String> {
    if let Some(adapter) = NativeSave::from_transport(&action.transport) {
        return adapter.validate_action(module, action);
    }
    let (transport, command, process, port, password, enabled) = match module {
        "minecraft" => ("stdin", "save-all flush", Some("main"), None, None, None),
        "projectzomboid" | "terraria" | "necesse" | "romestead" => {
            ("stdin", "save", Some("main"), None, None, None)
        }
        "unturned" => ("stdin", "Save", Some("main"), None, None, None),
        "sevendaystodie" => (
            "telnet",
            "saveworld",
            None,
            Some("telnet"),
            Some("telnet_password"),
            Some("telnet_enabled"),
        ),
        "arksurvivalevolved" | "arksurvivalascended" => (
            "source_rcon",
            "saveworld",
            None,
            Some("rcon"),
            Some("admin_password"),
            Some("rcon_enabled"),
        ),
        "palworld" => (
            "palworld_rest",
            "save",
            None,
            Some("rest_api"),
            Some("admin_password"),
            Some("rest_api_enabled"),
        ),
        "humanitz" => (
            "source_rcon",
            "save",
            None,
            Some("rcon"),
            Some("rcon_password"),
            Some("rcon_enabled"),
        ),
        "dontstarve" => ("stdin", "c_save()", Some("master"), None, None, None),
        "theforest" => ("stdin", "save", Some("main"), None, None, None),
        "rust" => (
            "websocket_rcon",
            "server.save",
            None,
            Some("rcon"),
            Some("rcon_password"),
            Some("rcon_web"),
        ),
        _ => return Err("This module has no declared native save command.".into()),
    };
    if action.id != "save_world"
        || action.transport != transport
        || action.command_template != command
        || action.target_required
        || action.destructive
        || !action.role_values.is_empty()
        || action.process_key.as_deref() != process
        || action.port_name.as_deref() != port
        || action.password_setting_key.as_deref() != password
        || action.enabled_setting_key.as_deref() != enabled
    {
        return Err("Saving requires the game's declared targetless native command.".into());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NativeSave {
    Satisfactory,
    Astroneer,
    Valheim,
    Dragonwilds,
    CoreKeeper,
    VRising,
    SonsOfTheForest,
    AbioticFactor,
    ReturnToMoria,
    Windrose,
    Enshrouded,
    RimWorld,
    Barotrauma,
    Soulmask,
    Nightingale,
    Conan,
    Scum,
}

impl NativeSave {
    pub(super) fn from_transport(transport: &str) -> Option<Self> {
        match transport {
            "satisfactory_https" => Some(Self::Satisfactory),
            "astroneer_save" => Some(Self::Astroneer),
            "valheim_native" => Some(Self::Valheim),
            "dragonwilds_save" => Some(Self::Dragonwilds),
            "corekeeper_save" => Some(Self::CoreKeeper),
            "vrising_save" => Some(Self::VRising),
            "sonsoftheforest_save" => Some(Self::SonsOfTheForest),
            "abioticfactor_save" => Some(Self::AbioticFactor),
            "returntomoria_save" => Some(Self::ReturnToMoria),
            "windrose_save" => Some(Self::Windrose),
            "enshrouded_save" => Some(Self::Enshrouded),
            "rimworld_save" => Some(Self::RimWorld),
            "barotrauma_save" => Some(Self::Barotrauma),
            "soulmask_save" => Some(Self::Soulmask),
            "nightingale_save" => Some(Self::Nightingale),
            "conanexiles_save" => Some(Self::Conan),
            "scum_save" => Some(Self::Scum),
            _ => None,
        }
    }

    pub(super) fn contract(self) -> (&'static str, &'static str) {
        match self {
            Self::Satisfactory => ("satisfactory", "save"),
            Self::Astroneer => ("astroneer", "save"),
            Self::Valheim => ("valheim", "save"),
            Self::Dragonwilds => ("runescapedragonwilds", "save"),
            Self::CoreKeeper => ("corekeeper", "save"),
            Self::VRising => ("vrising", "save"),
            Self::SonsOfTheForest => ("sonsoftheforest", "save"),
            Self::AbioticFactor => ("abioticfactor", "save"),
            Self::ReturnToMoria => ("returntomoria", "save"),
            Self::Windrose => ("windrose", "save"),
            Self::Enshrouded => ("enshrouded", "save"),
            Self::RimWorld => ("rimworld", "save"),
            Self::Barotrauma => ("barotrauma", "save"),
            Self::Soulmask => ("soulmask", "SaveWorld 1"),
            Self::Nightingale => ("nightingale", "save"),
            Self::Conan => ("conanexiles", "save"),
            Self::Scum => ("scum", "save_persistent_state"),
        }
    }

    pub(super) fn validate_action(
        self,
        module_id: &str,
        action: &ModulePlayerActionSpec,
    ) -> Result<(), String> {
        let (expected_module, command) = self.contract();
        let (port, password) = if matches!(self, Self::Astroneer) {
            (Some("console"), Some("console_password"))
        } else {
            (None, None)
        };
        if module_id != expected_module
            || action.id != "save_world"
            || action.command_template != command
            || action.target_required
            || action.destructive
            || !action.role_values.is_empty()
            || action.process_key.is_some()
            || action.port_name.as_deref() != port
            || action.password_setting_key.as_deref() != password
            || action.enabled_setting_key.is_some()
        {
            return Err("Native world saving requires the game's declared save contract.".into());
        }
        Ok(())
    }

    pub(super) async fn execute(
        self,
        state: &tauri::State<'_, DesktopState>,
        details: &InstanceDetails,
        action_id: Option<&str>,
        command: &str,
        submitted_at_unix_ms: u128,
        mutation: Option<tokio::sync::OwnedMutexGuard<()>>,
    ) -> Result<InstanceRuntimeCommandResult, String> {
        let (module_id, expected_command) = self.contract();
        if details.summary.module_id != module_id
            || action_id != Some("save_world")
            || command != expected_command
            || mutation.is_none()
        {
            return Err("Native world saving requires a declared save_world action.".into());
        }
        let process = details
            .active_run
            .as_ref()
            .and_then(|run| {
                run.processes.iter().find(|process| {
                    process.is_primary
                        && process.status == "running"
                        && process.pid.is_some_and(|pid| pid > 0)
                })
            })
            .ok_or("Native world saving requires the current managed process.")?;
        let response = match self {
            Self::Satisfactory => {
                super::commands_satisfactory_world::save_current_world(state, details, mutation)
                    .await?
            }
            Self::Valheim => {
                crate::live_players::valheim_control::save_world(state, details, mutation).await?
            }
            Self::Dragonwilds => {
                return crate::live_players::dragonwilds_control::save_world(
                    state,
                    details,
                    submitted_at_unix_ms,
                    mutation,
                )
                .await;
            }
            Self::RimWorld => {
                return super::commands_native_save::rimworld(
                    state,
                    details,
                    submitted_at_unix_ms,
                    mutation,
                )
                .await;
            }
            Self::CoreKeeper | Self::VRising | Self::SonsOfTheForest | Self::Barotrauma => {
                return super::commands_managed_save_control::save_world(
                    state,
                    details,
                    submitted_at_unix_ms,
                    mutation,
                )
                .await;
            }
            Self::AbioticFactor
            | Self::ReturnToMoria
            | Self::Nightingale
            | Self::Conan
            | Self::Scum => {
                return crate::live_players::file_ipc::unreal_world_save::save_world(
                    state,
                    details,
                    submitted_at_unix_ms,
                    mutation,
                )
                .await;
            }
            Self::Astroneer => {
                return super::commands_native_save::astroneer(
                    state,
                    details,
                    submitted_at_unix_ms,
                    mutation,
                )
                .await;
            }
            Self::Windrose => {
                return crate::live_players::file_ipc::windrose_world_save::save_world(
                    state,
                    details,
                    submitted_at_unix_ms,
                    mutation,
                )
                .await;
            }
            Self::Enshrouded => {
                return crate::live_players::enshrouded_native::save_world(
                    state,
                    details,
                    submitted_at_unix_ms,
                    mutation,
                )
                .await;
            }
            Self::Soulmask => {
                return crate::live_players::soulmask::save_world(
                    state,
                    details,
                    submitted_at_unix_ms,
                    mutation,
                )
                .await;
            }
        };
        Ok(InstanceRuntimeCommandResult {
            instance_id: details.summary.id.clone(),
            process_key: process.process_key.clone(),
            display_name: format!("{module_id} native save"),
            pid: process.pid.unwrap_or_default(),
            command: command.into(),
            response_text: Some(response),
            write_confirmation_pending: false,
            save_completion: Some(WorldSaveCompletion::Confirmed),
            submitted_at_unix_ms,
        })
    }
}

#[cfg(test)]
#[path = "commands_world_save_tests.rs"]
mod tests;
