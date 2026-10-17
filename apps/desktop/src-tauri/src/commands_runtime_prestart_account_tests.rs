#![cfg(windows)]

use super::*;
use crate::steam_workshop_account_sessions::SteamAccountLoginTarget;
use std::future::{Future, poll_fn};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::sync::Arc;
use std::task::Poll;
use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject};

const BUDGET: Duration = Duration::from_secs(45);

struct FixtureRoot(PathBuf);

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Named native events gate the actual child, rather than guessing when its
/// output reader, process cleanup, or asynchronous resource locks will run.
struct NativeEvent {
    name: String,
    handle: Arc<OwnedHandle>,
}

impl NativeEvent {
    fn new(label: &str) -> std::io::Result<Self> {
        let name = format!(
            "Local\\LGSM-prestart-{}-{label}",
            uuid::Uuid::new_v4().simple()
        );
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let handle = unsafe { CreateEventW(std::ptr::null(), 1, 0, wide.as_ptr()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self {
            name,
            handle: Arc::new(unsafe { OwnedHandle::from_raw_handle(handle.cast()) }),
        })
    }

    fn signal(&self) -> std::io::Result<()> {
        if unsafe { SetEvent(self.handle.as_raw_handle().cast()) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    async fn wait(&self) -> Result<(), Box<dyn std::error::Error>> {
        let handle = self.handle.clone();
        tokio::task::spawn_blocking(move || {
            match unsafe { WaitForSingleObject(handle.as_raw_handle().cast(), 45_000) } {
                WAIT_OBJECT_0 => Ok(()),
                WAIT_TIMEOUT => Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "The synthetic SteamCMD did not reach its native barrier.",
                )),
                _ => Err(std::io::Error::last_os_error()),
            }
        })
        .await??;
        Ok(())
    }
}

impl Drop for NativeEvent {
    fn drop(&mut self) {
        // Failure paths also wake the bounded waiter and any blocked child.
        let _ = self.signal();
    }
}

struct Fixture {
    storage: StorageBootstrap,
    descriptor: ModuleDescriptor,
    module: ModuleDetails,
    first: InstanceDetails,
    second: InstanceDetails,
    program: PathBuf,
    entered: NativeEvent,
    release: NativeEvent,
    _root: FixtureRoot,
}

impl Fixture {
    async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = FixtureRoot(std::env::temp_dir().join(format!(
            "lgsm-prestart-account-{}",
            uuid::Uuid::new_v4().simple()
        )));
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        let paths = app_storage::StoragePaths {
            app_data_root: root.0.join("app-data"),
            settings_path: root.0.join("app-data/settings.json"),
            database_path: root.0.join("app-data/db/lgs.db"),
            logs_root: root.0.join("app-data/logs"),
            modules_root: workspace.join("modules"),
            migrations_root: workspace.join("migrations"),
            steamcmd_root: root.0.join("steamcmd"),
            games_root: root.0.join("games"),
            instances_root: root.0.join("instances"),
            archives_root: root.0.join("instances/.trash"),
        };
        let storage = app_storage::bootstrap_storage_with_paths(paths)?;
        initialize_database(&storage.paths).await?;
        let mut descriptor = discover_modules(&storage.paths.modules_root)?
            .into_iter()
            .find(|module| module.summary.id == "valheim")
            .unwrap();
        // The shipped Valheim policy is independent. This synthetic descriptor
        // exercises a future Steam-backed shared-program configuration without
        // changing the real catalog or running the game's executable.
        descriptor.storage.program_sharing = app_modules::ModuleProgramSharing::Shared;
        descriptor.runtime.program_sharing = app_core::InstanceProgramMode::Shared;
        sync_modules(&storage.paths, std::slice::from_ref(&descriptor)).await?;
        let program = storage.paths.games_root.join("valheim");
        fs::create_dir_all(&program)?;
        fs::write(
            program.join("valheim_server.exe"),
            b"preserved fixture server",
        )?;
        app_storage::record_library_program_baseline(&program, &descriptor, true, None)?;
        sync_game_installs(
            &storage.paths,
            &[GameInstallSyncRecord {
                module_id: "valheim".into(),
                install_root: program.to_string_lossy().into_owned(),
                install_state: InstallState::Installed,
                current_version: None,
                mark_verified: true,
            }],
        )
        .await?;
        let mut instances = Vec::new();
        for name in ["Prestart account fixture A", "Prestart account fixture B"] {
            let created = app_storage::create_instance_with_options(
                &storage.paths,
                &descriptor,
                CreateInstanceInput {
                    name: name.into(),
                    module_id: "valheim".into(),
                },
                app_storage::InstanceCreationOptions {
                    program_mode: Some(app_core::InstanceProgramMode::Shared),
                    program_install_root: Some(program.clone()),
                    prefer_existing_install: true,
                    require_clean_program: true,
                    ..Default::default()
                },
            )
            .await?;
            let instance =
                read_instance_details(&storage.paths, &created.provisioning.summary.id).await?;
            assert!(app_storage::instance_uses_library_program(
                super::super::commands_program_storage::instance_root(&instance)?
            )?);
            assert_eq!(
                fs::canonicalize(
                    super::super::commands_runtime_lifecycle::private_runtime_install_root(
                        &instance
                    )?
                )?,
                fs::canonicalize(&program)?
            );
            instances.push(instance);
        }
        let second = instances.pop().unwrap();
        let first = instances.pop().unwrap();
        let module = map_module_details_with_install_state(
            &storage.settings,
            &descriptor,
            Some(&program.to_string_lossy()),
        );
        assert!(matches!(
            module.summary.install_state,
            InstallState::Installed
        ));
        let entered = NativeEvent::new("entered")?;
        let release = NativeEvent::new("release")?;
        fs::create_dir_all(&storage.paths.steamcmd_root)?;
        fs::write(
            storage.paths.steamcmd_root.join("barriers"),
            format!("{}\n{}", entered.name, release.name),
        )?;
        compile_steamcmd(&storage.paths.steamcmd_root).await?;
        Ok(Self {
            storage,
            descriptor,
            module,
            first,
            second,
            program,
            entered,
            release,
            _root: root,
        })
    }
}

async fn compile_steamcmd(directory: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let source = directory.join("fixture.cs");
    fs::write(
        &source,
        r#"
using System;
using System.IO;
using System.Threading;
public static class Installer {
  public static int Main(string[] args) {
    int script = Array.IndexOf(args, "+runscript");
    if (script < 0) {
      Console.WriteLine("Steam Console Client (c) Valve Corporation\nLoading Steam API...OK");
      return 0;
    }
    string root = null, login = null;
    foreach (string line in File.ReadAllLines(args[script + 1])) {
      if (line.StartsWith("force_install_dir ")) root = line.Substring(18).Trim().Trim('"');
      if (line.StartsWith("login ")) login = line.Trim();
    }
    if (root == null || login == null) return 80;
    File.AppendAllText("native-attempts", login + "\n");
    if (login != "login anonymous") {
      File.WriteAllText(Path.Combine(root, "unexpected-account-attempt"), "must not run");
      return 81;
    }
    string[] barriers = File.ReadAllLines("barriers");
    using (EventWaitHandle entered = EventWaitHandle.OpenExisting(barriers[0]))
    using (EventWaitHandle release = EventWaitHandle.OpenExisting(barriers[1])) {
      entered.Set();
      if (!release.WaitOne(TimeSpan.FromSeconds(45))) return 82;
    }
    Console.WriteLine("ERROR! Failed to install app '896660' (No subscription)");
    return 0;
  }
}
"#,
    )?;
    let executable = directory.join("steamcmd.exe");
    let framework = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("Microsoft.NET/Framework64/v4.0.30319/csc.exe");
    let output = tokio::task::spawn_blocking(move || -> std::io::Result<_> {
        let mut child = std::process::Command::new(framework)
            .args(["/nologo", "/target:exe"])
            .arg(format!("/out:{}", executable.display()))
            .arg(source)
            .creation_flags(0x0800_0000)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        if unsafe { WaitForSingleObject(child.as_raw_handle().cast(), 45_000) } != WAIT_OBJECT_0 {
            let _ = child.kill();
            child.wait()?;
            return Err(std::io::Error::other(
                "Synthetic SteamCMD compilation did not finish.",
            ));
        }
        child.wait_with_output()
    })
    .await??;
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[derive(Clone, Copy)]
enum Handoff {
    SharedInstanceStarted,
    AuthorizationRevoked,
}

async fn check_handoff(handoff: Handoff) -> Result<(), Box<dyn std::error::Error>> {
    let _serial = crate::commands::tests::command_smoke_lock().lock().await;
    let fixture = Fixture::new().await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&fixture.storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    app_steamcmd::ensure_steamcmd_installed(&state.install_locks, &fixture.storage.settings)
        .await?;
    let target = SteamAccountLoginTarget::Instance(fixture.first.summary.id.clone());
    let (sender, _input) = tokio::sync::mpsc::channel(1);
    let sessions = &state.steam_workshop_account_sessions;
    sessions.begin(
        &target,
        "prestart-fixture",
        sender,
        &fixture.storage.paths.app_data_root,
    )?;
    sessions.finish(
        "prestart-fixture",
        Ok(app_steamcmd::SteamWorkshopAccountAuthenticationResult {
            account_name: "synthetic_account".into(),
            steamcmd_root: fixture
                .storage
                .paths
                .steamcmd_root
                .to_string_lossy()
                .into_owned(),
        }),
    );
    let authorization = sessions.authorize_account(
        &target,
        "synthetic_account",
        &fixture.storage.paths.steamcmd_root,
        &fixture.storage.paths.app_data_root,
    )?;
    sessions.record_server_download(
        &target,
        "synthetic_account",
        &authorization,
        "valheim",
        fs::canonicalize(&fixture.program)?,
    );
    let id = &fixture.first.summary.id;
    let reservation = match state.try_reserve_runtime_start(id, "manual")? {
        RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
        other => panic!("unexpected start reservation: {other:?}"),
    };
    let _instance = state.acquire_instance_mutation(id).await;
    let roots = [fixture.program.clone()];
    let guard =
        app_steamcmd::acquire_game_install_lifecycle(&state.install_locks, "valheim", &roots)
            .await?;
    let mut update = Box::pin(run_prestart_update_if_needed(
        &state,
        &fixture.storage,
        PrestartUpdateRequest {
            descriptor: &fixture.descriptor,
            module: &fixture.module,
            instance: &fixture.first,
            install_guard: guard,
            reservation: &reservation,
            context: PrestartUpdateLogContext {
                source: "manual",
                console_log_path: None,
            },
        },
    ));
    tokio::select! {
        result = &mut update => panic!("prestart ended before the native barrier: {}", result.err().unwrap_or_default()),
        ready = fixture.entered.wait() => ready?,
    }
    // The module resource is the first exclusive gate, and its Tokio mutex is
    // FIFO. Polling this pending waiter while A owns the gate queues B before
    // A's fallback; no production callback or timing delay controls the handoff.
    let mut next_owner = Box::pin(app_steamcmd::acquire_game_install_lifecycle(
        &state.install_locks,
        "valheim",
        &roots,
    ));
    poll_fn(|context| {
        assert!(next_owner.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    fixture.release.signal()?;
    let held = tokio::time::timeout(BUDGET, async {
        tokio::select! {
            guard = &mut next_owner => guard,
            result = &mut update => panic!("fallback bypassed the queued program owner: {}", result.err().unwrap_or_default()),
        }
    }).await??;
    poll_fn(|context| {
        assert!(update.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    match handoff {
        Handoff::SharedInstanceStarted => {
            app_storage::mark_instance_process_started_with_identity(
                &fixture.storage.paths,
                &app_storage::StartedInstanceProcess {
                    instance_id: &fixture.second.summary.id,
                    session_id: Some("synthetic-shared-run"),
                    process_key: "primary",
                    display_name: "Synthetic shared run",
                    pid: 4242,
                    log_path: "synthetic-shared-run.log",
                    is_primary: true,
                },
                None,
            )
            .await?;
        }
        Handoff::AuthorizationRevoked => sessions.clear_server_download(&target)?,
    }
    drop(held);
    let failure = tokio::time::timeout(BUDGET, &mut update)
        .await?
        .err()
        .expect("handoff must prevent account retry");
    match handoff {
        Handoff::SharedInstanceStarted => {
            assert!(failure.contains(&fixture.second.summary.name), "{failure}")
        }
        Handoff::AuthorizationRevoked => {
            assert!(failure.contains("steam_app_account_required"), "{failure}")
        }
    }
    assert_eq!(
        fs::read_to_string(fixture.storage.paths.steamcmd_root.join("native-attempts"))?,
        "login anonymous\n"
    );
    assert_eq!(
        fs::read(fixture.program.join("valheim_server.exe"))?,
        b"preserved fixture server"
    );
    assert!(!fixture.program.join("unexpected-account-attempt").exists());
    assert!(
        read_active_instance_run(&fixture.storage.paths, id)
            .await?
            .is_none()
    );
    let jobs = state.app_state.read().unwrap().jobs.clone();
    let job = jobs
        .iter()
        .find(|job| job.target_id.as_deref() == Some(id))
        .unwrap();
    assert!(matches!(job.status, JobStatus::Failed));
    assert!(!job.cancellable);
    assert!(!state.install_operations.request_cancel(&job.id)?);
    let _reacquired = tokio::time::timeout(
        BUDGET,
        app_steamcmd::acquire_game_install_lifecycle(&state.install_locks, "valheim", &roots),
    )
    .await??;
    Ok(())
}

#[tokio::test]
async fn prestart_account_handoff_rechecks_a_shared_run_before_a_second_native_attempt()
-> Result<(), Box<dyn std::error::Error>> {
    check_handoff(Handoff::SharedInstanceStarted).await
}

#[tokio::test]
async fn prestart_account_handoff_rechecks_revocation_after_the_program_lock_wait()
-> Result<(), Box<dyn std::error::Error>> {
    check_handoff(Handoff::AuthorizationRevoked).await
}
