//! Closed native Echo operations shared by Soulmask save and normal shutdown.
use app_core::{InstanceDetails, ModuleShutdownSpec, ProcessIdentity};
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

pub(super) const LIMIT: usize = 64 * 1024;

#[derive(Clone, Copy)]
pub(super) enum NativeOperation {
    SaveWorld,
    SaveAndExit,
}

pub(super) fn help_contract(operation: NativeOperation, body: &str) -> bool {
    let text = body.replace("\r\n", "\n");
    let (name, required) = match operation {
        NativeOperation::SaveWorld => (
            "SaveWorld",
            &[
                "Only save the world, do not exit the game.",
                "Parameter 1 means force save and write to hard disk immediately.",
            ][..],
        ),
        NativeOperation::SaveAndExit => (
            "SaveAndExit",
            &[
                "Alia: close exit shutdown",
                "After the number of seconds specified by the parameter, save and exit the game world.",
                "[close 300]",
            ][..],
        ),
    };
    let Some(start) = text.find(&format!(": {name}\n")) else {
        return false;
    };
    let block = text[start..].split("\n\n  ").next().unwrap_or_default();
    required.iter().all(|part| block.contains(part)) && !text.contains("help Not Found!")
}

fn read_response(stream: &mut TcpStream, nonce: &str, deadline: Instant) -> Result<String, String> {
    let marker = format!("{nonce} Not Found!\r\n");
    let mut body = Vec::new();
    while Instant::now() < deadline {
        stream
            .set_read_timeout(Some(
                deadline
                    .saturating_duration_since(Instant::now())
                    .max(Duration::from_millis(1)),
            ))
            .map_err(|_| "Cannot set the native console response deadline.")?;
        let mut buffer = [0; 4096];
        let count = stream
            .read(&mut buffer)
            .map_err(|_| "The native console response is incomplete.")?;
        if count == 0 || body.len() + count > LIMIT {
            return Err("The native console response is incomplete or exceeds its limit.".into());
        }
        body.extend_from_slice(&buffer[..count]);
        if body.ends_with(marker.as_bytes()) {
            let prefix = &body[..body.len() - marker.len()];
            if !prefix.is_empty() && !prefix.ends_with(b"\r\n") {
                return Err("The native console delimiter is invalid.".into());
            }
            return String::from_utf8(prefix.to_vec())
                .map_err(|_| "The native console response is not UTF-8.".into());
        }
    }
    Err("The native console response deadline expired.".into())
}

fn unknown() -> String {
    "The program could not verify this Soulmask save result. The command may already have executed and was not resent.".into()
}

pub(super) fn marker() -> String {
    format!("LGM_{:024x}", uuid::Uuid::new_v4().as_u128() >> 32)
}

pub(super) struct NativeTarget {
    pub(super) pid: u32,
    pub(super) process_key: String,
    identity: ProcessIdentity,
    pub(super) host: Ipv4Addr,
    pub(super) port: u16,
}

impl NativeTarget {
    pub(super) fn verify(&self) -> Result<(), String> {
        if !app_runtime::process_matches_identity(self.pid, &self.identity)
            .map_err(|_| "Cannot verify the native server process.")?
        {
            return Err("The native server process changed or stopped.".into());
        }
        if app_platform_win::WindowsPlatform::ipv4_tcp_listener_owners(self.host, self.port)?
            != BTreeSet::from([self.pid])
        {
            return Err(
                "The native Echo listener is not exclusively owned by this managed server.".into(),
            );
        }
        Ok(())
    }
    pub(super) fn resolve(
        details: &InstanceDetails,
        process_key: Option<&str>,
    ) -> Result<Self, String> {
        if details.summary.module_id != "soulmask" {
            return Err("Soulmask native Echo is not declared for this module.".into());
        }
        let run = details
            .active_run
            .as_ref()
            .ok_or("The current managed run is unavailable.")?;
        if details.summary.active_process_count != 1
            || run.process_count != 1
            || run.processes.len() != 1
        {
            return Err(
                "The native save requires one unambiguous managed Soulmask process.".into(),
            );
        }
        let mut candidates = run.processes.iter().filter(|p| {
            p.is_primary
                && p.status == "running"
                && p.run_id == run.run_id
                && p.session_id == run.session_id
                && p.exit_code.is_none()
                && !p.crash_flag
        });
        let process = candidates
            .next()
            .ok_or("The current managed Soulmask process is unavailable.")?;
        if candidates.next().is_some() {
            return Err("The managed Soulmask process is ambiguous.".into());
        }
        let pid = process
            .pid
            .filter(|p| *p > 0)
            .ok_or("The native server PID is unavailable.")?;
        let identity = process
            .process_identity
            .as_ref()
            .filter(|i| i.creation_time != 0)
            .ok_or("The native process identity is unavailable.")?;
        if !std::path::Path::new(&identity.image_path)
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case("WSServer-Win64-Shipping.exe"))
        {
            return Err("The managed process is not the Soulmask dedicated server.".into());
        }
        if process_key.is_some_and(|key| key != process.process_key.as_str()) {
            return Err(
                "The Soulmask command target differs from its current managed primary process."
                    .into(),
            );
        }
        let instance = std::path::Path::new(&details.config_file_path)
            .parent()
            .and_then(std::path::Path::parent)
            .ok_or("The instance program directory is unavailable.")?;
        let runtime = app_storage::resolve_instance_runtime_root(instance)
            .map_err(|_| "Cannot resolve this instance's program directory.")?;
        if dunce::canonicalize(&identity.image_path)
            .map_err(|_| "Cannot identify the native executable.")?
            != dunce::canonicalize(runtime.join("WS/Binaries/Win64/WSServer-Win64-Shipping.exe"))
                .map_err(|_| "The instance's native executable is unavailable.")?
        {
            return Err(
                "The native server process belongs to another instance program directory.".into(),
            );
        }
        let mut ports = details.ports.iter().filter(|port| port.name == "echo");
        let port = ports
            .next()
            .filter(|p| p.port != 0 && p.protocol.eq_ignore_ascii_case("tcp"))
            .ok_or("Configure the Soulmask Echo TCP port before immediate saving.")?;
        if ports.next().is_some() {
            return Err("The Soulmask Echo port is ambiguous.".into());
        }
        let host = app_storage::normalize_query_host(&details.summary.bind_ip)
            .parse::<Ipv4Addr>()
            .ok()
            .filter(|ip| !ip.is_unspecified() && !ip.is_multicast() && !ip.is_broadcast())
            .ok_or("The native console requires a local IPv4 bind address.")?;
        let target = Self {
            pid,
            process_key: process.process_key.clone(),
            identity: identity.clone(),
            host,
            port: port.port,
        };
        target.verify()?;
        Ok(target)
    }
}

pub(super) struct NativeConsole {
    stream: TcpStream,
    pub(super) target: NativeTarget,
}

impl NativeConsole {
    pub(super) fn connect(details: &InstanceDetails) -> Result<Self, String> {
        let target = NativeTarget::resolve(details, None)?;
        let socket = socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::STREAM, None)
            .map_err(|_| "Cannot open the local native console socket.")?;
        // Binding the source rejects non-local destinations even when an instance's
        // advertised address is accidentally configured as the native bind address.
        socket
            .bind(&SocketAddr::from((target.host, 0)).into())
            .map_err(|_| "The native console address is not local.")?;
        socket
            .connect_timeout(
                &SocketAddr::from((target.host, target.port)).into(),
                Duration::from_secs(3),
            )
            .map_err(|_| "The native console is unavailable.")?;
        let mut stream: TcpStream = socket.into();
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .map_err(|_| "Cannot set the native console deadline.")?;
        let probe = marker();
        stream
            .write_all(format!("help\r\n{probe}\r\n").as_bytes())
            .map_err(|_| "The native save contract query failed.")?;
        if !help_contract(
            NativeOperation::SaveWorld,
            &read_response(
                &mut stream,
                &probe,
                Instant::now() + Duration::from_secs(60),
            )?,
        ) {
            return Err("This native server does not advertise the required native command contract; no save or shutdown was sent.".into());
        }
        target.verify()?;
        Ok(Self { stream, target })
    }
    pub(super) fn save_world_once(&mut self) -> Result<(), String> {
        let nonce = marker();
        // Leave help pagination, then invoke exactly one fixed native save command.
        self.stream
            .write_all(format!("q\r\nSaveWorld 1\r\n{nonce}\r\n").as_bytes())
            .map_err(|_| unknown())?;
        let body = read_response(
            &mut self.stream,
            &nonce,
            Instant::now() + Duration::from_secs(15),
        )
        .map_err(|_| unknown())?;
        self.target.verify().map_err(|_| unknown())?;
        // Unknown text is not a durable-save receipt. The native valid save command
        // has no completion text; only its queue delimiter establishes delivery.
        if body.lines().any(|line| {
            let text = line.to_ascii_lowercase();
            text.contains("not found") || text.contains("failed") || text.contains("error")
        }) {
            return Err(unknown());
        }
        let _ = self.stream.write_all(b"dc\r\n");
        Ok(())
    }
}

pub(crate) fn validate_shutdown_spec(
    details: &InstanceDetails,
    shutdown: &ModuleShutdownSpec,
) -> Result<(), String> {
    if details.summary.module_id != "soulmask" {
        return Ok(());
    }
    let [command] = shutdown.commands.as_slice() else {
        return Err("Soulmask normal shutdown requires one native SaveAndExit command.".into());
    };
    if command.transport != "soulmask_echo_shutdown"
        || command.command != "SaveAndExit 1"
        || command.fallback_transport.is_some()
        || command
            .port_name
            .as_deref()
            .is_some_and(|port| port != "echo")
        || command.password_setting_key.is_some()
        || command.enabled_setting_key.is_some()
    {
        return Err("Soulmask normal shutdown requires its fixed native SaveAndExit command without a fallback transport.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_shutdown_contract_is_distinct_from_save_and_backup() {
        let text = "  2: SaveAndExit\r\n\tAlia: close exit shutdown\r\n\tDESC: After the number of seconds specified by the parameter, save and exit the game world.\r\n\tExample: [close 300] means that after the 300-second countdown, the server will be saved and closed.\r\n\r\n  3: StopCloseServer\r\n";
        assert!(help_contract(NativeOperation::SaveAndExit, text));
        assert!(!help_contract(NativeOperation::SaveWorld, text));
        for invalid in [
            text.replace("SaveAndExit", "BackupDatabase"),
            text.replace(
                "save and exit the game world",
                "disconnect the management connection",
            ),
            text.replace("[close 300]", "[close]"),
        ] {
            assert!(!help_contract(NativeOperation::SaveAndExit, &invalid));
        }
        let marker = marker();
        assert_eq!(marker.len(), 28);
        assert!(marker[4..].bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn native_save_contract_never_adopts_backup_or_exit_help() {
        let text = "  4: SaveWorld\r\n\tDESC: Only save the world, do not exit the game. Parameter 1 means force save and write to hard disk immediately.\r\n\r\n  5: BackupDatabase\r\n";
        assert!(help_contract(NativeOperation::SaveWorld, text));
        for invalid in [
            text.replace("SaveWorld", "BackupDatabase"),
            text.replace(
                "Only save the world, do not exit the game.",
                "Save and exit.",
            ),
            text.replace("Parameter 1", "Parameter 0"),
        ] {
            assert!(!help_contract(NativeOperation::SaveWorld, &invalid));
        }
    }
    #[test]
    fn native_echo_response_requires_the_exact_complete_nonce() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(b"answer\r\nnonce Not Found!\r\n").unwrap();
        });
        let mut stream = TcpStream::connect(address).unwrap();
        assert_eq!(
            read_response(
                &mut stream,
                "nonce",
                Instant::now() + Duration::from_secs(1)
            )
            .unwrap(),
            "answer\r\n"
        );
        worker.join().unwrap();
    }
}
