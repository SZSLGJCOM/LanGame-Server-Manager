//! Refresh this instance's explicitly requested Thunderstore roots while its
//! startup reservation and private-program lifecycle lease remain held.
use super::super::commands_mod_staging::{
    ThunderstoreStartPlan, inspect_thunderstore_startup,
    stage_thunderstore_online_mod_sources_for_startup,
};
use super::*;

pub(in crate::commands) struct ThunderstoreStartRequest<'a> {
    pub descriptor: &'a ModuleDescriptor,
    pub instance: &'a InstanceDetails,
    pub install_guard: app_steamcmd::GameInstallLifecycleGuard,
    pub reservation: &'a RuntimeStartReservationLease,
    pub context: super::super::commands_runtime_prestart_update::PrestartUpdateLogContext<'a>,
}

pub(in crate::commands) struct ThunderstoreStartOutcome {
    pub install_guard: app_steamcmd::GameInstallLifecycleGuard,
    pub warnings: Vec<app_core::RuntimeStartWarning>,
}

fn cancelled(cancellation: &app_steamcmd::InstallCancellation) -> Result<(), String> {
    if cancellation.is_cancelled() {
        Err("启动前 Thunderstore 模组检查已取消，实例未启动。".into())
    } else {
        Ok(())
    }
}

fn warning(
    plan: &ThunderstoreStartPlan,
    reason: Option<app_core::RuntimeModUpdateReason>,
    log_path: Option<&str>,
) -> app_core::RuntimeStartWarning {
    app_core::RuntimeStartWarning {
        code: if reason.is_some() {
            app_core::RuntimeStartWarningCode::ModUpdateUsingCache
        } else {
            app_core::RuntimeStartWarningCode::ModUpdateSourceRequired
        },
        provider: Some(app_core::RuntimeModProvider::Thunderstore),
        reason: Some(reason.unwrap_or(app_core::RuntimeModUpdateReason::Unknown)),
        item_ids: if reason.is_some() {
            plan.item_ids()
        } else {
            plan.source_required.clone()
        },
        message: if reason.is_some() {
            "Thunderstore 模组更新未完成；已核验现有受管模组、依赖和加载器，将使用当前完整内容继续启动。版本可能不是最新，请稍后重新检查。".into()
        } else {
            "现有 Thunderstore 模组的文件已核验，但原始根包或版本选择尚未确认，最新版未检查。将使用当前内容继续启动，请在模组页明确选择来源和更新策略。".into()
        },
        log_path: log_path.map(str::to_owned),
        map_key: None,
        http_status: None,
    }
}

async fn inspect(
    operation: &StorageContextOperationGuard,
    install: Arc<app_steamcmd::GameInstallLifecycleGuard>,
    target: ResolvedManualModTarget,
    previous: Option<ThunderstoreStartPlan>,
    cancellation: app_steamcmd::InstallCancellation,
) -> Result<ThunderstoreStartPlan, String> {
    spawn_blocking_storage_context_task(operation, move || {
        let _install = install;
        cancelled(&cancellation)?;
        if let Some(previous) = previous {
            previous.verify_unchanged(&target)?;
        }
        let proof = inspect_thunderstore_startup(&target)?;
        cancelled(&cancellation)?;
        Ok::<_, String>(proof)
    })
    .await
    .map_err(|error| format!("Thunderstore Mod verification worker failed: {error}"))?
}

fn report(
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
    context: &super::super::commands_runtime_prestart_update::PrestartUpdateLogContext<'_>,
    warning: Option<&app_core::RuntimeStartWarning>,
) {
    let message = warning
        .map(|warning| warning.message.as_str())
        .unwrap_or("Thunderstore 模组已按原始根包和版本策略检查，继续启动。");
    if let Some(path) = context.console_log_path {
        let _ = append_prestart_update_console_line(path, message, "");
    }
    append_desktop_app_log(
        storage,
        if warning.is_some() { "warn" } else { "info" },
        "instance.prestart_thunderstore.completed",
        message,
        json!({"source":context.source,"instance_id":instance.summary.id,"module_id":instance.summary.module_id,"warning":warning.map(|warning|warning.code)}),
    );
}

/// Never call the public Mod installer here: it reacquires stopped-instance
/// locks. Preparation is temporary-only; the existing package transaction is
/// the sole publisher of payload, dependency declarations and root intent.
pub(in crate::commands) async fn update_before_start(
    _state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    request: ThunderstoreStartRequest<'_>,
) -> Result<ThunderstoreStartOutcome, String> {
    update_before_start_inner(
        _state,
        storage,
        request,
        #[cfg(test)]
        None,
    )
    .await
}

async fn update_before_start_inner(
    _state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    request: ThunderstoreStartRequest<'_>,
    #[cfg(test)] fixture: Option<&mod_online::FixtureProvider>,
) -> Result<ThunderstoreStartOutcome, String> {
    let mods = module_mods_spec_from_manifest(&request.descriptor.manifest_toml);
    let Some(source) = mods
        .as_ref()
        .and_then(|mods| mods.source.as_ref())
        .filter(|source| source.provider.eq_ignore_ascii_case("thunderstore"))
    else {
        return Ok(ThunderstoreStartOutcome {
            install_guard: request.install_guard,
            warnings: Vec::new(),
        });
    };
    let root = PathBuf::from(
        super::super::commands_runtime_lifecycle::private_runtime_install_root(request.instance)?,
    );
    let profile = super::super::commands_mod_staging::thunderstore_game_profile(
        &request.instance.summary.module_id,
    )?;
    request
        .install_guard
        .ensure_scope(&request.instance.summary.module_id, &root)
        .map_err(|error| steamcmd_error_message(&error))?;
    let target =
        resolve_manual_mod_target(storage, request.instance.summary.id.clone(), false).await?;
    if target.target_path != root.join(profile.target_relative) {
        return Err(
            "Thunderstore Mod target does not belong to this instance's private runtime".into(),
        );
    }
    let install = Arc::new(request.install_guard);
    let cancellation = app_steamcmd::InstallCancellation::new();
    let operation = request.reservation.storage_operation();
    let result: Result<Vec<app_core::RuntimeStartWarning>, String> = {
        let update = async {
            let proof = inspect(
                operation,
                install.clone(),
                target.clone(),
                None,
                cancellation.clone(),
            )
            .await?;
            if proof.packages.is_empty() {
                return Ok(Vec::new());
            }
            if !proof.source_required.is_empty() {
                let proof = inspect(
                    operation,
                    install.clone(),
                    target.clone(),
                    Some(proof),
                    cancellation.clone(),
                )
                .await?;
                return Ok(vec![warning(
                    &proof,
                    None,
                    request.context.console_log_path,
                )]);
            }
            cancelled(&cancellation)?;
            let prepared = async {
                #[cfg(test)]
                if let Some(fixture) = fixture {
                    return mod_online::prepare_thunderstore_fixture(
                        &request.instance.summary.module_id,
                        source,
                        &proof.references,
                        &root,
                        &proof.packages,
                        &cancellation,
                        fixture,
                    )
                    .await;
                }
                mod_online::prepare_thunderstore_install_for_startup(
                    &request.instance.summary.module_id,
                    source,
                    &proof.references,
                    &root,
                    &proof.packages,
                    &cancellation,
                )
                .await
            }
            .await;
            cancelled(&cancellation)?;
            let prepared = match prepared {
                Ok(prepared) => prepared,
                Err(failure) => {
                    let Some(reason) = failure.reason() else {
                        return Err(failure.message);
                    };
                    // The provider did not publish anything. Hash the actual old
                    // payload, mutable operator data and authoritative record again.
                    let proof = inspect(
                        operation,
                        install.clone(),
                        target.clone(),
                        Some(proof),
                        cancellation.clone(),
                    )
                    .await?;
                    return Ok(vec![warning(
                        &proof,
                        Some(reason),
                        request.context.console_log_path,
                    )]);
                }
            };
            if prepared.sources().is_empty() {
                inspect(
                    operation,
                    install.clone(),
                    target.clone(),
                    Some(proof),
                    cancellation.clone(),
                )
                .await?;
                return Ok(Vec::new());
            }
            app_storage::ensure_program_archive_dependencies(&storage.paths, &root)
            .await
            .map_err(|error| {
                format!(
                    "Thunderstore Mod update is protected by a preserved program archive: {error}"
                )
            })?;
            let worker_install = install.clone();
            let worker_target = target.clone();
            let worker_cancellation = cancellation.clone();
            let mut warnings = spawn_blocking_storage_context_task(operation, move || {
                let _install = worker_install;
                cancelled(&worker_cancellation)?;
                proof.verify_unchanged(&worker_target)?;
                stage_thunderstore_online_mod_sources_for_startup(
                    worker_target.clone(),
                    prepared.sources(),
                    &proof,
                    &worker_cancellation,
                )?;
                // Stage, conflicts, ownership and local IO errors are fatal. Only
                // temporary provider preparation failures may use the old proof.
                let actual = inspect_thunderstore_startup(&worker_target)?;
                cancelled(&worker_cancellation)?;
                Ok::<_, String>(if actual.source_required.is_empty() {
                    Vec::new()
                } else {
                    vec![warning(&actual, None, None)]
                })
            })
            .await
            .map_err(|error| format!("Thunderstore Mod publication worker failed: {error}"))??;
            for warning in &mut warnings {
                warning.log_path = request.context.console_log_path.map(str::to_owned);
            }
            Ok(warnings)
        };
        tokio::pin!(update);
        tokio::select! {
            biased;
            _=request.reservation.cancelled()=> {cancellation.cancel();let _=update.await;Err("启动前 Thunderstore 模组检查已取消，实例未启动。".into())},
            result=&mut update=>result,
        }
    };
    let warnings = result?;
    report(
        storage,
        request.instance,
        &request.context,
        warnings.first(),
    );
    Ok(ThunderstoreStartOutcome {
        install_guard: Arc::try_unwrap(install).map_err(
            |_| "Thunderstore verification has not released its private-program lifecycle lease",
        )?,
        warnings,
    })
}

#[cfg(test)]
#[path = "commands_thunderstore_start_tests.rs"]
mod tests;
