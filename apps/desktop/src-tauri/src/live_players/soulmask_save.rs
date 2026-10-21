//! Soulmask forced saving keeps the server alive; normal stop uses SaveAndExit separately.
use super::native_console::{
    LIMIT, NativeConsole, NativeOperation, NativeTarget, help_contract, marker,
};
use crate::commands::{InstanceRuntimeCommandResult, WorldSaveCompletion};
use crate::state::{StorageContextTaskLease, spawn_blocking_storage_context_task};
use app_core::InstanceDetails;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpSocket, TcpStream};

fn save(
    details: &InstanceDetails,
    submitted_at_unix_ms: u128,
) -> Result<InstanceRuntimeCommandResult, String> {
    if !matches!(details.summary.status, app_core::InstanceStatus::Running) {
        return Err("Immediate Soulmask saving requires its current running instance.".into());
    }
    let mut console = NativeConsole::connect(details)?;
    console.save_world_once()?;
    Ok(InstanceRuntimeCommandResult {
        instance_id: details.summary.id.clone(),
        process_key: console.target.process_key.clone(),
        display_name: "Soulmask native forced save".into(),
        pid: console.target.pid,
        command: "SaveWorld 1".into(),
        response_text: Some(
            "The native console processed one forced world-save request. The game remains running."
                .into(),
        ),
        write_confirmation_pending: false,
        save_completion: Some(WorldSaveCompletion::Requested),
        submitted_at_unix_ms,
    })
}

pub(crate) async fn save_world(
    state: &crate::state::DesktopState,
    details: &InstanceDetails,
    submitted_at_unix_ms: u128,
    mutation: Option<tokio::sync::OwnedMutexGuard<()>>,
) -> Result<InstanceRuntimeCommandResult, String> {
    let mutation = mutation.ok_or("Native saving requires the instance mutation reservation.")?;
    let operation = state.begin_storage_context_operation("Soulmask immediate save")?;
    let details = details.clone();
    crate::state::spawn_blocking_storage_context_task(&operation, move || {
        let _mutation = mutation;
        save(&details, submitted_at_unix_ms)
    })
    .await
    .map_err(
        |_| "The program could not verify the Soulmask save result; the command was not resent.",
    )?
}

pub(crate) async fn save_and_exit<L: StorageContextTaskLease>(
    storage_lease: &L,
    details: &InstanceDetails,
    process_key: Option<&str>,
) -> Result<(), String> {
    let details = details.clone();
    let process_key = process_key.map(str::to_owned);
    // The existing stop lease also works after AppExit seals new operations.
    // A cancelled await can leave this read-only worker running, never a sender.
    let target = spawn_blocking_storage_context_task(storage_lease, move || {
        NativeTarget::resolve(&details, process_key.as_deref())
    })
    .await
    .map_err(|_| "Soulmask native shutdown target validation failed; no command was sent.")??;
    let socket = TcpSocket::new_v4().map_err(|_| "Cannot open the native console socket.")?;
    socket
        .bind(SocketAddr::from((target.host, 0)))
        .map_err(|_| "The native console address is not local.")?;
    let mut stream = tokio::time::timeout(
        Duration::from_secs(3),
        socket.connect(SocketAddr::from((target.host, target.port))),
    )
    .await
    .map_err(|_| "The native console connection deadline expired.")?
    .map_err(|_| "The native console is unavailable.")?;
    shutdown_exchange(&mut stream, async {
        spawn_blocking_storage_context_task(storage_lease, move || target.verify())
            .await
            .map_err(|_| "Soulmask native shutdown ownership validation failed.")?
    })
    .await
}

async fn shutdown_exchange(
    stream: &mut TcpStream,
    verify_owner: impl std::future::Future<Output = Result<(), String>>,
) -> Result<(), String> {
    let nonce = marker();
    tokio::time::timeout(
        Duration::from_secs(3),
        stream.write_all(format!("help\r\n{nonce}\r\n").as_bytes()),
    )
    .await
    .map_err(|_| "The native shutdown contract query deadline expired.")?
    .map_err(|_| "The native shutdown contract query failed.")?;
    let body = tokio::time::timeout(
        Duration::from_secs(60),
        read_shutdown_response(stream, &nonce),
    )
    .await
    .map_err(|_| "The native shutdown contract response deadline expired.")??;
    if !help_contract(NativeOperation::SaveAndExit, &body) {
        return Err("This native server does not advertise the required native command contract; no save or shutdown was sent.".into());
    }
    verify_owner.await?;
    // All network operations belong to the calling future. Dropping it closes
    // this connection; detached blocking workers can never dispatch shutdown.
    send_save_and_exit(stream).await
}

async fn read_shutdown_response(stream: &mut TcpStream, nonce: &str) -> Result<String, String> {
    let marker = format!("{nonce} Not Found!\r\n");
    let mut body = Vec::new();
    loop {
        let mut buffer = [0; 4096];
        let count = stream
            .read(&mut buffer)
            .await
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
}

async fn send_save_and_exit(writer: &mut (impl AsyncWrite + Unpin)) -> Result<(), String> {
    // A partial write is an unknown result, never permission to reconnect or replay.
    tokio::time::timeout(
        Duration::from_secs(3),
        writer.write_all(b"q\r\nSaveAndExit 1\r\n"),
    )
    .await
    .map_err(|_| shutdown_unknown())?
    .map_err(|_| shutdown_unknown())
}

fn shutdown_unknown() -> String {
    "Soulmask's native shutdown command may have been sent. It was not resent; managed process ownership is retained until exit is verified.".into()
}

#[cfg(test)]
#[path = "soulmask_stop_tests.rs"]
mod tests;
