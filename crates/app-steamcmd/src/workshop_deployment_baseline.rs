//! Detect changes to every actual loading root before replacement, including
//! initially missing or repairable damaged deployments. Hashes do not grant
//! ownership over additional operator files.
use std::io;
use std::path::{Path, PathBuf};

use crate::workshop_cache_integrity::{self as integrity, Files};
use crate::{InstallCancellation, steam_depot};

#[derive(Debug, Clone)]
pub struct WorkshopDeploymentBaseline {
    id: String,
    path: PathBuf,
    files: Option<Files>,
}

fn current(path: &Path, cancellation: &InstallCancellation) -> io::Result<Option<Files>> {
    match integrity::inventory_allow_empty(path, cancellation) {
        Ok(files) => Ok(Some(files)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            // An absent loading root cannot conceal a linked existing ancestor.
            for ancestor in path.ancestors().skip(1) {
                match std::fs::symlink_metadata(ancestor) {
                    Ok(_) => {
                        steam_depot::check_plain(ancestor, true)?;
                        return Ok(None);
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
            Err(steam_depot::invalid(
                "Workshop deployment has no plain ancestor",
            ))
        }
        Err(error) => Err(error),
    }
}

impl WorkshopDeploymentBaseline {
    pub fn capture(id: &str, path: &Path, cancellation: &InstallCancellation) -> io::Result<Self> {
        if !(6..=20).contains(&id.len())
            || path.file_name().and_then(|name| name.to_str()) != Some(id)
            || crate::normalize_steam_workshop_item_ids(&[id.into()]) != [id]
        {
            return Err(steam_depot::invalid(
                "invalid Workshop loading-root identity",
            ));
        }
        Ok(Self {
            id: id.into(),
            path: path.into(),
            files: current(path, cancellation)?,
        })
    }

    pub fn item_id(&self) -> &str {
        &self.id
    }

    pub fn verify(&self, path: &Path, cancellation: &InstallCancellation) -> io::Result<()> {
        if self.path != path || current(path, cancellation)? != self.files {
            return Err(steam_depot::invalid(
                "Workshop actual deployment changed during the update; replacement refused to preserve operator data",
            ));
        }
        Ok(())
    }

    /// Compare a moved original in the caller's private transaction. This does
    /// not advance the loading baseline or authorize continuing with the cache.
    pub fn verify_moved_original(
        &self,
        path: &Path,
        cancellation: &InstallCancellation,
    ) -> io::Result<()> {
        if path.file_name().and_then(|name| name.to_str()) != Some(self.id.as_str())
            || current(path, cancellation)? != self.files
        {
            return Err(steam_depot::invalid(
                "original Workshop loading root changed before publication; preserved transaction data must not be deleted",
            ));
        }
        Ok(())
    }

    /// Only a confirmed publication of the exact source tree advances the
    /// baseline. A new user file cannot be accepted as our own publication.
    pub fn accept_publication(
        &mut self,
        source: &Path,
        path: &Path,
        cancellation: &InstallCancellation,
    ) -> io::Result<()> {
        if self.path != path
            || source.file_name().and_then(|name| name.to_str()) != Some(self.id.as_str())
        {
            return Err(steam_depot::invalid(
                "unexpected Workshop publication identity",
            ));
        }
        let expected = integrity::inventory(source, cancellation)?;
        if current(path, cancellation)?.as_ref() != Some(&expected) {
            return Err(steam_depot::invalid(
                "published Workshop root differs from its source; operator data and publication state were not confirmed",
            ));
        }
        self.files = Some(expected);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_loading_root_detects_late_data_and_only_exact_owned_publication_advances() {
        let root = crate::tests::unique_test_root();
        let target = root.join("actual/123456");
        let source = root.join("source/123456");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        let cancel = InstallCancellation::new();
        let missing = WorkshopDeploymentBaseline::capture("123456", &target, &cancel).unwrap();
        std::fs::create_dir_all(target.join("Config")).unwrap();
        std::fs::write(target.join("Config/user.ini"), "keep new operator data").unwrap();
        assert!(missing.verify(&target, &cancel).is_err());
        std::fs::remove_dir_all(&target).unwrap();
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("old.pak"), "repairable damaged bytes").unwrap();
        let mut baseline = WorkshopDeploymentBaseline::capture("123456", &target, &cancel).unwrap();
        baseline.verify(&target, &cancel).unwrap();
        std::fs::write(source.join("new.pak"), "published package").unwrap();
        std::fs::remove_file(target.join("old.pak")).unwrap();
        std::fs::write(target.join("new.pak"), "published package").unwrap();
        baseline
            .accept_publication(&source, &target, &cancel)
            .unwrap();
        baseline.verify(&target, &cancel).unwrap();
        std::fs::create_dir(target.join("Config")).unwrap();
        std::fs::write(target.join("Config/user.ini"), "keep late operator data").unwrap();
        assert!(baseline.verify(&target, &cancel).is_err());
        assert!(
            baseline
                .accept_publication(&source, &target, &cancel)
                .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(target.join("Config/user.ini")).unwrap(),
            "keep late operator data"
        );
        assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
        std::fs::remove_dir_all(root).unwrap();
    }
}
