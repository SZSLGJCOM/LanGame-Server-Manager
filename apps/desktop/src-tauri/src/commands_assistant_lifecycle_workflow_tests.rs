//! Native process acceptance over disposable settings, package and database.
//! A local console stub exercises LGSM lifecycle through the bundled Necesse
//! contract, not compatibility with the real game. It binds only owned loopback UDP.
use super::assistant_repair_integration_tests::read_repair_model_request;
use super::assistant_tool_fixtures::{assert_native_tool_available, openai_tool_response};
use super::*;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[path = "commands_assistant_lifecycle_firewall_fixture.rs"]
mod firewall;

pub(in crate::commands) fn fixture_firewall_applies(
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
) -> Result<bool, String> {
    firewall::applies(storage, instance)
}

struct Fixture {
    app: tauri::App<tauri::test::MockRuntime>,
    storage: StorageBootstrap,
    instance_id: String,
    _firewall: firewall::FirewallFixtureGuard,
    port_reservation: std::sync::Mutex<Option<std::net::UdpSocket>>,
    _environment: ProgramDataEnvGuard,
}

impl Fixture {
    async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = temp_test_dir("assistant-lifecycle");
        let environment = ProgramDataEnvGuard::set(&root.join("programdata"));
        let settings = isolated_smoke_app_settings(&root)?;
        let package = root.join("games/necesse");
        fs::create_dir_all(package.join("jre/bin"))?;
        fs::write(
            package.join("Server.jar"),
            b"Owned synthetic native-process fixture",
        )?;
        let source = root.join("lifecycle-fixture.cs");
        fs::write(
            &source,
            r#"
using System;
using System.Net;
using System.Net.Sockets;
using System.Threading;
class LifecycleFixture {
    static int Main(string[] args) {
        if (Array.IndexOf(args, "--fail-start") >= 0) {
            Console.Error.WriteLine("Fixture requested startup failure");
            return 7;
        }
        int ipIndex = Array.IndexOf(args, "-ip");
        int portIndex = Array.IndexOf(args, "-port");
        if (ipIndex < 0 || portIndex < 0 || args[ipIndex + 1] != "127.0.0.1") return 8;
        int port = Int32.Parse(args[portIndex + 1]);
        using (var deadline = new Timer(delegate { Environment.Exit(9); }, null, 90000, Timeout.Infinite))
        using (var socket = new UdpClient(new IPEndPoint(IPAddress.Loopback, port))) {
            Console.CancelKeyPress += delegate { Environment.Exit(0); };
            Console.WriteLine("Started server using port " + port + " with 8 slots");
            Console.Out.Flush();
            string line;
            while ((line = Console.ReadLine()) != null) {
                if (line.Trim() == "stop") return 0;
            }
            return 0;
        }
    }
}
"#,
        )?;
        let compiler = app_runtime::windows_system_directory()?
            .parent()
            .ok_or("Windows system directory has no parent")?
            .join("Microsoft.NET/Framework64/v4.0.30319/csc.exe");
        let compiled_executable = package.join("jre/bin/java.exe");
        let output_argument = format!("/out:{}", compiled_executable.display());
        let source_argument = source.to_string_lossy().into_owned();
        let compiled = tokio::task::spawn_blocking(move || {
            app_runtime::capture_windows_utility(
                &compiler,
                &["/nologo", "/target:exe", &output_argument, &source_argument],
                Duration::from_secs(30),
            )
        })
        .await??;
        if !compiled.status.success() {
            return Err(format!(
                "Lifecycle fixture compilation failed: {}\n{}",
                String::from_utf8_lossy(&compiled.stdout),
                String::from_utf8_lossy(&compiled.stderr)
            )
            .into());
        }
        record_fake_program_baseline(&settings, "necesse")?;
        let app = tauri::test::mock_builder()
            .manage(DesktopState::from_storage(
                &bootstrap_storage().expect("bootstrap isolated fixture storage"),
            ))
            .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
        let state = app.state::<DesktopState>();
        sync_modules_to_storage(state.clone()).await?;
        let created =
            create_fake_module_instance(state.clone(), "necesse", "LAN process fixture").await?;
        let storage = bootstrap_storage()?;
        {
            let mut current = state.app_state.write().unwrap();
            current.settings = storage.settings.clone();
            current.storage = storage.storage_status.clone();
        }
        let details = read_instance_details(&storage.paths, &created.summary.id).await?;
        let port_reservation = std::net::UdpSocket::bind("127.0.0.1:0")?;
        let port = port_reservation.local_addr()?.port();
        let mut settings_json: Value = serde_json::from_str(&details.settings_json)?;
        // Lifecycle acceptance must keep the owned executable across restarts.
        settings_json["program_update"] = json!({"policy": "pinned"});
        update_instance(
            &storage.paths,
            UpdateInstanceInput {
                id: details.summary.id.clone(),
                bind_ip: "127.0.0.1".into(),
                auto_backup_on_stop: false,
                backup_retention_count: details.backup_retention_count,
                settings_json: settings_json.to_string(),
                ports: vec![PortBinding {
                    name: "game".into(),
                    protocol: "udp".into(),
                    port,
                }],
            },
        )
        .await?;
        let details = read_instance_details(&storage.paths, &created.summary.id).await?;
        let firewall = firewall::FirewallFixtureGuard::register(
            &root,
            &storage,
            &details,
            &compiled_executable,
        )?;
        assert!(fixture_firewall_applies(&storage, &details)?);
        let mut unrelated = details.clone();
        unrelated.summary.id = "unregistered-instance".into();
        assert!(!fixture_firewall_applies(&storage, &unrelated)?);
        let mut unsafe_target = details.clone();
        unsafe_target.summary.bind_ip = "0.0.0.0".into();
        assert!(fixture_firewall_applies(&storage, &unsafe_target).is_err());
        unsafe_target = details.clone();
        unsafe_target.ports[0].port = if port == u16::MAX { port - 1 } else { port + 1 };
        assert!(fixture_firewall_applies(&storage, &unsafe_target).is_err());
        drop(firewall);
        assert!(!fixture_firewall_applies(&storage, &details)?);
        let firewall = firewall::FirewallFixtureGuard::register(
            &root,
            &storage,
            &details,
            &compiled_executable,
        )?;
        Ok(Self {
            app,
            storage,
            instance_id: created.summary.id,
            _firewall: firewall,
            port_reservation: std::sync::Mutex::new(Some(port_reservation)),
            _environment: environment,
        })
    }

    async fn start(&self) -> Result<StartInstanceResult, Box<dyn std::error::Error>> {
        // Release the OS-selected loopback port only when the owned process is
        // ready to start. A later remap cannot match the firewall fixture permit.
        drop(self.port_reservation.lock().unwrap().take());
        let expected = read_instance_details(&self.storage.paths, &self.instance_id).await?;
        let result = tokio::time::timeout(
            Duration::from_secs(45),
            Box::pin(start_instance_process_with_preconditions(
                None,
                &self.app.state::<DesktopState>(),
                &self.storage,
                self.instance_id.clone(),
                "manual",
                RuntimeStartPreconditions {
                    world_start: None,
                    instance: Some(expected),
                    file_changes: vec![],
                },
            )),
        )
        .await??;
        assert!(result.pid > 0);
        Ok(result)
    }

    async fn stop(&self) -> Result<StopInstanceResult, String> {
        tokio::time::timeout(
            Duration::from_secs(30),
            Box::pin(stop_instance_process(
                self.app.state::<DesktopState>(),
                self.instance_id.clone(),
            )),
        )
        .await
        .map_err(|_| "Fixture native stop exceeded its deadline".to_string())?
    }

    async fn preview(
        &self,
        action: &str,
    ) -> Result<AssistantConfirmOperationInput, Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let mut provider = stored_openai_compatible_ai_mock_settings();
        provider.base_url = format!("http://{}/v1", listener.local_addr()?);
        let server = async {
            for index in 0..4 {
                let (mut stream, _) = listener.accept().await?;
                let request = read_repair_model_request(&mut stream).await?;
                let (tool, arguments) = match index {
                    1 => (
                        "record_task_requirements",
                        json!({"settings":[],"ports":[],"forbiddenActions":[],"unverified":[]}),
                    ),
                    2 => ("finish_task_requirements", json!({})),
                    _ => (
                        "propose_operation",
                        json!({"action":action,"reason":"Perform the user's explicit lifecycle request."}),
                    ),
                };
                assert_native_tool_available(&request, tool);
                let body = openai_tool_response(&format!("lifecycle-{index}"), tool, arguments)
                    .to_string();
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await?;
                stream.shutdown().await?;
            }
            Ok::<(), Box<dyn std::error::Error>>(())
        };
        let operation = assistant_preview_operation_inner(
            self.app.state::<DesktopState>(),
            AssistantExecuteOperationInput {
                task: AssistantTaskRequest {
                    goal: AssistantTaskGoal::ApplyChange,
                    preserve_existing_mods: true,
                },
                settings: provider.clone(),
                prompt: format!("Please {action} for the selected instance."),
                context: None,
                selected_instance_id: Some(self.instance_id.clone()),
                selected_module_id: Some("necesse".into()),
            },
        );
        let ((), preview) = tokio::time::timeout(Duration::from_secs(45), async {
            tokio::try_join!(Box::pin(server), async {
                operation
                    .await
                    .map_err(Into::<Box<dyn std::error::Error>>::into)
            })
        })
        .await??;
        assert!(preview.requires_confirmation);
        Ok(AssistantConfirmOperationInput {
            continue_task: false,
            conversation_id: preview.conversation_id,
            settings: provider,
            confirmation_token: preview.confirmation_token.ok_or("confirmation token")?,
            plan_summary: preview.plan_summary.ok_or("plan summary")?,
        })
    }

    async fn confirm(
        &self,
        input: AssistantConfirmOperationInput,
    ) -> Result<AssistantExecuteOperationOutput, String> {
        tokio::time::timeout(
            Duration::from_secs(60),
            assistant_confirm_operation_inner(self.app.state::<DesktopState>(), input),
        )
        .await
        .map_err(|_| "Fixture assistant confirmation exceeded its deadline".to_string())?
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Always reap an owned test process, including assertion or startup failure.
        if let Some(mut running) = self
            .app
            .state::<DesktopState>()
            .runtime_supervisor
            .lock()
            .unwrap()
            .take_running_for_stop(&self.instance_id)
            && let Err(error) = stop_managed_instance(&mut running)
        {
            eprintln!("Owned lifecycle fixture process cleanup failed: {error}");
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_stop_confirmation_stops_only_the_bound_native_run() -> TestResult {
    let _guard = command_smoke_lock().lock().await;
    let fixture = Fixture::new().await?;
    let started = fixture.start().await?;
    let confirmation = fixture.preview("stop_server").await?;
    assert_eq!(
        read_instance_details(&fixture.storage.paths, &fixture.instance_id)
            .await?
            .active_run
            .ok_or("preview run")?
            .run_id,
        started.run_id
    );
    let output = fixture.confirm(confirmation.clone()).await?;
    assert_eq!(
        output.task.as_ref().ok_or("task")?.status,
        AssistantTaskStatus::Completed
    );
    let receipt = output.verification.as_ref().ok_or("stop verification")?;
    assert_eq!(receipt.status, AssistantVerificationStatus::Verified);
    assert_eq!(receipt.evidence["stoppedRunId"], started.run_id);
    assert!(
        read_instance_details(&fixture.storage.paths, &fixture.instance_id)
            .await?
            .active_run
            .is_none()
    );
    assert!(inspect_process_identity(started.pid)?.is_none());
    assert!(output.follow_up.is_none() && output.continuation.is_none());
    assert!(fixture.confirm(confirmation).await.is_err());
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_restart_confirmation_verifies_a_distinct_native_run() -> TestResult {
    let _guard = command_smoke_lock().lock().await;
    let fixture = Fixture::new().await?;
    let old = fixture.start().await?;
    let output = fixture
        .confirm(fixture.preview("restart_server").await?)
        .await?;
    assert_eq!(
        output.task.as_ref().ok_or("task")?.status,
        AssistantTaskStatus::Completed,
        "{:?}",
        output.verification
    );
    let receipt = output.verification.as_ref().ok_or("restart verification")?;
    assert_eq!(receipt.status, AssistantVerificationStatus::Verified);
    assert_eq!(receipt.evidence["stopReceipt"]["stoppedRunId"], old.run_id);
    let current = read_instance_details(&fixture.storage.paths, &fixture.instance_id)
        .await?
        .active_run
        .ok_or("new run")?;
    assert_ne!(current.run_id, old.run_id);
    assert_ne!(current.session_id, old.session_id);
    assert_eq!(receipt.run_id, Some(current.run_id));
    assert!(inspect_process_identity(old.pid)?.is_none());
    assert!(output.follow_up.is_none() && output.continuation.is_none());
    fixture.stop().await?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_lifecycle_confirmation_rejects_replacement_run_without_stopping_it() -> TestResult
{
    let _guard = command_smoke_lock().lock().await;
    let fixture = Fixture::new().await?;
    let old = fixture.start().await?;
    let confirmation = fixture.preview("stop_server").await?;
    fixture.stop().await?;
    let replacement = fixture.start().await?;
    assert_ne!(old.run_id, replacement.run_id);
    let error = fixture
        .confirm(confirmation)
        .await
        .expect_err("stale run confirmation");
    assert!(
        error.contains("changed") && error.contains("preview"),
        "{error}"
    );
    let current = read_instance_details(&fixture.storage.paths, &fixture.instance_id)
        .await?
        .active_run
        .ok_or("replacement must remain running")?;
    assert_eq!(current.run_id, replacement.run_id);
    assert!(inspect_process_identity(replacement.pid)?.is_some());
    fixture.stop().await?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_restart_native_start_failure_preserves_stop_receipt_and_fails_task() -> TestResult
{
    let _guard = command_smoke_lock().lock().await;
    let fixture = Fixture::new().await?;
    let old = fixture.start().await?;
    // The module's supported launch argument makes the same native executable
    // exit with code 7 on its next launch. The current process keeps running.
    let current = read_instance_details(&fixture.storage.paths, &fixture.instance_id).await?;
    let mut settings: Value = serde_json::from_str(&current.settings_json)?;
    settings["custom_launch_flags"] = json!("--fail-start");
    update_instance(
        &fixture.storage.paths,
        UpdateInstanceInput {
            id: fixture.instance_id.clone(),
            bind_ip: current.summary.bind_ip,
            auto_backup_on_stop: current.auto_backup_on_stop,
            backup_retention_count: current.backup_retention_count,
            settings_json: settings.to_string(),
            ports: current.ports,
        },
    )
    .await?;
    let output = fixture
        .confirm(fixture.preview("restart_server").await?)
        .await?;
    let receipt = output.verification.as_ref().ok_or("failed verification")?;
    assert_eq!(receipt.status, AssistantVerificationStatus::Failed);
    assert_eq!(
        output.task.as_ref().ok_or("task")?.status,
        AssistantTaskStatus::Failed
    );
    assert_eq!(receipt.evidence["stopReceipt"]["stoppedRunId"], old.run_id);
    assert!(
        receipt.evidence["operationError"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    );
    assert!(
        read_instance_details(&fixture.storage.paths, &fixture.instance_id)
            .await?
            .active_run
            .is_none()
    );
    assert!(inspect_process_identity(old.pid)?.is_none());
    assert!(output.follow_up.is_none() && output.continuation.is_none());
    Ok(())
}
