const ASSISTANT_CONFIG_SCAN_ENTRIES: usize = 4096;
const ASSISTANT_CONFIG_SCAN_DEPTH: usize = 12;
const ASSISTANT_CONFIG_FILE_BYTES: usize = 256 * 1024;
const ASSISTANT_CONFIG_LIST_PAGE: usize = 64;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AssistantConfigFilePage {
    pub files: Vec<String>,
    pub next_offset: Option<usize>,
    pub scan_truncated: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AssistantConfigFileSlice {
    // Only a validated instance-relative filename is exposed to the model.
    #[serde(rename = "file")]
    pub path: String,
    pub content: String,
    pub offset_bytes: usize,
    pub next_offset_bytes: Option<usize>,
    pub truncated: bool,
}

pub(super) fn read_assistant_instance_config_documents(
    config_file_path: &str,
) -> Result<Vec<AssistantOperationConfigDocument>, String> {
    let Some(root) = assistant_instance_config_root(config_file_path)? else {
        return Ok(Vec::new());
    };
    let mut paths = Vec::new();
    collect_assistant_config_paths(&root, &mut paths)?;
    paths
        .into_iter()
        .take(ASSISTANT_CONFIG_DOCUMENT_LIMIT)
        .map(|path| {
            let mut document = read_assistant_config_document(path.clone())?;
            document.path = assistant_relative_config_path(&root, &path)?;
            Ok(document)
        })
        .collect()
}

pub(super) fn list_assistant_instance_config_files(
    config_file_path: &str,
    offset: usize,
    limit: usize,
) -> Result<AssistantConfigFilePage, String> {
    let Some(root) = assistant_instance_config_root(config_file_path)? else {
        return Ok(AssistantConfigFilePage {
            files: Vec::new(),
            next_offset: None,
            scan_truncated: false,
        });
    };
    let (paths, scan_truncated) = scan_assistant_config_paths(&root)?;
    let end = offset
        .saturating_add(limit.clamp(1, ASSISTANT_CONFIG_LIST_PAGE))
        .min(paths.len());
    let files = paths
        .iter()
        .skip(offset)
        .take(end.saturating_sub(offset))
        .map(|path| assistant_relative_config_path(&root, path))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(AssistantConfigFilePage {
        files,
        next_offset: (end < paths.len()).then_some(end),
        scan_truncated,
    })
}

pub(super) fn read_assistant_instance_config_file(
    config_file_path: &str,
    relative_path: &str,
    offset_bytes: usize,
    max_bytes: usize,
) -> Result<AssistantConfigFileSlice, String> {
    let relative = validate_assistant_relative_config_path(relative_path)?;
    let root = assistant_instance_config_root(config_file_path)?
        .ok_or_else(|| String::from("Instance configuration directory is unavailable."))?;
    let path = root.join(&relative);
    let content = read_assistant_config_text(&path)?;
    if offset_bytes > content.len() || !content.is_char_boundary(offset_bytes) {
        return Err(String::from(
            "Configuration offset must be a UTF-8 boundary in the redacted document.",
        ));
    }
    let mut end = offset_bytes
        .saturating_add(max_bytes.clamp(4, ASSISTANT_CONFIG_DOCUMENT_BYTES))
        .min(content.len());
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    Ok(AssistantConfigFileSlice {
        path: relative,
        content: content[offset_bytes..end].to_string(),
        offset_bytes,
        next_offset_bytes: (end < content.len()).then_some(end),
        truncated: end < content.len(),
    })
}

pub(super) fn collect_assistant_config_paths(
    root: &Path,
    paths: &mut Vec<PathBuf>,
) -> Result<(), String> {
    paths.extend(scan_assistant_config_paths(root)?.0);
    Ok(())
}

fn scan_assistant_config_paths(root: &Path) -> Result<(Vec<PathBuf>, bool), String> {
    let _root_guards = guard_assistant_config_directories(root)?;
    let mut directories = vec![(root.to_path_buf(), 0)];
    let mut paths = Vec::new();
    let mut remaining = ASSISTANT_CONFIG_SCAN_ENTRIES;
    let mut truncated = false;
    while let Some((directory, depth)) = directories.pop() {
        let _directory_guards = guard_assistant_config_directories(&directory)?;
        let entries = fs::read_dir(&directory)
            .map_err(|error| format!("Cannot list instance configuration directory: {error}"))?;
        let mut children = Vec::new();
        for entry in entries {
            if remaining == 0 {
                truncated = true;
                break;
            }
            remaining -= 1;
            let entry =
                entry.map_err(|error| format!("Cannot inspect configuration entry: {error}"))?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)
                .map_err(|error| format!("Cannot inspect configuration entry: {error}"))?;
            if assistant_config_metadata_is_link(&metadata) {
                truncated = true;
                continue;
            }
            if metadata.is_dir() {
                if depth < ASSISTANT_CONFIG_SCAN_DEPTH {
                    children.push((path, depth + 1));
                } else {
                    truncated = true;
                }
            } else if metadata.is_file() && assistant_config_file_is_supported(&path) {
                if assistant_relative_config_path(root, &path).is_ok() {
                    paths.push(path);
                } else {
                    truncated = true;
                }
            }
        }
        children.sort_by(|left, right| right.0.cmp(&left.0));
        directories.extend(children);
        if remaining == 0 && !directories.is_empty() {
            truncated = true;
            break;
        }
    }
    paths.sort();
    Ok((paths, truncated))
}

pub(super) fn assistant_config_file_is_supported(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return false;
    };
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "cfg"
            | "conf"
            | "ini"
            | "json"
            | "lua"
            | "properties"
            | "toml"
            | "txt"
            | "xml"
            | "yaml"
            | "yml"
    )
}

pub(super) fn read_assistant_config_document(
    path: PathBuf,
) -> Result<AssistantOperationConfigDocument, String> {
    let content = read_assistant_config_text(&path)?;
    let truncated = content.len() > ASSISTANT_CONFIG_DOCUMENT_BYTES;
    let content = if truncated {
        truncate_assistant_prompt_text(&content, ASSISTANT_CONFIG_DOCUMENT_BYTES)
    } else {
        content
    };
    Ok(AssistantOperationConfigDocument {
        path: path.to_string_lossy().to_string(),
        content,
        truncated,
    })
}

fn read_assistant_config_text(path: &Path) -> Result<String, String> {
    if !assistant_config_file_is_supported(path) {
        return Err(String::from(
            "Unsupported instance configuration file type.",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| String::from("Configuration file has no parent directory."))?;
    let _directory_guards = guard_assistant_config_directories(parent)?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Cannot inspect configuration file: {error}"))?;
    if !metadata.is_file() || assistant_config_metadata_is_link(&metadata) {
        return Err(String::from(
            "Configuration files cannot be links or reparse points.",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        };
        options
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .share_mode(FILE_SHARE_READ);
    }
    let file = options
        .open(path)
        .map_err(|error| format!("Cannot open configuration file: {error}"))?;
    let opened = file
        .metadata()
        .map_err(|error| format!("Cannot inspect opened configuration file: {error}"))?;
    if !opened.is_file() || assistant_config_metadata_is_link(&opened) {
        return Err(String::from(
            "Configuration files cannot be links or reparse points.",
        ));
    }
    if opened.len() > ASSISTANT_CONFIG_FILE_BYTES as u64 {
        return Err(String::from(
            "Configuration file exceeds the 256 KiB safe read size limit.",
        ));
    }
    let mut bytes = Vec::new();
    file.take((ASSISTANT_CONFIG_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Cannot read configuration file: {error}"))?;
    if bytes.len() > ASSISTANT_CONFIG_FILE_BYTES {
        return Err(String::from(
            "Configuration file exceeds the 256 KiB safe read size limit.",
        ));
    }
    redact_assistant_file_content(path, &decode_assistant_config_text(&bytes)?)
}

fn redact_assistant_file_content(path: &Path, content: &str) -> Result<String, String> {
    // Redact the complete document before pagination. Include the file stem in
    // the existing credential policy: cluster_token.txt has no key in its body.
    let file_key = path
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(|| String::from("Configuration filename is not valid UTF-8."))?;
    Ok(crate::assistant::redact_assistant_file_text(
        file_key, content,
    ))
}

fn decode_assistant_config_text(bytes: &[u8]) -> Result<String, String> {
    let utf16 = bytes
        .strip_prefix(&[0xff, 0xfe])
        .map(|body| (body, true))
        .or_else(|| bytes.strip_prefix(&[0xfe, 0xff]).map(|body| (body, false)));
    let content = if let Some((body, little_endian)) = utf16 {
        if body.len() % 2 != 0 {
            return Err(String::from(
                "Configuration encoding contains an incomplete UTF-16 code unit.",
            ));
        }
        let units = body
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                if little_endian {
                    u16::from_le_bytes([pair[0], pair[1]])
                } else {
                    u16::from_be_bytes([pair[0], pair[1]])
                }
            })
            .collect::<Vec<_>>();
        String::from_utf16(&units)
            .map_err(|_| String::from("Configuration encoding contains invalid UTF-16."))?
    } else {
        let body = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
        std::str::from_utf8(body)
            .map_err(|_| {
                String::from("Configuration encoding must be UTF-8 or UTF-16 with a BOM.")
            })?
            .to_owned()
    };
    // NULs can hide assignment keys from redaction, including UTF-16 without a BOM.
    if content.contains('\0') {
        return Err(String::from(
            "Configuration text cannot contain NUL characters.",
        ));
    }
    Ok(content)
}

fn assistant_instance_config_root(config_file_path: &str) -> Result<Option<PathBuf>, String> {
    let path = Path::new(config_file_path);
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(String::from(
            "Instance configuration root must be an absolute managed path.",
        ));
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => {
            return Err(format!(
                "Cannot inspect instance configuration root: {error}"
            ));
        }
    };
    if metadata
        .as_ref()
        .is_some_and(assistant_config_metadata_is_link)
    {
        return Err(String::from(
            "Instance configuration root cannot be a link or reparse point.",
        ));
    }
    let root = if metadata.as_ref().is_some_and(fs::Metadata::is_dir) {
        path
    } else {
        path.parent()
            .ok_or_else(|| String::from("Instance configuration path has no parent directory."))?
    };
    if !root.exists() {
        return Ok(None);
    }
    guard_assistant_config_directories(root)?;
    Ok(Some(root.to_path_buf()))
}

fn assistant_relative_config_path(root: &Path, path: &Path) -> Result<String, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| String::from("Configuration path is outside the instance directory."))?
        .to_str()
        .ok_or_else(|| String::from("Configuration path is not valid UTF-8."))?
        .replace('\\', "/");
    validate_assistant_relative_config_path(&relative)
}

fn validate_assistant_relative_config_path(value: &str) -> Result<String, String> {
    let relative = value.replace('\\', "/");
    if relative.len() > 1024
        || relative.is_empty()
        || relative.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.ends_with(['.', ' '])
                || part.chars().any(|ch| {
                    ch.is_control() || matches!(ch, ':' | '*' | '?' | '"' | '<' | '>' | '|')
                })
        })
    {
        return Err(String::from(
            "Configuration path must be a plain relative file path without traversal or streams.",
        ));
    }
    if !assistant_config_file_is_supported(Path::new(&relative)) {
        return Err(String::from(
            "Unsupported instance configuration file type.",
        ));
    }
    Ok(relative)
}

fn assistant_config_metadata_is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

fn guard_assistant_config_directories(path: &Path) -> Result<Vec<fs::File>, String> {
    if !path.is_absolute() {
        return Err(String::from("Configuration directory must be absolute."));
    }
    let mut guards = Vec::new();
    for directory in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        let metadata = fs::symlink_metadata(directory)
            .map_err(|error| format!("Cannot inspect configuration directory: {error}"))?;
        if !metadata.is_dir() || assistant_config_metadata_is_link(&metadata) {
            return Err(String::from(
                "Configuration directories cannot be links or reparse points.",
            ));
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY,
                FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
            };
            // Holding each ancestor without delete sharing prevents a directory
            // replacement from redirecting the subsequent file open on Windows.
            options
                .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
        }
        let guard = options
            .open(directory)
            .map_err(|error| format!("Cannot open configuration directory: {error}"))?;
        let opened = guard
            .metadata()
            .map_err(|error| format!("Cannot inspect opened configuration directory: {error}"))?;
        if !opened.is_dir() || assistant_config_metadata_is_link(&opened) {
            return Err(String::from(
                "Configuration directories cannot be links or reparse points.",
            ));
        }
        guards.push(guard);
    }
    Ok(guards)
}

#[cfg(test)]
#[path = "config_documents_tests.rs"]
mod config_documents_tests;
