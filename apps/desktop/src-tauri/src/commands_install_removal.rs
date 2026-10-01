use super::ProtectedInstallDataPath;
use app_storage::program_removals_db::{self as removals_db, ProgramRemovalRecord};
use app_storage::{ProgramInstallRecord, ProgramInstallScope, StoragePaths, program_removal_files};
use std::fs;
use std::path::{Path, PathBuf};

#[path = "commands_install_removal_journal.rs"]
mod journal;

pub(super) struct RemovalOutcome {
    pub preserved_data_paths: Vec<String>,
}

pub(super) async fn validate_library_removal(
    paths: &app_storage::StoragePaths,
    module_id: &str,
    install_id: i64,
    install_root: &Path,
) -> Result<(), String> {
    let root =
        program_removal_files::normalize_path(install_root).map_err(|error| error.to_string())?;
    let owner = app_storage::read_program_install_owner(paths, &root)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| String::from("Program installation owner is missing."))?;
    if owner.id != install_id
        || owner.module_id != module_id
        || owner.scope != ProgramInstallScope::Library
        || owner.owner_instance_id.is_some()
    {
        return Err(String::from(
            "Program installation owner changed; files were preserved.",
        ));
    }
    let acquisition = app_storage::is_instance_program_acquisition(paths, module_id, &root)
        .map_err(|error| error.to_string())?;
    if !acquisition {
        let games = program_removal_files::normalize_path(&paths.games_root)
            .map_err(|error| error.to_string())?;
        if root.parent() != Some(games.as_path()) {
            return Err(String::from("unsafe_path"));
        }
        app_storage::ensure_library_program_path_isolated(paths, &root)
            .await
            .map_err(|error| error.to_string())?;
    }
    for managed in [
        &paths.archives_root,
        &paths.modules_root,
        &paths.steamcmd_root,
    ] {
        let managed =
            program_removal_files::normalize_path(managed).map_err(|error| error.to_string())?;
        if managed.starts_with(&root) || root.starts_with(&managed) {
            return Err(String::from("unsafe_path"));
        }
    }
    Ok(())
}

/// The caller owns the module lifecycle and instance mutation leases until this
/// future completes, including compensation and committed payload cleanup.
pub(super) async fn remove_library_program(
    paths: &StoragePaths,
    record: &ProgramInstallRecord,
    protected: &[ProtectedInstallDataPath],
) -> Result<RemovalOutcome, String> {
    if record.scope != ProgramInstallScope::Library || record.owner_instance_id.is_some() {
        return Err("只允许卸载未绑定实例的游戏库程序".into());
    }
    validate_library_removal(paths, &record.module_id, record.id, &record.install_root).await?;
    app_storage::ensure_program_archive_dependencies(paths, &record.install_root)
        .await
        .map_err(|error| error.to_string())?;
    if !record
        .install_root
        .try_exists()
        .map_err(|error| error.to_string())?
    {
        removals_db::mark_missing_removed(
            paths,
            &record.module_id,
            record.id,
            &record.install_root,
        )
        .await
        .map_err(|error| error.to_string())?;
        return Ok(RemovalOutcome {
            preserved_data_paths: Vec::new(),
        });
    }
    let owned_record = record.clone();
    let protected: Vec<_> = protected
        .iter()
        .map(|item| ProtectedInstallDataPath {
            source: item.source.clone(),
            path: item.path.clone(),
        })
        .collect();
    let journal = tokio::task::spawn_blocking(move || {
        let retained = retained_relative_paths(&owned_record.install_root, &protected)?;
        journal::Journal::prepare(&owned_record, retained)
    })
    .await
    .map_err(|error| format!("准备卸载任务失败：{error}"))??;
    let removal = ProgramRemovalRecord {
        operation_id: journal.operation_id.clone(),
        module_id: journal.module_id.clone(),
        install_id: journal.install_id,
        source_root: journal.source.clone(),
        phase: "prepared".into(),
        journal_json: journal.encode()?,
    };
    if let Err(error) = removals_db::begin(paths, &removal).await {
        // No payload has moved yet. Even an ambiguous INSERT commit leaves a
        // prepared record whose later recovery observes the untouched source.
        return match run_journal(&journal, JournalAction::Rollback).await {
            Ok(()) => Err(format!("记录卸载恢复意图失败：{error}")),
            Err(cleanup) => Err(format!(
                "记录卸载恢复意图失败：{error}；清理准备目录失败：{cleanup}"
            )),
        };
    }
    if let Err(error) = run_journal(&journal, JournalAction::Stage).await {
        rollback_prepared(paths, &removal, &journal)
            .await
            .map_err(|rollback| format!("{error}；恢复安装目录失败：{rollback}"))?;
        return Err(error);
    }
    if let Err(error) = removals_db::commit(paths, &removal).await {
        // SQLite may have committed before reporting a connection failure.
        // Only its persisted phase can authorize rollback versus final purge.
        let observed = removals_db::list(paths, &removal.module_id)
            .await
            .map_err(|read| {
                format!("提交卸载状态失败：{error}；无法确认提交结果，恢复记录与文件已保留：{read}")
            })?
            .into_iter()
            .find(|entry| entry.operation_id == removal.operation_id)
            .ok_or_else(|| format!("提交卸载状态失败：{error}；恢复记录已变化，已保留隔离目录"))?;
        if observed.journal_json != removal.journal_json
            || observed.source_root != removal.source_root
            || observed.install_id != removal.install_id
        {
            return Err(format!(
                "提交卸载状态失败：{error}；恢复记录已变化，已保留隔离目录"
            ));
        }
        removals_db::validate_recovery(paths, &observed)
            .await
            .map_err(|read| {
                format!("提交卸载状态失败：{error}；恢复条件发生变化，文件已保留：{read}")
            })?;
        if observed.phase == "prepared" {
            rollback_prepared(paths, &observed, &journal)
                .await
                .map_err(|rollback| {
                    format!("提交卸载状态失败：{error}；恢复安装目录失败：{rollback}")
                })?;
            return Err(format!("提交卸载状态失败，安装目录已恢复：{error}"));
        }
        finish_committed(paths, &observed, &journal).await?;
    } else {
        let mut committed = removal;
        committed.phase = "committed".into();
        finish_committed(paths, &committed, &journal).await?;
    }
    Ok(RemovalOutcome {
        preserved_data_paths: journal.preserved_data_paths(),
    })
}

/// Recovery runs only during an explicit uninstall/delete operation. Prepared
/// operations restore the program; committed operations only finish its purge.
pub(super) async fn recover_library_removals(
    paths: &StoragePaths,
    module_id: &str,
) -> Result<app_core::ProgramCleanupResult, String> {
    let mut result = app_core::ProgramCleanupResult::default();
    for record in removals_db::list(paths, module_id)
        .await
        .map_err(|error| error.to_string())?
    {
        let journal = journal::Journal::decode(&record.journal_json)?;
        if journal.operation_id != record.operation_id
            || journal.module_id != record.module_id
            || journal.install_id != record.install_id
            || journal.source != record.source_root
        {
            return Err("卸载恢复记录与文件日志不匹配，现有目录已保留".into());
        }
        validate_library_removal(
            paths,
            &record.module_id,
            record.install_id,
            &record.source_root,
        )
        .await?;
        app_storage::ensure_program_archive_dependencies(paths, &record.source_root)
            .await
            .map_err(|error| error.to_string())?;
        removals_db::validate_recovery(paths, &record)
            .await
            .map_err(|error| error.to_string())?;
        if record.phase == "prepared" {
            rollback_prepared(paths, &record, &journal).await?;
        } else {
            finish_committed(paths, &record, &journal).await?;
            result
                .removed_install_roots
                .push(record.source_root.to_string_lossy().into_owned());
            result
                .preserved_data_paths
                .extend(journal.preserved_data_paths());
        }
    }
    Ok(result)
}

async fn rollback_prepared(
    paths: &StoragePaths,
    record: &ProgramRemovalRecord,
    journal: &journal::Journal,
) -> Result<(), String> {
    run_journal(journal, JournalAction::Rollback).await?;
    removals_db::finish(paths, record)
        .await
        .map_err(|error| error.to_string())
}

async fn finish_committed(
    paths: &StoragePaths,
    record: &ProgramRemovalRecord,
    journal: &journal::Journal,
) -> Result<(), String> {
    run_journal(journal, JournalAction::Finish)
        .await
        .map_err(|error| {
            format!("卸载状态已提交，清理程序文件失败；再次卸载可继续恢复：{error}")
        })?;
    removals_db::finish(paths, record)
        .await
        .map_err(|error| format!("程序文件已清理，清理卸载恢复记录失败：{error}"))
}

enum JournalAction {
    Stage,
    Rollback,
    Finish,
}

async fn run_journal(journal: &journal::Journal, action: JournalAction) -> Result<(), String> {
    let journal = journal.clone();
    tokio::task::spawn_blocking(move || match action {
        JournalAction::Stage => journal.stage(),
        JournalAction::Rollback => journal.rollback(),
        JournalAction::Finish => journal.finish_committed(),
    })
    .await
    .map_err(|error| format!("卸载文件任务失败：{error}"))?
}

fn retained_relative_paths(
    root: &Path,
    protected: &[ProtectedInstallDataPath],
) -> Result<Vec<PathBuf>, String> {
    let canonical_root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let mut paths = Vec::new();
    for item in protected {
        let path = item
            .path
            .as_deref()
            .ok_or_else(|| format!("无法确定需要保留的数据路径，已拒绝卸载：{}", item.source))?;
        for ancestor in path.ancestors() {
            if ancestor.as_os_str().is_empty() {
                continue;
            }
            let metadata = fs::symlink_metadata(ancestor)
                .map_err(|error| format!("检查保留数据 {} 失败：{error}", ancestor.display()))?;
            if super::install_data::metadata_is_link(&metadata) {
                return Err(format!(
                    "保留数据路径包含链接，已拒绝卸载：{}",
                    ancestor.display()
                ));
            }
        }
        let canonical = fs::canonicalize(path).map_err(|error| error.to_string())?;
        let relative = canonical
            .strip_prefix(&canonical_root)
            .map_err(|_| format!("保留数据 {} 不在安装目录内，已拒绝卸载", path.display()))?;
        if relative.as_os_str().is_empty() {
            return Err(format!(
                "存档覆盖整个安装目录，无法安全卸载：{}",
                path.display()
            ));
        }
        paths.push(relative.to_path_buf());
    }
    paths.sort_by_key(|path| path.components().count());
    let mut retained: Vec<PathBuf> = Vec::new();
    for path in paths {
        if !retained.iter().any(|parent| path.starts_with(parent)) {
            retained.push(path);
        }
    }
    Ok(retained)
}

#[cfg(test)]
#[path = "commands_install_removal_tests.rs"]
mod tests;
