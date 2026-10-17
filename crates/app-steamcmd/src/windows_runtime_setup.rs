//! Verify actual native imports before launch and prepare allowlisted Microsoft
//! runtimes. Installation owns a separate cross-process lease, never a game root.
use super::windows_runtime_catalog::{self as catalog, RuntimePackage};
use super::*;
use crate::http_download::source_unavailable;
use app_core::InstallPhase;
use app_runtime::WindowsRuntimeRequirement;

#[path = "windows_runtime_setup_process.rs"]
mod process;

const PREPARATION_BUDGET: Duration = Duration::from_secs(15 * 60);
const PACKAGE_CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const ROOT_MARKER: &str = ".langame-windows-runtime-packages";
const REBOOT_MARKER: &str = "reboot-required.json";
const INSTALLING_MARKER: &str = "installation-in-progress.json";

struct RuntimePlan {
    package: RuntimePackage,
    requirements: Vec<WindowsRuntimeRequirement>,
}

pub async fn prepare_windows_runtime_dependencies<F>(
    lock_context: &InstallLockContext,
    settings: &AppSettings,
    module_id: &str,
    uses_tmodloader: bool,
    executables: &[PathBuf],
    cancellation: InstallCancellation,
    mut on_progress: F,
) -> Result<(), SteamCmdError>
where
    F: FnMut(InstallProgressUpdate),
{
    #[cfg(not(windows))]
    {
        let _ = (
            lock_context,
            settings,
            module_id,
            uses_tmodloader,
            executables,
            cancellation,
            on_progress,
        );
        Ok(())
    }
    #[cfg(windows)]
    cancellation.scope(async {
        let deadline = InstallDeadline::new("preparing Windows server runtimes", PREPARATION_BUDGET);
        deadline.check_cancelled()?;
        on_progress(InstallProgressUpdate::stage(InstallPhase::Verifying,
            "正在检查启动程序的 Windows 运行库依赖..."));
        let inputs = executables.to_vec();
        let inspection = tokio::task::spawn_blocking(move || collect_requirements(&inputs));
        let requirements = deadline.run(inspection).await.map_err(|_| operation_timeout(deadline))?
            .map_err(|error| runtime_error(format!("运行库依赖检查任务失败：{error}")))??;
        let needs_xna = requires_xna(module_id, uses_tmodloader);
        if requirements.is_empty() && !needs_xna { return Ok(()); }
        let tools = Path::new(&settings.steamcmd_root).parent().filter(|root| root.is_absolute())
            .ok_or_else(|| runtime_error("SteamCMD 工具目录必须具有绝对父路径。"))?;
        let root = tools.join("runtimes/windows-packages");
        let coordinator = install_operation::InstallCoordinator::for_resource(lock_context, "runtime-prerequisites")
            .map_err(|error| install_acquire_error(error, deadline))?;
        on_progress(InstallProgressUpdate::stage(InstallPhase::Queued,
            "正在等待 Windows 运行库准备锁..."));
        let guard = coordinator.acquire_mode(install_operation::LockMode::Exclusive, deadline).await
            .map_err(|error| install_acquire_error(error, deadline))?;
        prepare_cache_root(&root)?;
        process::check_reboot_marker(&root, deadline).await?;
        let client = http_client()?;
        let plans = planned_packages(requirements, needs_xna);
        let mut guard = Some(guard);
        for plan in plans {
            let package = plan.package;
            deadline.check_cancelled()?;
            let probe = process::probe_all(&plan.requirements, deadline).await?;
            let path = package_path(&root, package);
            let fresh = cache_is_fresh(&path, SystemTime::now());
            // Fixed historical releases have no update stream. v14 is checked
            // at most daily, even when an older installed runtime loads today.
            if probe.ready && !package.rolling { continue; }
            let download = if !fresh || !path.is_file() {
                let detail = format!("正在下载并验证 {}...", package.name);
                on_progress(InstallProgressUpdate::stage(InstallPhase::Downloading, &detail));
                let result = http_download::download_file_with_progress(&client, package.url, &path,
                    DownloadIntegrity { sha256: package.sha256, ..Default::default() }, deadline,
                    |downloaded, total| on_progress(InstallProgressUpdate::download(&detail, downloaded, total))).await;
                match result {
                    Ok(downloaded) => {
                        process::verify_package(downloaded.path(), package, deadline).await?;
                        downloaded.record_success();
                        downloaded.persist(&path)?;
                        Ok(())
                    }
                    Err(error) if probe.ready && source_unavailable(&error) => {
                        on_progress(InstallProgressUpdate::stage(InstallPhase::Preparing,
                            format!("{} 更新源暂不可用，已复检并使用当前可用运行库。", package.name)));
                        continue;
                    }
                    Err(error) => Err(error),
                }
            } else { Ok(()) };
            download?;
            validate_cache_path(&path)?;
            process::verify_package(&path, package, deadline).await?;
            let update_needed = if probe.ready && package.rolling {
                let package_version = process::package_version(&path, deadline).await?;
                version_is_newer(&package_version, probe.version.as_deref())?
            } else { !probe.ready };
            if !update_needed { continue; }
            deadline.check_cancelled()?;
            on_progress(InstallProgressUpdate::stage(InstallPhase::Installing,
                format!("正在安装 {}；如系统要求请允许 UAC。安装开始后会等待实际完成，再响应取消。", package.name)));
            let boot = process::boot_identity(deadline).await?;
            // The detached blocking owner keeps this lease even if its async
            // caller is dropped. Elevated installers must never be terminated
            // halfway through an MSI transaction or mistaken for unstarted work.
            let owned_guard = guard.take().ok_or_else(|| runtime_error("运行库安装锁已释放。"))?;
            let worker_path = path.clone();
            let worker_root = root.clone();
            let worker = tokio::task::spawn_blocking(move || {
                let result = process::install(package, &worker_path, &worker_root, &boot);
                (owned_guard, result)
            });
            let result = await_atomic_worker(worker, deadline, &mut on_progress).await?;
            guard = Some(result.0);
            result.1?;
            on_progress(InstallProgressUpdate::stage(InstallPhase::Verifying,
                format!("正在复检 {}...", package.name)));
            let verification = InstallCancellation::new().scope(process::probe_all(&plan.requirements,
                InstallDeadline::new("verifying installed Windows runtime", Duration::from_secs(45)))).await?;
            if !verification.ready {
                return Err(runtime_error(format!("{} 安装器已退出，但实际依赖仍无法加载；已阻止启动。请检查游戏目录内的运行库文件或 Windows 安装器日志。", package.name)));
            }
            deadline.check_cancelled()?;
        }
        deadline.check_cancelled()?;
        on_progress(InstallProgressUpdate::stage(InstallPhase::Preparing, "Windows 运行库依赖已就绪。"));
        deadline.check_cancelled()?;
        Ok(())
    }).await
}

async fn await_atomic_worker<T, F>(
    mut worker: tokio::task::JoinHandle<T>,
    deadline: InstallDeadline,
    on_progress: &mut F,
) -> Result<T, SteamCmdError>
where
    T: Send + 'static,
    F: FnMut(InstallProgressUpdate),
{
    let result = tokio::select! {
        result = &mut worker => result,
        _ = tokio::time::sleep_until(deadline.expires_at()) => {
            on_progress(InstallProgressUpdate::stage(InstallPhase::Installing,
                "运行库安装仍在进行。为保持系统一致性，正在等待安装器实际退出..."));
            worker.await
        }
    };
    result.map_err(|error| {
        runtime_error(format!(
            "运行库安装状态未知；请检查 Windows 安装器后重试：{error}"
        ))
    })
}

fn collect_requirements(
    executables: &[PathBuf],
) -> Result<Vec<WindowsRuntimeRequirement>, SteamCmdError> {
    let mut requirements = Vec::new();
    for executable in executables {
        for requirement in
            app_runtime::inspect_windows_runtime_requirements(executable).map_err(runtime_error)?
        {
            // A runtime next to A.exe cannot satisfy B.exe in another directory.
            // Preserve each consumer's DLL search boundary through verification.
            for directory in &requirement.search_directories {
                requirements.push(WindowsRuntimeRequirement {
                    search_directories: vec![directory.clone()],
                    ..requirement.clone()
                });
            }
        }
    }
    requirements.sort();
    requirements.dedup();
    Ok(requirements)
}

fn planned_packages(
    requirements: Vec<WindowsRuntimeRequirement>,
    needs_xna: bool,
) -> Vec<RuntimePlan> {
    let mut plans = std::collections::BTreeMap::new();
    for requirement in requirements {
        let package = catalog::package(requirement.family, requirement.architecture);
        plans
            .entry(package.id)
            .or_insert_with(|| RuntimePlan {
                package,
                requirements: Vec::new(),
            })
            .requirements
            .push(requirement);
    }
    if needs_xna {
        plans.insert(
            catalog::XNA.id,
            RuntimePlan {
                package: catalog::XNA,
                requirements: Vec::new(),
            },
        );
    }
    plans.into_values().collect()
}

fn requires_xna(module_id: &str, uses_tmodloader: bool) -> bool {
    module_id == "terraria" && !uses_tmodloader
}

fn runtime_error(detail: impl Into<String>) -> SteamCmdError {
    SteamCmdError::RuntimePreparation {
        detail: detail.into(),
    }
}

fn package_path(root: &Path, package: RuntimePackage) -> PathBuf {
    let extension = if package.kind == catalog::InstallerKind::XnaMsi {
        "msi"
    } else {
        "exe"
    };
    root.join(format!("{}.{}", package.id, extension))
}

fn cache_is_fresh(path: &Path, now: SystemTime) -> bool {
    fs::symlink_metadata(path)
        .ok()
        .filter(|metadata| metadata.is_file() && !is_reparse(metadata))
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| now.duration_since(modified).ok())
        .is_some_and(|age| age < PACKAGE_CHECK_INTERVAL)
}

fn prepare_cache_root(root: &Path) -> Result<(), SteamCmdError> {
    validate_cache_path(root)?;
    let marker = root.join(ROOT_MARKER);
    if root.exists()
        && !marker.is_file()
        && fs::read_dir(root)
            .map_err(|error| runtime_error(format!("无法读取运行库缓存：{error}")))?
            .next()
            .is_some()
    {
        return Err(runtime_error(format!(
            "保留未受管理的运行库目录：{}",
            root.display()
        )));
    }
    fs::create_dir_all(root).map_err(|source| SteamCmdError::CreatePath {
        path: root.to_owned(),
        source,
    })?;
    validate_cache_path(&marker)?;
    if !marker.exists() {
        fs::write(marker, b"LanGame trusted Windows runtime packages v1\n")
            .map_err(|error| runtime_error(format!("无法标记运行库缓存：{error}")))?;
    }
    Ok(())
}

fn validate_cache_path(path: &Path) -> Result<(), SteamCmdError> {
    if !path.is_absolute() {
        return Err(runtime_error("运行库缓存必须使用绝对路径。"));
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.file_type().is_symlink() || is_reparse(&metadata) => {
                return Err(runtime_error(format!(
                    "运行库缓存不能包含链接或重解析点：{}",
                    ancestor.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(runtime_error(format!("无法验证运行库缓存路径：{error}"))),
        }
    }
    Ok(())
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}

fn version_parts(version: &str) -> Option<Vec<u32>> {
    let version = version.trim().trim_start_matches(['v', 'V']);
    let parts = version
        .split('.')
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    (parts.len() >= 2 && parts.len() <= 4).then_some(parts)
}

fn version_is_newer(package: &str, installed: Option<&str>) -> Result<bool, SteamCmdError> {
    let mut package =
        version_parts(package).ok_or_else(|| runtime_error("微软运行库安装包版本无效。"))?;
    let Some(mut installed) = installed.and_then(version_parts) else {
        return Ok(true);
    };
    package.resize(4, 0);
    installed.resize(4, 0);
    Ok(package > installed)
}

#[cfg(test)]
#[path = "windows_runtime_setup_tests.rs"]
mod tests;
