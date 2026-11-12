use super::http_download::download_file_with_progress;
use super::*;
use app_core::InstallPhase;

#[path = "minecraft_java_metadata.rs"]
mod metadata;

#[cfg(any(test, feature = "test-support"))]
tokio::task_local! {
    static PACKAGE_FIXTURE: (String, String, u64);
}

/// Scope only the upstream package description; download integrity, extraction,
/// Java verification and publication still execute normally in fixture tests.
#[cfg(any(test, feature = "test-support"))]
pub fn with_minecraft_java_package_fixture<T>(
    url: String,
    sha256: String,
    size: u64,
    future: impl std::future::Future<Output = T>,
) -> impl std::future::Future<Output = T> {
    PACKAGE_FIXTURE.scope((url, sha256, size), Box::pin(future))
}

pub(super) async fn latest_runtime_package(
    client: &reqwest::Client,
    major: u16,
    deadline: InstallDeadline,
) -> Result<(String, String, u64), SteamCmdError> {
    let package = metadata::fetch_java_package(client, major, deadline).await?;
    Ok((package.link, package.checksum, package.size))
}

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
    let script = minecraft_jre_extraction_script(&archive_path, &staging_root);
    on_progress(InstallProgressUpdate::stage(
        InstallPhase::Extracting,
        format!("Extracting Temurin Java {required_major} runtime..."),
    ));
    // The archive and stage are absolute. CreateProcess's working directory
    // still has legacy length limits even when native file handles do not.
    let output = match run_powershell(&script, None, deadline).await {
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
    .await?;
    // The caller records this isolated payload immediately. Join cleanup of
    // extraction scratch directories before they can enter that inventory.
    await_failed_directory_cleanup(vec![rollback_root.clone(), staging_root.clone()]).await;
    if rollback_root.exists() || staging_root.exists() {
        return Err(SteamCmdError::MinecraftJrePreparation {
            detail: String::from(
                "Java extraction scratch could not be removed before inventory publication",
            ),
        });
    }
    Ok(())
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

fn minecraft_jre_extraction_script(archive: &Path, stage: &Path) -> String {
    format!(
        "$ErrorActionPreference='Stop'\nAdd-Type -TypeDefinition @'\n{}\n'@\n{}\nExpand-MinecraftJavaArchive {} {}\n",
        include_str!("install_files.cs"),
        include_str!("minecraft_java_extract.ps1"),
        ps_literal(archive),
        ps_literal(stage),
    )
}

#[cfg(all(test, windows))]
#[path = "minecraft_java_extract_tests.rs"]
mod extract_tests;
