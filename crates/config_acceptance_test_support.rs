// This test-only source is included by two crates that use complementary helper subsets.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use app_modules::ModuleDescriptor;
use serde_json::{Map, Value};

#[path = "config_acceptance_test_paths.rs"]
pub(crate) mod paths;
pub(crate) use paths::{safe_relative_path, unique_system_temp_root};
#[path = "config_acceptance_initial_files.rs"]
pub(crate) mod initial_files;
pub(crate) use initial_files::{InitialConfigFile, parse_initial_files};

pub(crate) const CONFIG_ACCEPTANCE_INSTANCE_NAME: &str = "LanGame configuration acceptance";

pub(crate) fn config_acceptance_instance_id(module_id: &str) -> String {
    format!("acceptance-{module_id}")
}

#[derive(Debug)]
pub(crate) struct ConfigAcceptanceFixture {
    pub(crate) module_id: String,
    pub(crate) settings: Map<String, Value>,
    pub(crate) initial_files: Vec<InitialConfigFile>,
    pub(crate) expected_files: Vec<ExpectedConfigFile>,
    pub(crate) expected_launch: ExpectedLaunchPlan,
}

#[derive(Debug)]
pub(crate) struct ExpectedConfigFile {
    pub(crate) root: ExpectedOutputRoot,
    pub(crate) path: PathBuf,
    pub(crate) format: String,
    pub(crate) keys: Option<Map<String, Value>>,
    pub(crate) entries: Option<Vec<ExpectedConfigEntry>>,
    pub(crate) fragments: Option<Vec<String>>,
}

#[derive(Debug)]
pub(crate) struct ExpectedConfigEntry {
    pub(crate) key: String,
    pub(crate) value: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ExpectedOutputRoot {
    Config,
    Install,
    Saves,
    Instance,
}

#[derive(Debug)]
pub(crate) struct ExpectedLaunchPlan {
    pub(crate) executable_suffix: Option<PathBuf>,
    pub(crate) arguments: Vec<String>,
}

pub(crate) fn repository_root() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    root.canonicalize().unwrap_or(root)
}

pub(crate) fn discover_fixture_paths(
    modules: &[ModuleDescriptor],
) -> Result<BTreeMap<String, Vec<PathBuf>>, String> {
    let mut discovered = BTreeMap::new();

    for module in modules {
        let fixtures_root = module.root.join("config-fixtures");
        if !fixtures_root.exists() {
            continue;
        }
        if !fixtures_root.is_dir() {
            return Err(format!(
                "config fixture location is not a directory: {}",
                fixtures_root.display()
            ));
        }

        let entries = fs::read_dir(&fixtures_root).map_err(|source| {
            format!(
                "failed to read config fixture directory {}: {source}",
                fixtures_root.display()
            )
        })?;
        let mut paths = entries
            .map(|entry| {
                entry.map(|entry| entry.path()).map_err(|source| {
                    format!(
                        "failed to read config fixture directory {}: {source}",
                        fixtures_root.display()
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        paths.retain(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        });
        paths.sort();

        if !paths.is_empty() {
            discovered.insert(module.summary.id.clone(), paths);
        }
    }

    Ok(discovered)
}

pub(crate) fn resolve_fixture_selection(
    known_modules: &BTreeSet<String>,
    fixtures: &BTreeMap<String, Vec<PathBuf>>,
    requested: Option<&str>,
) -> Result<Vec<String>, String> {
    let Some(requested) = requested else {
        let fixture_modules = fixtures.keys().cloned().collect::<BTreeSet<_>>();
        let missing = known_modules
            .difference(&fixture_modules)
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(format!("modules have no fixtures: {}", missing.join(", ")));
        }
        let unknown = fixture_modules
            .difference(known_modules)
            .cloned()
            .collect::<Vec<_>>();
        if !unknown.is_empty() {
            return Err(format!(
                "fixtures reference unknown modules: {}",
                unknown.join(", ")
            ));
        }
        return Ok(known_modules.iter().cloned().collect());
    };

    if requested.trim().is_empty() {
        return Err(String::from("module selection must not be empty"));
    }

    let mut selected = Vec::new();
    let mut seen = BTreeSet::new();
    for module_id in requested
        .split(',')
        .map(str::trim)
        .filter(|module_id| !module_id.is_empty())
    {
        if !seen.insert(module_id) {
            return Err(format!("duplicate module selection: {module_id}"));
        }
        if !known_modules.contains(module_id) {
            return Err(format!("unknown module selection: {module_id}"));
        }
        if !fixtures.contains_key(module_id) {
            return Err(format!("module has no fixtures: {module_id}"));
        }
        selected.push(String::from(module_id));
    }

    if selected.is_empty() {
        return Err(String::from("module selection must not be empty"));
    }

    Ok(selected)
}

pub(crate) fn read_acceptance_fixture(
    fixture_path: &Path,
) -> Result<ConfigAcceptanceFixture, String> {
    let text = fs::read_to_string(fixture_path).map_err(|source| {
        format!(
            "failed to read config fixture {}: {source}",
            fixture_path.display()
        )
    })?;
    let document = serde_json::from_str::<Value>(&text).map_err(|source| {
        format!(
            "invalid fixture JSON at {}: {source}",
            fixture_path.display()
        )
    })?;
    let root = require_object(&document, "fixture root", fixture_path)?;

    if root.get("fixture_version").and_then(Value::as_u64) != Some(1) {
        return Err(format!(
            "fixture_version must be 1 at {}",
            fixture_path.display()
        ));
    }
    let module_id = require_string(root, "module_id", fixture_path)?;
    if module_id.trim().is_empty() {
        return Err(format!(
            "module_id must not be empty at {}",
            fixture_path.display()
        ));
    }
    let settings = root
        .get("settings")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| format!("settings must be an object at {}", fixture_path.display()))?;
    let initial_files = parse_initial_files(root, fixture_path)?;

    let expected = root
        .get("expected")
        .and_then(Value::as_object)
        .ok_or_else(|| format!("expected must be an object at {}", fixture_path.display()))?;
    let expected_files = parse_expected_files(expected, fixture_path)?;
    let expected_launch = parse_expected_launch(expected, fixture_path)?;
    let has_native_assertion = expected_files.iter().any(|file| {
        file.keys.as_ref().is_some_and(|keys| !keys.is_empty())
            || file
                .entries
                .as_ref()
                .is_some_and(|entries| !entries.is_empty())
            || file
                .fragments
                .as_ref()
                .is_some_and(|fragments| !fragments.is_empty())
    }) || expected_launch.executable_suffix.is_some()
        || !expected_launch.arguments.is_empty();
    if !has_native_assertion {
        return Err(format!(
            "fixture must assert at least one native output at {}",
            fixture_path.display()
        ));
    }

    Ok(ConfigAcceptanceFixture {
        module_id,
        settings,
        initial_files,
        expected_files,
        expected_launch,
    })
}

fn parse_expected_files(
    expected: &Map<String, Value>,
    fixture_path: &Path,
) -> Result<Vec<ExpectedConfigFile>, String> {
    let files = expected
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            format!(
                "expected.files must be an array at {}",
                fixture_path.display()
            )
        })?;

    files
        .iter()
        .enumerate()
        .map(|(index, file)| {
            let file = require_object(file, &format!("expected.files[{index}]"), fixture_path)?;
            let root = match require_string(file, "root", fixture_path)?.as_str() {
                "config" => ExpectedOutputRoot::Config,
                "install" => ExpectedOutputRoot::Install,
                "saves" => ExpectedOutputRoot::Saves,
                "instance" => ExpectedOutputRoot::Instance,
                other => {
                    return Err(format!(
                        "unsupported expected output root {other:?} at {}",
                        fixture_path.display()
                    ));
                }
            };
            let raw_path = require_string(file, "path", fixture_path)?;
            let path = safe_relative_path(&raw_path, fixture_path)?;
            let format = require_string(file, "format", fixture_path)?;
            if format.trim().is_empty() {
                return Err(format!(
                    "expected file format must not be empty at {}",
                    fixture_path.display()
                ));
            }
            let keys = parse_optional_object(file, "keys", fixture_path)?;
            let entries = file
                .get("entries")
                .map(|entries| {
                    let entries = entries.as_array().ok_or_else(|| {
                        format!("entries must be an array at {}", fixture_path.display())
                    })?;
                    entries
                        .iter()
                        .map(|entry| {
                            let entry = require_object(entry, "expected entry", fixture_path)?;
                            let key = require_string(entry, "key", fixture_path)?;
                            if key.trim().is_empty() {
                                return Err(format!(
                                    "expected entry key must not be empty at {}",
                                    fixture_path.display()
                                ));
                            }
                            Ok(ExpectedConfigEntry {
                                key,
                                value: entry.get("value").cloned().ok_or_else(|| {
                                    format!(
                                        "expected entry value is required at {}",
                                        fixture_path.display()
                                    )
                                })?,
                            })
                        })
                        .collect::<Result<Vec<_>, String>>()
                })
                .transpose()?;
            let fragments = file
                .get("fragments")
                .map(|fragments| {
                    let fragments = fragments.as_array().ok_or_else(|| {
                        format!("fragments must be an array at {}", fixture_path.display())
                    })?;
                    fragments
                        .iter()
                        .map(|fragment| {
                            fragment
                                .as_str()
                                .filter(|fragment| !fragment.trim().is_empty())
                                .map(String::from)
                                .ok_or_else(|| {
                                    format!(
                                        "expected text fragments must be non-empty strings at {}",
                                        fixture_path.display()
                                    )
                                })
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?;

            if [keys.is_some(), entries.is_some(), fragments.is_some()]
                .into_iter()
                .filter(|present| *present)
                .count()
                != 1
            {
                return Err(format!(
                    "expected file must declare exactly one of keys, entries, or fragments at {}",
                    fixture_path.display()
                ));
            }
            if keys.as_ref().is_some_and(Map::is_empty)
                || entries.as_ref().is_some_and(Vec::is_empty)
                || fragments.as_ref().is_some_and(Vec::is_empty)
            {
                return Err(format!(
                    "expected file assertion must not be empty at {}",
                    fixture_path.display()
                ));
            }
            Ok(ExpectedConfigFile {
                root,
                path,
                format,
                keys,
                entries,
                fragments,
            })
        })
        .collect()
}

fn parse_expected_launch(
    expected: &Map<String, Value>,
    fixture_path: &Path,
) -> Result<ExpectedLaunchPlan, String> {
    let launch = expected
        .get("launch")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            format!(
                "expected.launch must be an object at {}",
                fixture_path.display()
            )
        })?;
    let executable_suffix = match launch.get("executable_suffix") {
        Some(Value::Null) => None,
        Some(Value::String(suffix)) => Some(safe_relative_path(suffix, fixture_path)?),
        _ => {
            return Err(format!(
                "expected.launch.executable_suffix must be a string or null at {}",
                fixture_path.display()
            ));
        }
    };
    let arguments = launch
        .get("arguments")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            format!(
                "expected.launch.arguments must be an array at {}",
                fixture_path.display()
            )
        })?
        .iter()
        .map(|argument| {
            argument.as_str().map(String::from).ok_or_else(|| {
                format!(
                    "expected launch arguments must be strings at {}",
                    fixture_path.display()
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ExpectedLaunchPlan {
        executable_suffix,
        arguments,
    })
}

fn require_object<'a>(
    value: &'a Value,
    field: &str,
    fixture_path: &Path,
) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{field} must be an object at {}", fixture_path.display()))
}

fn require_string(
    object: &Map<String, Value>,
    field: &str,
    fixture_path: &Path,
) -> Result<String, String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .map(String::from)
        .ok_or_else(|| format!("{field} must be a string at {}", fixture_path.display()))
}

fn parse_optional_object(
    object: &Map<String, Value>,
    field: &str,
    fixture_path: &Path,
) -> Result<Option<Map<String, Value>>, String> {
    object
        .get(field)
        .map(|value| {
            value
                .as_object()
                .cloned()
                .ok_or_else(|| format!("{field} must be an object at {}", fixture_path.display()))
        })
        .transpose()
}
