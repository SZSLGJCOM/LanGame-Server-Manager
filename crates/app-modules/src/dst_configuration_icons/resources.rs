use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

pub(super) struct Resources {
    root: PathBuf,
    images: PathBuf,
    pub recovered_snapshot: bool,
}

impl Resources {
    pub fn open(install_root: &Path) -> Result<Option<Self>, String> {
        Self::open_images(install_root, Path::new("data/images"))
    }

    pub fn open_cache(app_data_root: &Path) -> Result<Option<Self>, String> {
        let Some(mut resources) = Self::open_images(
            app_data_root,
            Path::new("cache/dontstarve-configuration-icons/data/images"),
        )?
        else {
            return Ok(None);
        };
        match fs::symlink_metadata(&resources.images) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let previous = resources
                    .root
                    .join("cache/dontstarve-configuration-icons/data/images.previous");
                match fs::symlink_metadata(&previous) {
                    Ok(_) => {
                        resources.images = previous;
                        resources.recovered_snapshot = true;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(format!("failed to inspect previous icon cache: {error}"));
                    }
                }
            }
            Err(error) => return Err(format!("failed to inspect icon cache: {error}")),
            Ok(_) => {}
        }
        Ok(Some(resources))
    }

    fn open_images(root: &Path, images: &Path) -> Result<Option<Self>, String> {
        let metadata = match fs::symlink_metadata(root) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("failed to inspect DST installation: {error}")),
        };
        if is_reparse(&metadata) || !metadata.is_dir() {
            return Err(String::from(
                "DST installation must be a directory without a reparse point",
            ));
        }
        let root = fs::canonicalize(root)
            .map_err(|error| format!("failed to resolve DST installation: {error}"))?;
        let images = root.join(images);
        Ok(Some(Self {
            root,
            images,
            recovered_snapshot: false,
        }))
    }

    pub fn read(
        &self,
        atlas: &str,
        extension: &str,
        limit: usize,
    ) -> Result<Option<Vec<u8>>, String> {
        if !matches!(
            atlas,
            "worldgen_customization" | "worldsettings_customization"
        ) || !matches!(extension, "xml" | "tex")
        {
            return Err(String::from("unsupported DST icon resource"));
        }
        let filename = format!("{atlas}.{extension}");
        let loose_path = self.images.join(&filename);
        if let Some(file) = open_checked_file(&self.root, &loose_path, limit as u64)? {
            return read_limited(file, limit).map(Some);
        }
        Ok(None)
    }
}

fn open_checked_file(root: &Path, path: &Path, limit: u64) -> Result<Option<File>, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| String::from("DST asset path escaped installation"))?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if is_reparse(&metadata) => {
                return Err(String::from(
                    "DST asset path contains a symbolic link or reparse point",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("failed to inspect DST asset path: {error}")),
        }
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let file = options
        .open(path)
        .map_err(|error| format!("failed to open DST asset: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("failed to inspect opened DST asset: {error}"))?;
    if !metadata.is_file() || is_reparse(&metadata) || metadata.len() > limit {
        return Err(String::from(
            "DST asset is not a regular file within its size limit",
        ));
    }
    let resolved = fs::canonicalize(path)
        .map_err(|error| format!("failed to resolve opened DST asset: {error}"))?;
    if !resolved.starts_with(root) {
        return Err(String::from(
            "DST asset path escaped its installation directory",
        ));
    }
    Ok(Some(file))
}

pub(super) fn is_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn read_limited(reader: impl Read, limit: usize) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut output)
        .map_err(|error| format!("failed to read DST icon resource: {error}"))?;
    if output.len() > limit {
        return Err(String::from("DST icon resource exceeds its byte limit"));
    }
    Ok(output)
}

#[cfg(test)]
#[path = "resources_tests.rs"]
mod tests;
