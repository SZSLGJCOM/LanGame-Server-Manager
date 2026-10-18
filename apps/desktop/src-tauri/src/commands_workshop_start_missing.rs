//! Missing Palworld deployment can be reconstructed only from this instance's
//! Workshop membership. A global cache sharing a PackageName is not membership.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Request {
    pub(super) settings: app_core::AppSettings,
    pub(super) root: PathBuf,
    pub(super) app: u32,
    pub(super) wanted: BTreeSet<String>,
    pub(super) removed: BTreeSet<String>,
}

pub(super) fn missing_names(
    settings: &Value,
    inventory: &ManualModInventoryResult,
) -> Result<BTreeSet<String>, String> {
    let mut names = super::super::workshop_start_plan::enabled_palworld_package_names(settings)?;
    // Preserve local/manual and explicitly disabled packages already identified
    // by inventory. Losing bytes is not permission to adopt another author.
    for name in inventory
        .items
        .iter()
        .filter_map(|item| item.inferred_id.as_deref())
    {
        names.remove(&name.trim().to_lowercase());
    }
    Ok(names)
}

pub(super) fn package_name(path: &Path) -> std::io::Result<Option<String>> {
    let bytes = match app_steamcmd::steam_depot::bounded(&path.join("Info.json"), 1024 * 1024) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    Ok(serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|value| {
            value["PackageName"]
                .as_str()
                .map(|name| name.trim().to_lowercase())
        })
        .filter(|name| !name.is_empty()))
}

pub(super) fn matching_sources(
    settings: &app_core::AppSettings,
    root: &Path,
    app: u32,
    ids: &[String],
    names: &BTreeSet<String>,
) -> Result<BTreeMap<String, Vec<PathBuf>>, String> {
    let mut result = BTreeMap::new();
    for id in ids {
        let mut choices = Vec::new();
        for base in [
            root.to_path_buf(),
            PathBuf::from(app_steamcmd::steamcmd_status(settings).root),
        ] {
            let path = base
                .join("steamapps/workshop/content")
                .join(app.to_string())
                .join(id);
            if package_name(&path)
                .map_err(|error| {
                    format!("Cannot read this instance's Workshop package metadata: {error}")
                })?
                .is_some_and(|name| names.contains(&name))
            {
                choices.push(path);
            }
        }
        choices.sort();
        choices.dedup();
        result.insert(id.clone(), choices);
    }
    Ok(result)
}

pub(super) async fn resolve(
    operation: &StorageContextOperationGuard,
    install: Arc<app_steamcmd::GameInstallLifecycleGuard>,
    steamcmd: Arc<app_steamcmd::SteamCmdLifecycleGuard>,
    request: Request,
    cancellation: app_steamcmd::InstallCancellation,
) -> Result<Vec<String>, String> {
    spawn_blocking_storage_context_task(operation, move || {
        resolve_locked(&install, &steamcmd, &request, &cancellation)
    })
    .await
    .map_err(|error| format!("Missing Workshop deployment inspection worker failed: {error}"))?
}

fn resolve_locked(
    install: &app_steamcmd::GameInstallLifecycleGuard,
    steamcmd: &app_steamcmd::SteamCmdLifecycleGuard,
    request: &Request,
    cancellation: &app_steamcmd::InstallCancellation,
) -> Result<Vec<String>, String> {
    let candidates = app_steamcmd::WorkshopCacheTransaction::instance_candidate_ids(
        &request.settings,
        request.app,
        &request.root,
        install,
        steamcmd,
        cancellation,
    )
    .map_err(|error| steamcmd_error_message(&error))?;
    let candidates = candidates
        .into_iter()
        .filter(|id| !request.removed.contains(id))
        .collect::<Vec<_>>();
    let sources = matching_sources(
        &request.settings,
        &request.root,
        request.app,
        &candidates,
        &request.wanted,
    )?;
    let mut matches = BTreeMap::<String, BTreeSet<String>>::new();
    for (id, choices) in sources {
        if cancellation.is_cancelled() {
            return Err("启动前模组更新已取消，实例未启动。".into());
        }
        for path in choices {
            let ids = vec![id.clone()];
            let source = BTreeMap::from([(id.clone(), vec![path.clone()])]);
            let proof = app_steamcmd::WorkshopCacheTransaction::verify_scoped(
                &request.settings,
                app_steamcmd::WorkshopCacheScope {
                    consumer_app_id: request.app,
                    install_root: &request.root,
                    ids: &ids,
                    fallback_sources: Some(&source),
                },
                install,
                steamcmd,
                cancellation,
            )
            .map_err(|error| steamcmd_error_message(&error))?;
            if let Some(proof) = proof
                && let Some(name) = package_name(&path).map_err(|error| error.to_string())?
                && request.wanted.contains(&name)
            {
                proof
                    .verify_deployed_item(&id, &path, cancellation)
                    .map_err(|error| error.to_string())?;
                matches.entry(name).or_default().insert(id.clone());
            }
        }
    }
    let mut result = BTreeSet::new();
    for name in &request.wanted {
        let ids=matches.get(name).ok_or_else(||format!("启用的 PackageName `{name}` 缺少实际模组包，也没有属于该实例并可通过文件哈希校验的完整 Workshop 缓存。请在 Mod 页面重新选择或安装该包。"))?;
        if ids.len() != 1 {
            return Err(format!(
                "多个属于该实例的完整 Workshop 缓存声明相同 PackageName `{name}`；请在 Mod 页面明确选择后再启动。"
            ));
        }
        result.extend(ids.iter().cloned());
    }
    Ok(result.into_iter().collect())
}

#[cfg(test)]
#[path = "commands_workshop_start_missing_tests.rs"]
mod tests;
