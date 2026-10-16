//! Local archives use the same exact-file routes as online Thunderstore packages.
use super::*;

pub(super) fn stage_sources(
    target: ResolvedManualModTarget,
    sources: Vec<PathBuf>,
) -> Result<ManualModStageResult, String> {
    if sources.is_empty() {
        return Err("no mod sources were provided".into());
    }
    validate_manual_mod_source_inputs(&sources, &target.accepts)?;
    let game = thunderstore_game_profile(&target.module_id)?;
    let root = target
        .target_path
        .ancestors()
        .nth(Path::new(game.target_relative).components().count())
        .ok_or("Mod target has no runtime root")?
        .to_path_buf();
    if target.target_path != root.join(game.target_relative) {
        return Err("Thunderstore target does not match the instance runtime".into());
    }
    let root_lock = manual_mod_target_lock(&root)?;
    let _root_guard = root_lock
        .lock()
        .map_err(|_| "Runtime Mod target lock poisoned")?;
    let plugin_lock = manual_mod_target_lock(&target.target_path)?;
    let _plugin_guard = plugin_lock
        .lock()
        .map_err(|_| "Plugin Mod target lock poisoned")?;
    validate_manual_mod_target_path(&root)?;
    validate_manual_mod_target_tree(&target.target_path)?;
    for source in &sources {
        validate_manual_mod_source_destination(source, &root)?;
    }
    let transaction = create_manual_mod_transaction(&root)?;
    let prepared = (|| {
        let mut items = Vec::new();
        let mut files = 0;
        let mut bytes = 0;
        let mut replace_roots = HashSet::new();
        for (index, source) in sources.iter().enumerate() {
            let metadata = online_packages::local_import_metadata(source)?;
            let payload = prepare_payload(source, index, &transaction)?;
            let (mut prepared, count, size, roots) =
                online_packages::prepare_local_thunderstore_transaction(
                    &target,
                    &payload,
                    metadata.as_ref(),
                    &transaction,
                )?;
            for item in &mut prepared {
                item.source_path = source.to_string_lossy().into_owned();
            }
            items.extend(prepared);
            files += count;
            bytes += size;
            replace_roots.extend(roots);
        }
        online_packages::finalize_thunderstore_transaction(&target, &transaction, &[])?;
        let plugin_transaction = ManualModTransactionPaths {
            root: transaction.root.clone(),
            staging: transaction.staging.join(game.target_relative),
            partials: transaction.partials.clone(),
            backup: transaction.backup.clone(),
        };
        let names = staged_manual_mod_root_names(&plugin_transaction)?;
        Ok::<_, String>((items, files, bytes, replace_roots, names))
    })();
    let (mut items, copied_file_count, copied_total_bytes, replace_roots, affected_root_names) =
        prepared.map_err(|error| manual_mod_staging_failure(error, &transaction, true))?;
    if let Err(failure) = commit_manual_mod_transaction(&transaction, &root, &replace_roots) {
        return Err(manual_mod_staging_failure(
            failure.message,
            &transaction,
            failure.rollback_complete,
        ));
    }
    if let Err(warning) = cleanup_manual_mod_transaction(&transaction) {
        for item in &mut items {
            item.message = Some(match item.message.take() {
                Some(message) => format!("{message}; {warning}"),
                None => warning.clone(),
            });
        }
    }
    Ok(ManualModStageResult {
        instance_id: target.instance_id,
        module_id: target.module_id,
        source_label: target.source_label,
        target_label: target.target_label,
        target_path: target.target_path.to_string_lossy().into_owned(),
        affected_root_names,
        items,
        copied_file_count,
        copied_total_bytes,
    })
}

fn prepare_payload(
    source: &Path,
    index: usize,
    transaction: &ManualModTransactionPaths,
) -> Result<PathBuf, String> {
    let metadata = validate_manual_mod_source_filesystem_entry(source)?;
    if metadata.is_dir() {
        return Ok(source.to_path_buf());
    }
    let stem = source.file_stem().ok_or("Imported Mod has no name")?;
    let payload = transaction
        .root
        .join(format!("local-import-{index}"))
        .join(stem);
    fs::create_dir_all(&payload).map_err(|error| error.to_string())?;
    if source
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
    {
        crate::commands::commands_mods::mod_online::reject_loader_archive(source)?;
        extract_zip_archive_to_directory(
            source,
            &payload,
            &transaction.partials,
            &mut ManualModStagingRegistry::default(),
            ManualModArchiveLayout::ThunderstorePayload,
        )?;
    } else {
        let name = source.file_name().ok_or("Imported Mod has no filename")?;
        copy_mod_payload(source, &payload.join(name))?;
    }
    Ok(payload)
}
