use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::{ExpectedOutputRoot, safe_relative_path};

#[derive(Debug)]
pub(crate) struct InitialConfigFile {
    pub(crate) root: ExpectedOutputRoot,
    pub(crate) path: PathBuf,
    content: Vec<u8>,
}

pub(crate) struct InitialFileRoots<'a> {
    pub(crate) config: &'a Path,
    pub(crate) install: &'a Path,
    pub(crate) saves: &'a Path,
    pub(crate) instance: &'a Path,
}

pub(crate) fn parse_initial_files(
    fixture: &Map<String, Value>,
    fixture_path: &Path,
) -> Result<Vec<InitialConfigFile>, String> {
    let Some(initial) = fixture.get("initial") else {
        return Ok(Vec::new());
    };
    let initial = initial
        .as_object()
        .ok_or_else(|| format!("initial must be an object at {}", fixture_path.display()))?;
    let files = initial
        .get("files")
        .and_then(Value::as_array)
        .filter(|files| !files.is_empty())
        .ok_or_else(|| {
            format!(
                "initial.files must be a non-empty array at {}",
                fixture_path.display()
            )
        })?;

    let mut parsed = Vec::with_capacity(files.len());
    let mut paths = BTreeSet::new();
    for (index, file) in files.iter().enumerate() {
        let file = file.as_object().ok_or_else(|| {
            format!(
                "initial.files[{index}] must be an object at {}",
                fixture_path.display()
            )
        })?;
        let root = parse_root(file, index, fixture_path)?;
        let raw_path = file.get("path").and_then(Value::as_str).ok_or_else(|| {
            format!(
                "initial.files[{index}].path must be a string at {}",
                fixture_path.display()
            )
        })?;
        let path = safe_relative_path(raw_path, fixture_path)?;
        let logical_target = if root == ExpectedOutputRoot::Config {
            (
                ExpectedOutputRoot::Instance,
                Path::new("config").join(&path),
            )
        } else {
            (root, path.clone())
        };
        if !paths.insert(logical_target) {
            return Err(format!(
                "duplicate initial file {:?}:{} at {}",
                root,
                path.display(),
                fixture_path.display()
            ));
        }
        let content = parse_content(file.get("content"), index, fixture_path)?;
        parsed.push(InitialConfigFile {
            root,
            path,
            content,
        });
    }
    Ok(parsed)
}

pub(crate) fn materialize_initial_files(
    files: &[InitialConfigFile],
    roots: &InitialFileRoots<'_>,
    fixture_path: &Path,
) -> Result<(), String> {
    let mut planned = Vec::with_capacity(files.len());
    let mut destinations = BTreeSet::new();
    for file in files {
        let root = root_path(file.root, roots);
        let canonical_root = fs::canonicalize(root).map_err(|source| {
            format!(
                "failed to resolve initial file root {} for fixture {}: {source}",
                root.display(),
                fixture_path.display()
            )
        })?;
        let destination = canonical_root.join(&file.path);
        if !destination.starts_with(&canonical_root) {
            return Err(format!(
                "initial file target escaped root at {}",
                fixture_path.display()
            ));
        }
        if !destinations.insert(destination.clone()) {
            return Err(format!(
                "initial files resolve to duplicate target {} at {}",
                destination.display(),
                fixture_path.display()
            ));
        }
        if destination.exists() {
            return Err(format!(
                "initial file target already exists {} at {}",
                destination.display(),
                fixture_path.display()
            ));
        }
        planned.push((file, canonical_root, destination));
    }

    for (file, canonical_root, destination) in planned {
        let parent = destination
            .parent()
            .ok_or_else(|| format!("initial file has no parent at {}", fixture_path.display()))?;
        fs::create_dir_all(parent).map_err(|source| {
            format!(
                "failed to create initial file parent {}: {source}",
                parent.display()
            )
        })?;
        let canonical_parent = fs::canonicalize(parent).map_err(|source| {
            format!(
                "failed to resolve initial file parent {}: {source}",
                parent.display()
            )
        })?;
        if !canonical_parent.starts_with(&canonical_root) {
            return Err(format!(
                "initial file parent escaped root at {}",
                fixture_path.display()
            ));
        }
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .map_err(|source| {
                format!(
                    "failed to create initial file {}: {source}",
                    destination.display()
                )
            })?;
        output.write_all(&file.content).map_err(|source| {
            format!(
                "failed to write initial file {}: {source}",
                destination.display()
            )
        })?;
    }
    Ok(())
}

fn parse_root(
    file: &Map<String, Value>,
    index: usize,
    fixture_path: &Path,
) -> Result<ExpectedOutputRoot, String> {
    match file.get("root").and_then(Value::as_str) {
        Some("config") => Ok(ExpectedOutputRoot::Config),
        Some("install") => Ok(ExpectedOutputRoot::Install),
        Some("saves") => Ok(ExpectedOutputRoot::Saves),
        Some("instance") => Ok(ExpectedOutputRoot::Instance),
        other => Err(format!(
            "unsupported initial.files[{index}].root {other:?} at {}",
            fixture_path.display()
        )),
    }
}

fn parse_content(
    content: Option<&Value>,
    index: usize,
    fixture_path: &Path,
) -> Result<Vec<u8>, String> {
    match content {
        Some(Value::String(content)) => Ok(content.as_bytes().to_vec()),
        Some(value @ (Value::Object(_) | Value::Array(_))) => {
            let mut content = serde_json::to_vec_pretty(value).map_err(|source| {
                format!("failed to serialize initial.files[{index}].content: {source}")
            })?;
            content.push(b'\n');
            Ok(content)
        }
        _ => Err(format!(
            "initial.files[{index}].content must be text, an object, or an array at {}",
            fixture_path.display()
        )),
    }
}

fn root_path<'a>(root: ExpectedOutputRoot, roots: &'a InitialFileRoots<'_>) -> &'a Path {
    match root {
        ExpectedOutputRoot::Config => roots.config,
        ExpectedOutputRoot::Install => roots.install,
        ExpectedOutputRoot::Saves => roots.saves,
        ExpectedOutputRoot::Instance => roots.instance,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn config_acceptance_initial_file_parser_rejects_escape_and_duplicate() {
        let fixture = Path::new("fixture.json");
        let escaping = json!({"initial":{"files":[{
            "root":"install", "path":"../escape.json", "content":{}
        }]}});
        assert!(
            parse_initial_files(escaping.as_object().unwrap(), fixture)
                .unwrap_err()
                .contains("unsafe relative path")
        );

        let duplicate = json!({"initial":{"files":[
            {"root":"install", "path":"native.json", "content":{}},
            {"root":"install", "path":"native.json", "content":{}}
        ]}});
        assert!(
            parse_initial_files(duplicate.as_object().unwrap(), fixture)
                .unwrap_err()
                .contains("duplicate initial file")
        );
    }

    #[test]
    fn config_acceptance_initial_file_parser_rejects_root_alias_duplicate() {
        let document = json!({"initial":{"files":[
            {"root":"config", "path":"native.json", "content":{}},
            {"root":"instance", "path":"config/native.json", "content":{}}
        ]}});
        let error = parse_initial_files(document.as_object().unwrap(), Path::new("fixture.json"))
            .unwrap_err();
        assert!(error.contains("duplicate initial file"));
    }

    #[test]
    fn config_acceptance_initial_file_materializer_rechecks_root_escape() {
        let root = super::super::unique_system_temp_root("initial-files");
        let instance = root.join("instance");
        let config = instance.join("config");
        let install = root.join("install");
        let saves = root.join("saves");
        for directory in [&instance, &config, &install, &saves] {
            fs::create_dir_all(directory).unwrap();
        }
        let files = vec![InitialConfigFile {
            root: ExpectedOutputRoot::Install,
            path: PathBuf::from("../escape.json"),
            content: b"escape".to_vec(),
        }];
        let error = materialize_initial_files(
            &files,
            &InitialFileRoots {
                config: &config,
                install: &install,
                saves: &saves,
                instance: &instance,
            },
            Path::new("fixture.json"),
        )
        .unwrap_err();
        assert!(error.contains("escaped root"));
        assert!(!root.join("escape.json").exists());
        let _ = fs::remove_dir_all(root);
    }
}
