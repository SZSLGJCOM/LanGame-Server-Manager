use super::{DeadlineTcpStream, SourceRconCompletion, conan, source_rcon_exchange_with_delivery};
use std::time::Duration;

#[derive(Debug)]
pub(crate) struct RconCommandFailure {
    pub(crate) message: String,
    pub(crate) command_may_have_been_sent: bool,
}

impl RconCommandFailure {
    pub(crate) fn before_send(message: String) -> Self {
        Self {
            message,
            command_may_have_been_sent: false,
        }
    }

    pub(crate) fn after_send_attempt(message: String) -> Self {
        Self {
            message,
            command_may_have_been_sent: true,
        }
    }
}

fn ark_first_shutdown_response(module_id: &str, command: &str) -> bool {
    matches!(module_id, "arksurvivalevolved" | "arksurvivalascended") && command == "DoExit"
}

// Ordinary RCON requests still require a complete response. ARK's exact DoExit
// must release RCON after its first response: a follow-up query contends with
// native shutdown. Only independently observed full-tree exit proves stopping.
pub(crate) fn source_rcon_shutdown_exec(
    module_id: &str,
    endpoint: &str,
    password: &str,
    command: &str,
) -> Result<String, RconCommandFailure> {
    let mut stream =
        DeadlineTcpStream::connect(endpoint, Duration::from_secs(3)).map_err(|error| {
            RconCommandFailure::before_send(format!(
                "failed to connect to RCON `{endpoint}`: {error}"
            ))
        })?;
    let mut attempted = false;
    let result = if module_id == "conanexiles" {
        conan::exchange_with_delivery(
            &mut stream,
            password,
            command,
            &format!("LgsmBoundary{}", uuid::Uuid::new_v4().simple()),
            &mut attempted,
        )
    } else {
        let first_shutdown_response = ark_first_shutdown_response(module_id, command);
        let completion = match module_id {
            _ if first_shutdown_response => SourceRconCompletion::ArkShutdownFirstResponse,
            "arksurvivalevolved" | "arksurvivalascended" | "squad" => {
                SourceRconCompletion::PlayerList
            }
            "rust" => SourceRconCompletion::Rust,
            _ => SourceRconCompletion::ResponseValue,
        };
        let response = source_rcon_exchange_with_delivery(
            &mut stream,
            password,
            command,
            completion,
            &mut attempted,
        );
        if first_shutdown_response {
            // Explicitly close both directions before lifecycle waits on the
            // retained launch owner. No marker, reconnect, or command replay.
            response.and_then(|response| {
                stream
                    .shutdown()
                    .map_err(|error| format!("RCON shutdown socket closure failed: {error}"))?;
                Ok(response)
            })
        } else {
            response
        }
    };
    result.map_err(|message| RconCommandFailure {
        message,
        command_may_have_been_sent: attempted,
    })
}
