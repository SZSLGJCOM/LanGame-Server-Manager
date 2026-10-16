//! Routes every validated package file; payload is never silently discarded.
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::commands) enum ThunderstorePayloadKind {
    Plugin,
    Config,
    Patcher,
    Metadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::commands) struct ThunderstorePayloadRoute {
    pub(in crate::commands) source_relative: PathBuf,
    pub(in crate::commands) kind: ThunderstorePayloadKind,
    pub(in crate::commands) relative: PathBuf,
}

/// Files have already passed ZIP/filesystem safety validation. Check paths here
/// too so directory and ZIP callers share Windows routing semantics.
pub(in crate::commands) fn thunderstore_payload_routes(
    files: &[PathBuf],
) -> Result<Vec<ThunderstorePayloadRoute>, String> {
    let mut destinations = HashSet::new();
    let mut routes = Vec::with_capacity(files.len());
    for file in files {
        let parts = relative_components(file)
            .ok_or_else(|| format!("Invalid Thunderstore payload path: {}", file.display()))?;
        let (kind, offset) = if parts[0].eq_ignore_ascii_case("BepInEx") {
            let kind = parts.get(1).and_then(|part| runtime_kind(part)).ok_or_else(|| {
                format!("Thunderstore loader payload {} must be installed through the reviewed game loader", file.display())
            })?;
            (kind, 2)
        } else if let Some(kind) = runtime_kind(parts[0]) {
            (kind, 1)
        } else if parts[0].eq_ignore_ascii_case("dotnet")
            || ["winhttp.dll", "doorstop_config.ini", ".doorstop_version"]
                .iter()
                .any(|name| parts[0].eq_ignore_ascii_case(name))
            || parts[0].to_ascii_lowercase().starts_with(".langame-")
        {
            return Err(format!(
                "Thunderstore loader or manager payload {} cannot be installed as a Mod",
                file.display()
            ));
        } else if root_metadata(&parts) {
            (ThunderstorePayloadKind::Metadata, 0)
        } else {
            (ThunderstorePayloadKind::Plugin, 0)
        };
        if parts.len() <= offset {
            return Err(format!(
                "Thunderstore runtime directory is a file: {}",
                file.display()
            ));
        }
        let relative: PathBuf = parts[offset..].iter().collect();
        let container = match kind {
            ThunderstorePayloadKind::Plugin | ThunderstorePayloadKind::Metadata => "plugins",
            ThunderstorePayloadKind::Config => "config",
            ThunderstorePayloadKind::Patcher => "patchers",
        };
        let identity = format!(
            "{container}/{}",
            relative.to_string_lossy().replace('\\', "/").to_lowercase()
        );
        if !destinations.insert(identity) {
            return Err(format!(
                "Thunderstore payload paths collide at {}",
                file.display()
            ));
        }
        routes.push(ThunderstorePayloadRoute {
            source_relative: file.clone(),
            kind,
            relative,
        });
    }
    Ok(routes)
}

fn runtime_kind(value: &str) -> Option<ThunderstorePayloadKind> {
    if value.eq_ignore_ascii_case("plugins") {
        Some(ThunderstorePayloadKind::Plugin)
    } else if value.eq_ignore_ascii_case("config") {
        Some(ThunderstorePayloadKind::Config)
    } else if value.eq_ignore_ascii_case("patchers") {
        Some(ThunderstorePayloadKind::Patcher)
    } else {
        None
    }
}

fn relative_components(path: &Path) -> Option<Vec<&str>> {
    let value = path.to_str()?;
    if value.is_empty() || value.starts_with(['/', '\\']) || value.contains('\0') {
        return None;
    }
    let parts = value.split(['/', '\\']).collect::<Vec<_>>();
    if parts
        .iter()
        .any(|part| part.is_empty() || matches!(*part, "." | "..") || part.contains(':'))
    {
        return None;
    }
    Some(parts)
}

fn root_metadata(parts: &[&str]) -> bool {
    parts.len() == 1
        && matches!(
            parts[0].to_ascii_lowercase().as_str(),
            "manifest.json"
                | "icon.png"
                | "readme"
                | "readme.md"
                | "readme.txt"
                | "changelog"
                | "changelog.md"
                | "changelog.txt"
                | "license"
                | "license.md"
                | "license.txt"
                | "licence"
                | "licence.md"
                | "licence.txt"
                | "notice"
                | "notice.md"
                | "notice.txt"
                | "third_party_notices"
                | "third_party_notices.md"
                | "third_party_notices.txt"
        )
}

#[cfg(test)]
#[path = "commands_mod_thunderstore_layout_tests.rs"]
mod tests;
