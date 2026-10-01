use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use super::{AtlasFiles, atlas, resources, texture};

const DATA_COMPONENTS: [&str; 3] = ["cache", "dontstarve-configuration-icons", "data"];
const FILENAMES: [&str; 4] = [
    "worldgen_customization.xml",
    "worldgen_customization.tex",
    "worldsettings_customization.xml",
    "worldsettings_customization.tex",
];

pub(super) fn write_snapshot(app_data_root: &Path, atlases: &[AtlasFiles]) -> Result<(), String> {
    let data = checked_data_directory(app_data_root)?;
    let images = data.join("images");
    let pending = data.join("images.pending");
    let previous = data.join("images.previous");
    let current_files = snapshot_files(&images)?;
    if current_files.is_none() && snapshot_files(&previous)?.is_some() {
        fs::rename(&previous, &images)
            .map_err(|error| format!("failed to restore previous icon cache: {error}"))?;
    }
    let current_files = snapshot_files(&images)?;
    if current_files
        .as_ref()
        .is_some_and(|files| files.len() == atlases.len() * 2)
    {
        let cached = resources::Resources::open_cache(app_data_root)?;
        if let Some(cached) = cached {
            let unchanged = atlases.iter().try_fold(true, |unchanged, atlas| {
                Ok::<_, String>(
                    unchanged
                        && cached
                            .read(atlas.name, "xml", atlas::MAX_ATLAS_XML_BYTES)?
                            .as_deref()
                            == Some(atlas.xml.as_slice())
                        && cached
                            .read(atlas.name, "tex", texture::MAX_TEXTURE_BYTES)?
                            .as_deref()
                            == Some(atlas.texture.as_slice()),
                )
            })?;
            if unchanged {
                return Ok(());
            }
        }
    }

    remove_snapshot(&pending)?;
    fs::create_dir(&pending)
        .map_err(|error| format!("failed to create pending icon cache: {error}"))?;
    let result = (|| {
        for atlas in atlases {
            for (extension, bytes) in [("xml", &atlas.xml), ("tex", &atlas.texture)] {
                let filename = format!("{}.{extension}", atlas.name);
                if !FILENAMES.contains(&filename.as_str()) {
                    return Err(String::from("unexpected icon cache filename"));
                }
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(pending.join(filename))
                    .map_err(|error| format!("failed to create cached atlas: {error}"))?;
                file.write_all(bytes)
                    .and_then(|()| file.sync_all())
                    .map_err(|error| format!("failed to persist cached atlas: {error}"))?;
            }
        }
        // Publish both XML/TEX pairs together. A failed replacement restores the
        // prior complete directory; individual files never replace a live pair.
        remove_snapshot(&previous)?;
        if current_files.is_some() {
            fs::rename(&images, &previous)
                .map_err(|error| format!("failed to retain previous icon cache: {error}"))?;
        }
        if let Err(error) = fs::rename(&pending, &images) {
            if current_files.is_some() {
                fs::rename(&previous, &images).map_err(|restore| {
                    format!("failed to publish icon cache: {error}; failed to restore previous cache: {restore}")
                })?;
            }
            return Err(format!("failed to publish icon cache: {error}"));
        }
        remove_snapshot(&previous)
    })();
    let cleanup = remove_snapshot(&pending);
    match (result, cleanup) {
        (Err(error), Err(cleanup)) => {
            Err(format!("{error}; pending cache cleanup failed: {cleanup}"))
        }
        (Err(error), _) | (_, Err(error)) => Err(error),
        _ => Ok(()),
    }
}

fn checked_data_directory(app_data_root: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(app_data_root).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || resources::is_reparse(&metadata) {
        return Err(String::from(
            "icon cache app-data root is not a regular directory",
        ));
    }
    let root = fs::canonicalize(app_data_root).map_err(|error| error.to_string())?;
    let mut current = root.clone();
    for component in DATA_COMPONENTS {
        current.push(component);
        if let Err(error) = fs::create_dir(&current)
            && error.kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(format!("failed to create icon cache directory: {error}"));
        }
        let metadata = fs::symlink_metadata(&current).map_err(|error| error.to_string())?;
        let resolved = fs::canonicalize(&current).map_err(|error| error.to_string())?;
        if !metadata.is_dir() || resources::is_reparse(&metadata) || !resolved.starts_with(&root) {
            return Err(String::from(
                "icon cache directory contains a reparse point or escaped app data",
            ));
        }
    }
    Ok(current)
}

fn snapshot_files(directory: &Path) -> Result<Option<Vec<PathBuf>>, String> {
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_dir() || resources::is_reparse(&metadata) {
        return Err(String::from(
            "icon cache snapshot is not a regular directory",
        ));
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| String::from("invalid icon cache filename"))?;
        if files.len() == FILENAMES.len() || !FILENAMES.contains(&name) {
            return Err(String::from(
                "icon cache contains unrelated files; leaving them untouched",
            ));
        }
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
        let limit = if name.ends_with(".xml") {
            atlas::MAX_ATLAS_XML_BYTES
        } else {
            texture::MAX_TEXTURE_BYTES
        };
        if !metadata.is_file() || resources::is_reparse(&metadata) || metadata.len() > limit as u64
        {
            return Err(String::from(
                "icon cache snapshot contains an unsafe or oversized file",
            ));
        }
        files.push(entry.path());
    }
    Ok(Some(files))
}

fn remove_snapshot(directory: &Path) -> Result<(), String> {
    if let Some(files) = snapshot_files(directory)? {
        for file in files {
            fs::remove_file(file)
                .map_err(|error| format!("failed to remove cached atlas: {error}"))?;
        }
        fs::remove_dir(directory)
            .map_err(|error| format!("failed to remove cache snapshot: {error}"))?;
    }
    Ok(())
}
