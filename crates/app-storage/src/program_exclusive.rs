use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use app_core::{InstallState, InstanceProgramMode, InstanceProgramSource};
use app_modules::{ModuleDescriptor, ModuleProgramSharing};
use serde::Serialize;

use crate::{StorageError, StoragePaths};

pub(crate) const PROGRAM_USAGE: &str = ".langame-program-usage.json";

#[path = "program_exclusive_reuse.rs"]
mod reuse;
pub(crate) use reuse::unused_library_can_be_reused;

#[derive(Debug, Clone, Serialize)]
pub struct InstanceProgramCreationPlan {
    pub action: String,
    pub program_path: String,
    pub can_create: bool,
    pub reason: Option<String>,
    pub requires_archive_inventory: bool,
}

/// A preview checks ownership, inventory and surviving shipped defaults, not
/// every program file hash.
/// Creation repeats the checks under its module lease and verifies the payload.
pub async fn inspect_instance_program_creation(
    paths: &StoragePaths,
    descriptor: &ModuleDescriptor,
    mode: Option<InstanceProgramMode>,
    source: InstanceProgramSource,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<InstanceProgramCreationPlan, StorageError> {
    inspect_creation_sources(paths, descriptor, mode, source, cancellation, true).await
}

pub(crate) async fn inspect_creation_sources(
    paths: &StoragePaths,
    descriptor: &ModuleDescriptor,
    mode: Option<InstanceProgramMode>,
    source: InstanceProgramSource,
    cancellation: Option<Arc<AtomicBool>>,
    include_archived_sources: bool,
) -> Result<InstanceProgramCreationPlan, StorageError> {
    crate::instance_creation_io::check_creation_cancelled(cancellation.as_deref())?;
    let installation = crate::read_library_program_install(paths, &descriptor.summary.id).await?;
    let has_repairable_library = installation
        .as_ref()
        .is_some_and(|install| install.install_state != InstallState::NotInstalled);
    let fallback = paths.games_root.join(
        descriptor
            .install
            .as_ref()
            .map(|install| install.shared_game_dir.as_str())
            .unwrap_or(&descriptor.summary.id),
    );
    let Some(installation) = installation.filter(|install| {
        install.install_state == InstallState::Installed && install.install_root.is_dir()
    }) else {
        let instance_source = crate::read_module_instance_installs(paths, &descriptor.summary.id)
            .await?
            .into_iter()
            .find(|record| {
                record.install.scope == crate::ProgramInstallScope::Instance
                    && record.install.install_state == InstallState::Installed
                    && record.install.install_root.is_dir()
            })
            .map(|record| record.install.install_root);
        if instance_source.is_none() && !include_archived_sources {
            return Ok(InstanceProgramCreationPlan {
                action: "independent_install".into(),
                program_path: fallback.to_string_lossy().into_owned(),
                can_create: false,
                reason: None,
                requires_archive_inventory: true,
            });
        }
        let archive_source = if instance_source.is_some() {
            None
        } else {
            crate::read_archived_program_sources(paths)
                .await?
                .into_iter()
                .find(|record| {
                    record.module_id == descriptor.summary.id && record.install_root.is_dir()
                })
                .map(|record| record.install_root)
        };
        let candidate = instance_source.or(archive_source);
        let can_seed = candidate.is_some();
        let can_repair = source == InstanceProgramSource::Verified
            && has_repairable_library
            && descriptor.install.as_ref().is_some_and(|install| {
                install.download_url_windows.is_some()
                    || install.source == Some(app_core::InstallSource::MinecraftJava)
                    || descriptor.summary.steam_app_id.is_some_and(|id| id > 0)
            });
        return Ok(InstanceProgramCreationPlan {
            requires_archive_inventory: false,
            action: "independent_install".into(),
            program_path: candidate.unwrap_or(fallback).to_string_lossy().into_owned(),
            can_create: can_seed || can_repair,
            reason: Some(
                if can_seed || can_repair {
                    "将复用已验证程序，必要时自动补齐原版文件后创建新服务器。"
                } else {
                    "请先下载并验证服务器安装。"
                }
                .into(),
            ),
        });
    };
    let pool = crate::storage_db::connect_pool(paths).await?;
    let counts = sqlx::query_as::<_, (i64, i64)>(
        "SELECT COUNT(*), COALESCE(SUM(runtime_mode='independent'),0) FROM instances WHERE install_id=?1",
    ).bind(installation.id).fetch_one(&pool).await;
    pool.close().await;
    let (references, independent_references) = counts?;
    let root = installation.install_root;
    if source == InstanceProgramSource::Local
        && (references != 0 || library_was_exclusively_used(&root)?)
    {
        return Ok(InstanceProgramCreationPlan {
            requires_archive_inventory: false,
            action: "independent_install".into(),
            program_path: root.to_string_lossy().into_owned(),
            can_create: false,
            reason: Some(
                "这份安装已含有实例数据，不能隐式导入存档或 Mods；请使用经过验证的独立原版来源。"
                    .into(),
            ),
        });
    }
    let reusable = if references == 0
        && source == InstanceProgramSource::Verified
        && mode != Some(InstanceProgramMode::Shared)
    {
        unused_library_can_be_reused(paths, &root, &descriptor.summary.id, cancellation).await?
    } else {
        false
    };
    let sharing = !library_was_exclusively_used(&root)?
        && source == InstanceProgramSource::Verified
        && mode != Some(InstanceProgramMode::Independent)
        && descriptor.storage.program_sharing == ModuleProgramSharing::Shared
        && independent_references == 0;
    Ok(InstanceProgramCreationPlan {
        requires_archive_inventory: false,
        action: if reusable {
            "existing_install"
        } else if sharing {
            "shared_install"
        } else {
            "independent_install"
        }
        .into(),
        program_path: root.to_string_lossy().into_owned(),
        can_create: true,
        reason: if reusable || sharing {
            None
        } else {
            Some("现有安装将保留为程序来源；新实例仅复制已验证的程序文件。".into())
        },
    })
}

pub(crate) fn unused_library_is_fresh(
    root: &Path,
    module_id: &str,
    cancellation: Option<&AtomicBool>,
) -> Result<bool, StorageError> {
    if library_was_exclusively_used(root)?
        || crate::program_library_retention::retained_library_program_source(root, module_id)?
    {
        return Ok(false);
    }
    let Some(package) = crate::program_seed::read_package_inventory(root, module_id)? else {
        return Ok(false);
    };
    let mut untrusted = BTreeSet::new();
    crate::program_adoption::retain_unlisted_paths(root, &package, &mut untrusted, cancellation)?;
    untrusted.retain(|path| !path.eq_ignore_ascii_case(".langame-initial-package.json"));
    Ok(untrusted.is_empty())
}

pub(crate) fn library_was_exclusively_used(root: &Path) -> Result<bool, StorageError> {
    match std::fs::symlink_metadata(root.join(PROGRAM_USAGE)) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(StorageError::ReadPath {
            path: root.join(PROGRAM_USAGE),
            source,
        }),
    }
}

pub(crate) fn prepare_exclusive_reference(
    program_root: &Path,
    instance_root: &Path,
    module_id: &str,
    cancellation: Option<&AtomicBool>,
) -> Result<PathBuf, StorageError> {
    let reusable = if library_was_exclusively_used(program_root)? {
        crate::program_runtime::previous_exclusive_instance(program_root, module_id)?;
        !crate::program_library_retention::retained_library_program_source(program_root, module_id)?
            && crate::program_seed::retired_library_is_clean(
                program_root,
                module_id,
                true,
                cancellation,
            )?
    } else {
        crate::program_seed::require_initial_package_tree(program_root, module_id, cancellation)?;
        unused_library_is_fresh(program_root, module_id, cancellation)?
    };
    if !reusable {
        return Err(StorageError::CleanLibraryProgramRequired {
            path: program_root.to_owned(),
        });
    }
    let root = crate::program_runtime::prepare_exclusive_program_reference(
        program_root,
        instance_root,
        module_id,
    )?;
    crate::program_runtime::record_exclusive_program_use(program_root, instance_root)?;
    Ok(root)
}
