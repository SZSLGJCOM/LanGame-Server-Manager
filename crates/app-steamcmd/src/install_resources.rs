use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use super::install_operation::{InstallCoordinator, InstallOperationGuard, LockMode};
use super::{InstallDeadline, SteamCmdError};

pub(crate) struct ResourceLocks {
    roots: Vec<PathBuf>,
    _locks: Vec<InstallOperationGuard>,
}

impl ResourceLocks {
    pub(crate) async fn acquire(
        module_ids: &[&str],
        roots: &[PathBuf],
        deadline: InstallDeadline,
    ) -> Result<Self, SteamCmdError> {
        if roots.is_empty() {
            return Err(scope_error(
                Path::new(""),
                "a lifecycle lease requires a resource root",
            ));
        }
        let mut resources = BTreeMap::new();
        for module_id in module_ids {
            if module_id.is_empty()
                || !module_id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
            {
                return Err(scope_error(
                    Path::new(module_id),
                    "invalid lifecycle module ID",
                ));
            }
            resources.insert(format!("module\0{module_id}"), LockMode::Exclusive);
        }
        let mut normalized = roots
            .iter()
            .map(|root| canonical_resource_path(root))
            .collect::<Result<Vec<_>, _>>()?;
        normalized.sort();
        normalized.dedup();
        // An exclusive parent already covers nested targets. Collapse these
        // before acquiring locks, so a lease cannot conflict with itself.
        let roots: Vec<PathBuf> = normalized
            .iter()
            .filter(|root| {
                !normalized
                    .iter()
                    .any(|parent| parent != *root && root.starts_with(parent))
            })
            .cloned()
            .collect();
        for root in &roots {
            for ancestor in root.ancestors() {
                let mode = if ancestor == root {
                    LockMode::Exclusive
                } else {
                    LockMode::Shared
                };
                resources
                    .entry(format!("path\0{}", ancestor.to_string_lossy()))
                    .and_modify(|current| *current = (*current).max(mode))
                    .or_insert(mode);
            }
        }
        let mut locks = Vec::with_capacity(resources.len());
        // Stable ordering is shared across processes. Siblings share ancestor
        // intents; mutating a parent conflicts with its entire subtree.
        for (key, mode) in resources {
            let coordinator = InstallCoordinator::for_resource(&key)
                .map_err(|error| super::install_acquire_error(error, deadline))?;
            locks.push(
                coordinator
                    .acquire_mode(mode, deadline)
                    .await
                    .map_err(|error| super::install_acquire_error(error, deadline))?,
            );
        }
        Ok(Self {
            roots,
            _locks: locks,
        })
    }

    pub(crate) fn ensure_root(&self, root: &Path) -> Result<(), SteamCmdError> {
        let actual = canonical_resource_path(root)?;
        if self.roots.iter().any(|held| actual.starts_with(held)) {
            Ok(())
        } else {
            Err(scope_error(
                root,
                "lifecycle lease does not cover this resource root",
            ))
        }
    }

    pub(crate) fn ensure_disjoint(&self, root: &Path) -> Result<(), SteamCmdError> {
        let actual = canonical_resource_path(root)?;
        if self
            .roots
            .iter()
            .any(|held| actual.starts_with(held) || held.starts_with(&actual))
        {
            Err(scope_error(
                root,
                "SteamCMD and game resource directories overlap; choose separate, non-nested SteamCMD and server directories",
            ))
        } else {
            Ok(())
        }
    }
}

pub(crate) fn scope_error(path: &Path, message: &'static str) -> SteamCmdError {
    SteamCmdError::InstallOperationLock {
        action: "validate installation lifecycle scope",
        path: path.to_owned(),
        source: io::Error::new(io::ErrorKind::InvalidInput, message),
    }
}

/// Resolve an existing ancestor and append the missing tail without creating
/// files. Identity stays stable before/after installation and resolves aliases.
pub(crate) fn canonical_resource_path(root: &Path) -> Result<PathBuf, SteamCmdError> {
    if !root.is_absolute()
        || root
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(scope_error(
            root,
            "resource root must be absolute without traversal",
        ));
    }
    #[cfg(windows)]
    for component in root.components() {
        if let Component::Normal(name) = component {
            let name = name.to_string_lossy();
            if name.ends_with(['.', ' '])
                || name
                    .chars()
                    .any(|ch| ch.is_control() || matches!(ch, ':' | '<' | '>' | '|' | '?' | '*'))
            {
                return Err(scope_error(root, "ambiguous Windows resource path"));
            }
        }
    }
    let mut ancestor = root.to_owned();
    let mut missing = Vec::new();
    let mut normalized = loop {
        match fs::canonicalize(&ancestor) {
            Ok(path) => {
                if !path.is_dir() {
                    return Err(scope_error(root, "resource root points to a file"));
                }
                break path;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let name = ancestor
                    .file_name()
                    .ok_or_else(|| scope_error(root, "resource root has no existing ancestor"))?
                    .to_owned();
                missing.push(name);
                ancestor.pop();
            }
            Err(source) => {
                return Err(SteamCmdError::InstallOperationLock {
                    action: "resolve installation resource path",
                    path: ancestor,
                    source,
                });
            }
        }
    };
    for name in missing.into_iter().rev() {
        normalized.push(name);
    }
    let text = normalized
        .to_str()
        .ok_or_else(|| scope_error(root, "resource root is not UTF-8"))?;
    #[cfg(windows)]
    {
        Ok(PathBuf::from(text.to_lowercase()))
    }
    #[cfg(not(windows))]
    {
        let _ = text;
        Ok(normalized)
    }
}

#[cfg(test)]
#[path = "install_resources_tests.rs"]
mod tests;
