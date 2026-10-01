use super::*;
use serde_json::{Map as JsonMap, Value as JsonValue, json};

#[path = "evidence_files.rs"]
mod files;
use files::{guard_path, io_error, is_link, read_evidence_source, safe_component};

#[path = "evidence_lua.rs"]
mod lua;
use lua::read_metadata;

const PAGE_ITEMS: usize = 5;
const MAX_SCAN_ENTRIES: usize = 512;
const MAX_PAGE_BYTES: usize = 10 * 1024;
const MAX_ITEM_BYTES: usize = 1800;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DstInstalledModEvidencePage {
    pub entries: Vec<DstInstalledModEvidence>,
    pub total_matches: usize,
    pub next_offset: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DstInstalledModEvidence {
    pub folder_name: String,
    pub source: Option<String>,
    pub status: String,
    pub files: serde_json::Value,
    pub metadata: serde_json::Value,
    pub message: Option<String>,
}

/// Returns direct declared directory candidates, without resolving display names.
pub fn dst_mod_dependency_names(metadata: &serde_json::Value) -> Vec<String> {
    let mut names = Vec::new();
    let Some(groups) = metadata
        .get("mod_dependencies")
        .and_then(JsonValue::as_array)
    else {
        return names;
    };
    for group in groups {
        let Some(candidates) = group.as_object() else {
            continue;
        };
        for (key, value) in candidates {
            let name = if key == "workshop" {
                let Some(name) = value.as_str().filter(|name| name.starts_with("workshop-")) else {
                    continue;
                };
                name
            } else if value.as_bool() == Some(false) {
                key.as_str()
            } else {
                continue;
            };
            if validate_directory_name(name).is_err()
                || names.iter().any(|existing| existing == name)
            {
                continue;
            }
            names.push(name.to_owned());
            if names.len() == 5 {
                return names;
            }
        }
    }
    names
}

/// Extracts untrusted directory candidates for metadata reads, not root causes.
pub fn dst_mod_error_names(lines: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    for line in lines {
        let Some(name) = native_mod_error_name(line) else {
            continue;
        };
        if !names.iter().any(|existing| existing == name) {
            names.push(name.to_owned());
            if names.len() == 5 {
                break;
            }
        }
    }
    names
}

fn native_mod_error_name(line: &str) -> Option<&str> {
    let line = line.trim();
    let body = if let Some(timestamped) = line.strip_prefix('[') {
        let (timestamp, body) = timestamped.split_once("]: ")?;
        let mut parts = timestamp.split(':');
        let hours = parts.next()?;
        let minutes = parts.next()?;
        let seconds = parts.next()?;
        if parts.next().is_some()
            || hours.len() < 2
            || !hours.bytes().all(|value| value.is_ascii_digit())
            || ![minutes, seconds].iter().all(|part| {
                part.len() == 2
                    && part.bytes().all(|value| value.is_ascii_digit())
                    && part.parse::<u8>().is_ok_and(|value| value < 60)
            })
        {
            return None;
        }
        body
    } else {
        line
    };
    let rest = body.strip_prefix("MOD ERROR: ")?;
    let delimiter = rest
        .find(": ")
        .or_else(|| rest.strip_suffix(':').map(str::len))?;
    let name = if let Some(display_start) = rest.find(" (").filter(|index| *index < delimiter) {
        // ModInfoname appends a display label. Its contents are not filesystem
        // evidence, even if they contain other names, paths, or instructions.
        let display = &rest[display_start + 1..];
        let mut depth = 0usize;
        let mut label_end = None;
        for (index, character) in display.char_indices() {
            match character {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        label_end = Some(index + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        let suffix = &display[label_end?..];
        if suffix != ":" && !suffix.starts_with(": ") {
            return None;
        }
        &rest[..display_start]
    } else {
        &rest[..delimiter]
    };
    validate_directory_name(name).ok()?;
    Some(name)
}

pub fn read_dst_installed_mod_evidence(
    install_root: &Path,
    extra_roots: &[PathBuf],
    names: &[String],
    offset: usize,
) -> Result<DstInstalledModEvidencePage, String> {
    if names.len() > 10 || extra_roots.len() > 4 {
        return Err(String::from(
            "Read at most 10 Mod names from at most 4 extra roots.",
        ));
    }
    for name in names {
        validate_directory_name(name)?;
    }
    let roots = evidence_roots(install_root, extra_roots)?;
    let mut candidates = Vec::new();
    let mut scanned = 0;
    let mut requested = names.to_vec();
    requested.sort();
    requested.dedup();
    for root in &roots {
        let Some(_guards) = guard_path(&root.path)? else {
            continue;
        };
        if names.is_empty() {
            for entry in fs::read_dir(&root.path).map_err(io_error)? {
                scanned += 1;
                if scanned > MAX_SCAN_ENTRIES {
                    return Err(String::from(
                        "Mod directory scan limit exceeded; query exact directory names.",
                    ));
                }
                let entry = entry.map_err(io_error)?;
                let metadata = fs::symlink_metadata(entry.path()).map_err(io_error)?;
                if is_link(&metadata) {
                    return Err(String::from(
                        "Mod evidence paths cannot cross symlinks or reparse points.",
                    ));
                }
                if !metadata.is_dir() {
                    continue;
                }
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| String::from("Mod directory name is not UTF-8."))?;
                let Some(name) = root.logical_name(&name) else {
                    continue;
                };
                validate_directory_name(&name)?;
                if let Some(candidate) = inspect_candidate(root, &name)? {
                    candidates.push(candidate);
                }
            }
        } else {
            for name in &requested {
                if let Some(candidate) = inspect_candidate(root, name)? {
                    candidates.push(candidate);
                }
            }
        }
    }
    for name in requested {
        if !candidates.iter().any(|candidate| candidate.name == name) {
            candidates.push(ModCandidate {
                name,
                path: None,
                source: None,
            });
        }
    }
    candidates.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.source.cmp(&right.source))
    });
    let total_matches = candidates.len();
    let entries = candidates
        .iter()
        .skip(offset)
        .take(PAGE_ITEMS)
        .map(read_candidate)
        .collect::<Result<Vec<_>, _>>()?;
    let end = offset.saturating_add(entries.len());
    let page = DstInstalledModEvidencePage {
        entries,
        total_matches,
        next_offset: (end < total_matches).then_some(end),
    };
    if serde_json::to_vec(&page)
        .map_err(|error| error.to_string())?
        .len()
        > MAX_PAGE_BYTES
    {
        return Err(String::from(
            "Mod evidence page exceeded its output byte limit.",
        ));
    }
    Ok(page)
}

struct EvidenceRoot {
    path: PathBuf,
    label: String,
    workshop: bool,
}

impl EvidenceRoot {
    fn logical_name(&self, directory: &str) -> Option<String> {
        if self.workshop {
            (!directory.is_empty() && directory.bytes().all(|value| value.is_ascii_digit()))
                .then(|| format!("workshop-{directory}"))
        } else {
            Some(directory.to_owned())
        }
    }
}

struct ModCandidate {
    name: String,
    path: Option<PathBuf>,
    source: Option<String>,
}

fn evidence_roots(install: &Path, extra: &[PathBuf]) -> Result<Vec<EvidenceRoot>, String> {
    let mut roots = vec![EvidenceRoot {
        path: install.join("mods"),
        label: String::from("install/mods"),
        workshop: false,
    }];
    for (index, base) in std::iter::once(install)
        .chain(extra.iter().map(PathBuf::as_path))
        .enumerate()
    {
        let label = if index == 0 {
            String::from("install")
        } else {
            format!("extra-{}", index - 1)
        };
        guard_path(base)?;
        // Only explicitly supplied roots are authorized. Do not borrow a sibling
        // instance's UGC tree or search a shared ancestor for other installs.
        for path in build_workshop_root_candidates(base, DST_STEAM_APP_ID)
            .into_iter()
            .filter(|path| path.starts_with(base))
        {
            let relative = path
                .strip_prefix(base)
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            roots.push(EvidenceRoot {
                path,
                label: format!("{label}/{relative}"),
                workshop: true,
            });
        }
        if index != 0 {
            for shard in ["Master", "Caves"] {
                let relative = format!("ugc/{shard}/content/{DST_STEAM_APP_ID}");
                roots.push(EvidenceRoot {
                    path: base.join(&relative),
                    label: format!("{label}/{relative}"),
                    workshop: true,
                });
            }
        }
    }
    let mut seen = HashSet::new();
    roots.retain(|root| seen.insert(root.path.to_string_lossy().to_ascii_lowercase()));
    Ok(roots)
}

pub fn validate_directory_name(name: &str) -> Result<(), String> {
    if name.len() > 128
        || !safe_component(name)
        || name
            .strip_prefix("workshop-")
            .is_some_and(|id| id.is_empty() || !id.bytes().all(|value| value.is_ascii_digit()))
    {
        return Err(String::from(
            "Supply a plain Mod directory name without paths, streams, or traversal.",
        ));
    }
    Ok(())
}

fn inspect_candidate(root: &EvidenceRoot, name: &str) -> Result<Option<ModCandidate>, String> {
    let directory = if root.workshop {
        let Some(id) = name.strip_prefix("workshop-") else {
            return Ok(None);
        };
        id
    } else {
        name
    };
    let path = root.path.join(directory);
    let Some(guards) = guard_path(&path)? else {
        return Ok(None);
    };
    if !guards
        .last()
        .ok_or("Mod directory could not be inspected.")?
        .metadata()
        .map_err(io_error)?
        .is_dir()
    {
        return Ok(None);
    }
    Ok(Some(ModCandidate {
        name: name.to_owned(),
        path: Some(path),
        source: Some(format!("{}/{directory}", root.label)),
    }))
}

fn read_candidate(candidate: &ModCandidate) -> Result<DstInstalledModEvidence, String> {
    let mut entry = DstInstalledModEvidence {
        folder_name: candidate.name.clone(),
        source: candidate.source.clone(),
        status: String::from("missing"),
        files: json!({"modinfo": "missing", "modmain": "missing"}),
        metadata: JsonValue::Null,
        message: None,
    };
    let Some(path) = candidate.path.as_ref() else {
        return Ok(entry);
    };
    let _guards = guard_path(path)?.ok_or("Mod directory changed during inspection.")?;
    for (key, file) in [("modinfo", "modinfo.lua"), ("modmain", "modmain.lua")] {
        if let Some(guards) = guard_path(&path.join(file))? {
            entry.files[key] = json!(if guards
                .last()
                .ok_or("Mod file could not be inspected.")?
                .metadata()
                .map_err(io_error)?
                .is_file()
            {
                "present"
            } else {
                "not_file"
            });
        }
    }
    if entry.files["modinfo"] != "present" {
        entry.status = String::from("missing_modinfo");
        return Ok(entry);
    }
    match read_metadata(path, &candidate.name) {
        Ok(metadata) => {
            entry.status = String::from("read");
            entry.metadata = metadata;
        }
        Err(message) => {
            entry.status = String::from("parse_error");
            entry.message = Some(message);
        }
    }
    if serde_json::to_vec(&entry)
        .map_err(|error| error.to_string())?
        .len()
        > MAX_ITEM_BYTES
    {
        entry.status = String::from("budget_error");
        entry.metadata = JsonValue::Null;
        entry.message = Some(String::from(
            "Mod metadata exceeded the evidence output byte limit; dependencies were not truncated.",
        ));
    }
    Ok(entry)
}

#[cfg(test)]
#[path = "evidence_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "evidence_dependency_tests.rs"]
mod dependency_tests;
