use super::http_download::download_file_with_progress;
use super::*;
use app_core::InstallPhase;

#[path = "minecraft_java_metadata.rs"]
mod metadata;

pub(super) async fn ensure_minecraft_jre<F>(
    client: &reqwest::Client,
    install_root: &Path,
    required_major: u16,
    deadline: InstallDeadline,
    on_progress: &mut F,
) -> Result<(), SteamCmdError>
where
    F: FnMut(InstallProgressUpdate),
{
    let jre_root = install_root.join("jre");
    let java_executable = jre_root.join("bin").join("java.exe");
    if java_executable.is_file()
        && java_major_version(&java_executable, deadline).await? == Some(required_major)
    {
        return Ok(());
    }

    let package = metadata::fetch_java_package(client, required_major, deadline).await?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let archive_path = install_root.join(format!(".jre-{stamp}.zip"));
    let staging_root = install_root.join(format!(".jre-stage-{stamp}"));
    let rollback_root = install_root.join(format!(".jre-rollback-{stamp}"));
    let rejected_root = install_root.join(format!(".jre-rejected-{stamp}"));
    let download_detail = format!("Downloading Temurin Java {required_major} runtime...");
    on_progress(InstallProgressUpdate::stage(
        InstallPhase::Downloading,
        &download_detail,
    ));
    let archive = download_file_with_progress(
        client,
        &package.link,
        &archive_path,
        DownloadIntegrity {
            sha256: Some(&package.checksum),
            size: Some(package.size),
            ..Default::default()
        },
        deadline,
        |bytes, total| {
            on_progress(InstallProgressUpdate::download(
                &download_detail,
                bytes,
                total.or(Some(package.size)),
            ));
        },
    )
    .await?;
    let archive_path = archive.path().to_path_buf();
    let script = format!(
        "$ErrorActionPreference='Stop'\nExpand-Archive -LiteralPath {archive} -DestinationPath {stage} -Force\n",
        archive = ps_literal(&archive_path),
        stage = ps_literal(&staging_root),
    );
    on_progress(InstallProgressUpdate::stage(
        InstallPhase::Extracting,
        format!("Extracting Temurin Java {required_major} runtime..."),
    ));
    let output = match run_powershell(&script, Some(install_root), deadline).await {
        Ok(output) => output,
        Err(error @ SteamCmdError::InstallProcessCleanupFailed { .. }) => return Err(error),
        Err(error) => {
            let _ = fs::remove_file(&archive_path);
            await_failed_directory_cleanup(vec![staging_root.clone()]).await;
            return Err(error);
        }
    };
    let _ = fs::remove_file(&archive_path);
    if !output.status.success() {
        await_failed_directory_cleanup(vec![staging_root.clone()]).await;
        return Err(SteamCmdError::MinecraftJrePreparation {
            detail: output_excerpt(&output.stdout, &output.stderr),
        });
    }
    if let Err(error) = deadline.check_cancelled() {
        await_failed_directory_cleanup(vec![staging_root.clone()]).await;
        return Err(error);
    }
    let Some(staged_java) = find_file_named(&staging_root, "java.exe").filter(|path| {
        path.parent().and_then(Path::file_name) == Some(std::ffi::OsStr::new("bin"))
    }) else {
        await_failed_directory_cleanup(vec![staging_root.clone()]).await;
        return Err(SteamCmdError::MinecraftJrePreparation {
            detail: String::from("Temurin archive does not contain bin/java.exe"),
        });
    };
    let Some(staged_jre_root) = staged_java
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
    else {
        await_failed_directory_cleanup(vec![staging_root.clone()]).await;
        return Err(SteamCmdError::MinecraftJrePreparation {
            detail: String::from("Temurin archive has an invalid runtime layout"),
        });
    };

    on_progress(InstallProgressUpdate::stage(
        InstallPhase::Installing,
        format!("Publishing Temurin Java {required_major} runtime..."),
    ));
    if jre_root.exists()
        && let Err(source) = fs::rename(&jre_root, &rollback_root)
    {
        await_failed_directory_cleanup(vec![staging_root.clone()]).await;
        return Err(SteamCmdError::MinecraftJrePreparation {
            detail: format!("failed to stage current JRE for rollback: {source}"),
        });
    }
    if let Err(source) = fs::rename(&staged_jre_root, &jre_root) {
        let publish_error = SteamCmdError::MinecraftJrePreparation {
            detail: format!("failed to publish Java runtime: {source}"),
        };
        let quarantined =
            match restore_published_directory(&jre_root, &rollback_root, &rejected_root, false) {
                Ok(quarantined) => quarantined,
                Err(recovery_error) => {
                    await_failed_directory_cleanup(vec![staging_root.clone()]).await;
                    return Err(recovery_error);
                }
            };
        let mut cleanup_paths = vec![staging_root.clone()];
        cleanup_paths.extend(quarantined);
        await_failed_directory_cleanup(cleanup_paths).await;
        return Err(publish_error);
    }
    on_progress(InstallProgressUpdate::stage(
        InstallPhase::Verifying,
        format!("Verifying Temurin Java {required_major} runtime..."),
    ));
    let validation = java_major_version(&java_executable, deadline).await;
    finish_published_jre(
        validation,
        required_major,
        &jre_root,
        &rollback_root,
        &rejected_root,
        &staging_root,
    )
    .await
}

pub(super) async fn java_major_version(
    executable: &Path,
    deadline: InstallDeadline,
) -> Result<Option<u16>, SteamCmdError> {
    let mut command = Command::new(executable);
    command.arg("-version");
    let output = match run_command_capture(command, deadline).await {
        Ok(output) => output,
        Err(SteamCmdError::SpawnCommand { .. }) => return Ok(None),
        Err(error) => return Err(error),
    };
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let Some(version) = text.split('"').nth(1) else {
        return Ok(None);
    };
    let mut numbers = version.split('.');
    let Some(first) = numbers.next().and_then(|value| value.parse::<u16>().ok()) else {
        return Ok(None);
    };
    let major = if first == 1 {
        numbers.next().and_then(|value| value.parse::<u16>().ok())
    } else {
        Some(first)
    };
    Ok(major)
}

fn find_file_named(root: &Path, name: &str) -> Option<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).ok()?.filter_map(Result::ok) {
            let path = entry.path();
            if entry.file_type().ok()?.is_dir() {
                pending.push(path);
            } else if entry
                .file_name()
                .to_str()
                .is_some_and(|value| value.eq_ignore_ascii_case(name))
            {
                return Some(path);
            }
        }
    }
    None
}
