use super::*;
use app_core::{ProgramCleanupResult, ProgramCleanupRetention};
use app_storage::ProgramInstallRecord;

/// The caller owns the module lifecycle lease throughout dependency checks,
/// staging, database publication and cleanup. Never acquire that lease twice.
pub(super) async fn cleanup_libraries(
    paths: &app_storage::StoragePaths,
    descriptor: &ModuleDescriptor,
    keep_base: bool,
    guard: &app_steamcmd::GameInstallLifecycleGuard,
) -> Result<ProgramCleanupResult, String> {
    for pending in app_storage::program_removals_db::list(paths, &descriptor.summary.id)
        .await
        .map_err(|error| error.to_string())?
    {
        guard
            .ensure_scope(&descriptor.summary.id, &pending.source_root)
            .map_err(|error| error.to_string())?;
    }
    let mut result =
        install_removal::recover_library_removals(paths, &descriptor.summary.id).await?;
    let plan = app_storage::plan_module_library_cleanup(paths, descriptor, keep_base)
        .await
        .map_err(|error| error.to_string())?;
    for record in plan.installations {
        if Some(record.id) == plan.keep_install_id {
            continue;
        }
        if let Err(error) = guard.ensure_scope(&descriptor.summary.id, &record.install_root) {
            retain(&mut result, &record, error.to_string());
            continue;
        }
        if let Err(error) = install_removal::validate_library_removal(
            paths,
            &record.module_id,
            record.id,
            &record.install_root,
        )
        .await
        {
            retain(&mut result, &record, error);
            continue;
        }
        let references = match super::super::commands_program_storage::shared_program_references(
            paths,
            &descriptor.summary.id,
            &record.install_root,
        )
        .await
        {
            Ok(references) => references,
            Err(error) => {
                retain(&mut result, &record, error);
                continue;
            }
        };
        if !references.is_empty() {
            retain(&mut result, &record, "in_use");
            continue;
        }
        if let Err(error) =
            app_storage::ensure_program_archive_dependencies(paths, &record.install_root).await
        {
            // Keep the detailed source error: corrupt recovery metadata and a
            // normal archive dependency both require retaining this program.
            retain(&mut result, &record, error.to_string());
            continue;
        }
        if record.install_state == InstallState::NotInstalled {
            // Data-only remnants were deliberately retained by an earlier
            // uninstall. They are never reinterpreted as disposable payload.
            continue;
        }
        let mut protected =
            match collect_protected_install_data_paths(paths, descriptor, &record.install_root)
                .await
            {
                Ok(protected) => protected,
                Err(error) => {
                    retain(&mut result, &record, error);
                    continue;
                }
            };
        let unresolved = protected
            .iter()
            .filter(|item| item.path.is_none())
            .cloned()
            .collect::<Vec<_>>();
        if let Err(error) = reject_protected_install_data(
            ModuleInstallOperation::Uninstall,
            &descriptor.summary.name,
            &record.install_root,
            &unresolved,
        ) {
            retain(&mut result, &record, error);
            continue;
        }
        if keep_base && record.install_root.exists() {
            let root = record.install_root.clone();
            let module = descriptor.clone();
            let retained = tokio::task::spawn_blocking(move || {
                app_storage::library_cleanup_retained_paths(&root, &module)
            })
            .await
            .map_err(|error| format!("program cleanup inspection task failed: {error}"))
            .and_then(|result| result.map_err(|error| error.to_string()));
            let retained = match retained {
                Ok(retained) => retained,
                Err(error) => {
                    retain(&mut result, &record, error);
                    continue;
                }
            };
            let Some(retained) = retained else {
                retain(&mut result, &record, "unverified_package");
                continue;
            };
            protected.extend(retained.into_iter().map(|path| ProtectedInstallDataPath {
                source: String::from("保留未经确认或已修改的文件"),
                path: Some(path),
            }));
        }
        if let Some(process) = &descriptor.process {
            let executable = record.install_root.join(&process.executable);
            if protected
                .iter()
                .filter_map(|item| item.path.as_deref())
                .any(|path| path_is_same_or_within(&executable, path))
            {
                retain(
                    &mut result,
                    &record,
                    "服务器可执行文件包含在需要保留的文件中，已保留整套程序。",
                );
                continue;
            }
        }
        let removed =
            match install_removal::remove_library_program(paths, &record, &protected).await {
                Ok(removed) => removed,
                Err(error) if keep_base => {
                    retain(&mut result, &record, error);
                    continue;
                }
                Err(error) => {
                    return Err(format!(
                        "清理 {} 失败：{error}；已移除的程序目录：{}",
                        record.install_root.display(),
                        result.removed_install_roots.join("；")
                    ));
                }
            };
        result
            .removed_install_roots
            .push(record.install_root.to_string_lossy().into_owned());
        result
            .preserved_data_paths
            .extend(removed.preserved_data_paths);
    }
    result.preserved_data_paths.sort();
    result.preserved_data_paths.dedup();
    Ok(result)
}

fn retain(
    result: &mut ProgramCleanupResult,
    record: &ProgramInstallRecord,
    reason: impl Into<String>,
) {
    result.retained_installs.push(ProgramCleanupRetention {
        install_root: record.install_root.to_string_lossy().into_owned(),
        reason: reason.into(),
    });
}

/// Called after permanent deletion, while retirement still owns its leases.
/// A cleanup failure must not turn the already committed deletion into failure.
pub(in crate::commands) async fn after_instance_deletion(
    paths: &app_storage::StoragePaths,
    module_id: &str,
    guard: &app_steamcmd::GameInstallLifecycleGuard,
) -> ProgramCleanupResult {
    let attempt = async {
        if list_instances(paths)
            .await
            .map_err(|error| error.to_string())?
            .iter()
            .any(|instance| instance.module_id == module_id)
        {
            return Ok(ProgramCleanupResult::default());
        }
        let descriptors =
            discover_modules(&paths.modules_root).map_err(|error| error.to_string())?;
        let descriptor = find_descriptor(&descriptors, module_id)?;
        cleanup_libraries(paths, descriptor, true, guard).await
    }
    .await;
    attempt.unwrap_or_else(|reason| ProgramCleanupResult {
        retained_installs: vec![ProgramCleanupRetention {
            install_root: paths.games_root.to_string_lossy().into_owned(),
            reason,
        }],
        ..Default::default()
    })
}

pub(super) async fn uninstall_module_game_transaction(
    state: tauri::State<'_, DesktopState>,
    module_id: String,
) -> Result<app_core::ModuleUninstallResult, String> {
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    sync_modules(&storage.paths, &descriptors)
        .await
        .map_err(|error| error.to_string())?;
    let descriptor = find_descriptor(&descriptors, &module_id)?;
    let _instance_guards =
        acquire_module_instance_mutations(&state, &storage.paths, &module_id).await?;
    let before = app_storage::plan_module_library_cleanup(&storage.paths, descriptor, false)
        .await
        .map_err(|error| error.to_string())?;
    let mut roots = before
        .installations
        .iter()
        .map(|record| record.install_root.clone())
        .collect::<Vec<_>>();
    let default = build_game_install_sync_record(&storage.settings, descriptor, false, None);
    roots.push(PathBuf::from(&default.install_root));
    let lifecycle_guard = app_steamcmd::acquire_game_install_lifecycle(&module_id, &roots)
        .await
        .map_err(|error| steamcmd_error_message(&error))?;
    let running = running_instance_labels_for_module(&storage.paths, &module_id).await?;
    ensure_module_unused_for_uninstall(&descriptor.summary, &running)?;
    // Discover the canonical library too, including an installation that has
    // never been selected as the newest override. Registration rejects overlap.
    let default = build_game_install_sync_record(&storage.settings, descriptor, false, None);
    sync_game_installs(&storage.paths, &[default])
        .await
        .map_err(|error| error.to_string())?;
    let job_id = new_background_job_id("module-uninstall", &module_id);
    let label = format!("Uninstall {}", descriptor.summary.name);
    insert_background_job(
        &state,
        &storage,
        BackgroundJob {
            id: job_id.clone(),
            kind: JobKind::UninstallGame,
            label: label.clone(),
            status: JobStatus::Running,
            progress_percent: 5.0,
            install_progress: None,
            cancellable: false,
            cancel_requested: false,
            target_id: Some(module_id.clone()),
            detail: Some(String::from("正在清理该游戏的库程序目录")),
            output_excerpt: None,
        },
    )?;
    set_module_install_state(&state, &module_id, InstallState::Uninstalling)?;
    let cleaned = cleanup_libraries(&storage.paths, descriptor, false, &lifecycle_guard)
        .await
        .and_then(|cleanup| {
            if cleanup.removed_install_roots.is_empty() && !cleanup.retained_installs.is_empty() {
                let reasons = cleanup
                    .retained_installs
                    .iter()
                    .map(|retained| {
                        let reason = if retained.reason == "in_use" {
                            "仍有实例使用此服务器程序"
                        } else {
                            &retained.reason
                        };
                        format!("{}：{reason}", retained.install_root)
                    })
                    .collect::<Vec<_>>()
                    .join("；");
                Err(format!("没有可安全卸载的库程序：{reasons}"))
            } else {
                Ok(cleanup)
            }
        });
    let override_root = load_module_install_root_override(&storage.paths, &module_id).await;
    let mut refreshed =
        module_summary_with_install_state(&storage.settings, descriptor, override_root.as_deref());
    let result = match cleaned {
        Ok(cleanup) => {
            let records =
                app_storage::plan_module_library_cleanup(&storage.paths, descriptor, false)
                    .await
                    .map_err(|error| error.to_string());
            records.map(|records| {
                let probes = records
                    .installations
                    .iter()
                    .map(|record| {
                        probe_module_install_state_with_override(
                            &storage.settings,
                            &module_id,
                            descriptor.summary.steam_app_id,
                            descriptor.install.as_ref(),
                            descriptor.process.as_ref(),
                            Some(&record.install_root.to_string_lossy()),
                        )
                    })
                    .collect::<Vec<_>>();
                let installed = probes
                    .iter()
                    .any(|probe| probe.install_state == InstallState::Installed);
                let install_state = if installed {
                    InstallState::Installed
                } else {
                    probes
                        .iter()
                        .find(|probe| probe.install_state != InstallState::NotInstalled)
                        .map(|probe| probe.install_state.clone())
                        .unwrap_or(InstallState::NotInstalled)
                };
                app_core::ModuleUninstallResult {
                    module_id: module_id.clone(),
                    install_state,
                    executable_exists: probes.iter().any(|probe| probe.executable_exists),
                    cleanup,
                }
            })
        }
        Err(error) => Err(error),
    };
    if let Ok(result) = &result {
        refreshed.install_state = result.install_state.clone();
    }
    mutate_app_state(&state, |app_state| {
        merge_module_install_state(app_state, &refreshed);
        if let Some(job) = app_state.jobs.iter_mut().find(|job| job.id == job_id) {
            match &result {
                Ok(result) => {
                    job.status = JobStatus::Completed;
                    job.progress_percent = 100.0;
                    job.detail = Some(format!(
                        "已移除 {} 套库程序，保留 {} 套仍有用途的程序",
                        result.cleanup.removed_install_roots.len(),
                        result.cleanup.retained_installs.len()
                    ));
                    job.output_excerpt = serde_json::to_string(&result.cleanup).ok();
                }
                Err(error) => {
                    job.status = JobStatus::Failed;
                    job.detail = Some(error.clone());
                    job.output_excerpt = Some(error.clone());
                }
            }
        }
    })?;
    append_desktop_app_log(
        &storage,
        if result.is_ok() { "info" } else { "error" },
        "module.uninstall.completed",
        "Module library cleanup finished",
        json!({"module_id": module_id, "result": result.as_ref().ok(), "error": result.as_ref().err()}),
    );
    result
}
