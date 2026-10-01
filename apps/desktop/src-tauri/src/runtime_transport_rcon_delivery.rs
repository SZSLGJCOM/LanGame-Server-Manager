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

// Ordinary RCON requests still require a complete response. Only the shutdown
// caller may replace a missing response with independently observed tree exit.
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
        let completion = match module_id {
            "arksurvivalevolved" | "arksurvivalascended" | "squad" => {
                SourceRconCompletion::PlayerList
            }
            "rust" => SourceRconCompletion::Rust,
            _ => SourceRconCompletion::ResponseValue,
        };
        source_rcon_exchange_with_delivery(
            &mut stream,
            password,
            command,
            completion,
            &mut attempted,
        )
    };
    result.map_err(|message| RconCommandFailure {
        message,
        command_may_have_been_sent: attempted,
    })
}
