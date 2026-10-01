use super::minecraft_install::{minecraft_java_spec, select_minecraft_version};
use super::minecraft_metadata::fetch_minecraft_version_details;
use super::*;

/// Confirms the selected Minecraft release without changing program files,
/// revisions or install records. This allows another shared instance to start
/// while an already-current program is in use. Other installers must update
/// under their ordinary mutation admission instead of guessing from a timestamp.
pub async fn current_program_version(
    settings: &AppSettings,
    module: &ModuleDetails,
    root: &Path,
    guard: &GameInstallLifecycleGuard,
    cancellation: &InstallCancellation,
) -> Result<Option<String>, SteamCmdError> {
    current_program_version_with_java_probe(settings, module, root, guard, cancellation,
        |path, deadline| async move { super::minecraft_java::java_major_version(&path, deadline).await }).await
}

pub(crate) async fn current_program_version_with_java_probe<F, Fut>(
    settings: &AppSettings,
    module: &ModuleDetails,
    root: &Path,
    guard: &GameInstallLifecycleGuard,
    cancellation: &InstallCancellation,
    java_probe: F,
) -> Result<Option<String>, SteamCmdError>
where
    F: FnOnce(PathBuf, InstallDeadline) -> Fut,
    Fut: std::future::Future<Output = Result<Option<u16>, SteamCmdError>>,
{
    guard.ensure_scope(&module.summary.id, root)?;
    let Some(install) = module
        .install
        .as_ref()
        .filter(|install| is_minecraft_java_install(install))
    else {
        return Ok(None);
    };
    match read_program_install_revision(Path::new(&settings.servers_root), &module.summary.id, root)
    {
        Ok(_) => {}
        // An interrupted update needs the installer recovery path. It must never
        // be treated as an already-current program, even if its jar matches.
        Err(SteamCmdError::PackageRevisionPending { .. }) => return Ok(None),
        Err(error) => return Err(error),
    }
    cancellation
        .scope(async {
            let deadline = InstallDeadline::new(
                "checking the current server version",
                Duration::from_secs(60),
            );
            deadline.check_cancelled()?;
            let spec = minecraft_java_spec(install);
            let client = http_client()?;
            let manifest: MinecraftVersionManifest = fetch_json(
                &client,
                spec.manifest_url
                    .as_deref()
                    .unwrap_or(MINECRAFT_VERSION_MANIFEST_URL),
                deadline,
            )
            .await?;
            let selected = select_minecraft_version(&manifest, &spec.version)?;
            let details = fetch_minecraft_version_details(&client, selected, deadline).await?;
            let download = details.downloads.server.ok_or_else(|| {
                SteamCmdError::MinecraftServerJarUnavailable {
                    version: selected.id.clone(),
                }
            })?;
            let Ok(metadata) = read_minecraft_install_metadata(root) else {
                return Ok(None);
            };
            if metadata.version_id != selected.id
                || !metadata.server_sha1.eq_ignore_ascii_case(&download.sha1)
                || metadata.required_java_major != details.java_version.major_version
                || !root.join("jre/bin/java.exe").is_file()
            {
                return Ok(None);
            }
            if java_probe(root.join("jre/bin/java.exe"), deadline).await?
                != Some(details.java_version.major_version)
            {
                return Ok(None);
            }
            let jar = resolve_install_relative_path(&module.summary.id, root, &spec.server_jar)?;
            let token = Some(cancellation.clone());
            let error_path = jar.clone();
            let digest = tokio::task::spawn_blocking(move || sha1_file_hex(&jar, deadline, token))
                .await
                .map_err(|error| SteamCmdError::WriteMinecraftServerFile {
                    path: error_path,
                    source: std::io::Error::other(format!(
                        "artifact verification worker failed: {error}"
                    )),
                })??;
            deadline.check_cancelled()?;
            Ok(digest
                .eq_ignore_ascii_case(&download.sha1)
                .then(|| selected.id.clone()))
        })
        .await
}
