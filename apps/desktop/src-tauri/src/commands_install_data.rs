use super::{ModuleDescriptor, ProtectedInstallDataPath};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

const MAX_DATA_ENTRIES: usize = 4096;
const MAX_DATA_DEPTH: usize = 64;

pub(super) fn declared_install_root_retained_paths(
    descriptor: &ModuleDescriptor,
    install_root: &Path,
) -> Result<Vec<ProtectedInstallDataPath>, String> {
    descriptor.storage.validate_retained_paths()?;
    let mut retained = Vec::new();
    for relative in &descriptor.storage.retained_paths {
        let path = install_root.join(relative);
        if contains_preserved_data(&path)? {
            retained.push(ProtectedInstallDataPath {
                source: format!("模块声明的原生配置 `{relative}`"),
                path: Some(path),
            });
        }
    }
    Ok(retained)
}

pub(super) fn declared_install_root_data_path(
    descriptor: &ModuleDescriptor,
    install_root: &Path,
) -> Result<Option<ProtectedInstallDataPath>, String> {
    let Some(template) = descriptor.storage.saves_path_template.as_deref() else {
        return Ok(None);
    };
    if !template.contains("paths.install_root") {
        return Ok(None);
    }

    let source = format!("模块声明的存档路径模板 `{template}`");
    let Some(path) = app_storage::install_save_directory_prefix(template, install_root) else {
        // A root-wide or unresolvable declaration cannot distinguish program
        // files from retained data. Keep the existing conservative protection.
        return Ok(Some(ProtectedInstallDataPath { source, path: None }));
    };
    Ok(
        contains_preserved_data(&path)?.then_some(ProtectedInstallDataPath {
            source,
            path: Some(path),
        }),
    )
}

pub(super) fn contains_preserved_data(path: &Path) -> Result<bool, String> {
    if !plain_directory_chain(path)? {
        return Ok(true);
    }
    let mut pending = vec![(path.to_path_buf(), 0)];
    let mut entries_seen = 0;
    while let Some((directory, depth)) = pending.pop() {
        let metadata = match fs::symlink_metadata(&directory) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(data_check_error(&directory, &error)),
        };
        if metadata_is_link(&metadata) || !metadata.is_dir() {
            return Ok(true);
        }
        if depth > MAX_DATA_DEPTH {
            return Err(format!(
                "检查存档目录 {} 时超过 {MAX_DATA_DEPTH} 层目录上限，已拒绝卸载",
                path.display()
            ));
        }
        let entries =
            fs::read_dir(&directory).map_err(|error| data_check_error(&directory, &error))?;
        for entry in entries {
            let entry = entry.map_err(|error| data_check_error(&directory, &error))?;
            entries_seen += 1;
            if entries_seen > MAX_DATA_ENTRIES {
                return Err(format!(
                    "检查存档目录 {} 时超过 {MAX_DATA_ENTRIES} 个目录项上限，已拒绝卸载",
                    path.display()
                ));
            }
            let entry_path = entry.path();
            let metadata = fs::symlink_metadata(&entry_path)
                .map_err(|error| data_check_error(&entry_path, &error))?;
            if metadata_is_link(&metadata) || !metadata.is_dir() {
                return Ok(true);
            }
            pending.push((entry_path, depth + 1));
        }
    }
    Ok(false)
}

// Inspect ancestors from the filesystem root before inspecting their children.
// symlink_metadata on only the final path would still follow an intermediate
// junction/symlink, possibly misclassifying an external empty tree as disposable.
fn plain_directory_chain(path: &Path) -> Result<bool, String> {
    let ancestors = path.ancestors().collect::<Vec<_>>();
    for ancestor in ancestors.into_iter().rev() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let metadata = match fs::symlink_metadata(ancestor) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(true),
            Err(error) => return Err(data_check_error(ancestor, &error)),
        };
        if metadata_is_link(&metadata) {
            return Ok(false);
        }
        if !metadata.is_dir() {
            if ancestor == path {
                return Ok(false);
            }
            return Err(format!(
                "检查存档目录 {} 失败：父路径 {} 不是目录，已拒绝卸载",
                path.display(),
                ancestor.display()
            ));
        }
    }
    Ok(true)
}

pub(super) fn metadata_is_link(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x0000_0400 != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn data_check_error(path: &Path, error: &std::io::Error) -> String {
    format!("检查存档路径 {} 失败，已拒绝卸载：{error}", path.display())
}

#[cfg(test)]
#[path = "commands_install_data_tests.rs"]
mod tests;
