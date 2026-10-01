use super::http_download::download_file_with_progress;
use super::minecraft_metadata::fetch_minecraft_version_details;
use super::*;
use app_core::InstallPhase;

pub(super) async fn install_or_update_minecraft_java_module<F>(
    settings: &AppSettings,
    module: &ModuleDetails,
    install: &InstallSpec,
    process: &ProcessSpec,
    operation: &str,
    deadline: InstallDeadline,
    on_progress: &mut F,
) -> Result<ModuleInstallResult, SteamCmdError>
where
    F: FnMut(InstallProgressUpdate),
{
    let install_root = PathBuf::from(&settings.games_root).join(&install.shared_game_dir);
    fs::create_dir_all(&install_root).map_err(|source| SteamCmdError::CreatePath {
        path: install_root.clone(),
        source,
    })?;

    let minecraft = minecraft_java_spec(install);
    let manifest_url = minecraft
        .manifest_url
        .clone()
        .unwrap_or_else(|| String::from(MINECRAFT_VERSION_MANIFEST_URL));
    let server_jar_path =
        resolve_install_relative_path(&module.summary.id, &install_root, &minecraft.server_jar)?;

    on_progress(InstallProgressUpdate::stage(
        InstallPhase::Preparing,
        "Fetching Mojang Minecraft Java version manifest...",
    ));

    let client = http_client()?;
    let manifest: MinecraftVersionManifest = fetch_json(&client, &manifest_url, deadline).await?;
    let selected_version = select_minecraft_version(&manifest, &minecraft.version)?;

    on_progress(InstallProgressUpdate::stage(
        InstallPhase::Preparing,
        format!("Resolving Minecraft Java server {}", selected_version.id),
    ));

    let version_details =
        fetch_minecraft_version_details(&client, selected_version, deadline).await?;
    let server_download = version_details.downloads.server.ok_or_else(|| {
        SteamCmdError::MinecraftServerJarUnavailable {
            version: selected_version.id.clone(),
        }
    })?;
    let required_java_major = version_details.java_version.major_version;
    on_progress(InstallProgressUpdate::stage(
        InstallPhase::Preparing,
        format!("Preparing Temurin Java {required_java_major} runtime..."),
    ));
    ensure_minecraft_jre(
        &client,
        &install_root,
        required_java_major,
        deadline,
        on_progress,
    )
    .await?;
    let java_executable = install_root.join("jre").join("bin").join("java.exe");

    let existing_metadata = read_minecraft_install_metadata(&install_root).ok();
    let existing_candidate = existing_metadata
        .as_ref()
        .map(|metadata| {
            metadata.version_id == selected_version.id.as_str()
                && metadata
                    .server_sha1
                    .eq_ignore_ascii_case(&server_download.sha1)
                && metadata.required_java_major == required_java_major
                && java_executable.is_file()
                && server_jar_path.exists()
        })
        .unwrap_or(false);
    let existing_matches = if existing_candidate {
        on_progress(InstallProgressUpdate::stage(
            InstallPhase::Verifying,
            format!("Verifying Minecraft Java server {}...", selected_version.id),
        ));
        let path = server_jar_path.clone();
        let cancellation = InstallCancellation::current();
        tokio::task::spawn_blocking(move || sha1_file_hex(&path, deadline, cancellation))
            .await
            .map_err(|error| SteamCmdError::WriteMinecraftServerFile {
                path: server_jar_path.clone(),
                source: std::io::Error::other(format!(
                    "artifact verification worker failed: {error}"
                )),
            })?
            .map(|sha1| sha1.eq_ignore_ascii_case(&server_download.sha1))
            .or_else(|error| match error {
                error @ (SteamCmdError::InstallCancelled { .. }
                | SteamCmdError::OperationTimedOut { .. }) => Err(error),
                _ => Ok(false),
            })?
    } else {
        false
    };

    if existing_matches {
        let after = probe_module_install_state(
            settings,
            &module.summary.id,
            module.summary.steam_app_id,
            Some(install),
            Some(process),
        );
        let excerpt = format!(
            "Minecraft Java server {} is already current. server.jar verified with SHA1 {}.",
            selected_version.id, server_download.sha1
        );
        on_progress(
            InstallProgressUpdate::stage(
                InstallPhase::Ready,
                format!("Minecraft Java server {} is current", selected_version.id),
            )
            .with_output(excerpt.clone()),
        );

        return Ok(ModuleInstallResult {
            module_id: module.summary.id.clone(),
            steam_app_id: 0,
            operation: String::from(operation),
            install_root: after.install_root,
            executable_path: after.executable_path,
            executable_exists: after.executable_exists,
            install_state: after.install_state,
            current_version: Some(selected_version.id.clone()),
            output_excerpt: excerpt,
        });
    }

    let download_detail = format!(
        "Downloading Minecraft Java server {}...",
        selected_version.id
    );
    on_progress(InstallProgressUpdate::stage(
        InstallPhase::Downloading,
        &download_detail,
    ));

    let server = download_file_with_progress(
        &client,
        &server_download.url,
        &server_jar_path,
        DownloadIntegrity {
            sha1: Some(&server_download.sha1),
            size: Some(server_download.size),
            ..Default::default()
        },
        deadline,
        |bytes, total| {
            on_progress(InstallProgressUpdate::download(
                &download_detail,
                bytes,
                total.or(Some(server_download.size)),
            ));
        },
    )
    .await?;
    // The verified jar and metadata publish synchronously as one commit region.
    // Observe stop requests before entering it, never between its two writes.
    deadline.check_cancelled()?;
    on_progress(InstallProgressUpdate::stage(
        InstallPhase::Installing,
        format!(
            "Publishing Minecraft Java server {}...",
            selected_version.id
        ),
    ));
    publish_minecraft_staging_file(&server_jar_path, server.path())?;

    let metadata = MinecraftInstallMetadata {
        version_id: selected_version.id.clone(),
        version_type: selected_version.version_type.clone(),
        manifest_url: manifest_url.clone(),
        version_url: selected_version.url.clone(),
        server_url: server_download.url.clone(),
        server_sha1: server_download.sha1.clone(),
        server_size: server_download.size,
        server_jar: minecraft.server_jar.clone(),
        required_java_major,
        downloaded_at_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0),
    };
    write_minecraft_install_metadata(&install_root, &metadata)?;

    on_progress(InstallProgressUpdate::stage(
        InstallPhase::Verifying,
        format!("Verifying Minecraft Java server {}...", selected_version.id),
    ));
    let after = probe_module_install_state(
        settings,
        &module.summary.id,
        module.summary.steam_app_id,
        Some(install),
        Some(process),
    );
    if !matches!(after.install_state, InstallState::Installed) || !after.executable_exists {
        return Err(SteamCmdError::InstallationVerificationFailed {
            module_id: module.summary.id.clone(),
            operation: String::from(operation),
            detail: format!("server artifact or Java {required_java_major} runtime is missing"),
        });
    }

    let excerpt = format!(
        "Minecraft Java server {} downloaded to {} and verified with SHA1 {}.",
        selected_version.id,
        server_jar_path.display(),
        server_download.sha1
    );
    on_progress(
        InstallProgressUpdate::stage(
            InstallPhase::Ready,
            format!("Minecraft Java server {} ready", selected_version.id),
        )
        .with_output(excerpt.clone()),
    );

    Ok(ModuleInstallResult {
        module_id: module.summary.id.clone(),
        steam_app_id: 0,
        operation: String::from(operation),
        install_root: after.install_root,
        executable_path: after.executable_path,
        executable_exists: after.executable_exists,
        install_state: after.install_state,
        current_version: Some(selected_version.id.clone()),
        output_excerpt: excerpt,
    })
}

pub(super) fn minecraft_java_spec(install: &InstallSpec) -> MinecraftJavaInstallSpec {
    install
        .minecraft
        .clone()
        .unwrap_or(MinecraftJavaInstallSpec {
            version: String::from("latest_release"),
            manifest_url: Some(String::from(MINECRAFT_VERSION_MANIFEST_URL)),
            server_jar: String::from("server.jar"),
            java_policy: String::from("mojang_version_metadata"),
            default_distribution: String::from("vanilla"),
            distributions: Vec::new(),
        })
}

pub(super) fn select_minecraft_version<'a>(
    manifest: &'a MinecraftVersionManifest,
    requested: &str,
) -> Result<&'a MinecraftVersionManifestEntry, SteamCmdError> {
    let normalized = requested.trim().to_ascii_lowercase();
    let version_id = match normalized.as_str() {
        "" | "latest" | "latest_release" | "release" => manifest.latest.release.as_str(),
        "latest_snapshot" | "snapshot" => manifest.latest.snapshot.as_str(),
        _ => requested.trim(),
    };

    manifest
        .versions
        .iter()
        .find(|entry| entry.id == version_id)
        .ok_or_else(|| SteamCmdError::MinecraftVersionNotFound {
            version: String::from(version_id),
        })
}

fn write_minecraft_install_metadata(
    install_root: &Path,
    metadata: &MinecraftInstallMetadata,
) -> Result<(), SteamCmdError> {
    let path = minecraft_metadata_path(install_root);
    let text = serde_json::to_string_pretty(metadata).map_err(|source| {
        SteamCmdError::WriteMinecraftServerFile {
            path: path.clone(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, source),
        }
    })?;
    write_minecraft_server_file_atomically(&path, text.as_bytes())
}
