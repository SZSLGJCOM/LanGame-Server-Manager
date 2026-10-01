use super::*;

pub(super) struct NativeRuntime<'a, 's, R: tauri::Runtime> {
    pub app: &'a tauri::AppHandle<R>,
    pub state: &'a tauri::State<'s, DesktopState>,
    pub storage: &'a StorageBootstrap,
    pub descriptor: &'a ModuleDescriptor,
    pub smoke: &'a readiness::SmokeManifest,
    pub package: &'a package::NativePackage,
    pub settings: &'a serde_json::Map<String, Value>,
    pub effective_install_root: &'a Path,
    pub offline_dst: bool,
    pub elevated_fixture: bool,
}

impl<R: tauri::Runtime> NativeRuntime<'_, '_, R> {
    pub async fn run_cycle(
        &self,
        details: &InstanceDetails,
        cycle: &str,
        observation_seconds: u64,
    ) -> Result<i64, Box<dyn std::error::Error>> {
        let Self {
            state,
            storage,
            descriptor,
            smoke,
            package,
            settings,
            effective_install_root,
            offline_dst,
            elevated_fixture,
            ..
        } = self;
        let state = *state;
        let offline_dst = *offline_dst;
        let elevated_fixture = *elevated_fixture;
        let id = details.summary.id.as_str();
        let module_id = &details.summary.module_id;
        if cycle == "initial" {
            smoke.require_fresh_world(effective_install_root, &package.root)?;
            if std::env::var("LANGAME_NATIVE_ARK_CREATURES").as_deref() == Ok("true")
                && matches!(
                    module_id.as_str(),
                    "arksurvivalevolved" | "arksurvivalascended"
                )
            {
                let result = crate::commands::commands_ark_tools::prepare_ark_tools(
                    state.clone(),
                    crate::commands::commands_ark_tools::PrepareInput {
                        instance_id: id.into(),
                        allow_matching_symbols_download: true,
                    },
                )
                .await?;
                if !result.installed {
                    return Err("Native ARK extension preparation did not complete".into());
                }
                println!(
                    "NATIVE_ARK_CREATURE module={module_id} phase=extension_installed isolated=true"
                );
            }
        }
        let offsets = smoke.log_baselines(details, effective_install_root)?;
        let preview = if offline_dst {
            Some(
                crate::commands::commands_dst_world_state::preview_dontstarve_world_start(
                    state.clone(),
                    id.to_owned(),
                )
                .await?,
            )
        } else {
            None
        };
        package.cleanup_allowed.store(false, Ordering::SeqCst);
        let start_requested = Instant::now();
        let started = match start_instance_process_after_reconcile(
            None,
            state,
            storage,
            id.to_owned(),
            "manual",
            preview,
        )
        .await
        {
            Ok(started) => started,
            Err(error) => {
                eprintln!(
                    "NATIVE_DIAGNOSTIC module={module_id} phase=start_failed elapsed_ms={}",
                    start_requested.elapsed().as_millis(),
                );
                let mut logs = pending_start_console_log_snapshot(state, id, 80)
                    .into_iter()
                    .collect::<Vec<_>>();
                match diagnostics::failed_run_logs(details, effective_install_root, &package.root) {
                    Ok(recorded) => logs.extend(recorded),
                    Err(detail) => eprintln!("NATIVE_DIAGNOSTIC phase=start log_read={detail}"),
                }
                print_native_failure("start", &error, &logs, settings, &package.root);
                return Err(
                    "native startup failed; bounded redacted diagnostics printed above".into(),
                );
            }
        };
        println!(
            "NATIVE_LIFECYCLE module={module_id} phase=started cycle={cycle} processes={} bind=default firewall={} elapsed_ms={}",
            started.process_count,
            if elevated_fixture {
                "owned_elevated_rules"
            } else {
                "non_elevated_policy"
            },
            start_requested.elapsed().as_millis(),
        );
        let running = read_instance_details(&storage.paths, id).await?;
        let readiness_started = Instant::now();
        if let Err(error) = smoke
            .wait_ready(
                descriptor,
                &running,
                effective_install_root,
                &package.root,
                offsets,
            )
            .await
        {
            eprintln!(
                "NATIVE_DIAGNOSTIC module={module_id} phase=readiness_failed elapsed_ms={}",
                readiness_started.elapsed().as_millis(),
            );
            diagnostics::report_failed_readiness(
                descriptor,
                &running,
                effective_install_root,
                &package.root,
            )
            .await;
            let mut logs = started
                .processes
                .iter()
                .take(8)
                .map(|process| app_storage::read_log_path_snapshot(process.log_path.clone(), 80))
                .collect::<Vec<_>>();
            match diagnostics::failed_run_logs(details, effective_install_root, &package.root) {
                Ok(recorded) => logs.extend(recorded),
                Err(detail) => eprintln!("NATIVE_DIAGNOSTIC phase=readiness log_read={detail}"),
            }
            print_native_failure("readiness", &error, &logs, settings, &package.root);
            return Err(
                "native readiness failed; bounded redacted diagnostics printed above".into(),
            );
        }
        println!(
            "NATIVE_LIFECYCLE module={module_id} phase=native_ready cycle={cycle} declared_probes=passed variant={}",
            if offline_dst {
                "offline_lan"
            } else {
                "isolated"
            }
        );
        Box::pin(player_counts::verify(self.app, state, descriptor, &running)).await?;
        Box::pin(gm_tools::verify(self, &running, cycle)).await?;
        Box::pin(theforest_probe::verify(self, &running)).await?;
        diagnostics::report_success(
            "ready",
            details,
            effective_install_root,
            &package.root,
            settings,
        )
        .await?;
        if observation_seconds > 0 {
            println!(
                "NATIVE_LIFECYCLE module={module_id} phase=diagnostic_observation cycle={cycle} seconds={observation_seconds}"
            );
            tokio::time::sleep(Duration::from_secs(observation_seconds)).await;
            diagnostics::report_success(
                "observed",
                details,
                effective_install_root,
                &package.root,
                settings,
            )
            .await?;
        }
        let stop_started = Instant::now();
        let stopped = match stop_instance_process(state.clone(), id.to_owned()).await {
            Ok(stopped) => stopped,
            Err(error) => {
                theforest_probe::report_stop_failure(state, &running);
                let logs =
                    diagnostics::failed_run_logs(&running, effective_install_root, &package.root)
                        .unwrap_or_else(|detail| {
                            eprintln!("NATIVE_DIAGNOSTIC phase=stop log_read={detail}");
                            Vec::new()
                        });
                print_native_failure("stop", &error, &logs, settings, &package.root);
                return Err(error.into());
            }
        };
        theforest_probe::verify_stopped(&running, &stopped, stop_started.elapsed())?;
        diagnostics::report_success(
            "stopped",
            details,
            effective_install_root,
            &package.root,
            settings,
        )
        .await?;
        if read_active_instance_run(&storage.paths, id)
            .await?
            .is_some()
        {
            return Err("native stop left an active run".into());
        }
        if state
            .runtime_supervisor
            .lock()
            .map_err(|_| "runtime supervisor lock poisoned")?
            .is_tracked(id)
            || state
                .pending_runtime_start_instance_ids()?
                .iter()
                .any(|pending| pending == id)
        {
            return Err("native stop left a tracked or pending process".into());
        }
        package.cleanup_allowed.store(true, Ordering::SeqCst);
        let run_id = running
            .active_run
            .as_ref()
            .ok_or("native ready run has no persisted identity")?
            .run_id;
        println!(
            "NATIVE_LIFECYCLE module={module_id} phase=cycle_stopped cycle={cycle} run_id={run_id} active_run=none"
        );
        Ok(run_id)
    }
}
pub(super) fn print_native_failure(
    phase: &str,
    error: &str,
    logs: &[LogTailSnapshot],
    settings: &serde_json::Map<String, Value>,
    root: &Path,
) {
    let secret_keys = [
        "password",
        "secret",
        "token",
        "api_key",
        "apikey",
        "authorization",
        "ticket",
        "gameid",
        "game_id",
        "game id",
    ];
    let secrets = settings
        .iter()
        .filter(|(key, _)| {
            secret_keys
                .iter()
                .any(|name| key.to_ascii_lowercase().contains(name))
        })
        .filter_map(|(_, value)| value.as_str())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    for (index, text) in std::iter::once(error.to_owned())
        .chain(logs.iter().map(|log| log.lines.join("\n")))
        .enumerate()
    {
        let mut redacted = text
            .lines()
            .map(|line| {
                let lower = line.to_ascii_lowercase();
                if secret_keys.iter().any(|key| lower.contains(key))
                    || lower.contains("settings_json")
                    || diagnostics::contains_private_runtime_identity(line)
                {
                    "[sensitive diagnostic line omitted]".to_owned()
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        for secret in &secrets {
            redacted = redacted.replace(secret, "[redacted]");
        }
        redacted = diagnostics::redact_fixture_paths(redacted, root);
        let mut start = redacted.len().saturating_sub(16 * 1024);
        while !redacted.is_char_boundary(start) {
            start += 1;
        }
        eprintln!(
            "NATIVE_DIAGNOSTIC phase={phase} stream={index}\n{}",
            &redacted[start..]
        );
    }
}
