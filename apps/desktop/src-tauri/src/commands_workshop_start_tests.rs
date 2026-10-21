use super::*;
use tauri::Manager;

const ITEM: &str = "123456789";
const INFO: &str = r#"{"PackageName":"FixtureMod"}"#;

struct Fixture {
    root: PathBuf,
    storage: StorageBootstrap,
    descriptor: ModuleDescriptor,
    instance: InstanceDetails,
    program: PathBuf,
    target: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl Fixture {
    async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "lgsm-workshop-start-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .unwrap();
        let storage = app_storage::bootstrap_storage_with_paths(app_storage::StoragePaths {
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
        })?;
        initialize_database(&storage.paths).await?;
        let descriptor = discover_modules(&storage.paths.modules_root)?
            .into_iter()
            .find(|module| module.summary.id == "palworld")
            .unwrap();
        sync_modules(&storage.paths, std::slice::from_ref(&descriptor)).await?;
        let library = storage.paths.games_root.join("palworld");
        fs::create_dir_all(library.join("Pal/Binaries/Win64"))?;
        fs::write(
            library.join("Pal/Binaries/Win64/PalServer-Win64-Shipping-Cmd.exe"),
            b"inert server fixture",
        )?;
        app_storage::record_library_program_baseline(&library, &descriptor, true, None)?;
        sync_game_installs(
            &storage.paths,
            &[GameInstallSyncRecord {
                module_id: "palworld".into(),
                install_root: library.to_string_lossy().into_owned(),
                install_state: InstallState::Installed,
                current_version: None,
                mark_verified: true,
            }],
        )
        .await?;
        let created = app_storage::create_instance_with_options(
            &storage.paths,
            &descriptor,
            CreateInstanceInput {
                name: "Workshop startup fixture".into(),
                module_id: "palworld".into(),
            },
            app_storage::InstanceCreationOptions {
                program_mode: Some(app_core::InstanceProgramMode::Independent),
                prefer_existing_install: false,
                require_clean_program: true,
                ..Default::default()
            },
        )
        .await?;
        let instance =
            read_instance_details(&storage.paths, &created.provisioning.summary.id).await?;
        let program = PathBuf::from(
            super::super::super::commands_runtime_lifecycle::private_runtime_install_root(
                &instance,
            )?,
        );
        let mut settings: Value = serde_json::from_str(&instance.settings_json)?;
        settings["mod_package_names"] = json!("FixtureMod");
        settings["program_update"] = json!({"policy":"pinned"});
        let instance = app_storage::update_instance(
            &storage.paths,
            UpdateInstanceInput {
                id: instance.summary.id.clone(),
                bind_ip: instance.summary.bind_ip.clone(),
                auto_backup_on_stop: instance.auto_backup_on_stop,
                backup_retention_count: instance.backup_retention_count,
                settings_json: serde_json::to_string(&settings)?,
                ports: instance.ports.clone(),
            },
        )
        .await?;
        let target = program.join("Pal/Binaries/Win64/Mods/Workshop");
        let cache = program.join(format!("steamapps/workshop/content/1623730/{ITEM}"));
        for directory in [&cache, &target.join(ITEM)] {
            fs::create_dir_all(directory)?;
            fs::write(directory.join("Info.json"), INFO)?;
            fs::write(directory.join("old.pak"), "cached")?;
        }
        fs::create_dir_all(target.join("999999999"))?;
        fs::write(
            target.join("999999999/Info.json"),
            r#"{"PackageName":"DisabledMod"}"#,
        )?;
        fs::write(target.join("999999999/keep.pak"), "disabled")?;
        fs::write(
            program.join("steamapps/workshop/appworkshop_1623730.acf"),
            format!(
                "\"AppWorkshop\" {{ \"appid\" \"1623730\" \"WorkshopItemsInstalled\" {{ \"{ITEM}\" {{ \"manifest\" \"1\" \"size\" \"{}\" }} }} }}",
                INFO.len() + 6
            ),
        )?;
        fs::create_dir_all(&storage.paths.steamcmd_root)?;
        fs::write(
            storage.paths.steamcmd_root.join("install-root"),
            program.to_string_lossy().as_bytes(),
        )?;
        compile_native(&storage.paths.steamcmd_root).await?;
        Ok(Self {
            root,
            storage,
            descriptor,
            instance,
            program,
            target,
        })
    }

    async fn guard(
        &self,
        state: &DesktopState,
    ) -> Result<app_steamcmd::GameInstallLifecycleGuard, String> {
        app_steamcmd::acquire_game_install_lifecycle(
            &state.install_locks,
            "palworld",
            std::slice::from_ref(&self.program),
        )
        .await
        .map_err(|error| error.to_string())
    }
}

async fn compile_native(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let source = root.join("fixture.cs");
    fs::write(
        &source,
        r#"
using System;
using System.IO;
using System.Threading;
public static class WorkshopStartFixture {
  private static string NativePath(string path) {
    string absolute = Path.GetFullPath(path);
    return absolute.StartsWith(@"\\?\", StringComparison.Ordinal) ? absolute : @"\\?\" + absolute;
  }
  public static int Main(string[] args) {
    Console.WriteLine("Steam Console Client (c) Valve Corporation - fixture\nLoading Steam API...OK");
    int index = Array.IndexOf(args, "+runscript");
    if (index < 0) return 0;
    string script = File.ReadAllText(NativePath(args[index+1]));
    File.WriteAllText("captured-script", script);
    if (!script.Contains("workshop_download_item 1623730 123456789 validate")) return 81;
    string workshop = Path.Combine(NativePath(File.ReadAllText("install-root")), "steamapps", "workshop");
    string item = Path.Combine(workshop, "content", "1623730", "123456789");
    if (File.Exists("fail")) { Console.WriteLine("ERROR! Download item 123456789 failed (No Connection)."); return 0; }
    if (File.Exists("unchanged")) { Console.WriteLine("Success. Downloaded item 123456789 to cache"); return 0; }
    if (File.Exists("pause-for-target-change")) {
      File.WriteAllText("entered", "ready"); Console.Out.Flush();
      while (!File.Exists("resume")) Thread.Sleep(10);
    }
    if (File.Exists("partial")) {
      File.WriteAllText(Path.Combine(item, "old.pak"), "broken");
      File.WriteAllText(Path.Combine(item, "partial.pak"), "partial");
      File.WriteAllText(Path.Combine(workshop, "appworkshop_1623730.acf"), "partial download metadata");
      Console.WriteLine("ERROR! Download item 123456789 failed (No Connection)."); return 0;
    }
    if (File.Exists("stall")) {
      File.WriteAllText(Path.Combine(item, "old.pak"), "broken");
      File.WriteAllText("entered", "ready");
      Console.Out.Flush(); new ManualResetEvent(false).WaitOne(); return 0;
    }
    if (File.Exists("renamed")) File.WriteAllText(Path.Combine(item, "Info.json"), "{\"PackageName\":\"RenamedFixtureMod\"}");
    File.Delete(Path.Combine(item, "old.pak"));
    File.WriteAllText(Path.Combine(item, "new.pak"), "latest");
    long size = new FileInfo(Path.Combine(item, "Info.json")).Length + 6;
    File.WriteAllText(Path.Combine(workshop, "appworkshop_1623730.acf"),
      "\"AppWorkshop\" { \"appid\" \"1623730\" \"WorkshopItemsInstalled\" { \"123456789\" { \"manifest\" \"2\" \"size\" \"" + size + "\" } } }");
    Console.WriteLine("Success. Downloaded item 123456789 to cache"); return 0;
  }
}
"#,
    )?;
    // Framework csc targets legacy path handling by default. Keep this native
    // fixture usable at the same long managed-test paths as the Rust caller.
    fs::write(
        root.join("steamcmd.exe.config"),
        r#"<configuration><runtime><AppContextSwitchOverrides value="Switch.System.IO.UseLegacyPathHandling=false;Switch.System.IO.BlockLongPaths=false"/></runtime></configuration>"#,
    )?;
    let compiler = PathBuf::from(std::env::var_os("SystemRoot").ok_or("SystemRoot is missing")?)
        .join("Microsoft.NET/Framework64/v4.0.30319/csc.exe");
    let mut command = tokio::process::Command::new(compiler);
    command
        .creation_flags(0x0800_0000)
        .kill_on_drop(true)
        .args(["/nologo", "/target:exe"])
        .arg(format!("/out:{}", root.join("steamcmd.exe").display()))
        .arg(source);
    let output = tokio::time::timeout(Duration::from_secs(30), command.output()).await??;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(())
}

async fn exercise(fault: &str, cancel: bool) -> Result<(), Box<dyn std::error::Error>> {
    let _serial = crate::commands::tests::command_smoke_lock().lock().await;
    let fixture = Fixture::new().await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&fixture.storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    app_steamcmd::ensure_steamcmd_installed(&state.install_locks, &fixture.storage.settings)
        .await?;
    let modified_config = match fault {
        "modified-shipped-config" => Some("Config/user.ini"),
        "nested-shipped-config" => Some("Config/Info.json"),
        _ => None,
    };
    if let Some(relative) = modified_config {
        let cache = fixture
            .program
            .join(format!("steamapps/workshop/content/1623730/{ITEM}"));
        for directory in [&cache, &fixture.target.join(ITEM)] {
            fs::create_dir_all(directory.join("Config"))?;
            fs::write(directory.join(relative), "default")?;
        }
        fs::write(
            fixture
                .program
                .join("steamapps/workshop/appworkshop_1623730.acf"),
            format!(
                r#""AppWorkshop" {{ "appid" "1623730" "WorkshopItemsInstalled" {{ "{ITEM}" {{ "manifest" "1" "size" "{}" }} }} }}"#,
                INFO.len() + 6 + 7
            ),
        )?;
    }
    assert!(
        inspect_workshop_items(
            &fixture.storage.settings,
            1623730,
            &fixture.program,
            &[ITEM.into()]
        )?
        .items
        .iter()
        .any(|item| item.item_id == ITEM && item.installed)
    );
    // Boundary fixture for a prior successful native validation. The production
    // receipt writer hashes the real old files; the failing run cannot certify them.
    {
        let guard = fixture.guard(&state).await?;
        let steamcmd = guard.acquire_steamcmd(&fixture.storage.settings).await?;
        app_steamcmd::record_validated_workshop_items(
            &SteamWorkshopDownloadResult {
                consumer_app_id: 1623730,
                install_root: fixture.program.to_string_lossy().into_owned(),
                workshop_root: fixture
                    .program
                    .join("steamapps/workshop/content/1623730")
                    .to_string_lossy()
                    .into_owned(),
                items: vec![app_steamcmd::SteamWorkshopDownloadItemResult {
                    item_id: ITEM.into(),
                    expected_path: fixture
                        .program
                        .join(format!("steamapps/workshop/content/1623730/{ITEM}"))
                        .to_string_lossy()
                        .into_owned(),
                    expected_path_exists: true,
                }],
                output_excerpt: "simulated prior native validate success".into(),
            },
            &guard,
            &steamcmd,
            &fixture.storage.settings,
            &app_steamcmd::InstallCancellation::new(),
        )?;
    }
    if matches!(
        fault,
        "network"
            | "network-user-data"
            | "corrupt-cache"
            | "missing-cache"
            | "corrupt-deployment"
            | "missing-deployment"
    ) {
        fs::write(fixture.storage.paths.steamcmd_root.join("fail"), "yes")?;
    }
    if let Some(relative) = modified_config {
        fs::write(
            fixture.target.join(ITEM).join(relative),
            "operator configuration",
        )?;
        fs::write(fixture.storage.paths.steamcmd_root.join("fail"), "yes")?;
    }
    if fault == "unknown-loadable" {
        fs::write(
            fixture.target.join(format!("{ITEM}/unknown.pak")),
            "keep unverified original payload",
        )?;
        fs::write(fixture.storage.paths.steamcmd_root.join("fail"), "yes")?;
    }
    if fault == "late-config" {
        fs::write(
            fixture
                .storage
                .paths
                .steamcmd_root
                .join("pause-for-target-change"),
            "yes",
        )?;
    }
    if matches!(fault, "partial" | "renamed") {
        fs::write(fixture.storage.paths.steamcmd_root.join(fault), "yes")?;
    }
    if fault == "renamed-user-data" {
        fs::write(fixture.storage.paths.steamcmd_root.join("renamed"), "yes")?;
    }
    if fault == "unchanged-user-data" {
        fs::write(fixture.storage.paths.steamcmd_root.join("unchanged"), "yes")?;
    }
    if fault.ends_with("user-data") {
        fs::create_dir_all(fixture.target.join(format!("{ITEM}/Config")))?;
        fs::write(
            fixture.target.join(format!("{ITEM}/Config/user.ini")),
            "operator configuration",
        )?;
        fs::write(
            fixture.target.join(format!("{ITEM}/user-note.txt")),
            "keep original note",
        )?;
    }
    if fault == "corrupt-cache" {
        fs::write(
            fixture
                .program
                .join(format!("steamapps/workshop/content/1623730/{ITEM}/old.pak")),
            "BROKEN",
        )?;
    }
    if fault == "missing-cache" {
        fs::remove_file(
            fixture
                .program
                .join(format!("steamapps/workshop/content/1623730/{ITEM}/old.pak")),
        )?;
    }
    if fault == "corrupt-deployment" {
        fs::write(fixture.target.join(format!("{ITEM}/old.pak")), "BROKEN")?;
    }
    if matches!(fault, "missing-deployment" | "missing-deployment-success") {
        fs::remove_dir_all(fixture.target.join(ITEM))?;
    }
    if cancel {
        fs::write(fixture.storage.paths.steamcmd_root.join("stall"), "yes")?;
    }
    let id = &fixture.instance.summary.id;
    let reservation = match state.try_reserve_runtime_start(id, "manual")? {
        RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
        other => panic!("unexpected reservation: {other:?}"),
    };
    let _instance = state.acquire_instance_mutation(id).await;
    let enablement_path = fixture
        .program
        .join("Pal/Binaries/Win64/Mods/PalModSettings.ini");
    let enablement_before = fs::read(&enablement_path)?;
    let guard = fixture.guard(&state).await?;
    let mut update = Box::pin(update_before_start(
        &state,
        &fixture.storage,
        WorkshopStartRequest {
            descriptor: &fixture.descriptor,
            instance: &fixture.instance,
            install_guard: guard,
            reservation: &reservation,
            context:
                super::super::super::commands_runtime_prestart_update::PrestartUpdateLogContext {
                    source: "manual",
                    console_log_path: None,
                },
        },
    ));
    if cancel || fault == "late-config" {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                tokio::select! {
                    result = &mut update => {
                        let detail = match result {
                            Ok(outcome) => format!("warnings: {:?}", outcome.warnings),
                            Err(error) => error,
                        };
                        panic!("update finished before cancellation: {detail}; jobs: {:?}", state.app_state.read().unwrap().jobs);
                    },
                    _ = tokio::time::sleep(Duration::from_millis(10)) => {
                        if fixture.storage.paths.steamcmd_root.join("entered").exists() { break; }
                    }
                }
            }
        }).await?;
        if fault == "late-config" {
            fs::create_dir(fixture.target.join(format!("{ITEM}/Config")))?;
            fs::write(
                fixture.target.join(format!("{ITEM}/Config/user.ini")),
                "keep configuration added during native download",
            )?;
            fs::write(fixture.storage.paths.steamcmd_root.join("resume"), "yes")?;
        } else {
            let job = state
                .app_state
                .read()
                .unwrap()
                .jobs
                .iter()
                .find(|job| matches!(job.kind, JobKind::DownloadWorkshop))
                .unwrap()
                .id
                .clone();
            super::super::super::commands_install_progress::cancel_installation_job(
                state.clone(),
                job,
            )?;
        }
    }
    let result = tokio::time::timeout(Duration::from_secs(15), &mut update).await?;
    drop(update);
    if modified_config.is_some() || fault == "unknown-loadable" {
        assert!(
            !fixture
                .storage
                .paths
                .steamcmd_root
                .join("captured-script")
                .exists(),
            "ambiguous operator configuration must stop before native or deployment mutation"
        );
    } else {
        let script =
            fs::read_to_string(fixture.storage.paths.steamcmd_root.join("captured-script"))?;
        assert!(script.contains(&format!("workshop_download_item 1623730 {ITEM} validate")));
        assert!(
            !script.contains("999999999"),
            "disabled packages must not be refreshed or enabled"
        );
    }
    let jobs = state.app_state.read().unwrap().jobs.clone();
    let job = jobs
        .iter()
        .find(|job| matches!(job.kind, JobKind::DownloadWorkshop))
        .unwrap();
    if matches!(
        fault,
        "corrupt-cache"
            | "missing-cache"
            | "modified-shipped-config"
            | "nested-shipped-config"
            | "unknown-loadable"
            | "late-config"
    ) || cancel
    {
        assert!(result.is_err());
        if modified_config.is_some() {
            assert!(
                result
                    .as_ref()
                    .err()
                    .is_some_and(|error| error.contains("configuration"))
            );
        }
        if fault == "unknown-loadable" {
            assert!(
                result
                    .as_ref()
                    .err()
                    .is_some_and(|error| error.contains("unknown loadable"))
            );
        }
        if fault == "late-config" {
            assert!(
                result
                    .as_ref()
                    .err()
                    .is_some_and(|error| error.contains("deployment changed"))
            );
        }
        assert!(matches!(
            (&job.status, cancel),
            (JobStatus::Failed, false) | (JobStatus::Cancelled, true)
        ));
        assert_eq!(
            fs::read_to_string(fixture.target.join(format!("{ITEM}/old.pak")))?,
            "cached"
        );
        assert!(!fixture.target.join(format!("{ITEM}/new.pak")).exists());
        if cancel {
            assert_eq!(
                fs::read_to_string(
                    fixture
                        .program
                        .join(format!("steamapps/workshop/content/1623730/{ITEM}/old.pak"))
                )?,
                "cached"
            );
        }
    } else if fault == "unchanged-user-data" {
        let outcome = result?;
        assert!(
            outcome.warnings.is_empty(),
            "unexpected cache fallback: {:?}; job: {job:?}",
            outcome.warnings
        );
        assert!(matches!(job.status, JobStatus::Completed));
        assert_eq!(
            fs::read_to_string(fixture.target.join(format!("{ITEM}/old.pak")))?,
            "cached"
        );
        assert!(!fixture.target.join(format!("{ITEM}/new.pak")).exists());
        drop(outcome);
    } else if !fault.is_empty() && fault != "missing-deployment-success" {
        let outcome = result?;
        outcome
            .install_guard
            .ensure_scope("palworld", &fixture.program)?;
        assert_eq!(outcome.warnings.len(), 1);
        assert_eq!(
            outcome.warnings[0].code,
            app_core::RuntimeStartWarningCode::ModUpdateUsingCache
        );
        assert_eq!(
            outcome.warnings[0].provider,
            Some(app_core::RuntimeModProvider::SteamWorkshop)
        );
        assert_eq!(outcome.warnings[0].item_ids, [ITEM]);
        assert_eq!(job.warnings, outcome.warnings);
        assert!(matches!(job.status, JobStatus::Failed));
        assert_eq!(
            fs::read_to_string(fixture.target.join(format!("{ITEM}/old.pak")))?,
            "cached"
        );
        assert_eq!(
            fs::read_to_string(fixture.target.join(format!("{ITEM}/Info.json")))?,
            INFO
        );
        assert!(!fixture.target.join(format!("{ITEM}/new.pak")).exists());
        assert!(!fixture.target.join(format!("{ITEM}/partial.pak")).exists());
        assert_eq!(
            fs::read_to_string(
                fixture
                    .program
                    .join(format!("steamapps/workshop/content/1623730/{ITEM}/old.pak"))
            )?,
            "cached"
        );
        drop(outcome);
    } else {
        let outcome = result?;
        outcome
            .install_guard
            .ensure_scope("palworld", &fixture.program)?;
        assert!(
            outcome.warnings.is_empty(),
            "unexpected cache fallback: {:?}; job: {job:?}",
            outcome.warnings
        );
        assert!(matches!(job.status, JobStatus::Completed));
        assert!(!fixture.target.join(format!("{ITEM}/old.pak")).exists());
        assert_eq!(
            fs::read_to_string(fixture.target.join(format!("{ITEM}/new.pak")))?,
            "latest"
        );
        assert_eq!(
            fs::read_to_string(fixture.target.join(format!("{ITEM}/Info.json")))?,
            INFO
        );
        drop(outcome);
    }
    assert_eq!(
        fs::read_to_string(fixture.target.join("999999999/keep.pak"))?,
        "disabled"
    );
    assert_eq!(
        read_instance_details(&fixture.storage.paths, id)
            .await?
            .settings_json,
        fixture.instance.settings_json
    );
    assert_eq!(fs::read(&enablement_path)?, enablement_before);
    if fault.ends_with("user-data") {
        assert_eq!(
            fs::read_to_string(fixture.target.join(format!("{ITEM}/Config/user.ini")))?,
            "operator configuration"
        );
        assert_eq!(
            fs::read_to_string(fixture.target.join(format!("{ITEM}/user-note.txt")))?,
            "keep original note"
        );
    }
    if let Some(relative) = modified_config {
        assert_eq!(
            fs::read_to_string(fixture.target.join(ITEM).join(relative))?,
            "operator configuration"
        );
        assert_eq!(
            fs::read_to_string(
                fixture
                    .program
                    .join(format!("steamapps/workshop/content/1623730/{ITEM}"))
                    .join(relative)
            )?,
            "default"
        );
    }
    if fault == "unknown-loadable" {
        assert_eq!(
            fs::read_to_string(fixture.target.join(format!("{ITEM}/unknown.pak")))?,
            "keep unverified original payload"
        );
    }
    if fault == "late-config" {
        assert_eq!(
            fs::read_to_string(fixture.target.join(format!("{ITEM}/Config/user.ini")))?,
            "keep configuration added during native download"
        );
    }
    assert!(
        read_active_instance_run(&fixture.storage.paths, id)
            .await?
            .is_none()
    );
    assert!(!state.runtime_supervisor.lock().unwrap().is_tracked(id));
    let _next = tokio::time::timeout(Duration::from_secs(3), fixture.guard(&state)).await??;
    Ok(())
}

#[tokio::test]
async fn pinned_program_refreshes_cached_workshop_and_deploys_latest_without_changing_enablement()
-> Result<(), Box<dyn std::error::Error>> {
    exercise("", false).await
}

#[tokio::test]
async fn workshop_network_failure_uses_hash_verified_cache_without_publishing_a_run()
-> Result<(), Box<dyn std::error::Error>> {
    exercise("network", false).await
}

#[tokio::test]
async fn cancelling_workshop_start_settles_job_and_releases_program_lease()
-> Result<(), Box<dyn std::error::Error>> {
    exercise("", true).await
}

#[tokio::test]
async fn partial_workshop_update_and_package_rename_restore_the_old_loadable_package()
-> Result<(), Box<dyn std::error::Error>> {
    exercise("partial", false).await?;
    exercise("renamed", false).await
}

#[tokio::test]
async fn same_size_corrupt_or_missing_cache_cannot_bypass_workshop_failure()
-> Result<(), Box<dyn std::error::Error>> {
    exercise("corrupt-cache", false).await?;
    exercise("missing-cache", false).await
}

#[tokio::test]
async fn complete_cache_repairs_missing_or_same_size_corrupted_actual_deployment()
-> Result<(), Box<dyn std::error::Error>> {
    exercise("corrupt-deployment", false).await?;
    exercise("missing-deployment", false).await?;
    exercise("missing-deployment-success", false).await
}

#[tokio::test]
async fn workshop_fallback_preserves_original_operator_data_and_refuses_destructive_replacement()
-> Result<(), Box<dyn std::error::Error>> {
    exercise("network-user-data", false).await?;
    exercise("renamed-user-data", false).await?;
    exercise("unchanged-user-data", false).await?;
    exercise("modified-shipped-config", false).await
}

#[tokio::test]
async fn workshop_preserves_nested_configuration_and_unknown_loadable_files()
-> Result<(), Box<dyn std::error::Error>> {
    exercise("nested-shipped-config", false).await?;
    exercise("unknown-loadable", false).await
}

#[tokio::test]
async fn workshop_download_window_configuration_change_blocks_both_publish_and_cached_replacement()
-> Result<(), Box<dyn std::error::Error>> {
    exercise("late-config", false).await
}

#[tokio::test]
async fn no_active_mods_and_other_providers_do_not_start_workshop_jobs()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = crate::commands::tests::command_smoke_lock().lock().await;
    let mut fixture = Fixture::new().await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&fixture.storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    let reservation =
        match state.try_reserve_runtime_start(&fixture.instance.summary.id, "manual")? {
            RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
            other => panic!("unexpected reservation: {other:?}"),
        };
    let mut settings: Value = serde_json::from_str(&fixture.instance.settings_json)?;
    settings["mod_package_names"] = json!("");
    fixture.instance.settings_json = serde_json::to_string(&settings)?;
    for provider in [Some("steam"), Some("other"), None] {
        let mut descriptor = fixture.descriptor.clone();
        match provider {
            Some(provider) => descriptor.workshop.as_mut().unwrap().provider = provider.into(),
            None => descriptor.workshop = None,
        }
        let outcome = update_before_start(&state, &fixture.storage, WorkshopStartRequest {
            descriptor: &descriptor, instance: &fixture.instance,
            install_guard: fixture.guard(&state).await?, reservation: &reservation,
            context: super::super::super::commands_runtime_prestart_update::PrestartUpdateLogContext {
                source: "manual", console_log_path: None,
            },
        }).await?;
        outcome
            .install_guard
            .ensure_scope("palworld", &fixture.program)?;
        assert!(outcome.warnings.is_empty());
        drop(outcome);
    }
    assert!(state.app_state.read().unwrap().jobs.is_empty());
    assert!(
        !fixture
            .storage
            .paths
            .steamcmd_root
            .join("captured-script")
            .exists()
    );
    assert_eq!(
        fs::read_to_string(fixture.target.join(format!("{ITEM}/old.pak")))?,
        "cached"
    );
    Ok(())
}
