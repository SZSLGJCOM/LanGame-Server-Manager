//! Officially validate and certify an existing Steam tree without allocating,
//! moving or cleaning a package. Any non-package content blocks certification.
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use app_core::{AppSettings, InstallState, ModuleDetails};
use app_modules::ModuleDescriptor;
use app_steamcmd::{
    InstallCancellation, RETAINED_INSTALL_DATA_MARKER, acquire_game_install_lifecycle,
    install_or_update_module_at_with_progress_and_cancellation,
    probe_module_install_state_with_override,
};
use app_storage::{
    GameInstallSyncRecord, StoragePaths, list_instance_archives, list_instances,
    read_program_install_owner, sync_game_installs,
};
use serde_json::json;

use super::{emit, ensure_target_unused, persist_verified_install, steam_inspect, steam_seed};

fn existing_steam_root(
    settings: &AppSettings,
    descriptor: &ModuleDescriptor,
) -> io::Result<(PathBuf, u32)> {
    let app_id = descriptor
        .summary
        .steam_app_id
        .filter(|id| *id != 0)
        .ok_or_else(|| steam_seed::invalid("certify requires a Steam module"))?;
    let install = descriptor
        .install
        .as_ref()
        .ok_or_else(|| steam_seed::invalid("module installation specification is missing"))?;
    if install.download_url_windows.is_some() {
        return Err(steam_seed::invalid(
            "certify cannot install direct-download packages",
        ));
    }
    let games = steam_seed::checked_root(Path::new(&settings.games_root))?;
    let root = steam_seed::checked_root(&games.join(&install.shared_game_dir))?;
    if root == games || !root.starts_with(&games) {
        return Err(steam_seed::invalid(
            "certification target must be inside the selected library",
        ));
    }
    Ok((root, app_id))
}

fn preflight(root: &Path) -> io::Result<()> {
    steam_inspect::ensure_plain_tree(root)?;
    // A retained-data marker makes the production installer allocate elsewhere.
    // Existing identity or acquisition ownership cannot be silently replaced.
    for marker in [
        RETAINED_INSTALL_DATA_MARKER,
        ".langame-program-acquisition.json",
        ".langame-steam-cache-seed.json",
        ".langame-program-usage.json",
        ".langame-program-identity.json",
    ] {
        match fs::symlink_metadata(root.join(marker)) {
            Ok(_) => {
                return Err(steam_seed::invalid(format!(
                    "certification requires an unused existing tree; manager marker remains: {marker}"
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn archive_directory_is_empty(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
        Err(error) => return Err(error),
        Ok(_) => (),
    }
    steam_seed::check_plain(path, true)?;
    Ok(fs::read_dir(path)?.next().transpose()?.is_none())
}

async fn require_empty_instance_catalog(paths: &StoragePaths) -> Result<(), String> {
    if !list_instances(paths)
        .await
        .map_err(|error| error.to_string())?
        .is_empty()
    {
        return Err(
            "certify requires an empty instance catalog; no existing instance is rebound".into(),
        );
    }
    // Unknown historical archives have no trustworthy dependency metadata.
    // Reject them without attempting to infer that a library is unrelated.
    if !archive_directory_is_empty(&paths.archives_root).map_err(|error| error.to_string())? {
        return Err(
            "certify requires an empty archive directory, including unknown entries".into(),
        );
    }
    let archives = list_instance_archives(paths)
        .await
        .map_err(|error| error.to_string())?;
    if !archives.archives.is_empty()
        || !archives.pending_deletions.is_empty()
        || !archives.issues.is_empty()
    {
        return Err("certify requires no remaining archive or deletion journal".into());
    }
    Ok(())
}

async fn registered_root(paths: &StoragePaths, root: &Path) -> Result<String, String> {
    // Storage upserts use the registered spelling as their key. Keep it even
    // when filesystem checks canonicalize an equivalent slash/case spelling.
    Ok(read_program_install_owner(paths, root)
        .await
        .map_err(|error| error.to_string())?
        .map(|owner| owner.install_root)
        .unwrap_or_else(|| root.to_path_buf())
        .to_string_lossy()
        .into_owned())
}

pub(super) async fn run(
    paths: &StoragePaths,
    settings: &AppSettings,
    descriptor: &ModuleDescriptor,
) -> Result<(), String> {
    let paths = paths.clone();
    let settings = settings.clone();
    let descriptor = descriptor.clone();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    // Dropping the command waiter does not abort the admitted mutation. The
    // owned worker retains the lease through validation, baseline and DB commit.
    let worker = tokio::spawn(async move {
        let result = certify_owned(&paths, &settings, &descriptor).await;
        if let Err(Err(reason)) = sender.send(result) {
            emit(
                json!({"event":"certification_failed", "module_id":descriptor.summary.id,
                "reason":reason, "waiter_dropped":true}),
            );
        }
    });
    let result = receiver.await;
    worker
        .await
        .map_err(|error| format!("certification worker failed: {error}"))?;
    result.map_err(|error| format!("certification result was unavailable: {error}"))?
}

async fn certify_owned(
    paths: &StoragePaths,
    settings: &AppSettings,
    descriptor: &ModuleDescriptor,
) -> Result<(), String> {
    let (root, app_id) =
        existing_steam_root(settings, descriptor).map_err(|error| error.to_string())?;
    let module_id = &descriptor.summary.id;
    let guard = acquire_game_install_lifecycle(module_id, std::slice::from_ref(&root))
        .await
        .map_err(|error| error.to_string())?;
    require_empty_instance_catalog(paths).await?;
    ensure_target_unused(paths, module_id, &root)
        .await
        .map_err(|error| error.to_string())?;
    let install_root = registered_root(paths, &root).await?;
    let worker_root = root.clone();
    let (guard, result) = tokio::task::spawn_blocking(move || {
        let result = preflight(&worker_root);
        (guard, result)
    })
    .await
    .map_err(|error| format!("certification preflight failed: {error}"))?;
    result.map_err(|error| error.to_string())?;
    // Failure or interruption must not leave a selectable Installed record.
    // Existing manager inventories are retained but never trusted as evidence.
    sync_game_installs(
        paths,
        &[GameInstallSyncRecord {
            module_id: module_id.clone(),
            install_root: install_root.clone(),
            install_state: InstallState::Incomplete,
            current_version: None,
            mark_verified: false,
        }],
    )
    .await
    .map_err(|error| error.to_string())?;
    let module = ModuleDetails {
        summary: descriptor.summary.clone(),
        schema_json: descriptor.schema_json.clone(),
        default_ports: descriptor.default_ports.clone(),
        install: descriptor.install.clone(),
        process: descriptor.process.clone(),
        workshop: descriptor.workshop.clone(),
        mods: None,
        runtime: descriptor.runtime.clone(),
    };
    emit(json!({"event":"certification_started", "module_id":module_id, "root":root}));
    let cancellation = InstallCancellation::new();
    Box::pin(install_or_update_module_at_with_progress_and_cancellation(
        settings,
        &module,
        &root,
        &guard,
        true,
        &cancellation,
        |update| {
            emit(json!({"event":"progress", "module_id":module_id,
            "percent":update.progress_percent, "detail":update.detail}))
        },
    ))
    .await
    .map_err(|error| error.to_string())?;
    let probe = probe_module_install_state_with_override(
        settings,
        module_id,
        Some(app_id),
        descriptor.install.as_ref(),
        descriptor.process.as_ref(),
        Some(root.to_string_lossy().as_ref()),
    );
    if probe.install_state != InstallState::Installed
        || !probe
            .steam_manifest
            .as_ref()
            .is_some_and(|manifest| manifest.complete)
    {
        return Err(
            "official validation did not produce a complete installed Steam package".into(),
        );
    }
    let worker_root = root.clone();
    let worker_steamcmd = paths.steamcmd_root.clone();
    let worker_module = module_id.clone();
    // A cancelled waiter cannot release the lease while hashing continues.
    let (guard, verified) = tokio::task::spawn_blocking(move || {
        let result = steam_inspect::verify_for_certification(
            &worker_root,
            &worker_steamcmd,
            app_id,
            |mut event| {
                event["module_id"] = json!(worker_module);
                event["root"] = json!(worker_root);
                emit(event);
            },
        );
        (guard, result)
    })
    .await
    .map_err(|error| format!("certification verifier failed: {error}"))?;
    verified.map_err(|error| error.to_string())?;
    require_empty_instance_catalog(paths).await?;
    ensure_target_unused(paths, module_id, &root)
        .await
        .map_err(|error| error.to_string())?;
    // No Steam mutation follows the exact inventory check. This function moves
    // the same lease into baseline hashing and retains it through registration.
    persist_verified_install(
        guard,
        paths,
        descriptor,
        GameInstallSyncRecord {
            module_id: module_id.clone(),
            install_root,
            install_state: InstallState::Installed,
            current_version: probe.current_version,
            mark_verified: true,
        },
        true,
        None,
    )
    .await
    .map_err(|error| error.to_string())?;
    emit(
        json!({"event":"certified", "module_id":module_id, "root":root,
        "instances_started":false}),
    );
    Ok(())
}

#[cfg(test)]
#[path = "steam_certify_tests.rs"]
mod tests;
