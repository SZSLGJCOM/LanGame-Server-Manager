use super::http_download::download_file_with_progress;
use super::minecraft_metadata::fetch_minecraft_version_details;
use super::*;
use app_core::InstallPhase;

#[path = "minecraft_publication.rs"]
mod publication;

pub(super) struct MinecraftInstallRequest<'a> {
    pub(super) settings: &'a AppSettings,
    pub(super) module: &'a ModuleDetails,
    pub(super) install: &'a InstallSpec,
    pub(super) process: &'a ProcessSpec,
    pub(super) operation: &'a str,
    pub(super) deadline: InstallDeadline,
    pub(super) revision_key: &'a str,
}

pub(super) async fn install_or_update_minecraft_java_module<F, C, Fut>(
    request: MinecraftInstallRequest<'_>,
    on_progress: &mut F,
    prepare_fresh_payload: &mut C,
) -> Result<ModuleInstallResult, SteamCmdError>
where
    F: FnMut(InstallProgressUpdate),
    C: FnMut(PathBuf) -> Fut,
    Fut: std::future::Future<Output = Result<(), SteamCmdError>>,
{
    let MinecraftInstallRequest {
        settings,
        module,
        install,
        process,
        operation,
        deadline,
        revision_key,
    } = request;
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
    // Validate the configured relative path before allocating an owned payload.
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
    let publication = publication::MinecraftPublication::new(&install_root, &module.summary.id)?;
    // This phase owns several large network/process futures. Keep its state on
    // the heap instead of embedding it in every enclosing update/start future.
    let preparation = Box::pin(async {
        on_progress(InstallProgressUpdate::stage(
            InstallPhase::Preparing,
            format!("Preparing Temurin Java {required_java_major} runtime..."),
        ));
        // A version probe of an old JRE is not package provenance. This root is
        // empty, so every inventoried Java file comes from the verified ZIP.
        ensure_minecraft_jre(
            &client,
            &publication.stage,
            required_java_major,
            deadline,
            on_progress,
        )
        .await?;
        let staged_jar = resolve_install_relative_path(
            &module.summary.id,
            &publication.stage,
            &minecraft.server_jar,
        )?;
        let detail = format!(
            "Downloading Minecraft Java server {}...",
            selected_version.id
        );
        on_progress(InstallProgressUpdate::stage(
            InstallPhase::Downloading,
            &detail,
        ));
        let server = download_file_with_progress(
            &client,
            &server_download.url,
            &staged_jar,
            DownloadIntegrity {
                sha1: Some(&server_download.sha1),
                size: Some(server_download.size),
                ..Default::default()
            },
            deadline,
            |bytes, total| {
                on_progress(InstallProgressUpdate::download(
                    &detail,
                    bytes,
                    total.or(Some(server_download.size)),
                ))
            },
        )
        .await?;
        server.persist(&staged_jar)?;
        write_minecraft_install_metadata(&publication.stage, &metadata)?;
        deadline.check_cancelled()?;
        on_progress(InstallProgressUpdate::stage(
            InstallPhase::Verifying,
            "Recording the original Minecraft package inventory...",
        ));
        prepare_fresh_payload(publication.stage.clone()).await?;
        on_progress(InstallProgressUpdate::stage(
            InstallPhase::Preparing,
            "Preserving Minecraft configuration, worlds and operator files...",
        ));
        publication.merge_existing(deadline).await?;
        on_progress(InstallProgressUpdate::stage(
            InstallPhase::Installing,
            format!(
                "Publishing Minecraft Java server {}...",
                selected_version.id
            ),
        ));
        // All preceding work only touched the isolated stage. A failed source
        // request or inventory callback must leave the old revision usable.
        deadline.check_cancelled()?;
        let revision =
            begin_game_install_revision(Path::new(&settings.servers_root), revision_key)?;
        publication
            .publish(Path::new(&minecraft.server_jar), deadline)
            .await?;
        Ok(revision)
    })
    .await;
    let revision = match preparation {
        Ok(revision) => revision,
        Err(error) => {
            if !matches!(&error, SteamCmdError::InstallProcessCleanupFailed { .. }) {
                publication.recover(false).await?;
            }
            return Err(error);
        }
    };

    if let Err(error) = deadline.check_cancelled() {
        publication.recover(true).await?;
        return Err(error);
    }

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
        publication.recover(true).await?;
        return Err(SteamCmdError::InstallationVerificationFailed {
            module_id: module.summary.id.clone(),
            operation: String::from(operation),
            detail: format!("server artifact or Java {required_java_major} runtime is missing"),
        });
    }
    if let Err(error) = deadline.check_cancelled() {
        publication.recover(true).await?;
        return Err(error);
    }
    complete_game_install_revision(Path::new(&settings.servers_root), revision_key, revision)?;
    publication.finish();

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
