//! Admit a new desktop only after the previous runtime has finished its exit.
use super::{Client, StartupFailure, security, wire};
use crate::runtime_service::{exit_deadline, exit_intent};
use std::ffi::OsString;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::time::Instant;

const POLL_INTERVAL: Duration = Duration::from_millis(50);

pub(super) struct StartupBudgets {
    pub(super) shutdown: Duration,
    pub(super) launch: Duration,
    pub(super) probe: Duration,
}

impl Default for StartupBudgets {
    fn default() -> Self {
        Self {
            shutdown: exit_deadline::EXIT_GRACE_TIMEOUT,
            launch: Duration::from_secs(20),
            probe: Duration::from_secs(2),
        }
    }
}

#[cfg(any(test, feature = "desktop-reliability"))]
pub(super) fn connect(
    endpoint: security::Endpoint,
    arguments: &[OsString],
    cancelled: &AtomicBool,
    budgets: StartupBudgets,
) -> Result<Client, String> {
    connect_owned(endpoint, arguments, cancelled, budgets).map_err(|failure| failure.message)
}

pub(super) fn connect_owned(
    endpoint: security::Endpoint,
    arguments: &[OsString],
    cancelled: &AtomicBool,
    budgets: StartupBudgets,
) -> Result<Client, StartupFailure> {
    tauri::async_runtime::block_on(connect_ready_owned(endpoint, arguments, cancelled, budgets))
}

#[cfg(test)]
pub(super) async fn connect_ready(
    endpoint: security::Endpoint,
    arguments: &[OsString],
    cancelled: &AtomicBool,
    budgets: StartupBudgets,
) -> Result<Client, String> {
    connect_ready_owned(endpoint, arguments, cancelled, budgets)
        .await
        .map_err(|failure| failure.message)
}

async fn connect_ready_owned(
    endpoint: security::Endpoint,
    arguments: &[OsString],
    cancelled: &AtomicBool,
    budgets: StartupBudgets,
) -> Result<Client, StartupFailure> {
    let mut child: Option<Child> = None;
    let result = connect_inner(endpoint.clone(), arguments, cancelled, budgets, &mut child).await;
    result.map_err(|message| {
        let cleanup = child
            .as_ref()
            .map(|child| cleanup_client(&endpoint, child))
            .transpose();
        match cleanup {
            Ok(client) => StartupFailure {
                message,
                cleanup_client: client.flatten().map(Box::new),
            },
            Err(error) => StartupFailure {
                message: format!(
                    "{message}; could not retain the newly launched runtime for shutdown: {error}"
                ),
                cleanup_client: None,
            },
        }
    })
}

fn cleanup_client(endpoint: &security::Endpoint, child: &Child) -> Result<Option<Client>, String> {
    let Some(process) =
        app_runtime::inspect_process_identity(child.id()).map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let target = app_runtime::ProcessExitTarget::capture(child.id(), &process)
        .map_err(|error| error.to_string())?;
    Ok(target.map(|target| {
        Client::for_exit_handoff(
            endpoint.clone(),
            security::ServiceIdentity {
                pid: child.id(),
                process,
            },
            std::sync::Arc::new(target),
        )
    }))
}

async fn connect_inner(
    endpoint: security::Endpoint,
    arguments: &[OsString],
    cancelled: &AtomicBool,
    budgets: StartupBudgets,
    child: &mut Option<Child>,
) -> Result<Client, String> {
    let mut launch_deadline = None;
    let mut discovery_deadline = None;
    // Observing another closing connection must not renew the shutdown budget.
    let mut shutdown_deadline = None;
    loop {
        if child.is_none() {
            check_cancelled(cancelled)?;
        }
        match Client::connect(endpoint.clone()) {
            Ok(client) => {
                if wait_for_admission(
                    &client,
                    cancelled,
                    child.is_none(),
                    &budgets,
                    &mut shutdown_deadline,
                    launch_deadline,
                )
                .await?
                {
                    return Ok(client);
                }
                // The pinned kernel handle, rather than a missing pipe or a
                // reused PID, proves the previous owner has actually exited.
                if let Some(child) = child.as_mut() {
                    let status = child.try_wait().map_err(|error| error.to_string())?;
                    return Err(format!(
                        "A runtime process exited before startup admission completed (launched process status: {status:?})"
                    ));
                }
                discovery_deadline = None;
                continue;
            }
            Err(error) if error.raw_os_error() == Some(231) => {
                // A busy secured name still has an owner. Starting another
                // process here would compete with an existing shutdown owner.
                let deadline = *discovery_deadline.get_or_insert_with(|| {
                    launch_deadline.unwrap_or_else(|| Instant::now() + budgets.launch)
                });
                if Instant::now() >= deadline {
                    return Err(format!(
                        "Runtime service control channel remained busy during startup: {error}"
                    ));
                }
                tokio::time::sleep_until((Instant::now() + POLL_INTERVAL).min(deadline)).await;
                continue;
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    || matches!(error.raw_os_error(), Some(2 | 109 | 232 | 233)) => {}
            Err(error) => return Err(error.to_string()),
        }
        if child.is_none() {
            check_cancelled(cancelled)?;
            use std::os::windows::process::CommandExt;
            // Match the production detached creator's console inheritance.
            *child = Some(
                Command::new(std::env::current_exe().map_err(|error| error.to_string())?)
                    .args(arguments)
                    .creation_flags(0x0000_0008)
                    .spawn()
                    .map_err(|error| format!("Cannot launch runtime service: {error}"))?,
            );
            launch_deadline = Some(Instant::now() + budgets.launch);
        }
        if let Some(status) = child
            .as_mut()
            .ok_or("Runtime startup lost its child process")?
            .try_wait()
            .map_err(|error| error.to_string())?
            && !status.success()
        {
            return Err(format!(
                "Runtime service exited before becoming ready: {status}"
            ));
        }
        let deadline = launch_deadline.ok_or("Runtime startup lost its deadline")?;
        if Instant::now() >= deadline {
            return Err("Runtime service did not become ready within 20 seconds; inspect its startup before retrying".into());
        }
        tokio::time::sleep_until((Instant::now() + POLL_INTERVAL).min(deadline)).await;
    }
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::SeqCst) {
        Err("Runtime service startup was cancelled by application exit".into())
    } else {
        Ok(())
    }
}

/// `false` means the original process exited; its client must never be installed.
async fn wait_for_admission(
    client: &Client,
    cancelled: &AtomicBool,
    may_cancel: bool,
    budgets: &StartupBudgets,
    shutdown_deadline: &mut Option<Instant>,
    launch_deadline: Option<Instant>,
) -> Result<bool, String> {
    let identity = client
        .service_identity
        .as_ref()
        .ok_or("Runtime startup requires a verified service identity")?;
    let target = client
        .service_target
        .as_ref()
        .ok_or("Runtime startup requires the original process handle")?;
    let first_probe_deadline = (Instant::now() + budgets.probe)
        .min(launch_deadline.unwrap_or_else(|| Instant::now() + budgets.probe));
    let mut final_exit = false;
    let mut last_probe_error = None;
    #[cfg(feature = "desktop-reliability")]
    let mut reported_wait = false;
    loop {
        if may_cancel {
            check_cancelled(cancelled)?;
        }
        if !target.is_running().map_err(|error| error.to_string())? {
            return Ok(false);
        }
        // The helper publishes intent before letting the old interface exit,
        // even if its final-exit IPC has not reached the service yet.
        final_exit |= exit_intent::is_marked(identity).map_err(|error| error.to_string())?;
        if final_exit {
            shutdown_deadline.get_or_insert_with(|| Instant::now() + budgets.shutdown);
            #[cfg(feature = "desktop-reliability")]
            if !reported_wait {
                crate::runtime_service::fixture::record_startup_waiting(identity.pid)?;
                reported_wait = true;
            }
        } else {
            let deadline = shutdown_deadline.unwrap_or(first_probe_deadline);
            let deadline = launch_deadline.map_or(deadline, |launch| deadline.min(launch));
            let probe_deadline = (Instant::now() + budgets.probe).min(deadline);
            match probe_status(client, probe_deadline).await {
                Ok(status) => {
                    if !status.shutdown_in_progress && !status.final_exit_requested {
                        // Do not race the independent exit owner's publication
                        // while the status request was being answered.
                        final_exit =
                            exit_intent::is_marked(identity).map_err(|error| error.to_string())?;
                        if !final_exit {
                            return Ok(true);
                        }
                    }
                    final_exit |= status.final_exit_requested;
                    shutdown_deadline.get_or_insert_with(|| Instant::now() + budgets.shutdown);
                }
                Err(StatusProbeError::Unavailable(error)) => {
                    last_probe_error = Some(error);
                }
                Err(StatusProbeError::Rejected(error)) => {
                    if !target.is_running().map_err(|error| error.to_string())? {
                        return Ok(false);
                    }
                    return Err(error);
                }
            }
        }
        if !target.is_running().map_err(|error| error.to_string())? {
            return Ok(false);
        }
        let deadline = shutdown_deadline.unwrap_or(first_probe_deadline);
        // A child created by this startup flight must become ready within its
        // original launch allowance, even if it starts shutting down. Return
        // its cleanup client promptly so an already requested exit can retain
        // the user's original final-exit deadline.
        let deadline = launch_deadline.map_or(deadline, |launch| deadline.min(launch));
        if Instant::now() >= deadline {
            return Err(if shutdown_deadline.is_some() {
                "The previous runtime service has not finished shutting down; its data is still protected. Retry after shutdown completes.".into()
            } else {
                format!(
                    "Runtime service did not answer its startup status probe within the deadline: {}",
                    last_probe_error.as_deref().unwrap_or("no status response")
                )
            });
        }
        tokio::time::sleep_until((Instant::now() + POLL_INTERVAL).min(deadline)).await;
    }
}

/// Only the read-only status probe may be repeated. No product command is sent
/// until admission succeeds, and no accepted mutation is replayed on a new owner.
enum StatusProbeError {
    Unavailable(String),
    Rejected(String),
}

async fn probe_status(
    client: &Client,
    deadline: Instant,
) -> Result<wire::Status, StatusProbeError> {
    let identity = client.service_identity.as_ref().ok_or_else(|| {
        StatusProbeError::Rejected("Runtime status probe requires a verified identity".into())
    })?;
    let operation = async {
        let mut pipe = match security::connect_when_available_until(
            &client.endpoint.control(),
            deadline,
        )
        .await
        {
            Ok(pipe) => pipe,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                return Err(StatusProbeError::Rejected(error.to_string()));
            }
            Err(error) => return Err(StatusProbeError::Unavailable(error.to_string())),
        };
        match security::verify_service_identity(&pipe, identity) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                return Err(StatusProbeError::Rejected(error.to_string()));
            }
            Err(error) => return Err(StatusProbeError::Unavailable(error.to_string())),
        }
        let request = wire::Request {
            protocol: wire::PROTOCOL,
            command: "runtime_service_status".into(),
            args: serde_json::Value::Null,
        };
        wire::write_frame(&mut pipe, &request)
            .await
            .map_err(StatusProbeError::Unavailable)?;
        let response: wire::Response = wire::read_frame(&mut pipe)
            .await
            .map_err(StatusProbeError::Unavailable)?;
        let status: wire::Status = serde_json::from_value(
            response.result.map_err(StatusProbeError::Rejected)?,
        )
        .map_err(|error| {
            StatusProbeError::Rejected(format!("Invalid runtime service startup status: {error}"))
        })?;
        if status.pid != identity.pid || status.protocol != wire::PROTOCOL {
            return Err(StatusProbeError::Rejected(
                "Runtime service startup status does not match its verified identity and protocol"
                    .into(),
            ));
        }
        Ok(status)
    };
    tokio::time::timeout_at(deadline, operation)
        .await
        .unwrap_or_else(|_| {
            Err(StatusProbeError::Unavailable(
                "Runtime status probe timed out".into(),
            ))
        })
}
