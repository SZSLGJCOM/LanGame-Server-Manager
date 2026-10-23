//! Opt-in native acceptance. The caller supplies a sanitized, disposable source
//! copy; no existing instance, configuration, world, or SDK user context is used.
use super::*;
use app_core::InstanceProgramMode;

#[path = "commands_asa_curseforge_native_acceptance_files.rs"]
mod files;
#[path = "commands_asa_curseforge_native_acceptance_proxy.rs"]
mod proxy;

type AcceptanceResult<T> = Result<T, Box<dyn std::error::Error>>;
const MODULE_ID: &str = "arksurvivalascended";
const MOD_IDS: [i64; 2] = [1715971, 941450];
const UNAVAILABLE_ID: i64 = 2147483647;
const SDK_ROOT: &str = "ShooterGame/Binaries/Win64/ShooterGame";

#[tokio::test(flavor = "current_thread")]
#[ignore = "opt-in: sanitized real ASA package, two cached-Mod production starts, native readiness/stop, then unavailable-Mod rejection; network and substantial disk space required"]
async fn asa_curseforge_cached_native_start_stop_and_failure() -> AcceptanceResult<()> {
    let _guard = command_smoke_lock().lock().await;
    let proxy_input = match std::env::var("LANGAME_ASA_CF_HTTP_PROXY") {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err("ASA acceptance HTTP proxy must be a valid loopback socket address".into());
        }
    };
    let http_proxy = proxy::parse(proxy_input.as_deref())?;
    let source = PathBuf::from(
        std::env::var_os("LANGAME_NATIVE_PACKAGE_ROOT")
            .ok_or("LANGAME_NATIVE_PACKAGE_ROOT is required")?,
    );
    files::plain_path(&source)?;
    let source = dunce::canonicalize(source)?;
    let report_root = PathBuf::from(
        std::env::var_os("LANGAME_ASA_CF_REPORT_ROOT")
            .ok_or("LANGAME_ASA_CF_REPORT_ROOT is required")?,
    );
    files::plain_path(&report_root)?;
    let report_root = dunce::canonicalize(report_root)?;
    if source.starts_with(&report_root) || report_root.starts_with(&source) {
        return Err("native reports and source package must have separate roots".into());
    }
    let report_dir = report_root.join(format!("asa-cf-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&report_dir)?;
    let mut report = json!({"module_id": MODULE_ID, "status": "running", "cycles": []});
    report["http_proxy"] = json!({"enabled":http_proxy.is_some(), "address":http_proxy.map(|address| address.to_string())});
    files::save_report(&report_dir, &report)?;
    let result = Box::pin(run_acceptance(
        &source,
        &report_dir,
        &mut report,
        http_proxy,
    ))
    .await;
    report["status"] = json!(if result.is_ok() { "passed" } else { "failed" });
    if let Err(error) = &result {
        report["failure"] = json!(files::safe_error(&error.to_string()));
    }
    files::save_report(&report_dir, &report)?;
    println!("ASA_CF_ACCEPTANCE {}", serde_json::to_string(&report)?);
    println!("ASA_CF_REPORT {}", report_dir.display());
    result.map_err(|_| "native ASA CurseForge acceptance failed; inspect its selected evidence and retained owned root".into())
}

async fn run_acceptance(
    source: &Path,
    report_dir: &Path,
    report: &mut Value,
    http_proxy: Option<std::net::SocketAddrV4>,
) -> AcceptanceResult<()> {
    // Refuse user context before copying even if the game would ignore it.
    if source.join("ShooterGame/Saved").try_exists()? {
        return Err("native source must omit ShooterGame/Saved".into());
    }
    files::check_source_user_data(&source.join(SDK_ROOT).join("ModsUserData"))?;
    files::library_projection(&source.join(SDK_ROOT), &MOD_IDS)?;
    let copy_source = source.to_path_buf();
    let package = tokio::task::spawn_blocking(move || {
        package::copy_package(
            &copy_source,
            MODULE_ID,
            Some(Path::new("ShooterGame/Saved")),
            64 * 1024 * 1024 * 1024,
            Duration::from_secs(1200),
        )
    })
    .await??;
    // Cleanup is permitted only after every acceptance condition and a separate
    // exact-image process enumeration succeed. All failures retain the fixture.
    package.cleanup_allowed.store(false, Ordering::SeqCst);
    report["owned_root"] = json!(package.root);
    files::save_report(report_dir, report)?;
    let env_guard = environment::NativeEnvironment::set(&package.root)?;
    let descriptors = discover_modules(workspace_root().join("modules"))?;
    let descriptor = find_descriptor(&descriptors, MODULE_ID)?;
    let smoke = readiness::SmokeManifest::load(descriptor)?;
    if !smoke.preconditions.is_empty() {
        return Err("ASA acceptance has an unsupported native prerequisite".into());
    }
    save_app_settings(AppSettings {
        archives_root: String::new(),
        servers_root: package.root.join("i").to_string_lossy().into_owned(),
        games_root: package.root.to_string_lossy().into_owned(),
        modules_root: workspace_root()
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: package.root.join("steamcmd").to_string_lossy().into_owned(),
    })?;
    let storage = bootstrap_storage()?;
    initialize_database(&storage.paths).await?;
    sync_modules(&storage.paths, &descriptors).await?;
    sync_game_installs(
        &storage.paths,
        &[GameInstallSyncRecord {
            module_id: MODULE_ID.into(),
            install_root: package.install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: None,
            mark_verified: true,
        }],
    )
    .await?;
    let desktop = DesktopState::default();
    desktop
        .app_state
        .write()
        .map_err(|_| "desktop state lock failed")?
        .settings = storage.settings.clone();
    let app = tauri::test::mock_builder()
        .manage(desktop)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    let created = app_storage::create_instance_with_options(
        &storage.paths,
        descriptor,
        CreateInstanceInput {
            name: "asa-cf".into(),
            module_id: MODULE_ID.into(),
        },
        app_storage::InstanceCreationOptions {
            program_mode: Some(InstanceProgramMode::Independent),
            ..Default::default()
        },
    )
    .await?
    .provisioning;
    let initial = read_instance_details(&storage.paths, &created.summary.id).await?;
    let id = initial.summary.id.clone();
    let program_root = creation::program_root(&initial)?;
    files::plain_path(&program_root)?;
    if !dunce::canonicalize(&program_root)?.starts_with(dunce::canonicalize(&package.root)?)
        || app_storage::instance_program_mode(creation::instance_root(&initial)?)?
            != InstanceProgramMode::Independent
    {
        return Err("ASA instance program is not independently owned by this fixture".into());
    }
    // Declared first so every unwind stops owned processes before releasing the
    // firewall guard. ASA's guard itself retains rules on an unconfirmed stop.
    let mut firewall_cleanup = None;
    let cleanup = NativeCleanup {
        state: &state,
        instance_ids: vec![id.clone()],
        cleanup_allowed: Arc::clone(&package.cleanup_allowed),
    };
    let result: AcceptanceResult<()> = Box::pin(async {
        let source_sdk = package.install_root.join(SDK_ROOT);
        let destination_sdk = program_root.join(SDK_ROOT);
        tokio::task::spawn_blocking(move || files::seed_cache(&source_sdk, &destination_sdk, &MOD_IDS)
            .map_err(|error| error.to_string())).await??;
        let mut settings: serde_json::Map<String, Value> = serde_json::from_str(&initial.settings_json)?;
        settings.extend(smoke.fixture.settings.clone());
        settings.extend(diagnostics::fixture_settings_override(std::env::var("LANGAME_NATIVE_SETTINGS_JSON").ok().as_deref())?);
        for (key, value) in [
            ("program_update", json!({"policy":"pinned"})),
            ("map_name", json!("TheIsland_WP")), ("additional_maps", json!([])),
            ("mod_ids_csv", json!("1715971,941450")), ("passive_mod_ids_csv", json!("")),
            // Discard arbitrary fixture flags. This opt-in proxy remains one
            // validated argument; its use by native ASA needs live comparison.
            ("custom_launch_flags", json!(proxy::launch_flag(http_proxy))), ("server_name", json!("LanGame ASA CF acceptance")),
            ("server_password", json!(uuid::Uuid::new_v4().to_string())),
            ("admin_password", json!(uuid::Uuid::new_v4().to_string())),
        ] { settings.insert(key.into(), value); }
        for key in ["public_server", "list_on_steam", "list_on_eos"] {
            if let Some(value) = settings.get_mut(key) {
                if value.is_boolean() { *value = json!(false); }
                else if value.is_number() { *value = json!(0); }
            }
        }
        let details = update_settings(&storage, &initial, &settings).await?;
        report["bind"] = json!("native_wildcard");
        report["bind_address"] = json!(details.summary.bind_ip);
        report["ports"] = json!(details.ports);
        let executable = program_root.join(&descriptor.process.as_ref().ok_or("ASA executable is absent")?.executable);
        files::ensure_no_owned_process(&executable)?;
        let firewall_instance = details.clone();
        let firewall_root = package.root.clone();
        firewall_cleanup = Some(tokio::task::spawn_blocking(move ||
            firewall::NativeFirewall::prepare_asa_native(&firewall_instance, &firewall_root)).await??);
        report["firewallCleanupPassed"] = json!(false);
        let before = files::library_projection(&program_root.join(SDK_ROOT), &MOD_IDS)?;
        report["cache_before"] = json!(before);
        report["native_log_path"] = json!(native_log_path(&details)?);
        files::save_report(report_dir, report)?;
        let fixture = NativeCycleFixture {
            state: &state, storage: &storage, descriptor, smoke: &smoke,
            package_root: &package.root, program_root: &program_root,
            executable: &executable, report_dir,
        };
        let first = Box::pin(fixture.run_cycle(&details, "initial", report)).await?;
        let warm = read_instance_details(&storage.paths, &id).await?;
        let second = Box::pin(fixture.run_cycle(&warm, "warm", report)).await?;
        if first.0 == second.0 || first.1 == second.1 {
            return Err("warm startup reused the previous persisted run or OS process identity".into());
        }
        report["cache_after"] = json!(files::library_projection(&program_root.join(SDK_ROOT), &MOD_IDS)?);
        settings.insert("mod_ids_csv".into(), json!(format!("1715971,941450,{UNAVAILABLE_ID}")));
        let invalid = update_settings(&storage, &warm, &settings).await?;
        let cutoff = now_ms()?;
        report["attempt_since_ms"] = json!(cutoff);
        let failed = Box::pin(start_instance_process_after_reconcile(None, &state, &storage, id.clone(), "manual", None)).await;
        let failure = match failed {
            Ok(_) => return Err("unavailable Mod unexpectedly passed production startup".into()),
            Err(error) => error,
        };
        report["unavailable_mod"] = json!({"project_id":UNAVAILABLE_ID, "rejected":true, "reason":files::safe_error(&failure)});
        // Verify release before the cleanup guard can conceal a leaked child.
        verify_stopped(&state, &storage, &id, &executable).await?;
        let (lines, valid, terminal) = files::native_evidence(&native_log_path(&invalid)?, cutoff)?;
        report["unavailable_mod"]["native_cf_lines"] = json!(lines);
        let gate_timeout = failure.contains("ASA CurseForge Mod validation did not complete within 900000 ms;");
        if terminal {
            report["unavailable_mod"]["kind"] = json!("current_native_mod_failure");
        } else if gate_timeout && MOD_IDS.iter().all(|id| valid.contains(id)) && !valid.contains(&UNAVAILABLE_ID) {
            report["unavailable_mod"]["kind"] = json!("required_mod_not_validated_timeout");
            // CFCore 1.47's server final validation follows the successful
            // online GetMods callback. This is a source-based inference from
            // current final records, not an invented query-start log line.
            report["unavailable_mod"]["query_success_evidence"] = json!("inferred_from_current_final_mod_validation");
        } else { return Err("unavailable-Mod rejection lacks current native failure or a strict CF gate timeout with current cached-Mod validation".into()); }
        report["unavailable_mod"]["active_run_absent"] = json!(true);
        report["unavailable_mod"]["supervisor_untracked"] = json!(true);
        report["unavailable_mod"]["pending_start_absent"] = json!(true);
        report["unavailable_mod"]["owned_processes_absent"] = json!(true);
        files::save_report(report_dir, report)?;
        Ok(())
    }).await;
    drop(cleanup);
    package.cleanup_allowed.store(false, Ordering::SeqCst);
    let executable = program_root.join(
        &descriptor
            .process
            .as_ref()
            .ok_or("ASA executable is absent")?
            .executable,
    );
    let firewall_result: AcceptanceResult<()> = if let Some(mut firewall) = firewall_cleanup.take()
    {
        match verify_stopped(&state, &storage, &id, &executable).await {
            Ok(()) => tokio::task::spawn_blocking(move || firewall.finish())
                .await
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() }),
            Err(error) => Err(error),
        }
    } else {
        Ok(())
    };
    if report.get("firewallCleanupPassed").is_some() {
        report["firewallCleanupPassed"] = json!(firewall_result.is_ok());
        if let Err(error) = &firewall_result {
            report["firewallCleanupFailure"] = json!(files::safe_error(&error.to_string()));
        }
    }
    if result.is_err() {
        if let Some(path) = report["native_log_path"].as_str()
            && let Ok((lines, _, _)) = files::native_evidence(
                Path::new(path),
                report["attempt_since_ms"].as_u64().unwrap_or(u64::MAX),
            )
        {
            report["failure_native_cf_lines"] = json!(lines);
        }
        return result;
    }
    firewall_result?;
    files::ensure_no_owned_process(&executable)?;
    package.cleanup_allowed.store(true, Ordering::SeqCst);
    drop(app);
    drop(env_guard);
    let owned_root = package.root.clone();
    drop(package);
    if owned_root.try_exists()? {
        return Err("ASA acceptance passed but owned fixture cleanup failed".into());
    }
    report["cleanup"] = json!("passed");
    Ok(())
}

async fn update_settings(
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
    settings: &serde_json::Map<String, Value>,
) -> AcceptanceResult<InstanceDetails> {
    let mut ports = instance.ports.clone();
    let declared = [
        ("game", "udp", 27777),
        ("peer", "udp", 27778),
        ("query", "udp", 47015),
        ("rcon", "tcp", 47020),
    ];
    if ports.len() != declared.len()
        || declared.iter().any(|(name, protocol, _)| {
            ports
                .iter()
                .filter(|port| port.name == *name && port.protocol.eq_ignore_ascii_case(protocol))
                .count()
                != 1
        })
    {
        return Err("ASA acceptance requires its four declared native ports".into());
    }
    for port in &mut ports {
        port.port = declared
            .iter()
            .find(|(name, _, _)| port.name == *name)
            .ok_or("ASA acceptance encountered an undeclared port")?
            .2;
    }
    Ok(app_storage::update_instance(
        &storage.paths,
        UpdateInstanceInput {
            id: instance.summary.id.clone(),
            // This ASA build binds its native listener to the wildcard address.
            // Keep production's strict check intact and use separate fixture ports.
            bind_ip: "0.0.0.0".into(),
            auto_backup_on_stop: false,
            backup_retention_count: 2,
            settings_json: serde_json::to_string(settings)?,
            ports,
        },
    )
    .await?)
}

struct NativeCycleFixture<'fixture, 'app> {
    state: &'fixture tauri::State<'app, DesktopState>,
    storage: &'fixture StorageBootstrap,
    descriptor: &'fixture ModuleDescriptor,
    smoke: &'fixture readiness::SmokeManifest,
    package_root: &'fixture Path,
    program_root: &'fixture Path,
    executable: &'fixture Path,
    report_dir: &'fixture Path,
}

impl NativeCycleFixture<'_, '_> {
    async fn run_cycle(
        &self,
        details: &InstanceDetails,
        cycle: &str,
        report: &mut Value,
    ) -> AcceptanceResult<(i64, (u32, u64))> {
        let offsets = self.smoke.log_baselines(details, self.program_root)?;
        report["attempt_since_ms"] = json!(now_ms()?);
        files::save_report(self.report_dir, report)?;
        let started = Box::pin(start_instance_process_after_reconcile(
            None,
            self.state,
            self.storage,
            details.summary.id.clone(),
            "manual",
            None,
        ))
        .await?;
        let running = read_instance_details(&self.storage.paths, &details.summary.id).await?;
        let primary = running
            .active_run
            .as_ref()
            .ok_or("production startup did not publish an active run")?
            .processes
            .iter()
            .find(|process| process.process_key == "main")
            .ok_or("ASA main process is absent")?;
        let pid = primary.pid.ok_or("ASA main PID is absent")?;
        let identity = primary
            .process_identity
            .as_ref()
            .ok_or("ASA OS process identity is absent")?;
        let cutoff = identity
            .creation_time
            .checked_div(10_000)
            .and_then(|ms| ms.checked_sub(11_644_473_600_000))
            .ok_or("ASA OS creation time is invalid")?;
        let ready = self
            .smoke
            .wait_ready(
                self.descriptor,
                &running,
                self.program_root,
                self.package_root,
                offsets,
            )
            .await;
        let stopped = stop_instance_process(self.state.clone(), details.summary.id.clone()).await;
        let (lines, valid, terminal) = files::native_evidence(&native_log_path(details)?, cutoff)?;
        let cache = files::library_projection(&self.program_root.join(SDK_ROOT), &MOD_IDS)?;
        let record = json!({"cycle":cycle, "run_id":started.run_id, "pid":pid, "creation_time":identity.creation_time,
        "since_ms":cutoff, "native_readiness":ready.is_ok(), "normal_stop":stopped.is_ok(), "native_cf_lines":lines, "cache":cache});
        report["cycles"]
            .as_array_mut()
            .ok_or("acceptance report has invalid cycles")?
            .push(record);
        files::save_report(self.report_dir, report)?;
        ready?;
        stopped?;
        verify_stopped(
            self.state,
            self.storage,
            &details.summary.id,
            self.executable,
        )
        .await?;
        if terminal || !MOD_IDS.iter().all(|id| valid.contains(id)) {
            return Err(
                "fresh native Mod valid evidence is missing for a configured cached Mod".into(),
            );
        }
        files::require_current_installed(&cache)?;
        Ok((started.run_id, (pid, identity.creation_time)))
    }
}

async fn verify_stopped(
    state: &DesktopState,
    storage: &StorageBootstrap,
    id: &str,
    executable: &Path,
) -> AcceptanceResult<()> {
    if read_instance_details(&storage.paths, id)
        .await?
        .active_run
        .is_some()
        || state
            .runtime_supervisor
            .lock()
            .map_err(|_| "runtime supervisor lock failed")?
            .is_tracked(id)
        || state
            .pending_runtime_start_instance_ids()?
            .iter()
            .any(|pending| pending == id)
    {
        return Err(
            "ASA stop/failure left an active run, supervisor entry, or pending start".into(),
        );
    }
    files::ensure_no_owned_process(executable)?;
    Ok(())
}

fn native_log_path(instance: &InstanceDetails) -> AcceptanceResult<PathBuf> {
    let maps = app_core::ark_maps::processes(instance)?;
    if maps.len() != 1 || maps[0].process_key != "main" {
        return Err("ASA acceptance requires one main map".into());
    }
    Ok(PathBuf::from(&maps[0].native_log_path))
}

fn now_ms() -> AcceptanceResult<u64> {
    Ok(u64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
    )?)
}
