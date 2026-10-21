use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};

use super::{DstMapIconSources, ModAssets, ReadBudget, atlas, encode_png, resources};

#[path = "dst_map_inventory_icon.rs"]
mod inventory;

// Small single-image atlases can be numerous. Bound actual metadata I/O while
// still inspecting every authorized XML before claiming a unique element.
const MAX_INDEX_XML_BYTES: usize = 8 * 1024 * 1024;
const MAX_INDEX_ENTRIES: usize = 8192;
const MAX_INDEX_DEPTH: usize = 16;
const MAX_XML_BYTES: usize = 1024 * 1024;
const MAX_MOD_ATLAS_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DstMapIconFallbackKind {
    RegisteredAlias,
    IndexedExact,
    IndexedAlias,
    EntityInventory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DstMapIconFallback {
    pub icon: String,
    pub element: String,
    pub atlas: String,
    pub kind: DstMapIconFallbackKind,
}

#[derive(Default)]
pub(super) struct CompletedIcons {
    pub icons: BTreeMap<String, Vec<u8>>,
    pub provenance: Vec<DstMapIconFallback>,
    pub issues: Vec<String>,
}

#[derive(Default)]
struct CompletionPlan {
    by_atlas: BTreeMap<String, Vec<Selected>>,
    issues: Vec<String>,
}

struct Selected {
    icon: String,
    candidate: Candidate,
    kind: DstMapIconFallbackKind,
}

#[derive(Clone)]
struct Candidate {
    atlas: String,
    element: String,
    rectangle: atlas::Rectangle,
    texture_filename: String,
    straight_alpha: bool,
}

enum Choice {
    Unique(Candidate),
    Ambiguous,
}

#[derive(Default)]
struct Catalog(BTreeMap<String, Choice>);

impl Catalog {
    fn add(&mut self, path: &str, parsed: &atlas::Atlas, wanted: &BTreeSet<String>) {
        for key in wanted {
            if let Some(rectangle) = parsed.elements.get(key) {
                if let Some(previous) = self.0.get(key) {
                    if matches!(previous, Choice::Unique(candidate) if candidate.atlas == path) {
                        continue;
                    }
                    self.0.insert(key.clone(), Choice::Ambiguous);
                } else {
                    self.0.insert(
                        key.clone(),
                        Choice::Unique(Candidate {
                            atlas: path.to_string(),
                            element: parsed.element_names[key].clone(),
                            rectangle: *rectangle,
                            texture_filename: parsed.texture_filename.clone(),
                            straight_alpha: parsed.straight_alpha,
                        }),
                    );
                }
            }
        }
    }
}

pub(super) fn load(
    root: &Path,
    mods: &ModAssets,
    sources: DstMapIconSources<'_>,
    pending: &BTreeSet<&str>,
    budget: &mut ReadBudget,
) -> CompletedIcons {
    let plan = plan(root, mods, sources, pending, budget);
    let mut output = CompletedIcons {
        issues: plan.issues,
        ..CompletedIcons::default()
    };
    for selected in plan.by_atlas.into_values() {
        let Some(first) = selected.first() else {
            continue;
        };
        let pixels = read_texture(&first.candidate, root, mods, budget);
        for item in selected {
            let icon = match &pixels {
                Ok(pixels) => budget
                    .crop_icon(pixels, item.candidate.rectangle)
                    .and_then(encode_png),
                Err(error) => Err(error.clone()),
            };
            match icon {
                Ok(png) => {
                    output.icons.insert(item.icon.clone(), png);
                    output.provenance.push(DstMapIconFallback {
                        icon: item.icon,
                        element: item.candidate.element,
                        atlas: item.candidate.atlas,
                        kind: item.kind,
                    });
                }
                Err(error) => output.issues.push(format!(
                    "DST map icon completion {} from {}: {error}",
                    item.icon, item.candidate.atlas
                )),
            }
        }
    }
    output
}

fn plan(
    root: &Path,
    mods: &ModAssets,
    sources: DstMapIconSources<'_>,
    pending: &BTreeSet<&str>,
    budget: &mut ReadBudget,
) -> CompletionPlan {
    let registered_atlases = sources.registered_atlases;
    let mut output = CompletionPlan::default();
    if pending.is_empty() {
        return output;
    }
    let aliases: BTreeSet<_> = pending.iter().filter_map(|name| alternate(name)).collect();
    let registered = match registered_catalog(root, mods, registered_atlases, &aliases, budget) {
        Ok(catalog) => catalog,
        Err(error) => {
            output.issues.push(format!(
                "Registered map icon completion sources are uncertain: {error}"
            ));
            return output;
        }
    };
    let mut indexed_pending = Vec::new();
    for name in pending {
        let Some(alias) = alternate(name) else {
            indexed_pending.push(*name);
            continue;
        };
        if !select(
            name,
            registered.0.get(&alias),
            DstMapIconFallbackKind::RegisteredAlias,
            &mut output,
        ) {
            indexed_pending.push(*name);
        }
    }
    if indexed_pending.is_empty() {
        return output;
    }
    let wanted = indexed_pending
        .iter()
        .flat_map(|name| {
            [Some(atlas::map_element_key(name)), alternate(name)]
                .into_iter()
                .flatten()
        })
        .collect();
    let indexed = match indexed_catalog(mods, registered_atlases, &wanted, budget) {
        Ok(catalog) => catalog,
        Err(error) => {
            output.issues.push(format!(
                "Mod map icon completion index is incomplete: {error}"
            ));
            return output;
        }
    };
    for name in indexed_pending {
        if select(
            name,
            indexed.0.get(&atlas::map_element_key(name)),
            DstMapIconFallbackKind::IndexedExact,
            &mut output,
        ) {
            continue;
        }
        if let Some(alias) = alternate(name) {
            if select(
                name,
                indexed.0.get(&alias),
                DstMapIconFallbackKind::IndexedAlias,
                &mut output,
            ) {
                continue;
            }
        }
        match inventory::candidate(root, mods, sources, name, budget) {
            Ok(Some(candidate)) => {
                select(
                    name,
                    Some(&Choice::Unique(candidate)),
                    DstMapIconFallbackKind::EntityInventory,
                    &mut output,
                );
            }
            Ok(None) => {}
            Err(error) => output
                .issues
                .push(format!("DST entity inventory icon {name}: {error}")),
        }
    }
    output
}

fn alternate(name: &str) -> Option<String> {
    let key = atlas::map_element_key(name);
    key.strip_suffix(".png")
        .filter(|stem| !stem.is_empty())
        .map(|stem| format!("{stem}.tex"))
        .or_else(|| {
            key.strip_suffix(".tex")
                .filter(|stem| !stem.is_empty())
                .map(|stem| format!("{stem}.png"))
        })
}

// A known candidate, including an unusable or ambiguous one, terminates this
// tier. A lower-priority resource must not silently replace its meaning.
fn select(
    icon: &str,
    choice: Option<&Choice>,
    kind: DstMapIconFallbackKind,
    output: &mut CompletionPlan,
) -> bool {
    let Some(choice) = choice else {
        return false;
    };
    match choice {
        Choice::Ambiguous => output
            .issues
            .push(format!("DST map icon completion is ambiguous: {icon}")),
        Choice::Unique(candidate) => output
            .by_atlas
            .entry(candidate.atlas.clone())
            .or_default()
            .push(Selected {
                icon: icon.to_string(),
                candidate: candidate.clone(),
                kind,
            }),
    }
    true
}

fn read_texture(
    candidate: &Candidate,
    root: &Path,
    mods: &ModAssets,
    budget: &mut ReadBudget,
) -> Result<image::RgbaImage, String> {
    let (asset_root, relative) = source(root, mods, &candidate.atlas)?;
    let texture_path = relative.rsplit_once('/').map_or_else(
        || candidate.texture_filename.clone(),
        |(parent, _)| format!("{parent}/{}", candidate.texture_filename),
    );
    if !super::mod_assets::safe_component(&candidate.texture_filename)
        || !candidate.texture_filename.ends_with(".tex")
    {
        return Err("completion atlas references an unsafe texture filename".into());
    }
    let limit = if candidate.atlas.starts_with("../mods/") {
        MAX_MOD_ATLAS_BYTES
    } else {
        super::MAX_ATLAS_BYTES
    };
    let bytes = budget
        .read(asset_root, &texture_path, limit)?
        .ok_or_else(|| String::from("completion atlas texture is not installed"))?;
    budget.decode_with_alpha(&bytes, candidate.straight_alpha)
}

fn source<'a>(
    root: &'a Path,
    mods: &'a ModAssets,
    path: &'a str,
) -> Result<(&'a Path, String), String> {
    if path.starts_with("../mods/") {
        let (root, relative) = mods.resolve(path, ".xml")?;
        return Ok((root, relative.to_string()));
    }
    if super::native_atlas_suffix(path).is_none()
        && !path
            .strip_prefix("images/")
            .is_some_and(|name| super::mod_assets::safe_component(name) && name.ends_with(".xml"))
    {
        return Err("unsupported native completion atlas".into());
    }
    Ok((root, format!("data/{path}")))
}

fn registered_catalog(
    root: &Path,
    mods: &ModAssets,
    registered: &[String],
    wanted: &BTreeSet<String>,
    budget: &mut ReadBudget,
) -> Result<Catalog, String> {
    let mut catalog = Catalog::default();
    let paths = super::icon_atlas_paths(registered).collect::<BTreeSet<_>>();
    for path in paths {
        let (root, relative) = source(root, mods, path)?;
        let Some(xml) = budget.read(root, &relative, MAX_XML_BYTES)? else {
            if registered.iter().any(|registered| registered == path) {
                return Err(format!("{path}: registered minimap atlas disappeared"));
            }
            continue;
        };
        let (parsed, _) = atlas::parse_selected_map_elements(&xml, wanted)
            .map_err(|error| format!("{path}: {error}"))?;
        if !path.starts_with("../mods/") {
            let suffix = path
                .strip_prefix("minimap/minimap_data")
                .and_then(|value| value.strip_suffix(".xml"))
                .unwrap_or_default();
            if parsed.texture_filename != format!("minimap_atlas{suffix}.tex") {
                return Err(format!("{path}: unexpected native texture filename"));
            }
        }
        catalog.add(path, &parsed, wanted);
    }
    Ok(catalog)
}

fn indexed_catalog(
    mods: &ModAssets,
    registered: &[String],
    wanted: &BTreeSet<String>,
    budget: &mut ReadBudget,
) -> Result<Catalog, String> {
    let mut catalog = Catalog::default();
    let registered: BTreeSet<_> = registered.iter().map(String::as_str).collect();
    let mut entries = 0usize;
    let mut xml_bytes = 0usize;
    for (name, root) in &mods.roots {
        let root = root.as_ref().map_err(|error| format!("{name}: {error}"))?;
        let mut pending = VecDeque::from([(String::new(), 0usize)]);
        while let Some((relative, depth)) = pending.pop_front() {
            let directory = root.join(&relative);
            let metadata = fs::symlink_metadata(&directory)
                .map_err(|error| format!("{name}/{relative}: {error}"))?;
            let resolved = fs::canonicalize(&directory)
                .map_err(|error| format!("{name}/{relative}: {error}"))?;
            if !metadata.is_dir() || resources::is_reparse(&metadata) || !resolved.starts_with(root)
            {
                return Err(format!("{name}/{relative}: unsafe atlas index directory"));
            }
            for entry in
                fs::read_dir(&directory).map_err(|error| format!("{name}/{relative}: {error}"))?
            {
                entries += 1;
                if entries > MAX_INDEX_ENTRIES {
                    return Err("atlas index exceeds its directory entry limit".into());
                }
                let entry = entry.map_err(|error| format!("atlas index entry: {error}"))?;
                let filename = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| "atlas index filename is not UTF-8")?;
                let relative = if relative.is_empty() {
                    filename.clone()
                } else {
                    format!("{relative}/{filename}")
                };
                let metadata = fs::symlink_metadata(entry.path())
                    .map_err(|error| format!("{name}/{relative}: {error}"))?;
                if resources::is_reparse(&metadata) {
                    return Err(format!(
                        "{name}/{relative}: atlas index contains a reparse point"
                    ));
                }
                if metadata.is_dir() {
                    if depth >= MAX_INDEX_DEPTH {
                        return Err("atlas index exceeds its directory depth limit".into());
                    }
                    if !super::mod_assets::safe_component(&filename) {
                        return Err("atlas index directory name is unsafe".into());
                    }
                    pending.push_back((relative, depth + 1));
                } else if metadata.is_file() && filename.ends_with(".xml") {
                    let path = format!("../mods/{name}/{relative}");
                    if path.len() > 512 || !super::mod_assets::safe_component(&filename) {
                        return Err("atlas index resource path is unsafe".into());
                    }
                    if registered.contains(path.as_str()) {
                        continue;
                    }
                    let remaining = MAX_INDEX_XML_BYTES.saturating_sub(xml_bytes);
                    if metadata.len() > remaining as u64 {
                        return Err("atlas index exceeds its cumulative XML byte limit".into());
                    }
                    let xml = budget
                        .read(root, &relative, MAX_XML_BYTES.min(remaining))?
                        .ok_or_else(|| format!("{path}: indexed atlas disappeared"))?;
                    // The reader also bounds a concurrently growing file by
                    // the remaining allowance, not only its earlier metadata.
                    xml_bytes += xml.len();
                    if !is_atlas(&xml)? {
                        continue;
                    }
                    let Some(parsed) = atlas::parse_atlas_for_map_index(&xml, wanted)
                        .map_err(|error| format!("{path}: {error}"))?
                    else {
                        continue;
                    };
                    catalog.add(&path, &parsed, wanted);
                }
            }
        }
    }
    Ok(catalog)
}

fn is_atlas(bytes: &[u8]) -> Result<bool, String> {
    let xml = std::str::from_utf8(bytes).map_err(|_| "indexed XML is not UTF-8")?;
    let mut reader = Reader::from_str(xml);
    loop {
        match reader
            .read_event()
            .map_err(|error| format!("invalid indexed XML: {error}"))?
        {
            Event::Start(element) | Event::Empty(element) => {
                return Ok(element.name().as_ref() == "Atlas");
            }
            Event::Decl(_) | Event::Comment(_) => {}
            Event::Text(text) if text.as_ref().bytes().all(|byte| byte.is_ascii_whitespace()) => {}
            _ => return Err("indexed XML has no supported root".into()),
        }
    }
}

#[cfg(test)]
#[path = "dst_map_icon_fallback_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "dst_map_inventory_icon_tests.rs"]
mod inventory_tests;

#[cfg(test)]
#[path = "dst_map_icon_index_tests.rs"]
mod index_tests;
