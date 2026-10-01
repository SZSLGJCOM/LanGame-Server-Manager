use super::{security, wire};
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{Emitter, Manager};

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "client_ctrl_c_tests.rs"]
mod ctrl_c_tests;

pub(crate) struct Client {
    endpoint: security::Endpoint,
    service_identity: Option<security::ServiceIdentity>,
    service_target: Option<Arc<app_runtime::ProcessExitTarget>>,
    closing: AtomicBool,
    exit_requested: Arc<AtomicBool>,
}

pub(super) struct ExitRequest {
    requested: Arc<AtomicBool>,
    completed: bool,
}

impl ExitRequest {
    pub(super) fn commit(mut self) {
        self.completed = true;
    }
}

impl Drop for ExitRequest {
    fn drop(&mut self) {
        if !self.completed {
            self.requested.store(false, Ordering::SeqCst);
        }
    }
}

impl Client {
    pub(super) fn is_exit_requested(&self) -> bool {
        self.exit_requested.load(Ordering::SeqCst)
    }

    pub(super) fn is_closing(&self) -> bool {
        self.closing.load(Ordering::SeqCst) || self.is_exit_requested()
    }

    pub(super) fn close_interface(&self) {
        self.closing.store(true, Ordering::SeqCst);
    }

    pub(super) fn try_begin_exit(&self) -> Option<ExitRequest> {
        // Claim the request before spawning so repeated tray clicks cannot send
        // overlapping shutdowns. Only a failed watchdog setup releases the claim;
        // committed exits keep the deadline even if graceful shutdown fails.
        self.exit_requested
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| ExitRequest {
                requested: Arc::clone(&self.exit_requested),
                completed: false,
            })
    }

    pub(super) fn new(endpoint: security::Endpoint) -> Self {
        Self {
            endpoint,
            service_identity: None,
            service_target: None,
            closing: AtomicBool::new(false),
            exit_requested: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(super) fn connect(endpoint: security::Endpoint) -> std::io::Result<Self> {
        let pipe = security::connect_verified(&endpoint.control())?;
        let identity = security::service_identity(&pipe)?;
        let target = app_runtime::ProcessExitTarget::capture(identity.pid, &identity.process)?
            .ok_or_else(|| std::io::Error::other("Runtime service exited while connecting"))?;
        let mut client = Self::new(endpoint);
        client.service_identity = Some(identity);
        client.service_target = Some(Arc::new(target));
        Ok(client)
    }

    pub(super) fn service_target(&self) -> Option<Arc<app_runtime::ProcessExitTarget>> {
        self.service_target.as_ref().map(Arc::clone)
    }

    pub(super) fn handoff_exit(&self, deadline_tick_ms: u64) -> Result<(), String> {
        let identity = self
            .service_identity
            .as_ref()
            .ok_or("Exit handoff requires the original runtime process identity")?;
        super::exit_handoff::handoff(identity, deadline_tick_ms)
    }

    pub(super) fn for_exit_handoff(
        endpoint: security::Endpoint,
        identity: security::ServiceIdentity,
        target: Arc<app_runtime::ProcessExitTarget>,
    ) -> Self {
        let mut client = Self::new(endpoint);
        client.service_identity = Some(identity);
        client.service_target = Some(target);
        client
    }

    fn ensure_request_admitted(&self, command: &str) -> Result<(), String> {
        if self.is_exit_requested() && !security::is_control_command(command) {
            return Err("Application final exit is in progress".into());
        }
        Ok(())
    }

    pub(crate) async fn request(&self, command: &str, args: Value) -> Result<Value, String> {
        self.request_with_connection_deadline(command, args, None)
            .await
    }

    async fn request_with_connection_deadline(
        &self,
        command: &str,
        args: Value,
        connection_deadline: Option<tokio::time::Instant>,
    ) -> Result<Value, String> {
        self.ensure_request_admitted(command)?;
        let endpoint = if security::is_control_command(command) {
            self.endpoint.control()
        } else {
            self.endpoint.clone()
        };
        let connection = match connection_deadline {
            Some(deadline) => security::connect_when_available_until(&endpoint, deadline).await,
            None => security::connect_when_available(&endpoint).await,
        };
        let mut pipe = connection.map_err(|e| {
            format!("Cannot reach the runtime service: {e}. No operation was sent.")
        })?;
        if let Some(identity) = &self.service_identity {
            security::verify_service_identity(&pipe, identity).map_err(|e| e.to_string())?;
        }
        // Connecting can yield while the tray accepts final exit. Recheck before
        // sending; already-sent mutations still belong to the service until settled.
        self.ensure_request_admitted(command)?;
        let request = wire::Request {
            protocol: wire::PROTOCOL,
            command: command.into(),
            args,
        };
        tokio::time::timeout(
            Duration::from_secs(15),
            wire::write_frame(&mut pipe, &request),
        )
        .await
        .map_err(
            |_| "Sending the runtime request timed out; inspect task state before retrying",
        )??;
        receive_response(command, &mut pipe).await
    }

    pub(super) async fn stop_for_tray_exit(&self, deadline_tick_ms: u64) -> Result<(), String> {
        let target = self
            .service_target
            .as_ref()
            .ok_or("Tray exit requires the original verified runtime process handle")?;
        let identity = self
            .service_identity
            .as_ref()
            .ok_or("Tray exit requires the original runtime process identity")?;
        if !target.is_running().map_err(|e| e.to_string())? {
            self.close_interface();
            return Ok(());
        }
        // A short-lived busy control pipe must not discard the only save/stop
        // request. Only connection admission consumes this remaining allowance;
        // once sent, the request is never replayed after a response failure.
        let connection_deadline =
            tokio::time::Instant::from_std(super::exit_deadline::local_deadline(deadline_tick_ms)?);
        let shutdown = self
            .request_with_connection_deadline(
                "runtime_service_tray_exit",
                serde_json::json!({
                    "deadline_tick_ms": deadline_tick_ms,
                }),
                Some(connection_deadline),
            )
            .await;
        // A disconnected response may mean the original service already exited.
        // Inspect its pinned handle, never a replacement PID or pipe owner.
        if !target.is_running().map_err(|e| e.to_string())? {
            self.close_interface();
            return Ok(());
        }
        let receipt = shutdown?;
        if receipt["shutdown_accepted"] != true || receipt["pid"] != identity.pid {
            return Err("Runtime service returned an invalid shutdown ownership receipt".into());
        }
        // Acceptance means the service owns its stop task and native deadline.
        // It says nothing about completed saves; the interface may now close.
        self.close_interface();
        Ok(())
    }
}

async fn receive_response(
    command: &str,
    pipe: &mut (impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin),
) -> Result<Value, String> {
    let response: wire::Response = tokio::time::timeout(Duration::from_secs(1830), wire::read_frame(pipe))
        .await.map_err(|_| "Runtime response timed out; the operation may still be running")?
        .map_err(|error| format!("Runtime connection ended after the request was sent ({error}); inspect task state before retrying"))?;
    if command == "runtime_service_shutdown" && response.result.is_ok() {
        // Reading the receipt proves saving/stopping completed. Losing its ACK
        // must not turn successful saving and stopping into an error.
        let _ = tokio::time::timeout(Duration::from_secs(15), wire::write_frame(pipe, &true)).await;
    }
    response.result
}

pub(super) fn connect_or_start() -> Result<Client, String> {
    let endpoint = security::Endpoint::current().map_err(|e| e.to_string())?;
    let mut arguments = vec![std::ffi::OsString::from("--runtime-service")];
    if crate::lan_host::is_lan_host_requested() {
        arguments.push("--lan-host".into());
    }
    connect_or_start_with_arguments(endpoint, &arguments)
}

pub(super) fn connect_or_start_with_arguments(
    endpoint: security::Endpoint,
    arguments: &[std::ffi::OsString],
) -> Result<Client, String> {
    match tauri::async_runtime::block_on(async { Client::connect(endpoint.clone()) }) {
        Ok(client) => return Ok(client),
        Err(error) if matches!(error.raw_os_error(), Some(2 | 231)) => (),
        Err(error) => return Err(error.to_string()),
    }
    use std::os::windows::process::CommandExt;
    let mut command =
        std::process::Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    // DETACHED_PROCESS keeps the service independent of the UI console.
    // CREATE_NEW_PROCESS_GROUP also disables Ctrl+C, which the first managed
    // game inherits even when it creates its own console.
    command.args(arguments).creation_flags(0x0000_0008);
    let mut child = command
        .spawn()
        .map_err(|e| format!("Cannot launch runtime service: {e}"))?;
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        match tauri::async_runtime::block_on(async { Client::connect(endpoint.clone()) }) {
            Ok(client) => return Ok(client),
            Err(error) if !matches!(error.raw_os_error(), Some(2 | 231)) => {
                return Err(error.to_string());
            }
            Err(_) => (),
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())?
            && !status.success()
        {
            return Err(format!(
                "Runtime service exited before becoming ready: {status}"
            ));
        }
        if std::time::Instant::now() >= deadline {
            return Err("Runtime service did not become ready within 20 seconds; inspect its startup before retrying".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub(super) fn forward(invoke: tauri::ipc::Invoke<tauri::Wry>) {
    let command = invoke.message.command().to_owned();
    let args = match invoke.message.payload() {
        tauri::ipc::InvokeBody::Json(value) => value.clone(),
        _ => {
            invoke
                .resolver
                .reject("Runtime commands require JSON arguments");
            return;
        }
    };
    let app = invoke.message.webview().app_handle().clone();
    tauri::async_runtime::spawn(async move {
        let result = app.state::<Client>().request(&command, args).await;
        invoke
            .resolver
            .respond(result.map_err(tauri::ipc::InvokeError::from));
    });
}

pub(super) fn relay_events(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut cursor = super::events::Cursor::default();
        loop {
            if app.state::<Client>().is_closing() {
                return;
            }
            match app
                .state::<Client>()
                .request("runtime_service_events", cursor.request_args())
                .await
            {
                Ok(response) => {
                    if let Ok((readback, events)) = cursor.accept(response) {
                        for event in events {
                            if ![
                                crate::runtime_log_stream::RUNTIME_LOG_STREAM_EVENT,
                                "app-shutdown-failed",
                            ]
                            .contains(&event.name.as_str())
                            {
                                continue;
                            }
                            let _ = app.emit(&event.name, event.payload);
                        }
                        if readback {
                            let _ = app.emit(super::events::RESET_EVENT, cursor.request_args());
                        }
                    }
                }
                Err(_) => {
                    if app.state::<Client>().is_closing() {
                        return;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    });
}

pub(crate) async fn stop_service(app: &tauri::AppHandle) -> Result<(), String> {
    let client = app.state::<Client>();
    let status = client
        .request("runtime_service_status", Value::Null)
        .await?;
    let pid = status["pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
        .ok_or("Runtime service did not return a process identity")?;
    let identity = app_runtime::inspect_process_identity(pid)
        .map_err(|e| e.to_string())?
        .ok_or("Runtime service exited before shutdown was requested")?;
    let receipt = client
        .request("runtime_service_shutdown", Value::Null)
        .await?;
    if receipt["shutdown_completed"] != true || receipt["pid"] != pid {
        return Err("Runtime service returned an invalid shutdown receipt".into());
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        tokio::time::sleep(Duration::from_millis(200)).await;
        if !app_runtime::process_matches_identity(pid, &identity).map_err(|e| e.to_string())? {
            client.close_interface();
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(
                "Servers stopped successfully but the runtime service process has not exited"
                    .into(),
            );
        }
    }
}
