use super::steam_install_retained::RetainedSteamInstall;
use super::steamcmd_manifest_recovery::InstalledManifest;
use super::steamcmd_stream::{sleep_with_deadline, steamcmd_output_is_retryable_file_lock};
use super::*;
use app_core::InstallPhase;

#[path = "steam_install_dependencies.rs"]
pub(super) mod shared_dependencies;

/// Observers for installation progress and a verified, isolated payload before
/// retained operator data is merged. In-place installations never invoke the
/// payload callback. It may persist package metadata, but must not move the tree.
pub struct ModuleInstallCallbacks<P, C> {
    pub on_progress: P,
    pub prepare_fresh_payload: C,
}

/// A caller-owned installation target and its optional cached Steam identity.
/// Passwords and Steam Guard input belong to interactive authentication only.
pub struct SteamModuleInstallRequest<'a> {
    pub settings: &'a AppSettings,
    pub module: &'a ModuleDetails,
    pub install_root: &'a Path,
    pub guard: &'a GameInstallLifecycleGuard,
    pub validate: bool,
    pub cancellation: &'a InstallCancellation,
    pub account_name: Option<&'a str>,
}

/// Installs to one explicitly selected program directory while the caller owns
/// the lifecycle lock. The caller must retain that lock through its filesystem
/// ownership and persistence transition; this function never reacquires it.
pub async fn install_or_update_module_at_with_progress_and_cancellation<F>(
    settings: &AppSettings,
    module: &ModuleDetails,
    install_root: &Path,
    guard: &GameInstallLifecycleGuard,
    validate: bool,
    cancellation: &InstallCancellation,
    on_progress: F,
) -> Result<ModuleInstallResult, SteamCmdError>
where
    F: FnMut(InstallProgressUpdate),
{
    install_or_update_module_at_with_callbacks(
        settings,
        module,
        install_root,
        guard,
        validate,
        cancellation,
        ModuleInstallCallbacks {
            on_progress,
            prepare_fresh_payload: |_| async { Ok(()) },
        },
    )
    .await
}

/// The payload callback is awaited under the installation lease. Blocking work
/// belongs in the caller's blocking worker and must finish before returning;
/// callback errors prevent publication and keep the prior installation intact.
pub async fn install_or_update_module_at_with_callbacks<P, C, Fut>(
    settings: &AppSettings,
    module: &ModuleDetails,
    install_root: &Path,
    guard: &GameInstallLifecycleGuard,
    validate: bool,
    cancellation: &InstallCancellation,
    callbacks: ModuleInstallCallbacks<P, C>,
) -> Result<ModuleInstallResult, SteamCmdError>
where
    P: FnMut(InstallProgressUpdate),
    C: FnMut(PathBuf) -> Fut,
    Fut: std::future::Future<Output = Result<(), SteamCmdError>>,
{
    install_or_update_module_at_with_login_and_callbacks(
        SteamModuleInstallRequest {
            settings,
            module,
            install_root,
            guard,
            validate,
            cancellation,
            account_name: None,
        },
        callbacks,
    )
    .await
}

/// Uses a previously authenticated Steam account for every native attempt,
/// including manifest recovery and shared dependency validation. A failed or
/// mismatched cached login never falls back to anonymous installation.
pub async fn install_or_update_module_at_with_login_and_callbacks<P, C, Fut>(
    request: SteamModuleInstallRequest<'_>,
    callbacks: ModuleInstallCallbacks<P, C>,
) -> Result<ModuleInstallResult, SteamCmdError>
where
    P: FnMut(InstallProgressUpdate),
    C: FnMut(PathBuf) -> Fut,
    Fut: std::future::Future<Output = Result<(), SteamCmdError>>,
{
    let SteamModuleInstallRequest {
        settings,
        module,
        install_root,
        guard,
        validate,
        cancellation,
        account_name,
    } = request;
    let account_name = account_name
        .map(normalize_workshop_account_name)
        .transpose()?;
    guard.ensure_scope(&module.summary.id, install_root)?;
    let (target_settings, target_module) = explicit_install_target(settings, module, install_root)?;
    let revision_key = package_revision::program_revision_key(&module.summary.id, install_root)?;
    cancellation
        .scope(install_module(
            &target_settings,
            &target_module,
            validate,
            InstallLifecycle {
                guard,
                revision_key: &revision_key,
                account_name: account_name.as_deref(),
            },
            InstallDeadline::new("game server install or validation", MODULE_INSTALL_TIMEOUT),
            std::time::Instant::now(),
            callbacks,
        ))
        .await
}

fn explicit_install_target(
    settings: &AppSettings,
    module: &ModuleDetails,
    install_root: &Path,
) -> Result<(AppSettings, ModuleDetails), SteamCmdError> {
    package_revision::validate_program_revision_root(install_root)?;
    let invalid = || SteamCmdError::PackageRevisionIo {
        path: install_root.to_owned(),
        source: std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "program target must have a UTF-8 parent and directory name",
        ),
    };
    let parent = install_root
        .parent()
        .and_then(Path::to_str)
        .ok_or_else(invalid)?;
    let name = install_root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(invalid)?;
    let mut target_settings = settings.clone();
    target_settings.games_root = parent.to_owned();
    let mut target_module = module.clone();
    let install =
        target_module
            .install
            .as_mut()
            .ok_or_else(|| SteamCmdError::MissingInstallSpec {
                module_id: module.summary.id.clone(),
            })?;
    install.shared_game_dir = name.to_owned();
    Ok((target_settings, target_module))
}

struct InstallLifecycle<'a> {
    guard: &'a GameInstallLifecycleGuard,
    revision_key: &'a str,
    account_name: Option<&'a str>,
}

async fn install_module<F, C, Fut>(
    settings: &AppSettings,
    module: &ModuleDetails,
    validate: bool,
    lifecycle: InstallLifecycle<'_>,
    deadline: InstallDeadline,
    started_at: std::time::Instant,
    callbacks: ModuleInstallCallbacks<F, C>,
) -> Result<ModuleInstallResult, SteamCmdError>
where
    F: FnMut(InstallProgressUpdate),
    C: FnMut(PathBuf) -> Fut,
    Fut: std::future::Future<Output = Result<(), SteamCmdError>>,
{
    let ModuleInstallCallbacks {
        mut on_progress,
        mut prepare_fresh_payload,
    } = callbacks;
    let InstallLifecycle {
        guard: game_guard,
        revision_key,
        account_name,
    } = lifecycle;
    let mut on_progress = |mut update: InstallProgressUpdate| {
        if let Some(progress) = update.install_progress.as_mut() {
            progress.elapsed_seconds = started_at.elapsed().as_secs();
        }
        on_progress(update);
    };
    deadline.check_cancelled()?;
    on_progress(InstallProgressUpdate::stage(
        InstallPhase::Preparing,
        format!("Preparing {}...", module.summary.name),
    ));
    let install = module
        .install
        .as_ref()
        .ok_or_else(|| SteamCmdError::MissingInstallSpec {
            module_id: module.summary.id.clone(),
        })?;
    let process = module
        .process
        .as_ref()
        .ok_or_else(|| SteamCmdError::MissingProcessSpec {
            module_id: module.summary.id.clone(),
        })?;
    let minecraft_java = is_minecraft_java_install(install);
    let steam_app_id = module.summary.steam_app_id.unwrap_or(0);
    let direct_download_url = if cfg!(windows) {
        install.download_url_windows.as_deref()
    } else {
        None
    };
    if !minecraft_java && direct_download_url.is_none() && steam_app_id == 0 {
        return Err(SteamCmdError::MissingInstallSource {
            module_id: module.summary.id.clone(),
        });
    }

    // Dependency preparation belongs to the explicit System action. A managed
    // runtime that updated itself can be reverified here; reject missing or
    // never-verified dependencies before creating files or invalidating revisions.
    let _steamcmd_operation = if !minecraft_java && direct_download_url.is_none() {
        on_progress(InstallProgressUpdate::stage(
            InstallPhase::Queued,
            "Waiting for the SteamCMD runtime lock...",
        ));
        let guard = game_guard
            .acquire_steamcmd_with_deadline(settings, deadline)
            .await?;
        guard
            .require_ready_with_progress(settings, deadline, |update| {
                on_progress(InstallProgressUpdate::preparing_steamcmd(update));
            })
            .await?;
        Some(guard)
    } else {
        None
    };

    let install_root = PathBuf::from(&settings.games_root).join(&install.shared_game_dir);
    fs::create_dir_all(&install_root).map_err(|source| SteamCmdError::CreatePath {
        path: install_root.clone(),
        source,
    })?;

    let before = probe_module_install_state(
        settings,
        &module.summary.id,
        module.summary.steam_app_id,
        Some(install),
        Some(process),
    );
    let operation = if validate {
        String::from("validate")
    } else if matches!(before.install_state, InstallState::Installed) {
        String::from("update")
    } else {
        String::from("install")
    };
    // Native installers may write in place or publish a whole replacement.
    // Invalidate private runtime baselines before either operation can begin.
    let revision = begin_game_install_revision(Path::new(&settings.servers_root), revision_key)?;
    let install_result = async {
        if minecraft_java {
            return install_or_update_minecraft_java_module(
                settings,
                module,
                install,
                process,
                &operation,
                deadline,
                &mut on_progress,
            )
            .await;
        }

        if let Some(download_url) = direct_download_url {
            on_progress(InstallProgressUpdate::stage(
                InstallPhase::Preparing,
                format!(
                    "Using direct download payload for {}...",
                    module.summary.name
                ),
            ));
            return install_or_update_module_from_download(
                DirectDownloadInstallRequest {
                    settings,
                    module,
                    install,
                    process,
                    steam_app_id,
                    operation: &operation,
                    download_url,
                    deadline,
                },
                &mut on_progress,
                &mut prepare_fresh_payload,
            )
            .await;
        }

        let steamcmd = require_steamcmd_ready(settings)?;
        on_progress(InstallProgressUpdate::stage(
            InstallPhase::Preparing,
            format!("SteamCMD ready: {}", steamcmd.executable_path),
        ));

        let mut retained =
            RetainedSteamInstall::prepare(&install_root, module, before.install_state)?;
        let acquisition_root = retained
            .as_ref()
            .map(|retained| &retained.stage)
            .unwrap_or(&install_root)
            .clone();
        let install_result = async {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_millis())
                .unwrap_or(0);
            let script_root = steamcmd_script_root();
            fs::create_dir_all(&script_root).map_err(|source| SteamCmdError::CreatePath {
                path: script_root.clone(),
                source,
            })?;
            let script_path =
                script_root.join(format!("steamcmd-{}-{stamp}.txt", module.summary.id));

            let script_lines = steamcmd_install_script_lines_with_login(
                module, steam_app_id, &acquisition_root, validate, account_name,
            );

            fs::write(
                &script_path,
                script_lines.join(
                    "
",
                ),
            )
            .map_err(|source| SteamCmdError::WriteScript {
                path: script_path.clone(),
                source,
            })?;

            on_progress(InstallProgressUpdate::stage(
                InstallPhase::Preparing,
                format!(
                    "Starting SteamCMD {} for AppID {}...",
                    operation, steam_app_id
                ),
            ));

            let mut last_excerpt = String::new();
            let mut command_succeeded = false;
            let mut manifest_recovery: Option<PathBuf> = None;
            // Existing transient retries retain their budget; one additional
            // attempt is reserved for a positively identified old manifest.
            for attempt in 0..6 {
                deadline.check_cancelled()?;
                let installed_manifest = if manifest_recovery.is_none() {
                    InstalledManifest::before_attempt(&acquisition_root, steam_app_id).await
                } else {
                    None
                };
                let output = match run_steamcmd_script_with_progress(
                    &steamcmd.executable_path,
                    &script_path,
                    &steamcmd.root,
                    deadline,
                    |update| on_progress(with_retry_context(update, attempt)),
                )
                .await
                {
                    Ok(output) => output,
                    Err(error) => {
                        let _ = fs::remove_file(&script_path);
                        return Err(error);
                    }
                };

                let succeeded = match super::steam_app_access::verify_command_result(
                    steam_app_id, account_name, &output,
                ) {
                    Ok(()) => true,
                    Err(error @ (SteamCmdError::SteamAppAccountRequired { .. }
                        | SteamCmdError::SteamAccountAuthenticationFailed { .. })) => {
                        let _ = fs::remove_file(&script_path);
                        return Err(error);
                    }
                    Err(_) => false,
                };
                last_excerpt = if succeeded {
                    output.excerpt.clone()
                } else {
                    super::steam_app_access::failure_context(steam_app_id, &output)
                };
                if succeeded {
                    if let Some(backup) = &manifest_recovery {
                        on_progress(InstallProgressUpdate::stage(
                            InstallPhase::Verifying,
                            format!(
                                "Steam manifest recovery validation completed; verifying server files. Preserved manifests: {}",
                                backup.display()
                            ),
                        ));
                    }
                    command_succeeded = true;
                    break;
                }

                if let Some(installed) = installed_manifest
                    && let Some(denied) = installed.matching_denial(&output.denied_manifests)
                {
                    on_progress(
                        InstallProgressUpdate::stage(
                            InstallPhase::Preparing,
                            format!(
                                "Steam denied installed manifest {} for AppID {}, depot {}. Preserving manifests before one validation recovery...",
                                denied.manifest, denied.app, denied.depot
                            ),
                        )
                        .with_output(last_excerpt.clone()),
                    );
                    let backup = match installed.preserve_and_reset(deadline).await {
                        Ok(backup) => backup,
                        Err(error) => {
                            let _ = fs::remove_file(&script_path);
                            return Err(match error {
                                SteamCmdError::SteamCmdCommandFailed { output_excerpt } => {
                                    SteamCmdError::SteamCmdCommandFailed {
                                        output_excerpt: format!("{last_excerpt}\n{output_excerpt}"),
                                    }
                                }
                                error => error,
                            });
                        }
                    };
                    on_progress(InstallProgressUpdate::stage(
                        InstallPhase::Preparing,
                        format!(
                            "Preserved Steam manifests at {}. Retrying AppID {} with validation...",
                            backup.display(), steam_app_id
                        ),
                    ));
                    manifest_recovery = Some(backup);
                    let lines = steamcmd_install_script_lines_with_login(
                        module, steam_app_id, &acquisition_root, true, account_name,
                    );
                    if let Err(source) = fs::write(&script_path, lines.join("\r\n")) {
                        let _ = fs::remove_file(&script_path);
                        return Err(SteamCmdError::WriteScript {
                            path: script_path.clone(), source,
                        });
                    }
                    continue;
                }

                if let Some(backup) = &manifest_recovery {
                    last_excerpt.push_str(&format!(
                        "\nSteam manifest recovery failed; preserved manifests: {}",
                        backup.display()
                    ));
                    let _ = fs::remove_file(&script_path);
                    return Err(SteamCmdError::SteamCmdCommandFailed {
                        output_excerpt: last_excerpt,
                    });
                }

                let retryable_missing_configuration = attempt == 0
                    && last_excerpt
                        .to_ascii_lowercase()
                        .contains("missing configuration");
                if retryable_missing_configuration {
                    on_progress(
                        InstallProgressUpdate::stage(
                            InstallPhase::Preparing,
                            "SteamCMD reported missing configuration. Retrying once...",
                        )
                        .with_output(last_excerpt.clone()),
                    );
                    if let Err(error) = sleep_with_deadline(Duration::from_secs(3), deadline).await
                    {
                        let _ = fs::remove_file(&script_path);
                        return Err(error);
                    }
                    continue;
                }

                let retryable_file_lock =
                    attempt < 4 && steamcmd_output_is_retryable_file_lock(&last_excerpt);
                if retryable_file_lock {
                    on_progress(
                InstallProgressUpdate::stage(
                    InstallPhase::Preparing,
                    String::from(
                        "SteamCMD hit a transient file lock while writing the install. Retrying...",
                    ),
                )
                .with_output(last_excerpt.clone()),
            );
                    if let Err(error) = sleep_with_deadline(Duration::from_secs(5), deadline).await
                    {
                        let _ = fs::remove_file(&script_path);
                        return Err(error);
                    }
                    continue;
                }

                let _ = fs::remove_file(&script_path);
                return Err(SteamCmdError::SteamCmdCommandFailed {
                    output_excerpt: last_excerpt,
                });
            }

            if !command_succeeded {
                let _ = fs::remove_file(&script_path);
                return Err(SteamCmdError::SteamCmdCommandFailed {
                    output_excerpt: last_excerpt,
                });
            }
            // Steam can report the main app installed while shared runtimes
            // remain pending. Complete those before inventory or publication.
            let completion = shared_dependencies::complete_shared_dependencies(
                shared_dependencies::SharedDependencyCompletion {
                    steamcmd: &steamcmd,
                    module,
                    app_id: steam_app_id,
                    root: &acquisition_root,
                    script: &script_path,
                    deadline,
                    account_name,
                },
                &mut on_progress,
                last_excerpt,
            )
            .await;
            let _ = fs::remove_file(&script_path);
            let excerpt = completion?;
            deadline.check_cancelled()?;
            if let Some(retained) = retained.as_mut() {
                let (stage_settings, stage_module) =
                    explicit_install_target(settings, module, &retained.stage)?;
                let staged = probe_module_install_state(
                    &stage_settings,
                    &stage_module.summary.id,
                    stage_module.summary.steam_app_id,
                    stage_module.install.as_ref(),
                    stage_module.process.as_ref(),
                );
                if staged.install_state != InstallState::Installed {
                    return Err(SteamCmdError::InstallationVerificationFailed {
                        module_id: module.summary.id.clone(),
                        operation: operation.clone(),
                        detail: staged.diagnostics.join(" "),
                    });
                }
                for name in [
                    ".langame-initial-package.json",
                    ".langame-clean-package.json",
                ] {
                    let path = retained.stage.join(name);
                    match fs::symlink_metadata(&path) {
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(source) => return Err(SteamCmdError::CreatePath { path, source }),
                        Ok(_) => {
                            return Err(SteamCmdError::InstallationVerificationFailed {
                                module_id: module.summary.id.clone(),
                                operation: operation.clone(),
                                detail:
                                    "Fresh Steam payload contains manager-owned package inventory."
                                        .into(),
                            });
                        }
                    }
                }
                on_progress(InstallProgressUpdate::stage(
                    InstallPhase::Verifying,
                    "Recording the original server package inventory...",
                ));
                deadline.check_cancelled()?;
                prepare_fresh_payload(retained.stage.clone()).await?;
                deadline.check_cancelled()?;
                retained.publish(deadline).await?;
            }

            on_progress(InstallProgressUpdate::stage(
                InstallPhase::Verifying,
                "Verifying installed server files...",
            ));
            let after = probe_module_install_state(
                settings,
                &module.summary.id,
                module.summary.steam_app_id,
                Some(install),
                Some(process),
            );
            if after.install_state != InstallState::Installed {
                return Err(SteamCmdError::InstallationVerificationFailed {
                    module_id: module.summary.id.clone(),
                    operation: operation.clone(),
                    detail: after.diagnostics.join(" "),
                });
            }
            deadline.check_cancelled()?;
            Ok((after, excerpt))
        }
        .await;
        if let Err(error) = &install_result
            && !matches!(error, SteamCmdError::InstallProcessCleanupFailed { .. })
            && let Some(retained) = retained.as_ref()
        {
            retained.recover().await?;
        }
        let (after, excerpt) = install_result?;
        if let Some(retained) = retained {
            retained.commit();
        }

        on_progress(
            InstallProgressUpdate::stage(
                InstallPhase::Ready,
                format!(
                    "{} complete. Executable ready at {}",
                    capitalize_operation(&operation),
                    after.executable_path
                ),
            )
            .with_output(excerpt.clone()),
        );

        Ok(ModuleInstallResult {
            module_id: module.summary.id.clone(),
            steam_app_id,
            operation,
            install_root: after.install_root,
            executable_path: after.executable_path,
            executable_exists: after.executable_exists,
            install_state: after.install_state,
            current_version: after.current_version,
            output_excerpt: excerpt,
        })
    }
    .await;
    if install_result.is_ok() {
        complete_game_install_revision(Path::new(&settings.servers_root), revision_key, revision)?;
    }
    install_result
}

fn with_retry_context(mut update: InstallProgressUpdate, retry: usize) -> InstallProgressUpdate {
    // Keep the real attempt visible after the brief backoff message has passed.
    // This context belongs to the operation, not to either native output stream.
    if retry > 0 {
        update.detail = format!("Retry {retry}: {}", update.detail);
    }
    update
}

#[cfg(test)]
pub(super) fn steamcmd_install_script_lines(
    module: &ModuleDetails,
    steam_app_id: u32,
    install_root: &Path,
    validate: bool,
) -> Vec<String> {
    steamcmd_install_script_lines_with_login(module, steam_app_id, install_root, validate, None)
}

pub(super) fn steamcmd_install_script_lines_with_login(
    module: &ModuleDetails,
    steam_app_id: u32,
    install_root: &Path,
    validate: bool,
    account_name: Option<&str>,
) -> Vec<String> {
    let mut script_lines = vec![
        String::from("@ShutdownOnFailedCommand 1"),
        String::from("@NoPromptForPassword 1"),
    ];

    if module
        .summary
        .supported_platforms
        .iter()
        .any(|platform| platform.eq_ignore_ascii_case("windows"))
    {
        script_lines.push(String::from("@sSteamCmdForcePlatformType windows"));
        script_lines.push(String::from("@sSteamCmdForcePlatformBitness 64"));
    }

    script_lines.push(format!(
        "force_install_dir {}",
        steamcmd_script_path(install_root)
    ));
    script_lines.push(super::steamcmd_account::workshop_login_line(account_name));

    let mut app_update_line = format!("app_update {steam_app_id}");
    if validate {
        app_update_line.push_str(" validate");
    }
    script_lines.push(app_update_line);
    script_lines.push(String::from("quit"));
    script_lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_context_survives_connection_and_download_reports_without_changing_measurements() {
        let waiting =
            InstallProgressUpdate::stage(InstallPhase::Preparing, "Waiting for client config...");
        assert_eq!(
            with_retry_context(waiting.clone(), 0).detail,
            waiting.detail
        );
        assert_eq!(
            with_retry_context(waiting, 1).detail,
            "Retry 1: Waiting for client config..."
        );
        let download = InstallProgressUpdate::download("Downloading server files", 25, Some(100))
            .with_output("current native output");
        let retried = with_retry_context(download.clone(), 2);
        assert_eq!(retried.detail, "Retry 2: Downloading server files");
        assert_eq!(retried.install_progress, download.install_progress);
        assert_eq!(retried.output_excerpt, download.output_excerpt);
    }
}

#[cfg(all(test, windows))]
#[path = "steam_install_retained_tests.rs"]
mod retained_tests;

#[cfg(test)]
#[path = "steamcmd_dependency_tests.rs"]
mod dependency_tests;
