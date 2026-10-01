use super::http_download::download_file_with_progress;
use super::install_publication_metadata::InstallPublicationMetadata;
use super::*;
use app_core::InstallPhase;

static ARCHIVE_INSTALL_SEQUENCE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

struct ArchiveScratchPaths {
    staging_root: PathBuf,
    rollback_root: PathBuf,
    rejected_root: PathBuf,
    publish_phase_path: PathBuf,
    archive_path: PathBuf,
}

impl ArchiveScratchPaths {
    fn new(parent: &Path) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let sequence = ARCHIVE_INSTALL_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let operation = format!("{:x}-{stamp:x}-{sequence:x}", std::process::id());
        // Keep transactional siblings on the same volume, without repeating
        // an installation name that may already contain an acquisition UUID.
        let sibling = |suffix| parent.join(format!(".lg-{operation}.{suffix}"));
        Self {
            staging_root: sibling("stage"),
            rollback_root: sibling("rollback"),
            rejected_root: sibling("rejected"),
            publish_phase_path: sibling("publish.state"),
            archive_path: sibling("download.zip"),
        }
    }
}

pub(super) struct DirectDownloadInstallRequest<'a> {
    pub(super) settings: &'a AppSettings,
    pub(super) module: &'a ModuleDetails,
    pub(super) install: &'a InstallSpec,
    pub(super) process: &'a ProcessSpec,
    pub(super) steam_app_id: u32,
    pub(super) operation: &'a str,
    pub(super) download_url: &'a str,
    pub(super) deadline: InstallDeadline,
}

pub(super) async fn install_or_update_module_from_download<F, C, Fut>(
    request: DirectDownloadInstallRequest<'_>,
    on_progress: &mut F,
    prepare_fresh_payload: &mut C,
) -> Result<ModuleInstallResult, SteamCmdError>
where
    F: FnMut(InstallProgressUpdate),
    C: FnMut(PathBuf) -> Fut,
    Fut: std::future::Future<Output = Result<(), SteamCmdError>>,
{
    let DirectDownloadInstallRequest {
        settings,
        module,
        install,
        process,
        steam_app_id,
        operation,
        download_url,
        deadline,
    } = request;
    let install_root = PathBuf::from(&settings.games_root).join(&install.shared_game_dir);
    let install_parent = install_root
        .parent()
        .unwrap_or(Path::new(&settings.games_root));
    fs::create_dir_all(install_parent).map_err(|source| SteamCmdError::CreatePath {
        path: install_parent.to_path_buf(),
        source,
    })?;

    let ArchiveScratchPaths {
        staging_root,
        rollback_root,
        rejected_root,
        publish_phase_path,
        archive_path,
    } = ArchiveScratchPaths::new(install_parent);
    let verification_relative = install
        .verification_path
        .as_deref()
        .and_then(safe_install_relative_path)
        .ok_or_else(|| SteamCmdError::InvalidInstallRelativePath {
            module_id: module.summary.id.clone(),
            path: install.verification_path.clone().unwrap_or_default(),
        })?;
    let staging_verification = staging_root.join(verification_relative);
    let preserve_retained_data = has_retained_install_data(&install_root);
    let publication_metadata = InstallPublicationMetadata::read(
        &install_root,
        &module.summary.id,
        preserve_retained_data,
    )?;
    if preserve_retained_data
        && probe_module_install_state(
            settings,
            &module.summary.id,
            module.summary.steam_app_id,
            Some(install),
            Some(process),
        )
        .install_state
            != InstallState::NotInstalled
    {
        return Err(SteamCmdError::DirectDownloadFailed {
            output_excerpt: String::from(
                "Retained installation data conflicts with the required server payload.",
            ),
        });
    }
    let download_detail = format!(
        "Downloading {} package from {}...",
        module.summary.name, download_url
    );
    on_progress(InstallProgressUpdate::stage(
        InstallPhase::Downloading,
        &download_detail,
    ));

    let client = http_client()?;
    let archive = download_file_with_progress(
        &client,
        download_url,
        &archive_path,
        install
            .download_integrity_windows
            .as_ref()
            .map(|integrity| DownloadIntegrity {
                sha256: Some(&integrity.sha256),
                size: Some(integrity.size),
                ..Default::default()
            })
            .unwrap_or_default(),
        deadline,
        |bytes, total| {
            on_progress(InstallProgressUpdate::download(
                &download_detail,
                bytes,
                total,
            ));
        },
    )
    .await?;
    let archive_path = archive.path().to_path_buf();
    let script = super::install_transaction::direct_download_stage_script(
        &install_root,
        &staging_root,
        &rollback_root,
        &publish_phase_path,
        &staging_verification,
        &archive_path,
        &publication_metadata,
    );

    on_progress(InstallProgressUpdate::stage(
        InstallPhase::Extracting,
        format!("Extracting {} package...", module.summary.name),
    ));
    let preparation = async {
        let output = run_powershell(&script, Some(install_parent), deadline).await?;
        if !output.status.success() {
            return Err(SteamCmdError::DirectDownloadFailed {
                output_excerpt: output_excerpt(&output.stdout, &output.stderr),
            });
        }
        deadline.check_cancelled()?;
        on_progress(InstallProgressUpdate::stage(
            InstallPhase::Verifying,
            "Recording the original server package inventory...",
        ));
        deadline.check_cancelled()?;
        prepare_fresh_payload(staging_root.clone()).await?;
        deadline.check_cancelled()?;
        let publish = super::install_transaction::prepared_install_publish_script(
            &install_root,
            &staging_root,
            &rollback_root,
            &publish_phase_path,
            &staging_verification,
            &publication_metadata,
        );
        run_powershell(&publish, Some(install_parent), deadline).await
    }
    .await;
    let output = match preparation {
        Ok(output) => output,
        Err(error @ SteamCmdError::InstallProcessCleanupFailed { .. }) => return Err(error),
        Err(error) => {
            recover_direct_download_install(
                &install_root,
                &staging_root,
                &rollback_root,
                &rejected_root,
                &publish_phase_path,
                &archive_path,
                false,
            )
            .await?;
            return Err(error);
        }
    };
    if !output.status.success() {
        recover_direct_download_install(
            &install_root,
            &staging_root,
            &rollback_root,
            &rejected_root,
            &publish_phase_path,
            &archive_path,
            false,
        )
        .await?;
        return Err(SteamCmdError::DirectDownloadFailed {
            output_excerpt: output_excerpt(&output.stdout, &output.stderr),
        });
    }

    if let Err(error) = deadline.check_cancelled() {
        recover_direct_download_install(
            &install_root,
            &staging_root,
            &rollback_root,
            &rejected_root,
            &publish_phase_path,
            &archive_path,
            true,
        )
        .await?;
        return Err(error);
    }

    on_progress(
        InstallProgressUpdate::stage(
            InstallPhase::Verifying,
            format!("Verifying extracted files in {}...", install_root.display()),
        )
        .with_output(output_excerpt(&output.stdout, &output.stderr)),
    );

    let after = probe_module_install_state(
        settings,
        &module.summary.id,
        module.summary.steam_app_id,
        Some(install),
        Some(process),
    );
    if after.install_state != InstallState::Installed {
        recover_direct_download_install(
            &install_root,
            &staging_root,
            &rollback_root,
            &rejected_root,
            &publish_phase_path,
            &archive_path,
            true,
        )
        .await?;
        return Err(SteamCmdError::InstallationVerificationFailed {
            module_id: module.summary.id.clone(),
            operation: String::from(operation),
            detail: after.diagnostics.join(" "),
        });
    }
    if let Err(error) = deadline.check_cancelled() {
        recover_direct_download_install(
            &install_root,
            &staging_root,
            &rollback_root,
            &rejected_root,
            &publish_phase_path,
            &archive_path,
            true,
        )
        .await?;
        return Err(error);
    }
    // The previous payload remains intact until the newly published payload
    // has passed the same probe used by normal install-state detection.
    schedule_verified_directory_cleanup(vec![rollback_root.clone(), staging_root.clone()]);
    archive.record_success();
    let _ = fs::remove_file(&publish_phase_path);

    let excerpt = output_excerpt(&output.stdout, &output.stderr);
    on_progress(
        InstallProgressUpdate::stage(
            InstallPhase::Ready,
            format!(
                "{} complete. Executable ready at {}",
                capitalize_operation(operation),
                after.executable_path
            ),
        )
        .with_output(excerpt.clone()),
    );

    Ok(ModuleInstallResult {
        module_id: module.summary.id.clone(),
        steam_app_id,
        operation: String::from(operation),
        install_root: after.install_root,
        executable_path: after.executable_path,
        executable_exists: after.executable_exists,
        install_state: after.install_state,
        current_version: after.current_version,
        output_excerpt: excerpt,
    })
}

#[cfg(all(test, windows))]
#[path = "archive_install_retained_tests.rs"]
mod retained_tests;

#[cfg(all(test, windows))]
#[path = "archive_install_path_tests.rs"]
mod path_tests;
