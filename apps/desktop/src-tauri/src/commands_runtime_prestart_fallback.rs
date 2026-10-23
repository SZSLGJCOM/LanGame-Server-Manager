//! Failed source checks can reuse verified bytes; installation uncertainty,
//! cancellation, ownership, metadata and filesystem errors never authorize it.
use super::*;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

pub(super) fn permits_installed_program(error: &app_steamcmd::SteamCmdError) -> bool {
    use app_steamcmd::SteamCmdError as Error;
    matches!(
        error,
        Error::HttpDownload { .. }
            | Error::HttpDownloadInterrupted { .. }
            | Error::OperationTimedOut { .. }
            | Error::PrepareSteamCmd { .. }
            | Error::SteamCmdPreparationStalled { .. }
            | Error::SteamCmdPreparationTimedOut { .. }
            | Error::SteamCmdCommandFailed { .. }
            | Error::MissingSteamCmdExecutable { .. }
            | Error::SteamCmdNotReady { .. }
    )
}

pub(super) struct InstalledProgramFallback<'a> {
    pub storage: &'a StorageBootstrap,
    pub descriptor: &'a ModuleDescriptor,
    pub module: &'a ModuleDetails,
    pub instance: &'a InstanceDetails,
    pub reservation: &'a RuntimeStartReservationLease,
    pub cancellation: &'a app_steamcmd::InstallCancellation,
    pub context: &'a PrestartUpdateLogContext<'a>,
}

struct CancelVerificationOnDrop(Arc<AtomicBool>);

impl Drop for CancelVerificationOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

impl InstalledProgramFallback<'_> {
    pub(super) async fn recover(
        &self,
        guard: app_steamcmd::GameInstallLifecycleGuard,
        error: &app_steamcmd::SteamCmdError,
    ) -> Result<(PrestartUpdateOutcome, String), String> {
        let original = steamcmd_error_message(error);
        report(
            self.storage,
            self.instance,
            self.context,
            "failed",
            &original,
        );
        let guard = self
            .verify(guard)
            .await
            .map_err(|reason| format!("{original}\n现有服务器程序无法安全复用：{reason}"))?;
        let message = String::from(
            "游戏更新检查未完成；已核验现有服务器程序完整，使用已有版本继续启动，尚未确认是否为最新版。",
        );
        let warning = app_core::RuntimeStartWarning {
            code: app_core::RuntimeStartWarningCode::ProgramUpdateUsingInstalled,
            provider: None,
            reason: None,
            item_ids: Vec::new(),
            message: message.clone(),
            log_path: self.context.console_log_path.map(str::to_owned),
            map_key: None,
            http_status: None,
        };
        Ok((
            PrestartUpdateOutcome {
                install_guard: guard,
                warnings: vec![warning],
            },
            message,
        ))
    }

    async fn verify(
        &self,
        guard: app_steamcmd::GameInstallLifecycleGuard,
    ) -> Result<app_steamcmd::GameInstallLifecycleGuard, String> {
        if self.cancellation.is_cancelled() || self.reservation.is_cancelled() {
            return Err("启动前更新已取消，实例未启动。".into());
        }
        let instance = self.instance;
        let root = PathBuf::from(
            super::super::commands_runtime_lifecycle::private_runtime_install_root(instance)?,
        );
        guard
            .ensure_scope(&instance.summary.module_id, &root)
            .map_err(|error| steamcmd_error_message(&error))?;
        let uses_library = app_storage::instance_uses_library_program(
            super::super::commands_program_storage::instance_root(instance)?,
        )
        .map_err(|error| error.to_string())?;
        let binding =
            app_storage::read_instance_program_install(&self.storage.paths, &instance.summary.id)
                .await
                .map_err(|error| error.to_string())?
                .ok_or("instance program ownership is missing")?;
        let ownership_matches = if uses_library {
            binding.install.scope == app_storage::ProgramInstallScope::Library
                && binding.install.owner_instance_id.is_none()
        } else {
            binding.install.scope == app_storage::ProgramInstallScope::Instance
                && binding.install.owner_instance_id.as_deref()
                    == Some(instance.summary.id.as_str())
        };
        if !ownership_matches
            || binding.instance_id != instance.summary.id
            || binding.install.module_id != instance.summary.module_id
            || binding.install.install_state != InstallState::Installed
            || fs::canonicalize(&binding.install.install_root).map_err(|error| error.to_string())?
                != fs::canonicalize(&root).map_err(|error| error.to_string())?
        {
            return Err("instance program ownership does not match its installed runtime".into());
        }
        let settings = self.storage.settings.clone();
        let descriptor = self.descriptor.clone();
        let module = self.module.clone();
        let signal = CancelVerificationOnDrop(Arc::new(AtomicBool::new(false)));
        let worker_signal = signal.0.clone();
        // Keep the program and storage leases until all bounded file reads end,
        // including cancellation of the caller or the visible background job.
        let mut worker = spawn_blocking_storage_context_task(self.reservation, move || {
            app_steamcmd::read_program_install_revision(
                Path::new(&settings.servers_root),
                &module.summary.id,
                &root,
            )
            .map_err(|error| steamcmd_error_message(&error))?;
            let probe = app_steamcmd::probe_module_install_state_with_override(
                &settings,
                &module.summary.id,
                module.summary.steam_app_id,
                module.install.as_ref(),
                module.process.as_ref(),
                Some(&root.to_string_lossy()),
            );
            if probe.install_state != InstallState::Installed || !probe.executable_exists {
                return Err(format!("服务器程序不完整：{}", probe.diagnostics.join(" ")));
            }
            if !app_storage::library_program_is_pristine(&root, &descriptor, Some(&worker_signal))
                .map_err(|error| error.to_string())?
            {
                return Err("缺少可信程序清单或文件哈希不匹配，请先校验或修复服务器程序。".into());
            }
            Ok(guard)
        });
        let result = tokio::select! {
            biased;
            _ = self.cancellation.cancelled() => None,
            _ = self.reservation.cancelled() => None,
            result = &mut worker => Some(result),
        };
        let result =
            match result {
                Some(result) => result,
                None => {
                    signal.0.store(true, Ordering::Release);
                    drop(worker.await.map_err(|error| {
                        format!("program verification worker failed: {error}")
                    })??);
                    return Err("启动前更新已取消，实例未启动。".into());
                }
            }
            .map_err(|error| format!("program verification worker failed: {error}"))?;
        if self.cancellation.is_cancelled() || self.reservation.is_cancelled() {
            return Err("启动前更新已取消，实例未启动。".into());
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_steamcmd::SteamCmdError as Error;

    #[test]
    fn cancellation_ownership_permissions_and_uncertain_cleanup_never_allow_installed_fallback() {
        for error in [
            Error::InstallCancelled {
                operation: "prestart check".into(),
            },
            Error::InstallProcessCleanupFailed {
                operation: "native installer".into(),
                detail: "process still running".into(),
            },
            Error::SteamCmdPreparationCleanupFailed {
                output_excerpt: "unreaped process".into(),
            },
            Error::PackageRevisionPending {
                path: PathBuf::from("pending"),
            },
            Error::PackageRevisionIo {
                path: PathBuf::from("inert-revision"),
                source: std::io::ErrorKind::PermissionDenied.into(),
            },
            Error::InvalidSteamCmdOwnership {
                path: PathBuf::from("inert-steamcmd"),
            },
            Error::WriteMinecraftServerFile {
                path: PathBuf::from("inert-server.jar"),
                source: std::io::ErrorKind::PermissionDenied.into(),
            },
            Error::InstallationVerificationFailed {
                module_id: "minecraft".into(),
                operation: "verification".into(),
                detail: "hash mismatch".into(),
            },
            Error::SteamAppAccountRequired {
                app_id: 896660,
                output_excerpt: "access required".into(),
            },
        ] {
            assert!(!permits_installed_program(&error), "{error}");
        }
    }
}
