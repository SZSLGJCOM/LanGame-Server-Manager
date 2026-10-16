use super::*;
use std::collections::BTreeMap;
use std::path::Component;

#[derive(Clone, Serialize)]
pub(super) struct ServerMapping {
    pub source: PathBuf,
    pub target: PathBuf,
    pub replace: bool,
}

#[derive(Clone, Serialize)]
pub(super) struct ServerScope {
    pub target: PathBuf,
    pub mappings: Vec<ServerMapping>,
    pub clear: Vec<PathBuf>,
}

impl ServerScope {
    pub fn new(target: PathBuf, mappings: Vec<ServerMapping>) -> Self {
        Self {
            target,
            mappings,
            clear: Vec::new(),
        }
    }
}

#[derive(Clone, Serialize)]
pub(super) struct ServerPlan {
    pub scopes: Vec<ServerScope>,
    pub warnings: Vec<String>,
    pub guards: Vec<PathBuf>,
}

impl ServerPlan {
    pub fn new(scopes: Vec<ServerScope>, warnings: Vec<String>) -> Self {
        Self {
            scopes,
            warnings,
            guards: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub(super) struct Projection {
    pub inventory: io::Inventory,
    pub origins: BTreeMap<PathBuf, PathBuf>,
    pub files: u64,
    pub bytes: u64,
}

#[derive(Clone)]
struct Node {
    path: PathBuf,
    value: Option<(u64, String)>,
    origin: PathBuf,
    imported: bool,
}

fn key(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

fn relative(path: &Path) -> bool {
    path.components()
        .all(|part| matches!(part, Component::Normal(_)))
        && !path.to_string_lossy().contains(':')
}

fn remove(nodes: &mut BTreeMap<String, Node>, prefix: &Path) {
    let name = key(prefix);
    if name.is_empty() {
        nodes.clear();
        return;
    }
    nodes.remove(&name);
    let directory = format!("{name}/");
    let children: Vec<_> = nodes
        .range(directory.clone()..)
        .take_while(|(path, _)| path.starts_with(&directory))
        .map(|(path, _)| path.clone())
        .collect();
    for child in children {
        nodes.remove(&child);
    }
}

fn index(
    tree: &io::Inventory,
    root: &Path,
    imported: bool,
) -> Result<BTreeMap<String, Node>, StorageError> {
    let mut nodes = BTreeMap::new();
    for (path, value) in &tree.entries {
        if !relative(path)
            || nodes
                .insert(
                    key(path),
                    Node {
                        path: path.clone(),
                        value: value.clone(),
                        origin: root.join(path),
                        imported,
                    },
                )
                .is_some()
        {
            return Err(invalid(root, "Save paths collide or escape their scope."));
        }
    }
    Ok(nodes)
}

pub(super) fn validate(context: &Context, plan: &ServerPlan) -> Result<(), StorageError> {
    if plan.scopes.is_empty() || plan.scopes.len() > 8 || plan.guards.len() > 8 {
        return Err(invalid(&context.target, "Invalid save import scope count."));
    }
    let primary = normalize_path(&context.target)?;
    let generated = normalize_path(&instance_root(&context.record).join("GeneratedWorlds"))?;
    let mut targets = Vec::new();
    for scope in &plan.scopes {
        let target = normalize_path(&scope.target)?;
        if target != primary
            && !(context.record.summary.module_id == "sevendaystodie" && target == generated)
        {
            return Err(invalid(
                &target,
                "The import target is outside this game's owned save scope.",
            ));
        }
        if targets
            .iter()
            .any(|other: &PathBuf| target.starts_with(other) || other.starts_with(&target))
        {
            return Err(invalid(&target, "Import destination scopes overlap."));
        }
        targets.push(target);
        if scope.mappings.is_empty()
            || scope.mappings.len() > 200_000
            || scope.clear.len() > 200_000
            || scope.clear.iter().any(|path| !relative(path))
            || scope
                .mappings
                .iter()
                .any(|mapping| !relative(&mapping.source) || !relative(&mapping.target))
        {
            return Err(invalid(&scope.target, "Invalid native save mapping."));
        }
    }
    for guard in &plan.guards {
        let guard = crate::instance_isolation::paths::normalize_resource_path(guard)?;
        let root = normalize_path(&instance_root(&context.record))?;
        let install = normalize_path(&context.install)?;
        if !guard.starts_with(root) && !guard.starts_with(install) {
            return Err(invalid(
                &guard,
                "A native save selector is outside this instance.",
            ));
        }
    }
    Ok(())
}

pub(super) fn guards(plan: &ServerPlan) -> Result<Vec<String>, StorageError> {
    plan.guards
        .iter()
        .map(|path| {
            let bytes = io::read_prefix(path, 1024 * 1024 + 1)?;
            if bytes.len() > 1024 * 1024 {
                return Err(invalid(path, "Native save selector exceeds 1 MiB."));
            }
            Ok(io::digest(bytes))
        })
        .collect()
}

pub(super) fn project(
    scope: &ServerScope,
    source: &Path,
    tree: &io::Inventory,
    current: Option<&io::Inventory>,
) -> Result<Projection, StorageError> {
    let source_nodes = index(tree, source, true)?;
    let mut nodes = current
        .map(|tree| index(tree, &scope.target, false))
        .transpose()?
        .unwrap_or_default();
    for path in &scope.clear {
        remove(&mut nodes, path);
    }
    for mapping in &scope.mappings {
        let source_key = key(&mapping.source);
        let root = source_nodes.get(&source_key);
        if !mapping.source.as_os_str().is_empty() && root.is_none() {
            return Err(invalid(
                &source.join(&mapping.source),
                "Required native save payload is missing.",
            ));
        }
        if root.is_some_and(|node| node.value.is_some()) && mapping.target.as_os_str().is_empty() {
            return Err(invalid(
                source,
                "A save file cannot replace a directory root.",
            ));
        }
        if mapping.replace {
            remove(&mut nodes, &mapping.target);
        }
        let prefix = format!("{source_key}/");
        let selected: Vec<_> = if source_key.is_empty() {
            source_nodes.values().collect()
        } else {
            root.into_iter()
                .chain(
                    source_nodes
                        .range(prefix.clone()..)
                        .take_while(|(path, _)| path.starts_with(&prefix))
                        .map(|(_, node)| node),
                )
                .collect()
        };
        if root.is_none() && !mapping.target.as_os_str().is_empty() {
            nodes.insert(
                key(&mapping.target),
                Node {
                    path: mapping.target.clone(),
                    value: None,
                    origin: source.to_owned(),
                    imported: true,
                },
            );
        }
        for node in selected {
            let suffix: PathBuf = node
                .path
                .components()
                .skip(mapping.source.components().count())
                .collect();
            let path = if suffix.as_os_str().is_empty() {
                mapping.target.clone()
            } else {
                mapping.target.join(suffix)
            };
            if path.as_os_str().is_empty() {
                continue;
            }
            if path.components().count() > 64 {
                return Err(invalid(
                    &path,
                    "Mapped saves exceed the 64-level directory limit.",
                ));
            }
            if node.value.is_some() {
                let extension = path
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if matches!(
                    extension.as_str(),
                    "exe" | "dll" | "bat" | "cmd" | "ps1" | "sh"
                ) {
                    return Err(invalid(&node.origin, "Executable files are not save data."));
                }
            }
            let name = key(&path);
            if nodes
                .get(&name)
                .is_some_and(|old| old.imported && old.value != node.value)
            {
                return Err(invalid(&path, "Native save mappings overlap."));
            }
            // A source file owns its entire leaf, including an old directory at that name.
            if node.value.is_some() {
                remove(&mut nodes, &path);
            }
            nodes.insert(
                name,
                Node {
                    path,
                    value: node.value.clone(),
                    origin: node.origin.clone(),
                    imported: true,
                },
            );
        }
    }
    let mut parents = BTreeMap::new();
    for node in nodes.values() {
        for parent in node
            .path
            .ancestors()
            .skip(1)
            .filter(|path| !path.as_os_str().is_empty())
        {
            let name = key(parent);
            if nodes.get(&name).is_some_and(|node| node.value.is_some()) {
                return Err(invalid(
                    parent,
                    "A native save directory conflicts with a file.",
                ));
            }
            if !nodes.contains_key(&name) {
                parents.entry(name).or_insert_with(|| parent.to_owned());
            }
            if nodes.len() + parents.len() > 200_000 {
                return Err(invalid(source, "Imported saves exceed 200000 entries."));
            }
        }
    }
    for (name, parent) in parents {
        nodes.entry(name).or_insert(Node {
            path: parent,
            value: None,
            origin: source.to_owned(),
            imported: false,
        });
    }
    if nodes.len() > 200_000 {
        return Err(invalid(source, "Imported saves exceed 200000 entries."));
    }
    let mut files = 0;
    let mut bytes = 0_u64;
    let mut origins = BTreeMap::new();
    let mut entries = BTreeMap::new();
    for node in nodes.values() {
        // One native spelling per parent is required: Windows accepts both
        // separators and case variants, but the readback manifest preserves
        // the directory names created in staging.
        let mut prefix = PathBuf::new();
        let mut path = PathBuf::new();
        for component in node.path.components() {
            prefix.push(component);
            let name = nodes
                .get(&key(&prefix))
                .and_then(|node| node.path.file_name())
                .unwrap_or(component.as_os_str());
            path.push(name);
        }
        if let Some((length, _)) = &node.value {
            if node.imported {
                files += 1;
                bytes += length;
            }
            origins.insert(path.clone(), node.origin.clone());
        }
        entries.insert(path, node.value.clone());
    }
    let total_bytes = entries
        .values()
        .flatten()
        .try_fold(0_u64, |sum, (bytes, _)| sum.checked_add(*bytes))
        .filter(|sum| *sum <= 1024 * 1024 * 1024 * 1024)
        .ok_or_else(|| {
            invalid(
                source,
                "Imported saves exceed the 1 TiB verification limit.",
            )
        })?;
    let inventory = io::Inventory {
        digest: io::digest(serde_json::to_vec(&entries)?),
        files: origins.len() as u64,
        bytes: total_bytes,
        entries,
    };
    Ok(Projection {
        inventory,
        origins,
        files,
        bytes,
    })
}

pub(super) fn write(projection: &Projection, destination: &Path) -> Result<(), StorageError> {
    use crate::instance_archive_files::native;
    let _root = native::open(destination, true, false)
        .map_err(|error| invalid(destination, error.to_string()))?;
    let mut directories: Vec<_> = projection
        .inventory
        .entries
        .iter()
        .filter(|(_, entry)| entry.is_none())
        .map(|(path, _)| path)
        .collect();
    directories.sort_by_key(|path| path.components().count());
    for relative in directories {
        let target = destination.join(relative);
        let parent = target
            .parent()
            .ok_or_else(|| invalid(&target, "Save directory has no parent."))?;
        let _guard = native::open(parent, true, false)
            .map_err(|error| invalid(parent, error.to_string()))?;
        fs::create_dir(&target).map_err(|error| invalid(&target, error.to_string()))?;
    }
    for (relative, origin) in &projection.origins {
        let length = projection
            .inventory
            .entries
            .get(relative)
            .and_then(Option::as_ref)
            .map(|(length, _)| *length)
            .ok_or_else(|| invalid(relative, "Missing save content binding."))?;
        io::copy_file(origin, &destination.join(relative), length)?;
    }
    if io::inventory(destination, None)? != projection.inventory {
        return Err(invalid(
            destination,
            "Staged native saves changed while copying.",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "save_import_server_plan_tests.rs"]
mod tests;
