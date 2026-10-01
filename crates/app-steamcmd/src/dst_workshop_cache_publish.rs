use std::fs;
use std::path::{Path, PathBuf};

#[cfg(test)]
use std::sync::Mutex;

use crate::dst_workshop_cache::DstWorkshopCacheError;
use crate::dst_workshop_cache_fs::{inspect_tree, require_plain_file};

pub(crate) struct PublishStage<'a> {
    pub(crate) stage: &'a Path,
    pub(crate) staged_content: &'a Path,
    pub(crate) staged_manifest: &'a Path,
    pub(crate) content_root: &'a Path,
    pub(crate) target_manifest: &'a Path,
    pub(crate) item_ids: Vec<&'a str>,
    pub(crate) manifest_unchanged: bool,
}

struct PublishedItem {
    target: PathBuf,
    backup: Option<PathBuf>,
}

impl PublishStage<'_> {
    pub(crate) fn publish(self) -> Result<(), DstWorkshopCacheError> {
        self.prevalidate()?;
        let backups = self.stage.join("backups");
        fs::create_dir(&backups)
            .map_err(|source| io("create rollback directory", &backups, source))?;

        let mut published = Vec::with_capacity(self.item_ids.len());
        for item_id in &self.item_ids {
            let target = self.content_root.join(item_id);
            let backup = backups.join(item_id);
            let had_target = target.exists();
            if had_target && let Err(source) = stage_old_item(item_id, &target, &backup) {
                return abort_publish(
                    self.stage,
                    &mut published,
                    io("stage old item", &target, source),
                    Vec::new(),
                );
            }

            let staged = self.staged_content.join(item_id);
            if let Err(source) = fs::rename(&staged, &target) {
                let mut rollback_failures = Vec::new();
                if had_target {
                    restore_backup(&backup, &target, &mut rollback_failures);
                }
                return abort_publish(
                    self.stage,
                    &mut published,
                    io("publish item", &target, source),
                    rollback_failures,
                );
            }
            published.push(PublishedItem {
                target,
                backup: had_target.then_some(backup),
            });
        }

        if self.manifest_unchanged {
            return Ok(());
        }

        let manifest_backup = self.stage.join("manifest.backup");
        let had_manifest = self.target_manifest.exists();
        if had_manifest && let Err(source) = fs::rename(self.target_manifest, &manifest_backup) {
            return abort_publish(
                self.stage,
                &mut published,
                io("stage old manifest", self.target_manifest, source),
                Vec::new(),
            );
        }
        if let Err(source) = fs::rename(self.staged_manifest, self.target_manifest) {
            let mut rollback_failures = Vec::new();
            if had_manifest {
                restore_backup(
                    &manifest_backup,
                    self.target_manifest,
                    &mut rollback_failures,
                );
            }
            return abort_publish(
                self.stage,
                &mut published,
                io("publish manifest", self.target_manifest, source),
                rollback_failures,
            );
        }

        Ok(())
    }

    fn prevalidate(&self) -> Result<(), DstWorkshopCacheError> {
        for item_id in &self.item_ids {
            inspect_tree(&self.staged_content.join(item_id), 0)?;
            let target = self.content_root.join(item_id);
            if target.exists() {
                inspect_tree(&target, 0)?;
            }
        }
        if !self.manifest_unchanged {
            require_plain_file(self.staged_manifest)?;
            if self.target_manifest.exists() {
                require_plain_file(self.target_manifest)?;
            }
        }
        Ok(())
    }
}

fn abort_publish(
    stage: &Path,
    published: &mut Vec<PublishedItem>,
    original: DstWorkshopCacheError,
    mut rollback_failures: Vec<String>,
) -> Result<(), DstWorkshopCacheError> {
    rollback_published(published, &mut rollback_failures);
    if rollback_failures.is_empty() {
        return Err(original);
    }
    Err(DstWorkshopCacheError::RollbackFailed {
        stage: stage.to_path_buf(),
        message: format!("{original}; {}", rollback_failures.join("; ")),
    })
}

fn rollback_published(published: &mut Vec<PublishedItem>, failures: &mut Vec<String>) {
    while let Some(item) = published.pop() {
        let replacement_removed = if item.target.exists() {
            match fs::remove_dir_all(&item.target) {
                Ok(()) => true,
                Err(error) => {
                    failures.push(format!(
                        "could not remove replacement {}: {error}",
                        item.target.display()
                    ));
                    false
                }
            }
        } else {
            true
        };
        if replacement_removed && let Some(backup) = item.backup {
            restore_backup(&backup, &item.target, failures);
        }
    }
}

fn restore_backup(backup: &Path, target: &Path, failures: &mut Vec<String>) {
    if !backup.exists() {
        failures.push(format!("rollback data is missing for {}", target.display()));
        return;
    }
    if let Err(error) = fs::rename(backup, target) {
        failures.push(format!("could not restore {}: {error}", target.display()));
    }
}

fn io(operation: &'static str, path: &Path, source: std::io::Error) -> DstWorkshopCacheError {
    DstWorkshopCacheError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
static FAIL_ITEM_BACKUP: Mutex<Option<String>> = Mutex::new(None);

#[cfg(test)]
pub(crate) fn fail_item_backup_for_test(item_id: &str) {
    *FAIL_ITEM_BACKUP.lock().unwrap() = Some(item_id.to_owned());
}

#[cfg(test)]
fn stage_old_item(item_id: &str, target: &Path, backup: &Path) -> std::io::Result<()> {
    let mut requested = FAIL_ITEM_BACKUP.lock().unwrap();
    if requested.as_deref() == Some(item_id) {
        *requested = None;
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "controlled test conflict while staging old item",
        ));
    }
    drop(requested);
    fs::rename(target, backup)
}

#[cfg(not(test))]
fn stage_old_item(_item_id: &str, target: &Path, backup: &Path) -> std::io::Result<()> {
    fs::rename(target, backup)
}
