use super::*;
use crate::commands::commands_satisfactory_world::native_world::NativeWorld;
use inventory::save_inventory;
use runtime::print_native_failure;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

#[path = "commands_native_archive.rs"]
mod archive;
#[cfg(windows)]
#[path = "commands_asa_curseforge_native_acceptance.rs"]
mod asa_curseforge_native_acceptance;
#[path = "commands_native_astroneer_probe.rs"]
mod astroneer_console;
#[path = "commands_native_astroneer_readiness.rs"]
mod astroneer_readiness;
#[path = "commands_native_barotrauma_campaign.rs"]
mod barotrauma_campaign;
#[path = "commands_native_catalog.rs"]
mod catalog;
#[path = "commands_native_creation.rs"]
mod creation;
#[path = "commands_native_diagnostics.rs"]
mod diagnostics;
#[path = "commands_dragonwilds_compatibility_tests.rs"]
mod dragonwilds_compatibility;
#[path = "commands_enshrouded_compatibility_tests.rs"]
mod enshrouded_compatibility;
#[path = "commands_native_environment.rs"]
mod environment;
#[cfg(windows)]
#[path = "commands_existing_instance_acceptance.rs"]
mod existing_instances;
#[path = "commands_file_mods_acceptance.rs"]
mod file_mods_acceptance;
use crate::commands::tests::native_firewall as firewall;
#[path = "commands_native_gm_tools.rs"]
mod gm_tools;
#[path = "commands_native_inventory.rs"]
mod inventory;
#[path = "commands_native_maintenance.rs"]
mod maintenance;
#[path = "commands_native_manual_save_probe.rs"]
mod manual_save_probe;
#[cfg(windows)]
#[path = "commands_native_moria_shutdown_control.rs"]
mod moria_shutdown_control;
#[path = "commands_native_multi_instance.rs"]
mod multi_instance;
#[path = "commands_native_log_readiness.rs"]
mod native_logs;
#[path = "commands_native_package.rs"]
mod package;
#[path = "commands_native_player_counts.rs"]
mod player_counts;
#[path = "commands_native_prestart_probe.rs"]
mod prestart_probe;
#[path = "commands_native_readiness.rs"]
mod readiness;
#[path = "commands_native_runtime.rs"]
mod runtime;
#[path = "commands_native_satisfactory_frm.rs"]
mod satisfactory_frm;
#[path = "commands_native_satisfactory_isolation.rs"]
mod satisfactory_isolation;
#[path = "commands_native_save_failure_probe.rs"]
mod save_failure_probe;
#[path = "commands_native_save_guard_probe.rs"]
mod save_guard_probe;
#[path = "commands_native_seed_probe.rs"]
mod seed_probe;
#[path = "commands_native_settings_readback.rs"]
mod settings_readback;
#[path = "commands_native_sons_backup_probe.rs"]
mod sons_backup_probe;
#[path = "commands_native_sons_world_fixture.rs"]
mod sons_world_fixture;
#[path = "commands_native_squad_probe.rs"]
mod squad_probe;
#[path = "commands_native_storage_fixture.rs"]
mod storage_fixture;
#[path = "commands_native_theforest_probe.rs"]
mod theforest_probe;

#[cfg(windows)]
#[test]
#[ignore = "private child entry for the owned Moria exit helper"]
fn moria_console_exit_helper_entry() {
    if std::env::var("LANGAME_MORIA_EXIT_HELPER_CHILD").as_deref() == Ok("1") {
        std::process::exit(app_runtime::run_moria_console_exit_helper_worker());
    }
}

struct NativeCleanup<'a> {
    state: &'a DesktopState,
    instance_ids: Vec<String>,
    // Populated only by the dedicated isolation scope. Remove an ID before
    // attempting start: an unpublished failed start must remain unknown.
    never_started_instance_ids: std::collections::BTreeSet<String>,
    cleanup_allowed: Arc<AtomicBool>,
}

impl NativeCleanup<'_> {
    fn mark_start_attempted(&mut self, instance_id: &str) -> Result<(), &'static str> {
        if !self.instance_ids.iter().any(|owned| owned == instance_id) {
            return Err("Native startup has no fixture cleanup owner");
        }
        self.never_started_instance_ids.remove(instance_id);
        Ok(())
    }
}

impl Drop for NativeCleanup<'_> {
    fn drop(&mut self) {
        let pending_starts = self.state.pending_runtime_start_instance_ids();
        let Ok(mut supervisor) = self.state.runtime_supervisor.lock() else {
            self.cleanup_allowed.store(false, Ordering::SeqCst);
            return;
        };
        let previously_allowed = self.cleanup_allowed.load(Ordering::SeqCst);
        let mut stopped = Vec::with_capacity(self.instance_ids.len());
        for instance_id in &self.instance_ids {
            let stopped_tree = supervisor.take_running_for_stop(instance_id).map(|mut running| match stop_managed_instance(&mut running) {
                Ok(processes) => {
                    for process in processes.iter().take(8) {
                        let code = process.exit_code.map(|value| format!("0x{:08X}", value as u32));
                        eprintln!(
                            "NATIVE_CLEANUP module={} phase=stopped process={} run_id={} exit_code={:?} exit_hex={} owned_tree=finished",
                            running.summary.module_id,
                            process.process_key,
                            process.run_id,
                            process.exit_code,
                            code.as_deref().unwrap_or("unavailable"),
                        );
                    }
                    true
                }
                Err(_) => false,
            });
            let pending = pending_starts
                .as_ref()
                .ok()
                .map(|ids| ids.iter().any(|pending| pending == instance_id));
            let evidence = cleanup_stop_evidence(
                stopped_tree,
                self.never_started_instance_ids.contains(instance_id),
                pending,
            );
            if stopped_tree.is_none() && evidence == Some(true) {
                eprintln!(
                    "NATIVE_CLEANUP phase=not_started start_attempted=false pending_start=false"
                );
            }
            stopped.push(evidence);
        }
        // A failed startup may have never published a supervisor entry. Its
        // absence alone cannot establish that every spawned child was stopped.
        self.cleanup_allowed.store(
            cleanup_confirmed(&stopped, previously_allowed),
            Ordering::SeqCst,
        );
        if !self.cleanup_allowed.load(Ordering::SeqCst) {
            eprintln!("NATIVE_LIFECYCLE cleanup=process_stop_failed");
        }
    }
}

fn cleanup_stop_evidence(
    stopped: Option<bool>,
    never_started: bool,
    pending_start: Option<bool>,
) -> Option<bool> {
    // A known stop failure wins. A missing or attempted startup never gains
    // permission from an absent supervisor entry or failed pending-start read.
    stopped.or_else(|| (never_started && pending_start == Some(false)).then_some(true))
}

fn cleanup_confirmed(stopped: &[Option<bool>], previously_allowed: bool) -> bool {
    stopped.iter().all(|result| match result {
        Some(stopped) => *stopped,
        None => previously_allowed,
    })
}

#[test]
fn native_cleanup_does_not_assume_failed_start_was_stopped() {
    let state = DesktopState::default();
    let cleanup_allowed = Arc::new(AtomicBool::new(false));
    drop(NativeCleanup {
        state: &state,
        instance_ids: vec!["not-published-start".into()],
        never_started_instance_ids: Default::default(),
        cleanup_allowed: Arc::clone(&cleanup_allowed),
    });
    assert!(!cleanup_allowed.load(Ordering::SeqCst));
}

#[test]
fn native_pair_cleanup_requires_both_owned_process_trees_to_finish() {
    assert!(cleanup_confirmed(&[Some(true), Some(true)], false));
    for stopped in [
        [Some(false), Some(true)],
        [Some(true), Some(false)],
        [None, Some(true)],
        [Some(true), None],
    ] {
        assert!(!cleanup_confirmed(&stopped, false));
    }
    assert!(cleanup_confirmed(&[None, None], true));
    assert!(!cleanup_confirmed(&[Some(false), None], true));
}

#[test]
fn isolation_cleanup_accepts_only_proven_unattempted_missing_instances() {
    let untouched = cleanup_stop_evidence(None, true, Some(false));
    assert!(cleanup_confirmed(&[untouched, Some(true)], false));
    for evidence in [
        cleanup_stop_evidence(None, false, Some(false)),
        cleanup_stop_evidence(None, true, Some(true)),
        cleanup_stop_evidence(None, true, None),
        cleanup_stop_evidence(Some(false), true, Some(false)),
    ] {
        assert!(!cleanup_confirmed(&[evidence, Some(true)], false));
    }
    let state = DesktopState::default();
    let cleanup_allowed = Arc::new(AtomicBool::new(false));
    let mut cleanup = NativeCleanup {
        state: &state,
        instance_ids: vec!["owned".into()],
        never_started_instance_ids: ["owned".into()].into_iter().collect(),
        cleanup_allowed: Arc::clone(&cleanup_allowed),
    };
    assert!(cleanup.mark_start_attempted("foreign").is_err());
    cleanup.mark_start_attempted("owned").unwrap();
    assert!(!cleanup.never_started_instance_ids.contains("owned"));
    drop(cleanup);
    assert!(!cleanup_allowed.load(Ordering::SeqCst));
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NativeScope {
    FullLifecycle,
    StartStop,
    TwoInstanceLifecycle,
    SatisfactoryIsolation,
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "opt-in: selects one local package, copies into disposable storage, creates instances, verifies production start/stop/restart and retained data; official acquisition is a separate explicit mode"]
async fn native_package_lifecycle() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let descriptors = discover_modules(workspace_root().join("modules"))?;
    let cases = catalog::from_env(&descriptors)?;
    for (module_id, source) in cases {
        let started = Instant::now();
        println!("NATIVE_CATALOG module={module_id} phase=begin");
        Box::pin(run_native_package(
            module_id.clone(),
            source,
            NativeScope::FullLifecycle,
        ))
        .await?;
        println!(
            "NATIVE_CATALOG module={module_id} phase=passed elapsed_ms={}",
            started.elapsed().as_millis()
        );
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "opt-in: two isolated Terraria, Valheim or Minecraft instances; peer stays running through the primary start/save/stop cycle, then both follow production lifecycle cleanup"]
async fn native_two_instance_lifecycle() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let descriptors = discover_modules(workspace_root().join("modules"))?;
    for (module_id, source) in catalog::from_env(&descriptors)? {
        if !matches!(module_id.as_str(), "terraria" | "valheim" | "minecraft") {
            return Err(
                "native simultaneous lifecycle supports Terraria, Valheim and Minecraft".into(),
            );
        }
        if std::env::var("LANGAME_NATIVE_MANUAL_SAVE").as_deref() != Ok("true") {
            return Err(
                "native simultaneous lifecycle requires LANGAME_NATIVE_MANUAL_SAVE=true".into(),
            );
        }
        Box::pin(run_native_package(
            module_id,
            source,
            NativeScope::TwoInstanceLifecycle,
        ))
        .await?;
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "opt-in: two independent Satisfactory instances, real HTTPS worlds/saves and credential isolation; no clients, FRM or downloads"]
async fn native_satisfactory_instance_isolation() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let descriptors = discover_modules(workspace_root().join("modules"))?;
    let mut cases = catalog::from_env(&descriptors)?;
    if cases.len() != 1 || cases[0].0 != "satisfactory" {
        return Err("Satisfactory isolation requires exactly its one local package".into());
    }
    let (module_id, source) = cases.remove(0);
    Box::pin(run_native_package(
        module_id,
        source,
        NativeScope::SatisfactoryIsolation,
    ))
    .await
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "opt-in: isolated local package, production start/readiness/stop; no existing instance or save is used"]
async fn native_start_stop() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let descriptors = discover_modules(workspace_root().join("modules"))?;
    for (module_id, source) in catalog::from_env(&descriptors)? {
        Box::pin(run_native_package(
            module_id,
            source,
            NativeScope::StartStop,
        ))
        .await?;
    }
    Ok(())
}

#[cfg(windows)]
#[tokio::test(flavor = "current_thread")]
#[ignore = "diagnostic only: fresh native Moria without loader preparation, fixed exit once and bounded owned-tree observation; no production-start/save acceptance"]
async fn native_moria_no_loader_shutdown_control() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    if std::env::var("LANGAME_NATIVE_MORIA_NO_LOADER_CONTROL").as_deref() != Ok("true") {
        return Err("native Moria baseline requires its explicit diagnostic opt-in".into());
    }
    let descriptors = discover_modules(workspace_root().join("modules"))?;
    let selected = catalog::from_env(&descriptors)?;
    if selected.len() != 1 || selected[0].0 != "returntomoria" {
        return Err(
            "native no-loader baseline is limited to exactly one fresh Moria fixture".into(),
        );
    }
    for (module_id, source) in selected {
        Box::pin(run_native_package(
            module_id,
            source,
            NativeScope::StartStop,
        ))
        .await?;
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "opt-in: isolated local package, production automatic startup and declared manual save action; no existing instance or world is used"]
async fn native_manual_save() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    if std::env::var("LANGAME_NATIVE_MANUAL_SAVE").as_deref() != Ok("true") {
        return Err("native manual save requires LANGAME_NATIVE_MANUAL_SAVE=true".into());
    }
    if std::env::var("LANGAME_NATIVE_SAVE_NOT_READY").is_ok() {
        return Err(
            "Use the separate native_no_client_save_guard acceptance for expected rejections"
                .into(),
        );
    }
    let descriptors = discover_modules(workspace_root().join("modules"))?;
    for (module_id, source) in catalog::from_env(&descriptors)? {
        if module_id == "squad" {
            return Err("Squad has no persistent-world manual save action".into());
        }
        Box::pin(run_native_package(
            module_id,
            source,
            NativeScope::StartStop,
        ))
        .await?;
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "opt-in: RimWorld/Abiotic without clients; formal backend must reject before native save, then normal stop and a fresh two-cycle restart"]
async fn native_no_client_save_guard() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    if std::env::var("LANGAME_NATIVE_MANUAL_SAVE").as_deref() != Ok("true")
        || std::env::var("LANGAME_NATIVE_MANUAL_SAVE_RESTART").as_deref() != Ok("true")
    {
        return Err("Native no-client acceptance requires formal dispatch and both cycles".into());
    }
    let descriptors = discover_modules(workspace_root().join("modules"))?;
    for (module_id, source) in catalog::from_env(&descriptors)? {
        if !save_guard_probe::expected(&module_id)? {
            return Err(
                "Native no-client acceptance requires its explicit closed-module opt-in".into(),
            );
        }
        Box::pin(run_native_package(
            module_id,
            source,
            NativeScope::StartStop,
        ))
        .await?;
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "opt-in: isolated local packages and frontend-exported GM commands; verifies production start, native effects, stop and owned-process cleanup"]
async fn native_game_tools() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    if std::env::var("LANGAME_NATIVE_GM_TOOLS").as_deref() != Ok("true") {
        return Err("native game tools require LANGAME_NATIVE_GM_TOOLS=true".into());
    }
    let descriptors = discover_modules(workspace_root().join("modules"))?;
    for (module_id, source) in catalog::from_env(&descriptors)? {
        let started = Instant::now();
        println!("NATIVE_GM_CATALOG module={module_id} phase=begin");
        Box::pin(run_native_package(
            module_id.clone(),
            source,
            NativeScope::StartStop,
        ))
        .await?;
        println!(
            "NATIVE_GM_CATALOG module={module_id} phase=passed elapsed_ms={}",
            started.elapsed().as_millis()
        );
    }
    Ok(())
}

async fn run_native_package(
    module_id: String,
    source: PathBuf,
    scope: NativeScope,
) -> Result<(), Box<dyn std::error::Error>> {
    let cycle_only = scope == NativeScope::StartStop;
    let observation_seconds = diagnostics::observation_seconds()?;
    let descriptors = discover_modules(workspace_root().join("modules"))?;
    let descriptor = find_descriptor(&descriptors, &module_id)?;
    let creation_mode = creation::CreationMode::from_env(descriptor)?;
    if scope == NativeScope::SatisfactoryIsolation {
        satisfactory_isolation::validate(&module_id, &creation_mode, observation_seconds)?;
    }
    if scope == NativeScope::TwoInstanceLifecycle
        && !matches!(
            creation_mode.instances,
            creation::InstanceMode::IndependentPair | creation::InstanceMode::SharedPair
        )
    {
        return Err(
            "native simultaneous lifecycle requires independent_pair or shared_pair".into(),
        );
    }
    let maintenance_mode = maintenance::ProgramMaintenance::from_env(&creation_mode)?;
    if cycle_only
        && (creation_mode.acquisition != creation::Acquisition::LocalPackage
            || creation_mode.instances != creation::InstanceMode::Default)
    {
        return Err("native single-cycle tests require one isolated local package instance".into());
    }
    if creation_mode.acquisition == creation::Acquisition::Official
        && std::env::var_os("LANGAME_NATIVE_STEAMCMD_ROOT").is_some()
    {
        return Err("official native acquisition must use its disposable SteamCMD root; unset LANGAME_NATIVE_STEAMCMD_ROOT".into());
    }
    let mut smoke = readiness::SmokeManifest::load(descriptor)?;
    let elevated = WindowsPlatform::current_process_is_elevated()?;
    let elevated_fixture = firewall::elevated_fixture_allowed(
        &module_id,
        elevated,
        std::env::var("LANGAME_NATIVE_ALLOW_ELEVATED_SCUM")
            .ok()
            .as_deref(),
        std::env::var("LANGAME_NATIVE_ABIOTIC_ELEVATED_ISOLATED")
            .ok()
            .as_deref(),
    )?;
    let offline_dst = module_id == "dontstarve"
        && std::env::var("LANGAME_NATIVE_DST_OFFLINE_LAN").as_deref() == Ok("true");
    for precondition in &smoke.preconditions {
        if offline_dst && precondition.kind == "secret" && precondition.id == "dst-cluster-token" {
            continue;
        }
        match precondition.kind.as_str() {
            "operator_acknowledgement" => {
                let key = precondition.environment.as_deref().ok_or("missing acknowledgement environment")?;
                if std::env::var(key).ok().as_deref() != precondition.expected.as_deref() {
                    return Err(format!("native prerequisite: {}", precondition.reason_code).into());
                }
                // This propagates the caller's explicit prior acknowledgement;
                // the runner never creates or changes the acknowledgement env.
                smoke.fixture.settings.insert(precondition.setting_key.clone().ok_or("missing acknowledged setting")?, json!(true));
            }
            "elevation" => {
                if !elevated {
                    return Err(format!("native prerequisite: {}", precondition.reason_code).into());
                }
            }
            "secret" => return Err("DST native lifecycle requires explicit offline LAN mode; no online credential is used".into()),
            _ => return Err("unsupported native prerequisite".into()),
        }
    }
    let source = source.canonicalize()?;
    let satisfactory_frm = satisfactory_frm::Fixture::from_env(&module_id, &source)?;
    let excluded = match descriptor.storage.saves_path_template.as_deref() {
        Some(template) if template.contains("paths.install_root") => {
            let data = app_storage::install_save_directory_prefix(template, &source)
                .ok_or("cannot safely separate existing world data from the native package")?;
            Some(data.strip_prefix(&source)?.to_path_buf())
        }
        _ => None,
    };
    let max_bytes = std::env::var("LANGAME_NATIVE_COPY_MAX_BYTES")
        .ok()
        .map(|value| value.parse::<u64>())
        .transpose()?
        .unwrap_or(64 * 1024 * 1024 * 1024);
    let copy_seconds = std::env::var("LANGAME_NATIVE_COPY_TIMEOUT_SECONDS")
        .ok()
        .map(|value| value.parse::<u64>())
        .transpose()?
        .unwrap_or(1200);
    if max_bytes == 0 || max_bytes > 256 * 1024 * 1024 * 1024 || !(1..=3600).contains(&copy_seconds)
    {
        return Err("native copy limits are invalid".into());
    }
    let copy_module = module_id.clone();
    let copy_started = Instant::now();
    let package = tokio::task::spawn_blocking(move || {
        package::copy_package(
            &source,
            &copy_module,
            excluded.as_deref(),
            max_bytes,
            Duration::from_secs(copy_seconds),
        )
    })
    .await??;
    println!(
        "NATIVE_LIFECYCLE module={module_id} phase=package_copy bytes={} elapsed_ms={} acquisition={} creation_scope={} source_provenance=unverified",
        package.copied_bytes,
        copy_started.elapsed().as_millis(),
        creation_mode.acquisition_label(),
        creation_mode.creation_scope(),
    );
    let env_guard = environment::NativeEnvironment::set(&package.root)?;
    let (storage, desktop) = storage_fixture::prepare(AppSettings {
        archives_root: String::new(),
        // A full lifecycle keeps the library outside the instances root so
        // uninstall can enforce the same boundary as a normal installation.
        // Start/stop-only probes retain the short path for deep native DLLs.
        servers_root: if cycle_only {
            package.root.clone()
        } else {
            package.root.join("i")
        }
        .to_string_lossy()
        .into_owned(),
        games_root: package.root.to_string_lossy().into_owned(),
        modules_root: workspace_root()
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: match std::env::var_os("LANGAME_NATIVE_STEAMCMD_ROOT") {
            Some(path) => {
                let path = PathBuf::from(path).canonicalize()?;
                if !path.is_dir() {
                    return Err("configured native tool root must be an existing directory".into());
                }
                path.to_string_lossy().into_owned()
            }
            None => package.root.join("steamcmd").to_string_lossy().into_owned(),
        },
    })?;
    initialize_database(&storage.paths).await?;
    sync_modules(&storage.paths, &descriptors).await?;
    app_storage::sync_game_installs(
        &storage.paths,
        &[app_storage::GameInstallSyncRecord {
            module_id: module_id.clone(),
            install_root: package.install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: None,
            mark_verified: true,
        }],
    )
    .await?;
    let app = tauri::test::mock_builder()
        .manage(desktop)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    let mut seed_probe = None;
    let mut first_instance = if creation_mode.instances != creation::InstanceMode::Default {
        let first = creation_mode
            .create(state.clone(), &storage, descriptor, &package, "peer")
            .await?;
        if creation_mode.instances == creation::InstanceMode::SecondPrivate {
            let probe = seed_probe::SeedProbe::prepare(&first, descriptor, &package.root).await?;
            // Keep the first program modified. The second creation must obtain
            // an official source automatically without changing this instance.
            seed_probe = Some(probe);
        }
        let config = fs::read(&first.config_file_path)?;
        let saves = save_inventory(Path::new(&first.saves_path))?;
        let programs = inventory::program_files(descriptor, &creation::program_root(&first)?)?;
        Some((first, config, saves, programs))
    } else {
        None
    };
    let initial = creation_mode
        .create(state.clone(), &storage, descriptor, &package, "n")
        .await?;
    let id = initial.summary.id.clone();
    let effective_install_root = creation::program_root(&initial)?;
    if let Some((first, ..)) = &first_instance {
        creation_mode.verify_pair(first, &initial)?;
    }
    if let Some(probe) = &seed_probe {
        probe.verify(&initial, descriptor).await?;
    }
    // Declared first so error unwinding stops the server before removing rules.
    let mut firewall_cleanup = None;
    // Declared before cleanup: error unwinding stops the owned game before the
    // guard considers deleting this fixture's newly allocated credential entries.
    let mut satisfactory_world = NativeWorld::new(
        &initial,
        &package.root,
        Arc::clone(&package.cleanup_allowed),
    )?;
    let mut satisfactory_peer_world = if scope == NativeScope::SatisfactoryIsolation {
        let (peer, ..) = first_instance
            .as_ref()
            .ok_or("Satisfactory isolation has no independent peer")?;
        NativeWorld::new(peer, &package.root, Arc::clone(&package.cleanup_allowed))?
    } else {
        None
    };
    let instance_ids: Vec<_> = std::iter::once(id.clone())
        .chain(
            first_instance
                .as_ref()
                .filter(|_| {
                    matches!(
                        scope,
                        NativeScope::TwoInstanceLifecycle | NativeScope::SatisfactoryIsolation
                    )
                })
                .map(|(peer, ..)| peer.summary.id.clone()),
        )
        .collect();
    let never_started_instance_ids = if scope == NativeScope::SatisfactoryIsolation {
        instance_ids.iter().cloned().collect()
    } else {
        Default::default()
    };
    let mut cleanup = NativeCleanup {
        state: &state,
        instance_ids,
        never_started_instance_ids,
        cleanup_allowed: Arc::clone(&package.cleanup_allowed),
    };
    let mut settings: serde_json::Map<String, Value> =
        serde_json::from_str(&initial.settings_json)?;
    settings.extend(smoke.fixture.settings.clone());
    settings.extend(diagnostics::fixture_settings_override(
        std::env::var("LANGAME_NATIVE_SETTINGS_JSON")
            .ok()
            .as_deref(),
    )?);
    if scope == NativeScope::SatisfactoryIsolation {
        // This fixture forbids settings overrides and tests isolated worlds,
        // so explicitly pin its package instead of inheriting automatic update.
        settings.insert("program_update".into(), json!({"policy": "pinned"}));
        settings.insert("auto_pause_when_empty".into(), json!(true));
        settings.insert("allow_insecure_local_api".into(), json!(false));
    }
    for name in ["public_server", "list_on_steam", "list_on_eos"] {
        if let Some(value) = settings.get_mut(name) {
            *value = if value.is_boolean() {
                json!(false)
            } else if value.is_number() {
                json!(0)
            } else {
                value.clone()
            };
        }
    }
    if module_id == "sevendaystodie" {
        settings.insert("visibility".into(), json!(0));
    }
    if module_id == "astroneer" {
        // Shipping supports ABSLOG; keep early-exit diagnostics inside this
        // disposable runtime even when the native console stays empty.
        let log = effective_install_root.join("Astro/Saved/Logs/Astro.log");
        settings.insert(
            "extra_launch_args".into(),
            json!(format!("-ABSLOG=\"{}\"", log.display())),
        );
        println!("NATIVE_DIAGNOSTIC module=astroneer file_logging=explicit_fixture_path");
    }
    if offline_dst {
        for (key, value) in [
            ("offline_cluster", json!(true)),
            ("lan_only_cluster", json!(true)),
            ("enable_caves", json!(true)),
            ("cluster_token", json!("")),
            ("disable_data_collection", json!(true)),
            ("master_world_size", json!("small")),
            ("caves_world_size", json!("small")),
        ] {
            settings.insert(key.into(), value);
        }
    }
    readiness::ark_maps::apply_fixture_maps(
        &module_id,
        &mut settings,
        std::env::var("LANGAME_NATIVE_ARK_MAPS").ok().as_deref(),
    )?;
    let details = app_storage::update_instance(
        &storage.paths,
        UpdateInstanceInput {
            id: id.clone(),
            bind_ip: "0.0.0.0".into(),
            auto_backup_on_stop: false,
            backup_retention_count: 2,
            settings_json: serde_json::to_string(&settings)?,
            ports: initial.ports,
        },
    )
    .await?;
    if let Some(frm) = &satisfactory_frm {
        frm.stage(&details, &effective_install_root, &package.root)?;
    }
    let saves_before_start = save_inventory(Path::new(&details.saves_path))?;
    if elevated_fixture {
        firewall_cleanup = Some(firewall::NativeFirewall::prepare(&details, &package.root)?);
    }
    let runtime = runtime::NativeRuntime {
        app: app.handle(),
        state: &state,
        storage: &storage,
        descriptor,
        smoke: &smoke,
        package: &package,
        settings: &settings,
        effective_install_root: &effective_install_root,
        offline_dst,
        elevated_fixture,
        satisfactory_world: satisfactory_world.as_ref(),
        satisfactory_frm: satisfactory_frm.as_ref(),
    };
    #[cfg(windows)]
    if std::env::var("LANGAME_NATIVE_MORIA_NO_LOADER_CONTROL").as_deref() == Ok("true") {
        Box::pin(moria_shutdown_control::verify(&runtime, &details)).await?;
        drop(cleanup);
        drop(app);
        drop(env_guard);
        let disposable_root = package.root.clone();
        drop(package);
        if disposable_root.exists() {
            return Err("native Moria control disposable-directory cleanup failed".into());
        }
        println!(
            "NATIVE_MORIA_CONTROL phase=complete scope=diagnostic_native_baseline cleanup=passed production_startup=not_exercised formal_save=not_called"
        );
        return Ok(());
    }
    if scope == NativeScope::SatisfactoryIsolation {
        let (peer, ..) = first_instance
            .as_ref()
            .ok_or("Satisfactory isolation has no peer instance")?;
        let peer_world = satisfactory_peer_world
            .as_ref()
            .ok_or("Satisfactory isolation has no peer native world")?;
        Box::pin(satisfactory_isolation::run(
            &runtime,
            &details,
            peer,
            peer_world,
            &mut cleanup,
        ))
        .await?;
        drop(cleanup);
        if let Some(world) = &mut satisfactory_world {
            world.finish()?;
        }
        if let Some(world) = &mut satisfactory_peer_world {
            world.finish()?;
        }
        drop(app);
        drop(env_guard);
        let disposable_root = package.root.clone();
        drop(package);
        if disposable_root.exists() {
            return Err(
                "Satisfactory isolation passed but disposable directory cleanup failed".into(),
            );
        }
        println!(
            "NATIVE_SATISFACTORY_ISOLATION phase=complete owned_process_cleanup=passed credential_cleanup=passed disposable_cleanup=passed gui=not_tested"
        );
        return Ok(());
    }
    if cycle_only {
        // This selector has its own explicit scope. It neither invokes nor
        // weakens the separate archive/restore/uninstall lifecycle acceptance.
        let first_run =
            Box::pin(runtime.run_cycle(&details, "initial", observation_seconds)).await?;
        if std::env::var("LANGAME_NATIVE_MANUAL_SAVE_RESTART").as_deref() == Ok("true") {
            if std::env::var("LANGAME_NATIVE_MANUAL_SAVE").as_deref() != Ok("true") {
                return Err("manual save restart requires the declared save probe".into());
            }
            let reloaded = read_instance_details(&storage.paths, &id).await?;
            let second_run = Box::pin(runtime.run_cycle(&reloaded, "restart", 0)).await?;
            if first_run == second_run {
                return Err("manual save restart reused its previous managed run".into());
            }
            if save_guard_probe::expected(&module_id)? {
                println!(
                    "NATIVE_SAVE_GUARD module={module_id} phase=restarted fresh_managed_run=true native_readiness=passed formal_save=not_performed content_readback=not_claimed"
                );
            } else {
                println!(
                    "NATIVE_MANUAL_SAVE module={module_id} phase=restarted fresh_managed_run=true native_readiness=passed content_readback=not_claimed"
                );
            }
        }
        drop(cleanup);
        if let Some(world) = &mut satisfactory_world {
            world.finish()?;
        }
        if let Some(mut firewall) = firewall_cleanup {
            firewall.finish()?;
        }
        drop(app);
        drop(env_guard);
        let disposable_root = package.root.clone();
        drop(package);
        if disposable_root.exists() {
            return Err("native start/stop passed but disposable directory cleanup failed".into());
        }
        println!(
            "NATIVE_LIFECYCLE module={module_id} phase=complete scope=production_start_stop cleanup=passed gui=not_tested"
        );
        return Ok(());
    }
    // Reuse the acquired official source before startup or an explicit update
    // invalidates its original package inventory. Later phases still archive,
    // restore, launch and delete the actual maintained instance.
    Box::pin(archive::verify_retirement_with_library(&runtime, &details)).await?;
    // Native DST rewrites its INI whitespace. Keep the exact manager output
    // before the first native run for the later canonical restore check.
    let dst_managed_ini = maintenance::capture_dst_managed_ini(&details)?;
    // Keep independent lifecycle phases out of this long fixture's state. A
    // cycle includes production startup and observation futures; embedding two
    // cycles exhausted the default Windows test stack in the native run.
    let first_run = if scope == NativeScope::TwoInstanceLifecycle {
        let (peer, ..) = first_instance
            .as_ref()
            .ok_or("native simultaneous lifecycle has no peer")?;
        let (run_id, peer) = Box::pin(multi_instance::run_initial(
            &runtime,
            &details,
            peer,
            observation_seconds,
        ))
        .await?;
        first_instance = Some((
            peer.clone(),
            fs::read(&peer.config_file_path)?,
            save_inventory(Path::new(&peer.saves_path))?,
            inventory::program_files(descriptor, &creation::program_root(&peer)?)?,
        ));
        run_id
    } else {
        Box::pin(runtime.run_cycle(&details, "initial", observation_seconds)).await?
    };
    let stopped_config = fs::read(&details.config_file_path)?;
    let reloaded = read_instance_details_from_storage(state.clone(), id.clone()).await?;
    settings_readback::verify_stopped(&details, &reloaded, &effective_install_root)?;
    if fs::read(&reloaded.config_file_path)? != stopped_config {
        return Err("native configuration changed during stopped readback".into());
    }
    // run_cycle captures new log/status baselines on every invocation. The
    // first run's readiness output cannot satisfy the restarted run.
    let second_run = Box::pin(runtime.run_cycle(&reloaded, "restart", 0)).await?;
    if first_run == second_run {
        return Err("native restart reused its previous persisted run identity".into());
    }
    println!(
        "NATIVE_LIFECYCLE module={module_id} phase=restarted persisted_readback=verified fresh_run=verified"
    );
    let reloaded = if let Some(restored) = Box::pin(maintenance::verify_backup_restore(
        &runtime,
        &reloaded,
        dst_managed_ini.as_ref(),
    ))
    .await?
    {
        let restored_run = Box::pin(runtime.run_cycle(&restored, "after_restore", 0)).await?;
        if restored_run == second_run {
            return Err("native restore restart reused its previous persisted run identity".into());
        }
        // Restoration can intentionally publish canonical settings. Carry only
        // the independently verified readback into the remaining lifecycle.
        restored
    } else {
        reloaded
    };
    if Box::pin(maintenance::verify_program_maintenance(
        &runtime,
        &reloaded,
        maintenance_mode,
    ))
    .await?
    {
        Box::pin(runtime.run_cycle(&reloaded, "after_program_maintenance", 0)).await?;
    }
    let saves = save_inventory(Path::new(&reloaded.saves_path))?;
    // Some servers wait for a player before creating a world. Still exercise
    // their empty-data uninstall path, without claiming native save generation.
    let observation = if saves.is_empty() {
        "none"
    } else if saves == saves_before_start {
        "unchanged"
    } else {
        "generated"
    };
    println!("NATIVE_LIFECYCLE module={module_id} phase=stopped save_observation={observation}");
    let backup = if saves.is_empty() {
        None
    } else {
        let backup = crate::commands::create_instance_backup(state.clone(), id.clone()).await?;
        if backup.file_count == 0
            || save_inventory(&Path::new(&backup.backup_path).join("saves"))? != saves
        {
            return Err("native save backup does not contain the stopped save bytes".into());
        }
        Some(backup)
    };
    let stopped_config = fs::read(&reloaded.config_file_path)?;
    let native_configs = descriptor
        .storage
        .retained_paths
        .iter()
        .map(|relative| {
            let path = effective_install_root.join(relative);
            save_inventory(&path).map(|inventory| (path, inventory))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let retained_root = creation::instance_root(&reloaded)?.join("installation-retained");
    let retained_installation = save_inventory(&retained_root)?;
    let programs = inventory::program_files(descriptor, &effective_install_root)?;
    let library = app_storage::read_library_program_install(&storage.paths, &module_id)
        .await?
        .ok_or("native uninstall has no registered library")?;
    let shared = !crate::commands::commands_program_storage::shared_program_references(
        &storage.paths,
        &module_id,
        &library.install_root,
    )
    .await?
    .is_empty();
    let uninstall = uninstall_module_game(app.handle().clone(), module_id.clone()).await;
    if shared {
        let error = uninstall
            .err()
            .ok_or("library uninstall succeeded while a shared instance still references it")?;
        if !error.contains("仍有实例使用此服务器程序") {
            return Err(
                "library uninstall failed without reaching the shared-reference guard".into(),
            );
        }
    } else {
        uninstall?;
    }
    if inventory::program_files(descriptor, &effective_install_root)? != programs
        || save_inventory(&retained_root)? != retained_installation
    {
        return Err(
            "library uninstall changed an instance program or retained installation data".into(),
        );
    }
    if save_inventory(Path::new(&reloaded.saves_path))? != saves {
        return Err("native uninstallation changed declared save data".into());
    }
    if fs::read(&reloaded.config_file_path)? != stopped_config {
        return Err("native uninstallation changed instance configuration".into());
    }
    for (path, inventory) in &native_configs {
        if save_inventory(path)? != *inventory {
            return Err("native uninstallation changed retained native configuration".into());
        }
    }
    println!(
        "NATIVE_LIFECYCLE module={module_id} phase=library_uninstall outcome={} program_entries=preserved saves_preserved={} native_config_files_preserved={}",
        if shared {
            "shared_reference_rejected"
        } else {
            "library_only_instance_program_unchanged"
        },
        saves.len(),
        native_configs
            .iter()
            .map(|(_, files)| files.len())
            .sum::<usize>()
    );
    // Capture the stopped projection, including any game-owned roster written
    // during the last run. Archive restoration must match this exact snapshot;
    // comparing creation-time settings would reject valid native projections.
    let stopped_details = read_instance_details_from_storage(state.clone(), id.clone()).await?;
    settings_readback::verify_stopped(&reloaded, &stopped_details, &effective_install_root)?;
    if fs::read(&stopped_details.config_file_path)? != stopped_config {
        return Err("native configuration changed before archive capture".into());
    }
    let deleted = archive_instance_record(app.handle().clone(), id.clone()).await?;
    let archive = PathBuf::from(
        deleted
            .archived_instance_root
            .as_ref()
            .ok_or("instance archiving did not create an archive")?,
    );
    archive::verify_archived_native_data(
        &storage.paths,
        &deleted.archive_id,
        Path::new(&reloaded.saves_path),
        Path::new(&deleted.previous_instance_root),
        &archive,
        &saves,
        "save data",
    )
    .await?;
    for (path, inventory) in &native_configs {
        archive::verify_archived_native_data(
            &storage.paths,
            &deleted.archive_id,
            path,
            Path::new(&deleted.previous_instance_root),
            &archive,
            inventory,
            "retained native configuration",
        )
        .await?;
    }
    let backup_preserved = backup.as_ref().is_none_or(|backup| {
        archive
            .join("backups")
            .join(&backup.backup_id)
            .join("backup.json")
            .is_file()
    });
    if !backup_preserved {
        return Err("native instance archiving did not retain saves and backup".into());
    }
    if let Some(backup) = &backup
        && save_inventory(
            &archive
                .join("backups")
                .join(&backup.backup_id)
                .join("saves"),
        )? != saves
    {
        return Err("native instance archiving changed backed-up save bytes".into());
    }
    if fs::read(archive.join("config/instance.json"))? != stopped_config
        || save_inventory(&archive.join("installation-retained"))? != retained_installation
        || Path::new(&deleted.previous_instance_root).exists()
        || list_instances(&storage.paths)
            .await?
            .iter()
            .any(|instance| instance.id == id)
    {
        return Err("native instance archiving did not archive configuration and retained installation data or remove the active record".into());
    }
    println!("NATIVE_LIFECYCLE module={module_id} phase=archived archive=verified");
    let restored = Box::pin(archive::restore_and_verify(
        &runtime,
        &stopped_details,
        &archive,
        &saves,
        &programs,
    ))
    .await?;
    Box::pin(runtime.run_cycle(&restored, "after_archive_restore", 0)).await?;
    Box::pin(archive::delete_restored(&runtime, &restored)).await?;
    if let Some((first, config, saves, programs)) = first_instance {
        if let Some(probe) = &seed_probe {
            probe.verify_first_preserved()?;
        }
        if read_active_instance_run(&storage.paths, &first.summary.id)
            .await?
            .is_some()
            || fs::read(&first.config_file_path)? != config
            || save_inventory(Path::new(&first.saves_path))? != saves
            || inventory::program_files(descriptor, &creation::program_root(&first)?)? != programs
        {
            return Err(
                "native lifecycle changed its retained peer's program, configuration or saves"
                    .into(),
            );
        }
        let retained_program = Box::pin(archive::delete_restored(&runtime, &first)).await?;
        if !list_instances(&storage.paths).await?.is_empty() {
            return Err("native peer deletion did not release its instance association".into());
        }
        if let Some(probe) = &seed_probe {
            probe.verify_first_deleted(retained_program)?;
        }
        println!(
            "NATIVE_LIFECYCLE module={module_id} phase=first_instance_deleted library=preserved remaining_instances=0"
        );
    }
    if !list_instances(&storage.paths).await?.is_empty() {
        return Err("native final library removal still has active instance records".into());
    }
    let removed = uninstall_module_game(app.handle().clone(), module_id.clone()).await?;
    if removed.module_id != module_id
        || removed.install_state != InstallState::NotInstalled
        || removed.executable_exists
        || !removed.cleanup.retained_installs.is_empty()
    {
        return Err("native final library uninstall did not release every unused program".into());
    }
    println!(
        "NATIVE_LIFECYCLE module={module_id} phase=library_final_uninstall unused_programs=removed retained_data_paths={}",
        removed.cleanup.preserved_data_paths.len(),
    );
    if std::env::var_os("LANGAME_NATIVE_STEAMCMD_ROOT").is_none()
        && package.root.join("steamcmd/steamcmd.exe").is_file()
    {
        let before = probe_steamcmd_status(state.clone())?;
        if !before.ready || !before.can_uninstall {
            return Err("native SteamCMD acquisition did not produce a ready managed tool".into());
        }
        let removed = uninstall_steamcmd(state.clone()).await?;
        if removed.executable_exists
            || removed.can_uninstall
            || package.root.join("steamcmd").exists()
        {
            return Err("native SteamCMD uninstall did not remove its owned tool directory".into());
        }
        println!(
            "NATIVE_LIFECYCLE module={module_id} phase=steamcmd_uninstall managed_tool=removed"
        );
    }
    drop(cleanup);
    if let Some(world) = &mut satisfactory_world {
        world.finish()?;
    }
    if let Some(mut firewall) = firewall_cleanup {
        firewall.finish()?;
        println!("NATIVE_LIFECYCLE module={module_id} phase=firewall_cleaned exact_rules=verified");
    }
    drop(app);
    drop(env_guard);
    let disposable_root = package.root.clone();
    drop(package);
    if disposable_root.exists() {
        return Err("native lifecycle passed but disposable directory cleanup failed".into());
    }
    println!(
        "NATIVE_LIFECYCLE module={module_id} phase=complete cleanup=passed lifecycle_scope=production_start_stop_restart creation_scope={} acquisition={} gui=not_tested",
        creation_mode.creation_scope(),
        creation_mode.acquisition_label(),
    );
    Ok(())
}
