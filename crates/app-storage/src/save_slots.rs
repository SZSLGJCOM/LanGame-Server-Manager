//! Groups a read-only save listing into the native world/save slots each game
//! already defines for import, and marks the slot its current settings select.
//!
//! The slot layouts are the same native target layouts the save importer writes
//! (`save_import_profiles.rs`, `save_import_server_worlds*.rs`), and the
//! "current" names come from the same settings and helpers. Anything a layout
//! does not claim falls back to one row per top-level directory plus one row
//! for loose top-level files, so every listed file belongs to exactly one row.

use super::SaveFileEntry;
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SaveSlotLabel {
    /// `save_slot`, `world`, `map`, `world_id`, `campaign` or `session`.
    pub kind: &'static str,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SaveSlot {
    /// Stable, unique row key.
    pub id: String,
    pub name: String,
    pub label: Option<SaveSlotLabel>,
    /// `native` for a game slot, `directory` / `files` for fallback rows.
    pub kind: &'static str,
    /// Relative roots (directories or files) whose listing entries belong to this slot.
    pub paths: Vec<String>,
    pub size_bytes: u64,
    pub file_count: u64,
    pub modified_at_unix_ms: Option<u64>,
    /// The slot the instance's current settings select (当前使用).
    pub current: bool,
}

/// A name that may be used as a single native path component. Shared with the
/// save importer, which rejects the same values before writing a slot.
pub(crate) fn is_native_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['/', '\\', ':'])
        && !matches!(name, "." | "..")
        && !name.ends_with(['.', ' '])
        && !name.chars().any(char::is_control)
}

/// The configured native name for `key`, when it is a usable single component.
pub(crate) fn native_setting<'a>(settings: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    settings
        .get(key)
        .and_then(Value::as_str)
        .filter(|name| is_native_name(name))
}

/// The configured positive `save_slot`, optionally bounded.
pub(crate) fn configured_save_slot(
    settings: &Map<String, Value>,
    maximum: Option<u64>,
) -> Option<u64> {
    settings
        .get("save_slot")
        .and_then(Value::as_u64)
        .filter(|slot| *slot > 0 && maximum.is_none_or(|max| *slot <= max))
}

/// The Forest keeps multiplayer slots at `Multiplayer/Slot<n>`.
pub(crate) fn theforest_slot_dir(slot: u64) -> String {
    format!("Multiplayer/Slot{slot}")
}

/// Sons of the Forest keeps dedicated slots at `DedicatedServer/Multiplayer/<ten digits>`.
pub(crate) fn sons_slot_dir(slot: u64) -> String {
    format!("DedicatedServer/Multiplayer/{slot:010}")
}

/// Lower-case filename prefix of the Rust world the settings select: empty for a
/// custom map URL. `Err(key)` names a missing numeric setting.
pub(crate) fn rust_world_prefix(
    settings: &Map<String, Value>,
    level: &str,
) -> Result<String, &'static str> {
    let custom = settings
        .get("level_url")
        .and_then(Value::as_str)
        .is_some_and(|url| !url.trim().is_empty());
    if custom {
        return Ok(String::new());
    }
    if matches!(level, "Procedural Map" | "Barren") {
        let number = |key: &'static str| settings.get(key).and_then(Value::as_u64).ok_or(key);
        return Ok(format!(
            "{}.{}.{}.",
            level.replace(' ', "").to_ascii_lowercase(),
            number("world_size")?,
            number("seed")?
        ));
    }
    Ok(format!("{}.", level.to_ascii_lowercase()))
}

/// How one game names its slots, relative to the live saves root.
enum Layout {
    /// Each directory at `parent/<name>` (where `*` matches any one component)
    /// is a slot; `depth` > 1 joins that many components into its name.
    Directories {
        parent: &'static [&'static str],
        depth: usize,
        label: &'static str,
    },
    /// Files directly in `parent` whose name `key` maps to a slot name; files
    /// sharing a key form one slot (e.g. a save and its sidecars).
    Files {
        parent: &'static [&'static str],
        key: fn(&str) -> Option<String>,
        label: &'static str,
    },
}

enum Current {
    Name(String),
    Prefix(String),
}

fn layouts(module_id: &str) -> &'static [Layout] {
    use Layout::*;
    match module_id {
        "theforest" => &[Directories {
            parent: &["Multiplayer"],
            depth: 1,
            label: "save_slot",
        }],
        "sonsoftheforest" => &[Directories {
            parent: &["DedicatedServer", "Multiplayer"],
            depth: 1,
            label: "save_slot",
        }],
        "sevendaystodie" => &[Directories {
            parent: &[],
            depth: 2,
            label: "world",
        }],
        "unturned" => &[Directories {
            parent: &["Level"],
            depth: 1,
            label: "map",
        }],
        "windrose" => &[Directories {
            parent: &["RocksDB_v2", "*", "Worlds"],
            depth: 1,
            label: "world_id",
        }],
        "vrising" => &[Directories {
            parent: &["v4"],
            depth: 1,
            label: "world",
        }],
        "romestead" => &[Directories {
            parent: &[],
            depth: 1,
            label: "world",
        }],
        "valheim" => &[
            Directories {
                parent: &["worlds_local"],
                depth: 1,
                label: "world",
            },
            Files {
                parent: &["worlds_local"],
                key: valheim_world_key,
                label: "world",
            },
        ],
        "necesse" => &[
            Files {
                parent: &[],
                key: necesse_zip_key,
                label: "world",
            },
            Directories {
                parent: &[],
                depth: 1,
                label: "world",
            },
        ],
        "astroneer" => &[Files {
            parent: &[],
            key: astroneer_key,
            label: "session",
        }],
        "terraria" => &[Files {
            parent: &[],
            key: terraria_key,
            label: "world",
        }],
        "corekeeper" => &[Files {
            parent: &[],
            key: corekeeper_key,
            label: "save_slot",
        }],
        "returntomoria" => &[Files {
            parent: &[],
            key: moria_key,
            label: "world",
        }],
        "barotrauma" => &[Files {
            parent: &[],
            key: barotrauma_key,
            label: "campaign",
        }],
        "rust" => &[Files {
            parent: &[],
            key: rust_key,
            label: "world",
        }],
        "satisfactory" => &[Files {
            parent: &["server"],
            key: satisfactory_key,
            label: "session",
        }],
        _ => &[],
    }
}

/// Native layouts that only count when the slot carries its game's marker file.
fn slot_marker(module_id: &str) -> Option<&'static str> {
    match module_id {
        "sevendaystodie" => Some("main.ttw"),
        "romestead" => Some("world_desc"),
        "necesse" => Some("world.dat"),
        _ => None,
    }
}

fn current_slot(module_id: &str, settings: &Map<String, Value>) -> Option<Current> {
    let name = |key| native_setting(settings, key).map(|name| Current::Name(name.to_owned()));
    match module_id {
        "theforest" => configured_save_slot(settings, Some(5)).map(|slot| {
            Current::Name(
                theforest_slot_dir(slot)
                    .rsplit('/')
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
            )
        }),
        "sonsoftheforest" => configured_save_slot(settings, None).map(|slot| {
            Current::Name(
                sons_slot_dir(slot)
                    .rsplit('/')
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
            )
        }),
        "sevendaystodie" => Some(Current::Name(format!(
            "{}/{}",
            native_setting(settings, "game_world")?,
            native_setting(settings, "world_name")?
        ))),
        "unturned" => name("map"),
        "windrose" => name("world_island_id"),
        "vrising" => name("save_name"),
        "romestead" => name("auto_start_world_name"),
        "valheim" | "necesse" => name("world_name"),
        "astroneer" => Some(Current::Name(
            native_setting(settings, "active_save_file_name")
                .unwrap_or("SAVE_1")
                .to_owned(),
        )),
        "terraria" => native_setting(settings, "world_file")
            .and_then(terraria_key)
            .map(Current::Name),
        "corekeeper" => settings.get("world_index").map(|value| match value {
            Value::String(text) => Current::Name(text.clone()),
            other => Current::Name(other.to_string()),
        }),
        "returntomoria" => native_setting(settings, "world_file_name")
            .and_then(moria_key)
            .map(Current::Name),
        "rust" => rust_world_prefix(settings, native_setting(settings, "level")?)
            .ok()
            .filter(|prefix| !prefix.is_empty())
            .map(Current::Prefix),
        _ => None,
    }
}

fn valheim_world_key(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    [".fwl", ".db", ".fwl.old", ".db.old"]
        .iter()
        .find_map(|suffix| {
            lower
                .strip_suffix(suffix)
                .map(|stem| name[..stem.len()].to_owned())
        })
        .filter(|stem| !stem.is_empty())
}

fn necesse_zip_key(name: &str) -> Option<String> {
    name.strip_suffix(".zip")
        .filter(|stem| !stem.is_empty())
        .map(str::to_owned)
}

fn astroneer_key(name: &str) -> Option<String> {
    crate::astroneer_saves::parse_save_filename(name).map(|(slot, _)| slot.to_owned())
}

fn terraria_key(name: &str) -> Option<String> {
    let stem = name
        .strip_suffix(".wld.bak")
        .or_else(|| name.strip_suffix(".wld"))?;
    (!stem.is_empty()).then(|| stem.to_owned())
}

fn corekeeper_key(name: &str) -> Option<String> {
    let stem = name
        .strip_suffix(".world.gzip")
        .or_else(|| name.strip_suffix(".world.gzip.bak"))?;
    (!stem.is_empty() && stem.bytes().all(|byte| byte.is_ascii_digit())).then(|| stem.to_owned())
}

fn moria_key(name: &str) -> Option<String> {
    let stem = name.strip_suffix(".sav")?;
    stem.starts_with("MW_").then(|| stem.to_owned())
}

fn barotrauma_key(name: &str) -> Option<String> {
    name.strip_suffix("_CharacterData.xml")
        .or_else(|| name.strip_suffix(".save"))
        .filter(|stem| !stem.is_empty())
        .map(str::to_owned)
}

/// Rust world files are `<prefix>.sav`, `<prefix>.sav.<n>` and `<prefix>.map`.
fn rust_key(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let end = lower
        .find(".sav")
        .or_else(|| lower.strip_suffix(".map").map(str::len))?;
    (end > 0).then(|| name[..end].to_owned())
}

fn satisfactory_key(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let stem = lower.strip_suffix(".sav")?;
    (!stem.is_empty() && !lower.starts_with("serversettings"))
        .then(|| name[..stem.len()].to_owned())
}

#[derive(Default)]
struct Totals {
    size_bytes: u64,
    file_count: u64,
    modified_at_unix_ms: Option<u64>,
}

impl Totals {
    fn add(&mut self, entry: &SaveFileEntry) {
        if entry.is_directory {
            return;
        }
        self.size_bytes = self.size_bytes.saturating_add(entry.size_bytes);
        self.file_count += 1;
        self.modified_at_unix_ms = self.modified_at_unix_ms.max(entry.modified_at_unix_ms);
    }
}

struct Draft {
    name: String,
    label: Option<SaveSlotLabel>,
    kind: &'static str,
    paths: Vec<String>,
    totals: Totals,
}

fn matches_parent(parts: &[&str], parent: &[&str]) -> bool {
    parts.len() >= parent.len()
        && parent
            .iter()
            .zip(parts)
            .all(|(want, part)| *want == "*" || want == part)
}

type Claim = (String, String, String, &'static str);

/// The native slot (row key, covered path, name, label kind) claiming `entry`.
fn claim(
    layouts: &[Layout],
    directories: &std::collections::HashSet<&str>,
    has_marker: &dyn Fn(&str) -> bool,
    entry: &SaveFileEntry,
) -> Option<Claim> {
    let parts: Vec<&str> = entry.relative_path.split('/').collect();
    for layout in layouts {
        match layout {
            Layout::Directories {
                parent,
                depth,
                label,
            } => {
                let end = parent.len() + depth;
                if parts.len() < end || !matches_parent(&parts, parent) {
                    continue;
                }
                let root = parts[..end].join("/");
                if !directories.contains(root.as_str()) || !has_marker(&root) {
                    continue;
                }
                let name = parts[parent.len()..end].join("/");
                return Some((format!("d:{root}"), root, name, *label));
            }
            Layout::Files { parent, key, label } => {
                if entry.is_directory
                    || parts.len() != parent.len() + 1
                    || !matches_parent(&parts, parent)
                {
                    continue;
                }
                let Some(name) = key(parts[parent.len()]) else {
                    continue;
                };
                let scope = parent.join("/");
                return Some((
                    format!("f:{scope}/{name}"),
                    entry.relative_path.clone(),
                    name,
                    *label,
                ));
            }
        }
    }
    None
}

/// Groups `entries` (as produced by the bounded listing) into slots.
pub(crate) fn group_save_slots(
    module_id: &str,
    settings: &Map<String, Value>,
    entries: &[SaveFileEntry],
) -> (bool, Vec<SaveSlot>) {
    let layouts = layouts(module_id);
    let directories: std::collections::HashSet<&str> = entries
        .iter()
        .filter(|entry| entry.is_directory)
        .map(|entry| entry.relative_path.as_str())
        .collect();
    let files: std::collections::HashSet<&str> = entries
        .iter()
        .filter(|entry| !entry.is_directory)
        .map(|entry| entry.relative_path.as_str())
        .collect();
    let marker = slot_marker(module_id);
    let has_marker = |root: &str| {
        marker.is_none_or(|marker| files.contains(format!("{root}/{marker}").as_str()))
    };
    // First claim entries for native slots, so their container directories
    // (e.g. `Multiplayer`) are not shown as separate empty fallback rows.
    let claims: Vec<_> = entries
        .iter()
        .map(|entry| claim(layouts, &directories, &has_marker, entry))
        .collect();
    let native_roots: std::collections::BTreeSet<String> = claims
        .iter()
        .filter_map(|claim| claim.as_ref())
        .map(|claim| claim.1.clone())
        .collect();
    let contains_native = |directory: &str| {
        let prefix = format!("{directory}/");
        native_roots
            .range(prefix.clone()..)
            .next()
            .is_some_and(|root| root.starts_with(&prefix))
    };
    let mut drafts: BTreeMap<String, Draft> = BTreeMap::new();
    for (entry, claimed) in entries.iter().zip(claims) {
        let parts: Vec<&str> = entry.relative_path.split('/').collect();
        if claimed.is_none() && entry.is_directory && contains_native(&entry.relative_path) {
            continue;
        }
        let (key, path, name, label, kind) = match claimed {
            Some((key, path, name, label)) => (key, path, name, Some(label), "native"),
            None if parts.len() > 1 || entry.is_directory => {
                // Beside native slots, cover individual paths so this row never
                // claims a slot's files through its shared top-level directory.
                let path = if contains_native(parts[0]) {
                    entry.relative_path.clone()
                } else {
                    parts[0].to_owned()
                };
                (
                    format!("t:{}", parts[0]),
                    path,
                    parts[0].to_owned(),
                    None,
                    "directory",
                )
            }
            None => (
                String::from("t:"),
                entry.relative_path.clone(),
                String::new(),
                None,
                "files",
            ),
        };
        let draft = drafts.entry(key).or_insert_with(|| Draft {
            label: label.map(|kind| SaveSlotLabel {
                kind,
                value: slot_label_value(kind, &name),
            }),
            name,
            kind,
            paths: Vec::new(),
            totals: Totals::default(),
        });
        if !draft.paths.contains(&path)
            && !draft
                .paths
                .iter()
                .any(|root| path.starts_with(&format!("{root}/")))
        {
            draft.paths.push(path);
        }
        draft.totals.add(entry);
    }
    let current = current_slot(module_id, settings);
    let mut slots: Vec<SaveSlot> = drafts
        .into_iter()
        .map(|(id, draft)| SaveSlot {
            id,
            current: draft.kind == "native"
                && match &current {
                    Some(Current::Name(name)) => *name == draft.name,
                    Some(Current::Prefix(prefix)) => {
                        format!("{}.", draft.name.to_ascii_lowercase()).starts_with(prefix.as_str())
                    }
                    None => false,
                },
            name: draft.name,
            label: draft.label,
            kind: draft.kind,
            paths: draft.paths,
            size_bytes: draft.totals.size_bytes,
            file_count: draft.totals.file_count,
            modified_at_unix_ms: draft.totals.modified_at_unix_ms,
        })
        .collect();
    slots.sort_by(|left, right| {
        right
            .current
            .cmp(&left.current)
            .then_with(|| (left.kind != "native").cmp(&(right.kind != "native")))
            .then_with(|| right.modified_at_unix_ms.cmp(&left.modified_at_unix_ms))
            .then_with(|| left.name.cmp(&right.name))
    });
    (!layouts.is_empty(), slots)
}

/// The value shown beside a label: slot numbers without padding, otherwise the name.
fn slot_label_value(kind: &str, name: &str) -> String {
    if kind == "save_slot" {
        let digits = name.trim_start_matches("Slot").trim_start_matches('0');
        return if digits.is_empty() {
            String::from("0")
        } else {
            digits.to_owned()
        };
    }
    name.split('/').next().unwrap_or(name).to_owned()
}

#[cfg(test)]
#[path = "save_slots_tests.rs"]
mod tests;
