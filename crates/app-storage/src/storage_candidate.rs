use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::StorageError;
use crate::atomic_file::create_file_atomically;
use crate::instance_archive_files::native::{self, OwnedNode};
#[cfg(not(windows))]
use crate::managed_console_log::owned_fs::FileIdentity;

const MARKER: &str = ".langame-server-manager.json";
const CLAIM: &str = ".langame-server-manager.initializing";
const MAX_MARKER_BYTES: u64 = 2048;
const PRODUCT: &str = "cn.langame.servermanager";
const DIRECTORIES: [&str; 7] = [
    "cmd",
    "cmd/steamcmd",
    "server-files",
    "instances",
    "instances/.trash",
    "app-data",
    "app-data/ServerManager",
];

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootMarker {
    product: String,
    schema_version: u32,
    directory: serde_json::Value,
}

/// Keep the checked Windows directory chain and marker unchanged until the
/// caller has prepared its private metadata. Game contents are not traversed.
pub(super) struct CandidateGuard {
    _nodes: Vec<OwnedNode>,
    _claim: Option<InitialClaim>,
}

pub(super) fn prepare(root: &Path) -> Result<CandidateGuard, StorageError> {
    super::create_normal_directory(root)?;
    let root_node = pin_directory(root)?;
    let marker_path = root.join(MARKER);
    if super::check_plain_file_if_present(&marker_path)? {
        let marker = read_marker(root, &root_node)?;
        if root
            .join(CLAIM)
            .try_exists()
            .map_err(|error| read_error(root, error))?
        {
            return Err(rejected(root, "数据目录正在初始化，请稍后重试。"));
        }
        let mut nodes = vec![root_node, marker];
        for name in DIRECTORIES {
            nodes.push(pin_directory(&root.join(name))?);
        }
        return Ok(CandidateGuard {
            _nodes: nodes,
            _claim: None,
        });
    }

    require_entries(root, &[])?;
    let claim = InitialClaim::create(&root.join(CLAIM))?;
    // A pinned directory can still receive new entries from another process.
    // Recheck after claiming it; never adopt an external initializer's files.
    require_entries(root, &[CLAIM])?;
    let mut nodes = vec![root_node];
    for name in DIRECTORIES {
        let directory = root.join(name);
        fs::create_dir(&directory).map_err(|source| StorageError::CreatePath {
            path: directory.clone(),
            source,
        })?;
        nodes.push(pin_directory(&directory)?);
    }
    require_entries(
        root,
        &[CLAIM, "cmd", "server-files", "instances", "app-data"],
    )?;
    require_entries(&root.join("cmd"), &["steamcmd"])?;
    require_entries(&root.join("app-data"), &["ServerManager"])?;
    for name in [
        "cmd/steamcmd",
        "server-files",
        "instances/.trash",
        "app-data/ServerManager",
    ] {
        require_entries(&root.join(name), &[])?;
    }
    require_entries(&root.join("instances"), &[".trash"])?;
    for name in [
        "",
        "cmd/steamcmd",
        "server-files",
        "instances",
        "instances/.trash",
        "app-data/ServerManager",
    ] {
        super::probe_writable_directory(&root.join(name))?;
    }
    let marker = RootMarker {
        product: PRODUCT.to_owned(),
        schema_version: 1,
        directory: serde_json::to_value(
            nodes[0]
                .identity()
                .map_err(|error| read_error(root, error))?,
        )?,
    };
    create_file_atomically(&marker_path, &serde_json::to_vec_pretty(&marker)?).map_err(
        |source| StorageError::WriteConfig {
            path: marker_path,
            source,
        },
    )?;
    nodes.push(read_marker(root, &nodes[0])?);
    Ok(CandidateGuard {
        _nodes: nodes,
        _claim: Some(claim),
    })
}

fn read_marker(root: &Path, directory: &OwnedNode) -> Result<OwnedNode, StorageError> {
    let path = root.join(MARKER);
    let mut node =
        native::open_verified_file(&path, false).map_err(|error| read_error(&path, error))?;
    let mut bytes = Vec::new();
    node.reader()
        .take(MAX_MARKER_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| read_error(&path, error))?;
    if bytes.len() as u64 > MAX_MARKER_BYTES {
        return Err(rejected(&path, "数据目录标识过大，未接管此目录。"));
    }
    let marker: RootMarker = serde_json::from_slice(&bytes)
        .map_err(|_| rejected(&path, "无法识别数据目录标识，未接管此目录。"))?;
    if marker.product != PRODUCT
        || marker.schema_version != 1
        || marker.directory
            != serde_json::to_value(
                directory
                    .identity()
                    .map_err(|error| read_error(root, error))?,
            )?
    {
        return Err(rejected(
            &path,
            "数据目录标识与当前目录不匹配，未接管此目录。",
        ));
    }
    Ok(node)
}

fn pin_directory(path: &Path) -> Result<OwnedNode, StorageError> {
    super::require_normal_directory(path)?;
    native::open(path, true, false).map_err(|error| read_error(path, error))
}

fn require_entries(path: &Path, allowed: &[&str]) -> Result<(), StorageError> {
    let entries = fs::read_dir(path).map_err(|error| read_error(path, error))?;
    for (index, entry) in entries.enumerate() {
        let entry = entry.map_err(|error| read_error(path, error))?;
        if index >= allowed.len() || !allowed.iter().any(|name| entry.file_name() == *name) {
            return Err(rejected(
                path,
                "目录已有无法确认归属的内容，未接管此目录；请选择独立的空目录。",
            ));
        }
    }
    Ok(())
}

fn read_error(path: &Path, source: io::Error) -> StorageError {
    StorageError::ReadPath {
        path: path.to_owned(),
        source,
    }
}

fn rejected(path: &Path, message: &str) -> StorageError {
    read_error(path, io::Error::new(io::ErrorKind::InvalidData, message))
}

struct InitialClaim {
    _file: File,
    #[cfg(not(windows))]
    path: std::path::PathBuf,
    #[cfg(not(windows))]
    identity: FileIdentity,
}

impl InitialClaim {
    fn create(path: &Path) -> Result<Self, StorageError> {
        let mut options = OpenOptions::new();
        options.create_new(true).read(true).write(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_FLAG_DELETE_ON_CLOSE, FILE_FLAG_OPEN_REPARSE_POINT,
            };
            // The OS removes exactly this newly created claim on close, even
            // after a crash. No existing claim is opened, overwritten or deleted.
            options
                .share_mode(0)
                .custom_flags(FILE_FLAG_DELETE_ON_CLOSE | FILE_FLAG_OPEN_REPARSE_POINT);
        }
        let file = options
            .open(path)
            .map_err(|source| StorageError::CreatePath {
                path: path.to_owned(),
                source,
            })?;
        Ok(Self {
            #[cfg(not(windows))]
            identity: crate::managed_console_log::owned_fs::identity(&file)
                .map_err(|error| read_error(path, error))?,
            _file: file,
            #[cfg(not(windows))]
            path: path.to_owned(),
        })
    }
}

#[cfg(not(windows))]
impl Drop for InitialClaim {
    fn drop(&mut self) {
        let result = (|| {
            let mut node = native::open_verified_file(&self.path, true)?;
            if crate::managed_console_log::owned_fs::identity(node.reader())? != self.identity {
                return Err(io::Error::other(
                    "Storage initialization claim was replaced",
                ));
            }
            node.remove()
        })();
        if let Err(error) = result {
            eprintln!(
                "Cannot remove storage initialization claim {}: {error}",
                self.path.display()
            );
        }
    }
}
