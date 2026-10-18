//! Only update failures may use an independently verified old package. Normal
//! startup configuration, ownership, context, cleanup and permission errors remain fatal.
use super::*;

pub(super) struct UpdateFailure {
    pub(super) message: String,
    pub(super) fallback_reason: Option<app_core::RuntimeModUpdateReason>,
    pub(super) recovery_safe: bool,
}

impl From<String> for UpdateFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            fallback_reason: None,
            recovery_safe: true,
        }
    }
}

pub(super) fn deployment_failure(message: String) -> UpdateFailure {
    UpdateFailure {
        message,
        fallback_reason: Some(app_core::RuntimeModUpdateReason::Install),
        recovery_safe: true,
    }
}

pub(super) fn staging_failure(
    error: super::super::commands_mod_staging::WorkshopModStageFailure,
) -> UpdateFailure {
    if error.guard_failed {
        error.message.into()
    } else {
        deployment_failure(error.message)
    }
}

pub(super) fn download_failure(error: app_steamcmd::SteamCmdError) -> UpdateFailure {
    use app_core::RuntimeModUpdateReason as Reason;
    use app_steamcmd::SteamCmdError as Error;
    let reason = match &error {
        Error::WorkshopAccountRequired { .. } | Error::SteamAccountAuthenticationFailed { .. } => {
            Some(Reason::Authentication)
        }
        Error::WorkshopDecryptionKeyMissing { .. } => Some(Reason::AccessDenied),
        Error::HttpDownload { .. } | Error::HttpDownloadInterrupted { .. } => Some(Reason::Network),
        Error::SteamCmdCommandFailed { .. } => Some(Reason::Download),
        Error::PrepareSteamCmd { .. }
        | Error::SteamCmdPreparationStalled { .. }
        | Error::SteamCmdPreparationTimedOut { .. }
        | Error::OperationTimedOut { .. } => Some(Reason::Unknown),
        Error::WorkshopInventoryRead { source, .. }
            if source.kind() != std::io::ErrorKind::PermissionDenied =>
        {
            Some(Reason::Install)
        }
        _ => None,
    };
    UpdateFailure {
        message: steamcmd_error_message(&error),
        fallback_reason: reason,
        recovery_safe: !matches!(
            error,
            Error::InstallProcessCleanupFailed { .. }
                | Error::SteamCmdPreparationCleanupFailed { .. }
        ),
    }
}

pub(super) fn cache_warning(
    ids: &[String],
    reason: app_core::RuntimeModUpdateReason,
    log_path: Option<&str>,
) -> app_core::RuntimeStartWarning {
    app_core::RuntimeStartWarning {
        code: app_core::RuntimeStartWarningCode::ModUpdateUsingCache,
        provider: app_core::RuntimeModProvider::SteamWorkshop,
        reason,
        item_ids: ids.to_vec(),
        message: "创意工坊模组更新未完成；已恢复并校验完整缓存，使用已有版本继续启动，尚未确认是否为最新版。".into(),
        log_path: log_path.map(str::to_owned),
        map_key: None,
        http_status: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_steamcmd::SteamCmdError as Error;

    #[test]
    fn deployment_guard_failures_never_authorize_cached_fallback_even_when_incoming_matches_old_bytes()
     {
        let guarded = staging_failure(
            super::super::super::commands_mod_staging::WorkshopModStageFailure {
                message: "controlled moved-original change; incoming equals original baseline"
                    .into(),
                guard_failed: true,
            },
        );
        assert!(guarded.fallback_reason.is_none());
        assert!(
            guarded.recovery_safe,
            "cache recovery is allowed after the native child exited, target publication is not"
        );
        let ordinary = staging_failure(
            super::super::super::commands_mod_staging::WorkshopModStageFailure {
                message: "controlled ordinary stage-copy failure".into(),
                guard_failed: false,
            },
        );
        assert_eq!(
            ordinary.fallback_reason,
            Some(app_core::RuntimeModUpdateReason::Install)
        );
    }

    #[test]
    fn cancellation_cleanup_ownership_program_and_permission_errors_do_not_use_cache() {
        for error in [
            Error::InstallCancelled {
                operation: "Workshop update".into(),
            },
            Error::InstallProcessCleanupFailed {
                operation: "SteamCMD".into(),
                detail: "unreaped child".into(),
            },
            Error::MissingInstalledExecutable {
                module_id: "palworld".into(),
                operation: "program validation".into(),
                path: "inert-fixture".into(),
            },
            Error::InvalidSteamCmdOwnership {
                path: PathBuf::from("inert-fixture"),
            },
            Error::WorkshopInventoryRead {
                path: PathBuf::from("inert-fixture"),
                source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
            },
        ] {
            assert!(download_failure(error).fallback_reason.is_none());
        }
        assert!(
            UpdateFailure::from("storage context changed".to_owned())
                .fallback_reason
                .is_none()
        );
    }

    #[test]
    fn warning_contains_only_closed_classification_and_never_native_error_details() {
        let failure = download_failure(Error::SteamCmdCommandFailed {
            output_excerpt: "https://private.invalid/?token=secret".into(),
        });
        let warning = cache_warning(&["123456".into()], failure.fallback_reason.unwrap(), None);
        assert!(!serde_json::to_string(&warning).unwrap().contains("secret"));
        assert_eq!(warning.reason, app_core::RuntimeModUpdateReason::Download);
    }
}
