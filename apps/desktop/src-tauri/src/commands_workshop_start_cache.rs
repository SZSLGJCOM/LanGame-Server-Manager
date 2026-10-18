//! Owned IO for startup cache protection, restoration and actual deployment.
use super::*;

pub(super) enum Protection {
    Transaction(
        app_steamcmd::WorkshopCacheTransaction,
        Vec<app_steamcmd::WorkshopDeploymentProtection>,
        Vec<app_steamcmd::WorkshopDeploymentBaseline>,
    ),
    SkipUpdate(
        app_steamcmd::VerifiedWorkshopCache,
        Vec<app_steamcmd::WorkshopDeploymentProtection>,
        Vec<app_steamcmd::WorkshopDeploymentBaseline>,
    ),
}

#[derive(Default)]
pub(super) struct DeploymentIntegrity {
    pub(super) verified: Option<app_steamcmd::VerifiedWorkshopCache>,
    pub(super) preserved: Vec<app_steamcmd::WorkshopDeploymentProtection>,
    pub(super) baselines: Vec<app_steamcmd::WorkshopDeploymentBaseline>,
}

pub(super) struct Plan {
    pub(super) root: PathBuf,
    pub(super) app: u32,
    pub(super) ids: Vec<String>,
    pub(super) fallback_names: Option<std::collections::BTreeSet<String>>,
    pub(super) manual_target: Option<PathBuf>,
}

fn storage_full(error: &app_steamcmd::SteamCmdError) -> bool {
    matches!(error,app_steamcmd::SteamCmdError::WorkshopInventoryRead { source,.. }
        if source.kind() == std::io::ErrorKind::StorageFull || source.raw_os_error() == Some(112))
}

pub(super) async fn prepare(
    operation: &StorageContextOperationGuard,
    mutation: Arc<workshop_download::WorkshopMutation>,
    settings: app_core::AppSettings,
    plan: Plan,
    cancellation: app_steamcmd::InstallCancellation,
) -> Result<Protection, String> {
    spawn_blocking_storage_context_task(operation, move || {
        let sources = plan
            .fallback_names
            .as_ref()
            .map(|names| {
                super::missing::matching_sources(&settings, &plan.root, plan.app, &plan.ids, names)
            })
            .transpose()?;
        let scope = app_steamcmd::WorkshopCacheScope {
            consumer_app_id: plan.app,
            install_root: &plan.root,
            ids: &plan.ids,
            fallback_sources: sources.as_ref(),
        };
        let baselines = if let Some(target) = &plan.manual_target {
            plan.ids
                .iter()
                .map(|id| {
                    app_steamcmd::WorkshopDeploymentBaseline::capture(
                        id,
                        &target.join(id),
                        &cancellation,
                    )
                    .map_err(|error| {
                        format!(
                            "Cannot protect this instance's actual Workshop loading root: {error}"
                        )
                    })
                })
                .collect::<Result<Vec<_>, String>>()?
        } else {
            Vec::new()
        };
        match app_steamcmd::WorkshopCacheTransaction::prepare_scoped(
            &settings,
            scope,
            mutation.install(),
            mutation.steamcmd(),
            &cancellation,
        ) {
            Ok(mut transaction) => {
                let preserved = if transaction.verified_cache().is_some() {
                    protect_deployment(
                        transaction.verified_cache(),
                        plan.manual_target.as_deref(),
                        &cancellation,
                    )
                } else if let Some(target) = &plan.manual_target {
                    plan.ids
                        .iter()
                        .filter_map(|id| {
                            match transaction.protect_uncertified_deployment_with_retained_files(
                                id,
                                &target.join(id),
                                super::super::commands_mod_staging::protected_operator_mod_file,
                                &cancellation,
                            ) {
                                Ok(Some(proof)) => Some(Ok(proof)),
                                Ok(None) => None,
                                Err(error) => Some(Err(error.to_string())),
                            }
                        })
                        .collect()
                } else {
                    Ok(Vec::new())
                };
                match preserved {
                    Ok(preserved) => Ok(Protection::Transaction(transaction, preserved, baselines)),
                    Err(error) => {
                        transaction.commit().map_err(|cleanup| {
                            format!("{error}; Workshop protection cleanup failed: {cleanup}")
                        })?;
                        Err(error)
                    }
                }
            }
            Err(error) if storage_full(&error) => {
                // No native update has run. Recheck the original cache rather
                // than mutating an old working package without recovery space.
                match app_steamcmd::WorkshopCacheTransaction::verify_scoped(
                    &settings,
                    scope,
                    mutation.install(),
                    mutation.steamcmd(),
                    &cancellation,
                )
                .map_err(|error| steamcmd_error_message(&error))?
                {
                    Some(verified) => {
                        let preserved = protect_deployment(
                            Some(&verified),
                            plan.manual_target.as_deref(),
                            &cancellation,
                        )?;
                        Ok(Protection::SkipUpdate(verified, preserved, baselines))
                    }
                    None => Err(steamcmd_error_message(&error)),
                }
            }
            Err(error) => Err(steamcmd_error_message(&error)),
        }
    })
    .await
    .map_err(|error| format!("Workshop cache protection worker failed: {error}"))?
}

fn protect_deployment(
    verified: Option<&app_steamcmd::VerifiedWorkshopCache>,
    target: Option<&Path>,
    cancellation: &app_steamcmd::InstallCancellation,
) -> Result<Vec<app_steamcmd::WorkshopDeploymentProtection>, String> {
    let (Some(verified), Some(target)) = (verified, target) else {
        return Ok(Vec::new());
    };
    let mut protected = Vec::new();
    for item in &verified.download_result().items {
        if let Some(proof)=verified.protect_deployed_item_with_retained_files(&item.item_id,&target.join(&item.item_id),super::super::commands_mod_staging::protected_operator_mod_file,super::super::commands_mod_staging::untracked_loadable_mod_file,cancellation)
            .map_err(|error|format!("为保留该实例原有的配置和用户文件，已停止自动替换 Workshop 实际加载目录：{error}"))? {
            protected.push(proof);
        }
    }
    Ok(protected)
}

pub(super) async fn rollback(
    operation: &StorageContextOperationGuard,
    mutation: Arc<workshop_download::WorkshopMutation>,
    mut transaction: app_steamcmd::WorkshopCacheTransaction,
) -> Result<Option<app_steamcmd::VerifiedWorkshopCache>, String> {
    spawn_blocking_storage_context_task(operation, move || {
        let _mutation = mutation;
        transaction.rollback().map_err(|error| {
            format!(
                "无法安全恢复更新前的 Workshop 模组；已保留恢复数据 {}：{error}",
                transaction.recovery_path().display()
            )
        })
    })
    .await
    .map_err(|error| format!("Workshop cache recovery worker failed: {error}"))?
}

pub(super) async fn commit(
    operation: &StorageContextOperationGuard,
    mutation: Arc<workshop_download::WorkshopMutation>,
    mut transaction: app_steamcmd::WorkshopCacheTransaction,
) -> Result<(), String> {
    spawn_blocking_storage_context_task(operation, move || {
        let _mutation = mutation;
        transaction.commit().map_err(|error| {
            format!(
                "Workshop cache recovery cleanup failed at {}: {error}",
                transaction.recovery_path().display()
            )
        })
    })
    .await
    .map_err(|error| format!("Workshop cache commit worker failed: {error}"))?
}

pub(super) async fn record_validation(
    operation: &StorageContextOperationGuard,
    mutation: Arc<workshop_download::WorkshopMutation>,
    settings: app_core::AppSettings,
    downloaded: SteamWorkshopDownloadResult,
    cancellation: app_steamcmd::InstallCancellation,
) -> Result<(), String> {
    spawn_blocking_storage_context_task(operation, move || {
        app_steamcmd::record_validated_workshop_items(
            &downloaded,
            mutation.install(),
            mutation.steamcmd(),
            &settings,
            &cancellation,
        )
        .map_err(|error| steamcmd_error_message(&error))
    })
    .await
    .map_err(|error| format!("Workshop validation receipt worker failed: {error}"))?
}

pub(super) struct DeploymentTarget<'a> {
    pub descriptor: &'a ModuleDescriptor,
    pub instance: &'a InstanceDetails,
    pub reservation: &'a RuntimeStartReservationLease,
}

pub(super) async fn deploy(
    storage: &StorageBootstrap,
    target: &DeploymentTarget<'_>,
    mutation: Arc<workshop_download::WorkshopMutation>,
    downloaded: SteamWorkshopDownloadResult,
    integrity: DeploymentIntegrity,
    cancellation: app_steamcmd::InstallCancellation,
) -> Result<Vec<app_steamcmd::WorkshopDeploymentBaseline>, UpdateFailure> {
    let DeploymentTarget {
        descriptor,
        instance,
        reservation,
    } = target;
    let verified = integrity.verified;
    let mut baselines = integrity.baselines;
    let operation = reservation.storage_operation();
    let downloaded = if instance.summary.module_id == "dontstarve" && verified.is_some() {
        downloaded
    } else {
        dst_workshop::finish_download(instance, operation, mutation.clone(), Ok(downloaded))
            .await
            .map_err(|error| super::policy::deployment_failure(error.message))?
    };
    if should_stage_downloaded_workshop_items(&descriptor.manifest_toml) {
        let target = resolve_manual_mod_target(storage, instance.summary.id.clone(), true).await?;
        let worker_mutation = mutation.clone();
        let worker_cancellation = cancellation.clone();
        let startup_cancelled = operation.cancellation_token();
        let old_verified = verified.clone();
        let preserved = integrity.preserved;
        baselines=spawn_blocking_storage_context_task(operation, move || {
            let _mutation = worker_mutation;
            if worker_cancellation.is_cancelled() || startup_cancelled.load(Ordering::Acquire) {
                return Err(UpdateFailure::from("启动前模组更新已取消，实例未启动。".to_owned()));
            }
            for baseline in &baselines {
                baseline.verify(&target.target_path.join(baseline.item_id()),&worker_cancellation).map_err(|error|error.to_string())?;
            }
            // All original extra data is checked before any root publication.
            // A changed native package cannot be merged safely without a known
            // configuration policy: recover the old cache and retain this root.
            for proof in &preserved {
                proof.verify(&target.target_path.join(proof.item_id()),&worker_cancellation)
                    .map_err(|error|error.to_string())?;
                let item=downloaded.items.iter().find(|item|item.item_id==proof.item_id()).ok_or_else(||UpdateFailure::from("protected Workshop item is not requested".to_owned()))?;
                if !proof.payload_unchanged(Path::new(&item.expected_path),&worker_cancellation).map_err(|error|error.to_string())? {
                    return Err(super::policy::deployment_failure("新模组包需要替换包含原有配置或用户文件的加载目录；已拒绝整目录替换以保留这些数据，将尝试使用更新前的完整包。".into()));
                }
            }
            let mut publish=downloaded.clone();
            publish.items.retain(|item|!preserved.iter().any(|proof|proof.item_id()==item.item_id));
            let already_current = publish.items.is_empty() || old_verified.as_ref().is_some_and(|verified| {
                publish.items.iter().all(|item| {
                    verified
                        .verify_deployed_item(
                            &item.item_id,
                            &target.target_path.join(&item.item_id),
                            &worker_cancellation,
                        )
                        .is_ok()
                })
            });
            if !already_current {
                let before_commit=|| {
                    for baseline in &baselines { baseline.verify(&target.target_path.join(baseline.item_id()),&worker_cancellation).map_err(|error|error.to_string())?; }
                    Ok(())
                };
                let before_cleanup=|backup:&Path| {
                    for item in &publish.items {
                        let baseline=baselines.iter().find(|baseline|baseline.item_id()==item.item_id).ok_or("Workshop publication has no original loading-root baseline")?;
                        baseline.verify_moved_original(&backup.join(&item.item_id),&worker_cancellation).map_err(|error|error.to_string())?;
                    }
                    Ok(())
                };
                super::super::commands_mod_staging::stage_downloaded_workshop_items_with_preconditions(&publish,&target,&before_commit,&before_cleanup).map_err(super::policy::staging_failure)?;
                for item in &publish.items {
                    let baseline=baselines.iter_mut().find(|baseline|baseline.item_id()==item.item_id).ok_or_else(||UpdateFailure::from("Workshop publication has no protected original loading root".to_owned()))?;
                    baseline.accept_publication(Path::new(&item.expected_path),&target.target_path.join(&item.item_id),&worker_cancellation).map_err(|error|error.to_string())?;
                }
            }
            if let Some(verified) = old_verified {
                for item in &verified.download_result().items {
                    if preserved.iter().any(|proof|proof.item_id()==item.item_id) { continue; }
                    verified
                        .verify_deployed_item(
                            &item.item_id,
                            &target.target_path.join(&item.item_id),
                            &worker_cancellation,
                        )
                        .map_err(|error| {
                            format!(
                                "恢复的 Workshop 模组 {} 未通过实际加载目录校验：{error}",
                                item.item_id
                            )
                        })?;
                }
            }
            for proof in preserved { proof.verify(&target.target_path.join(proof.item_id()),&worker_cancellation).map_err(|error|error.to_string())?; }
            Ok::<_, UpdateFailure>(baselines)
        })
        .await
        .map_err(|error| format!("Workshop deployment worker failed: {error}"))??;
    }
    if instance.summary.module_id == "dontstarve"
        && let Some(verified) = verified
    {
        let ugc_root = Path::new(&instance.config_file_path)
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| "无法确定饥荒实例的模组目录".to_owned())?
            .join("data/ugc");
        spawn_blocking_storage_context_task(operation, move || {
            let _mutation = mutation;
            for shard in app_core::dst_shards::DST_SHARDS {
                for item in &verified.download_result().items {
                    repair_dst_item(
                        &verified,
                        &ugc_root.join(shard.directory),
                        item,
                        &cancellation,
                    )?;
                }
            }
            Ok::<_, String>(())
        })
        .await
        .map_err(|error| format!("DST Workshop recovery validation worker failed: {error}"))?
        .map_err(super::policy::deployment_failure)?;
    }
    Ok(baselines)
}

fn repair_dst_item(
    verified: &app_steamcmd::VerifiedWorkshopCache,
    shard: &Path,
    item: &app_steamcmd::SteamWorkshopDownloadItemResult,
    cancellation: &app_steamcmd::InstallCancellation,
) -> Result<(), String> {
    let target = shard.join("content/322330").join(&item.item_id);
    if cancellation.is_cancelled() {
        return Err("启动前模组更新已取消，实例未启动。".into());
    }
    let source = Path::new(&item.expected_path)
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or("饥荒 Workshop 缓存路径无效")?;
    app_steamcmd::deploy_verified_dst_workshop_cache(
        source,
        shard,
        std::slice::from_ref(&item.item_id),
        verified,
    )
    .map_err(|error| error.to_string())?;
    verified
        .verify_deployed_item(&item.item_id, &target, cancellation)
        .map_err(|error| error.to_string())
}
