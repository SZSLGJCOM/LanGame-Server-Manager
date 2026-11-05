use super::*;
use crate::http_download::download_file_with_progress;
use app_core::terraria::{TML_RELEASE, managed_runtime_dir};
use std::io::ErrorKind;

#[path = "tmodloader_sources.rs"]
mod sources;

// GitHub's stable release asset and Microsoft's release-metadata/8.0/releases.json.
// ScriptCaller forces DOTNET_ROLL_FORWARD=Disable for its own dotnet/ directory.
const TML_SHA256: &str = "5aaed81b245044286c3ade200f8bac9162ef932ee8ff11a31e05da49a186c07b";
const DOTNET_URL: &str =
    "https://builds.dotnet.microsoft.com/dotnet/Runtime/8.0.0/dotnet-runtime-8.0.0-win-x64.zip";
const DOTNET_SHA512: &str = "b786869739c87ea286ee494fbf72cc92cf0cd91c1f22ede0133a523aa150740dd89cdca6a1530bd1f39d24db4f34939e17e2b0dce3d7efd06e4eced30b530643";
const RECEIPT: &str = ".langame-tmodloader-release";
const REQUIRED_FILES: &[&str] = &[
    "LaunchUtils/busybox64.exe",
    "LaunchUtils/ScriptCaller.sh",
    "LaunchUtils/BashUtils.sh",
    "LaunchUtils/EnvironmentFix.sh",
    "LaunchUtils/DotNetVersion.sh",
    "LaunchUtils/InstallDotNet.sh",
    "Libraries/Native/Windows/SDL2.dll",
    "tModLoader.dll",
    "tModLoader.deps.json",
    "tModLoader.runtimeconfig.json",
    "dotnet/dotnet.exe",
    "dotnet/host/fxr/8.0.0/hostfxr.dll",
    "dotnet/shared/Microsoft.NETCore.App/8.0.0/coreclr.dll",
    "dotnet/shared/Microsoft.NETCore.App/8.0.0/hostpolicy.dll",
    "dotnet/shared/Microsoft.NETCore.App/8.0.0/System.Private.CoreLib.dll",
];

/// Install the supported stable release and its private .NET into this instance.
/// Custom runtimes, worlds, and Workshop content are outside this directory.
pub async fn prepare_tmodloader_runtime(
    lock_context: &InstallLockContext,
    instance_root: &Path,
    cancellation: InstallCancellation,
    mut on_progress: impl FnMut(InstallProgressUpdate),
) -> Result<PathBuf, SteamCmdError> {
    // This is a complete 94 MB server distribution. Even while its downloads
    // are making progress, a short prerequisite-probe budget can expire.
    let deadline = InstallDeadline::new("准备 tModLoader", crate::MODULE_INSTALL_TIMEOUT);
    cancellation
        .scope(prepare_instance(
            lock_context,
            instance_root,
            deadline,
            &mut on_progress,
        ))
        .await
}

async fn prepare_instance(
    lock_context: &InstallLockContext,
    instance_root: &Path,
    deadline: InstallDeadline,
    progress: &mut impl FnMut(InstallProgressUpdate),
) -> Result<PathBuf, SteamCmdError> {
    deadline.check_cancelled()?;
    let destination = managed_runtime_dir(instance_root);
    let root = destination
        .parent()
        .ok_or_else(|| preparation_error("无效的 tModLoader 安装路径。"))?;
    // The start worker already holds the game program lease. Only lock this
    // instance's separate tModLoader tree to avoid nesting the program lock.
    let _guard = ResourceLocks::acquire(lock_context, &[], &[root.to_owned()], deadline).await?;
    // Retirement holds an exclusive lease on the whole instance. It may
    // have finished while this worker queued. Never recreate its old root.
    let config = instance_root.join("config/instance.json");
    crate::steam_depot::check_plain(&config, false).map_err(|error| path_error(&config, error))?;
    for ancestor in root.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.is_dir() && !is_reparse(&metadata) => {}
            Ok(_) => {
                return Err(preparation_error(format!(
                    "tModLoader 安装路径包含链接或非目录：{}",
                    ancestor.display()
                )));
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(path_error(ancestor, error)),
        }
    }
    progress(InstallProgressUpdate::stage(
        InstallPhase::Verifying,
        format!("正在检查 tModLoader {TML_RELEASE}…"),
    ));
    let receipt = format!("{TML_RELEASE}\n{TML_SHA256}\n{DOTNET_SHA512}\n");
    let installed = owns_destination(&destination, &receipt)?;
    if installed && has_required_files(&destination)? {
        deadline.check_cancelled()?;
        return Ok(destination);
    }
    fs::create_dir_all(root).map_err(|error| path_error(root, error))?;
    // Claim a new directory before making any files eligible for cleanup.
    let work = scratch(root, "work");
    fs::create_dir(&work).map_err(|error| path_error(&work, error))?;
    let stage = work.join("pkg");
    let result = stage_release(&work, &stage, deadline, progress)
        .await
        .and_then(|()| {
            fs::write(stage.join(RECEIPT), &receipt).map_err(|error| path_error(&stage, error))?;
            deadline.check_cancelled()?;
            publish(&stage, &destination, root, &receipt, progress)?;
            Ok(destination.clone())
        });
    if !matches!(
        &result,
        Err(SteamCmdError::InstallProcessCleanupFailed { .. })
    ) {
        await_failed_directory_cleanup(vec![work]).await;
    }
    result
}

async fn stage_release(
    work: &Path,
    stage: &Path,
    deadline: InstallDeadline,
    progress: &mut impl FnMut(InstallProgressUpdate),
) -> Result<(), SteamCmdError> {
    let download = work.join("download.zip");
    let loader = sources::download(&download, deadline, progress).await?;
    progress(InstallProgressUpdate::stage(
        InstallPhase::Extracting,
        "正在解压 tModLoader…",
    ));
    extract_archive(loader.path(), stage, deadline).await?;
    drop(loader);
    let dotnet = download_file_with_progress(
        &http_client()?,
        DOTNET_URL,
        &download,
        DownloadIntegrity {
            sha512: Some(DOTNET_SHA512),
            size: Some(33_177_718),
            ..Default::default()
        },
        deadline,
        |bytes, total| {
            progress(InstallProgressUpdate::download(
                "正在下载 tModLoader 所需的 .NET 8.0.0…",
                bytes,
                total,
            ))
        },
    )
    .await?;
    progress(InstallProgressUpdate::stage(
        InstallPhase::Extracting,
        "正在解压 tModLoader 所需的 .NET 8.0.0…",
    ));
    extract_archive(dotnet.path(), &stage.join("dotnet"), deadline).await?;
    progress(InstallProgressUpdate::stage(
        InstallPhase::Verifying,
        "正在检查 tModLoader 与 .NET 启动文件…",
    ));
    if !has_required_files(stage)? {
        return Err(preparation_error(
            "tModLoader 或其 .NET 运行时缺少启动文件。",
        ));
    }
    Ok(())
}

fn path_error(path: &Path, error: std::io::Error) -> SteamCmdError {
    preparation_error(format!(
        "无法访问 tModLoader 安装路径 {}：{error}",
        path.display()
    ))
}

/// Refuse to replace any operator directory, even when its name matches a release.
fn owns_destination(destination: &Path, receipt: &str) -> Result<bool, SteamCmdError> {
    let metadata = match fs::symlink_metadata(destination) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(path_error(destination, error)),
        Ok(metadata) => metadata,
    };
    if metadata.is_dir() && !is_reparse(&metadata) {
        let marker = destination.join(RECEIPT);
        match fs::symlink_metadata(&marker) {
            Ok(metadata)
                if metadata.is_file()
                    && !is_reparse(&metadata)
                    && metadata.len() == receipt.len() as u64 =>
            {
                if fs::read(&marker).map_err(|error| path_error(&marker, error))?
                    == receipt.as_bytes()
                {
                    return Ok(true);
                }
            }
            Err(error) if error.kind() != ErrorKind::NotFound => {
                return Err(path_error(&marker, error));
            }
            _ => {}
        }
    }
    Err(preparation_error(format!(
        "tModLoader 安装位置已有其他内容，已保留。请先移走该目录再重试：{}",
        destination.display()
    )))
}

fn has_required_files(directory: &Path) -> Result<bool, SteamCmdError> {
    for relative in REQUIRED_FILES {
        let path = directory.join(relative);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() && !is_reparse(&metadata) && metadata.len() > 0 => {}
            Ok(_) => return Ok(false),
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(path_error(&path, error)),
        }
        // A published tree is plain when created. Treat redirected descendants
        // as damaged instead of running code from outside the instance.
        match crate::steam_depot::check_plain(&path, false) {
            Ok(()) => {}
            Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::InvalidData) => {
                return Ok(false);
            }
            Err(error) => return Err(path_error(&path, error)),
        }
    }
    Ok(true)
}

fn publish(
    stage: &Path,
    destination: &Path,
    root: &Path,
    receipt: &str,
    progress: &mut impl FnMut(InstallProgressUpdate),
) -> Result<(), SteamCmdError> {
    let mut preserved = None;
    if owns_destination(destination, receipt)? {
        // Repair only a directory published by us. Keep its logs and any files
        // written there by third-party mods, even after a successful repair.
        let previous = scratch(root, "preserved");
        fs::rename(destination, &previous).map_err(|error| path_error(destination, error))?;
        preserved = Some(previous);
    }
    if let Err(error) = fs::rename(stage, destination) {
        if let Some(previous) = &preserved
            && let Err(rollback) = fs::rename(previous, destination)
        {
            return Err(preparation_error(format!(
                "发布 tModLoader 失败：{error}。原运行时保留在 {}，还原失败：{rollback}",
                previous.display()
            )));
        }
        return Err(path_error(destination, error));
    }
    if let Some(previous) = preserved {
        progress(InstallProgressUpdate::stage(
            InstallPhase::Verifying,
            format!("已修复 tModLoader，原运行时保留在 {}。", previous.display()),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn queued_preparation_cannot_recreate_a_retired_instance() {
        let root = crate::tests::unique_test_root();
        fs::create_dir_all(&root).unwrap();
        let retired = root.join("retired");
        let result = prepare_tmodloader_runtime(
            crate::test_install_lock_context(),
            &retired,
            InstallCancellation::new(),
            |_| {},
        )
        .await;
        assert!(result.is_err());
        assert!(!retired.exists());
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn publication_preserves_an_operator_directory_and_rolls_back_a_failed_repair() {
        let root = crate::tests::unique_test_root();
        let destination = root.join(TML_RELEASE);
        let stage = root.join("missing-stage");
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("operator-file"), b"keep").unwrap();
        let refused = publish(&stage, &destination, &root, "receipt", &mut |_| {});
        assert!(refused.unwrap_err().to_string().contains("已有其他内容"));
        fs::write(destination.join(RECEIPT), b"receipt").unwrap();
        assert!(publish(&stage, &destination, &root, "receipt", &mut |_| {}).is_err());
        assert_eq!(
            fs::read(destination.join("operator-file")).unwrap(),
            b"keep"
        );
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }
}
