use super::*;
use app_steamcmd::{SteamCmdError, SteamWorkshopDownloadItemResult};

pub(super) struct WorkshopDownloadFailure {
    pub message: String,
    pub output_excerpt: Option<String>,
}

impl From<SteamCmdError> for WorkshopDownloadFailure {
    fn from(error: SteamCmdError) -> Self {
        Self {
            message: steamcmd_error_message(&error),
            output_excerpt: steamcmd_error_excerpt(&error).or_else(|| Some(error.to_string())),
        }
    }
}

impl From<String> for WorkshopDownloadFailure {
    fn from(message: String) -> Self {
        Self {
            output_excerpt: Some(message.clone()),
            message,
        }
    }
}

struct WorkshopDownloadPlan {
    requested_ids: Vec<String>,
    download_ids: Vec<String>,
}

impl WorkshopDownloadPlan {
    // Callers validate the complete input before inspecting or mutating packages.
    fn from_snapshot(ids: &[String], snapshot: &SteamWorkshopInstallationSnapshot) -> Self {
        let installed = snapshot
            .items
            .iter()
            .filter(|item| item.installed)
            .map(|item| item.item_id.as_str())
            .collect::<HashSet<_>>();
        let mut seen = HashSet::new();
        let requested_ids = ids
            .iter()
            .map(|id| id.trim())
            .filter(|id| !id.is_empty() && seen.insert(*id))
            .map(String::from)
            .collect::<Vec<_>>();
        let download_ids = requested_ids
            .iter()
            .filter(|id| !installed.contains(id.as_str()))
            .cloned()
            .collect();
        Self {
            requested_ids,
            download_ids,
        }
    }
}

enum WorkshopInstallOwner {
    Instance(super::commands_program_storage::ModProgramMutation),
    AlreadyLocked(app_steamcmd::GameInstallLifecycleGuard),
}

pub(super) struct WorkshopMutation {
    program: WorkshopInstallOwner,
    steamcmd: app_steamcmd::SteamCmdLifecycleGuard,
}

impl WorkshopMutation {
    pub(super) fn for_instance(
        program: super::commands_program_storage::ModProgramMutation,
        steamcmd: app_steamcmd::SteamCmdLifecycleGuard,
    ) -> Self {
        Self {
            program: WorkshopInstallOwner::Instance(program),
            steamcmd,
        }
    }

    fn install(&self) -> &app_steamcmd::GameInstallLifecycleGuard {
        match &self.program {
            WorkshopInstallOwner::Instance(program) => &program.install,
            WorkshopInstallOwner::AlreadyLocked(install) => install,
        }
    }
}

/// The import command already owns the stopped instance mutation lease. This
/// only acquires program/SteamCMD leases, avoiding recursive instance locking.
pub(super) async fn prepare_locked_dst_import_items(
    storage: &StorageBootstrap,
    operation: &StorageContextOperationGuard,
    instance: &InstanceDetails,
    ids: &[String],
    preference: app_network::SourcePreference,
) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }
    if instance.summary.module_id != "dontstarve" {
        return Err("DST import requires a DST instance.".to_owned());
    }
    let install_root =
        PathBuf::from(super::commands_runtime_lifecycle::private_runtime_install_root(instance)?);
    let install = app_steamcmd::acquire_game_install_lifecycle(
        "dontstarve",
        std::slice::from_ref(&install_root),
    )
    .await
    .map_err(|error| steamcmd_error_message(&error))?;
    app_storage::ensure_program_archive_dependencies(&storage.paths, &install_root)
        .await
        .map_err(|error| error.to_string())?;
    crate::steam_workshop::validate_workshop_download_items(322_330, ids, preference).await?;
    let steamcmd = install
        .acquire_steamcmd(&storage.settings)
        .await
        .map_err(|error| steamcmd_error_message(&error))?;
    let mutation = std::sync::Arc::new(WorkshopMutation {
        program: WorkshopInstallOwner::AlreadyLocked(install),
        steamcmd,
    });
    let downloaded = prepare_workshop_items(
        &storage.settings,
        322_330,
        &install_root,
        ids,
        true,
        WorkshopOperation {
            storage: operation,
            mutation: mutation.clone(),
        },
        |_| {},
    )
    .await;
    dst_workshop::finish_download(instance, operation, mutation, downloaded)
        .await
        .map(|_| ())
        .map_err(|error| error.message)
}

pub(super) struct WorkshopOperation<'a> {
    pub storage: &'a StorageContextOperationGuard,
    pub mutation: std::sync::Arc<WorkshopMutation>,
}

pub(super) async fn prepare_workshop_items(
    settings: &AppSettings,
    consumer_app_id: u32,
    install_root: &Path,
    ids: &[String],
    missing_only: bool,
    operation: WorkshopOperation<'_>,
    mut on_progress: impl FnMut(InstallProgressUpdate),
) -> Result<SteamWorkshopDownloadResult, WorkshopDownloadFailure> {
    if !missing_only {
        return app_steamcmd::download_workshop_items_with_progress(
            settings,
            consumer_app_id,
            install_root,
            ids,
            operation.mutation.install(),
            &operation.mutation.steamcmd,
            on_progress,
        )
        .await
        .map_err(WorkshopDownloadFailure::from);
    }

    let before = inspect_cache(settings, consumer_app_id, install_root, ids, &operation).await?;
    let plan = WorkshopDownloadPlan::from_snapshot(ids, &before);
    let reused_count = plan.requested_ids.len() - plan.download_ids.len();
    let reuse_note = format!("Reusing {reused_count} cached Workshop item(s).");
    on_progress(InstallProgressUpdate {
        progress_percent: 10.0,
        install_progress: None,
        detail: format!(
            "{reuse_note} {} item(s) need downloading.",
            plan.download_ids.len()
        ),
        output_excerpt: String::new(),
    });

    let output_excerpt = if plan.download_ids.is_empty() {
        reuse_note
    } else {
        let downloaded = app_steamcmd::download_workshop_items_with_progress(
            settings,
            consumer_app_id,
            install_root,
            &plan.download_ids,
            operation.mutation.install(),
            &operation.mutation.steamcmd,
            &mut on_progress,
        )
        .await?;
        append_mod_install_note(downloaded.output_excerpt, Some(&reuse_note))
    };

    // Reinspect the entire request, including reused items. Inventory also discovers
    // unrelated cached IDs, which must never be included in this deployment.
    let after = inspect_cache(
        settings,
        consumer_app_id,
        install_root,
        &plan.requested_ids,
        &operation,
    )
    .await?;
    complete_download_result(&plan.requested_ids, install_root, after, output_excerpt)
        .map_err(WorkshopDownloadFailure::from)
}

async fn inspect_cache(
    settings: &AppSettings,
    consumer_app_id: u32,
    install_root: &Path,
    ids: &[String],
    operation: &WorkshopOperation<'_>,
) -> Result<SteamWorkshopInstallationSnapshot, WorkshopDownloadFailure> {
    let settings = settings.clone();
    let install_root = install_root.to_owned();
    let ids = ids.to_vec();
    let mutation = operation.mutation.clone();
    spawn_blocking_storage_context_task(operation.storage, move || {
        // A cancelled IPC waiter cannot release the cache lease while this
        // worker still verifies or repairs a cached package.
        let _mutation = mutation;
        let mut snapshot = inspect_workshop_items(&settings, consumer_app_id, &install_root, &ids)?;
        dst_workshop::verify_cached_items(&mut snapshot, &ids);
        Ok::<_, SteamCmdError>(snapshot)
    })
    .await
    .map_err(|error| {
        WorkshopDownloadFailure::from(format!("Workshop cache inspection failed: {error}"))
    })?
    .map_err(WorkshopDownloadFailure::from)
}

fn complete_download_result(
    requested_ids: &[String],
    install_root: &Path,
    snapshot: SteamWorkshopInstallationSnapshot,
    output_excerpt: String,
) -> Result<SteamWorkshopDownloadResult, String> {
    let mut inventory = snapshot
        .items
        .into_iter()
        .map(|item| (item.item_id.clone(), item))
        .collect::<HashMap<_, _>>();
    let items = requested_ids
        .iter()
        .map(|id| {
            let item = inventory
                .remove(id)
                .filter(|item| item.installed)
                .ok_or_else(|| {
                    format!("Workshop item {id} is missing from the local cache after preparation.")
                })?;
            Ok(SteamWorkshopDownloadItemResult {
                item_id: item.item_id,
                expected_path: item.path,
                expected_path_exists: item.installed,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(SteamWorkshopDownloadResult {
        consumer_app_id: snapshot.consumer_app_id,
        install_root: install_root.to_string_lossy().into_owned(),
        workshop_root: snapshot
            .searched_roots
            .into_iter()
            .next()
            .ok_or_else(|| String::from("Workshop cache inspection returned no search roots."))?,
        items,
        output_excerpt,
    })
}

#[cfg(test)]
#[path = "commands_workshop_download_tests.rs"]
mod tests;
