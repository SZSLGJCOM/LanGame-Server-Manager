use super::*;
use app_steamcmd::deploy_dst_workshop_cache;
use std::collections::BTreeMap;

pub(super) async fn finish_download(
    instance: &InstanceDetails,
    operation: &StorageContextOperationGuard,
    mutation: std::sync::Arc<workshop_download::WorkshopMutation>,
    downloaded: Result<SteamWorkshopDownloadResult, workshop_download::WorkshopDownloadFailure>,
) -> Result<SteamWorkshopDownloadResult, workshop_download::WorkshopDownloadFailure> {
    let mut result = downloaded?;
    if instance.summary.module_id != "dontstarve" || result.items.is_empty() {
        return Ok(result);
    }
    let ugc_root = Path::new(&instance.config_file_path)
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| String::from("无法确定饥荒实例的模组目录"))?
        .join("data")
        .join("ugc");
    let items = result.items.clone();
    let file_count = spawn_blocking_storage_context_task(operation, move || {
        // Retain the original instance, program and SteamCMD cache leases
        // through publication, including if the IPC caller disconnects.
        let _mutation = mutation;
        let sources = workshop_sources(&items)?;
        let mut file_count = 0;
        for (source, ids) in sources {
            for spec in app_core::dst_shards::DST_SHARDS {
                let shard = spec.directory;
                let deployed = deploy_dst_workshop_cache(&source, &ugc_root.join(shard), &ids)
                    .map_err(|error| format!("饥荒 {shard} 模组部署失败：{error}"))?;
                file_count += deployed.file_count;
            }
        }
        Ok::<_, String>(file_count)
    })
    .await
    .map_err(|error| format!("DST Workshop deployment task failed: {error}"))??;
    result.output_excerpt = append_mod_install_note(
        result.output_excerpt,
        Some(&format!(
            "Installed {file_count} file(s) into the isolated Master, Caves, Islands and Volcano Workshop caches."
        )),
    );
    Ok(result)
}

fn workshop_sources(
    items: &[app_steamcmd::SteamWorkshopDownloadItemResult],
) -> Result<BTreeMap<PathBuf, Vec<String>>, String> {
    let mut sources = BTreeMap::<PathBuf, Vec<String>>::new();
    for item in items {
        let path = Path::new(&item.expected_path);
        let app = path.parent();
        let content = app.and_then(Path::parent);
        if path.file_name().and_then(|name| name.to_str()) != Some(item.item_id.as_str())
            || app.and_then(Path::file_name).and_then(|name| name.to_str()) != Some("322330")
            || content
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                != Some("content")
        {
            return Err(format!(
                "模组 {} 缺少官方 Workshop 安装记录，请重新下载。",
                item.item_id
            ));
        }
        let source = content
            .and_then(Path::parent)
            .ok_or_else(|| format!("模组 {} 的 Workshop 路径无效", item.item_id))?;
        sources
            .entry(source.to_path_buf())
            .or_default()
            .push(item.item_id.clone());
    }
    Ok(sources)
}

// A directory alone is not an installed Steam UGC item. Incomplete downloads
// and the old mods/workshop-ID layout must not suppress a repairing download.
pub(super) fn verify_cached_items(
    snapshot: &mut SteamWorkshopInstallationSnapshot,
    ids: &[String],
) {
    if snapshot.consumer_app_id != 322_330 {
        return;
    }
    for item in &mut snapshot.items {
        if !ids.iter().any(|id| id.trim() == item.item_id) {
            continue;
        }
        let verified = snapshot.searched_roots.iter().find_map(|root| {
            let content = Path::new(root);
            if content.file_name().and_then(|name| name.to_str()) != Some("322330")
                || content
                    .parent()
                    .and_then(Path::file_name)
                    .and_then(|name| name.to_str())
                    != Some("content")
            {
                return None;
            }
            let workshop = content.parent()?.parent()?;
            deploy_dst_workshop_cache(workshop, workshop, std::slice::from_ref(&item.item_id))
                .ok()
                .map(|_| content.join(&item.item_id))
        });
        item.installed = verified.is_some();
        if let Some(path) = verified {
            item.path = path.to_string_lossy().into_owned();
        }
    }
}

#[cfg(test)]
#[path = "commands_dst_workshop_tests.rs"]
mod tests;
