use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};

use app_modules::ModuleDescriptor;

pub(super) fn from_env(descriptors: &[ModuleDescriptor]) -> Result<Vec<(String, PathBuf)>, String> {
    parse(
        descriptors,
        std::env::var_os("LANGAME_NATIVE_MODULE_ID"),
        std::env::var_os("LANGAME_NATIVE_PACKAGE_ROOT"),
        std::env::var_os("LANGAME_NATIVE_MODULE_IDS"),
        std::env::var_os("LANGAME_NATIVE_PACKAGES_ROOT"),
    )
}

fn parse(
    descriptors: &[ModuleDescriptor],
    single_id: Option<OsString>,
    single_root: Option<OsString>,
    batch_ids: Option<OsString>,
    batch_root: Option<OsString>,
) -> Result<Vec<(String, PathBuf)>, String> {
    let batch = batch_ids.is_some() || batch_root.is_some();
    if batch && (single_id.is_some() || single_root.is_some()) {
        return Err("native single and batch inputs are mutually exclusive".into());
    }
    let (ids, root) = if batch {
        (batch_ids, batch_root)
    } else {
        (single_id, single_root)
    };
    let ids = ids.ok_or("native module ID input is required")?;
    let ids = ids
        .to_str()
        .ok_or("native module IDs must be valid Unicode")?;
    let ids = if batch {
        ids.split(',').take(33).collect::<Vec<_>>()
    } else {
        vec![ids]
    };
    if ids.len() > 32 {
        return Err("native catalog is limited to 32 modules".into());
    }
    let mut seen = HashSet::new();
    let mut selected = Vec::new();
    for id in ids {
        let id = id.trim();
        if id.is_empty() || !seen.insert(id) {
            return Err("native module IDs must be nonempty and unique".into());
        }
        let descriptor = descriptors
            .iter()
            .find(|item| item.summary.id == id)
            .ok_or_else(|| format!("unknown native module ID: {id}"))?;
        selected.push(descriptor);
    }
    let root = plain_directory(Path::new(&root.ok_or("native package root is required")?))?;
    selected
        .into_iter()
        .map(|descriptor| {
            let source = if batch {
                let relative = Path::new(
                    &descriptor
                        .install
                        .as_ref()
                        .ok_or("native module has no installation directory")?
                        .shared_game_dir,
                );
                if relative.as_os_str().is_empty()
                    || relative
                        .components()
                        .any(|part| !matches!(part, Component::Normal(_)))
                {
                    return Err(
                        "native module installation directory must be relative and contained"
                            .into(),
                    );
                }
                let source = plain_directory(&root.join(relative))?;
                if !source.starts_with(&root) {
                    return Err("native module source escaped the selected package root".into());
                }
                source
            } else {
                root.clone()
            };
            Ok((descriptor.summary.id.clone(), source))
        })
        .collect()
}

fn plain_directory(path: &Path) -> Result<PathBuf, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| "native package directory is unavailable")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("native package directory must not be a reparse point".into());
        }
    }
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("native package source must be a plain directory".into());
    }
    drop(fs::read_dir(path).map_err(|_| "native package directory is unreadable")?);
    fs::canonicalize(path).map_err(|_| "cannot resolve native package directory".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_catalog_preserves_single_and_uses_declared_batch_directories() {
        let modules = app_modules::discover_modules(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../modules"),
        )
        .unwrap();
        let mut descriptor = modules
            .into_iter()
            .find(|item| item.summary.id == "rimworld")
            .unwrap();
        descriptor.install.as_mut().unwrap().shared_game_dir = "declared-package".into();
        let root = std::env::temp_dir().join(format!("native-catalog-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("declared-package")).unwrap();
        let descriptors = [descriptor];
        let single = parse(
            &descriptors,
            Some("rimworld".into()),
            Some(root.clone().into_os_string()),
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            single,
            vec![("rimworld".into(), root.canonicalize().unwrap())]
        );
        let batch = parse(
            &descriptors,
            None,
            None,
            Some("rimworld".into()),
            Some(root.clone().into_os_string()),
        )
        .unwrap();
        assert_eq!(
            batch,
            vec![(
                "rimworld".into(),
                root.join("declared-package").canonicalize().unwrap()
            )]
        );
        for ids in [
            "",
            "rimworld,",
            "rimworld,rimworld",
            "../outside",
            "unknown",
        ] {
            assert!(
                parse(
                    &descriptors,
                    None,
                    None,
                    Some(ids.into()),
                    Some(root.clone().into_os_string())
                )
                .is_err()
            );
        }
        assert!(
            parse(
                &descriptors,
                Some("rimworld".into()),
                None,
                Some("rimworld".into()),
                Some(root.clone().into_os_string())
            )
            .is_err()
        );
        let too_many = (0..33).map(|_| "rimworld").collect::<Vec<_>>().join(",");
        assert!(
            parse(
                &descriptors,
                None,
                None,
                Some(too_many.into()),
                Some(root.clone().into_os_string())
            )
            .unwrap_err()
            .contains("32 modules")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_catalog_rejects_missing_or_non_directory_roots() {
        let root =
            std::env::temp_dir().join(format!("native-catalog-file-{}", uuid::Uuid::new_v4()));
        assert!(plain_directory(&root).is_err());
        fs::write(&root, b"not a directory").unwrap();
        assert!(plain_directory(&root).is_err());
        fs::remove_file(root).unwrap();
    }
}
