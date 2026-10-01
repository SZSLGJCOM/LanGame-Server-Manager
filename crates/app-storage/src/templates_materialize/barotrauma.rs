use std::fs;
use std::path::Path;

use super::StorageError;
use crate::templates::templates_materialize::package_staging::copy_package_file;

const MAX_FILES: usize = 50_000;
const MAX_DEPTH: usize = 24;

/// Barotrauma resets cwd to its entry assembly directory. The actual executable,
/// dependencies and content must therefore live beside the instance-owned XML.
pub(super) fn materialize_runtime(install: &Path, instance: &Path) -> Result<(), StorageError> {
    if !install.join("DedicatedServer.exe").is_file() {
        return Ok(());
    }
    fs::create_dir_all(instance).map_err(|error| failure(instance, error))?;
    let source_root = fs::canonicalize(install).map_err(|error| failure(install, error))?;
    let target_root = fs::canonicalize(instance).map_err(|error| failure(instance, error))?;
    if target_root.starts_with(&source_root) || source_root.starts_with(&target_root) {
        return Err(failure(
            instance,
            "The Barotrauma instance and package roots must be separate.",
        ));
    }
    let mut files = 0;
    for entry in fs::read_dir(install).map_err(|error| failure(install, error))? {
        let entry = entry.map_err(|error| failure(install, error))?;
        let name = entry.file_name();
        let Some(name_text) = name.to_str() else {
            continue;
        };
        let lower = name_text.to_ascii_lowercase();
        if lower.ends_with(".dll")
            || lower.ends_with(".exe")
            || lower.ends_with(".deps.json")
            || lower.ends_with(".runtimeconfig.json")
            || lower == "steam_appid.txt"
        {
            copy_file(&entry.path(), &instance.join(name), false, &mut files)?;
        }
    }
    copy_directory(
        &install.join("Content"),
        &instance.join("Content"),
        false,
        0,
        &mut files,
    )?;
    // The shared package may have been used as an old server's working directory.
    // Only seed known shipped defaults; never inherit permissions, bans or saves.
    let source_data = install.join("Data");
    let target_data = instance.join("Data");
    reject_link(&source_data)?;
    if target_data.exists() {
        reject_link(&target_data)?;
    }
    fs::create_dir_all(&target_data).map_err(|error| failure(&target_data, error))?;
    for entry in fs::read_dir(&source_data).map_err(|error| failure(&source_data, error))? {
        let entry = entry.map_err(|error| failure(&source_data, error))?;
        let name = entry.file_name();
        let Some(name_text) = name.to_str() else {
            continue;
        };
        let lower = name_text.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "campaignsettings.xml"
                | "forbiddenwordlist.txt"
                | "karmasettings.xml"
                | "languageoptions.xml"
        ) || (lower.starts_with("permissionpresets") && lower.ends_with(".xml"))
        {
            copy_file(&entry.path(), &target_data.join(name), true, &mut files)?;
        }
    }
    Ok(())
}

fn copy_directory(
    source: &Path,
    target: &Path,
    preserve: bool,
    depth: usize,
    files: &mut usize,
) -> Result<(), StorageError> {
    if depth >= MAX_DEPTH {
        return Err(failure(
            source,
            "Package directory depth exceeds the limit.",
        ));
    }
    reject_link(source)?;
    if target.exists() {
        reject_link(target)?;
    }
    fs::create_dir_all(target).map_err(|error| failure(target, error))?;
    for entry in fs::read_dir(source).map_err(|error| failure(source, error))? {
        let entry = entry.map_err(|error| failure(source, error))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let kind = entry
            .file_type()
            .map_err(|error| failure(&source_path, error))?;
        if kind.is_dir() {
            copy_directory(&source_path, &target_path, preserve, depth + 1, files)?;
        } else {
            copy_file(&source_path, &target_path, preserve, files)?;
        }
    }
    Ok(())
}

fn copy_file(
    source: &Path,
    target: &Path,
    preserve: bool,
    files: &mut usize,
) -> Result<(), StorageError> {
    *files += 1;
    if *files > MAX_FILES {
        return Err(failure(source, "Package file count exceeds the limit."));
    }
    let source_metadata = reject_link(source)?;
    if !source_metadata.is_file() {
        return Err(failure(source, "Package entry is not a regular file."));
    }
    match fs::symlink_metadata(target) {
        Ok(_) => {
            let existing = reject_link(target)?;
            if !existing.is_file() {
                return Err(failure(target, "Instance entry is not a regular file."));
            }
            if preserve
                || (existing.len() == source_metadata.len()
                    && existing
                        .modified()
                        .ok()
                        .zip(source_metadata.modified().ok())
                        .is_some_and(|(target_time, source_time)| target_time == source_time))
            {
                return Ok(());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(failure(target, error)),
    }
    copy_package_file("barotrauma", source, target)
}

fn reject_link(path: &Path) -> Result<fs::Metadata, StorageError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| failure(path, error))?;
    let link = metadata.file_type().is_symlink();
    #[cfg(windows)]
    let link = {
        use std::os::windows::fs::MetadataExt;
        link || metadata.file_attributes() & 0x400 != 0
    };
    if link {
        return Err(failure(
            path,
            "Linked package or instance entries cannot be materialized.",
        ));
    }
    Ok(metadata)
}

fn failure(path: &Path, message: impl std::fmt::Display) -> StorageError {
    StorageError::ModuleSupportMaterialization {
        module_id: "barotrauma".into(),
        path: path.to_owned(),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_is_independent_and_preserves_instance_permissions_and_saves() {
        let root = std::env::temp_dir().join(format!(
            "barotrauma-layout-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let install = root.join("package");
        let instance = root.join("instance");
        fs::create_dir_all(install.join("Content")).unwrap();
        fs::create_dir_all(install.join("Data")).unwrap();
        fs::create_dir_all(instance.join("Data")).unwrap();
        fs::create_dir_all(instance.join("Content")).unwrap();
        fs::create_dir_all(instance.join("Multiplayer")).unwrap();
        fs::write(instance.join("Content/Operator.xml"), "operator content").unwrap();
        fs::write(install.join("DedicatedServer.exe"), "executable").unwrap();
        fs::write(install.join("DedicatedServer.dll"), "assembly").unwrap();
        fs::write(install.join("DedicatedServer.runtimeconfig.json"), "{}").unwrap();
        fs::write(install.join("Content/Vanilla.xml"), "package content").unwrap();
        fs::write(install.join("Data/campaignsettings.xml"), "default").unwrap();
        fs::write(
            install.join("Data/bannedplayers.xml"),
            "another server's bans",
        )
        .unwrap();
        fs::write(
            install.join("Data/bannedplayers.txt"),
            "another server's old bans",
        )
        .unwrap();
        fs::write(
            install.join("Data/clientpermissions.xml"),
            "package permissions",
        )
        .unwrap();
        fs::write(
            instance.join("Data/clientpermissions.xml"),
            "instance permissions",
        )
        .unwrap();
        fs::write(instance.join("serversettings.xml"), "instance settings").unwrap();
        fs::write(instance.join("Multiplayer/world.save"), "retained world").unwrap();
        materialize_runtime(&install, &instance).unwrap();
        assert_eq!(
            fs::read_to_string(instance.join("DedicatedServer.dll")).unwrap(),
            "assembly"
        );
        assert_eq!(
            fs::read_to_string(instance.join("Content/Vanilla.xml")).unwrap(),
            "package content"
        );
        assert_eq!(
            fs::read_to_string(instance.join("Data/clientpermissions.xml")).unwrap(),
            "instance permissions"
        );
        assert_eq!(
            fs::read_to_string(instance.join("Data/campaignsettings.xml")).unwrap(),
            "default"
        );
        assert!(!instance.join("Data/bannedplayers.xml").exists());
        assert!(!instance.join("Data/bannedplayers.txt").exists());
        assert_eq!(
            fs::read_to_string(instance.join("serversettings.xml")).unwrap(),
            "instance settings"
        );
        assert_eq!(
            fs::read_to_string(instance.join("Multiplayer/world.save")).unwrap(),
            "retained world"
        );
        fs::write(install.join("DedicatedServer.dll"), "updated assembly").unwrap();
        fs::write(
            instance.join("Data/campaignsettings.xml"),
            "operator campaign",
        )
        .unwrap();
        materialize_runtime(&install, &instance).unwrap();
        assert_eq!(
            fs::read_to_string(instance.join("DedicatedServer.dll")).unwrap(),
            "updated assembly"
        );
        assert_eq!(
            fs::read_to_string(instance.join("Content/Operator.xml")).unwrap(),
            "operator content"
        );
        assert_eq!(
            fs::read_to_string(instance.join("Data/campaignsettings.xml")).unwrap(),
            "operator campaign"
        );
        assert!(materialize_runtime(&install, &install).is_err());
        let fresh_instance = root.join("fresh-instance");
        materialize_runtime(&install, &fresh_instance).unwrap();
        assert!(!fresh_instance.join("Data/clientpermissions.xml").exists());
        assert!(!fresh_instance.join("Data/bannedplayers.xml").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
