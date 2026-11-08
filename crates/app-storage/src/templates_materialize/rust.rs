use super::managed_config_merge::{
    ManagedConfigMergePlan, ManagedConfigMutation, materialization_error, read_optional_bytes,
    read_required_rendered_file,
};
use crate::StorageError;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

const RAW_OWNERSHIP_FILE: &str = "rust-raw-config-ownership.json";

// These November controls are optional: absent overrides retain the operator's
// current native configuration, including values entered before the UI existed.
const OPTIONAL_PERFORMANCE_COMMANDS: [&str; 4] = [
    "net.position_lod",
    "net.position_lod_players",
    "server.batchpackets",
    "server.useplayerupdatejobs",
];

struct RawOwnership {
    path: PathBuf,
    original: Option<Vec<u8>>,
    lines: Vec<String>,
}

impl RawOwnership {
    fn read(config_dir: &Path, config_path: &Path) -> Result<Self, StorageError> {
        let path = config_dir.join(RAW_OWNERSHIP_FILE);
        let original = read_optional_bytes(&path)?;
        let lines: Vec<String> = match original.as_deref() {
            Some(bytes) => serde_json::from_slice(bytes).map_err(|error| {
                materialization_error(
                    "rust",
                    &path,
                    format!("invalid raw configuration ownership: {error}"),
                )
            })?,
            None => {
                let previous = crate::instances::read_instance_settings_json(config_path)?;
                let settings: Map<String, Value> = serde_json::from_str(&previous)?;
                raw_performance_lines(&settings)
            }
        };
        if lines
            .iter()
            .any(|line| line.lines().count() != 1 || !is_performance_assignment(line))
        {
            return Err(materialization_error(
                "rust",
                &path,
                "raw ownership must contain only performance assignments".to_owned(),
            ));
        }
        Ok(Self {
            path,
            original,
            lines,
        })
    }

    fn plan(self) -> Result<ManagedConfigMergePlan, StorageError> {
        Ok(ManagedConfigMergePlan {
            destination_path: self.path,
            replacement: serde_json::to_vec_pretty(&self.lines)?,
            original: self.original,
        })
    }
}

// A running save updates instance.json before it can publish server.cfg. Capture
// the last raw ownership once, and advance it only with a successful cfg write.
pub(super) fn capture_previous_ownership(
    context: &super::ModuleSupportMaterializationContext<'_>,
    config_path: &Path,
    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    if context.module_id != "rust" {
        return Ok(());
    }
    let ownership = RawOwnership::read(context.config_dir, config_path)?;
    if ownership.original.is_none() {
        files.apply(vec![ownership.plan()?])?;
    }
    Ok(())
}

fn raw_performance_lines(settings: &Map<String, Value>) -> Vec<String> {
    settings
        .get("server_cfg_extra")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .lines()
        .filter(|line| is_performance_assignment(line))
        .map(|line| line.trim().to_owned())
        .collect()
}

pub(super) fn materialize_server_configuration(
    source_path: &Path,
    destination_path: &Path,
    settings: &Map<String, Value>,
    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    let config_dir = source_path
        .parent()
        .expect("Rust rendered configuration has a parent");
    let mut ownership = RawOwnership::read(config_dir, &config_dir.join("instance.json"))?;
    let rendered = read_required_rendered_file(source_path, "rust")?;
    let original = read_optional_bytes(destination_path)?;
    let replacement = match original.as_deref() {
        None => rendered,
        Some(bytes) => {
            let existing = std::str::from_utf8(bytes).map_err(|error| {
                materialization_error(
                    "rust",
                    destination_path,
                    format!("refusing to replace non-UTF-8 existing server.cfg: {error}"),
                )
            })?;
            preserve_optional_performance_lines(&rendered, existing, &ownership.lines)
        }
    };
    ownership.lines = raw_performance_lines(settings);
    files.apply(vec![
        ManagedConfigMergePlan {
            destination_path: destination_path.to_owned(),
            replacement: replacement.into_bytes(),
            original,
        },
        ownership.plan()?,
    ])
}

fn command_matches(line: &str, command: &str) -> bool {
    let Some(name) = line.split_whitespace().next() else {
        return false;
    };
    // The reviewed console registry exposes these four unique bare names as
    // case-insensitive aliases. Explicit global.* names are not aliases.
    name.eq_ignore_ascii_case(command)
        || command
            .rsplit_once('.')
            .is_some_and(|(_, bare)| name.eq_ignore_ascii_case(bare))
}

fn command_assigns(line: &str, command: &str) -> bool {
    command_matches(line, command) && line.split_whitespace().nth(1).is_some()
}

fn is_performance_assignment(line: &str) -> bool {
    OPTIONAL_PERFORMANCE_COMMANDS
        .iter()
        .any(|command| command_assigns(line, command))
}

fn preserve_optional_performance_lines(
    rendered: &str,
    existing: &str,
    previous_raw: &[String],
) -> String {
    let lines: Vec<&str> = existing
        .trim_start_matches('\u{feff}')
        .split_inclusive('\n')
        .collect();
    let mut raw_lines: Vec<&str> = previous_raw.iter().map(String::as_str).collect();
    let mut raw_owned = vec![false; lines.len()];
    // Raw extras follow structured controls. Match their occurrences from the
    // end so an identical earlier structured or unmanaged line remains native.
    for (index, line) in lines.iter().enumerate().rev() {
        if let Some(owned) = raw_lines.iter().rposition(|raw| *raw == line.trim()) {
            raw_owned[index] = true;
            raw_lines.remove(owned);
        }
    }
    let preserved: String = lines
        .into_iter()
        .enumerate()
        .filter(|(index, line)| {
            !raw_owned[*index]
                && OPTIONAL_PERFORMANCE_COMMANDS.iter().any(|command| {
                    command_assigns(line, command)
                        && !rendered.lines().any(|line| command_assigns(line, command))
                })
        })
        .map(|(_, line)| {
            if line.ends_with('\n') {
                line.to_owned()
            } else {
                format!("{line}\n")
            }
        })
        .collect();
    if preserved.is_empty() {
        return rendered.to_owned();
    }
    let mut merged = String::with_capacity(rendered.len() + preserved.len());
    let mut inserted = false;
    for line in rendered.split_inclusive('\n') {
        if !inserted && line.trim().eq_ignore_ascii_case("server.writecfg") {
            merged.push_str(&preserved);
            inserted = true;
        }
        merged.push_str(line);
    }
    if !inserted {
        if !merged.is_empty() && !merged.ends_with('\n') {
            merged.push('\n');
        }
        merged.push_str(&preserved);
    }
    merged
}

#[cfg(test)]
#[path = "rust_tests.rs"]
mod tests;
