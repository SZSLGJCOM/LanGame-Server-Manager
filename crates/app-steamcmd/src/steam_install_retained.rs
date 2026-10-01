use super::install_publication_metadata::InstallPublicationMetadata;
use super::install_transaction::{prepared_install_publish_script, recover_install_publication};
use super::*;

/// SteamCMD must never write into an uninstalled tree that holds user data.
/// Only acquisition differs from ZIP installs; publication and recovery share
/// the same implementation, keeping the original tree until verification.
pub(super) struct RetainedSteamInstall {
    root: PathBuf,
    pub(super) stage: PathBuf,
    rollback: PathBuf,
    rejected: PathBuf,
    phase: PathBuf,
    verification: PathBuf,
    metadata: InstallPublicationMetadata,
    published: bool,
}

impl RetainedSteamInstall {
    pub(super) fn prepare(
        root: &Path,
        module: &ModuleDetails,
        before: InstallState,
    ) -> Result<Option<Self>, SteamCmdError> {
        if !has_retained_install_data(root) {
            return Ok(None);
        }
        if before != InstallState::NotInstalled {
            return Err(SteamCmdError::InstallationVerificationFailed {
                module_id: module.summary.id.clone(),
                operation: "install".into(),
                detail: "Retained installation data conflicts with the required server payload."
                    .into(),
            });
        }
        // Validate ancestors before giving the native installer a destination.
        // The shared publisher validates all source and staging entries again.
        for ancestor in root.ancestors().collect::<Vec<_>>().into_iter().rev() {
            if ancestor.as_os_str().is_empty() {
                continue;
            }
            let metadata =
                fs::symlink_metadata(ancestor).map_err(|source| SteamCmdError::CreatePath {
                    path: ancestor.to_path_buf(),
                    source,
                })?;
            #[cfg(windows)]
            let linked = {
                use std::os::windows::fs::MetadataExt;
                metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
            };
            #[cfg(not(windows))]
            let linked = metadata.file_type().is_symlink();
            if linked || !metadata.is_dir() {
                return Err(SteamCmdError::CreatePath {
                    path: ancestor.to_path_buf(),
                    source: std::io::Error::other(
                        "Retained installation contains a linked or non-directory ancestor.",
                    ),
                });
            }
        }
        let verification = module
            .install
            .as_ref()
            .and_then(|install| install.verification_path.as_deref())
            .or_else(|| {
                module
                    .process
                    .as_ref()
                    .map(|process| process.executable.as_str())
            })
            .and_then(safe_install_relative_path)
            .ok_or_else(|| SteamCmdError::InvalidInstallRelativePath {
                module_id: module.summary.id.clone(),
                path: "retained installation verification path".into(),
            })?;
        let parent = root.parent().ok_or_else(|| SteamCmdError::CreatePath {
            path: root.to_path_buf(),
            source: std::io::Error::other("Installation root has no parent."),
        })?;
        let name = root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("server");
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let stage = parent.join(format!(".{name}.stage-{}-{stamp}", std::process::id()));
        let metadata = InstallPublicationMetadata::read(root, &module.summary.id, true)?;
        fs::create_dir(&stage).map_err(|source| SteamCmdError::CreatePath {
            path: stage.clone(),
            source,
        })?;
        Ok(Some(Self {
            root: root.to_path_buf(),
            verification: stage.join(verification),
            metadata,
            stage,
            rollback: parent.join(format!(".{name}.rollback-{}-{stamp}", std::process::id())),
            rejected: parent.join(format!(".{name}.rejected-{}-{stamp}", std::process::id())),
            phase: parent.join(format!(
                ".{name}.publish-{}-{stamp}.state",
                std::process::id()
            )),
            published: false,
        }))
    }

    pub(super) async fn publish(&mut self, deadline: InstallDeadline) -> Result<(), SteamCmdError> {
        if !has_retained_install_data(&self.root) {
            return Err(SteamCmdError::InstallRollbackFailed {
                path: self.root.clone(),
                detail: "Retained installation data marker changed before publication.".into(),
            });
        }
        let script = prepared_install_publish_script(
            &self.root,
            &self.stage,
            &self.rollback,
            &self.phase,
            &self.verification,
            &self.metadata,
        );
        let output = run_powershell(&script, self.root.parent(), deadline).await?;
        if !output.status.success() {
            return Err(SteamCmdError::SteamCmdCommandFailed {
                output_excerpt: output_excerpt(&output.stdout, &output.stderr),
            });
        }
        self.published = true;
        deadline.check_cancelled()
    }

    pub(super) async fn recover(&self) -> Result<(), SteamCmdError> {
        recover_install_publication(
            &self.root,
            &self.stage,
            &self.rollback,
            &self.rejected,
            &self.phase,
            self.published,
        )
        .await
    }

    pub(super) fn commit(self) {
        schedule_verified_directory_cleanup(vec![self.rollback, self.stage]);
        let _ = fs::remove_file(self.phase);
    }
}
