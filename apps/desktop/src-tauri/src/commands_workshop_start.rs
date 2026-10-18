//! Refresh only this instance's active Workshop packages before native loading.
use super::workshop_start_plan::collect_start_workshop_item_ids;
use super::*;
use crate::steam_workshop_account_sessions::SteamAccountLoginTarget;

#[path = "commands_workshop_start_cache.rs"]
mod cache;
#[path = "commands_workshop_start_missing.rs"]
mod missing;
#[path = "commands_workshop_start_policy.rs"]
mod policy;
use policy::{UpdateFailure, cache_warning, download_failure};

pub(in crate::commands) struct WorkshopStartOutcome {
    pub install_guard: app_steamcmd::GameInstallLifecycleGuard,
    pub warnings: Vec<app_core::RuntimeStartWarning>,
}

pub(in crate::commands) struct WorkshopStartRequest<'a> {
    pub descriptor: &'a ModuleDescriptor,
    pub instance: &'a InstanceDetails,
    pub install_guard: app_steamcmd::GameInstallLifecycleGuard,
    pub reservation: &'a RuntimeStartReservationLease,
    pub context: super::super::commands_runtime_prestart_update::PrestartUpdateLogContext<'a>,
}

fn report(
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
    context: &super::super::commands_runtime_prestart_update::PrestartUpdateLogContext<'_>,
    outcome: &str,
    message: &str,
) {
    if let Some(path) = context.console_log_path {
        let _ = append_prestart_update_console_line(path, message, "");
    }
    append_desktop_app_log(
        storage,
        match outcome {
            "failed" => "error",
            "using_cache" => "warn",
            _ => "info",
        },
        &format!("instance.prestart_workshop.{outcome}"),
        message,
        json!({"source": context.source, "instance_id": instance.summary.id,
            "module_id": instance.summary.module_id, "outcome": outcome}),
    );
}

/// The start owner already holds instance/program leases. Never enter the public
/// Mod command here: it would reacquire those locks and could detach the program.
pub(in crate::commands) async fn update_before_start(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    request: WorkshopStartRequest<'_>,
) -> Result<WorkshopStartOutcome, String> {
    let Some(workshop) = request
        .descriptor
        .workshop
        .as_ref()
        .filter(|spec| spec.provider.eq_ignore_ascii_case("steam"))
    else {
        return Ok(WorkshopStartOutcome {
            install_guard: request.install_guard,
            warnings: Vec::new(),
        });
    };
    let consumer_app_id = workshop
        .consumer_app_id
        .filter(|id| *id > 0)
        .ok_or("Steam Workshop module is missing its consumer app ID")?;
    let settings: Value = serde_json::from_str(&request.instance.settings_json)
        .map_err(|error| format!("Cannot read Workshop startup settings: {error}"))?;
    let root = PathBuf::from(
        super::super::commands_runtime_lifecycle::private_runtime_install_root(request.instance)?,
    );
    let install_guard = Arc::new(request.install_guard);
    let deployment_target = cache::DeploymentTarget {
        descriptor: request.descriptor,
        instance: request.instance,
        reservation: request.reservation,
    };
    install_guard
        .ensure_scope(&request.instance.summary.module_id, &root)
        .map_err(|error| steamcmd_error_message(&error))?;
    let inventory = if matches!(
        request.instance.summary.module_id.as_str(),
        "palworld" | "squad"
    ) {
        Some(
            read_manual_mod_inventory_inner(
                storage,
                request.reservation.storage_operation(),
                request.instance.summary.id.clone(),
            )
            .await?,
        )
    } else {
        None
    };
    let mut ids = collect_start_workshop_item_ids(
        &request.instance.summary.module_id,
        &settings,
        inventory.as_ref(),
    )?;
    let mut early_steamcmd = None;
    if request.instance.summary.module_id == "palworld"
        && let Some(inventory) = &inventory
    {
        let wanted = missing::missing_names(&settings, inventory)?;
        if !wanted.is_empty() {
            let steamcmd = tokio::select! {
                biased;
                _=request.reservation.cancelled()=>return Err("启动前模组更新已取消，实例未启动。".into()),
                guard=install_guard.acquire_steamcmd(&storage.settings)=>guard.map_err(|error|steamcmd_error_message(&error))?,
            };
            let steamcmd = Arc::new(steamcmd);
            let cancellation = app_steamcmd::InstallCancellation::new();
            let mut excluded = super::workshop_start_plan::read_markers(
                &settings,
                "steam_workshop_removed_mod_ids",
            )?;
            excluded.extend(super::workshop_start_plan::read_markers(
                &settings,
                "steam_workshop_disabled_mod_ids",
            )?);
            let inspect = missing::resolve(
                request.reservation.storage_operation(),
                install_guard.clone(),
                steamcmd.clone(),
                missing::Request {
                    settings: storage.settings.clone(),
                    root: root.clone(),
                    app: consumer_app_id,
                    wanted,
                    removed: excluded,
                },
                cancellation.clone(),
            );
            tokio::pin!(inspect);
            let recovered = tokio::select! {
                biased;
                _=request.reservation.cancelled()=> { cancellation.cancel(); let _=inspect.await;return Err("启动前模组更新已取消，实例未启动。".into()); },
                result=&mut inspect=>result?,
            };
            ids.extend(recovered);
            ids.sort();
            ids.dedup();
            early_steamcmd = Some(steamcmd);
        }
    }
    if ids.is_empty() {
        return Ok(WorkshopStartOutcome {
            install_guard: Arc::try_unwrap(install_guard).map_err(|_| {
                "Workshop inspection has not released its lifecycle lease".to_owned()
            })?,
            warnings: Vec::new(),
        });
    }
    // Mod commands normally detach shared programs. Startup must preserve the
    // confirmed program identity, so shared bindings must be detached explicitly.
    if super::super::commands_program_storage::program_mode(request.instance)?
        != app_core::InstanceProgramMode::Independent
        && !app_storage::instance_uses_exclusive_program(
            super::super::commands_program_storage::instance_root(request.instance)?,
        )
        .map_err(|error| error.to_string())?
    {
        return Err("启动前模组更新需要独立程序；请先在程序管理中将此实例切换为独立程序。".into());
    }
    app_storage::ensure_program_archive_dependencies(&storage.paths, &root)
        .await
        .map_err(|error| format!("启动前模组更新受归档版本保护：{error}"))?;
    let job_id = new_background_job_id("instance-workshop-update", &request.instance.summary.id);
    let lease = InstallationJobLease::begin(state, job_id.clone())?;
    insert_background_job(
        state,
        storage,
        BackgroundJob {
            id: job_id.clone(),
            kind: JobKind::DownloadWorkshop,
            label: format!(
                "启动前更新 {} 的创意工坊模组",
                request.instance.summary.name
            ),
            status: JobStatus::Running,
            progress_percent: 1.0,
            install_progress: Some(queued_install_progress()),
            cancellable: true,
            cancel_requested: false,
            target_id: Some(request.instance.summary.id.clone()),
            detail: Some(format!("正在向 Steam 确认 {} 个模组的最新版本…", ids.len())),
            output_excerpt: None,
            warnings: Vec::new(),
        },
    )?;
    report(
        storage,
        request.instance,
        &request.context,
        "started",
        &format!(
            "正在检查并更新 {} 个创意工坊模组（包括已有缓存）…",
            ids.len()
        ),
    );
    let cancellation = lease.cancellation();
    let operation = request.reservation.storage_operation();
    let account_target = SteamAccountLoginTarget::Instance(request.instance.summary.id.clone());
    let update = async {
        let steamcmd = if let Some(steamcmd) = early_steamcmd {
            Arc::try_unwrap(steamcmd).map_err(|_| {
                "Workshop cache inspection has not released its SteamCMD lease".to_owned()
            })?
        } else {
            tokio::select! {
                biased;
                _ = request.reservation.cancelled() => return Err("启动前模组更新已取消，实例未启动。".into()),
                _ = cancellation.cancelled() => return Err("启动前模组更新已取消，实例未启动。".into()),
                guard = install_guard.acquire_steamcmd(&storage.settings) =>
                    guard.map_err(|error| steamcmd_error_message(&error))?,
            }
        };
        let mutation =
            std::sync::Arc::new(workshop_download::WorkshopMutation::for_locked_install(
                Arc::try_unwrap(install_guard).map_err(|_| {
                    "Workshop cache inspection has not released its lifecycle lease".to_owned()
                })?,
                steamcmd,
            ));
        let mut account_name = state
            .steam_workshop_account_sessions
            .workshop_download_account(
                &account_target,
                &storage.paths.steamcmd_root,
                &storage.paths.app_data_root,
            )?;
        // Freeze and hash both native cache locations before SteamCMD can write
        // in place. Only packages with native file hashes or a previous native
        // validation receipt can authorize continuing after an update failure.
        let fallback_names = if request.instance.summary.module_id == "palworld" {
            Some(super::workshop_start_plan::enabled_palworld_package_names(
                &settings,
            )?)
        } else {
            None
        };
        let manual_target =
            if should_stage_downloaded_workshop_items(&request.descriptor.manifest_toml) {
                Some(
                    resolve_manual_mod_target(storage, request.instance.summary.id.clone(), false)
                        .await?
                        .target_path,
                )
            } else {
                None
            };
        let protection = cache::prepare(
            operation,
            mutation.clone(),
            storage.settings.clone(),
            cache::Plan {
                root: root.clone(),
                app: consumer_app_id,
                ids: ids.clone(),
                fallback_names,
                manual_target,
            },
            cancellation.clone(),
        )
        .await?;
        let (transaction, preserved, mut baselines) = match protection {
            cache::Protection::Transaction(transaction, preserved, baselines) => {
                (transaction, preserved, baselines)
            }
            cache::Protection::SkipUpdate(verified, preserved, baselines) => {
                if cancellation.is_cancelled() || request.reservation.is_cancelled() {
                    return Err("启动前模组更新已取消，实例未启动。".into());
                }
                cache::deploy(
                    storage,
                    &deployment_target,
                    mutation.clone(),
                    verified.download_result().clone(),
                    cache::DeploymentIntegrity {
                        verified: Some(verified),
                        preserved,
                        baselines,
                    },
                    cancellation.clone(),
                )
                .await
                .map_err(|error| error.message)?;
                check_palworld_deployment(storage, operation, request.instance, &settings)
                    .await
                    .map_err(|error| error.message)?;
                if cancellation.is_cancelled() || request.reservation.is_cancelled() {
                    return Err("启动前模组更新已取消，实例未启动。".into());
                }
                let mut warning = cache_warning(
                    &ids,
                    app_core::RuntimeModUpdateReason::Install,
                    request.context.console_log_path,
                );
                warning.message = "没有足够空间保护旧模组包，已跳过本次更新；缓存已通过文件哈希校验，使用已有版本继续启动，尚未确认是否为最新版。".into();
                let install_guard = Arc::try_unwrap(mutation)
                    .map_err(|_| {
                        "Workshop cache verification has not released its lifecycle lease"
                            .to_owned()
                    })?
                    .into_locked_install()?;
                return Ok(WorkshopStartOutcome {
                    install_guard,
                    warnings: vec![warning],
                });
            }
        };
        let mut progress_error = None;
        let mut progress = |update: InstallProgressUpdate| {
            if let Err(error) = lease.update(|job| apply_install_progress(job, &update)) {
                progress_error.get_or_insert(error);
                cancellation.cancel();
            }
            if let Some(path) = request.context.console_log_path
                && let Err(error) = append_prestart_update_console_line(
                    path,
                    &update.detail,
                    &update.output_excerpt,
                )
            {
                progress_error.get_or_insert(error);
                cancellation.cancel();
            }
        };
        // Native SteamCMD checks the latest item manifest even for a complete
        // local cache. Only changed content needs transferring; never missing_only.
        let downloaded = loop {
            let result =
                app_steamcmd::download_workshop_items_with_login_and_cancellation_and_progress(
                    &storage.settings,
                    app_steamcmd::SteamWorkshopDownloadRequest {
                        consumer_app_id,
                        install_root: &root,
                        ids: &ids,
                        account_name: account_name.as_deref(),
                    },
                    mutation.install(),
                    mutation.steamcmd(),
                    cancellation,
                    &mut progress,
                )
                .await;
            match result {
                Ok(result) => break Ok(result),
                Err(error)
                    if account_name.is_none()
                        && matches!(
                            error,
                            app_steamcmd::SteamCmdError::WorkshopAccountRequired { .. }
                        )
                        && !cancellation.is_cancelled()
                        && !request.reservation.is_cancelled() =>
                {
                    account_name = match state
                        .steam_workshop_account_sessions
                        .workshop_download_account(
                            &account_target,
                            &storage.paths.steamcmd_root,
                            &storage.paths.app_data_root,
                        ) {
                        Ok(account) => account,
                        Err(error) => break Err(UpdateFailure::from(error)),
                    };
                    if account_name.is_none() {
                        break Err(download_failure(error));
                    }
                }
                Err(error) => break Err(download_failure(error)),
            }
        };
        drop(progress);
        if let Err(failure) = &downloaded
            && !failure.recovery_safe
        {
            // An unconfirmed native-process exit cannot race a rollback. The
            // complete recovery copies remain for explicit, safe recovery.
            return Err(failure.message.clone());
        }
        if let Some(error) = progress_error {
            cache::rollback(operation, mutation.clone(), transaction).await?;
            return Err(error);
        }
        if cancellation.is_cancelled() || request.reservation.is_cancelled() {
            cache::rollback(operation, mutation.clone(), transaction).await?;
            return Err("启动前模组更新已取消，实例未启动。".into());
        }
        let attempt: Result<(), UpdateFailure> = async {
            let downloaded = downloaded?;
            cache::record_validation(
                operation,
                mutation.clone(),
                storage.settings.clone(),
                downloaded.clone(),
                cancellation.clone(),
            )
            .await
            .map_err(policy::deployment_failure)?;
            lease.update(|job| {
                begin_install_deployment(job);
                job.detail = Some("正在将最新模组部署到当前实例…".into());
            })?;
            baselines = cache::deploy(
                storage,
                &deployment_target,
                mutation.clone(),
                downloaded,
                cache::DeploymentIntegrity {
                    verified: None,
                    preserved: preserved.clone(),
                    baselines: baselines.clone(),
                },
                cancellation.clone(),
            )
            .await?;
            check_palworld_deployment(storage, operation, request.instance, &settings).await?;
            Ok(())
        }
        .await;
        if cancellation.is_cancelled() || request.reservation.is_cancelled() {
            cache::rollback(operation, mutation.clone(), transaction).await?;
            return Err("启动前模组更新已取消，实例未启动。".into());
        }
        let warnings = match attempt {
            Ok(()) => {
                cache::commit(operation, mutation.clone(), transaction).await?;
                Vec::new()
            }
            Err(failure) => {
                let verified = cache::rollback(operation, mutation.clone(), transaction).await?;
                let Some(reason) = failure.fallback_reason else {
                    return Err(failure.message);
                };
                let Some(verified) = verified else {
                    return Err(format!(
                        "{}\n没有可通过文件哈希校验的完整旧模组缓存，请联网完成一次验证或重新下载后再启动。",
                        failure.message
                    ));
                };
                report(
                    storage,
                    request.instance,
                    &request.context,
                    "failed",
                    &failure.message,
                );
                cache::deploy(
                    storage,
                    &deployment_target,
                    mutation.clone(),
                    verified.download_result().clone(),
                    cache::DeploymentIntegrity {
                        verified: Some(verified),
                        preserved: preserved.clone(),
                        baselines: baselines.clone(),
                    },
                    cancellation.clone(),
                )
                .await
                .map_err(|error| error.message)?;
                check_palworld_deployment(storage, operation, request.instance, &settings)
                    .await
                    .map_err(|error| error.message)?;
                if cancellation.is_cancelled() || request.reservation.is_cancelled() {
                    return Err("启动前模组更新已取消，实例未启动。".into());
                }
                vec![cache_warning(
                    &ids,
                    reason,
                    request.context.console_log_path,
                )]
            }
        };
        let install_guard = std::sync::Arc::try_unwrap(mutation)
            .map_err(|_| "Workshop deployment has not released its lifecycle lease".to_owned())?
            .into_locked_install()?;
        Ok(WorkshopStartOutcome {
            install_guard,
            warnings,
        })
    };
    tokio::pin!(update);
    let result = tokio::select! {
        biased;
        _ = request.reservation.cancelled() => { cancellation.cancel(); update.await },
        result = &mut update => result,
    };
    match result {
        Ok(outcome) => {
            lease.update(|job| {
                if outcome.warnings.is_empty() {
                    complete_install_progress(job);
                    job.detail = Some("创意工坊模组已确认并更新，继续启动。".into());
                } else {
                    fail_install_progress(job, 1.0);
                    job.detail = Some(outcome.warnings[0].message.clone());
                }
                job.warnings = outcome.warnings.clone();
            })?;
            report(
                storage,
                request.instance,
                &request.context,
                if outcome.warnings.is_empty() {
                    "completed"
                } else {
                    "using_cache"
                },
                outcome
                    .warnings
                    .first()
                    .map(|warning| warning.message.as_str())
                    .unwrap_or("创意工坊模组已确认并更新，继续启动。"),
            );
            Ok(outcome)
        }
        Err(error) => {
            lease.update(|job| {
                fail_install_progress(job, 1.0);
                if cancellation.is_cancelled() || request.reservation.is_cancelled() {
                    job.status = JobStatus::Cancelled;
                }
                job.detail = Some(error.clone());
            })?;
            report(
                storage,
                request.instance,
                &request.context,
                if cancellation.is_cancelled() || request.reservation.is_cancelled() {
                    "cancelled"
                } else {
                    "failed"
                },
                &error,
            );
            Err(error)
        }
    }
}

async fn check_palworld_deployment(
    storage: &StorageBootstrap,
    operation: &StorageContextOperationGuard,
    instance: &InstanceDetails,
    settings: &Value,
) -> Result<(), UpdateFailure> {
    if instance.summary.module_id != "palworld" {
        return Ok(());
    }
    let after =
        read_manual_mod_inventory_inner(storage, operation, instance.summary.id.clone()).await?;
    let enabled = super::workshop_start_plan::enabled_palworld_package_names(settings)?;
    let available = after
        .items
        .iter()
        .filter(|item| {
            item.file_count > 0 && item.management.as_ref().is_none_or(|state| state.enabled)
        })
        .filter_map(|item| item.inferred_id.as_deref())
        .map(|name| name.trim().to_lowercase())
        .collect::<HashSet<_>>();
    for name in enabled {
        if !available.contains(&name) {
            return Err(UpdateFailure {
                message: format!(
                    "模组部署后缺少启用的 PackageName `{name}`；请在 Mod 页面检查启用项后再启动。"
                ),
                fallback_reason: Some(app_core::RuntimeModUpdateReason::Install),
                recovery_safe: true,
            });
        }
    }
    Ok(())
}

#[cfg(all(test, windows))]
#[path = "commands_workshop_start_tests.rs"]
mod tests;
