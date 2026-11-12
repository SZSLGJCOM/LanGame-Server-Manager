#![cfg(windows)]

use super::*;
use std::os::windows::process::CommandExt;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const BUDGET: Duration = Duration::from_secs(45);
// Independent depot wire fixture: app/depot 7, manifest 9, build 17, x.bin = abc.
const DEPOT: &str = "d017f671230000000a210a05782e62696e100318002a14a9993e364706816aba3e25717850c26c9cd0d89dbe12481f0d000000080710092000280348b8e1fe5317b8811b00000000ab15c432";
const COMPLETE: &str = r#""AppState" { "appid" "7" "StateFlags" "4" "buildid" "17" "TargetBuildID" "17" "InstalledDepots" { "7" { "manifest" "9" } } }"#;

struct Fixture {
    root: PathBuf,
    storage: StorageBootstrap,
    descriptor: ModuleDescriptor,
    instance: InstanceDetails,
    program: PathBuf,
    library: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl Fixture {
    async fn new(shared: bool) -> TestResult<Self> {
        // The legacy .NET installer fixture shares the managed scratch prefix;
        // keep its nested manifest path within the Windows MAX_PATH limit.
        let root = std::env::temp_dir().join(format!("repair-{}", uuid::Uuid::new_v4().simple()));
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        let paths = app_storage::StoragePaths {
            app_data_root: root.join("app-data"),
            settings_path: root.join("app-data/settings.json"),
            database_path: root.join("app-data/db/lgs.db"),
            logs_root: root.join("app-data/logs"),
            modules_root: workspace.join("modules"),
            migrations_root: workspace.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances/.trash"),
        };
        let storage = app_storage::bootstrap_storage_with_paths(paths)?;
        initialize_database(&storage.paths).await?;
        let mut descriptor = discover_modules(&storage.paths.modules_root)?
            .into_iter()
            .find(|module| module.summary.id == "valheim")
            .unwrap();
        descriptor.summary.steam_app_id = Some(7);
        descriptor.process.as_mut().unwrap().executable = "x.bin".into();
        if shared {
            descriptor.storage.program_sharing = app_modules::ModuleProgramSharing::Shared;
            descriptor.runtime.program_sharing = app_core::InstanceProgramMode::Shared;
        }
        sync_modules(&storage.paths, std::slice::from_ref(&descriptor)).await?;
        let library = storage.paths.games_root.join("valheim");
        fs::create_dir_all(library.join("steamapps"))?;
        fs::write(library.join("x.bin"), b"abc")?;
        fs::write(library.join("steamapps/appmanifest_7.acf"), COMPLETE)?;
        app_storage::record_library_program_baseline(&library, &descriptor, true, None)?;
        sync_game_installs(
            &storage.paths,
            &[GameInstallSyncRecord {
                module_id: "valheim".into(),
                install_root: library.to_string_lossy().into_owned(),
                install_state: InstallState::Installed,
                current_version: Some("17".into()),
                mark_verified: true,
            }],
        )
        .await?;
        let created = app_storage::create_instance_with_options(
            &storage.paths,
            &descriptor,
            CreateInstanceInput {
                name: "Repair".into(),
                module_id: "valheim".into(),
            },
            app_storage::InstanceCreationOptions {
                program_mode: Some(if shared {
                    app_core::InstanceProgramMode::Shared
                } else {
                    app_core::InstanceProgramMode::Independent
                }),
                program_install_root: shared.then(|| library.clone()),
                prefer_existing_install: shared,
                require_clean_program: true,
                ..Default::default()
            },
        )
        .await?;
        let instance =
            read_instance_details(&storage.paths, &created.provisioning.summary.id).await?;
        let program = PathBuf::from(
            super::super::commands_runtime_lifecycle::private_runtime_install_root(&instance)?,
        );
        assert_eq!(
            fs::canonicalize(&program)? == fs::canonicalize(&library)?,
            shared,
        );
        fs::create_dir_all(&instance.saves_path)?;
        fs::write(
            Path::new(&instance.saves_path).join("world.sentinel"),
            b"existing world",
        )?;
        fs::create_dir_all(storage.paths.steamcmd_root.join("depotcache"))?;
        let depot = DEPOT
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();
        fs::write(
            storage.paths.steamcmd_root.join("depotcache/7_9.manifest"),
            depot,
        )?;
        compile_steamcmd(&storage.paths.steamcmd_root).await?;
        Ok(Self {
            root,
            storage,
            descriptor,
            instance,
            program,
            library,
        })
    }

    fn module(&self) -> ModuleDetails {
        map_module_details_with_install_state(
            &self.storage.settings,
            &self.descriptor,
            Some(&self.program.to_string_lossy()),
        )
    }

    fn damage(&self, incomplete: bool) -> TestResult {
        if incomplete {
            fs::write(self.program.join("x.bin"), b"xyz")?;
            fs::write(
                self.program.join("steamapps/appmanifest_7.acf"),
                COMPLETE
                    .replace("\"StateFlags\" \"4\"", "\"StateFlags\" \"6\"")
                    .replacen(
                        '{',
                        "{ \"BytesToDownload\" \"50\" \"BytesDownloaded\" \"1\"",
                        1,
                    ),
            )?;
        } else {
            // Same build identity, but its required program file is missing.
            fs::remove_file(self.program.join("x.bin"))?;
        }
        assert_eq!(
            self.module().summary.install_state,
            if incomplete {
                InstallState::Incomplete
            } else {
                InstallState::Corrupted
            },
        );
        Ok(())
    }
}

async fn compile_steamcmd(directory: &Path) -> TestResult {
    let source = directory.join("fixture.cs");
    fs::write(
        &source,
        r#"
using System;
using System.IO;
public static class Installer {
  public static int Main(string[] args) {
    int script = Array.IndexOf(args, "+runscript");
    if (script < 0) {
      Console.WriteLine("Steam Console Client (c) Valve Corporation\nLoading Steam API...OK");
      return 0;
    }
    string root = null, update = null;
    foreach (string line in File.ReadAllLines(args[script + 1])) {
      if (line.StartsWith("force_install_dir ")) root = line.Substring(18).Trim().Trim('"');
      if (line.StartsWith("app_update ")) update = line.Trim();
    }
    if (root == null || update == null) return 80;
    File.AppendAllText("native-commands", update + "\n");
    if (File.Exists("reject-repair")) {
      Console.WriteLine("ERROR! Failed to install app '7' (No subscription)");
      return 0;
    }
    Directory.CreateDirectory(Path.Combine(root, "steamapps"));
    // A normal same-build update leaves damaged bytes alone. Only a real
    // validate request repairs them, just as the startup caller must request.
    if (update == "app_update 7 validate" && !File.Exists("ignore-validation"))
      File.WriteAllText(Path.Combine(root, "x.bin"), "abc");
    File.WriteAllText(Path.Combine(root, "steamapps/appmanifest_7.acf"),
      "\"AppState\" { \"appid\" \"7\" \"StateFlags\" \"4\" \"buildid\" \"17\" \"TargetBuildID\" \"17\" \"InstalledDepots\" { \"7\" { \"manifest\" \"9\" } } }");
    Console.WriteLine("Success! App '7' fully installed.");
    return 0;
  }
}
"#,
    )?;
    let executable = directory.join("steamcmd.exe");
    let framework = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("Microsoft.NET/Framework64/v4.0.30319/csc.exe");
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(framework)
            .args(["/nologo", "/target:exe"])
            .arg(format!("/out:{}", executable.display()))
            .arg(source)
            .creation_flags(0x0800_0000)
            .output()
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

async fn check_repair(incomplete: bool, pinned: bool, reject: bool) -> TestResult {
    let _serial = crate::commands::tests::command_smoke_lock().lock().await;
    let mut fixture = Fixture::new(false).await?;
    fixture.damage(incomplete)?;
    if pinned {
        let mut settings: Value = serde_json::from_str(&fixture.instance.settings_json)?;
        settings["program_update"] = json!({"policy": "pinned"});
        fixture.instance.settings_json = serde_json::to_string(&settings)?;
    }
    if reject {
        fs::write(
            fixture.storage.paths.steamcmd_root.join("reject-repair"),
            b"",
        )?;
    }
    let module = fixture.module();
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&fixture.storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    app_steamcmd::ensure_steamcmd_installed(&state.install_locks, &fixture.storage.settings)
        .await?;
    let id = &fixture.instance.summary.id;
    let reservation = match state.try_reserve_runtime_start(id, "manual")? {
        RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
        other => panic!("unexpected start reservation: {other:?}"),
    };
    let _instance = state.acquire_instance_mutation(id).await;
    let guard = app_steamcmd::acquire_game_install_lifecycle(
        &state.install_locks,
        "valheim",
        std::slice::from_ref(&fixture.program),
    )
    .await?;
    let result = tokio::time::timeout(
        BUDGET,
        run_prestart_update_if_needed(
            &state,
            &fixture.storage,
            PrestartUpdateRequest {
                descriptor: &fixture.descriptor,
                module: &module,
                instance: &fixture.instance,
                install_guard: guard,
                reservation: &reservation,
                context: PrestartUpdateLogContext {
                    source: "manual",
                    console_log_path: None,
                },
            },
        ),
    )
    .await
    .expect("startup repair must settle");
    let after = fixture.module();
    let commands = fixture.storage.paths.steamcmd_root.join("native-commands");
    if pinned {
        assert!(
            result.is_ok(),
            "Pinned policy skips mutation: {:?}",
            result.err()
        );
        assert!(!commands.exists());
        assert!(state.app_state.read().unwrap().jobs.is_empty());
        assert!(ensure_module_installed_for_start(&after).is_err());
    } else {
        let commands = fs::read_to_string(commands)?;
        assert!(
            !commands.is_empty(),
            "the real updater must receive the repair"
        );
        assert!(
            commands.lines().all(|line| line == "app_update 7 validate"),
            "{commands}"
        );
        let jobs = state.app_state.read().unwrap().jobs.clone();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].target_id.as_deref(), Some(id.as_str()));
        if reject {
            assert!(result.is_err(), "a failed repair cannot authorize startup");
            assert!(matches!(jobs[0].status, JobStatus::Failed));
            assert!(ensure_module_installed_for_start(&after).is_err());
        } else {
            let outcome = result.map_err(|error| format!("repair failed: {error}"))?;
            outcome
                .install_guard
                .ensure_scope("valheim", &fixture.program)?;
            assert!(outcome.warnings.is_empty());
            assert!(matches!(jobs[0].status, JobStatus::Completed));
            assert_eq!(after.summary.install_state, InstallState::Installed);
            ensure_module_installed_for_start(&after)?;
            assert_eq!(fs::read(fixture.program.join("x.bin"))?, b"abc");
            app_steamcmd::read_program_install_revision(
                &fixture.storage.paths.instances_root,
                "valheim",
                &fixture.program,
            )?;
            assert!(app_storage::library_program_is_pristine(
                &fixture.program,
                &fixture.descriptor,
                None,
            )?);
        }
    }
    assert_eq!(fs::read(fixture.library.join("x.bin"))?, b"abc");
    assert_eq!(
        fs::read_to_string(fixture.library.join("steamapps/appmanifest_7.acf"))?,
        COMPLETE
    );
    assert_eq!(
        fs::read(Path::new(&fixture.instance.saves_path).join("world.sentinel"))?,
        b"existing world"
    );
    assert!(
        read_active_instance_run(&fixture.storage.paths, id)
            .await?
            .is_none()
    );
    assert!(!state.runtime_supervisor.lock().unwrap().is_tracked(id));
    Ok(())
}

#[tokio::test]
async fn automatic_prestart_repairs_incomplete_and_same_build_corrupted_programs() -> TestResult {
    check_repair(true, false, false).await?;
    check_repair(false, false, false).await
}

#[tokio::test]
async fn pinned_prestart_keeps_broken_program_unstartable_without_downloading() -> TestResult {
    check_repair(false, true, false).await
}

#[tokio::test]
async fn failed_prestart_repair_does_not_authorize_partial_program_startup() -> TestResult {
    check_repair(true, false, true).await
}

#[tokio::test]
async fn automatic_prestart_repairs_shared_program_before_private_detachment() -> TestResult {
    use super::super::commands_save_startup;

    let _serial = crate::commands::tests::command_smoke_lock().lock().await;
    let fixture = Fixture::new(true).await?;
    fixture.damage(true)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&fixture.storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    app_steamcmd::ensure_steamcmd_installed(&state.install_locks, &fixture.storage.settings)
        .await?;
    let id = &fixture.instance.summary.id;
    let reservation = match state.try_reserve_runtime_start(id, "manual")? {
        RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
        other => panic!("unexpected start reservation: {other:?}"),
    };
    let instance_guard = state.acquire_instance_mutation(id).await;
    let roots = commands_save_startup::program_roots(&fixture.instance, &fixture.program)?;
    assert_eq!(
        roots.len(),
        2,
        "startup must reserve both shared and private roots"
    );
    let install_guard =
        app_steamcmd::acquire_game_install_lifecycle(&state.install_locks, "valheim", &roots)
            .await?;
    let (guards, repaired) = tokio::time::timeout(
        BUDGET,
        commands_save_startup::repair_shared_program_before_detach(
            &state,
            &fixture.storage,
            &fixture.descriptor,
            &fixture.instance,
            &reservation,
            (instance_guard, Some(install_guard)),
            PrestartUpdateLogContext {
                source: "manual",
                console_log_path: None,
            },
        ),
    )
    .await
    .expect("shared repair must settle")?;
    assert!(
        repaired.is_some(),
        "caller must skip a second program update after detachment"
    );
    assert!(repaired.unwrap().is_empty());
    assert_eq!(
        fixture.module().summary.install_state,
        InstallState::Installed
    );
    let (guards, detached) = commands_save_startup::ensure_private_program(
        &fixture.storage,
        &fixture.descriptor,
        fixture.instance.clone(),
        &reservation,
        guards,
    )
    .await?;
    let private_root = PathBuf::from(
        super::super::commands_runtime_lifecycle::private_runtime_install_root(&detached)?,
    );
    assert_ne!(
        fs::canonicalize(&private_root)?,
        fs::canonicalize(&fixture.library)?
    );
    for root in [&fixture.library, &private_root] {
        guards.1.as_ref().unwrap().ensure_scope("valheim", root)?;
        assert_eq!(fs::read(root.join("x.bin"))?, b"abc");
    }
    let module = map_module_details_with_install_state(
        &fixture.storage.settings,
        &fixture.descriptor,
        Some(&private_root.to_string_lossy()),
    );
    ensure_module_installed_for_start(&module)?;
    assert_eq!(module.summary.install_state, InstallState::Installed);
    assert_eq!(
        fs::read_to_string(fixture.storage.paths.steamcmd_root.join("native-commands"))?,
        "app_update 7 validate\n",
    );
    assert_eq!(
        fs::read(Path::new(&detached.saves_path).join("world.sentinel"))?,
        b"existing world",
    );
    assert!(
        read_active_instance_run(&fixture.storage.paths, id)
            .await?
            .is_none()
    );
    Ok(())
}

#[path = "commands_runtime_prestart_cross_module_tests.rs"]
mod cross_module;

#[path = "commands_runtime_prestart_attestation_tests.rs"]
mod attestation;
