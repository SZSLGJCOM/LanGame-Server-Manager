use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::fs::MetadataExt;
use std::path::{Component, Path, PathBuf, Prefix};

use serde::{Deserialize, Serialize};

pub(super) const MARKER: &str = ".lgsm-desktop-reliability.json";

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Config {
    pub root: PathBuf,
    pub url: String,
    pub nonce: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    schema_version: u32,
    nonce: String,
    host_pid: u32,
}

pub(super) fn checked_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute()
        || !matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_)))
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err("Fixture paths must be absolute local drive paths without traversal".into());
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.file_attributes() & 0x400 != 0 => {
                return Err(format!(
                    "Fixture path contains a reparse point: {}",
                    ancestor.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "Inspect fixture path {}: {error}",
                    ancestor.display()
                ));
            }
        }
    }
    Ok(())
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, String> {
        checked_path(path)?;
        let mut bytes = Vec::new();
        fs::File::open(path)
            .map_err(|error| error.to_string())?
            .take(16 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() > 16 * 1024 {
            return Err("Fixture config exceeds 16 KiB".into());
        }
        let config: Self = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        checked_path(&config.root)?;
        if config.root.parent() != path.parent() {
            return Err("Fixture root must be a new sibling of its config file".into());
        }
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        if !(32..=128).contains(&self.nonce.len())
            || !self
                .nonce
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(
                "Fixture nonce must contain 32..128 ASCII letters, digits or hyphens".into(),
            );
        }
        let url = tauri::Url::parse(&self.url).map_err(|error| error.to_string())?;
        if url.scheme() != "http"
            || url.host_str() != Some("127.0.0.1")
            || url.port().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/tests/helpers/desktop-reliability.html"
            || url.fragment().is_some()
            || url.query().is_some()
        {
            return Err("Fixture URL must be the loopback test page with an explicit port".into());
        }
        Ok(())
    }

    pub fn create_root(&self) -> Result<(), String> {
        // create_dir is exclusive: an existing directory is never adopted or cleared.
        fs::create_dir(&self.root).map_err(|error| format!("Create new fixture root: {error}"))?;
        checked_path(&self.root)?;
        let marker = Marker {
            schema_version: 1,
            nonce: self.nonce.clone(),
            host_pid: std::process::id(),
        };
        write_new_json(&self.root.join(MARKER), &marker)?;
        for name in ["profile", "data", "logs"] {
            fs::create_dir(self.root.join(name)).map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

pub(super) fn verify_server_root(root: &Path, nonce: &str) -> Result<(), String> {
    checked_path(root)?;
    let marker: Marker = serde_json::from_reader(
        fs::File::open(root.join(MARKER)).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    if marker.schema_version != 1 || marker.nonce != nonce || marker.host_pid == std::process::id()
    {
        return Err("Simulated server does not own this fixture marker".into());
    }
    Ok(())
}

pub(super) fn write_new_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("Create {}: {error}", path.display()))?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_reliability_config_rejects_external_urls_and_unknown_fields() {
        let mut config = Config {
            root: PathBuf::from(r"D:\fixture"),
            url: "http://127.0.0.1:43210/tests/helpers/desktop-reliability.html".into(),
            nonce: "a".repeat(32),
        };
        assert!(config.validate().is_ok());
        for url in [
            "http://localhost:43210/tests/helpers/desktop-reliability.html",
            "http://127.0.0.1:43210/",
            "https://127.0.0.1:43210/tests/helpers/desktop-reliability.html",
            "http://127.0.0.1:43210/tests/helpers/desktop-reliability.html?exec=x",
        ] {
            config.url = url.into();
            assert!(config.validate().is_err(), "{url}");
        }
        assert!(
            serde_json::from_str::<Config>(
                r#"{"root":"D:\\fixture","url":"x","nonce":"x","command":"x"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn desktop_reliability_config_never_adopts_an_existing_root() {
        let root = std::env::temp_dir().join(format!("lgsm-host-config-{}", uuid::Uuid::new_v4()));
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let config = Config {
            root: root.clone(),
            url: "http://127.0.0.1:43210/tests/helpers/desktop-reliability.html".into(),
            nonce: "a".repeat(32),
        };
        config.create_root().unwrap();
        let _cleanup = Cleanup(root.clone());
        fs::write(root.join("retained"), b"keep this data").unwrap();
        assert!(config.create_root().is_err());
        assert_eq!(fs::read(root.join("retained")).unwrap(), b"keep this data");
        assert!(verify_server_root(&root, &"b".repeat(32)).is_err());
        assert!(checked_path(Path::new(r"relative\path")).is_err());
        assert!(checked_path(Path::new(r"D:\fixture\..\outside")).is_err());
    }
}
